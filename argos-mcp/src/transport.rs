//! MCP transport abstraction — stdio vs HTTP.
//!
//! The [`McpTransport`] trait decouples the JSON-RPC protocol handler from the
//! wire. Slice 1 ships [`StdioTransport`] (stdin/stdout line-delimited JSON).
//! [`StubTransport`] lives in `#[cfg(test)]` so the server and client can be
//! tested deterministically without real I/O.
//!
//! HTTP/SSE transport is feature-gated behind `http-transport` (future).

use argos_core::Result;
use async_trait::async_trait;
use std::io::{BufRead, BufReader, Write};

#[cfg(test)]
use std::sync::Mutex;

/// Transport seam for MCP JSON-RPC message exchange.
///
/// Messages are line-delimited JSON strings. `read_message` returns `Ok(None)`
/// on EOF (graceful disconnect). `write_message` sends a single line.
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// Read the next line-delimited JSON-RPC message.
    ///
    /// Returns `Ok(None)` when the transport is closed (EOF).
    async fn read_message(&self) -> Result<Option<String>>;
    /// Write a line-delimited JSON-RPC message.
    async fn write_message(&self, msg: &str) -> Result<()>;
}

/// Stdio transport — reads from stdin, writes to stdout.
///
/// Each message is a single JSON line terminated by `\n`.
pub struct StdioTransport;

#[async_trait]
impl McpTransport for StdioTransport {
    async fn read_message(&self) -> Result<Option<String>> {
        let stdin = std::io::stdin();
        let mut line = String::new();
        let n = BufReader::new(stdin.lock()).read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim_end().to_string()))
    }

    async fn write_message(&self, msg: &str) -> Result<()> {
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "{msg}")?;
        stdout.flush()?;
        Ok(())
    }
}

/// Stub transport for tests — holds canned incoming messages and captures
/// outgoing messages. No real I/O. Fully deterministic.
#[cfg(test)]
#[derive(Default)]
pub struct StubTransport {
    /// Messages that `read_message` will return, in order.
    incoming: Mutex<Vec<String>>,
    /// Messages written via `write_message`, in order.
    outgoing: Mutex<Vec<String>>,
}

#[cfg(test)]
impl StubTransport {
    /// Create a stub pre-loaded with `messages` for `read_message`.
    pub fn with_messages(messages: Vec<String>) -> Self {
        Self {
            incoming: Mutex::new(messages),
            outgoing: Mutex::new(Vec::new()),
        }
    }

    /// Drain and return every message sent via `write_message`.
    pub fn take_outgoing(&self) -> Vec<String> {
        std::mem::take(&mut *self.outgoing.lock().unwrap())
    }
}

#[cfg(test)]
#[async_trait]
impl McpTransport for StubTransport {
    async fn read_message(&self) -> Result<Option<String>> {
        let mut incoming = self.incoming.lock().unwrap();
        if incoming.is_empty() {
            Ok(None)
        } else {
            Ok(Some(incoming.remove(0)))
        }
    }

    async fn write_message(&self, msg: &str) -> Result<()> {
        self.outgoing.lock().unwrap().push(msg.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stub_transport_reads_messages_in_order() {
        let transport = StubTransport::with_messages(vec![
            r#"{"method":"initialize"}"#.into(),
            r#"{"method":"tools/list"}"#.into(),
        ]);

        let msg1 = transport.read_message().await.unwrap();
        assert_eq!(msg1, Some(r#"{"method":"initialize"}"#.into()));

        let msg2 = transport.read_message().await.unwrap();
        assert_eq!(msg2, Some(r#"{"method":"tools/list"}"#.into()));

        // Third read returns None (EOF).
        let msg3 = transport.read_message().await.unwrap();
        assert!(msg3.is_none(), "empty stub should return None");
    }

    #[tokio::test]
    async fn stub_transport_returns_none_when_empty() {
        let transport = StubTransport::with_messages(vec![]);
        let msg = transport.read_message().await.unwrap();
        assert!(msg.is_none());
    }

    #[tokio::test]
    async fn stub_transport_captures_outgoing_messages() {
        let transport = StubTransport::with_messages(vec![]);
        transport.write_message(r#"{"result":"ok"}"#).await.unwrap();
        transport
            .write_message(r#"{"result":"also ok"}"#)
            .await
            .unwrap();

        let outgoing = transport.take_outgoing();
        assert_eq!(outgoing.len(), 2);
        assert_eq!(outgoing[0], r#"{"result":"ok"}"#);
        assert_eq!(outgoing[1], r#"{"result":"also ok"}"#);
    }

    #[tokio::test]
    async fn stub_transport_roundtrip() {
        let transport = StubTransport::with_messages(vec![
            r#"{"method":"tools/call","params":{"name":"wiki.query","args":"{\"q\":\"rust\"}"}}"#
                .into(),
        ]);

        // Read the request
        let request = transport.read_message().await.unwrap().unwrap();
        assert!(request.contains("tools/call"));

        // Write a response
        transport
            .write_message(r#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"text":"answer"}]}}"#)
            .await
            .unwrap();

        let outgoing = transport.take_outgoing();
        assert_eq!(outgoing.len(), 1);
        assert!(outgoing[0].contains("answer"));
    }

    #[tokio::test]
    async fn stub_transport_eof_after_exhausting_messages() {
        let transport = StubTransport::with_messages(vec!["msg1".into()]);
        let first = transport.read_message().await.unwrap();
        assert!(first.is_some());
        let second = transport.read_message().await.unwrap();
        assert!(second.is_none());
        let third = transport.read_message().await.unwrap();
        assert!(third.is_none());
    }
}
