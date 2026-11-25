//! Local SQLite store: disk-usage time series, cleaning audit log, explorer snapshots.
//! One file in the app config dir, WAL mode, schema migrations via `PRAGMA user_version`.
//! Callers must use it off the UI thread (scheduler thread / spawn_blocking).

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

/// Minimum spacing between usage samples (15 min).
pub const SAMPLE_EVERY: u64 = 15 * 60;
/// Full-resolution window; older samples are thinned to one per day.
pub const FINE_WINDOW: u64 = 30 * 86_400;
/// Daily samples are kept this long.
pub const KEEP_DAILY: u64 = 2 * 365 * 86_400;
/// Audit log retention.
pub const KEEP_HISTORY: u64 = 365 * 86_400;
/// Explorer snapshots: compare against one at least this old, keep at most this many per folder.
pub const SNAPSHOT_MIN_AGE: u64 = 3600;
const SNAPSHOTS_PER_DIR: i64 = 5;
const KEEP_SNAPSHOTS: u64 = 90 * 86_400;

const MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE usage (ts INTEGER PRIMARY KEY, used INTEGER NOT NULL, total INTEGER NOT NULL);
     CREATE TABLE history (
        id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, source TEXT NOT NULL,
        path TEXT NOT NULL, size INTEGER NOT NULL, disposal TEXT NOT NULL, ok INTEGER NOT NULL, reason TEXT);
     CREATE INDEX history_ts ON history(ts);
     CREATE TABLE snapshot (dir TEXT NOT NULL, ts INTEGER NOT NULL, name TEXT NOT NULL, size INTEGER NOT NULL,
        PRIMARY KEY (dir, ts, name));",
    // v2: small cached results (e.g. the last space breakdown) so the menu bar shows them instantly.
    "CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT NOT NULL, ts INTEGER NOT NULL);",
];

pub struct Store {
    conn: Mutex<Connection>,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub ts: u64,
    pub used: u64,
    pub total: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct HistoryRow {
    pub id: i64,
    pub ts: u64,
    /// "smart" | "quick" | "explorer" | "tool"
    pub source: String,
    pub path: String,
    pub size: u64,
    /// "delete" | "trash" | "emptytrash" | "command"
    pub disposal: String,
    pub ok: bool,
    pub reason: Option<String>,
}

pub struct NewHistory<'a> {
    pub source: &'a str,
    pub path: &'a str,
    pub size: u64,
    pub disposal: &'a str,
    pub ok: bool,
    pub reason: Option<&'a str>,
}

/// Per-child size change versus an older snapshot of the same folder.
#[derive(Debug, Default)]
pub struct Baseline {
    pub ts: u64,
    pub sizes: HashMap<String, u64>,
}

fn is_corrupt(e: &rusqlite::Error) -> bool {
    matches!(e, rusqlite::Error::SqliteFailure(f, _) if matches!(f.code, rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt))
}

fn migrate(c: &Connection) -> rusqlite::Result<()> {
    let v: usize = c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(v) {
        let tx = c.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1))?;
        tx.commit()?;
    }
    Ok(())
}

