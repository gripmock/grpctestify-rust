#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! Real protocol-level LSP tests — closes `lsp-ide` §6.1. Every other "LSP
//! test" in this repo (`lsp_server_tests.rs`, `lsp_variable_*_tests.rs`,
//! `src/lsp/handlers.rs`'s own unit tests) calls the underlying pure
//! functions directly, bypassing `tower_lsp`'s request dispatch entirely —
//! that's exactly the class of bug the `lsp-ide` audit found once
//! (`SectionType::Bench` completions existed and had a passing unit test,
//! but the `completion()` match arm never listed `Bench`, so real editor
//! completion silently returned nothing for BENCH). A unit test on
//! `get_section_key_completions` could never have caught that; only driving
//! the actual `textDocument/completion` request through the real
//! `LanguageServer` impl can.
//!
//! This spins up a real `tower_lsp::LspService`/`Server` pair connected by
//! an in-memory duplex pipe (the standard `tower-lsp` test pattern — no
//! stdio, no subprocess) and speaks real LSP JSON-RPC (`Content-Length`
//! framed) over it, acting as the client.

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tower_lsp::{LspService, Server};

use grpctestify::lsp::GrpctestifyLsp;

/// One end of the duplex pipe pair, framed as an LSP client would write to
/// the server and read its responses.
struct LspClient {
    write: DuplexStream,
    read: BufReader<DuplexStream>,
    next_id: i64,
}

impl LspClient {
    async fn write_message(&mut self, msg: &Value) {
        let body = serde_json::to_vec(msg).unwrap();
        self.write
            .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
            .await
            .unwrap();
        self.write.write_all(&body).await.unwrap();
    }

    async fn read_message(&mut self) -> Value {
        let mut content_length: usize = 0;
        loop {
            let mut line = String::new();
            loop {
                let mut byte = [0u8; 1];
                self.read.read_exact(&mut byte).await.unwrap();
                line.push(byte[0] as char);
                if line.ends_with("\r\n") {
                    break;
                }
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }
            if let Some(len) = trimmed.strip_prefix("Content-Length: ") {
                content_length = len.parse().unwrap();
            }
        }
        let mut body = vec![0u8; content_length];
        self.read.read_exact(&mut body).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    /// Send a request and read messages until the response with a matching
    /// `id` arrives, discarding any server->client notifications
    /// (`window/logMessage`, `textDocument/publishDiagnostics`, ...) along
    /// the way — a real client would route those elsewhere, not block on
    /// them.
    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await;
        loop {
            let msg = self.read_message().await;
            if msg.get("id") == Some(&json!(id)) {
                return msg["result"].clone();
            }
        }
    }

    /// Read messages until a notification with a matching `method` arrives,
    /// discarding anything else (e.g. an unrelated `window/logMessage`).
    async fn wait_for_notification(&mut self, method: &str) -> Value {
        loop {
            let msg = self.read_message().await;
            if msg.get("method") == Some(&json!(method)) {
                return msg["params"].clone();
            }
        }
    }

    async fn notify(&mut self, method: &str, params: Value) {
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .await;
    }
}

/// Spin up a real server task wired to one end of the pipe, hand back a
/// client already past `initialize`/`initialized` with `content` open as
/// `file:///test.gctf`.
async fn start_and_open(content: &str) -> LspClient {
    let (client_write, server_read) = tokio::io::duplex(64 * 1024);
    let (server_write, client_read) = tokio::io::duplex(64 * 1024);

    let (service, socket) = LspService::new(GrpctestifyLsp::new);
    tokio::spawn(async move {
        Server::new(server_read, server_write, socket)
            .serve(service)
            .await;
    });

    let mut client = LspClient {
        write: client_write,
        read: BufReader::new(client_read),
        next_id: 1,
    };

    client
        .request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": null,
                "capabilities": {},
            }),
        )
        .await;
    client.notify("initialized", json!({})).await;

    client
        .notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": "file:///test.gctf",
                    "languageId": "gctf",
                    "version": 1,
                    "text": content,
                }
            }),
        )
        .await;

    client
}

