//! PROTOTYPE: laying out sibling subtrees in parallel.
//!
//! Taffy's layout algorithms hand the document batches of child layouts (distinct children of one
//! node, whose inputs do not depend on each other). If the subtrees that a batch needs to lay out
//! are large enough then the document computes the batch's jobs concurrently on the current rayon
//! thread pool.
//!
//! # Safety
//!
//! This is a prototype and it is NOT sound. Each task accesses the document through a copy of one
//! raw pointer (wrapped in its own `LayoutPassState`), and so tasks hold aliasing
//! `&mut BaseDocument`s. It relies on the following holding in practice:
//!
//! - The layout tree is a tree, so that the tasks of a batch lay out disjoint sets of nodes
//! - Laying out a subtree only writes to the nodes in that subtree, with the exception of a
//!   node's cache and unrounded layout which are written by the task that lays out its parent
//!   (or its containing block if it is out-of-flow). The caches of a batch's nodes are read
//!   before the batch's tasks are spawned and written after they have all finished.
//! - No node is added to or removed from the document during layout
//! - Everything else that layout touches (styles, the viewport, images) is only read

use super::LayoutPassState;
use crate::{BaseDocument, NodeId as DomNodeId, dom_node_id};
use rayon::prelude::*;
use std::sync::OnceLock;
use taffy::{BlockFormattingContext, CacheTree as _, ChildLayoutJob, LayoutOutput};

/// The default minimum total weight of the subtrees that a batch of jobs would lay out for the
/// jobs to be computed in parallel. With the default weights this is very roughly 64µs of work.
pub(crate) const DEFAULT_MIN_BATCH_WEIGHT: u32 = 256;

/// How the cost of laying out a node (excluding its descendants) is estimated. The weight of a
/// subtree is the sum of the weights of its nodes.
#[doc(hidden)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ParallelLayoutWeights {
    /// The weight of a node that has no children and is not an inline root (a replaced element,
    /// a form control or an empty box)
    pub leaf: u32,
    /// The weight of a node that has children and is not an inline root
    pub container: u32,
    /// The weight of an inline root, in addition to the weight of its text
    pub inline_root: u32,
    /// The number of bytes of an inline root's text that have a weight of 1.
    /// If this is 0 then the weight of an inline root does not depend on its text.
    pub text_bytes_per_unit: u32,
}

impl ParallelLayoutWeights {
    /// Every node has the same weight, except that inline roots weigh 9 times as much
    pub const NODE_COUNT: Self = Self {
        leaf: 1,
        container: 1,
        inline_root: 9,
        text_bytes_per_unit: 0,
    };

    /// Weights in units of very roughly 0.25µs, from timing the layout of ten web pages on one
    /// machine: a leaf took about 0.25µs, a container about 3.5µs (excluding its descendants)
    /// and an inline root about 1.4µs plus 38ns per byte of text.
    pub const MEASURED: Self = Self {
        leaf: 1,
        container: 12,
        inline_root: 6,
        text_bytes_per_unit: 6,
    };
}

impl Default for ParallelLayoutWeights {
    fn default() -> Self {
        Self::MEASURED
    }
}

/// The minimum batch weight that documents are created with: the value of the
/// `BLITZ_PARALLEL_LAYOUT_THRESHOLD` environment variable if it is set (for experiments), or else
/// [`DEFAULT_MIN_BATCH_WEIGHT`]
pub(crate) fn initial_min_batch_weight() -> Option<u32> {
    static THRESHOLD: OnceLock<u32> = OnceLock::new();
    Some(*THRESHOLD.get_or_init(|| {
        std::env::var("BLITZ_PARALLEL_LAYOUT_THRESHOLD")
            .ok()
            .and_then(|threshold| threshold.parse().ok())
            .unwrap_or(DEFAULT_MIN_BATCH_WEIGHT)
    }))
}

