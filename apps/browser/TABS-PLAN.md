# Plan: Tabs (Multiple Embedded Documents) — Browser UI #363

## Context

The browser app at `apps/browser` currently hosts exactly one document at a time.

**Intended outcome:** A user can open multiple tabs, click between them to switch, close them with an X (except the last), open a new one with a +, and each tab independently retains its document, history (back/forward), and load lifecycle. Hidden tabs keep loading assets and resolving styles in the background.

## Approach

Render **one `web-view` element per tab** in the Dioxus tree, with `display: none` on inactive ones — rather than swapping a single `web-view`'s document. The sub-document lifecycle in blitz-dom already does the right thing under `display: none`:

- **Style resolution + asset fetching continue** for hidden sub-docs (verified: `packages/blitz-dom/src/resolve.rs:95-118` iterates `sub_document_nodes` and calls `sub_doc.resolve()` unconditionally; `flush_styles_to_layout` in `damage.rs` queues images and stylesheets with no `display: none` check).
- **Paint is skipped** for hidden sub-docs (`packages/blitz-paint/src/render.rs:191-193`).

This means hidden tabs preload exactly as desired without any blitz-dom changes for v1. The only caveat is that a `display: none` element has zero layout size, so the hidden sub-doc's viewport is `(0, 0)` until activated; layout settles fast enough on switch that this is acceptable for v1. (Follow-up: optionally seed the hidden viewport with the visible size for pre-warmed layout.)

## Data model

Per-tab state is the minimum that genuinely belongs to a tab. URL-bar display state and the visible window title are derived from the active tab and stay global.

```rust
type TabId = u64;  // monotonic, never reused

struct Tab {
    id: TabId,
    history: SyncStore<History>,                   // existing struct, per-tab
    loader: DocumentLoader,                        // per-tab — see rationale below
    document: Signal<Option<SubDocumentAttr>>,     // each tab's web-view binds to this
}

// In app(): replace the current flat state with
let tabs: Signal<Vec<Tab>> = use_signal(|| vec![Tab::new(home_url.clone())]);
let active_tab_id: Signal<TabId> = use_signal(|| /* first tab's id */);

// Stay global (per @NicoBurns feedback):
let mut url_input_value = use_signal(|| home_url.to_string());  // display state of URL bar
// Window title derives from active_tab().document; not its own signal.
```

**Rationale for these placements:**

- **`url_input_value` global:** Switching tabs overwrites it with the new active tab's current URL. In-progress typing isn't preserved across switches, which matches normal expectations and avoids per-tab signal proliferation.
- **Title global / derived:** Read via `BaseDocument::find_title_node().map(|n| n.text_content())` (existing API at `document.rs:1566-1574`, already used in `blitz-shell/src/window.rs`). The tab strip reads each tab's title the same way; the window title reads the active tab's. No new field needed.
- **`loader` per-tab — recommended:** Each tab loads independently in the background; concurrent loads in different tabs is table-stakes browser behavior. A global loader would have to multiplex across tabs (HashMap keyed by tab id, separate cancel handles, target signal selection) — strictly more complex than N independent loaders. Memory cost is negligible. Open to pushback if you'd rather keep it global; the cost is added wiring around "which load belongs to which tab," not anything fundamental.
- **`document` per-tab:** Required — each tab's `web-view` element binds to its own `SubDocumentAttr`.

## Implementation steps

### Step 1 — Extract `Tab` struct, single-tab path still works

File: `apps/browser/src/main.rs`

Move `history`, the `DocumentLoader` setup, and `content_doc` into a new `Tab` struct with a fresh `id`. Hold `tabs: Signal<Vec<Tab>>` (initialized with one tab) and `active_tab_id: Signal<TabId>`. Add a small helper `active_tab(&tabs, active_id) -> &Tab` (falls back to first tab if id is stale during a close-race).

Rewire chrome handlers (`back_action`, `forward_action`, `refresh_action`, `home_action`, the URL input `onkeydown`, the menu actions) to operate on `active_tab()`. Keep `url_input_value` global; sync its contents to the active tab's current URL whenever `active_tab_id` changes (via a `use_effect`).

