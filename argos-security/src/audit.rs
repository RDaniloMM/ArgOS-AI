//! SQLite-backed, tamper-evident audit log (ADR-006).
//!
//! [`HashChainAuditLog`] appends [`AuditEntry`]s to a SQLite `audit_log` table,
//! chaining each entry with `this_hash = sha256(prev_hash || canonical(payload))`
//! (genesis `prev_hash` = 64 zeros). [`AuditLog::verify_chain`] recomputes every
//! hash and chain link, detecting any in-place modification of the stored
//! payload (spec `audit-chain-tamper-detection`). Blocking `rusqlite` calls are
//! dispatched onto Tokio's blocking pool via `spawn_blocking` (ADR-004
//! single-writer discipline). The canonical payload format mirrors the
//! `traits.rs` stub so the hash-chain concept is unchanged across backends.

use std::path::Path;
use std::sync::{Arc, Mutex};

use argos_core::{ArgosError, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::traits::{AuditEntry, AuditFilter, AuditLog};

/// Genesis `prev_hash` for the first chain entry (64 zeros).
const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// SQLite-backed, append-only, tamper-evident audit log.
///
/// Holds a single `rusqlite` connection behind a `Mutex` (single-writer
/// discipline, ADR-004). Each async method clones the `Arc` and moves it into a
/// `spawn_blocking` closure so the async executor is never held on a blocking
/// SQLite operation.
pub struct HashChainAuditLog {
    conn: Arc<Mutex<Connection>>,
}

impl HashChainAuditLog {
    /// Open (or create) the audit database at `path`, enable WAL mode, and
    /// create the `audit_log` schema if it does not already exist.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path).map_err(map_sqlite)?;
        conn.execute_batch("PRAGMA journal_mode = WAL;")
            .map_err(map_sqlite)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS audit_log (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT    NOT NULL,
                subject   TEXT    NOT NULL,
                action    TEXT    NOT NULL,
                resource  TEXT    NOT NULL,
                result    TEXT    NOT NULL,
                prev_hash TEXT    NOT NULL,
                this_hash TEXT    NOT NULL
            )",
            [],
        )
        .map_err(map_sqlite)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

/// Canonical, deterministic encoding of an entry's payload for hashing.
///
/// Pipe-delimited (mirrors the `traits.rs` stub); the chain's integrity depends
/// only on determinism, not on the serialization format.
fn canonical(e: &AuditEntry) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        e.timestamp.to_rfc3339(),
        e.subject,
        e.action,
        e.resource,
        e.result
    )
}

