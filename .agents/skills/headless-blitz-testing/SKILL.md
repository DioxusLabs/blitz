---
name: headless-blitz-testing
description: How to test blitz / dioxus-native behaviour end-to-end on a headless machine — synthesizing input events, counting shell (redraw/cursor) callbacks, and rendering frames to PNG without opening a window.
---

# Headless end-to-end testing of blitz / dioxus-native

The CI/dev VMs used for this repo usually have **no GPU (no Vulkan/GL) and no display**, so the
default renderer panics and opening a window is not an option. Everything below runs headlessly.

## Where to put a harness

The easiest place is a throwaway example in the repo root crate (`blitz-examples`):
`examples/<name>.rs`, run with `cargo run --example <name>`. Its dev-dependencies already include
`dioxus`, `dioxus-native` (which re-exports all of `dioxus-native-dom`, e.g. `DioxusDocument`),
`blitz-dom`, `blitz-paint`, `anyrender`, `anyrender_vello_cpu`, `png`, `peniko`.
Package-local integration tests (e.g. `packages/dioxus-native-dom/tests/`) work too but their
dev-deps do not include the painting crates, so you cannot render PNGs there without editing
Cargo.toml. Delete throwaway examples afterwards and never commit them.

## Building a dioxus-native document headlessly

```rust
let vdom = VirtualDom::new(app);              // or new_with_props for injectable state
let mut doc = DioxusDocument::new(vdom, DocumentConfig {
    viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
    shell_provider: Some(my_shell.clone()),   // Arc<dyn ShellProvider>
    ..Default::default()
});
doc.initial_build();                          // first render of the vdom into the DOM
doc.inner.borrow_mut().resolve(0.0);          // style + layout; REQUIRED before reading final_layout()
```

- `DioxusDocument::inner` is `Rc<RefCell<BaseDocument>>`; use it for `resolve`, `query_selector`,
  `get_node(..).final_layout()`.
- **Layout is stale until you call `resolve`** — reading `final_layout()` right after `poll()`
  returns the pre-mutation geometry. Always `resolve(0.0)` first, otherwise a working feature looks
  broken.

## Instrumenting shell callbacks (redraw requests, cursor, title, clipboard…)

Implement `blitz_traits::shell::ShellProvider` on a counter type and pass it via
`DocumentConfig::shell_provider`. All trait methods have defaults, so only override what you count:

```rust
#[derive(Default)]
struct CountingShell { redraws: AtomicUsize }
impl ShellProvider for CountingShell {
    fn request_redraw(&self) { self.redraws.fetch_add(1, Ordering::Relaxed); }
}
```

Caveat that will silently ruin an assertion: `handle_ui_event` requests redraws on its own for
hover/cursor changes (`BaseDocument`), so a raw total is not attributable to the code under test.
**Reset the counter immediately before the step you are measuring** (e.g. right before `poll()`)
and assert on the delta.

## Synthesizing input without a mouse/keyboard

Build a `BlitzPointerEvent` and push `UiEvent::PointerMove/PointerDown/PointerUp` into
`Document::handle_ui_event` — this is exactly what `packages/blitz-shell/src/window.rs` does for real
winit input. Copy the `pointer_event(x, y)` helper from
`packages/blitz-html/tests/pointer_events.rs` (fields: `id`, `is_primary`, `coords` with
page/screen/client x/y, `button`, `buttons`, `mods`, `details`, `element`, `active_pointers`).

Programmatic (non-event) state updates: either drive a signal from an event handler, or inject shared
state (`Rc<Cell<T>>` as vdom props) and then `doc.vdom.mark_dirty(ScopeId::APP)`. In both cases the
DOM is only mutated when you call `doc.poll(None)` (this is the `MutationWriter` /
`DocumentMutator` path).

## Rendering a frame to PNG

Follow `examples/screenshot.rs`: `render_to_buffer::<VelloCpuImageRenderer, _>(|scene| { fill white
background; blitz_paint::paint_scene(scene, &mut inner, scale, w, h, 0, 0) }, w, h)` then encode with
the `png` crate. This is pure CPU and works with no GPU. For objective assertions, compare PNGs
numerically (e.g. numpy bounding box of dark pixels) instead of eyeballing them.

## Proving a fix with a negative control

The change is usually already committed on the branch, so `git stash` finds nothing. Revert just the
file under test into the working tree, re-run, then restore:

```
git checkout HEAD~1 -- packages/blitz-dom/src/mutator.rs   # broken state
cargo run --example <harness>
git checkout HEAD   -- packages/blitz-dom/src/mutator.rs   # restore
```

## Detached vs in-document mutations

Several behaviours (redraw requests, style invalidation) are gated on `NodeFlags::IS_IN_DOCUMENT`.
To exercise both sides, use the real mutator API on a live document rather than a dioxus render — a
dioxus render always ends by attaching its template nodes, so it can never produce a "detached only"
flush:

```rust
let mut inner = doc.inner.borrow_mut();
let mut m = inner.mutate();                       // DocumentMutator; effects fire on drop
let p = m.create_element(blitz_dom::qual_name!("div"), vec![]);  // detached
m.append_children(p, &[m.create_element(blitz_dom::qual_name!("span"), vec![])]);
// ... in-document case: m.append_children(main_id, &[n]) where main_id = query_selector("main")
```

Read a node's flag with `inner.get_node(id).unwrap().flags.is_in_document()`. Known trap: a node
moved from the document into a *detached* parent currently keeps `IS_IN_DOCUMENT == true`
(`add_children_to_parent` calls `process_added_subtree` whenever the parent/child flags differ, in
both directions), so "is it detached now?" assertions based on that flag can be misleading — assert
on observable behaviour and print the flag alongside it.

## Gotchas

- **Disk space**: full workspace debug builds are ~30 GB per repo. Builds fail with
  `No space left on device` / `rustc-LLVM ERROR: IO failure on output stream`. Check `df -h` first;
  stale sibling build caches (e.g. `~/repos/*/target`) are regenerable and are the safest thing to
  delete after confirming no `cargo`/`rustc` process is running.
- Default UA CSS gives `body` an 8px margin, so an element at `left: 300px; top: 200px` renders at
  ~(308, 208) in the PNG. Do not treat that offset as a bug.
- Lint/test commands: `cargo fmt --all`, `cargo clippy --workspace`, `cargo test --workspace`.
  `cargo fmt --all --check` will flag your throwaway harness — format it or ignore that diff.
- If a GUI window is genuinely unavoidable, use `--no-default-features --features cpu-softbuffer`.

## Devin Secrets Needed

None — everything above runs locally with no credentials or network access.
