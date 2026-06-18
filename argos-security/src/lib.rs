//! ArgOS security layer.
//!
//! Cross-cutting concerns: permission gating (PermissionGate), secret management
//! (SecretVault, incl. n8n credentials), and tamper-evident audit logging
//! (AuditLog). Wraps every action. Implementation lands in later tasks.
