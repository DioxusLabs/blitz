# CSS Positioning Implementation Plan for Blitz + Taffy

## Context

Blitz is a native browser renderer using Taffy for layout and Stylo for CSS. Currently, `position: absolute` children are positioned relative to their **direct DOM parent** in the Taffy layout tree, but CSS specifies they should be positioned relative to their **nearest positioned ancestor** (the "containing block"). Additionally, `position: fixed` is mapped to absolute (no viewport-relative behavior) and `position: sticky` is mapped to relative (no scroll-time clamping). This plan fixes all CSS positioning schemes.

**Local Taffy**: `~/Development/taffy` — Cargo patch override available but no Taffy changes needed for this work. Taffy already handles absolute positioning correctly; the fix is entirely in how Blitz builds the layout tree.

**Sticky-ready**: All architectural decisions below are designed so `position: sticky` can be added later without refactoring. Foundations for sticky (the `css_position` field, inset suppression, paint-time offset pattern) are laid in Unit 1.

## Phase 0: Cargo Setup

### 0.1 — Patch Taffy to use local copy (optional)

In `Cargo.toml`, uncomment and fix the taffy patch if Taffy changes become needed:

```toml
[patch."https://github.com/dioxuslabs/taffy"]
taffy = { path = "/Users/jonathankelley/Development/taffy" }
```

No Taffy source changes are required for absolute/fixed positioning. The patch is available for future sticky work or bugfixes.

## Phase 1: Audit Findings (Summary)

### Current Pipeline
```
DOM Mutation → Damage → Style (Stylo) → Layout Tree (collect_layout_children)
  → Style Flush (flush_styles_to_layout) → Layout (Taffy) → Paint (blitz-paint)
```

### Taffy Capabilities
- **Position enum**: Only `Relative` and `Absolute` (no Fixed/Sticky)
- **Absolute positioning**: Fully implemented in block, flex, grid algorithms
  - Filters absolute children from normal flow
  - Resolves insets against parent's border-box minus scrollbar
  - Handles auto margins, RTL, static position fallback
- **No detached node layout API** — containing block is always the parent node

### Blitz Current State
- **Style conversion** (`stylo_taffy/convert.rs:222-233`): `fixed→Absolute`, `sticky→Relative`, `static→Relative`
- **Stacking contexts** (`damage.rs:533`): Only hoists `position != static && z_index != 0`
- **Paint order** (`damage.rs:628-657`): neg-z → in-flow → pos-z
- **Inline abs positioning** (`inline.rs:599-622`): Custom handling for inline abs boxes
- **Layout tree** already diverges from DOM (anonymous blocks, display:contents)

### The Critical Bug: Containing Block
Taffy positions absolute children against their layout-tree parent. Blitz's layout tree mirrors DOM structure. So:
```html
<div style="position: relative">     ← should be containing block
  <div>                               ← non-positioned
    <div style="position: absolute">  ← WRONG: positioned against middle div
```

## Phase 2: Architecture — Reparent Absolute Children in Layout Tree

### Why Reparenting (not post-layout fixup)

- **Post-layout fixup fails** because percentage widths/heights on absolute children resolve against the containing block during layout. Wrong containing block → wrong sizes → can't fix after.
- **Taffy API changes** (custom containing block per child) would be invasive and still need positional fixup.
- **Reparenting works** because Taffy already handles absolute positioning correctly when children are under the right parent. The layout tree already diverges from DOM (anonymous blocks, display:contents), so this is a natural extension.

### Algorithm: Two-phase layout tree construction

After the existing `resolve_layout_children_recursive` pass, add a `reparent_out_of_flow_children` post-pass:

```
resolve_layout_children()     ← existing: builds layout_children from DOM
  ↓
reparent_out_of_flow_children() ← NEW: moves abs/fixed children to correct ancestor
  ↓
flush_styles_to_layout()      ← existing: converts styles, builds stacking contexts
  ↓
resolve_layout()              ← existing: Taffy compute_root_layout
```

### Data Structure Changes

#### Node (`node.rs`)

