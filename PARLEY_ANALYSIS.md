  ---
  Browser-Grade Text Layout: Gap Analysis

  Critical Gaps in Parley (inline text layout)

  Tier 1 — Needed for basic correctness

  ┌────────────────────────────────┬───────────────────────────────────┬───────────────────────────────────────────────────────────────────────────────────────────────────────┐
  │            Feature             │           CSS Property            │                                                Status                                                 │
  ├────────────────────────────────┼───────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────────────────────────┤
  │ Vertical align                 │ vertical-align                    │ Only bottom-aligned inline boxes; no baseline, middle, sub, super, top, bottom, text-top, text-bottom │
  ├────────────────────────────────┼───────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────────────────────────┤
  │ Text overflow / ellipsis       │ text-overflow, -webkit-line-clamp │ Not implemented                                                                                       │
  ├────────────────────────────────┼───────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────────────────────────┤
  │ Text transform                 │ text-transform                    │ Not implemented (uppercase, lowercase, capitalize)                                                    │
  ├────────────────────────────────┼───────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────────────────────────┤
  │ Overline decoration            │ text-decoration-line: overline    │ Not implemented (underline + strikethrough only)                                                      │
  ├────────────────────────────────┼───────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────────────────────────┤
  │ Decoration styles              │ text-decoration-style             │ Only solid; no dashed, dotted, wavy, double                                                           │
  ├────────────────────────────────┼───────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────────────────────────┤
  │ text-align-last                │ text-align-last                   │ Not implemented (last line of justified text)                                                         │
  ├────────────────────────────────┼───────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────────────────────────┤
  │ Inline box margins/padding     │ box model on <span> etc.          │ InlineBox is width+height only; no margin, padding, border                                            │
  ├────────────────────────────────┼───────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────────────────────────┤
  │ Lines with only inline boxes   │ —                                 │ FIXME in line_break.rs:1065 — not fully supported                                                     │
  ├────────────────────────────────┼───────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────────────────────────┤
  │ Mixed-direction content widths │ —                                 │ TODO in data.rs:543 — not handled at all                                                              │
  └────────────────────────────────┴───────────────────────────────────┴───────────────────────────────────────────────────────────────────────────────────────────────────────┘

  Tier 2 — Needed for real-world web content

  ┌───────────────────────────────┬─────────────────────────────────────────────────────────────────────────────────────────┬───────────────────────────────────────────────────┐
  │            Feature            │                                      CSS Property                                       │                      Status                       │
  ├───────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────────────────┤
  │ Hyphenation                   │ hyphens, hyphenate-character                                                            │ Not implemented                                   │
  ├───────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────────────────┤
  │ Writing modes                 │ writing-mode (vertical-rl, vertical-lr)                                                 │ Not implemented — horizontal only                 │
  ├───────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────────────────┤
  │ ::first-line / ::first-letter │ pseudo-elements                                                                         │ No hooks for style changes mid-layout             │
  ├───────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────────────────┤
  │ Text shadow                   │ text-shadow                                                                             │ Not implemented                                   │
  ├───────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────────────────┤
  │ Hanging punctuation           │ hanging-punctuation                                                                     │ Not implemented                                   │
  ├───────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────────────────┤
  │ text-justify                  │ text-justify                                                                            │ Only basic space distribution; no inter-character │
  ├───────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────────────────┤
  │ text-underline-position       │ text-underline-position                                                                 │ Not implemented (under, left, right)              │
  ├───────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────────────────┤
  │ text-decoration-skip-ink      │ text-decoration-skip-ink                                                                │ Not implemented                                   │
  ├───────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────────────────┤
  │ Font variant shorthands       │ font-variant-caps, font-variant-numeric, font-variant-position, font-variant-east-asian │ Must use raw OpenType tags via FontFeature        │
  ├───────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────────────────┤
  │ Soft hyphen / <wbr>           │ U+00AD, manual break hints                                                              │ No explicit API                                   │
  ├───────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────────────────┤
  │ CJK text spacing              │ text-autospace, text-spacing-trim                                                       │ Not implemented                                   │
  ├───────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────┼───────────────────────────────────────────────────┤
  │ Ruby annotations              │ <ruby>                                                                                  │ Not implemented                                   │
  └───────────────────────────────┴─────────────────────────────────────────────────────────────────────────────────────────┴───────────────────────────────────────────────────┘

  Tier 3 — Nice to have for completeness

  ┌──────────────────────────────────────────────────┬─────────────────────────────┐
  │                     Feature                      │           Status            │
  ├──────────────────────────────────────────────────┼─────────────────────────────┤
  │ dominant-baseline, alignment-baseline (SVG-like) │ Not implemented             │
  ├──────────────────────────────────────────────────┼─────────────────────────────┤
  │ initial-letter (drop caps)                       │ Not implemented             │
  ├──────────────────────────────────────────────────┼─────────────────────────────┤
  │ text-emphasis marks                              │ Not implemented             │
  ├──────────────────────────────────────────────────┼─────────────────────────────┤
  │ Emoji detection completeness                     │ TODO in shape/mod.rs:244    │
  ├──────────────────────────────────────────────────┼─────────────────────────────┤
  │ font-synthesis control                           │ Low-level only via fontique │
  └──────────────────────────────────────────────────┴─────────────────────────────┘

  Known Bugs / Incomplete Behavior

  - Ligatures don't break with letter-spacing (parley_tests/tests/styles/interactions.rs:73)
  - word-spacing doesn't expand content box for justified text (:161)
  - font-style doesn't auto-map to slnt axis for variable fonts (font_selection.rs:118)
  - Line metrics HACK: copies from previous line when unavailable (line_break.rs:796)

  ---
  Critical Gaps in Taffy (box layout engine)

  Taffy currently has no inline formatting context at all. From its CHANGELOG:
  ▎ "full flow layout: inline, inline-block and float layout have not been implemented."

  ┌──────────────────────────────────────────────────────────────────────────────────────────┬─────────────────────────────────────────────────────────┐
  │                                         Feature                                          │                         Status                          │
  ├──────────────────────────────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────┤
  │ display: inline                                                                          │ Not supported                                           │
  ├──────────────────────────────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────┤
  │ display: inline-block                                                                    │ Not supported                                           │
  ├──────────────────────────────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────┤
  │ display: inline-flex / inline-grid                                                       │ Not supported                                           │
  ├──────────────────────────────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────┤
  │ Inline formatting context                                                                │ No implementation                                       │
  ├──────────────────────────────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────┤
  │ Line box generation                                                                      │ Not implemented — must live in parley or a bridge layer │
  ├──────────────────────────────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────┤
  │ vertical-align on inline elements                                                        │ Not supported                                           │
  ├──────────────────────────────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────┤
  │ text-indent                                                                              │ Not supported                                           │
  ├──────────────────────────────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────┤
  │ white-space (layout-affecting parts: nowrap preventing line wrap, pre preserving breaks) │ Delegated to measure functions                          │
  ├──────────────────────────────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────┤
  │ Float interaction with inline content                                                    │ Not supported (float layout is block-only)              │
  ├──────────────────────────────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────┤
  │ text-align                                                                               │ Legacy only (for <center> / align="")                   │
  ├──────────────────────────────────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────┤
  │ Baseline alignment                                                                       │ Only in flexbox/grid, not inline flow                   │
  └──────────────────────────────────────────────────────────────────────────────────────────┴─────────────────────────────────────────────────────────┘

  ---
  Where Each Feature Should Live

  ┌──────────────────────────────────┬──────────────────────────────────────────┬───────────────────────────────────────┬──────────────────────────────────────────────────────┐
  │          Responsibility          │                  Parley                  │                 Taffy                 │                     Bridge/Blitz                     │
  ├──────────────────────────────────┼──────────────────────────────────────────┼───────────────────────────────────────┼──────────────────────────────────────────────────────┤
  │ Text shaping & glyph positioning │ Yes                                      │ —                                     │ —                                                    │
  ├──────────────────────────────────┼──────────────────────────────────────────┼───────────────────────────────────────┼──────────────────────────────────────────────────────┤
  │ Line breaking & wrapping         │ Yes                                      │ —                                     │ —                                                    │
  ├──────────────────────────────────┼──────────────────────────────────────────┼───────────────────────────────────────┼──────────────────────────────────────────────────────┤
  │ vertical-align (inline)          │ Yes — needs implementing                 │ —                                     │ —                                                    │
  ├──────────────────────────────────┼──────────────────────────────────────────┼───────────────────────────────────────┼──────────────────────────────────────────────────────┤
  │ Inline box margin/padding/border │ Partially — needs box model on InlineBox │ —                                     │ Could compute externally                             │
  ├──────────────────────────────────┼──────────────────────────────────────────┼───────────────────────────────────────┼──────────────────────────────────────────────────────┤
  │ text-overflow: ellipsis          │ Yes                                      │ —                                     │ —                                                    │
  ├──────────────────────────────────┼──────────────────────────────────────────┼───────────────────────────────────────┼──────────────────────────────────────────────────────┤
  │ text-transform                   │ —                                        │ —                                     │ Blitz (before passing text to parley)                │
  ├──────────────────────────────────┼──────────────────────────────────────────┼───────────────────────────────────────┼──────────────────────────────────────────────────────┤
  │ text-shadow                      │ —                                        │ —                                     │ Blitz (rendering layer)                              │
  ├──────────────────────────────────┼──────────────────────────────────────────┼───────────────────────────────────────┼──────────────────────────────────────────────────────┤
  │ Inline formatting context        │ —                                        │ Yes — or bridge                       │ Line box generation could be shared                  │
  ├──────────────────────────────────┼──────────────────────────────────────────┼───────────────────────────────────────┼──────────────────────────────────────────────────────┤
  │ display: inline-block sizing     │ —                                        │ Yes — needs IFC                       │ measure function exists but no IFC                   │
  ├──────────────────────────────────┼──────────────────────────────────────────┼───────────────────────────────────────┼──────────────────────────────────────────────────────┤
  │ Float exclusions in text         │ —                                        │ Yes — adjust available width per line │ Parley already takes max_advance per break_all_lines │
  ├──────────────────────────────────┼──────────────────────────────────────────┼───────────────────────────────────────┼──────────────────────────────────────────────────────┤
  │ ::first-line / ::first-letter    │ Yes — needs style-change hooks           │ —                                     │ Blitz triggers re-style                              │
  ├──────────────────────────────────┼──────────────────────────────────────────┼───────────────────────────────────────┼──────────────────────────────────────────────────────┤
  │ writing-mode                     │ Yes — deep architectural change          │ Yes — logical vs physical             │ Both need work                                       │
  └──────────────────────────────────┴──────────────────────────────────────────┴───────────────────────────────────────┴──────────────────────────────────────────────────────┘

  ---
  Recommended Priority Order

  1. Vertical-align in parley — you're already on this; it's a prerequisite for almost all real web content
  2. Inline box model in parley — add margin/padding/border to InlineBox so <span> styling works
  3. Inline formatting context in taffy (or bridge) — without this, taffy can't participate in inline layout at all; display: inline-block needs to size itself then hand off to parley
  4. Text overflow / ellipsis in parley — extremely common in real UIs
  5. Text-transform in blitz — easy win, just transform the string before handing to parley
  6. Hyphenation in parley — important for justified text and narrow columns
  7. Decoration styles (wavy/dashed/dotted) — common for spell-check UIs, links
  8. Float exclusions — taffy adjusts max_advance per line and passes to parley
  9. Writing modes — deep work in both parley and taffy, but needed for CJK markets

  The biggest architectural decision is where the inline formatting context lives. Right now there's a gap: taffy handles block/flex/grid and parley handles text shaping + line breaking, but nobody
   owns the IFC that bridges inline boxes, text runs, and line box generation. That bridge layer (whether in taffy, parley, or blitz) is the single most important missing piece.
