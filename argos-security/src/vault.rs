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

#[cfg(feature = "keyring-backend")]
const CHUNKED_SECRET_PREFIX: &str = "ARGOS_CHUNKED_SECRET_V1:";
#[cfg(feature = "keyring-backend")]
// Windows Credential Manager limits payload to 2560 bytes when UTF-16 encoded.
// Each UTF-16 code unit = 2 bytes, so max safe chars = 2560/2 = 1280.
// We use 1000 to leave headroom for non-ASCII and future keyring overhead.
const KEYRING_SECRET_CHUNK_CHARS: usize = 1_000;

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

    fn entry(&self, key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, key)
            .map_err(|e| ArgosError::Security(format!("keyring entry: {e}")))
    }

    fn chunk_key(key: &str, index: usize) -> String {
        format!("{key}.__argos_chunk.{index}")
    }

    fn chunk_marker(count: usize) -> String {
        format!("{CHUNKED_SECRET_PREFIX}{count}")
    }

    fn parse_chunk_marker(secret: &str) -> Option<usize> {
        secret
            .strip_prefix(CHUNKED_SECRET_PREFIX)
            .and_then(|count| count.parse::<usize>().ok())
    }

    fn secret_chunks(secret: &str) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut current = String::new();

        for ch in secret.chars() {
            if current.chars().count() >= KEYRING_SECRET_CHUNK_CHARS {
                chunks.push(current);
                current = String::new();
            }
            current.push(ch);
        }

        if !current.is_empty() || secret.is_empty() {
            chunks.push(current);
        }

        chunks
    }

    fn delete_entry_if_present(&self, key: &str) -> Result<()> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(ArgosError::Security(format!("keyring delete: {e}"))),
        }
    }

    fn delete_chunk_entries(&self, key: &str, count: usize) -> Result<()> {
        for index in 0..count {
            self.delete_entry_if_present(&Self::chunk_key(key, index))?;
        }
        Ok(())
    }

    fn delete_existing_chunks(&self, key: &str) -> Result<()> {
        let Ok(existing) = self.entry(key)?.get_password() else {
            return Ok(());
        };
        if let Some(count) = Self::parse_chunk_marker(&existing) {
            self.delete_chunk_entries(key, count)?;
        }
        Ok(())
    }
}

#[async_trait]
#[cfg(feature = "keyring-backend")]
impl SecretVault for KeyringVault {
    async fn store(&mut self, key: &str, secret: &str) -> Result<()> {
        self.delete_existing_chunks(key)?;

        if secret.chars().count() <= KEYRING_SECRET_CHUNK_CHARS {
            return self
                .entry(key)?
                .set_password(secret)
                .map_err(|e| ArgosError::Security(format!("keyring store: {e}")));
        }

        let chunks = Self::secret_chunks(secret);
        for (index, chunk) in chunks.iter().enumerate() {
            self.entry(&Self::chunk_key(key, index))?
                .set_password(chunk)
                .map_err(|e| ArgosError::Security(format!("keyring chunk store: {e}")))?;
        }

        self.entry(key)?
            .set_password(&Self::chunk_marker(chunks.len()))
            .map_err(|e| ArgosError::Security(format!("keyring store: {e}")))
    }

    async fn retrieve(&self, key: &str) -> Result<String> {
        let secret = self
            .entry(key)?
            .get_password()
            .map_err(|e| ArgosError::Security(format!("keyring retrieve: {e}")))?;

        let Some(count) = Self::parse_chunk_marker(&secret) else {
            return Ok(secret);
        };

        let mut combined = String::new();
        for index in 0..count {
            let chunk = self
                .entry(&Self::chunk_key(key, index))?
                .get_password()
                .map_err(|e| ArgosError::Security(format!("keyring chunk retrieve: {e}")))?;
            combined.push_str(&chunk);
        }
        Ok(combined)
    }

    async fn delete(&mut self, key: &str) -> Result<()> {
        if let Ok(existing) = self.entry(key)?.get_password() {
            if let Some(count) = Self::parse_chunk_marker(&existing) {
                self.delete_chunk_entries(key, count)?;
            }
        }
        // `delete_credential` is a no-op if the entry is absent (keyring crate
        // returns a NoEntryAccessError which we normalize to Ok).
        self.delete_entry_if_present(key)
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

    #[cfg(feature = "keyring-backend")]
    use crate::vault::KeyringVault;

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
        let _vault = KeyringVault::new("argos-test");
    }

    #[cfg(feature = "keyring-backend")]
    #[test]
    fn keyring_chunking_splits_and_marks_large_secrets() {
        let secret = "x".repeat(super::KEYRING_SECRET_CHUNK_CHARS + 7);
        let chunks = KeyringVault::secret_chunks(&secret);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks.concat(), secret);

        let marker = KeyringVault::chunk_marker(chunks.len());
        assert_eq!(KeyringVault::parse_chunk_marker(&marker), Some(2));
        assert_eq!(KeyringVault::parse_chunk_marker(&secret), None);
    }
}