Add field to preserve original CSS position (since `stylo_taffy/convert.rs` loses Fixed→Absolute and Sticky→Relative):

```rust
/// Original CSS position value (not the Taffy mapping)
pub css_position: style::computed_values::position::T,
```

Set during `flush_styles_to_layout_impl` alongside `node.style = stylo_taffy::to_taffy_style(style)`:
```rust
node.css_position = style.clone_position();
```

### Sticky-Safe Style Conversion (`stylo_taffy/convert.rs`)

**Do this in Unit 1** — sticky elements map to `taffy::Position::Relative`, but their insets define sticking thresholds, NOT layout offsets. If we pass insets through, Taffy applies them as relative offsets (wrong). Suppress now so sticky doesn't silently break:

```rust
pub fn to_taffy_style(style: &stylo::ComputedValues) -> taffy::Style<Atom> {
    let css_position = style.clone_position();
    // ...
    inset: if css_position == stylo::Position::Sticky {
        // Sticky insets are sticking thresholds, not layout offsets.
        // Raw values remain accessible via Stylo computed styles for scroll-time use.
        taffy::Rect::AUTO
    } else {
        taffy::Rect {
            left: self::inset(&pos.left),
            right: self::inset(&pos.right),
            top: self::inset(&pos.top),
            bottom: self::inset(&pos.bottom),
        }
    },
    // ...
}
```

### Core Implementation

#### File: `resolve.rs` — Add `reparent_out_of_flow_children`

```rust
fn reparent_out_of_flow_children(&mut self) {
    use style::computed_values::position::T as Position;

    // Collect reparenting operations: (child_id, old_parent_id, new_parent_id)
    let mut reparent_list: Vec<(usize, usize, usize)> = Vec::new();

    for (node_id, node) in self.nodes.iter() {
        let Some(style) = node.primary_styles() else { continue };
        let position = style.clone_position();

        let is_abs = position == Position::Absolute;
        let is_fixed = position == Position::Fixed;
        // NOTE: Sticky is intentionally NOT reparented.
        // Sticky elements stay in normal flow under their DOM parent.
        // Their visual offset is computed at scroll-time, not layout-time.
        if !is_abs && !is_fixed { continue; }

        let Some(current_parent) = *node.layout_parent.borrow() else { continue };

        let target = if is_fixed {
            self.find_fixed_containing_block(node_id)
        } else {
            self.find_absolute_containing_block(node_id)
        };

        if let Some(target) = target {
            if current_parent != target {
                reparent_list.push((node_id, current_parent, target));
            }
        }
    }

    // Apply reparenting
    for (child_id, old_parent, new_parent) in reparent_list {
        if let Some(ref mut children) = *self.nodes[old_parent].layout_children.borrow_mut() {
            children.retain(|&id| id != child_id);
        }
        if let Some(ref mut children) = *self.nodes[new_parent].layout_children.borrow_mut() {
            children.push(child_id);
        }
        self.nodes[child_id].layout_parent.set(Some(new_parent));
    }
}
```

#### Containing Block Resolution

```rust
fn find_absolute_containing_block(&self, node_id: usize) -> Option<usize> {
    // Walk DOM parents to find nearest positioned ancestor
    let mut current = self.nodes[node_id].parent?;
    loop {
        if self.is_positioned(&self.nodes[current]) {
            return Some(current);
        }
        match self.nodes[current].parent {
            Some(p) => current = p,
            None => return Some(current), // root = initial containing block
        }
    }
}

fn find_fixed_containing_block(&self, node_id: usize) -> Option<usize> {
    // Walk DOM parents looking for transform/filter/perspective ancestor
    // If none found, return root (viewport)
    let mut current = self.nodes[node_id].parent?;
    loop {
        if self.creates_containing_block_for_fixed(&self.nodes[current]) {
            return Some(current);
        }
        match self.nodes[current].parent {
            Some(p) => current = p,
            None => return Some(current), // root = viewport
        }
    }
}

fn is_positioned(&self, node: &Node) -> bool {
    node.primary_styles()
        .map(|s| s.clone_position() != Position::Static)
        .unwrap_or(false)
}

fn creates_containing_block_for_fixed(&self, node: &Node) -> bool {
    let Some(style) = node.primary_styles() else { return false };
    // CSS spec: transform, perspective, filter, will-change mentioning these
    !style.get_box().transform.0.is_empty()
        || !style.get_effects().filter.0.is_empty()
        // || style.get_box().perspective != Perspective::None
        // || will-change mentions transform/filter
}
```