/// A thread pool to compute batches on instead of the current thread pool. It is only created if the
/// `BLITZ_PARALLEL_LAYOUT_THREADS` environment variable is set (for experiments).
///
/// A thread that is waiting for a batch to be computed on the current thread pool can be given any
/// other task from that pool to run in the meantime. So a program that lays out documents from
/// tasks on a rayon thread pool that are not reentrant (such as the WPT runner) needs layout to
/// use a different thread pool, and to block while it waits for that pool.
fn dedicated_thread_pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        let thread_count = std::env::var("BLITZ_PARALLEL_LAYOUT_THREADS").ok()?;
        rayon::ThreadPoolBuilder::new()
            .num_threads(thread_count.parse().ok()?)
            .thread_name(|index| format!("blitz-layout-{index}"))
            .build()
            .ok()
    })
    .as_ref()
}

/// Per-node data computed before each layout pass that the parallel layout code uses to decide
/// whether a batch of child layouts is worth computing in parallel, and whether the children of
/// a block can be laid out independently of each other.
#[derive(Default)]
pub(crate) struct LayoutSubtreeInfo {
    /// For each node (indexed by node id) an estimate of the cost of laying out its subtree
    weights: Vec<u32>,
    /// For each node (indexed by node id) whether its subtree contains a floated box
    has_floats: Vec<bool>,
    /// How the cost of laying out a node is estimated
    node_weights: ParallelLayoutWeights,
}

/// The index of the slot that a node is stored in
#[inline(always)]
fn slot(node_id: DomNodeId) -> usize {
    (node_id.as_u64() as u32) as usize
}

/// Whether the jobs of a batch are divided between tasks so that each task has a similar weight
/// (rather than a similar number of jobs)
#[doc(hidden)]
pub static SPLIT_BY_WEIGHT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// The point at which to split a list of at least two weights into two contiguous groups whose
/// total weights differ the least. Returns the length and the total weight of the first group.
fn weight_split_point(weights: &[u32], total_weight: u64) -> (usize, u64) {
    let mut split = 1;
    let mut left_weight = weights[0] as u64;
    let mut prefix = left_weight;
    for (index, weight) in weights.iter().enumerate().skip(1).take(weights.len() - 2) {
        prefix += *weight as u64;
        if (2 * prefix).abs_diff(total_weight) < (2 * left_weight).abs_diff(total_weight) {
            split = index + 1;
            left_weight = prefix;
        } else if 2 * prefix > total_weight {
            break;
        }
    }
    (split, left_weight)
}

/// Run a function that spawns rayon tasks: on the dedicated thread pool if there is one,
/// and otherwise on the current thread (and so on the current thread pool)
fn run_on_layout_pool(f: impl FnOnce() + Send) {
    match dedicated_thread_pool() {
        // A rayon thread that waits for another thread pool still runs tasks from its own
        // thread pool in the meantime, so the wait is done by a thread that is not in a pool
        Some(pool) if pool.current_thread_index().is_none() => std::thread::scope(|s| {
            let result = s.spawn(|| pool.install(f)).join();
            if let Err(panic) = result {
                std::panic::resume_unwind(panic);
            }
        }),
        _ => f(),
    }
}

/// The minimum total weight of the subtrees below a node for their layouts to be rounded in
/// parallel. Rounding a node is much cheaper than laying it out, so this is much higher than the
/// minimum weight of a batch of layouts.
#[doc(hidden)]
pub static MIN_ROUND_BATCH_WEIGHT: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(1024);

