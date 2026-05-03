use std::collections::VecDeque;

use dioxus_native::prelude::*;

use crate::browser_history::HistoryEntry;
use crate::tab::Favicon;

/// Display name shown in the urlbar's "Search with …" suggestion row.
const SEARCH_ENGINE_NAME: &str = "DuckDuckGo";

#[derive(Clone, PartialEq)]
pub enum SuggestionKind {
    // Boxed because HistoryEntry is ~224B and the other variant is empty —
    // unboxed, every Suggestion (including Search rows) pays the worst-case
    // size. Cheap to box: at most ~7 entries per build_suggestions call.
    History(Box<HistoryEntry>),
    Search,
}

#[derive(Clone, PartialEq)]
pub struct Suggestion {
    pub kind: SuggestionKind,
    pub display_title: String,
    pub display_subtitle: String,
}

/// Build suggestion rows for `query` against the most-recent `recent` entries.
///
/// Returns an empty vec for an empty query.  Otherwise returns up to 6 history
/// rows (deduped by URL, most-recent first) followed by exactly one Search row.
pub fn build_suggestions(query: &str, recent: &VecDeque<HistoryEntry>) -> Vec<Suggestion> {
    if query.is_empty() {
        return vec![];
    }

    // Lowercase the needle once. Matching against entries uses
    // `contains_ignore_ascii_case` so we don't allocate per-entry — at up to
    // ~1000 history rows, three String allocs per entry per keystroke adds up.
    let q_lower = query.to_ascii_lowercase();
    let mut history_rows: Vec<Suggestion> = Vec::with_capacity(MAX_HISTORY_SUGGESTIONS);

    for entry in recent {
        if history_rows.len() >= MAX_HISTORY_SUGGESTIONS {
            break;
        }
        let url_str = entry.url.as_str();
        // Linear scan: capped at MAX_HISTORY_SUGGESTIONS, so faster than a
        // HashSet and avoids the per-key String allocation.
        if history_rows.iter().any(|s| s.display_subtitle == url_str) {
            continue;
        }
        let host = entry.url.host_str().unwrap_or("");
        let path = entry.url.path();
        if contains_ignore_ascii_case(host, &q_lower)
            || contains_ignore_ascii_case(path, &q_lower)
            || contains_ignore_ascii_case(&entry.title, &q_lower)
        {
            let display_title = if entry.title.is_empty() {
                url_str.to_owned()
            } else {
                entry.title.clone()
            };
            history_rows.push(Suggestion {
                kind: SuggestionKind::History(Box::new(entry.clone())),
                display_title,
                display_subtitle: url_str.to_owned(),
            });
        }
    }

    let mut result = history_rows;
    result.push(Suggestion {
        kind: SuggestionKind::Search,
        display_title: format!("Search with {SEARCH_ENGINE_NAME}: {query}"),
        display_subtitle: String::new(),
    });
    result
}

const MAX_HISTORY_SUGGESTIONS: usize = 6;

// Allocation-free case-insensitive substring check. Folds case for ASCII only,
// which is fine for URL/host/path matching and good enough for titles in a
// search box. `needle` must already be lowercased.
//
// Operates on raw bytes of the UTF-8 string. This is safe because
// `eq_ignore_ascii_case` only swaps ASCII A-Z/a-z; multi-byte UTF-8 sequences
// (whose bytes are all >= 0x80) compare bytewise unchanged, so the result is
// the same as a Unicode-aware byte-substring search — just without the cost of
// per-entry allocation.
fn contains_ignore_ascii_case(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    let h = haystack.as_bytes();
    let n = needle_lower.as_bytes();
    if h.len() < n.len() {
        return false;
    }
    h.windows(n.len())
        .any(|w| w.iter().zip(n).all(|(a, b)| a.eq_ignore_ascii_case(b)))
}

/// Autocomplete dropdown anchored below the urlbar input.
///
/// `selected_idx` indexes into the flat suggestion vec (0-based). Section
/// headers are visual only and are not counted in the index. Selection state
/// itself is owned by the parent: the parent decides what to do with hover
/// and pick events via `on_hover` and `on_pick`.
#[component]
pub fn UrlSuggestions(
    suggestions: ReadSignal<Vec<Suggestion>>,
    selected_idx: ReadSignal<Option<usize>>,
    on_hover: Callback<Option<usize>>,
    on_pick: Callback<Suggestion>,
) -> Element {
    let items = suggestions.read();
    // `build_suggestions` always emits history rows first, then search rows.
    // Find the boundary once instead of filtering twice.
    let split = items
        .iter()
        .position(|s| matches!(s.kind, SuggestionKind::Search))
        .unwrap_or(items.len());
    let (history, search) = items.split_at(split);
    let sel = selected_idx();

    rsx! {
        div { class: "urlbar-suggestions",
            if !history.is_empty() {
                div { class: "suggestion-section-header", "History" }
                for (idx, suggestion) in history.iter().enumerate() {
                    SuggestionRow {
                        idx,
                        suggestion: suggestion.clone(),
                        is_selected: sel == Some(idx),
                        on_hover,
                        on_pick,
                    }
                }
            }
            if !search.is_empty() {
                div { class: "suggestion-section-header", "Search Suggestions" }
                for (offset, suggestion) in search.iter().enumerate() {
                    SuggestionRow {
                        idx: split + offset,
                        suggestion: suggestion.clone(),
                        is_selected: sel == Some(split + offset),
                        on_hover,
                        on_pick,
                    }
                }
            }
        }
    }
}

