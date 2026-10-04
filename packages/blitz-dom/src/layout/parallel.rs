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

impl LayoutPassState<'_> {
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
                uncached_jobs.push(job);
            }
        }

        let min_batch_weight = self.parallel_layout_min_batch_weight;
        if uncached_jobs.len() >= 2 && min_batch_weight.is_some_and(|min| weight >= min) {
            // Group small jobs so that the subtrees laid out by each task are (on average)
            // at least a quarter of the weight of the smallest batch that is run in parallel
            let min_task_weight = (min_batch_weight.unwrap_or(0) / 4).max(1) as usize;
            let min_len = (min_task_weight * uncached_jobs.len() / (weight.max(1) as usize)).max(1);
            let doc_ptr = DocPtr(self.doc);
            let fields = self.pass_fields();
            let mut compute_jobs = || {
                uncached_jobs
                    .par_iter_mut()
                    .with_min_len(min_len)
                    .for_each(|job| {
                        // SAFETY: see the module docs. This is not sound in general.
                        let mut state = unsafe { LayoutPassState::for_task(doc_ptr, fields) };
                        job.output = state.compute_uncached_job(job);
                    });
            };
            match dedicated_thread_pool() {
                // A rayon thread that waits for another thread pool still runs tasks from its own
                // thread pool in the meantime, so the wait is done by a thread that is not in a pool
                Some(pool) if pool.current_thread_index().is_none() => std::thread::scope(|s| {
                    let result = s.spawn(|| pool.install(compute_jobs)).join();
                    if let Err(panic) = result {
                        std::panic::resume_unwind(panic);
                    }
                }),
                _ => compute_jobs(),
            }
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
