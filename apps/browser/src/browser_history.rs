use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use blitz_traits::net::Url;
use dioxus_native::prelude::*;

const MAX_HISTORY_ENTRIES: usize = 1000;

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = SECONDS_PER_MINUTE * 60;
const SECONDS_PER_DAY: u64 = SECONDS_PER_HOUR * 24;

pub type HistoryEntryId = u64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: HistoryEntryId,
    pub url: Url,
    pub title: String,
    pub favicon_url: Option<Url>,
    pub visited_at: SystemTime,
}

impl HistoryEntry {
    pub fn new(url: Url, title: String, favicon_url: Option<Url>) -> Self {
        Self {
            id: next_history_entry_id(),
            url,
            title,
            favicon_url,
            visited_at: SystemTime::now(),
        }
    }
}

fn next_history_entry_id() -> HistoryEntryId {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[derive(Default, Store)]
pub struct BrowsingHistory {
    pub entries: VecDeque<HistoryEntry>,
}

#[store(pub)]
impl<Lens> Store<BrowsingHistory, Lens> {
    fn record_visit(&self, entry: HistoryEntry)
    where
        Lens: Writable,
    {
        record_visit_into(&mut self.entries().write(), entry);
    }

    fn clear(&self)
    where
        Lens: Writable,
    {
        self.entries().write().clear();
    }
}

// Dedupe policy: only consecutive visits to the same URL fold into the head
// entry; non-consecutive revisits are recorded as new entries. This keeps the
// list a faithful chronological log rather than a most-recently-visited set,
// matching the simplest behavior a user can predict at a glance. The id of
// the existing head entry is preserved on fold so its rendered row is not
// remounted.
//
// Lives as a free function so it can be unit-tested without a Dioxus runtime;
// the Store method above is the public surface.
fn record_visit_into(history: &mut VecDeque<HistoryEntry>, entry: HistoryEntry) {
    if let Some(latest) = history.front_mut() {
        if latest.url == entry.url {
            latest.title = entry.title;
            latest.favicon_url = entry.favicon_url;
            latest.visited_at = entry.visited_at;
            return;
        }
    }
    history.push_front(entry);
    if history.len() > MAX_HISTORY_ENTRIES {
        history.truncate(MAX_HISTORY_ENTRIES);
    }
}

// Display helper. Lives here for proximity to the data type but is otherwise
// a UI concern; pass an explicit `now` so the page can drive a periodic
// re-render without each row reading the wall clock independently.
pub fn format_elapsed(visited_at: SystemTime, now: SystemTime) -> String {
    let secs = now
        .duration_since(visited_at)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    if secs < SECONDS_PER_MINUTE {
        "Just now".into()
    } else if secs < SECONDS_PER_HOUR {
        format!("{} min ago", secs / SECONDS_PER_MINUTE)
    } else if secs < SECONDS_PER_DAY {
        format!("{} hr ago", secs / SECONDS_PER_HOUR)
    } else {
        format!("{} days ago", secs / SECONDS_PER_DAY)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn entry(u: &str) -> HistoryEntry {
        HistoryEntry::new(url(u), String::new(), None)
    }

    #[test]
    fn folds_consecutive_same_url_visits() {
        let mut h = VecDeque::new();
        record_visit_into(&mut h, entry("https://a.test/"));
        let id_after_first = h[0].id;
        record_visit_into(&mut h, entry("https://a.test/"));
        assert_eq!(h.len(), 1);
        assert_eq!(
            h[0].id, id_after_first,
            "fold preserves the head entry's id"
        );
    }

    #[test]
    fn keeps_non_consecutive_revisits() {
        let mut h = VecDeque::new();
        record_visit_into(&mut h, entry("https://a.test/"));
        record_visit_into(&mut h, entry("https://b.test/"));
        record_visit_into(&mut h, entry("https://a.test/"));
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn truncates_to_max_entries() {
        let mut h = VecDeque::new();
        for i in 0..(MAX_HISTORY_ENTRIES + 5) {
            record_visit_into(&mut h, entry(&format!("https://a.test/{i}")));
        }
        assert_eq!(h.len(), MAX_HISTORY_ENTRIES);
    }

    #[test]
    fn format_elapsed_buckets() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100_000);
        assert_eq!(format_elapsed(now, now), "Just now");
        assert_eq!(
            format_elapsed(now - Duration::from_secs(120), now),
            "2 min ago"
        );
        assert_eq!(
            format_elapsed(now - Duration::from_secs(7_200), now),
            "2 hr ago"
        );
        assert_eq!(
            format_elapsed(now - Duration::from_secs(2 * 86_400), now),
            "2 days ago"
        );
    }
}
