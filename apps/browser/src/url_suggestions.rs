use std::collections::HashSet;
use std::sync::Arc;

use dioxus_native::prelude::*;
use nucleo::{
    Config, Nucleo,
    pattern::{CaseMatching, Normalization},
};
use tokio::sync::mpsc;

use crate::browser_history::{BrowsingHistory, BrowsingHistoryStoreExt, HistoryEntry};
use crate::tab::Favicon;

const SEARCH_ENGINE_NAME: &str = "DuckDuckGo";
const MAX_HISTORY_SUGGESTIONS: usize = 6;

#[derive(Clone, PartialEq)]
pub enum SuggestionKind {
    /// Use the literal urlbar text — same parse-or-search path as Enter on a bare input.
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

enum WorkerMessage {
    SetQuery(String),
    ReplaceEntries(Vec<HistoryEntry>),
}

#[derive(Clone)]
pub struct UrlSuggester {
    cmd_tx: mpsc::UnboundedSender<WorkerMessage>,
    suggestions: Signal<Vec<Suggestion>>,
}

impl UrlSuggester {
    pub fn suggestions(&self) -> Signal<Vec<Suggestion>> {
        self.suggestions
    }

    pub fn set_query(&self, query: String) {
        let _ = self.cmd_tx.send(WorkerMessage::SetQuery(query));
    }
}

fn new_history_matcher(notify: Arc<dyn Fn() + Send + Sync>) -> Nucleo<HistoryEntry> {
    Nucleo::new(Config::DEFAULT, notify, None, 1)
}

fn haystack_for(entry: &HistoryEntry) -> String {
    let url_str = entry.url.as_str();
    if entry.title.is_empty() {
        url_str.to_owned()
    } else {
        format!("{} {url_str}", entry.title)
    }
}

fn dedup_by_url(entries: &[HistoryEntry]) -> Vec<HistoryEntry> {
    let mut seen: HashSet<String> = HashSet::new();
    entries
        .iter()
        .filter(|e| seen.insert(e.url.to_string()))
        .cloned()
        .collect()
}

fn top_history_entries(snapshot: &nucleo::Snapshot<HistoryEntry>) -> Vec<HistoryEntry> {
    let take = snapshot
        .matched_item_count()
        .min(MAX_HISTORY_SUGGESTIONS as u32);
    snapshot
        .matched_items(0..take)
        .map(|item| item.data.clone())
        .collect()
}

fn assemble_suggestions(query: &str, history_entries: &[HistoryEntry]) -> Vec<Suggestion> {
    if query.is_empty() {
        return vec![];
    }

    let mut result: Vec<Suggestion> = Vec::with_capacity(MAX_HISTORY_SUGGESTIONS + 2);

    result.push(Suggestion {
        kind: SuggestionKind::Literal,
        display_title: query.to_string(),
        display_subtitle: String::new(),
    });

    for entry in history_entries {
        let url_str = entry.url.as_str();
        let display_title = if entry.title.is_empty() {
            url_str.to_owned()
        } else {
            entry.title.clone()
        };
        result.push(Suggestion {
            kind: SuggestionKind::History(Box::new(entry.clone())),
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

fn inject_entries(nucleo: &mut Nucleo<HistoryEntry>, entries: &[HistoryEntry]) {
    let injector = nucleo.injector();
    for entry in dedup_by_url(entries) {
        let haystack = haystack_for(&entry);
        let _ = injector.push(entry, move |_e, cols| {
            cols[0] = haystack.into();
        });
    }
}

/// Drive the matcher loop. Reads commands from `cmd_rx` and calls `publish`
/// whenever a tick produces a new snapshot. Exits when `cmd_rx` closes.
async fn run_worker(
    mut cmd_rx: mpsc::UnboundedReceiver<WorkerMessage>,
    mut publish: impl FnMut(Vec<Suggestion>) + 'static,
) {
    let notify = Arc::new(tokio::sync::Notify::new());
    let notify_fn: Arc<dyn Fn() + Send + Sync> = {
        let notify = notify.clone();
        Arc::new(move || notify.notify_one())
    };
    let mut nucleo: Nucleo<HistoryEntry> = new_history_matcher(notify_fn);
    let mut last_query = String::new();

    loop {
        tokio::select! {
            _ = notify.notified() => {}
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(WorkerMessage::SetQuery(new_query)) => {
                        let append = new_query.starts_with(&last_query);
                        nucleo.pattern.reparse(
                            0,
                            &new_query,
                            CaseMatching::Ignore,
                            Normalization::Smart,
                            append,
                        );
                        last_query = new_query;
                    }
                    Some(WorkerMessage::ReplaceEntries(entries)) => {
                        nucleo.restart(true);
                        inject_entries(&mut nucleo, &entries);
                        if !last_query.is_empty() {
                            nucleo.pattern.reparse(
                                0,
                                &last_query,
                                CaseMatching::Ignore,
                                Normalization::Smart,
                                false,
                            );
                        }
                    }
                    None => break,
                }
            }
        }

        let status = nucleo.tick(10);
        if status.changed {
            let entries = top_history_entries(nucleo.snapshot());
            publish(assemble_suggestions(&last_query, &entries));
        }
        if status.running {
            notify.notify_one();
        }
    }
}

pub fn provide_url_suggester(browsing_history: Store<BrowsingHistory>) {
    let mut suggestions: Signal<Vec<Suggestion>> = use_signal(Vec::new);

    let cmd_tx = use_hook(|| {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<WorkerMessage>();
        spawn(async move {
            run_worker(cmd_rx, move |s| suggestions.set(s)).await;
        });
        cmd_tx
    });

    use_context_provider(|| UrlSuggester {
        cmd_tx: cmd_tx.clone(),
        suggestions,
    });

    let cmd_tx_for_effect = cmd_tx.clone();
    use_effect(move || {
        let entries: Vec<HistoryEntry> =
            browsing_history.entries().read().iter().cloned().collect();
        let _ = cmd_tx_for_effect.send(WorkerMessage::ReplaceEntries(entries));
    });
}

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
    // Order is fixed: optional Literal row, then history rows, then a Search row.
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
            onpointerdown: move |evt| {
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
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::time::Duration;

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

    /// Run the worker against a scripted sequence of messages and return every
    /// publication it emitted, in order. The harness drops `cmd_tx` after
    /// queueing the messages so the worker exits once it has drained them.
    async fn drive_worker(messages: Vec<WorkerMessage>) -> Vec<Vec<Suggestion>> {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<WorkerMessage>();
        for m in messages {
            cmd_tx.send(m).unwrap();
        }
        drop(cmd_tx);

        let captured: Arc<Mutex<Vec<Vec<Suggestion>>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_for_publish = captured.clone();
        let publish = move |s: Vec<Suggestion>| {
            captured_for_publish.lock().unwrap().push(s);
        };

        // Bound test runtime in case the worker ever fails to terminate on
        // channel close — without a timeout a regression would hang CI.
        let _ = tokio::time::timeout(Duration::from_secs(2), run_worker(cmd_rx, publish)).await;

        Arc::try_unwrap(captured)
            .unwrap_or_else(|_| panic!("publish closure outlived run_worker"))
            .into_inner()
            .unwrap()
    }

    /// Synchronous nucleo driver for tests that only care about the assembled
    /// output for a single (entries, query) pair. Skips the async worker and
    /// channel plumbing — we have separate `#[tokio::test]`s for those.
    fn build_suggestions_sync(query: &str, recent: &VecDeque<HistoryEntry>) -> Vec<Suggestion> {
        if query.is_empty() {
            return assemble_suggestions(query, &[]);
        }
        let entries: Vec<HistoryEntry> = recent.iter().cloned().collect();
        let notify_fn: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        let mut nucleo = new_history_matcher(notify_fn);
        inject_entries(&mut nucleo, &entries);
        nucleo
            .pattern
            .reparse(0, query, CaseMatching::Ignore, Normalization::Smart, false);
        while nucleo.tick(50).running {}
        assemble_suggestions(query, &top_history_entries(nucleo.snapshot()))
    }

    fn count_history(suggestions: &[Suggestion]) -> usize {
        suggestions
            .iter()
            .filter(|s| matches!(s.kind, SuggestionKind::History(_)))
            .count()
    }

    #[test]
    fn empty_query_returns_empty() {
        let h = history(vec![entry("https://example.com/")]);
        assert!(build_suggestions_sync("", &h).is_empty());
    }

    #[test]
    fn literal_row_is_first() {
        let h = history(vec![]);
        let suggestions = build_suggestions_sync("rust", &h);
        assert!(matches!(suggestions[0].kind, SuggestionKind::Literal));
        assert_eq!(suggestions[0].display_title, "rust");
    }

    #[test]
    fn no_history_returns_literal_and_search_row() {
        let h = history(vec![]);
        let suggestions = build_suggestions_sync("rust", &h);
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
        let suggestions = build_suggestions_sync("RUST", &h);
        // 1 literal + 1 history + 1 search
        assert_eq!(suggestions.len(), 3);
        assert!(matches!(suggestions[1].kind, SuggestionKind::History(_)));
    }

    #[test]
    fn matches_url_host_substring() {
        let h = history(vec![entry("https://rust-lang.org/")]);
        let suggestions = build_suggestions_sync("rust", &h);
        assert_eq!(suggestions.len(), 3);
        assert!(matches!(suggestions[1].kind, SuggestionKind::History(_)));
    }

    #[test]
    fn matches_url_path_substring() {
        let h = history(vec![entry("https://example.com/rustacean")]);
        let suggestions = build_suggestions_sync("rustacean", &h);
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
        let suggestions = build_suggestions_sync("rust", &h);
        assert_eq!(count_history(&suggestions), 6);
        // Total: 1 literal + 6 history + 1 search
        assert_eq!(suggestions.len(), 8);
    }

    #[test]
    fn dedup_keeps_first_occurrence() {
        // VecDeque iterates front-to-back; front is most recent
        let mut h = VecDeque::new();
        h.push_back(entry_with_title("https://example.com/", "old rust"));
        h.push_front(entry_with_title("https://example.com/", "new rust"));
        let suggestions = build_suggestions_sync("rust", &h);
        let history_rows: Vec<_> = suggestions
            .iter()
            .filter(|s| matches!(s.kind, SuggestionKind::History(_)))
            .collect();
        assert_eq!(history_rows.len(), 1, "duplicate URL deduped to one row");
        assert_eq!(
            history_rows[0].display_title, "new rust",
            "keeps front-of-deque entry"
        );
    }

    #[test]
    fn search_row_is_always_last() {
        let h = history(vec![entry_with_title("https://rust-lang.org/", "Rust")]);
        let suggestions = build_suggestions_sync("rust", &h);
        assert!(!suggestions.is_empty());
        assert!(matches!(
            suggestions.last().unwrap().kind,
            SuggestionKind::Search
        ));
    }

    #[test]
    fn search_row_display_title_contains_query() {
        let h = history(vec![]);
        let suggestions = build_suggestions_sync("hello world", &h);
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
        let suggestions = build_suggestions_sync("rstlng", &h);
        assert_eq!(count_history(&suggestions), 1);
    }

    #[test]
    fn ranks_better_match_above_weaker_match() {
        // A strong contiguous match outranks a weaker subsequence match
        // regardless of input order — fuzzy ranking is global, not recency.
        let h = history(vec![
            entry_with_title("https://r.example.com/u/s/t", "Other"),
            entry_with_title("https://rust-lang.org/", "The Rust Programming Language"),
        ]);
        let suggestions = build_suggestions_sync("rust", &h);
        let history_rows: Vec<_> = suggestions
            .iter()
            .filter(|s| matches!(s.kind, SuggestionKind::History(_)))
            .collect();
        assert_eq!(history_rows.len(), 2);
        assert_eq!(history_rows[0].display_subtitle, "https://rust-lang.org/");
    }

    // Worker-level tests: drive the spawned task directly to cover the
    // command-handling state machine that the sync helper above doesn't
    // exercise (multi-query sequences, query-then-replace ordering).

    #[tokio::test]
    async fn worker_publishes_filtered_results_after_set_query() {
        let entries = vec![
            entry_with_title("https://rust-lang.org/", "Rust"),
            entry_with_title("https://golang.org/", "Go"),
        ];
        let publications = drive_worker(vec![
            WorkerMessage::ReplaceEntries(entries),
            WorkerMessage::SetQuery("rust".into()),
        ])
        .await;
        let last = publications.last().expect("worker published at least once");
        assert_eq!(count_history(last), 1);
        let history_row = last
            .iter()
            .find(|s| matches!(s.kind, SuggestionKind::History(_)))
            .unwrap();
        assert_eq!(history_row.display_subtitle, "https://rust-lang.org/");
    }

    #[tokio::test]
    async fn worker_narrows_results_when_query_extends() {
        // The append-mode reparse in nucleo is opportunistic; results must
        // still be correct (a strict subset of the prior match set).
        let entries = vec![
            entry_with_title("https://rust-lang.org/", "Rust"),
            entry_with_title("https://ruby-lang.org/", "Ruby"),
        ];
        let publications = drive_worker(vec![
            WorkerMessage::ReplaceEntries(entries),
            WorkerMessage::SetQuery("ru".into()),
            WorkerMessage::SetQuery("rus".into()),
            WorkerMessage::SetQuery("rust".into()),
        ])
        .await;
        let last = publications.last().expect("worker published at least once");
        let history_urls: Vec<_> = last
            .iter()
            .filter_map(|s| match &s.kind {
                SuggestionKind::History(e) => Some(e.url.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(history_urls, vec!["https://rust-lang.org/".to_string()]);
    }

    #[tokio::test]
    async fn worker_reapplies_query_after_replace_entries() {
        // SetQuery before ReplaceEntries: the worker must re-apply the pattern
        // to the freshly injected items, not lose query state on restart.
        let publications = drive_worker(vec![
            WorkerMessage::SetQuery("rust".into()),
            WorkerMessage::ReplaceEntries(vec![
                entry_with_title("https://rust-lang.org/", "Rust"),
                entry_with_title("https://golang.org/", "Go"),
            ]),
        ])
        .await;
        let last = publications.last().expect("worker published at least once");
        assert_eq!(count_history(last), 1);
        let history_row = last
            .iter()
            .find(|s| matches!(s.kind, SuggestionKind::History(_)))
            .unwrap();
        assert_eq!(history_row.display_subtitle, "https://rust-lang.org/");
    }

    #[tokio::test]
    async fn worker_publishes_empty_for_empty_query() {
        let entries = vec![entry_with_title("https://rust-lang.org/", "Rust")];
        let publications = drive_worker(vec![
            WorkerMessage::ReplaceEntries(entries),
            WorkerMessage::SetQuery("rust".into()),
            WorkerMessage::SetQuery(String::new()),
        ])
        .await;
        let last = publications.last().expect("worker published at least once");
        assert!(last.is_empty(), "empty query produces no suggestions");
    }
}
