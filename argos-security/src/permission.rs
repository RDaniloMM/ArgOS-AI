//! In-memory permission gate implementation (ADR-006).
//!
//! [`MatrixPermissionGate`] holds a set of `(subject, resource, action)` grants
//! and enforces **default-deny**: any check for a grant that is not present is
//! denied. Slice 1 uses exact-match only; wildcard matching is deferred to a
//! later slice (the [`PermissionGate`] seam allows it). Every tool invocation,
//! wiki mutation, n8n call, and ArgOS-MCP-server tool exposure routes through
//! this gate (spec `wiki-write-requires-permission`).

use std::collections::HashSet;

use argos_core::Result;
use async_trait::async_trait;

use crate::traits::{Permission, PermissionGate};

/// In-memory `(subject, resource, action)` permission matrix with default-deny.
pub struct MatrixPermissionGate {
    grants: HashSet<(String, String, String)>,
}

impl MatrixPermissionGate {
    /// Create an empty gate (everything denied by default).
    pub fn new() -> Self {
        Self {
            grants: HashSet::new(),
        }
    }
}

impl Default for MatrixPermissionGate {
    fn default() -> Self {
        Self::new()
    }
}

fn key(subject: &str, resource: &str, action: &str) -> (String, String, String) {
    (
        subject.to_string(),
        resource.to_string(),
        action.to_string(),
    )
}

#[async_trait]
impl PermissionGate for MatrixPermissionGate {
    async fn check(&self, subject: &str, resource: &str, action: &str) -> Result<Permission> {
        if self.grants.contains(&key(subject, resource, action)) {
            Ok(Permission::Allow)
        } else {
            // Default-deny: no grant present -> denied with a human-readable
            // reason that references the denied triple.
            Ok(Permission::Deny(format!(
                "no permission granted for {subject}/{resource}/{action}"
            )))
        }
    }

    async fn grant(&mut self, subject: &str, resource: &str, action: &str) -> Result<()> {
        self.grants.insert(key(subject, resource, action));
        Ok(())
    }

    async fn revoke(&mut self, subject: &str, resource: &str, action: &str) -> Result<()> {
        // Removing (rather than storing `false`) keeps absence == denied, which
        // is the default-deny invariant. No-op if the grant was never present.
        self.grants.remove(&key(subject, resource, action));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::permission::MatrixPermissionGate;
    use crate::traits::{Permission, PermissionGate};

    #[tokio::test]
    async fn check_ungranted_returns_deny() {
        // Default-deny: an ungranted permission is denied.
        let gate = MatrixPermissionGate::new();
        let decision = gate.check("agent", "wiki", "write").await.unwrap();
        assert!(matches!(decision, Permission::Deny(_)));
    }

    #[tokio::test]
    async fn grant_then_check_returns_allow() {
        let mut gate = MatrixPermissionGate::new();
        gate.grant("agent", "wiki", "write").await.unwrap();
        let decision = gate.check("agent", "wiki", "write").await.unwrap();
        assert_eq!(decision, Permission::Allow);
    }

    #[tokio::test]
    async fn revoke_then_check_returns_deny() {
        let mut gate = MatrixPermissionGate::new();
        gate.grant("agent", "wiki", "write").await.unwrap();
        assert_eq!(
            gate.check("agent", "wiki", "write").await.unwrap(),
            Permission::Allow
        );
        gate.revoke("agent", "wiki", "write").await.unwrap();
        let decision = gate.check("agent", "wiki", "write").await.unwrap();
        assert!(matches!(decision, Permission::Deny(_)));
    }

    #[tokio::test]
    async fn grant_different_subjects_independently() {
        let mut gate = MatrixPermissionGate::new();
        gate.grant("agent-1", "wiki", "read").await.unwrap();
        // agent-2 has no grant even with the same resource/action.
        assert!(matches!(
            gate.check("agent-2", "wiki", "read").await.unwrap(),
            Permission::Deny(_)
        ));
        assert_eq!(
            gate.check("agent-1", "wiki", "read").await.unwrap(),
            Permission::Allow
        );
    }

    #[tokio::test]
    async fn grant_different_resources_independently() {
        let mut gate = MatrixPermissionGate::new();
        gate.grant("agent", "wiki", "read").await.unwrap();
        // A different resource is not covered by the wiki grant.
        assert!(matches!(
            gate.check("agent", "n8n", "read").await.unwrap(),
            Permission::Deny(_)
        ));
        assert_eq!(
            gate.check("agent", "wiki", "read").await.unwrap(),
            Permission::Allow
        );
    }

    #[tokio::test]
    async fn grant_different_actions_independently() {
        let mut gate = MatrixPermissionGate::new();
        gate.grant("agent", "wiki", "read").await.unwrap();
        // A different action on the same resource is not covered.
        assert!(matches!(
            gate.check("agent", "wiki", "write").await.unwrap(),
            Permission::Deny(_)
        ));
        assert_eq!(
            gate.check("agent", "wiki", "read").await.unwrap(),
            Permission::Allow
        );
    }

    #[tokio::test]
    async fn revoke_never_granted_is_idempotent() {
        // Revoking a permission that was never granted must not error.
        let mut gate = MatrixPermissionGate::new();
        gate.revoke("ghost", "wiki", "write").await.unwrap();
    }

    #[tokio::test]
    async fn deny_carries_reason_string() {
        let gate = MatrixPermissionGate::new();
        let decision = gate.check("agent", "wiki", "write").await.unwrap();
        match decision {
            Permission::Deny(reason) => {
                assert!(!reason.is_empty());
                // The reason references the denied subject/resource/action.
                assert!(reason.contains("agent"));
                assert!(reason.contains("wiki"));
                assert!(reason.contains("write"));
            }
            Permission::Allow => panic!("expected Deny with a reason"),
        }
    }
}