### Integration Point in `resolve.rs`

```rust
pub fn resolve(&mut self, current_time_for_animations: f64) {
    // ... existing code ...
    self.resolve_layout_children();
    self.reparent_out_of_flow_children();  // ← NEW
    self.resolve_deferred_tasks();
    self.flush_styles_to_layout(root_node_id);
    self.resolve_layout();
    // ...
}
```

### Paint Order — No Major Changes Needed

The existing stacking context mechanism in `flush_styles_to_layout_impl` handles paint order separately from layout order. When iterating `layout_children` to build `paint_children`, reparented absolute children won't appear in their DOM parent's `layout_children` anymore. But they WILL appear in their containing block ancestor's `layout_children`.

The current code in `damage.rs:521-541` decides whether each child goes into `paint_children` (in-flow order) or gets hoisted to `stacking_context` (z-indexed). After reparenting, absolute children appear as children of the containing block, and the existing logic will:
1. If `z_index != 0`: hoist to stacking context (correct)
2. If `z_index == 0`: add to `paint_children` with `position_to_order` returning 2 for absolute (paints after in-flow children, correct per CSS spec)

**Potential issue**: The accumulated position offset for hoisted children (`damage.rs:557-562`) uses `final_layout.location` which is relative to the layout parent. After reparenting, this is relative to the containing block, which is correct for painting since the stacking context root is also the containing block ancestor.

## Phase 3: position: relative

### Status: Already Works

Taffy handles `Position::Relative` with inset offsets. In `block.rs:914`, after laying out relative items, it computes `inset_offset` from the style's inset values and adds it to the location. Blitz converts `position: relative` to `taffy::Position::Relative` and converts inset values. No changes needed.

**Note**: `position: static` also maps to `Relative` in Taffy. This is safe because Stylo computes inset as `auto` for static elements, so no offset gets applied.

## Phase 4: position: fixed

### Layout — Handled by Reparenting

With the reparenting approach, fixed children get reparented to the root element (or nearest transform ancestor). The root's containing block is the viewport (`available_space` in `resolve_layout`). Taffy positions them against the root's border box, which equals the viewport dimensions. This is correct.

### Paint — Scroll Compensation

Fixed elements must not move when the page scrolls. Currently, `render.rs` applies scroll offsets during rendering. Fixed elements need the viewport scroll offset undone.

#### File: `render.rs` — In `render_element` or `draw_children`

When painting a fixed-position node, compensate for accumulated scroll:

```rust
// When rendering a node that is position:fixed:
if node.css_position == Position::Fixed {
    // Undo viewport scroll so element stays fixed on screen
    pos.x += self.context.viewport_scroll.x as f64;
    pos.y += self.context.viewport_scroll.y as f64;
}
```

This goes in the render path where child positions are computed, likely in `render_node` or at the start of `render_element`.

## Phase 5: position: sticky

### What's Done Now (Foundations)
These are included in Unit 1 to prevent sticky from silently breaking:
- **`css_position` field** on Node — stores `Position::Sticky` (not lost to the `Relative` mapping)
- **Inset suppression** in `convert.rs` — sticky insets are set to `Auto` for Taffy so they don't get applied as relative offsets
- **Not reparented** — reparenting pass explicitly skips sticky (they stay in normal flow)
- **Stacking context** — `is_stacking_context_root` already returns `true` for sticky

### What's Needed Later (Scroll-Time Behavior)

#### 1. Scroll Container Resolution
Sticky elements stick relative to their nearest **scroll container** (ancestor with `overflow: auto|scroll|hidden`). Need a helper:
```rust
fn find_scroll_container(&self, node_id: usize) -> Option<usize> {
    // Walk DOM parents looking for overflow != visible
    // Blitz already tracks scroll_offset on nodes, so the infrastructure exists
}
```

