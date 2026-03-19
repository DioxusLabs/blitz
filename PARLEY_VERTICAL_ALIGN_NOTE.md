# Parley VerticalAlign API — alignment with CSS Inline 3

## Current state

Parley has a single `VerticalAlign` enum:

```rust
pub enum VerticalAlign {
    Baseline, Sub, Super, Top, Bottom, TextTop, TextBottom, Middle, Length(f32),
}
```

This mirrors the legacy CSS2 `vertical-align` property, which was a single value.

## CSS Inline Level 3 decomposition

Modern CSS (CSS Inline 3) decomposes `vertical-align` into a **shorthand** for three longhands:

| Longhand | Values | Purpose |
|---|---|---|
| `alignment-baseline` | `baseline`, `text-bottom`, `middle`, `text-top` (+ `alphabetic`, `ideographic`, `central`, `mathematical` in full spec) | Which baseline of the element aligns with which baseline of the parent |
| `baseline-shift` | `sub`, `super`, `top`, `center`, `bottom`, `<length-percentage>` | How much to shift from the chosen baseline |
| `baseline-source` | `auto`, `first`, `last` | Which baseline set (first or last) to use for alignment |

Stylo (the CSS engine used by Blitz/Servo) already parses `vertical-align` into these three longhands. Blitz currently does a lossy best-effort mapping from the three longhands back into parley's single enum (see `stylo_to_parley::vertical_align()`).

## Recommendation

Consider splitting parley's `VerticalAlign` into separate types that match the CSS Inline 3 model:

```rust
pub enum AlignmentBaseline {
    Baseline,
    TextBottom,
    Middle,
    TextTop,
    // Future: Alphabetic, Ideographic, Central, Mathematical
}

pub enum BaselineShift {
    Sub,
    Super,
    Top,
    Center,
    Bottom,
    Length(f32),
}

pub enum BaselineSource {
    Auto,
    First,
    Last,
}
```

Then `TextStyle` and `InlineBox` would carry these as separate fields instead of a single `vertical_align`.

### Benefits
- **1:1 mapping** from CSS engines (Stylo, etc.) — no lossy conversion needed
- **Spec-correct semantics** — `alignment-baseline` and `baseline-shift` are independent axes; combining them into one enum loses the ability to set e.g. `alignment-baseline: text-top` with a non-zero `baseline-shift` simultaneously
- **Forward-compatible** — `baseline-source: last` (for bottom-aligned content in table cells, etc.) can be supported without enum bloat

### Current workaround in Blitz
In `stylo_to_parley.rs`, the conversion prioritizes `baseline-shift` keywords, then falls through to `alignment-baseline` when the shift is zero. `baseline-source` is ignored entirely. This covers ~95% of real-world usage but is technically incorrect for combined values like `vertical-align: text-top sub`.

## References
- CSS Inline 3 spec: https://drafts.csswg.org/css-inline-3/#transverse-alignment
- Stylo shorthand impl: `stylo-0.12.0/properties/shorthands.rs` (vertical_align module)
- Blitz conversion: `packages/blitz-dom/src/stylo_to_parley.rs` (`vertical_align` function)
