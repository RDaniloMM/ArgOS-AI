//! Secret vault implementations (ADR-006).
//!
//! [`MemoryVault`] is the in-memory backend used by tests and as a stub — it
//! is non-persistent and MUST NOT be used to store real secrets in production.
//! [`KeyringVault`] wraps the OS keyring (Windows Credential Manager, macOS
//! Keychain, Linux Secret Service) and is the production-preferred backend; it
//! is feature-gated behind `keyring-backend` so the default build stays
//! platform-free. Secret material never lives in SQLite, `config.toml`, or
//! logs (ADR-006 / spec `secret-access-via-keyring`).

use std::collections::HashMap;

use argos_core::{ArgosError, Result};
use async_trait::async_trait;

use crate::traits::SecretVault;

/// In-memory [`SecretVault`] backed by a `HashMap`.
///
/// Non-persistent; intended for tests and stubs. Production MUST use
/// [`KeyringVault`] (feature `keyring-backend`).
pub struct MemoryVault {
    secrets: HashMap<String, String>,
}

impl MemoryVault {
    /// Create an empty vault.
    pub fn new() -> Self {
        Self {
            secrets: HashMap::new(),
        }
    }
}

impl Default for MemoryVault {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SecretVault for MemoryVault {
    async fn store(&mut self, key: &str, secret: &str) -> Result<()> {
        self.secrets.insert(key.to_string(), secret.to_string());
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<String> {
        self.secrets
            .get(key)
            .cloned()
            .ok_or_else(|| ArgosError::NotFound(format!("secret {key}")))
    }

    async fn delete(&mut self, key: &str) -> Result<()> {
        // No-op if absent (trait contract, consistent with the traits.rs stub).
        self.secrets.remove(key);
        Ok(())
    }

    async fn list(&self) -> Result<Vec<String>> {
        let mut keys: Vec<String> = self.secrets.keys().cloned().collect();
        keys.sort();
        Ok(keys)
    }
}

/// OS-keyring-backed [`SecretVault`] (feature `keyring-backend`).
///
/// Uses the `keyring` crate: Windows Credential Manager, macOS Keychain, or
/// Linux Secret Service. The OS keyring API has no enumeration primitive, so
/// [`SecretVault::list`] returns an empty vector (a sidecar index can be added
/// later); store/retrieve/delete map directly to the keyring entry operations.
#[cfg(feature = "keyring-backend")]
pub struct KeyringVault {
    service: String,
}

#[cfg(feature = "keyring-backend")]
impl KeyringVault {
    /// Create a vault scoped to `service` (the keyring "service" name).
    pub fn new(service: &str) -> Self {
        Self {
            service: service.to_string(),
        }
    }
}

#[async_trait]
#[cfg(feature = "keyring-backend")]
impl SecretVault for KeyringVault {
    async fn store(&mut self, key: &str, secret: &str) -> Result<()> {
        let entry = keyring::Entry::new(&self.service, key)
            .map_err(|e| ArgosError::Security(format!("keyring entry: {e}")))?;
        entry
            .set_password(secret)
            .map_err(|e| ArgosError::Security(format!("keyring store: {e}")))
    }

    async fn retrieve(&self, key: &str) -> Result<String> {
        let entry = keyring::Entry::new(&self.service, key)
            .map_err(|e| ArgosError::Security(format!("keyring entry: {e}")))?;
        entry
            .get_password()
            .map_err(|e| ArgosError::Security(format!("keyring retrieve: {e}")))
    }

    async fn delete(&mut self, key: &str) -> Result<()> {
        let entry = keyring::Entry::new(&self.service, key)
            .map_err(|e| ArgosError::Security(format!("keyring entry: {e}")))?;
        // `delete_credential` is a no-op if the entry is absent (keyring crate
        // returns a NoEntryAccessError which we normalize to Ok).
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(ArgosError::Security(format!("keyring delete: {e}"))),
        }
    }

    async fn list(&self) -> Result<Vec<String>> {
        // The OS keyring API exposes no enumeration; return an empty list. A
        // sidecar index can maintain known keys in a later slice.
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use crate::traits::SecretVault;
    use crate::vault::MemoryVault;

    #[tokio::test]
    async fn memory_vault_store_then_retrieve_returns_same_secret() {
        let mut v = MemoryVault::new();
        v.store("n8n-key", "s3cr3t").await.unwrap();
        assert_eq!(v.retrieve("n8n-key").await.unwrap(), "s3cr3t");
    }

    #[tokio::test]
    async fn memory_vault_retrieve_missing_returns_error() {
        let v = MemoryVault::new();
        assert!(v.retrieve("nope").await.is_err());
    }

    #[tokio::test]
    async fn memory_vault_delete_removes_key() {
        let mut v = MemoryVault::new();
        v.store("k", "v").await.unwrap();
        assert!(v.retrieve("k").await.is_ok());
        v.delete("k").await.unwrap();
        assert!(v.retrieve("k").await.is_err());
    }

    #[tokio::test]
    async fn memory_vault_list_returns_all_keys() {
        let mut v = MemoryVault::new();
        v.store("alpha", "1").await.unwrap();
        v.store("beta", "2").await.unwrap();
        let keys = v.list().await.unwrap();
        // Sorted for determinism.
        assert_eq!(keys, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[tokio::test]
    async fn memory_vault_delete_missing_is_ok_idempotent() {
        // The SecretVault trait says delete is "No-op if absent"; the in-memory
        // stub in traits.rs also returns Ok. MemoryVault stays consistent.
        let mut v = MemoryVault::new();
        v.delete("ghost").await.unwrap();
    }

    #[cfg(feature = "keyring-backend")]
    #[tokio::test]
    async fn keyring_vault_constructs() {
        // We only verify construction — real keyring ops need an OS keyring and
        // are not unit-testable in CI without platform fixtures.
        let _vault = crate::vault::KeyringVault::new("argos-test");
    }
}
