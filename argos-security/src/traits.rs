//! Security trait definitions.
//!
//! Cross-cutting security seams (ADR-006): permission gating, secret management
//! (incl. n8n credentials), and tamper-evident audit logging. These wrap every
//! tool invocation, wiki mutation, n8n call, and ArgOS-MCP-server tool exposure.
//! Secret material never lives in SQLite/config/logs — only behind [`SecretVault`].

use argos_core::{Result, Timestamp};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A permission decision returned by [`PermissionGate::check`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Permission {
    /// The action is permitted.
    Allow,
    /// The action is denied, with a human-readable reason.
    Deny(String),
}

/// A set of `(subject, resource, action)` grants.
///
/// A simple, serializable representation of the permission matrix. The
/// `Agent` trait keeps permissions as `Vec<String>` to avoid a circular
/// dependency; `PermissionSet` is the structured form used by the gate and
/// stored in the `permissions` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionSet {
    /// Grants as `(subject, resource, action)` tuples.
    pub permissions: Vec<(String, String, String)>,
}

/// One entry in the append-only, tamper-evident audit chain.
///
/// `this_hash = sha256(prev_hash || canonical(payload))`. Verification
/// recomputes each hash and checks the chain links, detecting any in-place
/// modification (ADR-006 / spec `audit-chain-tamper-detection`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    /// When the event occurred.
    pub timestamp: Timestamp,
    /// Who performed the action (agent/hand/tool/workflow/n8n-client).
    pub subject: String,
    /// What action was attempted (e.g. `wiki.write`, `n8n.execute`).
    pub action: String,
    /// What the action targeted.
    pub resource: String,
    /// The outcome (e.g. `ok`, `denied`).
    pub result: String,
    /// Hash of the previous entry (genesis = 64x`0`).
    pub prev_hash: String,
    /// Hash of this entry's payload + `prev_hash`.
    pub this_hash: String,
}

/// Filter for [`AuditLog::query`]. All fields optional — `None` means "any".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AuditFilter {
    /// Only entries at or after this timestamp.
    pub since: Option<Timestamp>,
    /// Only entries from this subject.
    pub subject: Option<String>,
    /// Only entries with this action.
    pub action: Option<String>,
}

/// Permission gate wrapping every action (subject x resource x action matrix).
///
/// Default-deny for sandboxed tiers; least-privilege for Tier-1/MCP. Every
/// tool invocation, wiki mutation, n8n call, and ArgOS-MCP-server tool
/// exposure routes through [`PermissionGate::check`].
#[async_trait]
pub trait PermissionGate: Send + Sync {
    /// Decide whether `subject` may perform `action` on `resource`.
    async fn check(&self, subject: &str, resource: &str, action: &str) -> Result<Permission>;
    /// Grant `subject` the right to perform `action` on `resource`.
    async fn grant(&mut self, subject: &str, resource: &str, action: &str) -> Result<()>;
    /// Revoke a previously granted right. No-op if absent.
    async fn revoke(&mut self, subject: &str, resource: &str, action: &str) -> Result<()>;
}

/// Secret vault — secrets (incl. n8n credentials) never live in SQLite.
///
/// KeyringVault is preferred (OS keyring); EncryptedFileVault is the fallback.
/// Secret material MUST NOT appear in `argos.db`, `config.toml`, or logs.
#[async_trait]
pub trait SecretVault: Send + Sync {
    /// Store `secret` under `key`, replacing any existing value.
    async fn store(&mut self, key: &str, secret: &str) -> Result<()>;
    /// Retrieve the secret under `key`. Errors if absent.
    async fn retrieve(&self, key: &str) -> Result<String>;
    /// Remove the secret under `key`. No-op if absent.
    async fn delete(&mut self, key: &str) -> Result<()>;
    /// List all stored secret keys (never the values).
    async fn list(&self) -> Result<Vec<String>>;
}

/// Append-only, tamper-evident audit log backed by a hash chain.
///
/// `record` appends an entry (computing its hash); `verify_chain` recomputes
/// every hash and chain link to detect in-place tampering. A human-readable
/// mirror lives in `.argos/audit/audit.log`.
#[async_trait]
pub trait AuditLog: Send + Sync {
    /// Append `entry` to the chain, computing `prev_hash`/`this_hash`.
    async fn record(&mut self, entry: &AuditEntry) -> Result<()>;
    /// Return entries matching `filter`, in chronological order.
    async fn query(&self, filter: &AuditFilter) -> Result<Vec<AuditEntry>>;
    /// Recompute the hash chain; `true` if intact, `false` if tampered.
    async fn verify_chain(&self) -> Result<bool>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use sha2::{Digest, Sha256};
    use std::collections::{HashMap, HashSet};