/// Position on the (empty) line right after a section header — landing on
/// the header line itself would trigger section-header completions instead
/// of the section-body completions each test actually wants to exercise.
fn completion_position_in_body(content: &str, header_needle: &str) -> Value {
    let header_line = content
        .lines()
        .position(|l| l.contains(header_needle))
        .unwrap_or_else(|| panic!("no line contains {header_needle:?} in:\n{content}"));
    json!({
        "textDocument": {"uri": "file:///test.gctf"},
        "position": {"line": (header_line + 1) as u32, "character": 0},
    })
}

fn completion_labels(result: &Value) -> Vec<String> {
    let items = result
        .as_array()
        .cloned()
        .or_else(|| result.get("items").and_then(|i| i.as_array()).cloned())
        .unwrap_or_default();
    items
        .iter()
        .filter_map(|i| i.get("label").and_then(|l| l.as_str()))
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn asserts_completion_offers_a_builtin_plugin_over_real_dispatch() {
    let content = "--- ENDPOINT ---\ntest.Service/Method\n\n--- ASSERTS ---\n\n";
    let mut client = start_and_open(content).await;

    let result = client
        .request(
            "textDocument/completion",
            completion_position_in_body(content, "--- ASSERTS ---"),
        )
        .await;
    let labels = completion_labels(&result);
    assert!(
        labels.iter().any(|l| l == "@is_uuid(...)"),
        "expected @is_uuid(...) in ASSERTS completions, got: {labels:?}"
    );
}

/// Regression anchor for the exact bug class `lsp-ide` §4.1 found: the
/// underlying `get_section_key_completions(&SectionType::Bench)` function
/// had a passing unit test the whole time — only the `completion()`
/// dispatch's match arm was missing `SectionType::Bench`. This test would
/// have caught it; the unit test on the pure function could not.
#[tokio::test]
async fn bench_completion_offers_canonical_keys_over_real_dispatch() {
    let content = "--- BENCH ---\n\n--- ENDPOINT ---\ntest.Service/Method\n\n--- REQUEST ---\n{}\n";
    let mut client = start_and_open(content).await;

    let result = client
        .request(
            "textDocument/completion",
            completion_position_in_body(content, "--- BENCH ---"),
        )
        .await;
    let labels = completion_labels(&result);
    assert!(
        labels.iter().any(|l| l.starts_with("concurrency")),
        "expected a 'concurrency' BENCH key completion over real dispatch, got: {labels:?}"
    );
}

#[tokio::test]
async fn hover_resolves_a_plugin_call_over_real_dispatch() {
    let content = "--- ENDPOINT ---\ntest.Service/Method\n\n--- ASSERTS ---\n@is_uuid(.id)\n";
    let mut client = start_and_open(content).await;

    let line = content
        .lines()
        .position(|l| l.contains("@is_uuid"))
        .unwrap() as u32;
    let result = client
        .request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///test.gctf"},
                "position": {"line": line, "character": 2},
            }),
        )
        .await;
    assert!(!result.is_null(), "expected hover content for @is_uuid");
    let text = result.to_string();
    assert!(
        text.to_lowercase().contains("uuid"),
        "hover content should mention uuid: {text}"
    );
}

#[tokio::test]
async fn code_action_offers_a_quick_fix_for_deprecated_retry_delay_spelling_over_real_dispatch() {
    let content = "--- OPTIONS ---\nretry-delay: 0.3\n\n--- ENDPOINT ---\ntest.Service/Method\n\n--- REQUEST ---\n{}\n";
    let mut client = start_and_open(content).await;

    let diagnostics = client
        .wait_for_notification("textDocument/publishDiagnostics")
        .await;
    let diag = diagnostics["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == json!("DEPRECATED_KEY_SPELLING"))
        .unwrap_or_else(|| panic!("no DEPRECATED_KEY_SPELLING diagnostic in {diagnostics}"))
        .clone();

    let result = client
        .request(
            "textDocument/codeAction",
            json!({
                "textDocument": {"uri": "file:///test.gctf"},
                "range": diag["range"],
                "context": {"diagnostics": [diag]},
            }),
        )
        .await;
    let actions = result.as_array().cloned().unwrap_or_default();
    assert!(
        actions
            .iter()
            .any(|a| a["title"].as_str().unwrap_or("").contains("retry_delay")),
        "expected a retry_delay quick-fix action over real dispatch, got: {actions:?}"
    );
}