/// `this_hash = sha256(prev_hash || canonical(payload))`.
fn hash_of(prev: &str, payload: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev.as_bytes());
    hasher.update(payload.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[async_trait]
impl AuditLog for HashChainAuditLog {
    async fn record(&mut self, entry: &AuditEntry) -> Result<()> {
        let conn = self.conn.clone();
        let timestamp = entry.timestamp.to_rfc3339();
        let subject = entry.subject.clone();
        let action = entry.action.clone();
        let resource = entry.resource.clone();
        let result = entry.result.clone();
        spawn(move || -> Result<()> {
            let conn = lock(&conn)?;
            // Read the last entry's this_hash (genesis if the table is empty).
            let prev_hash: Option<String> = conn
                .query_row(
                    "SELECT this_hash FROM audit_log ORDER BY id DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .optional()
                .map_err(map_sqlite)?;
            let prev_hash = prev_hash.unwrap_or_else(|| GENESIS_HASH.to_string());

            // Recompute the canonical payload from the fields being stored so
            // the hash is derived from exactly what lands in the table.
            let payload = format!("{timestamp}|{subject}|{action}|{resource}|{result}");
            let this_hash = hash_of(&prev_hash, &payload);

            conn.execute(
                "INSERT INTO audit_log \
                 (timestamp, subject, action, resource, result, prev_hash, this_hash) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![timestamp, subject, action, resource, result, prev_hash, this_hash],
            )
            .map_err(map_sqlite)?;
            Ok(())
        })
        .await
    }

    async fn query(&self, filter: &AuditFilter) -> Result<Vec<AuditEntry>> {
        let conn = self.conn.clone();
        let since = filter.since.map(|ts| ts.to_rfc3339());
        let subject = filter.subject.clone();
        let action = filter.action.clone();
        spawn(move || -> Result<Vec<AuditEntry>> {
            let conn = lock(&conn)?;

            // Build an optional WHERE clause from the supplied filters.
            let mut clauses: Vec<&str> = Vec::new();
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            if let Some(since) = &since {
                clauses.push("timestamp >= ?");
                params_vec.push(Box::new(since.clone()));
            }
            if let Some(subject) = &subject {
                clauses.push("subject = ?");
                params_vec.push(Box::new(subject.clone()));
            }
            if let Some(action) = &action {
                clauses.push("action = ?");
                params_vec.push(Box::new(action.clone()));
            }
            let where_sql = if clauses.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", clauses.join(" AND "))
            };
            let sql = format!(
                "SELECT timestamp, subject, action, resource, result, prev_hash, this_hash \
                 FROM audit_log {where_sql} ORDER BY id ASC"
            );
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();

            let mut stmt = conn.prepare(&sql).map_err(map_sqlite)?;
            let rows = stmt
                .query_map(params_refs.as_slice(), |row| {
                    let ts_str: String = row.get(0)?;
                    let subject: String = row.get(1)?;
                    let action: String = row.get(2)?;
                    let resource: String = row.get(3)?;
                    let result: String = row.get(4)?;
                    let prev_hash: String = row.get(5)?;
                    let this_hash: String = row.get(6)?;
                    let timestamp = DateTime::parse_from_rfc3339(&ts_str)
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?
                        .with_timezone(&Utc);
                    Ok(AuditEntry {
                        timestamp,
                        subject,
                        action,
                        resource,
                        result,
                        prev_hash,
                        this_hash,
                    })
                })
                .map_err(map_sqlite)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(map_sqlite)?;
            Ok(rows)
        })
        .await
    }

    async fn verify_chain(&self) -> Result<bool> {
        let conn = self.conn.clone();
        spawn(move || -> Result<bool> {
            let conn = lock(&conn)?;
            let mut stmt = conn
                .prepare(
                    "SELECT timestamp, subject, action, resource, result, prev_hash, this_hash \
                     FROM audit_log ORDER BY id ASC",
                )
                .map_err(map_sqlite)?;
            let entries = stmt
                .query_map([], |row| {
                    let ts_str: String = row.get(0)?;
                    let subject: String = row.get(1)?;
                    let action: String = row.get(2)?;
                    let resource: String = row.get(3)?;
                    let result: String = row.get(4)?;
                    let prev_hash: String = row.get(5)?;
                    let this_hash: String = row.get(6)?;
                    let timestamp = DateTime::parse_from_rfc3339(&ts_str)
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?
                        .with_timezone(&Utc);
                    Ok(AuditEntry {
                        timestamp,
                        subject,
                        action,
                        resource,
                        result,
                        prev_hash,
                        this_hash,
                    })
                })
                .map_err(map_sqlite)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(map_sqlite)?;

            let mut prev = GENESIS_HASH.to_string();
            for e in &entries {
                if e.prev_hash != prev {
                    return Ok(false);
                }
                let expected = hash_of(&e.prev_hash, &canonical(e));
                if e.this_hash != expected {
                    return Ok(false);
                }
                prev = e.this_hash.clone();
            }
            Ok(true)
        })
        .await
    }
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
    use crate::audit::HashChainAuditLog;
    use crate::traits::{AuditEntry, AuditFilter, AuditLog};
    use chrono::Utc;
    use rusqlite::Connection;
    use tempfile::tempdir;

    fn entry(subject: &str, action: &str) -> AuditEntry {
        AuditEntry {
            timestamp: Utc::now(),
            subject: subject.into(),
            action: action.into(),
            resource: "wiki".into(),
            result: "ok".into(),
            prev_hash: String::new(),
            this_hash: String::new(),
        }
    }

    fn open_temp() -> (tempfile::TempDir, std::path::PathBuf, HashChainAuditLog) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("audit.db");
        let log = HashChainAuditLog::open(&db_path).unwrap();
        (dir, db_path, log)
    }

    // `record` takes `&mut self`; every recording test binds `mut log`.

    #[tokio::test]
    async fn opens_and_creates_schema_on_temp_file() {
        let (_dir, _path, mut log) = open_temp();
        // A successful record proves the audit_log table exists.
        log.record(&entry("agent", "wiki.write")).await.unwrap();
        let all = log.query(&AuditFilter::default()).await.unwrap();
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn record_stores_entry_with_computed_hash() {
        let (_dir, _path, mut log) = open_temp();
        log.record(&entry("agent", "wiki.write")).await.unwrap();
        let entries = log.query(&AuditFilter::default()).await.unwrap();
        assert_eq!(entries.len(), 1);
        // The hash is computed (non-empty, 64 hex chars for sha256).
        assert_eq!(entries[0].this_hash.len(), 64);
        assert!(entries[0].this_hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn first_entry_has_genesis_prev_hash() {
        let (_dir, _path, mut log) = open_temp();
        log.record(&entry("agent", "wiki.write")).await.unwrap();
        let entries = log.query(&AuditFilter::default()).await.unwrap();
        // Genesis prev_hash = 64 zeros.
        assert_eq!(entries[0].prev_hash, "0".repeat(64));
    }

    #[tokio::test]
    async fn second_entry_prev_hash_equals_first_this_hash() {
        let (_dir, _path, mut log) = open_temp();
        log.record(&entry("a", "x")).await.unwrap();
        log.record(&entry("b", "y")).await.unwrap();
        let entries = log.query(&AuditFilter::default()).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].prev_hash, entries[0].this_hash);
    }

    #[tokio::test]
    async fn query_returns_entries_in_order() {
        let (_dir, _path, mut log) = open_temp();
        log.record(&entry("a", "x")).await.unwrap();
        log.record(&entry("b", "y")).await.unwrap();
        log.record(&entry("c", "z")).await.unwrap();
        let entries = log.query(&AuditFilter::default()).await.unwrap();
        assert_eq!(entries.len(), 3);
        // Chronological (insertion) order.
        assert_eq!(entries[0].subject, "a");
        assert_eq!(entries[1].subject, "b");
        assert_eq!(entries[2].subject, "c");
    }

    #[tokio::test]
    async fn query_filters_by_subject() {
        let (_dir, _path, mut log) = open_temp();
        log.record(&entry("agent-1", "wiki.write")).await.unwrap();
        log.record(&entry("agent-2", "n8n.run")).await.unwrap();
        let only1 = log
            .query(&AuditFilter {
                since: None,
                subject: Some("agent-1".into()),
                action: None,
            })
            .await
            .unwrap();
        assert_eq!(only1.len(), 1);
        assert_eq!(only1[0].subject, "agent-1");
    }

    #[tokio::test]
    async fn query_filters_by_action() {
        let (_dir, _path, mut log) = open_temp();
        log.record(&entry("a", "wiki.write")).await.unwrap();
        log.record(&entry("b", "n8n.run")).await.unwrap();
        log.record(&entry("c", "n8n.run")).await.unwrap();
        let runs = log
            .query(&AuditFilter {
                since: None,
                subject: None,
                action: Some("n8n.run".into()),
            })
            .await
            .unwrap();
        assert_eq!(runs.len(), 2);
        assert!(runs.iter().all(|e| e.action == "n8n.run"));
    }

    #[tokio::test]
    async fn verify_chain_true_for_unmodified_chain() {
        let (_dir, _path, mut log) = open_temp();
        log.record(&entry("a", "x")).await.unwrap();
        log.record(&entry("b", "y")).await.unwrap();
        log.record(&entry("c", "z")).await.unwrap();
        assert!(log.verify_chain().await.unwrap());
    }

    #[tokio::test]
    async fn verify_chain_false_after_tamper() {
        let (_dir, path, mut log) = open_temp();
        log.record(&entry("a", "x")).await.unwrap();
        log.record(&entry("b", "y")).await.unwrap();
        assert!(log.verify_chain().await.unwrap());

        // Tamper with the first entry's result via a second connection — the
        // recomputed hash no longer matches the stored this_hash.
        let conn2 = Connection::open(&path).unwrap();
        conn2
            .execute("UPDATE audit_log SET result = 'tampered' WHERE id = 1", [])
            .unwrap();
        assert!(!log.verify_chain().await.unwrap());
    }

    #[tokio::test]
    async fn record_multiple_entries_forms_valid_chain() {
        let (_dir, _path, mut log) = open_temp();
        for i in 0..5 {
            log.record(&entry(&format!("s{i}"), &format!("a{i}")))
                .await
                .unwrap();
        }
        let entries = log.query(&AuditFilter::default()).await.unwrap();
        assert_eq!(entries.len(), 5);
        // Each link's prev_hash equals the prior this_hash; genesis for first.
        assert_eq!(entries[0].prev_hash, "0".repeat(64));
        for w in entries.windows(2) {
            assert_eq!(w[1].prev_hash, w[0].this_hash);
        }
        assert!(log.verify_chain().await.unwrap());
    }
}
