//! WASM extension runtime trait + stub (Tier-3, deferred in slice 1).
//!
//! Slice 1 ships the `WasmRuntime` trait and a `WasmRuntimeStub` for testing.
//! The real wasmtime backend is gated behind the `wasm-backend` feature.

use argos_core::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// WASM runtime capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmCapabilities {
    pub max_memory_bytes: u64,
    pub max_execution_time_ms: u64,
    pub supports_wasi: bool,
}

impl Default for WasmCapabilities {
    fn default() -> Self {
        Self {
            max_memory_bytes: 64 * 1024 * 1024, // 64 MB
            max_execution_time_ms: 5000,        // 5 seconds
            supports_wasi: false,
        }
    }
}

/// Sandboxed WASM extension runtime (Tier-3, deferred in slice 1).
///
/// Loads and executes WASM modules with memory and execution time limits.
/// The real wasmtime backend is feature-gated; the default stub records
/// operations for testing.
#[async_trait]
pub trait WasmRuntime: Send + Sync {
    /// Start the runtime.
    async fn start(&mut self) -> Result<()>;
    /// Stop the runtime. Any loaded modules are cleared.
    async fn stop(&mut self) -> Result<()>;
    /// Load a WASM module from bytes.
    async fn load_module(&mut self, name: &str, wasm_bytes: &[u8]) -> Result<()>;
    /// Execute a function in a loaded module.
    async fn execute(&self, module: &str, function: &str, args: &[u8]) -> Result<Vec<u8>>;
    /// List loaded module names.
    fn modules(&self) -> Vec<String>;
    /// Runtime capabilities.
    fn capabilities(&self) -> WasmCapabilities;
}

/// Stub WASM runtime that records operations for testing.
///
/// start/stop/execute always succeed. load_module records the module name.
/// execute always returns an empty vec.
pub struct WasmRuntimeStub {
    modules: Vec<String>,
}

impl WasmRuntimeStub {
    pub fn new() -> Self {
        Self { modules: vec![] }
    }
}

impl Default for WasmRuntimeStub {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WasmRuntime for WasmRuntimeStub {
    async fn start(&mut self) -> Result<()> {
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.modules.clear();
        Ok(())
    }

    async fn load_module(&mut self, name: &str, _wasm_bytes: &[u8]) -> Result<()> {
        self.modules.push(name.to_string());
        Ok(())
    }

    async fn execute(&self, _module: &str, _function: &str, _args: &[u8]) -> Result<Vec<u8>> {
        Ok(vec![])
    }

    fn modules(&self) -> Vec<String> {
        self.modules.clone()
    }

    fn capabilities(&self) -> WasmCapabilities {
        WasmCapabilities::default()
    }
}

/// Real wasmtime backend (feature-gated, skeleton only — deferred to Phase 2).
#[cfg(feature = "wasm-backend")]
pub struct WasmtimeRuntime {
    #[allow(dead_code)]
    engine: wasmtime::Engine,
    #[allow(dead_code)]
    module_names: Vec<String>,
}

#[cfg(feature = "wasm-backend")]
impl WasmtimeRuntime {
    pub fn new() -> Self {
        Self {
            engine: wasmtime::Engine::default(),
            module_names: Vec::new(),
        }
    }
}

#[cfg(feature = "wasm-backend")]
impl Default for WasmtimeRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "wasm-backend")]
#[async_trait]
impl WasmRuntime for WasmtimeRuntime {
    async fn start(&mut self) -> Result<()> {
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.module_names.clear();
        Ok(())
    }

    async fn load_module(&mut self, name: &str, _wasm_bytes: &[u8]) -> Result<()> {
        // Real module compilation deferred to Phase 2
        self.module_names.push(name.to_string());
        Ok(())
    }

    async fn execute(&self, _module: &str, _function: &str, _args: &[u8]) -> Result<Vec<u8>> {
        Ok(vec![])
    }

    fn modules(&self) -> Vec<String> {
        self.module_names.clone()
    }

    fn capabilities(&self) -> WasmCapabilities {
        WasmCapabilities::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_starts_and_stops() {
        let mut rt = WasmRuntimeStub::new();
        rt.start().await.unwrap();
        rt.stop().await.unwrap();
        assert!(rt.modules().is_empty());
    }

    #[tokio::test]
    async fn stub_loads_module() {
        let mut rt = WasmRuntimeStub::new();
        rt.start().await.unwrap();
        rt.load_module("test_mod", b"wasm_bytes").await.unwrap();
        assert_eq!(rt.modules(), vec!["test_mod"]);
    }

    #[tokio::test]
    async fn stub_execute_returns_ok() {
        let mut rt = WasmRuntimeStub::new();
        rt.start().await.unwrap();
        rt.load_module("test_mod", b"wasm_bytes").await.unwrap();
        let result = rt.execute("test_mod", "main", &[]).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn stub_capabilities_are_default() {
        let rt = WasmRuntimeStub::new();
        let caps = rt.capabilities();
        assert_eq!(caps.max_memory_bytes, 64 * 1024 * 1024);
        assert_eq!(caps.max_execution_time_ms, 5000);
        assert!(!caps.supports_wasi);
    }

    #[tokio::test]
    async fn stub_stop_clears_modules() {
        let mut rt = WasmRuntimeStub::new();
        rt.start().await.unwrap();
        rt.load_module("a", b"x").await.unwrap();
        rt.load_module("b", b"x").await.unwrap();
        assert_eq!(rt.modules().len(), 2);
        rt.stop().await.unwrap();
        assert!(rt.modules().is_empty());
    }

    #[tokio::test]
    async fn stub_multiple_modules() {
        let mut rt = WasmRuntimeStub::new();
        rt.start().await.unwrap();
        rt.load_module("alpha", b"x").await.unwrap();
        rt.load_module("beta", b"x").await.unwrap();
        rt.load_module("gamma", b"x").await.unwrap();
        assert_eq!(rt.modules(), vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn wasm_capabilities_serialization_roundtrip() {
        let caps = WasmCapabilities {
            max_memory_bytes: 128 * 1024 * 1024,
            max_execution_time_ms: 10000,
            supports_wasi: true,
        };
        let json = serde_json::to_string(&caps).unwrap();
        let back: WasmCapabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(back, caps);
    }

    #[test]
    fn wasm_capabilities_default() {
        let caps = WasmCapabilities::default();
        assert_eq!(caps.max_memory_bytes, 64 * 1024 * 1024);
        assert_eq!(caps.max_execution_time_ms, 5000);
        assert!(!caps.supports_wasi);
    }
}
