use std::collections::{HashSet, VecDeque};

use dioxus_native::prelude::*;
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::browser_history::HistoryEntry;
use crate::tab::Favicon;

/// Display name shown in the urlbar's "Search with …" suggestion row.
const SEARCH_ENGINE_NAME: &str = "DuckDuckGo";

#[derive(Clone, PartialEq)]
pub enum SuggestionKind {
    /// Use the literal urlbar text. Picking this row runs the same
    /// parse-or-search path as pressing Enter on a bare input, giving users a
    /// way back to "what Enter would do" once they've started moving through
    /// the list with the arrow keys.
    Literal,
    // Boxed so an empty-variant Suggestion (Literal, Search) doesn't pay the
    // worst-case enum size set by HistoryEntry.
    History(Box<HistoryEntry>),
    Search,
}

#[derive(Clone, PartialEq)]
pub struct Suggestion {
    pub kind: SuggestionKind,
    display_title: String,
    display_subtitle: String,
}

/// Build suggestion rows for `query` against the most-recent `recent` entries.
///
/// Returns an empty vec for an empty query.  Otherwise returns one Literal row
/// (the "what Enter would do" action), followed by up to 6 history rows
/// (deduped by URL, ranked by fuzzy-match score against `title + url`, with
/// recency as the tiebreak), followed by exactly one Search row.
pub fn build_suggestions(query: &str, recent: &VecDeque<HistoryEntry>) -> Vec<Suggestion> {
    if query.is_empty() {
        return vec![];
    }

    let mut result: Vec<Suggestion> = Vec::with_capacity(MAX_HISTORY_SUGGESTIONS + 2);

    result.push(Suggestion {
        kind: SuggestionKind::Literal,
        display_title: query.to_string(),
        display_subtitle: String::new(),
    });

    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);

    // Score every unique URL; pick the top MAX_HISTORY_SUGGESTIONS afterward.
    // We have to look at all entries (up to ~1000) rather than stopping at the
    // first 6, since fuzzy ranking is global rather than recency-prefix.
    let mut scored: Vec<(u32, usize, &HistoryEntry)> = Vec::new();
    let mut seen_urls: HashSet<&str> = HashSet::new();
    let mut haystack_buf = Vec::new();

    for (recency_idx, entry) in recent.iter().enumerate() {
        let url_str = entry.url.as_str();
        if !seen_urls.insert(url_str) {
            continue;
        }
        // Concatenate the user-visible fields into one haystack. Fuzzy matching
        // finds subsequences across the whole string, so a query like "rust"
        // matches whether it appears in the title, host, or path.
        let haystack_string = if entry.title.is_empty() {
            url_str.to_owned()
        } else {
            format!("{} {url_str}", entry.title)
        };
        let haystack = Utf32Str::new(&haystack_string, &mut haystack_buf);
        if let Some(score) = pattern.score(haystack, &mut matcher) {
            scored.push((score, recency_idx, entry));
        }
    }

    // Higher score wins; ties go to the more-recent entry (lower recency_idx).
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.truncate(MAX_HISTORY_SUGGESTIONS);

    for (_, _, entry) in &scored {
        let url_str = entry.url.as_str();
        let display_title = if entry.title.is_empty() {
            url_str.to_owned()
        } else {
            entry.title.clone()
        };
        result.push(Suggestion {
            kind: SuggestionKind::History(Box::new((*entry).clone())),
            display_title,
            display_subtitle: url_str.to_owned(),
        });
    }

    result.push(Suggestion {
        kind: SuggestionKind::Search,
        display_title: format!("Search with {SEARCH_ENGINE_NAME}: {query}"),
        display_subtitle: String::new(),
    });
    result
}

const MAX_HISTORY_SUGGESTIONS: usize = 6;

