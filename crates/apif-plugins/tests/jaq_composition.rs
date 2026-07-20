//! Integration tests: pure plugins called from inside jaq/jq expressions.
//!
//! These exercise the jaq-fallback path of the assertion engine wired with the
//! real `PluginManager`, so `@plugin(...)` calls compose with jq operators
//! (pipes, `map`, `select`, `all`, arithmetic, comparison against fields).

use std::sync::Arc;

use apif_assert::{AssertionEngine, AssertionResult};
use apif_plugins::PluginManager;
use serde_json::json;

fn engine() -> AssertionEngine {
    AssertionEngine::with_registry(Arc::new(PluginManager::new()))
}

#[test]
fn len_plugin_compares_against_field() {
    let response = json!({ "items": [1, 2, 3], "expected_count": 3 });
    let result = engine()
        .evaluate("@len(.items) == .expected_count", &response, None, None)
        .unwrap();
    assert!(matches!(result, AssertionResult::Pass), "got {:?}", result);
}

#[test]
fn len_plugin_compares_against_field_mismatch() {
    let response = json!({ "items": [1, 2], "expected_count": 3 });
    let result = engine()
        .evaluate("@len(.items) == .expected_count", &response, None, None)
        .unwrap();
    assert!(
        matches!(result, AssertionResult::Fail { .. }),
        "got {:?}",
        result
    );
}

#[test]
fn is_email_over_array_with_map_all() {
    let response = json!({ "items": ["a@b.com", "c@d.org"] });
    let result = engine()
        .evaluate(".items | map(@is_email(.)) | all", &response, None, None)
        .unwrap();
    assert!(matches!(result, AssertionResult::Pass), "got {:?}", result);
}

#[test]
fn is_email_over_array_with_map_all_fails_on_bad_entry() {
    let response = json!({ "items": ["a@b.com", "not-an-email"] });
    let result = engine()
        .evaluate(".items | map(@is_email(.)) | all", &response, None, None)
        .unwrap();
    assert!(
        matches!(result, AssertionResult::Fail { .. }),
        "got {:?}",
        result
    );
}

#[test]
fn is_uuid_composes_with_select_and_length() {
    let response = json!({
        "users": [
            { "id": "550e8400-e29b-41d4-a716-446655440000" },
            { "id": "not-a-uuid" },
            { "id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8" }
        ]
    });
    let result = engine()
        .query("[.users[] | select(@is_uuid(.id))] | length", &response)
        .unwrap();
    assert_eq!(result, vec![json!(2)]);
}

#[test]
fn regex_plugin_inside_jq_pipe() {
    let response = json!({ "name": "Alice" });
    let result = engine()
        .evaluate(".name | @regex(., \"^A\")", &response, None, None)
        .unwrap();
    assert!(matches!(result, AssertionResult::Pass), "got {:?}", result);
}

#[test]
fn regex_plugin_inside_jq_pipe_negative() {
    let response = json!({ "name": "Bob" });
    let result = engine()
        .evaluate(".name | @regex(., \"^A\")", &response, None, None)
        .unwrap();
    assert!(
        matches!(result, AssertionResult::Fail { .. }),
        "got {:?}",
        result
    );
}

#[test]
fn context_plugin_in_jaq_is_clear_error() {
    // `@header` needs response headers and cannot be a pure jq function.
    let response = json!({ "list": [1] });
    let result = engine()
        .evaluate(".list | map(@header(\"y\")) | all", &response, None, None)
        .unwrap();
    let msg = match result {
        AssertionResult::Error(m) => m,
        AssertionResult::Fail { message, .. } => message,
        other => panic!("expected error/fail, got {:?}", other),
    };
    assert!(
        msg.contains("not available in jq expressions"),
        "unexpected message: {}",
        msg
    );
}
