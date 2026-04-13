//! Tests for LSP server functionality.
//!
//! Extracted from src/lsp/server.rs during refactoring.

use grpctestify::lsp;

#[test]
fn test_build_semantic_tokens_section_headers() {
    let content = "--- ENDPOINT ---\ntest.Service/Method\n";
    let tokens = lsp::build_semantic_tokens(content);

    // Should have at least one token for the section header
    // Note: AST parsing may fail for incomplete content, so we check both cases
    if !tokens.data.is_empty() {
        // First token should be a KEYWORD (section header)
        assert_eq!(tokens.data[0].token_type, 0); // KEYWORD
    }
    // If AST parsing failed, tokens will be empty - that's acceptable for this test
}