#### 2. Scroll-Time Offset Computation
Called during paint (not layout — no relayout needed on scroll):
```rust
fn compute_sticky_offset(&self, node_id: usize) -> Point<f32> {
    // 1. Get sticky thresholds from Stylo computed styles (pos.top, pos.bottom, etc.)
    // 2. Get element's normal-flow position from final_layout.location
    // 3. Get scroll container's scroll_offset and visible area
    // 4. Clamp position to stay within visible area minus thresholds
    // 5. Clamp again to stay within containing block bounds (sticky stops at CB edge)
    // 6. Return offset = clamped_position - normal_position
}
```

#### 3. Paint Integration
Apply sticky offset in `render.rs` as a translation, same pattern as fixed scroll compensation:
```rust
if node.css_position == Position::Sticky {
    let offset = self.context.dom.compute_sticky_offset(node_id);
    pos.x += offset.x as f64;
    pos.y += offset.y as f64;
}
```

This pattern mirrors the fixed-positioning paint hook, so the two won't conflict.

#### 4. Scroll Event Integration
When scroll events fire, sticky elements need repaint (not relayout). Blitz's existing `scroll_by` → repaint flow handles this — no architectural changes needed, just ensuring the paint path calls `compute_sticky_offset`.

### Why This Can Be Added Later Without Refactoring
- `css_position` field is already populated → sticky detection works everywhere
- Insets already suppressed → no wrong offsets to undo
- Reparenting already skips sticky → no tree structure to change
- Paint system already has a per-position-type offset hook (fixed) → sticky adds another case
- Scroll infrastructure (scroll_offset per node, scroll events) already exists

## Phase 6: Stacking Contexts

### Current State (`node.rs:838-868`)

`is_stacking_context_root` checks:
- opacity != 1.0
- position: fixed | sticky → always
- position: relative | absolute with z-index set
- position: static with z-index set AND flex/grid item
- TODOs for: mix-blend-mode, transforms, filter, clip-path, mask, isolation, contain

### Expand `is_stacking_context_root`

```rust
pub fn is_stacking_context_root(&self, is_flex_or_grid_item: bool) -> bool {
    // ... existing checks ...

    // Transform (any value other than none)
    if !style.get_box().transform.0.is_empty() { return true; }

    // Filter (any value other than none)
    if !style.get_effects().filter.0.is_empty() { return true; }

    // Mix-blend-mode (any value other than normal)
    // if style.get_effects().mix_blend_mode != MixBlendMode::Normal { return true; }

    // Isolation: isolate
    // if style.get_box().isolation == Isolation::Isolate { return true; }

    // Perspective (any value other than none)
    // if style.get_box().perspective != Perspective::None { return true; }

    false
}
```

### Unify Hoisting Check in `damage.rs:533`

Current hoisting condition is `position != Static && z_index != 0`. This should also hoist any child that creates a stacking context for other reasons:

```rust
// Replace:
if position != Position::Static && z_index != 0 {
// With:
let creates_stacking_context = child.is_stacking_context_root(is_flex_or_grid);
let should_hoist = (position != Position::Static && z_index != 0) || creates_stacking_context;
if should_hoist {
```

## Phase 7: Edge Cases

### Static Position Fallback After Reparenting

When an absolute element has no insets (all auto), CSS says it should appear at its "static position" — where it would have been in normal flow. Taffy tracks this internally. After reparenting, the static position needs adjustment since it's now relative to a different parent.