/// Call `round_subtree` for each node in parallel, recursively dividing the nodes into two
/// contiguous groups with weights that are as close to equal as possible
#[allow(clippy::too_many_arguments)]
fn round_subtrees_split_by_weight<'doc, F>(
    doc_ptr: DocPtr,
    fields: PassFields,
    nodes: &[taffy::NodeId],
    weights: &[u32],
    total_weight: u64,
    min_task_weight: u64,
    cumulative_x: f32,
    cumulative_y: f32,
    round_subtree: F,
) where
    F: Fn(&mut LayoutPassState<'doc>, taffy::NodeId, f32, f32) + Copy + Send + Sync,
{
    if nodes.len() <= 1 || total_weight < min_task_weight * 2 {
        for node in nodes {
            // SAFETY: see the module docs. This is not sound in general.
            let mut state = unsafe { LayoutPassState::for_task(doc_ptr, fields) };
            round_subtree(&mut state, *node, cumulative_x, cumulative_y);
        }
        return;
    }

    let (split, left_weight) = weight_split_point(weights, total_weight);
    let (left_nodes, right_nodes) = nodes.split_at(split);
    let (left_weights, right_weights) = weights.split_at(split);
    let round = |nodes, weights, weight| {
        round_subtrees_split_by_weight(
            doc_ptr,
            fields,
            nodes,
            weights,
            weight,
            min_task_weight,
            cumulative_x,
            cumulative_y,
            round_subtree,
        )
    };
    rayon::join(
        || round(left_nodes, left_weights, left_weight),
        || round(right_nodes, right_weights, total_weight - left_weight),
    );
}

/// Compute jobs in parallel by recursively dividing them into two contiguous groups with weights
/// that are as close to equal as possible, until a group is a single job or its weight is less
/// than twice `min_task_weight`.
fn compute_jobs_split_by_weight(
    doc_ptr: DocPtr,
    fields: PassFields,
    jobs: &mut [&mut ChildLayoutJob],
    weights: &[u32],
    total_weight: u64,
    min_task_weight: u64,
) {
    if jobs.len() <= 1 || total_weight < min_task_weight * 2 {
        for job in jobs.iter_mut() {
            // SAFETY: see the module docs. This is not sound in general.
            let mut state = unsafe { LayoutPassState::for_task(doc_ptr, fields) };
            job.output = state.compute_uncached_job(job);
        }
        return;
    }

    let (split, left_weight) = weight_split_point(weights, total_weight);
    let (left_jobs, right_jobs) = jobs.split_at_mut(split);
    let (left_weights, right_weights) = weights.split_at(split);
    let right_weight = total_weight - left_weight;
    rayon::join(
        || {
            compute_jobs_split_by_weight(
                doc_ptr,
                fields,
                left_jobs,
                left_weights,
                left_weight,
                min_task_weight,
            )
        },
        || {
            compute_jobs_split_by_weight(
                doc_ptr,
                fields,
                right_jobs,
                right_weights,
                right_weight,
                min_task_weight,
            )
        },
    );
}

#[derive(Copy, Clone)]
struct DocPtr(*mut BaseDocument);
// SAFETY: see the module docs. This is not sound in general.
unsafe impl Send for DocPtr {}
// SAFETY: see the module docs. This is not sound in general.
unsafe impl Sync for DocPtr {}
impl DocPtr {
    /// Get the pointer (a method so that closures capture the `DocPtr` rather than the pointer)
    fn get(&self) -> *mut BaseDocument {
        self.0
    }
}

/// The fields of a [`LayoutPassState`] other than the document: the state of the layout algorithm
/// that is running, which is copied into the state that each task of a batch lays out with
#[derive(Copy, Clone)]
pub(crate) struct PassFields {
    #[cfg(feature = "writing-mode")]
    layout_wm: stylo_taffy::WritingMode,
    #[cfg(feature = "writing-mode")]
    root_id: Option<DomNodeId>,
    #[cfg(feature = "writing-mode")]
    root_wm: stylo_taffy::WritingMode,
    #[cfg(feature = "writing-mode")]
    current_align_axis_is_inline: bool,
    #[cfg(feature = "writing-mode")]
    orthogonal_percent_basis: Option<(DomNodeId, f32)>,
}

impl<'doc> LayoutPassState<'doc> {
    /// The state of the running layout algorithm, to lay out a batch's jobs with
    fn pass_fields(&self) -> PassFields {
        PassFields {
            #[cfg(feature = "writing-mode")]
            layout_wm: self.layout_wm,
            #[cfg(feature = "writing-mode")]
            root_id: self.root_id,
            #[cfg(feature = "writing-mode")]
            root_wm: self.root_wm,
            #[cfg(feature = "writing-mode")]
            current_align_axis_is_inline: self.current_align_axis_is_inline,
            #[cfg(feature = "writing-mode")]
            orthogonal_percent_basis: self.orthogonal_percent_basis,
        }
    }

    /// A state for a task of a batch: the shared document and a copy of the state of the layout
    /// algorithm that started the batch
    ///
    /// # Safety
    /// See the module docs. This is not sound in general.
    unsafe fn for_task(doc_ptr: DocPtr, fields: PassFields) -> Self {
        let _ = &fields;
        Self {
            doc: unsafe { &mut *doc_ptr.get() },
            #[cfg(feature = "writing-mode")]
            layout_wm: fields.layout_wm,
            #[cfg(feature = "writing-mode")]
            root_id: fields.root_id,
            #[cfg(feature = "writing-mode")]
            root_wm: fields.root_wm,
            #[cfg(feature = "writing-mode")]
            current_align_axis_is_inline: fields.current_align_axis_is_inline,
            #[cfg(feature = "writing-mode")]
            orthogonal_percent_basis: fields.orthogonal_percent_basis,
        }
    }
}

impl BaseDocument {
    /// Set the minimum total weight (approximately: number of nodes) of the subtrees that a batch of
    /// child layouts would lay out for the batch to be computed in parallel. `None` disables parallel
    /// layout: every batch is then computed on the thread that requested it.
    #[doc(hidden)]
    pub fn set_parallel_layout_threshold(&mut self, min_batch_weight: Option<u32>) {
        self.parallel_layout_min_batch_weight = min_batch_weight;
    }

    /// Set how the cost of laying out a node is estimated
    #[doc(hidden)]
    pub fn set_parallel_layout_weights(&mut self, weights: ParallelLayoutWeights) {
        self.layout_subtree_info.node_weights = weights;
    }

    /// The estimated cost of laying out the subtree of a node, as of the last layout pass
    #[doc(hidden)]
    pub fn parallel_layout_subtree_weight(&self, node_id: DomNodeId) -> Option<u32> {
        self.layout_subtree_info.weights.get(slot(node_id)).copied()
    }

    /// Compute the weight of each subtree of the layout tree and whether it contains floats
    pub(crate) fn compute_layout_subtree_info(&mut self, root: DomNodeId) {
        let mut info = std::mem::take(&mut self.layout_subtree_info);
        info.weights.clear();
        info.has_floats.clear();

        // List the nodes so that each node comes after its parent, then add
        // each node's contribution to its parent's in reverse order.
        let mut order: Vec<(DomNodeId, Option<DomNodeId>)> = Vec::with_capacity(self.nodes.len());
        let mut stack: Vec<(DomNodeId, Option<DomNodeId>)> = vec![(root, None)];
        while let Some((node_id, parent_id)) = stack.pop() {
            order.push((node_id, parent_id));
            let node = &self.nodes[node_id];
            if let Some(children) = node.layout_children.borrow().as_ref() {
                stack.extend(children.iter().map(|child_id| (*child_id, Some(node_id))));
            }

            let slot = slot(node_id);
            if slot >= info.weights.len() {
                info.weights.resize(slot + 1, 0);
                info.has_floats.resize(slot + 1, false);
            }
            let node_weights = info.node_weights;
            info.weights[slot] = if node.flags.is_inline_root() {
                let text_len = node
                    .element_data()
                    .and_then(|element| element.inline_layout_data.as_ref())
                    .map(|inline_layout| inline_layout.text.len() as u32)
                    .unwrap_or(0);
                let text_weight = text_len.checked_div(node_weights.text_bytes_per_unit);
                node_weights.inline_root + text_weight.unwrap_or(0)
            } else if node
                .layout_children
                .borrow()
                .as_ref()
                .is_some_and(|c| !c.is_empty())
            {
                node_weights.container
            } else {
                node_weights.leaf
            };
            #[cfg(feature = "floats")]
            {
                use taffy::BlockItemStyle as _;
                let is_layout_node = node.primary_styles().is_some();
                info.has_floats[slot] = is_layout_node && node.layout_style().float().is_floated();
            }
        }
        for (node_id, parent_id) in order.into_iter().rev() {
            if let Some(parent_id) = parent_id {
                let (slot, parent_slot) = (slot(node_id), slot(parent_id));
                info.weights[parent_slot] =
                    info.weights[parent_slot].saturating_add(info.weights[slot]);
                info.has_floats[parent_slot] |= info.has_floats[slot];
            }
        }

        self.layout_subtree_info = info;
    }

    /// Whether the subtree of a node contains a floated box
    pub(crate) fn subtree_may_contain_floats(&self, node_id: taffy::NodeId) -> bool {
        self.layout_subtree_info
            .has_floats
            .get(slot(dom_node_id(node_id)))
            .copied()
            .unwrap_or(true)
    }
}

impl<'doc> LayoutPassState<'doc> {
    /// Round the layouts of the subtrees below a node (see `RoundTree::round_child_subtrees`),
    /// in parallel if they are large enough
    pub(crate) fn round_child_subtrees_maybe_in_parallel<F>(
        &mut self,
        node_id: taffy::NodeId,
        cumulative_x: f32,
        cumulative_y: f32,
        round_subtree: F,
    ) where
        F: Fn(&mut Self, taffy::NodeId, f32, f32) + Copy + Send + Sync,
    {
        use taffy::{RoundTree as _, TraversePartialTree as _};

        // In-flow children, then the out-of-flow boxes for which this node is the containing block
        let in_flow_child_count = self.child_count(node_id);
        let hoisted_child_count = self.hoisted_child_count(node_id);
        let child_ids = move |doc: &Self| {
            let in_flow = (0..in_flow_child_count)
                .map(|index| doc.get_child_id(node_id, index))
                .filter(|child| !doc.is_out_of_flow(*child));
            let hoisted =
                (0..hoisted_child_count).map(|index| doc.get_hoisted_child_id(node_id, index));
            in_flow.chain(hoisted).collect::<Vec<_>>()
        };

        let min_batch_weight = MIN_ROUND_BATCH_WEIGHT.load(std::sync::atomic::Ordering::Relaxed);
        let weight_of = |doc: &Self, node: taffy::NodeId| {
            let weights = &doc.layout_subtree_info.weights;
            weights.get(slot(dom_node_id(node))).copied().unwrap_or(1)
        };
        let is_parallel = self.parallel_layout_min_batch_weight.is_some()
            && in_flow_child_count + hoisted_child_count >= 2
            && weight_of(self, node_id) >= min_batch_weight;
        if !is_parallel {
            for index in 0..in_flow_child_count {
                let child = self.get_child_id(node_id, index);
                if !self.is_out_of_flow(child) {
                    round_subtree(self, child, cumulative_x, cumulative_y);
                }
            }
            for index in 0..hoisted_child_count {
                let child = self.get_hoisted_child_id(node_id, index);
                round_subtree(self, child, cumulative_x, cumulative_y);
            }
            return;
        }

        let children = child_ids(self);
        let weights: Vec<u32> = children.iter().map(|c| weight_of(self, *c)).collect();
        let total_weight: u64 = weights.iter().map(|weight| *weight as u64).sum();
        let doc_ptr = DocPtr(self.doc);
        let fields = self.pass_fields();
        run_on_layout_pool(|| {
            round_subtrees_split_by_weight(
                doc_ptr,
                fields,
                &children,
                &weights,
                total_weight,
                (min_batch_weight / 4).max(1) as u64,
                cumulative_x,
                cumulative_y,
                round_subtree,
            )
        });
    }

    /// Compute the layout for a job, without consulting (or storing the result to) the node's cache
    fn compute_uncached_job(&mut self, job: &ChildLayoutJob) -> LayoutOutput {
        if job.is_in_parent_bfc {
            let mut bfc = BlockFormattingContext::float_free();
            let mut block_ctx = bfc.detached_block_context();
            self.compute_child_layout_internal(job.node, job.input, Some(&mut block_ctx))
        } else {
            self.compute_child_layout_internal(job.node, job.input, None)
        }
    }

    /// Compute a batch of child layouts, in parallel if the subtrees that need to be laid out are
    /// large enough.
    ///
    /// The caches of the children are read before any tasks are spawned (so that no task is spawned
    /// for a job whose result is cached), and written after all of the tasks have finished.
    pub(crate) fn compute_layout_batch<'a>(
        &mut self,
        jobs: impl Iterator<Item = &'a mut ChildLayoutJob>,
    ) {
        let mut uncached_jobs: Vec<&mut ChildLayoutJob> = Vec::new();
        let mut job_weights: Vec<u32> = Vec::new();
        let mut weight: u32 = 0;
        for job in jobs {
            if let Some(output) = self.cache_get(job.node, &job.input) {
                job.output = output;
            } else {
                let job_weight = self
                    .layout_subtree_info
                    .weights
                    .get(slot(dom_node_id(job.node)))
                    .copied()
                    .unwrap_or(1);
                weight = weight.saturating_add(job_weight);
                job_weights.push(job_weight);
                uncached_jobs.push(job);
            }
        }

        let min_batch_weight = self.parallel_layout_min_batch_weight;
        if PROFILE.load(std::sync::atomic::Ordering::Relaxed)
            && uncached_jobs.len() >= 2
            && min_batch_weight.is_some_and(|min| weight >= min)
        {
            let batch_start = std::time::Instant::now();
            let mut jobs_ns = 0u64;
            let mut max_job_tinf = 0u64;
            for job in uncached_jobs.iter_mut() {
                PROFILE_STACK.with(|st| st.borrow_mut().push((0, 0)));
                let job_start = std::time::Instant::now();
                job.output = self.compute_uncached_job(job);
                let job_ns = job_start.elapsed().as_nanos() as u64;
                let (saved, _) = PROFILE_STACK.with(|st| st.borrow_mut().pop().unwrap());
                jobs_ns += job_ns;
                max_job_tinf = max_job_tinf.max(job_ns.saturating_sub(saved));
            }
            let batch_ns = batch_start.elapsed().as_nanos() as u64;
            let batch_tinf = batch_ns.saturating_sub(jobs_ns) + max_job_tinf;
            PROFILE_STACK.with(|st| {
                let mut st = st.borrow_mut();
                let top = st.last_mut().unwrap();
                top.0 += batch_ns.saturating_sub(batch_tinf);
                top.1 += batch_ns;
            });
        } else if uncached_jobs.len() >= 2 && min_batch_weight.is_some_and(|min| weight >= min) {
            // Group small jobs so that the subtrees laid out by each task are (on average)
            // at least a quarter of the weight of the smallest batch that is run in parallel
            let min_task_weight = (min_batch_weight.unwrap_or(0) / 4).max(1) as usize;
            let min_len = (min_task_weight * uncached_jobs.len() / (weight.max(1) as usize)).max(1);
            let doc_ptr = DocPtr(self.doc);
            let fields = self.pass_fields();
            let split_by_weight = SPLIT_BY_WEIGHT.load(std::sync::atomic::Ordering::Relaxed);
            let compute_jobs = || {
                if split_by_weight {
                    compute_jobs_split_by_weight(
                        doc_ptr,
                        fields,
                        &mut uncached_jobs,
                        &job_weights,
                        weight as u64,
                        min_task_weight as u64,
                    );
                } else {
                    uncached_jobs
                        .par_iter_mut()
                        .with_min_len(min_len)
                        .for_each(|job| {
                            // SAFETY: see the module docs. This is not sound in general.
                            let mut state = unsafe { LayoutPassState::for_task(doc_ptr, fields) };
                            job.output = state.compute_uncached_job(job);
                        });
                }
            };
            run_on_layout_pool(compute_jobs);
        } else {
            for job in uncached_jobs.iter_mut() {
                job.output = self.compute_uncached_job(job);
            }
        }

        for job in uncached_jobs {
            self.cache_store(job.node, &job.input, job.output.clone());
        }
    }
}

