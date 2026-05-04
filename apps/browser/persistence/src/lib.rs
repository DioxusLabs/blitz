//! Persistence layer for the browser app.
//!
//! Currently exposes [`HistoryStore`], an on-disk browsing-history store.
//!
//! ## Best-effort durability
//!
//! Every write path here is best-effort and may silently no-op:
//!
//! - On desktop, if `ProjectDirs::from` returns `None` or the data dir is
//!   unwritable, the sqlite connection is in-memory only — the on-disk view
//!   is empty across restarts but the in-process store still works.
//! - On mobile (Android, iOS) the store always opens an in-memory connection;
//!   history is in-process only and does not persist across launches.
//! - Per-statement sqlite errors are logged at `warn` and swallowed; callers
//!   never see a `Result`.
//!
//! This is intentional: history is UX state, not load-bearing on the rest of
//! the browser, and a transient disk failure must not propagate into a
//! navigation error. Callers must not rely on a write being durable.
//!
//! ## Threading
//!
//! All methods are synchronous and will block on sqlite I/O. The crate is
//! runtime-agnostic — callers are responsible for keeping disk work off any
//! latency-sensitive thread (e.g. by wrapping calls in `spawn_blocking`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, params};
use rusqlite_migration::{M, Migrations};
use url::Url;

// Schema migrations. Append a new `M::up(...)` for every schema change —
// never edit a previously-released entry, since the index in this list is
// the version number that gets stamped into `PRAGMA user_version` on disk.
fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(
        "CREATE TABLE history_entries (
           id          INTEGER PRIMARY KEY AUTOINCREMENT,
           url         TEXT    NOT NULL,
           title       TEXT    NOT NULL,
           favicon_url TEXT,
           visited_at  INTEGER NOT NULL
         );
         CREATE INDEX idx_history_visited_at ON history_entries(visited_at DESC);
         CREATE INDEX idx_history_url        ON history_entries(url);",
    )])
}

pub const MAX_HISTORY_ENTRIES: usize = 1000;

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

    /// Construct an entry from persisted parts, assigning a fresh in-memory id.
    fn from_parts(
        url: Url,
        title: String,
        favicon_url: Option<Url>,
        visited_at: SystemTime,
    ) -> Self {
        Self {
            id: next_history_entry_id(),
            url,
            title,
            favicon_url,
            visited_at,
        }
    }
}

fn next_history_entry_id() -> HistoryEntryId {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone)]
pub struct HistoryStore {
    conn: Arc<Mutex<Connection>>,
}

impl HistoryStore {
    pub fn open() -> Self {
        let mut conn = open_connection();
        if let Err(e) = migrations().to_latest(&mut conn) {
            tracing::warn!("history_store: schema migration failed: {e}");
        }
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    // Run `f` under the connection mutex. Returns None if the lock is
    // poisoned, in which case the caller silently degrades — history is
    // best-effort, never load-bearing on the rest of the browser.
    fn with_conn<R>(&self, f: impl FnOnce(&Connection) -> R) -> Option<R> {
        self.conn.lock().ok().map(|c| f(&c))
    }

    pub fn load_recent(&self, limit: usize) -> Vec<HistoryEntry> {
        self.with_conn(|conn| load_recent_inner(conn, limit))
            .unwrap_or_default()
    }

    pub fn record_visit(&self, entry: &HistoryEntry) {
        self.with_conn(|conn| record_visit_inner(conn, entry));
    }

    pub fn clear(&self) {
        self.with_conn(clear_inner);
    }

    // Patch the favicon for every row of `page_url` that doesn't yet have
    // one. Used by the background favicon probe so a successful resolution
    // survives across restarts.
    //
    // Note the asymmetry vs. `BrowsingHistory::set_favicon_by_id`: in memory
    // we patch a single entry by id, but on disk we patch *every* still-NULL
    // row for that URL. This matters when the user navigates A → B → A
    // before the first probe finishes — both A rows want the same icon,
    // and the disk catches up everywhere on the next restart-load.
    //
    // The `favicon_url IS NULL` filter prevents a stale probe from
    // clobbering a fresher resolution that landed first.
    pub fn set_favicon_by_url(&self, page_url: &Url, favicon_url: &Url) {
        self.with_conn(|conn| set_favicon_by_url_inner(conn, page_url, favicon_url));
    }
}

fn load_recent_inner(conn: &Connection, limit: usize) -> Vec<HistoryEntry> {
    let mut stmt = match conn.prepare(
        "SELECT url, title, favicon_url, visited_at
         FROM history_entries
         ORDER BY visited_at DESC
         LIMIT ?1",
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("history_store: load_recent prepare failed: {e}");
            return Vec::new();
        }
    };

