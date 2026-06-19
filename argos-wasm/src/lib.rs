//! ArgOS WASM extension tier.
//!
//! Sandboxed community skills via wasmtime (Tier-3, deferred in slice 1).
//! Slice 1 ships the `WasmRuntime` trait + stub; the real wasmtime backend is
//! gated behind the `wasm-backend` feature.

pub mod runtime;

#[cfg(feature = "wasm-backend")]
pub use runtime::WasmtimeRuntime;
pub use runtime::{WasmCapabilities, WasmRuntime, WasmRuntimeStub};
