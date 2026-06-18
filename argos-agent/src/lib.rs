//! ArgOS agent runtime.
//!
//! Agent lifecycle (observe -> think -> act tool-call loop), Hand specialization,
//! and Tier-1 compiled tools backed by the knowledge system and n8n connector.
//! The [`agent::Agent`] trait is the core abstraction; lifecycle state is modelled
//! by `argos_core::AgentState` whose transition table encodes the loop. Slice 1
//! ships a single generic agent (`Hand::None`). Implementation lands in later tasks.

pub mod agent;
pub mod registry;

pub use agent::{Agent, AgentOutput};
pub use registry::{ToolHandler, ToolInfo, ToolRegistry};

#[cfg(test)]
mod test_support;