    let rows = stmt.query_map(params![limit as i64], |row| {
        let url_str: String = row.get(0)?;
        let title: String = row.get(1)?;
        let favicon_str: Option<String> = row.get(2)?;
        let visited_secs: i64 = row.get(3)?;
        Ok((url_str, title, favicon_str, visited_secs))
    });

    let rows = match rows {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("history_store: load_recent query failed: {e}");
            return Vec::new();
        }
    };

    let mut entries = Vec::new();
    for row in rows.flatten() {
        let (url_str, title, favicon_str, visited_secs) = row;
        let url = match Url::parse(&url_str) {
            Ok(u) => u,
            Err(_) => continue,
        };
        let favicon_url = favicon_str.and_then(|s| Url::parse(&s).ok());
        let visited_at = UNIX_EPOCH + Duration::from_secs(visited_secs.max(0) as u64);
        entries.push(HistoryEntry::from_parts(
            url,
            title,
            favicon_url,
            visited_at,
        ));
    }
    entries
}

fn record_visit_inner(conn: &Connection, entry: &HistoryEntry) {
    if let Err(e) = record_visit_tx(conn, entry) {
        tracing::warn!("history_store: record_visit failed: {e}");
    }
}

// Wrap the upsert + prune in a single transaction so a crash between the
// two writes can't leave the on-disk row count over `MAX_HISTORY_ENTRIES`.
fn record_visit_tx(conn: &Connection, entry: &HistoryEntry) -> rusqlite::Result<()> {
    let visited_secs = entry
        .visited_at
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let url_str = entry.url.as_str();
    let favicon_str = entry.favicon_url.as_ref().map(|u| u.as_str().to_owned());

    let tx = conn.unchecked_transaction()?;

    // Mirror the in-memory consecutive-fold rule: if the most-recent
    // row has the same URL, update it in place rather than inserting
    // a new row. Keeps post-restart history aligned with the
    // in-memory view, which folds consecutive same-URL visits.
    let head: rusqlite::Result<(i64, String)> = tx.query_row(
        "SELECT id, url FROM history_entries ORDER BY visited_at DESC LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    );

    match head {
        Ok((id, head_url)) if head_url == url_str => {
            tx.execute(
                "UPDATE history_entries
                 SET title = ?1, favicon_url = ?2, visited_at = ?3
                 WHERE id = ?4",
                params![&entry.title, favicon_str, visited_secs, id],
            )?;
        }
        _ => {
            tx.execute(
                "INSERT INTO history_entries (url, title, favicon_url, visited_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![url_str, &entry.title, favicon_str, visited_secs],
            )?;
        }
    }

    // Prune rows beyond the in-memory cap so the disk view doesn't
    // diverge from the on-restart hydrated view.
    tx.execute(
        "DELETE FROM history_entries
         WHERE id NOT IN (
           SELECT id FROM history_entries
           ORDER BY visited_at DESC LIMIT ?1
         )",
        params![MAX_HISTORY_ENTRIES as i64],
    )?;

    tx.commit()
}

fn clear_inner(conn: &Connection) {
    if let Err(e) = conn.execute("DELETE FROM history_entries", []) {
        tracing::warn!("history_store: clear failed: {e}");
    }
}

fn set_favicon_by_url_inner(conn: &Connection, page_url: &Url, favicon_url: &Url) {
    if let Err(e) = conn.execute(
        "UPDATE history_entries SET favicon_url = ?1
         WHERE url = ?2 AND favicon_url IS NULL",
        params![favicon_url.as_str(), page_url.as_str()],
    ) {
        tracing::warn!("history_store: set_favicon_by_url failed: {e}");
    }
}

// Mobile (Android, iOS) skips the file path entirely and always lands in
// in-memory mode. `directories` isn't pulled in for those targets.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn open_file_connection() -> Result<Connection, String> {
    use directories::ProjectDirs;

    let dirs = ProjectDirs::from("com", "DioxusLabs", "Blitz")
        .ok_or_else(|| "ProjectDirs::from returned None".to_string())?;
    let db_path = dirs.data_dir().join("history.sqlite3");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create_dir_all({}): {e}", parent.display()))?;
    }
    Connection::open(&db_path).map_err(|e| format!("open({}): {e}", db_path.display()))
}

