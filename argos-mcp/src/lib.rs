//! ArgOS MCP — bidirectional Model Context Protocol.
//!
//! **T-030 (server)**: ArgOS exposes `wiki.query`, `workflow.recommend_reuse`,
//! and `workflow.similar` as MCP tools to n8n and other MCP clients, bound to a
//! local socket.
//!
//! **T-031 (client)**: ArgOS discovers and consumes n8n's MCP tools
//! (`list_workflows`, `run_workflow`) via the MCP JSON-RPC 2.0 protocol,
//! providing an alternative transport to the REST client.
//!
//! Both directions are permission-gated and audited. Slice 1 uses stdio
//! transport; HTTP/SSE is feature-gated behind `http-transport`.

pub mod client;
pub mod client_impl;
pub mod n8n_adapter;
pub mod protocol;
pub mod server;
pub mod server_impl;
pub mod transport;
pub mod types;
