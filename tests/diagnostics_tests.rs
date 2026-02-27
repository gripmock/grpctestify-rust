// Tests for diagnostic types and builder

use grpctestify::diagnostics::*;

#[test]
fn test_diagnostic_severity_serialization() {
    let severity = DiagnosticSeverity::Error;
    let json = serde_json::to_string(&severity).unwrap();
    assert_eq!(json, "\"error\"");

    let severity = DiagnosticSeverity::Warning;
    let json = serde_json::to_string(&severity).unwrap();
    assert_eq!(json, "\"warning\"");

    let severity = DiagnosticSeverity::Information;
    let json = serde_json::to_string(&severity).unwrap();
    assert_eq!(json, "\"information\"");

    let severity = DiagnosticSeverity::Hint;
    let json = serde_json::to_string(&severity).unwrap();
    assert_eq!(json, "\"hint\"");
}

#[test]
fn test_diagnostic_code_as_str() {
    assert_eq!(DiagnosticCode::JsonParseError.as_str(), "json_parse_error");
    assert_eq!(DiagnosticCode::UnclosedBrace.as_str(), "unclosed_brace");
    assert_eq!(DiagnosticCode::MissingSection.as_str(), "missing_section");
    assert_eq!(
        DiagnosticCode::UndefinedVariable.as_str(),
        "undefined_variable"
    );
    assert_eq!(DiagnosticCode::UnknownFunction.as_str(), "unknown_function");
}

#[test]
fn test_position_new() {
    let pos = Position::new(5, 10);
    assert_eq!(pos.line, 5);
    assert_eq!(pos.column, 10);
}

#[test]
fn test_position_default() {
    let pos = Position::default();
    assert_eq!(pos.line, 0);
    assert_eq!(pos.column, 0);
}

#[test]
fn test_range_new() {
    let range = Range::new(Position::new(1, 0), Position::new(1, 10));
    assert_eq!(range.start.line, 1);
    assert_eq!(range.start.column, 0);
    assert_eq!(range.end.line, 1);
    assert_eq!(range.end.column, 10);
}

#[test]
fn test_range_at_line() {
    let range = Range::at_line(5);
    assert_eq!(range.start.line, 5);
    assert_eq!(range.end.line, 5);
}

#[test]
fn test_diagnostic_builder_error() {
    let diagnostic = DiagnosticBuilder::error(
        DiagnosticCode::JsonParseError,
        "Failed to parse JSON",
        Range::at_line(5),
    )
    .with_suggestion("Check syntax")
    .with_context("{ invalid json")
    .build();

    assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
    assert_eq!(diagnostic.code, DiagnosticCode::JsonParseError);
    assert_eq!(diagnostic.message, "Failed to parse JSON");
    assert_eq!(diagnostic.suggestions.len(), 1);
    assert_eq!(diagnostic.context, Some("{ invalid json".to_string()));
}

#[test]
fn test_diagnostic_builder_warning() {
    let diagnostic = DiagnosticBuilder::warning(
        DiagnosticCode::UnusedVariable,
        "Unused variable",
        Range::at_line(10),
    )
    .build();

    assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
    assert_eq!(diagnostic.code, DiagnosticCode::UnusedVariable);
}

#[test]
fn test_diagnostic_builder_info() {
    let diagnostic = DiagnosticBuilder::info(
        DiagnosticCode::EmptySection,
        "Empty section",
        Range::at_line(15),
    )
    .build();

    assert_eq!(diagnostic.severity, DiagnosticSeverity::Information);
    assert_eq!(diagnostic.code, DiagnosticCode::EmptySection);
}

#[test]
fn test_diagnostic_builder_hint() {
    let diagnostic = DiagnosticBuilder::hint(
        DiagnosticCode::DeprecatedSymbol,
        "Deprecated symbol",
        Range::at_line(20),
    )
    .build();

    assert_eq!(diagnostic.severity, DiagnosticSeverity::Hint);
    assert_eq!(diagnostic.code, DiagnosticCode::DeprecatedSymbol);
}

#[test]
fn test_diagnostic_builder_with_file() {
    let diagnostic =
        DiagnosticBuilder::error(DiagnosticCode::JsonParseError, "Error", Range::at_line(1))
            .with_file("test.gctf")
            .build();

    assert_eq!(diagnostic.file, Some("test.gctf".to_string()));
}