/// Autocomplete dropdown anchored below the urlbar input.
///
/// `selected_idx` indexes into the flat suggestion vec (0-based). Section
/// headers are visual only and are not counted in the index. Hover only
/// produces a visual highlight (via the CSS `:hover` rule) — the
/// keyboard-selected row stays selected. Selection state itself is owned by
/// the parent, which receives picks via `on_pick`.
#[component]
pub fn UrlSuggestions(
    suggestions: ReadSignal<Vec<Suggestion>>,
    selected_idx: ReadSignal<Option<usize>>,
    on_pick: Callback<Suggestion>,
) -> Element {
    let items = suggestions.read();
    // Order is fixed: optional Literal row, then history rows, then a Search
    // row. Find the section boundaries once.
    let history_start = if items
        .first()
        .is_some_and(|s| matches!(s.kind, SuggestionKind::Literal))
    {
        1
    } else {
        0
    };
    let search_start = items
        .iter()
        .position(|s| matches!(s.kind, SuggestionKind::Search))
        .unwrap_or(items.len());
    let literal_slice = &items[..history_start];
    let history = &items[history_start..search_start];
    let search = &items[search_start..];
    let selected_idx = selected_idx();

    rsx! {
        div { class: "urlbar-suggestions",
            for (idx, suggestion) in literal_slice.iter().enumerate() {
                SuggestionRow {
                    idx,
                    suggestion: suggestion.clone(),
                    is_selected: selected_idx == Some(idx),
                    on_pick,
                }
            }
            if !history.is_empty() {
                div { class: "suggestion-section-header", "History" }
                for (offset, suggestion) in history.iter().enumerate() {
                    SuggestionRow {
                        idx: history_start + offset,
                        suggestion: suggestion.clone(),
                        is_selected: selected_idx == Some(history_start + offset),
                        on_pick,
                    }
                }
            }
            if !search.is_empty() {
                div { class: "suggestion-section-header", "Search Suggestions" }
                for (offset, suggestion) in search.iter().enumerate() {
                    SuggestionRow {
                        idx: search_start + offset,
                        suggestion: suggestion.clone(),
                        is_selected: selected_idx == Some(search_start + offset),
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
    on_pick: Callback<Suggestion>,
) -> Element {
    let row_class = if is_selected {
        "suggestion-row selected"
    } else {
        "suggestion-row"
    };
    let (favicon_url, show_subtitle) = match &suggestion.kind {
        SuggestionKind::History(entry) => (entry.favicon_url.clone(), true),
        SuggestionKind::Literal | SuggestionKind::Search => (None, false),
    };
    let pick = suggestion.clone();
    rsx! {
        div {
            class: row_class,
            "data-idx": "{idx}",
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
    fn literal_row_is_first() {
        let h = history(vec![]);
        let suggestions = build_suggestions("rust", &h);
        assert!(matches!(suggestions[0].kind, SuggestionKind::Literal));
        assert_eq!(suggestions[0].display_title, "rust");
    }

    #[test]
    fn no_history_returns_literal_and_search_row() {
        let h = history(vec![]);
        let suggestions = build_suggestions("rust", &h);
        // 1 literal + 1 search
        assert_eq!(suggestions.len(), 2);
        assert!(matches!(suggestions[0].kind, SuggestionKind::Literal));
        assert!(matches!(suggestions[1].kind, SuggestionKind::Search));
    }

    #[test]
    fn matches_title_substring_case_insensitive() {
        let h = history(vec![entry_with_title(
            "https://a.test/",
            "Rust Programming",
        )]);
        let suggestions = build_suggestions("RUST", &h);
        // 1 literal + 1 history + 1 search
        assert_eq!(suggestions.len(), 3);
        assert!(matches!(suggestions[1].kind, SuggestionKind::History(_)));
    }

    #[test]
    fn matches_url_host_substring() {
        let h = history(vec![entry("https://rust-lang.org/")]);
        let suggestions = build_suggestions("rust", &h);
        assert_eq!(suggestions.len(), 3);
        assert!(matches!(suggestions[1].kind, SuggestionKind::History(_)));
    }

    #[test]
    fn matches_url_path_substring() {
        let h = history(vec![entry("https://example.com/rustacean")]);
        let suggestions = build_suggestions("rustacean", &h);
        assert_eq!(suggestions.len(), 3);
        assert!(matches!(suggestions[1].kind, SuggestionKind::History(_)));
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
        // Total: 1 literal + 6 history + 1 search
        assert_eq!(suggestions.len(), 8);
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
        // Literal row is at [0]; Search row is the last row.
        let search = suggestions.last().expect("at least one row");
        assert!(matches!(search.kind, SuggestionKind::Search));
        assert_eq!(
            search.display_title,
            format!("Search with {SEARCH_ENGINE_NAME}: hello world")
        );
    }

    #[test]
    fn fuzzy_matches_subsequence_not_just_substring() {
        // "rstlng" is not a substring of any field, but is a subsequence of
        // "rust-lang.org" — fuzzy matching should still surface it.
        let h = history(vec![entry_with_title("https://rust-lang.org/", "Rust")]);
        let suggestions = build_suggestions("rstlng", &h);
        let history_count = suggestions
            .iter()
            .filter(|s| matches!(s.kind, SuggestionKind::History(_)))
            .count();
        assert_eq!(history_count, 1);
    }

    #[test]
    fn ranks_better_match_above_recent_weaker_match() {
        // Most-recent entry only matches weakly; an older entry is a strong
        // contiguous match. Ranking should put the strong match first.
        let mut h = VecDeque::new();
        h.push_back(entry_with_title(
            "https://rust-lang.org/",
            "The Rust Programming Language",
        )); // older, strong match
        h.push_front(entry_with_title("https://r.example.com/u/s/t", "Other")); // newer, weak match
        let suggestions = build_suggestions("rust", &h);
        let history_rows: Vec<_> = suggestions
            .iter()
            .filter(|s| matches!(s.kind, SuggestionKind::History(_)))
            .collect();
        assert_eq!(history_rows.len(), 2);
        assert_eq!(history_rows[0].display_subtitle, "https://rust-lang.org/");
    }
}