fn open_connection() -> Connection {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        match open_file_connection() {
            Ok(c) => return c,
            Err(e) => tracing::warn!(
                "history_store: file-backed open failed: {e}; falling back to in-memory"
            ),
        }
    }
    Connection::open_in_memory().expect("sqlite in-memory open must not fail")
}

// Tests target the inner SQL fns directly so a freshly migrated in-memory
// connection can be passed in without standing up the full `HistoryStore`
// wrapper. The behavior we care about — fold semantics, prune cap, NULL-only
// patching — lives in the inner fns.
#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use rusqlite::Connection;
    use url::Url;

    use super::{
        HistoryEntry, MAX_HISTORY_ENTRIES, clear_inner, load_recent_inner, migrations,
        record_visit_inner, set_favicon_by_url_inner,
    };

    fn make_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("in-memory open");
        migrations().to_latest(&mut conn).expect("migrate");
        conn
    }

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn entry_at(u: &str, secs: u64) -> HistoryEntry {
        HistoryEntry::from_parts(
            url(u),
            format!("Title for {u}"),
            None,
            UNIX_EPOCH + Duration::from_secs(secs),
        )
    }

    #[test]
    fn schema_bootstraps_cleanly() {
        let conn = make_conn();
        let entries = load_recent_inner(&conn, 10);
        assert!(entries.is_empty());
    }

    #[test]
    fn migrations_validate() {
        // Catches typos / out-of-order edits to the migration list before
        // they ever touch a real on-disk db.
        migrations().validate().expect("migrations valid");
    }

    #[test]
    fn record_and_load_round_trip() {
        let conn = make_conn();
        let e = entry_at("https://example.com/", 1_000_000);
        record_visit_inner(&conn, &e);
        let loaded = load_recent_inner(&conn, 10);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].url, e.url);
        assert_eq!(loaded[0].title, e.title);
        assert_eq!(loaded[0].favicon_url, e.favicon_url);
        // visited_at round-trips to the second.
        let stored_secs = loaded[0]
            .visited_at
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(stored_secs, 1_000_000);
    }

    #[test]
    fn load_recent_ordered_desc_by_visited_at() {
        let conn = make_conn();
        record_visit_inner(&conn, &entry_at("https://first.test/", 1_000));
        record_visit_inner(&conn, &entry_at("https://second.test/", 3_000));
        record_visit_inner(&conn, &entry_at("https://third.test/", 2_000));

        let loaded = load_recent_inner(&conn, 10);
        assert_eq!(loaded.len(), 3);
        // Most recent first.
        assert_eq!(loaded[0].url, url("https://second.test/"));
        assert_eq!(loaded[1].url, url("https://third.test/"));
        assert_eq!(loaded[2].url, url("https://first.test/"));
    }

    #[test]
    fn clear_empties_table() {
        let conn = make_conn();
        record_visit_inner(&conn, &entry_at("https://a.test/", 1_000));
        record_visit_inner(&conn, &entry_at("https://b.test/", 2_000));
        clear_inner(&conn);
        assert!(load_recent_inner(&conn, 10).is_empty());
    }

    #[test]
    fn folds_consecutive_same_url_visits() {
        let conn = make_conn();
        let first = HistoryEntry::from_parts(
            url("https://example.com/"),
            "Old title".to_string(),
            None,
            UNIX_EPOCH + Duration::from_secs(1_000),
        );
        let second = HistoryEntry::from_parts(
            url("https://example.com/"),
            "New title".to_string(),
            Some(url("https://example.com/favicon.ico")),
            UNIX_EPOCH + Duration::from_secs(2_000),
        );
        record_visit_inner(&conn, &first);
        record_visit_inner(&conn, &second);

        let loaded = load_recent_inner(&conn, 10);
        assert_eq!(
            loaded.len(),
            1,
            "consecutive same-URL visits fold to one row"
        );
        assert_eq!(loaded[0].title, "New title");
        assert_eq!(
            loaded[0].favicon_url.as_ref().map(|u| u.as_str()),
            Some("https://example.com/favicon.ico")
        );
        let secs = loaded[0]
            .visited_at
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(secs, 2_000);
    }

    #[test]
    fn keeps_non_consecutive_revisits() {
        let conn = make_conn();
        record_visit_inner(&conn, &entry_at("https://a.test/", 1_000));
        record_visit_inner(&conn, &entry_at("https://b.test/", 2_000));
        record_visit_inner(&conn, &entry_at("https://a.test/", 3_000));

        let loaded = load_recent_inner(&conn, 10);
        assert_eq!(loaded.len(), 3, "non-consecutive revisits are not folded");
    }

    #[test]
    fn set_favicon_patches_all_null_rows_for_url() {
        // Regression for the A → B → A case: the first probe must patch every
        // still-unresolved row for that URL, not just the most-recent one.
        let conn = make_conn();
        record_visit_inner(&conn, &entry_at("https://a.test/", 1_000));
        record_visit_inner(&conn, &entry_at("https://b.test/", 2_000));
        record_visit_inner(&conn, &entry_at("https://a.test/", 3_000));

        let favicon = url("https://a.test/favicon.ico");
        set_favicon_by_url_inner(&conn, &url("https://a.test/"), &favicon);

        let loaded = load_recent_inner(&conn, 10);
        let a_rows: Vec<_> = loaded
            .iter()
            .filter(|e| e.url == url("https://a.test/"))
            .collect();
        assert_eq!(a_rows.len(), 2);
        for row in a_rows {
            assert_eq!(row.favicon_url.as_ref(), Some(&favicon));
        }
        let b_row = loaded
            .iter()
            .find(|e| e.url == url("https://b.test/"))
            .expect("b row present");
        assert!(b_row.favicon_url.is_none());
    }

    #[test]
    fn set_favicon_does_not_overwrite_existing() {
        // A row that already has a favicon (e.g. resolved by a fresher probe)
        // must not be clobbered by a stale resolution.
        let conn = make_conn();
        record_visit_inner(&conn, &entry_at("https://a.test/", 1_000));
        let original = url("https://a.test/v1.ico");
        set_favicon_by_url_inner(&conn, &url("https://a.test/"), &original);

        let stale = url("https://a.test/stale.ico");
        set_favicon_by_url_inner(&conn, &url("https://a.test/"), &stale);

        let loaded = load_recent_inner(&conn, 10);
        assert_eq!(loaded[0].favicon_url.as_ref(), Some(&original));
    }

    #[test]
    fn prune_keeps_most_recent_max_entries() {
        let conn = make_conn();
        let overflow = (MAX_HISTORY_ENTRIES + 100) as u64;
        for i in 0u64..overflow {
            record_visit_inner(&conn, &entry_at(&format!("https://site{i}.test/"), i + 1));
        }
        let loaded = load_recent_inner(&conn, overflow as usize * 2);
        assert_eq!(loaded.len(), MAX_HISTORY_ENTRIES);
        // Most recent visit had visited_at = overflow.
        let top_secs = loaded[0]
            .visited_at
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(top_secs, overflow);
    }
}