#[test]
fn test_diagnostic_builder_with_suggestions() {
    let diagnostic = DiagnosticBuilder::error(
        DiagnosticCode::UnclosedBrace,
        "Unclosed brace",
        Range::at_line(1),
    )
    .with_suggestion("Add }")
    .with_suggestion("Check nesting")
    .build();

    assert_eq!(diagnostic.suggestions.len(), 2);
    assert_eq!(diagnostic.suggestions[0], "Add }");
    assert_eq!(diagnostic.suggestions[1], "Check nesting");
}

#[test]
fn test_gctf_diagnostics_json_parse_error() {
    let diagnostic = GctfDiagnostics::json_parse_error(5, 10, "unexpected token");

    assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
    assert_eq!(diagnostic.code, DiagnosticCode::JsonParseError);
    assert!(diagnostic.message.contains("Failed to parse JSON"));
    assert!(!diagnostic.suggestions.is_empty());
}

#[test]
fn test_gctf_diagnostics_json5_parse_error() {
    let diagnostic = GctfDiagnostics::json5_parse_error(5, 10, "unexpected token");

    assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
    assert_eq!(diagnostic.code, DiagnosticCode::Json5ParseError);
    assert!(diagnostic.message.contains("Failed to parse JSON5"));
}

#[test]
fn test_gctf_diagnostics_unclosed_brace() {
    let diagnostic = GctfDiagnostics::unclosed_brace(5, 10);

    assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
    assert_eq!(diagnostic.code, DiagnosticCode::UnclosedBrace);
    assert_eq!(diagnostic.message, "Unclosed brace '{'");
    assert_eq!(diagnostic.suggestions.len(), 1);
}

#[test]
fn test_gctf_diagnostics_missing_section() {
    let diagnostic = GctfDiagnostics::missing_section("ENDPOINT");

    assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
    assert_eq!(diagnostic.code, DiagnosticCode::MissingSection);
    assert!(diagnostic.message.contains("ENDPOINT"));
}

#[test]
fn test_gctf_diagnostics_undefined_variable() {
    let diagnostic = GctfDiagnostics::undefined_variable("myVar", 10, 5);

    assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
    assert_eq!(diagnostic.code, DiagnosticCode::UndefinedVariable);
    assert!(diagnostic.message.contains("myVar"));
}

#[test]
fn test_gctf_diagnostics_unknown_function() {
    let diagnostic = GctfDiagnostics::unknown_function("@unknown", 10, 5);

    assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
    assert_eq!(diagnostic.code, DiagnosticCode::UnknownFunction);
    assert!(diagnostic.message.contains("@unknown"));
}

#[test]
fn test_diagnostic_collection_push() {
    let mut collection = DiagnosticCollection::new();

    collection.error(DiagnosticCode::JsonParseError, "Error 1", Range::at_line(1));
    collection.warning(
        DiagnosticCode::UnusedVariable,
        "Warning 1",
        Range::at_line(2),
    );

    assert_eq!(collection.diagnostics.len(), 2);
    assert!(collection.has_errors());
    assert!(collection.has_warnings());
}

#[test]
fn test_diagnostic_collection_is_empty() {
    let collection = DiagnosticCollection::new();
    assert!(collection.is_empty());
}

#[test]
fn test_diagnostic_collection_errors() {
    let mut collection = DiagnosticCollection::new();

    collection.error(DiagnosticCode::JsonParseError, "Error", Range::at_line(1));
    collection.warning(DiagnosticCode::UnusedVariable, "Warning", Range::at_line(2));
    collection.info(DiagnosticCode::EmptySection, "Info", Range::at_line(3));

    let errors: Vec<_> = collection.errors().collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].severity, DiagnosticSeverity::Error);
}

#[test]
fn test_diagnostic_collection_warnings() {
    let mut collection = DiagnosticCollection::new();

    collection.error(DiagnosticCode::JsonParseError, "Error", Range::at_line(1));
    collection.warning(
        DiagnosticCode::UnusedVariable,
        "Warning 1",
        Range::at_line(2),
    );
    collection.warning(DiagnosticCode::EmptySection, "Warning 2", Range::at_line(3));

    let warnings: Vec<_> = collection.warnings().collect();
    assert_eq!(warnings.len(), 2);
}