    // --- PermissionGate stub ---
    struct StubPermissionGate {
        grants: HashSet<(String, String, String)>,
    }
    impl StubPermissionGate {
        fn new() -> Self {
            Self {
                grants: HashSet::new(),
            }
        }
        fn key(s: &str, r: &str, a: &str) -> (String, String, String) {
            (s.into(), r.into(), a.into())
        }
    }
    #[async_trait::async_trait]
    impl PermissionGate for StubPermissionGate {
        async fn check(
            &self,
            subject: &str,
            resource: &str,
            action: &str,
        ) -> argos_core::Result<Permission> {
            if self.grants.contains(&Self::key(subject, resource, action)) {
                Ok(Permission::Allow)
            } else {
                Ok(Permission::Deny(format!(
                    "no grant for {subject}/{resource}/{action}"
                )))
            }
        }
        async fn grant(
            &mut self,
            subject: &str,
            resource: &str,
            action: &str,
        ) -> argos_core::Result<()> {
            self.grants.insert(Self::key(subject, resource, action));
            Ok(())
        }
        async fn revoke(
            &mut self,
            subject: &str,
            resource: &str,
            action: &str,
        ) -> argos_core::Result<()> {
            self.grants.remove(&Self::key(subject, resource, action));
            Ok(())
        }
    }

    // --- SecretVault stub ---
    struct StubVault {
        secrets: HashMap<String, String>,
    }
    impl StubVault {
        fn new() -> Self {
            Self {
                secrets: HashMap::new(),
            }
        }
    }
    #[async_trait::async_trait]
    impl SecretVault for StubVault {
        async fn store(&mut self, key: &str, secret: &str) -> argos_core::Result<()> {
            self.secrets.insert(key.into(), secret.into());
            Ok(())
        }
        async fn retrieve(&self, key: &str) -> argos_core::Result<String> {
            self.secrets
                .get(key)
                .cloned()
                .ok_or_else(|| argos_core::ArgosError::NotFound(format!("secret {key}")))
        }
        async fn delete(&mut self, key: &str) -> argos_core::Result<()> {
            self.secrets.remove(key);
            Ok(())
        }
        async fn list(&self) -> argos_core::Result<Vec<String>> {
            let mut keys: Vec<String> = self.secrets.keys().cloned().collect();
            keys.sort();
            Ok(keys)
        }
    }

    // --- AuditLog stub with a real sha256 hash chain ---
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
    fn hash_of(prev: &str, payload: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(prev.as_bytes());
        hasher.update(payload.as_bytes());
        format!("{:x}", hasher.finalize())
    }
    struct StubAuditLog {
        entries: Vec<AuditEntry>,
    }
    impl StubAuditLog {
        fn new() -> Self {
            Self { entries: vec![] }
        }
    }
    #[async_trait::async_trait]
    impl AuditLog for StubAuditLog {
        async fn record(&mut self, entry: &AuditEntry) -> argos_core::Result<()> {
            let prev_hash = self
                .entries
                .last()
                .map(|e| e.this_hash.clone())
                .unwrap_or_else(|| "0".repeat(64));
            let payload = canonical(entry);
            let this_hash = hash_of(&prev_hash, &payload);
            let mut stored = entry.clone();
            stored.prev_hash = prev_hash;
            stored.this_hash = this_hash;
            self.entries.push(stored);
            Ok(())
        }
        async fn query(&self, filter: &AuditFilter) -> argos_core::Result<Vec<AuditEntry>> {
            let out: Vec<AuditEntry> = self
                .entries
                .iter()
                .filter(|e| {
                    if let Some(since) = &filter.since {
                        if e.timestamp < *since {
                            return false;
                        }
                    }
                    if let Some(subject) = &filter.subject {
                        if &e.subject != subject {
                            return false;
                        }
                    }
                    if let Some(action) = &filter.action {
                        if &e.action != action {
                            return false;
                        }
                    }
                    true
                })
                .cloned()
                .collect();
            Ok(out)
        }
        async fn verify_chain(&self) -> argos_core::Result<bool> {
            let mut prev = "0".repeat(64);
            for e in &self.entries {
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
        }
    }