After this step the app should behave identically to today, but with the plumbing in place for multiple tabs. Run it and verify before continuing.

### Step 2 — Render one `web-view` per tab

In the `rsx!` for the main frame, iterate `tabs()`:

```rust
for tab in tabs() {
    web-view {
        key: "{tab.id}",
        class: "webview",
        style: if tab.id == active_tab_id() { "display: block" } else { "display: none" },
        "__webview_document": tab.document(),
    }
}
```

Each web-view binds to its own tab's `SubDocumentAttr`. Inactive ones render with `display: none` — style/asset pipelines continue, paint is skipped (per the lifecycle exploration).

Note: confirm Dioxus's diffing keeps each `web-view` stable across renders — using a `key` keyed by `tab.id` ensures it doesn't unmount/remount unrelated tabs.

### Step 3 — Tab strip UI

Above the existing `.urlbar` div, add a `TabStrip` Dioxus component. Each tab renders as:

```rust
div {
    class: if is_active { "tab tab--active" } else { "tab" },
    onclick: move |_| active_tab_id.set(tab.id),
    span { class: "tab__title",
        // Live title via existing API; falls back to URL if absent.
        "{tab_title_or_url(&tab)}"
    }
    if tabs.read().len() > 1 {
        div {
            class: "tab__close",
            onclick: move |evt| { evt.stop_propagation(); close_tab(tab.id); },
            "×"
        }
    }
}
```

Plus a "+" button at the strip's end to call `open_new_tab(home_url.clone())`.

The close `×` is **only rendered when more than one tab is open** — disallowing close-of-last-tab per your preference. This is simpler than the open-fresh-home-tab fallback in the prior draft.

`tab_title_or_url(tab)` reads the live title via `BaseDocument::find_title_node()` on the tab's underlying doc, falling back to a shortened URL.

CSS in `apps/browser/assets/browser.css`: a horizontal flex container, fixed-height row, ellipsis on title overflow, distinct active-tab style. Existing patterns to follow: `.urlbar` (line 14), `.menu-dropdown` (line 90), `.fps-overlay` (line 137).

### Step 4 — Tab lifecycle

Three callbacks at app level:

- **`open_new_tab(url)`** — push a new `Tab`, set `active_tab_id` to it, focus URL input.
- **`close_tab(id)`** — remove from vec; if it was active, switch to the right neighbor (or left if it was the rightmost). The `tabs.len() > 1` guard in the strip ensures this is never called on the last tab.
- **`switch_tab(id)`** — `active_tab_id.set(id)`. The reactive `display: none` on each `web-view` flips visibility; `url_input_value` syncs via the `use_effect` from Step 1.

Closing a tab drops the `Tab`, which drops its `DocumentLoader` (cancels in-flight load via existing logic at `main.rs:538-540`), which drops the `BaseDocument` and all its DOM/style/image/font state (per ownership exploration: dominant cost is `Box<Slab<Node>>`, freed automatically).

### Step 5 — Window title

The existing `blitz-shell/src/window.rs` already reads `find_title_node()` for the window title. Today this is the (sole) document; with tabs, the chrome doc's title is stable ("Browser") and the per-tab title comes from each tab's sub-document. Decide:

- **Simplest:** keep window title as the active tab's title. After a tab switch or a new title load, propagate the active tab's title up to the window. Wire this through whatever existing mechanism `blitz-shell` uses to update the window title.
- **Acceptable v1 alternative:** leave the window title as "Browser" and only show titles in the tab strip. Defer dynamic window title to a follow-up.

Recommend the alternative for v1 — keeps the change purely in `apps/browser` without touching `blitz-shell`.

### Step 6 — Keyboard shortcuts (deferred unless trivial)

`Ctrl+T` (new tab), `Ctrl+W` (close tab), `Ctrl+Tab` / `Ctrl+Shift+Tab` (cycle). If the existing keydown handling in `main.rs` makes this a one-or-two-line add, include it; otherwise defer to a follow-up issue.

