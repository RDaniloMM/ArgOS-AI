//! SQLite-backed [`RelationalStore`] implementation.
//!
//! Solo-profile derived relational state (audit log, metadata, n8n refs,
//! permissions, provider configs, episodic memory) lives in a single SQLite
//! database file (`.argos/store/argos.db` per ADR-002). WAL mode is enabled at
//! open time and a single connection is guarded by a `Mutex` to enforce the
//! single-writer discipline mandated by ADR-004. Blocking `rusqlite` calls are
//! dispatched onto Tokio's blocking thread pool via `spawn_blocking` so the
//! async executor is never held on a synchronous SQLite operation.

use std::path::Path;
use std::sync::{Arc, Mutex};

use argos_core::{ArgosError, Result};
use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension};

use crate::traits::RelationalStore;

/// SQLite-backed implementation of [`RelationalStore`].
///
/// Holds a single `rusqlite` connection behind a `Mutex` (single-writer
/// discipline, ADR-004). The connection is wrapped in an `Arc` so each async
/// method can clone it and move the clone into a `spawn_blocking` closure,
/// keeping the async executor free of blocking SQLite calls.
pub struct SqliteRelationalStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteRelationalStore {
    /// Open (or create) a SQLite database at `path`, enable WAL mode, and create
    /// the derived-state schema (`kv_store`) if it does not already exist.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path).map_err(map_sqlite)?;
        // Enable WAL for concurrent-reader / single-writer semantics (ADR-004).
        conn.execute_batch("PRAGMA journal_mode = WAL;")
            .map_err(map_sqlite)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS kv_store (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )
        .map_err(map_sqlite)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Read the current SQLite journal mode. Used to verify WAL is active.
    pub async fn journal_mode(&self) -> Result<String> {
        let conn = self.conn.clone();
        spawn(move || -> Result<String> {
            let conn = lock(&conn)?;
            let mode: String = conn
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .map_err(map_sqlite)?;
            Ok(mode)
        })
        .await
    }
}

#[async_trait]
impl RelationalStore for SqliteRelationalStore {
    async fn put(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.clone();
        let key = key.to_string();
        let value = value.to_string();
        spawn(move || -> Result<()> {
            let conn = lock(&conn)?;
            conn.execute(
                "INSERT OR REPLACE INTO kv_store (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .map_err(map_sqlite)?;
            Ok(())
        })
        .await
    }

    async fn get(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.clone();
        let key = key.to_string();
        spawn(move || -> Result<Option<String>> {
            let conn = lock(&conn)?;
            let value: Option<String> = conn
                .query_row(
                    "SELECT value FROM kv_store WHERE key = ?1",
                    params![key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(map_sqlite)?;
            Ok(value)
        })
        .await
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let conn = self.conn.clone();
        let key = key.to_string();
        spawn(move || -> Result<()> {
            let conn = lock(&conn)?;
            conn.execute("DELETE FROM kv_store WHERE key = ?1", params![key])
                .map_err(map_sqlite)?;
            Ok(())
        })
        .await
    }

    async fn query(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        let conn = self.conn.clone();
        let pattern = escape_like(prefix);
        spawn(move || -> Result<Vec<(String, String)>> {
            let conn = lock(&conn)?;
            let mut stmt = conn
                .prepare(
                    "SELECT key, value FROM kv_store \
                     WHERE key LIKE ?1 ESCAPE '\\' ORDER BY key",
                )
                .map_err(map_sqlite)?;
            let rows = stmt
                .query_map(params![pattern], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(map_sqlite)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(map_sqlite)?;
            Ok(rows)
        })
        .await
    }
}

/// Escape a prefix for use as a SQLite `LIKE` pattern.
///
/// `%`, `_`, and `\` are escaped so the pattern matches a true prefix (no
/// wildcard interpretation), mirroring the in-memory stub's `starts_with`
/// semantics. A trailing `%` wildcard is appended.
fn escape_like(prefix: &str) -> String {
    let mut out = String::with_capacity(prefix.len() + 4);
    for ch in prefix.chars() {
        if ch == '%' || ch == '_' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('%');
    out
}

/// Map a `rusqlite` error into the ArgOS error vocabulary.
fn map_sqlite(err: rusqlite::Error) -> ArgosError {
    ArgosError::Storage(err.to_string())
}

/// Lock the shared connection, mapping a poisoned-mutex error into ArgOS.
fn lock(conn: &Mutex<Connection>) -> Result<std::sync::MutexGuard<'_, Connection>> {
    conn.lock().map_err(|e| ArgosError::Storage(e.to_string()))
}

/// Run a blocking closure on Tokio's blocking thread pool and flatten the
/// `JoinError`/inner-`Result` into a single [`Result`].
async fn spawn<T, F>(f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ArgosError::Storage(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::SqliteRelationalStore;
    use crate::traits::RelationalStore;
    use tempfile::tempdir;

    /// Open a fresh store backed by a temp SQLite file.
    fn open_temp() -> (tempfile::TempDir, SqliteRelationalStore) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("argos.db");
        let store = SqliteRelationalStore::open(&db_path).unwrap();
        (dir, store)
    }

    #[tokio::test]
    async fn sqlite_opens_and_creates_schema_on_temp_file() {
        let (_dir, store) = open_temp();
        // A successful put/get round-trip proves the kv_store table exists.
        store.put("k", "v").await.unwrap();
        assert_eq!(store.get("k").await.unwrap(), Some("v".to_string()));
    }

    #[tokio::test]
    async fn sqlite_put_then_get_returns_same_value() {
        let (_dir, store) = open_temp();
        store.put("alpha", "one").await.unwrap();
        store.put("beta", "two").await.unwrap();
        assert_eq!(store.get("alpha").await.unwrap(), Some("one".to_string()));
        assert_eq!(store.get("beta").await.unwrap(), Some("two".to_string()));
    }

    #[tokio::test]
    async fn sqlite_get_missing_returns_none() {
        let (_dir, store) = open_temp();
        // The trait returns Option<String>; absence is Ok(None) (Liskov-consistent
        // with the in-memory stub). The task prompt's "NotFound" wording would
        // break the committed trait contract, so absence is reported as None.
        assert_eq!(store.get("nope").await.unwrap(), None);
    }

    #[tokio::test]
    async fn sqlite_delete_removes_key() {
        let (_dir, store) = open_temp();
        store.put("k", "v").await.unwrap();
        assert!(store.get("k").await.unwrap().is_some());
        store.delete("k").await.unwrap();
        assert_eq!(store.get("k").await.unwrap(), None);
    }

    #[tokio::test]
    async fn sqlite_delete_missing_is_idempotent() {
        let (_dir, store) = open_temp();
        // Deleting a key that was never stored must not error.
        store.delete("ghost").await.unwrap();
    }

    #[tokio::test]
    async fn sqlite_query_returns_matching_prefix() {
        let (_dir, store) = open_temp();
        store.put("n8n:1", "alpha").await.unwrap();
        store.put("n8n:2", "beta").await.unwrap();
        store.put("perm:1", "gamma").await.unwrap();

        let n8n = store.query("n8n:").await.unwrap();
        assert_eq!(n8n.len(), 2);
        // Ordered by key.
        assert_eq!(n8n[0], ("n8n:1".to_string(), "alpha".to_string()));
        assert_eq!(n8n[1], ("n8n:2".to_string(), "beta".to_string()));

        // No matches yields an empty vec.
        assert!(store.query("zzz:").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn sqlite_wal_mode_is_enabled() {
        let (_dir, store) = open_temp();
        let mode = store.journal_mode().await.unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }
}