#[doc(hidden)]
pub static PROFILE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
thread_local! {
    static PROFILE_STACK: std::cell::RefCell<Vec<(u64, u64)>> = const { std::cell::RefCell::new(Vec::new()) };
}
#[doc(hidden)]
pub fn profile_begin() {
    PROFILE.store(true, std::sync::atomic::Ordering::Relaxed);
    PROFILE_STACK.with(|st| {
        let mut st = st.borrow_mut();
        st.clear();
        st.push((0, 0));
    });
}
/// Returns (time saved with unlimited threads, time inside outermost parallel batches,
/// time inside outermost layouts of inline boxes)
#[doc(hidden)]
pub fn profile_end() -> (u64, u64, u64) {
    use std::sync::atomic::Ordering::Relaxed;
    PROFILE.store(false, Relaxed);
    NODE_STACK.with(|st| st.borrow_mut().clear());
    INLINE_BOX_DEPTH.with(|d| d.set(0));
    let (saved, in_batches) = PROFILE_STACK.with(|st| st.borrow_mut().pop().unwrap());
    (saved, in_batches, INLINE_BOX_NS.swap(0, Relaxed))
}

/// Whether the profile treats the inline boxes of an inline root as if they were laid out in parallel
#[doc(hidden)]
pub static PROFILE_INLINE_WHATIF: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static INLINE_BOX_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct NodeFrame {
    is_inline_root: bool,
    parent_is_inline_root: bool,
    start: std::time::Instant,
    child_sum_ns: u64,
    child_sum_saved: u64,
    child_max_tinf: u64,
}
thread_local! {
    static NODE_STACK: std::cell::RefCell<Vec<NodeFrame>> = const { std::cell::RefCell::new(Vec::new()) };
    static INLINE_BOX_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

pub(crate) fn profile_node_enter(is_inline_root: bool) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    if !PROFILE.load(Relaxed) {
        return false;
    }
    NODE_STACK.with(|st| {
        let mut st = st.borrow_mut();
        let parent_is_inline_root = st.last().is_some_and(|f| f.is_inline_root);
        if parent_is_inline_root {
            PROFILE_STACK.with(|st| st.borrow_mut().push((0, 0)));
            INLINE_BOX_DEPTH.with(|d| d.set(d.get() + 1));
        }
        st.push(NodeFrame {
            is_inline_root,
            parent_is_inline_root,
            start: std::time::Instant::now(),
            child_sum_ns: 0,
            child_sum_saved: 0,
            child_max_tinf: 0,
        });
    });
    true
}