// Use `onmousedown` (not `onclick`) for picks so `prevent_default` suppresses
// the blur on the urlbar input before the pick fires.
#[component]
fn SuggestionRow(
    idx: usize,
    suggestion: Suggestion,
    is_selected: bool,
    on_hover: Callback<Option<usize>>,
    on_pick: Callback<Suggestion>,
) -> Element {
    let row_class = if is_selected {
        "suggestion-row selected"
    } else {
        "suggestion-row"
    };
    let (favicon_url, show_subtitle) = match &suggestion.kind {
        SuggestionKind::History(entry) => (entry.favicon_url.clone(), true),
        SuggestionKind::Search => (None, false),
    };
    let pick = suggestion.clone();
    rsx! {
        div {
            class: row_class,
            "data-idx": "{idx}",
            onmouseenter: move |_| on_hover.call(Some(idx)),
            onmousedown: move |evt| {
                evt.prevent_default();
                on_pick.call(pick.clone());
            },
            if matches!(suggestion.kind, SuggestionKind::History(_)) {
                Favicon { url: favicon_url, class: "suggestion-favicon" }
            }
            span { class: "suggestion-title", "{suggestion.display_title}" }
            if show_subtitle {
                span { class: "suggestion-url", "{suggestion.display_subtitle}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blitz_traits::net::Url;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn entry_with_title(u: &str, title: &str) -> HistoryEntry {
        HistoryEntry::new(url(u), title.to_string(), None)
    }

    fn entry(u: &str) -> HistoryEntry {
        entry_with_title(u, "")
    }

    fn history(entries: Vec<HistoryEntry>) -> VecDeque<HistoryEntry> {
        VecDeque::from(entries)
    }

    #[test]
    fn empty_query_returns_empty() {
        let h = history(vec![entry("https://example.com/")]);
        assert!(build_suggestions("", &h).is_empty());
    }

    #[test]
    fn no_history_returns_one_search_row() {
        let h = history(vec![]);
        let suggestions = build_suggestions("rust", &h);
        assert_eq!(suggestions.len(), 1);
        assert!(matches!(suggestions[0].kind, SuggestionKind::Search));
    }

    #[test]
    fn matches_title_substring_case_insensitive() {
        let h = history(vec![entry_with_title(
            "https://a.test/",
            "Rust Programming",
        )]);
        let suggestions = build_suggestions("RUST", &h);
        assert_eq!(suggestions.len(), 2); // 1 history + 1 search
        assert!(matches!(suggestions[0].kind, SuggestionKind::History(_)));
    }

    #[test]
    fn matches_url_host_substring() {
        let h = history(vec![entry("https://rust-lang.org/")]);
        let suggestions = build_suggestions("rust", &h);
        assert_eq!(suggestions.len(), 2);
        assert!(matches!(suggestions[0].kind, SuggestionKind::History(_)));
    }

    #[test]
    fn matches_url_path_substring() {
        let h = history(vec![entry("https://example.com/rustacean")]);
        let suggestions = build_suggestions("rustacean", &h);
        assert_eq!(suggestions.len(), 2);
        assert!(matches!(suggestions[0].kind, SuggestionKind::History(_)));
    }

    #[test]
    fn caps_at_six_history_rows() {
        let h = history(
            (0..10)
                .map(|i| entry_with_title(&format!("https://example.com/page{i}"), "rust"))
                .collect(),
        );
        let suggestions = build_suggestions("rust", &h);
        let history_count = suggestions
            .iter()
            .filter(|s| matches!(s.kind, SuggestionKind::History(_)))
            .count();
        assert_eq!(history_count, 6);
        // Total: 6 history + 1 search
        assert_eq!(suggestions.len(), 7);
    }

    #[test]
    fn dedup_keeps_most_recent_url() {
        // VecDeque iterates front-to-back; front is most recent
        let mut h = VecDeque::new();
        h.push_back(entry_with_title("https://example.com/", "old rust")); // less recent (back)
        h.push_front(entry_with_title("https://example.com/", "new rust")); // most recent (front)
        let suggestions = build_suggestions("rust", &h);
        let history_rows: Vec<_> = suggestions
            .iter()
            .filter(|s| matches!(s.kind, SuggestionKind::History(_)))
            .collect();
        assert_eq!(history_rows.len(), 1, "duplicate URL deduped to one row");
        assert_eq!(
            history_rows[0].display_title, "new rust",
            "keeps most recent"
        );
    }

    #[test]
    fn search_row_is_always_last() {
        let h = history(vec![entry_with_title("https://rust-lang.org/", "Rust")]);
        let suggestions = build_suggestions("rust", &h);
        assert!(!suggestions.is_empty());
        assert!(matches!(
            suggestions.last().unwrap().kind,
            SuggestionKind::Search
        ));
    }

    #[test]
    fn search_row_display_title_contains_query() {
        let h = history(vec![]);
        let suggestions = build_suggestions("hello world", &h);
        assert_eq!(
            suggestions[0].display_title,
            format!("Search with {SEARCH_ENGINE_NAME}: hello world")
        );
    }
}