impl Store {
    /// Open (creating/migrating) the database. A file that is not a valid SQLite database (corrupted,
    /// truncated) is moved aside to `<name>.corrupt-<unix time>` and a fresh one is created: history is
    /// a convenience and must never stop the app. Other errors (e.g. locked) are returned as-is.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        match Self::open_once(path) {
            Err(e) if is_corrupt(&e) => {
                let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let mut aside = path.as_os_str().to_owned();
                aside.push(format!(".corrupt-{ts}"));
                let _ = std::fs::rename(path, &aside);
                for ext in ["-wal", "-shm"] {
                    let mut side = path.as_os_str().to_owned();
                    side.push(ext);
                    let _ = std::fs::remove_file(side);
                }
                Self::open_once(path)
            }
            r => r,
        }
    }

    fn open_once(path: &Path) -> rusqlite::Result<Self> {
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let c = Connection::open(path)?;
        c.busy_timeout(std::time::Duration::from_secs(2))?; // before anything that may hit a lock
        c.pragma_update(None, "journal_mode", "WAL")?;
        c.pragma_update(None, "synchronous", "NORMAL")?;
        migrate(&c)?;
        Ok(Self { conn: Mutex::new(c) })
    }

    #[cfg(test)]
    pub fn memory() -> Self {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        Self { conn: Mutex::new(c) }
    }

    #[cfg(test)]
    pub fn schema_version(&self) -> i64 {
        self.conn.lock().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap_or(0)
    }

    // ---------- usage time series ----------

    /// Record a sample unless the last one is younger than SAMPLE_EVERY. Returns true if stored.
    pub fn record_usage(&self, now: u64, used: u64, total: u64) -> rusqlite::Result<bool> {
        let c = self.conn.lock();
        let last: Option<i64> = c.query_row("SELECT MAX(ts) FROM usage", [], |r| r.get(0))?;
        if last.is_some_and(|t| now < t as u64 + SAMPLE_EVERY) {
            return Ok(false);
        }
        c.execute("INSERT OR REPLACE INTO usage (ts, used, total) VALUES (?1, ?2, ?3)", params![now as i64, used as i64, total as i64])?;
        Ok(true)
    }

    pub fn usage_since(&self, since: u64) -> rusqlite::Result<Vec<Sample>> {
        let c = self.conn.lock();
        let mut st = c.prepare("SELECT ts, used, total FROM usage WHERE ts >= ?1 ORDER BY ts")?;
        let rows = st.query_map([since as i64], |r| Ok(Sample { ts: r.get::<_, i64>(0)? as u64, used: r.get::<_, i64>(1)? as u64, total: r.get::<_, i64>(2)? as u64 }))?;
        rows.collect()
    }

    // ---------- audit log ----------

    pub fn log(&self, now: u64, rows: &[NewHistory]) -> rusqlite::Result<()> {
        let mut c = self.conn.lock();
        let tx = c.transaction()?;
        {
            let mut st = tx.prepare("INSERT INTO history (ts, source, path, size, disposal, ok, reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")?;
            for r in rows {
                st.execute(params![now as i64, r.source, r.path, r.size as i64, r.disposal, r.ok, r.reason])?;
            }
        }
        tx.commit()
    }

    pub fn history(&self, limit: usize) -> rusqlite::Result<Vec<HistoryRow>> {
        let c = self.conn.lock();
        let mut st = c.prepare("SELECT id, ts, source, path, size, disposal, ok, reason FROM history ORDER BY ts DESC, id DESC LIMIT ?1")?;
        let rows = st.query_map([limit as i64], |r| {
            Ok(HistoryRow {
                id: r.get(0)?,
                ts: r.get::<_, i64>(1)? as u64,
                source: r.get(2)?,
                path: r.get(3)?,
                size: r.get::<_, i64>(4)? as u64,
                disposal: r.get(5)?,
                ok: r.get(6)?,
                reason: r.get(7)?,
            })
        })?;
        rows.collect()
    }

    // ---------- explorer snapshots ----------

    /// Store this listing of `dir` and return the most recent earlier snapshot that is at least
    /// SNAPSHOT_MIN_AGE old (so quick re-visits still compare against a meaningful baseline).
    /// A new snapshot is only written when the latest stored one is older than SNAPSHOT_MIN_AGE.
    pub fn snapshot(&self, dir: &str, now: u64, entries: &[(String, u64)]) -> rusqlite::Result<Option<Baseline>> {
        let mut c = self.conn.lock();
        let base_ts: Option<i64> = c
            .query_row("SELECT MAX(ts) FROM snapshot WHERE dir = ?1 AND ts <= ?2", params![dir, now.saturating_sub(SNAPSHOT_MIN_AGE) as i64], |r| r.get(0))
            .optional()?
            .flatten();
        let baseline = match base_ts {
            Some(ts) => {
                let mut st = c.prepare("SELECT name, size FROM snapshot WHERE dir = ?1 AND ts = ?2")?;
                let sizes = st.query_map(params![dir, ts], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64)))?.collect::<rusqlite::Result<HashMap<_, _>>>()?;
                Some(Baseline { ts: ts as u64, sizes })
            }
            None => None,
        };
        let latest: Option<i64> = c.query_row("SELECT MAX(ts) FROM snapshot WHERE dir = ?1", [dir], |r| r.get(0))?;
        if latest.is_none_or(|t| now >= t as u64 + SNAPSHOT_MIN_AGE) {
            let tx = c.transaction()?;
            {
                let mut st = tx.prepare("INSERT OR REPLACE INTO snapshot (dir, ts, name, size) VALUES (?1, ?2, ?3, ?4)")?;
                for (name, size) in entries {
                    st.execute(params![dir, now as i64, name, *size as i64])?;
                }
                // keep only the newest N generations for this folder
                tx.execute(
                    "DELETE FROM snapshot WHERE dir = ?1 AND ts NOT IN (SELECT DISTINCT ts FROM snapshot WHERE dir = ?1 ORDER BY ts DESC LIMIT ?2)",
                    params![dir, SNAPSHOTS_PER_DIR],
                )?;
            }
            tx.commit()?;
        }
        Ok(baseline)
    }

    // ---------- cached values ----------

    pub fn put_json<T: Serialize>(&self, key: &str, now: u64, v: &T) -> rusqlite::Result<()> {
        let json = serde_json::to_string(v).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        self.conn.lock().execute("INSERT OR REPLACE INTO kv (key, value, ts) VALUES (?1, ?2, ?3)", params![key, json, now as i64])?;
        Ok(())
    }

    /// Returns (value, stored_at). Unparseable values (older schema) read as missing.
    pub fn get_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<(T, u64)> {
        let (json, ts): (String, i64) = self.conn.lock().query_row("SELECT value, ts FROM kv WHERE key = ?1", [key], |r| Ok((r.get(0)?, r.get(1)?))).optional().ok()??;
        serde_json::from_str(&json).ok().map(|v| (v, ts as u64))
    }

    // ---------- retention ----------

    /// Thin usage to 1 sample / 15 min within FINE_WINDOW and 1 / day before it; drop old rows everywhere.
    pub fn prune(&self, now: u64) -> rusqlite::Result<()> {
        let c = self.conn.lock();
        let cutoff = now.saturating_sub(FINE_WINDOW) as i64;
        c.execute(
            "DELETE FROM usage WHERE ts >= ?1 AND ts NOT IN (SELECT MAX(ts) FROM usage WHERE ts >= ?1 GROUP BY ts / ?2)",
            params![cutoff, SAMPLE_EVERY as i64],
        )?;
        c.execute("DELETE FROM usage WHERE ts < ?1 AND ts NOT IN (SELECT MAX(ts) FROM usage WHERE ts < ?1 GROUP BY ts / 86400)", [cutoff])?;
        c.execute("DELETE FROM usage WHERE ts < ?1", [now.saturating_sub(KEEP_DAILY) as i64])?;
        c.execute("DELETE FROM history WHERE ts < ?1", [now.saturating_sub(KEEP_HISTORY) as i64])?;
        c.execute("DELETE FROM snapshot WHERE ts < ?1", [now.saturating_sub(KEEP_SNAPSHOTS) as i64])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const D: u64 = 86_400;

    #[test]
    fn migrates_file_db_and_is_idempotent() {
        let p = std::env::temp_dir().join(format!("tidy-store-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let s = Store::open(&p).unwrap();
        assert_eq!(s.schema_version(), MIGRATIONS.len() as i64);
        s.record_usage(1000, 1, 2).unwrap();
        drop(s);
        let s = Store::open(&p).unwrap(); // re-open: no migration re-run, data intact
        assert_eq!(s.schema_version(), MIGRATIONS.len() as i64);
        assert_eq!(s.usage_since(0).unwrap().len(), 1);
        let mode: String = s.conn.lock().query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode, "wal");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn corrupt_file_is_moved_aside_and_recreated() {
        let dir = std::env::temp_dir().join(format!("tidy-store-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("tidy.db");
        std::fs::write(&p, vec![0x42u8; 8192]).unwrap(); // not an SQLite file
        let s = Store::open(&p).unwrap();
        assert_eq!(s.schema_version(), MIGRATIONS.len() as i64);
        assert!(s.record_usage(1000, 1, 2).unwrap());
        let aside = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).any(|e| e.file_name().to_string_lossy().starts_with("tidy.db.corrupt-"));
        assert!(aside, "corrupt file kept for inspection, not deleted");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upgrades_a_v1_database_in_place() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(MIGRATIONS[0]).unwrap();
        c.execute_batch("PRAGMA user_version = 1; INSERT INTO usage VALUES (5, 1, 2);").unwrap();
        migrate(&c).unwrap();
        let s = Store { conn: Mutex::new(c) };
        assert_eq!(s.schema_version(), 2);
        assert_eq!(s.usage_since(0).unwrap().len(), 1, "v1 data survives");
        s.put_json("k", 9, &vec![1, 2, 3]).unwrap();
        assert_eq!(s.get_json::<Vec<i32>>("k"), Some((vec![1, 2, 3], 9)));
        assert_eq!(s.get_json::<String>("k"), None, "type mismatch reads as missing");
    }

    #[test]
    fn sampling_is_throttled_to_15_minutes() {
        let s = Store::memory();
        assert!(s.record_usage(10_000, 1, 10).unwrap());
        assert!(!s.record_usage(10_000 + 60, 2, 10).unwrap());
        assert!(s.record_usage(10_000 + SAMPLE_EVERY, 3, 10).unwrap());
        assert_eq!(s.usage_since(0).unwrap().len(), 2);
    }

    #[test]
    fn retention_keeps_fine_recent_and_daily_old() {
        let s = Store::memory();
        let now = 2000 * D;
        {
            let c = s.conn.lock();
            // 40 days of 5-minute samples (bypassing the throttle)
            let mut t = now - 40 * D;
            while t <= now {
                c.execute("INSERT INTO usage VALUES (?1, 1, 2)", [t as i64]).unwrap();
                t += 300;
            }
            c.execute("INSERT INTO usage VALUES (?1, 1, 2)", [(now - 3 * 365 * D) as i64]).unwrap();
        }
        s.prune(now).unwrap();
        let all = s.usage_since(0).unwrap();
        let old: Vec<_> = all.iter().filter(|x| x.ts < now - FINE_WINDOW).collect();
        let recent: Vec<_> = all.iter().filter(|x| x.ts >= now - FINE_WINDOW).collect();
        assert!(old.iter().all(|x| x.ts > now - KEEP_DAILY), "3-year-old sample must be gone");
        assert!(old.len() <= 11 && old.len() >= 9, "~1/day for the 10 old days, got {}", old.len());
        let per_day_recent = recent.len() as f64 / 30.0;
        assert!((95.0..=97.0).contains(&per_day_recent), "~96/day (15-min buckets), got {per_day_recent}");
        // no two recent samples in the same 15-minute bucket
        let mut buckets: Vec<_> = recent.iter().map(|x| x.ts / SAMPLE_EVERY).collect();
        buckets.dedup();
        assert_eq!(buckets.len(), recent.len());
    }

    #[test]
    fn audit_log_roundtrip_and_retention() {
        let s = Store::memory();
        let row = |p| NewHistory { source: "smart", path: p, size: 42, disposal: "delete", ok: true, reason: None };
        s.log(10, &[row("/old")]).unwrap();
        s.log(400 * D, &[row("/a"), NewHistory { ok: false, reason: Some("blocked"), ..row("/b") }]).unwrap();
        let h = s.history(10).unwrap();
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].path, "/b");
        assert_eq!(h[0].reason.as_deref(), Some("blocked"));
        s.prune(400 * D).unwrap();
        assert_eq!(s.history(10).unwrap().len(), 2);
    }

    #[test]
    fn snapshot_baseline_is_at_least_an_hour_old() {
        let s = Store::memory();
        let e = |n: &str, v| (n.to_string(), v);
        assert!(s.snapshot("/x", 1_000_000, &[e("a", 100)]).unwrap().is_none());
        // a quick re-visit: no baseline old enough yet, and no new snapshot written
        assert!(s.snapshot("/x", 1_000_060, &[e("a", 150)]).unwrap().is_none());
        // two hours later: baseline is the first snapshot
        let b = s.snapshot("/x", 1_007_200, &[e("a", 300)]).unwrap().unwrap();
        assert_eq!((b.ts, b.sizes["a"]), (1_000_000, 100));
        // generations are capped per folder
        for i in 0..10 {
            s.snapshot("/x", 2_000_000 + i * 4000, &[e("a", i)]).unwrap();
        }
        let n: i64 = s.conn.lock().query_row("SELECT COUNT(DISTINCT ts) FROM snapshot WHERE dir='/x'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, SNAPSHOTS_PER_DIR);
    }
}