**Mitigation**: Taffy computes static position relative to the layout parent's content box. After reparenting, the element's static position will be relative to the containing block's content box — which happens to be at (0, 0) relative to the containing block's content edge. This is actually incorrect (should be at the DOM parent's position within the containing block), but this is a rare edge case that can be addressed later.

### Inline Layout (`inline.rs`) Interaction

After reparenting, absolute inline boxes are no longer in their inline context's layout_children. The inline layout code (`inline.rs:303`) already sets absolute inline boxes to zero size. After reparenting, these boxes won't appear in the inline box list at all, so the zero-size code path won't trigger. Instead, they'll be laid out by Taffy as normal absolute children of the containing block. This is correct behavior — the inline context should not size them.

**One issue**: The inline box `ibox` entries in Parley's layout will reference node IDs that are no longer in the inline context's layout_children. These orphaned inline boxes should be filtered out. Add a check in `compute_inline_layout_inner` when iterating inline boxes to skip nodes that have been reparented.

### Hit Testing (`node.rs:878`)

The `hit()` method recursively descends DOM children. After reparenting, absolute children are no longer in the DOM parent's layout_children but ARE still in the DOM `children`. Hit testing uses `final_layout.location` which is now relative to the containing block, not the DOM parent. This means hit coordinates will be wrong.

**Fix**: During hit testing, for absolute/fixed children, compute their position relative to the DOM parent by walking the containing block chain and subtracting ancestor positions.

## Implementation Order (Prioritized)

### Unit 1: Foundation (includes sticky-safe groundwork)
- Add `css_position` field to `Node` struct (`node.rs`)
- Populate it during `flush_styles_to_layout_impl` (`damage.rs`)
- Suppress insets for `position: sticky` in `to_taffy_style` (`convert.rs`) — prevents wrong relative offsets
- Add `is_positioned()` and `creates_containing_block_for_fixed()` helpers
- **Test**: `cargo build -p blitz-dom`, verify sticky elements don't get inset offsets applied

### Unit 2: Containing Block Resolution + Reparenting
- Implement `find_absolute_containing_block()`
- Implement `find_fixed_containing_block()`
- Implement `reparent_out_of_flow_children()`
- Wire into `resolve()` pipeline after `resolve_layout_children()`
- **Test**: HTML with `position:relative` grandparent + `position:absolute` grandchild, insets resolve against grandparent

### Unit 3: Fixed Positioning Paint Fix
- Add viewport scroll compensation in `render.rs` for `css_position == Fixed`
- **Test**: Fixed header stays in place while scrolling

### Unit 4: Stacking Contexts Expansion
- Expand `is_stacking_context_root` (transform, filter)
- Unify hoisting check in `damage.rs`
- **Test**: Transform creates stacking context, z-index ordering with transforms

### Unit 5: Inline Layout Cleanup
- Filter reparented nodes from inline box iteration
- **Test**: Inline context with absolute child renders correctly

### Unit 6: Hit Testing Fix
- Update `hit()` to handle reparented absolute/fixed nodes
- **Test**: Click events hit absolute-positioned elements correctly

### Unit 7: Sticky Positioning (Future — foundations already in place from Unit 1)
- Implement `find_scroll_container()` helper
- Implement `compute_sticky_offset()` with threshold clamping + containing block bounds
- Add sticky offset application in `render.rs` paint path (same pattern as fixed)
- **Test**: Sticky header sticks during scroll, stops at containing block edge

## Key Files to Modify

| File                             | Changes                                                     |
| -------------------------------- | ----------------------------------------------------------- |
| `Cargo.toml`                     | Uncomment taffy patch, point to local                       |
| `blitz-dom/src/node/node.rs`     | Add `css_position` field, expand `is_stacking_context_root` |
| `blitz-dom/src/resolve.rs`       | Add `reparent_out_of_flow_children()` call in pipeline      |
| `blitz-dom/src/layout/damage.rs` | Unify stacking context hoisting check                       |
| `blitz-dom/src/layout/inline.rs` | Skip reparented nodes in inline box iteration               |
| `stylo_taffy/src/convert.rs`     | Suppress insets for sticky (Unit 1, day-1 fix)              |
| `blitz-paint/src/render.rs`      | Scroll compensation for fixed elements                      |

## Verification

1. `cargo build -p blitz-dom` — compiles with local taffy
2. `cargo test -p blitz-dom` — existing tests pass
3. Test HTML: absolute child inside nested non-positioned divs with positioned grandparent — child positions relative to grandparent
4. Test HTML: fixed header — stays in place during scroll
5. Test HTML: z-index ordering with transforms — correct stacking
6. Render real websites (e.g. news.ycombinator.com) — no regressions
