#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]
//! End-to-end smoke test of the actual `grpctestify lsp` binary spoken to over
//! real process stdio with `Content-Length`-framed JSON-RPC — the exact channel
//! VS Code/Neovim use when they spawn the server. This is the automated portion
//! of lsp-ide §6.2's "manual smoke test": launching a real editor's Extension
//! Development Host still needs a GUI this environment doesn't have, but the
//! server-side "does the shipped binary actually start and speak LSP over real
//! stdin/stdout" half is fully checkable here (the in-process `lsp_protocol_tests`
//! duplex harness never exercises the real binary or the real pipe).
//!
//! The document deliberately carries no ENDPOINT/PROTO — those make `didOpen`
//! kick off background schema/reflection resolution against an unreachable
//! address, which is irrelevant to this smoke test and would only add a network
//! connect-timeout to teardown. A BENCH-only doc keeps everything local, so the
//! completion round-trip proves real request dispatch without any network.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};

#[path = "support/mod.rs"]
mod support;
use support::cli_command;

struct StdioClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl StdioClient {
    fn spawn() -> Self {
        let mut child = cli_command()
            .arg("lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn grpctestify lsp");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = BufReader::new(child.stdout.take().expect("child stdout"));
        StdioClient {
            child,
            stdin,
            stdout,
            next_id: 0,
        }
    }

    fn send(&mut self, msg: &serde_json::Value) {
        let body = serde_json::to_vec(msg).unwrap();
        self.stdin
            .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
            .unwrap();
        self.stdin.write_all(&body).unwrap();
        self.stdin.flush().unwrap();
    }

    fn read_message(&mut self) -> serde_json::Value {
        let mut content_length = 0usize;
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).expect("read header line");
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                break;
            }
            if let Some(len) = trimmed.strip_prefix("Content-Length: ") {
                content_length = len.parse().expect("parse content-length");
            }
        }
        let mut body = vec![0u8; content_length];
        self.stdout.read_exact(&mut body).expect("read body");
        serde_json::from_slice(&body).expect("parse json body")
    }

    /// Send a request, then read messages until the response with a matching
    /// `id` arrives — skipping any interleaved notifications (e.g. diagnostics).
    fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.next_id += 1;
        let id = self.next_id;
        let mut msg = serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method
        });
        // Omit `params` entirely when there are none — LSP requests like
        // `shutdown` reject an explicit `params: null`.
        if !params.is_null() {
            msg["params"] = params;
        }
        self.send(&msg);
        loop {
            let msg = self.read_message();
            if msg.get("id").and_then(|v| v.as_i64()) == Some(id) {
                return msg;
            }
        }
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) {
        let mut msg = serde_json::json!({ "jsonrpc": "2.0", "method": method });
        if !params.is_null() {
            msg["params"] = params;
        }
        self.send(&msg);
    }
}

impl Drop for StdioClient {
    fn drop(&mut self) {
        // Force teardown regardless of any background work the server may have
        // started — the smoke test's assertions are already done by here.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn real_lsp_binary_speaks_lsp_over_process_stdio() {
    let mut client = StdioClient::spawn();

    // 1. initialize — the shipped binary must return real server capabilities
    //    over the real pipe, proving it's wired as a generic LSP server.
    let init = client.request(
        "initialize",
        serde_json::json!({ "capabilities": {}, "processId": null }),
    );
    let caps = &init["result"]["capabilities"];
    assert!(
        !caps["completionProvider"].is_null(),
        "server must advertise completion: {init}"
    );
    assert!(
        !caps["hoverProvider"].is_null(),
        "server must advertise hover: {init}"
    );
    assert!(
        !caps["textDocumentSync"].is_null(),
        "server must advertise text sync: {init}"
    );

    client.notify("initialized", serde_json::json!({}));

    // 2. Open a BENCH-only document and drive one real request over the process
    //    to prove request dispatch works end-to-end (not just the handshake).
    let uri = "file:///tmp/smoke.gctf";
    let text = "--- BENCH ---\n\n";
    client.notify(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": { "uri": uri, "languageId": "gctf", "version": 1, "text": text }
        }),
    );

    // BENCH-section completion (the same capability `lsp_protocol_tests` covers
    // in-process) — here it round-trips through the real binary's stdio, and is
    // resolved from the local schema table with no network.
    let completion = client.request(
        "textDocument/completion",
        serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": 1, "character": 0 }
        }),
    );
    assert!(
        completion.get("error").is_none(),
        "completion dispatch errored over stdio: {completion}"
    );
    assert!(
        completion.get("result").is_some(),
        "completion must return a JSON-RPC result over stdio: {completion}"
    );

    // 3. Graceful shutdown request must still answer over stdio; teardown of the
    //    process itself is handled by `Drop` (kill) so a stray background task
    //    can never hang the test.
    let shutdown = client.request("shutdown", serde_json::Value::Null);
    assert!(
        shutdown.get("error").is_none(),
        "shutdown errored over stdio: {shutdown}"
    );
    client.notify("exit", serde_json::Value::Null);
}

/// Editor-critical: completion on an ENDPOINT line whose ADDRESS points at a
/// dead server must degrade gracefully — the server's schema fetch is bounded
/// (a 3s `SCHEMA_TIMEOUT` in `create_schema_client`), so completion still
/// returns promptly with the local endpoint template instead of hanging the
/// editor on a connect timeout. A connection-*refused* address (127.0.0.1:1)
/// keeps this deterministic and network-free while still exercising the
/// schema-client-fails path. If this ever regresses to an unbounded connect,
/// the test hangs (and `Drop` still reaps the process).
#[test]
fn completion_on_dead_address_endpoint_degrades_without_hanging() {
    let mut client = StdioClient::spawn();
    client.request(
        "initialize",
        serde_json::json!({ "capabilities": {}, "processId": null }),
    );
    client.notify("initialized", serde_json::json!({}));

    let uri = "file:///tmp/dead.gctf";
    // ADDRESS is a refused port; ENDPOINT line is where schema completion fires.
    let text = "--- ADDRESS ---\n127.0.0.1:1\n\n--- ENDPOINT ---\npkg.Svc/Method\n";
    client.notify(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": { "uri": uri, "languageId": "gctf", "version": 1, "text": text }
        }),
    );

    let completion = client.request(
        "textDocument/completion",
        serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": 4, "character": 3 }
        }),
    );
    assert!(
        completion.get("error").is_none(),
        "completion errored on dead address: {completion}"
    );
    // The local endpoint template is always offered even when schema resolution
    // fails — proof completion returned useful items rather than stalling.
    let body = completion.to_string();
    assert!(
        body.contains("package.Service/Method"),
        "expected the local endpoint template despite dead schema server: {completion}"
    );

    client.request("shutdown", serde_json::Value::Null);
    client.notify("exit", serde_json::Value::Null);
}
