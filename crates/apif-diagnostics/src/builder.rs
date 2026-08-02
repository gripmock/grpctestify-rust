// Diagnostic builder and common diagnostic helpers

use super::types::*;

pub struct GctfDiagnostics;

impl GctfDiagnostics {
    // Parse errors

    pub fn json_parse_error(line: usize, column: usize, error: &str) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::JsonParseError,
            format!("Failed to parse JSON: {}", error),
            Range::new(Position::new(line, column), Position::new(line, column)),
        )
        .with_suggestion("Check for syntax errors in JSON")
        .with_suggestion("Ensure all braces and brackets are closed")
    }

    pub fn json5_parse_error(line: usize, column: usize, error: &str) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::Json5ParseError,
            format!("Failed to parse JSON5: {}", error),
            Range::new(Position::new(line, column), Position::new(line, column)),
        )
        .with_suggestion("Check for syntax errors in JSON5")
        .with_suggestion("JSON5 allows unquoted keys and trailing commas")
    }

    pub fn unclosed_brace(line: usize, column: usize) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UnclosedBrace,
            "Unclosed brace '{'",
            Range::new(Position::new(line, column), Position::new(line, column + 1)),
        )
        .with_suggestion("Add closing brace '}'")
    }

    pub fn unclosed_bracket(line: usize, column: usize) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UnclosedBracket,
            "Unclosed bracket '['",
            Range::new(Position::new(line, column), Position::new(line, column + 1)),
        )
        .with_suggestion("Add closing bracket ']'")
    }

    pub fn unclosed_string(line: usize, column: usize) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UnclosedString,
            "Unclosed string literal",
            Range::new(Position::new(line, column), Position::new(line, column + 1)),
        )
        .with_suggestion("Add closing quote '\"'")
    }

    pub fn invalid_escape(line: usize, column: usize, escape_char: char) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::InvalidEscape,
            format!("Invalid escape sequence '\\{}'", escape_char),
            Range::new(Position::new(line, column), Position::new(line, column + 2)),
        )
        .with_suggestion(
            "Valid escape sequences: \\n, \\t, \\r, \\\\, \\\", \\/, \\b, \\f, \\uXXXX",
        )
    }

    // Section errors

    pub fn missing_section(section_name: &str) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MissingSection,
            format!("Missing required section: {}", section_name),
            Range::default(),
        )
        .with_suggestion(format!("Add --- {} --- section", section_name))
    }

    pub fn invalid_section_header(line: usize, header: &str) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::InvalidSectionHeader,
            format!("Invalid section header: {}", header),
            Range::at_line(line),
        )
        .with_suggestion("Section headers should be: --- SECTION_NAME ---")
    }

    pub fn duplicate_section(section_name: &str, line: usize) -> Diagnostic {
        Diagnostic::warning(
            DiagnosticCode::DuplicateSection,
            format!("Duplicate section: {}", section_name),
            Range::at_line(line),
        )
        .with_suggestion("Remove duplicate section")
    }

    pub fn empty_section(section_name: &str, line: usize) -> Diagnostic {
        Diagnostic::warning(
            DiagnosticCode::EmptySection,
            format!("Empty section: {}", section_name),
            Range::at_line(line),
        )
    }

    pub fn unknown_section_type(line: usize, section_name: &str) -> Diagnostic {
        Diagnostic::warning(
            DiagnosticCode::UnknownSectionType,
            format!("Unknown section type: {}", section_name),
            Range::at_line(line),
        )
        .with_suggestion("Valid sections: ADDRESS, ENDPOINT, REQUEST, RESPONSE, ERROR, EXTRACT, ASSERTS, REQUEST_HEADERS, TLS, PROTO, OPTIONS")
    }

    // Semantic errors

    pub fn undefined_variable(var_name: &str, line: usize, column: usize) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UndefinedVariable,
            format!("Undefined variable: {}", var_name),
            Range::new(
                Position::new(line, column),
                Position::new(line, column + var_name.chars().count()),
            ),
        )
        .with_suggestion("Define variable in EXTRACT section before use")
    }

    pub fn unused_variable(var_name: &str, line: usize) -> Diagnostic {
        Diagnostic::hint(
            DiagnosticCode::UnusedVariable,
            format!("Unused variable: {}", var_name),
            Range::at_line(line),
        )
        .with_suggestion("Remove unused variable or use it in subsequent sections")
    }

    pub fn unknown_function(func_name: &str, line: usize, column: usize) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UnknownFunction,
            format!("Unknown function: {}", func_name),
            Range::new(
                Position::new(line, column),
                Position::new(line, column + func_name.chars().count()),
            ),
        )
        .with_suggestion("Available functions: @uuid, @email, @ip, @phone, @url, @header, @trailer")
    }

    // Validation errors

    pub fn with_asserts_without_asserts(line: usize) -> Diagnostic {
        Diagnostic::warning(
            DiagnosticCode::ValidationError,
            "with_asserts option set but no ASSERTS section follows",
            Range::at_line(line),
        )
        .with_suggestion("Add ASSERTS section after this RESPONSE")
    }

    pub fn missing_endpoint() -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MissingRequiredField,
            "Missing required ENDPOINT section",
            Range::default(),
        )
        .with_suggestion("Add --- ENDPOINT --- section with service/method")
    }

    pub fn missing_request_or_error() -> Diagnostic {
        Diagnostic::warning(
            DiagnosticCode::MissingRequiredField,
            "No REQUEST or ERROR section found",
            Range::default(),
        )
        .with_suggestion("Add REQUEST section for normal calls or ERROR section for error testing")
    }

    pub fn empty_request(line: usize) -> Diagnostic {
        Diagnostic::info(
            DiagnosticCode::EmptySection,
            "Empty REQUEST section will send empty JSON object {}",
            Range::at_line(line),
        )
    }

    pub fn empty_extract(line: usize) -> Diagnostic {
        Diagnostic::warning(
            DiagnosticCode::EmptySection,
            "EXTRACT section has no variables",
            Range::at_line(line),
        )
    }

    pub fn empty_asserts(line: usize) -> Diagnostic {
        Diagnostic::warning(
            DiagnosticCode::EmptySection,
            "ASSERTS section has no assertions",
            Range::at_line(line),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undefined_variable_caret_uses_char_count() {
        // Regression: end column must use char count, not byte length, so
        // non-ASCII identifiers get a correctly sized caret range.
        let name = "café"; // 4 chars, 5 bytes
        let diag = GctfDiagnostics::undefined_variable(name, 1, 3);
        assert_eq!(diag.range.start.column, 3);
        assert_eq!(diag.range.end.column, 3 + name.chars().count());
        assert_eq!(diag.range.end.column, 7);
    }

    #[test]
    fn unknown_function_caret_uses_char_count() {
        let name = "@naïve"; // 6 chars, 7 bytes
        let diag = GctfDiagnostics::unknown_function(name, 1, 0);
        assert_eq!(diag.range.end.column, name.chars().count());
        assert_eq!(diag.range.end.column, 6);
    }
    #[test]
    fn gctf_diagnostics_json_parse_error() {
        let diag = GctfDiagnostics::json_parse_error(5, 10, "unexpected token");
        assert_eq!(diag.severity, DiagnosticSeverity::Error);
        assert_eq!(diag.suggestions.len(), 2);
        assert!(diag.message.contains("unexpected token"));
    }

    #[test]
    fn gctf_diagnostics_json5_parse_error() {
        let diag = GctfDiagnostics::json5_parse_error(3, 5, "bad syntax");
        assert_eq!(diag.severity, DiagnosticSeverity::Error);
        assert_eq!(diag.code, DiagnosticCode::Json5ParseError);
    }

    #[test]
    fn gctf_diagnostics_unclosed_brace() {
        let diag = GctfDiagnostics::unclosed_brace(1, 0);
        assert_eq!(diag.severity, DiagnosticSeverity::Error);
        assert_eq!(diag.code, DiagnosticCode::UnclosedBrace);
        assert_eq!(diag.suggestions.len(), 1);
    }

    #[test]
    fn gctf_diagnostics_unclosed_bracket() {
        let diag = GctfDiagnostics::unclosed_bracket(2, 3);
        assert_eq!(diag.code, DiagnosticCode::UnclosedBracket);
    }

    #[test]
    fn gctf_diagnostics_unclosed_string() {
        let diag = GctfDiagnostics::unclosed_string(4, 0);
        assert_eq!(diag.code, DiagnosticCode::UnclosedString);
    }

    #[test]
    fn gctf_diagnostics_invalid_escape() {
        let diag = GctfDiagnostics::invalid_escape(1, 5, 'x');
        assert_eq!(diag.code, DiagnosticCode::InvalidEscape);
        assert!(diag.message.contains("\\x"));
    }

    #[test]
    fn gctf_diagnostics_missing_section() {
        let diag = GctfDiagnostics::missing_section("ENDPOINT");
        assert_eq!(diag.severity, DiagnosticSeverity::Error);
        assert_eq!(diag.suggestions.len(), 1);
        assert!(diag.message.contains("ENDPOINT"));
    }

    #[test]
    fn gctf_diagnostics_invalid_section_header() {
        let diag = GctfDiagnostics::invalid_section_header(3, "bad header");
        assert_eq!(diag.code, DiagnosticCode::InvalidSectionHeader);
    }

    #[test]
    fn gctf_diagnostics_duplicate_section() {
        let diag = GctfDiagnostics::duplicate_section("REQUEST", 10);
        assert_eq!(diag.severity, DiagnosticSeverity::Warning);
        assert_eq!(diag.code, DiagnosticCode::DuplicateSection);
    }

    #[test]
    fn gctf_diagnostics_empty_section() {
        let diag = GctfDiagnostics::empty_section("RESPONSE", 5);
        assert_eq!(diag.severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn gctf_diagnostics_unknown_section_type() {
        let diag = GctfDiagnostics::unknown_section_type(1, "CUSTOM");
        assert_eq!(diag.severity, DiagnosticSeverity::Warning);
        assert_eq!(diag.suggestions.len(), 1);
    }

    #[test]
    fn gctf_diagnostics_undefined_variable() {
        let diag = GctfDiagnostics::undefined_variable("x", 5, 10);
        assert_eq!(diag.severity, DiagnosticSeverity::Error);
        assert_eq!(diag.code, DiagnosticCode::UndefinedVariable);
        assert!(diag.message.contains("x"));
    }

    #[test]
    fn gctf_diagnostics_unused_variable() {
        let diag = GctfDiagnostics::unused_variable("unused", 7);
        assert_eq!(diag.severity, DiagnosticSeverity::Hint);
        assert_eq!(diag.code, DiagnosticCode::UnusedVariable);
    }

    #[test]
    fn gctf_diagnostics_unknown_function() {
        let diag = GctfDiagnostics::unknown_function("@bad_fn", 3, 0);
        assert_eq!(diag.severity, DiagnosticSeverity::Error);
        assert_eq!(diag.code, DiagnosticCode::UnknownFunction);
    }

    #[test]
    fn gctf_diagnostics_with_asserts_without_asserts() {
        let diag = GctfDiagnostics::with_asserts_without_asserts(8);
        assert_eq!(diag.severity, DiagnosticSeverity::Warning);
        assert!(diag.message.contains("with_asserts"));
    }

    #[test]
    fn gctf_diagnostics_missing_endpoint() {
        let diag = GctfDiagnostics::missing_endpoint();
        assert_eq!(diag.severity, DiagnosticSeverity::Error);
        assert_eq!(diag.code, DiagnosticCode::MissingRequiredField);
    }

    #[test]
    fn gctf_diagnostics_missing_request_or_error() {
        let diag = GctfDiagnostics::missing_request_or_error();
        assert_eq!(diag.severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn gctf_diagnostics_empty_request() {
        let diag = GctfDiagnostics::empty_request(5);
        assert_eq!(diag.severity, DiagnosticSeverity::Information);
    }

    #[test]
    fn gctf_diagnostics_empty_extract() {
        let diag = GctfDiagnostics::empty_extract(3);
        assert_eq!(diag.severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn gctf_diagnostics_empty_asserts() {
        let diag = GctfDiagnostics::empty_asserts(10);
        assert_eq!(diag.severity, DiagnosticSeverity::Warning);
    }
}
