//! ArgOS security layer.
//!
//! Cross-cutting concerns: permission gating ([`traits::PermissionGate`]), secret
//! management ([`traits::SecretVault`], incl. n8n credentials), and tamper-evident
//! audit logging ([`traits::AuditLog`]). These wrap every action. Implementation
//! lands in later tasks.

pub mod traits;

pub use traits::{
    AuditEntry, AuditFilter, AuditLog, Permission, PermissionGate, PermissionSet, SecretVault,
};
