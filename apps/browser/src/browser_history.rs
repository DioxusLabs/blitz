use std::collections::VecDeque;
use std::time::{Duration, SystemTime};

use blitz_traits::net::Url;
use dioxus_native::prelude::*;

pub use browser_persistence::{HistoryEntry, HistoryEntryId, HistoryStore, MAX_HISTORY_ENTRIES};

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = SECONDS_PER_MINUTE * 60;
const SECONDS_PER_DAY: u64 = SECONDS_PER_HOUR * 24;

#[derive(Default, Store)]
pub struct BrowsingHistory {
    pub entries: VecDeque<HistoryEntry>,
}

impl BrowsingHistory {
    /// Build from a pre-sorted list of persisted entries (visited_at DESC order).
    pub fn from_entries(entries: Vec<HistoryEntry>) -> Self {
        Self {
            entries: VecDeque::from(entries),
        }
    }
}

#[store(pub)]
impl<Lens> Store<BrowsingHistory, Lens> {
    fn record_visit(&self, entry: HistoryEntry) -> HistoryEntryId
    where
        Lens: Writable,
    {
        record_visit_inner(&mut self.entries().write(), entry)
    }

    fn set_favicon_by_id(&self, id: HistoryEntryId, favicon_url: Url)
    where
        Lens: Writable,
    {
        set_favicon_by_id_inner(&mut self.entries().write(), id, favicon_url);
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
fn record_visit_inner(history: &mut VecDeque<HistoryEntry>, entry: HistoryEntry) -> HistoryEntryId {
    if let Some(latest) = history.front_mut() {
        if latest.url == entry.url {
            latest.title = entry.title;
            latest.favicon_url = entry.favicon_url;
            latest.visited_at = entry.visited_at;
            return latest.id;
        }
    }
    let id = entry.id;
    history.push_front(entry);
    if history.len() > MAX_HISTORY_ENTRIES {
        history.truncate(MAX_HISTORY_ENTRIES);
    }
    id
}

// No-ops if the entry has aged out before the background favicon probe
// finished — the visit has already been pushed past the cap.
fn set_favicon_by_id_inner(
    history: &mut VecDeque<HistoryEntry>,
    id: HistoryEntryId,
    favicon_url: Url,
) {
    if let Some(entry) = history.iter_mut().find(|e| e.id == id) {
        entry.favicon_url = Some(favicon_url);
    }
}

// `now` is injected so the page can drive a periodic re-render without each
// row reading the wall clock independently.
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

// Single entry point for visit/favicon writes. Owns the in-memory `Store` and
// the on-disk `HistoryStore` together so callers don't have to write to both,
// and don't have to know that the two sides key favicons differently.
#[derive(Clone)]
pub struct HistoryService {
    browsing: Store<BrowsingHistory>,
    disk: HistoryStore,
}

impl HistoryService {
    pub fn new(browsing: Store<BrowsingHistory>, disk: HistoryStore) -> Self {
        Self { browsing, disk }
    }

    /// In-memory store, for read paths that need a reactive `Store` handle.
    pub fn browsing(&self) -> Store<BrowsingHistory> {
        self.browsing
    }

    pub fn record_visit(&self, entry: HistoryEntry) -> HistoryEntryId {
        let id = self.browsing.record_visit(entry.clone());
        let disk = self.disk.clone();
        dispatch_disk_write(move || disk.record_visit(&entry));
        id
    }

    /// Favicon resolution lands on both stores at once. The in-memory side is
    /// patched by id (so the specific row that triggered the probe gets the
    /// icon); the on-disk side is patched by URL (so every still-NULL row for
    /// that URL catches up).
    pub fn set_favicon(&self, id: HistoryEntryId, page_url: Url, favicon_url: Url) {
        self.browsing.set_favicon_by_id(id, favicon_url.clone());
        let disk = self.disk.clone();
        dispatch_disk_write(move || disk.set_favicon_by_url(&page_url, &favicon_url));
    }

    pub fn clear(&self) {
        self.browsing.clear();
        let disk = self.disk.clone();
        dispatch_disk_write(move || disk.clear());
    }
}

// Hop sync sqlite work off the calling thread when a tokio runtime is bound.
// If no runtime is bound we drop the write rather than blocking the caller —
// history is best-effort, and silently running sync I/O on the UI thread
// would mask the bug. `debug_assert!` surfaces it in development.
fn dispatch_disk_write(f: impl FnOnce() + Send + 'static) {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn_blocking(f);
        }
        Err(_) => {
            debug_assert!(
                false,
                "history: disk write dispatched outside a tokio runtime"
            );
            tracing::warn!("history: no tokio runtime; dropping disk write");
        }
    }
}

#[cfg(test)]
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
        record_visit_inner(&mut h, entry("https://a.test/"));
        let id_after_first = h[0].id;
        record_visit_inner(&mut h, entry("https://a.test/"));
        assert_eq!(h.len(), 1);
        assert_eq!(
            h[0].id, id_after_first,
            "fold preserves the head entry's id"
        );
    }

    #[test]
    fn keeps_non_consecutive_revisits() {
        let mut h = VecDeque::new();
        record_visit_inner(&mut h, entry("https://a.test/"));
        record_visit_inner(&mut h, entry("https://b.test/"));
        record_visit_inner(&mut h, entry("https://a.test/"));
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn truncates_to_max_entries() {
        let mut h = VecDeque::new();
        for i in 0..(MAX_HISTORY_ENTRIES + 5) {
            record_visit_inner(&mut h, entry(&format!("https://a.test/{i}")));
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