    fn sample_entry(subject: &str, action: &str) -> AuditEntry {
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

    #[test]
    fn permission_gate_trait_can_be_referenced() {
        let g: &dyn PermissionGate = &StubPermissionGate::new();
        let _ = g;
    }
    #[test]
    fn secret_vault_trait_can_be_referenced() {
        let v: &dyn SecretVault = &StubVault::new();
        let _ = v;
    }
    #[test]
    fn audit_log_trait_can_be_referenced() {
        let l: &dyn AuditLog = &StubAuditLog::new();
        let _ = l;
    }

    #[test]
    fn permission_allow_deny_variants() {
        assert_eq!(Permission::Allow, Permission::Allow);
        match Permission::Deny("no grant".into()) {
            Permission::Deny(reason) => assert_eq!(reason, "no grant"),
            Permission::Allow => panic!("expected Deny"),
        }
        assert_ne!(Permission::Allow, Permission::Deny("x".into()));
    }

    #[test]
    fn audit_entry_constructs() {
        let e = sample_entry("agent-1", "wiki.write");
        assert_eq!(e.subject, "agent-1");
        assert_eq!(e.action, "wiki.write");
        assert_eq!(e.resource, "wiki");
        assert_eq!(e.result, "ok");
    }

    #[test]
    fn audit_filter_constructs_with_none_defaults() {
        let f = AuditFilter::default();
        assert!(f.since.is_none());
        assert!(f.subject.is_none());
        assert!(f.action.is_none());
    }

    #[test]
    fn permission_set_constructs() {
        let set = PermissionSet {
            permissions: vec![("agent".into(), "wiki".into(), "write".into())],
        };
        assert_eq!(set.permissions.len(), 1);
        assert_eq!(set.permissions[0].1, "wiki");
    }

    #[tokio::test]
    async fn stub_permission_gate_grant_allow_deny() {
        let mut g = StubPermissionGate::new();
        // Ungranted -> Deny.
        let denied = g.check("agent", "wiki", "write").await.unwrap();
        assert!(matches!(denied, Permission::Deny(_)));

        // Grant -> Allow.
        g.grant("agent", "wiki", "write").await.unwrap();
        let allowed = g.check("agent", "wiki", "write").await.unwrap();
        assert_eq!(allowed, Permission::Allow);

        // Other action still denied.
        let other = g.check("agent", "wiki", "read").await.unwrap();
        assert!(matches!(other, Permission::Deny(_)));

        // Revoke -> Deny again.
        g.revoke("agent", "wiki", "write").await.unwrap();
        let after = g.check("agent", "wiki", "write").await.unwrap();
        assert!(matches!(after, Permission::Deny(_)));
    }

    #[tokio::test]
    async fn stub_secret_vault_store_retrieve_list_delete() {
        let mut v = StubVault::new();
        v.store("n8n-key", "s3cr3t").await.unwrap();
        assert_eq!(v.retrieve("n8n-key").await.unwrap(), "s3cr3t");
        // Missing secret errors (never returns the raw bytes from SQLite).
        let missing = v.retrieve("nope").await;
        assert!(missing.is_err());

        v.store("openai-key", "sk-abc").await.unwrap();
        let keys = v.list().await.unwrap();
        assert_eq!(keys, vec!["n8n-key".to_string(), "openai-key".to_string()]);

        v.delete("n8n-key").await.unwrap();
        assert!(v.retrieve("n8n-key").await.is_err());
        assert_eq!(v.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn stub_audit_log_record_query_roundtrip() {
        let mut log = StubAuditLog::new();
        log.record(&sample_entry("agent-1", "wiki.write"))
            .await
            .unwrap();
        log.record(&sample_entry("agent-2", "n8n.run"))
            .await
            .unwrap();

        let all = log.query(&AuditFilter::default()).await.unwrap();
        assert_eq!(all.len(), 2);

        let only_agent1 = log
            .query(&AuditFilter {
                since: None,
                subject: Some("agent-1".into()),
                action: None,
            })
            .await
            .unwrap();
        assert_eq!(only_agent1.len(), 1);
        assert_eq!(only_agent1[0].subject, "agent-1");
    }

    #[tokio::test]
    async fn stub_audit_log_verify_chain_valid() {
        let mut log = StubAuditLog::new();
        log.record(&sample_entry("a", "x")).await.unwrap();
        log.record(&sample_entry("b", "y")).await.unwrap();
        log.record(&sample_entry("c", "z")).await.unwrap();
        assert!(log.verify_chain().await.unwrap());
    }

    #[tokio::test]
    async fn stub_audit_log_detects_tamper() {
        let mut log = StubAuditLog::new();
        log.record(&sample_entry("a", "x")).await.unwrap();
        log.record(&sample_entry("b", "y")).await.unwrap();
        assert!(log.verify_chain().await.unwrap());

        // Tamper with the first entry's payload — the recomputed hash no longer matches.
        log.entries[0].result = "tampered".into();
        assert!(!log.verify_chain().await.unwrap());
    }
}