## Out of scope (deferred to follow-ups)

| Deferred | Why |
|---|---|
| **Drag-to-reorder tabs** (chrome-tabs / Reardon gist references in #363) | Pure UX polish; mutating `Vec<Tab>` order already works at the data layer. Worth its own issue. |
| **`target=_blank` → open in new tab** | Requires extending `NavigationOptions` (in blitz-dom) to carry target intent and threading it through `BrowserNavProvider`. Cross-crate change, deserves its own issue. The link-click handler at `blitz-dom/src/events/pointer.rs:514-531` doesn't read `target` today. |
| **Pre-warmed viewport for hidden tabs** | Today, hidden tabs have `(0, 0)` layout size because their host element is `display: none`. Resources still fetch (style runs), but layout is degenerate. A small follow-up could explicitly seed the hidden sub-doc viewport with the visible web-view's dimensions. |
| **Tab pinning, right-click menu, thumbnails, audio indicators** | Out of MVP. |
| **Persisting tabs across restarts** | Needs serialization story; not v1. |
| **Memory eviction for backgrounded tabs** | Not needed until users routinely have many tabs open. |
| **Dynamic window title from active tab** | Touches `blitz-shell`; defer per Step 5. |

## Critical files

- `apps/browser/src/main.rs` — primary edits: extract `Tab`, render per-tab `web-view`, add tab strip + lifecycle callbacks. Currently lines 58–467 are the relevant `app()` body and `History` impl.
- `apps/browser/assets/browser.css` — add `.tabstrip`, `.tab`, `.tab--active`, `.tab__close`, `.tab-new` rules. Follow patterns at `.urlbar` (line 14), `.menu-dropdown` (line 90).
- `apps/browser/src/icons.rs` — optional: SVG glyphs for `+` and `×` if we don't want plain text, mirroring `IconButton` (lines 14–34).

## Reusable existing code

- `BaseDocument::find_title_node()` at `packages/blitz-dom/src/document.rs:1566-1574` — title extraction. Existing usage in `blitz-shell/src/window.rs`.
- `History` struct (`apps/browser/src/main.rs:408-467`) — already a self-contained per-browser navigation stack; instantiate one per tab.
- `DocumentLoader` (`apps/browser/src/main.rs:529-618`) — already cancels in-flight loads on new navigation; instantiate one per tab.
- `req_from_string()` (`apps/browser/src/main.rs:370-384`) — URL/search parsing utility; reuse unchanged.
- `IconButton` (`apps/browser/src/icons.rs:14-34`) — reuse for the `+` and `×` glyphs if SVG preferred.
- `SubDocumentAttr` (`packages/dioxus-native-dom/src/sub_document.rs`) — the abstraction the `web-view` element binds to; one instance per tab.

## Verification

1. **Compile & basic sanity:** `cargo run -p browser`. Open the home page; confirm a single tab is visible in the strip and the page renders normally.
2. **Multi-tab open & switch:** Click "+", navigate tab 2 to `https://example.com`. Switch back to tab 1 — verify the previous page is still there at the same scroll position. Switch to tab 2 — same.
3. **Per-tab history:** In tab 1, navigate A → B → C, then go back to B. Switch to tab 2 (own history). Switch back to tab 1; verify it's still on B with forward to C available.
4. **Form state preservation:** In tab 1, type into a form input. Switch tab. Switch back. Input value should still be there (doc was never dropped).
5. **Background loading:** In a slow-loading tab, switch away mid-load. Switch back later — load should have continued and completed in the background.
6. **Close tab:** Close tab 2; tab 1 stays active. The "×" should not appear when only one tab remains, so closing the last tab is impossible.
7. **Loader cancellation:** Start a slow load in tab 1, immediately switch to tab 2 and start a different load. Both proceed independently (per-tab loader). Closing tab 1 mid-load cancels its load without affecting tab 2.
8. **No regressions:** menu still opens/closes, FPS overlay still renders, screenshot capture still works, link clicks still navigate within the active tab.