pub(crate) fn profile_node_exit() {
    use std::sync::atomic::Ordering::Relaxed;
    NODE_STACK.with(|st| {
        let mut st = st.borrow_mut();
        let frame = st.pop().unwrap();
        let ns = frame.start.elapsed().as_nanos() as u64;
        if frame.is_inline_root {
            let saved = if PROFILE_INLINE_WHATIF.load(Relaxed) {
                frame.child_sum_ns.saturating_sub(frame.child_max_tinf)
            } else {
                frame.child_sum_saved
            };
            PROFILE_STACK.with(|st| st.borrow_mut().last_mut().unwrap().0 += saved);
        }
        if frame.parent_is_inline_root {
            let (saved, in_batches) = PROFILE_STACK.with(|st| {
                let mut st = st.borrow_mut();
                let popped = st.pop().unwrap();
                st.last_mut().unwrap().1 += popped.1;
                popped
            });
            let _ = in_batches;
            let depth = INLINE_BOX_DEPTH.with(|d| {
                d.set(d.get() - 1);
                d.get()
            });
            if depth == 0 {
                INLINE_BOX_NS.fetch_add(ns, Relaxed);
            }
            let parent = st.last_mut().unwrap();
            parent.child_sum_ns += ns;
            parent.child_sum_saved += saved;
            parent.child_max_tinf = parent.child_max_tinf.max(ns.saturating_sub(saved));
        }
    });
}

/// The time taken by the phases of the last layout pass, in nanoseconds:
/// the subtree weight walk, the layout itself, and rounding
#[doc(hidden)]
pub static LAYOUT_PHASE_NS: [std::sync::atomic::AtomicU64; 3] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];
