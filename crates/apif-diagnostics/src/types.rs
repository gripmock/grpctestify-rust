use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    JsonParseError = 1001,
    Json5ParseError = 1002,
    UnexpectedEndOfFile = 1003,
    InvalidCharacter = 1004,
    UnclosedBrace = 1005,
    UnclosedBracket = 1006,
    UnclosedString = 1007,

    MissingSection = 2001,
    InvalidSectionHeader = 2002,
    DuplicateSection = 2003,
    EmptySection = 2004,
    InvalidSectionContent = 2005,
    UnknownSectionType = 2006,
    DuplicateKey = 2007,

    InvalidSyntax = 3001,
    UnexpectedToken = 3002,
    MissingToken = 3003,
    InvalidEscape = 3004,
    InvalidUnicodeEscape = 3005,

    UndefinedVariable = 4001,
    UnusedVariable = 4002,
    TypeMismatch = 4003,
    InvalidOperation = 4004,
    UnknownFunction = 4005,
    InvalidArgumentCount = 4006,
    InvalidArgumentType = 4007,

    MissingRequiredField = 5001,
    InvalidFieldValue = 5002,
    ValidationError = 5003,
    ConstraintViolation = 5004,

    UndefinedSymbol = 6001,
    UnusedSymbol = 6002,
    DeprecatedSymbol = 6003,
}

impl DiagnosticCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            DiagnosticCode::JsonParseError => "json_parse_error",
            DiagnosticCode::Json5ParseError => "json5_parse_error",
            DiagnosticCode::UnexpectedEndOfFile => "unexpected_eof",
            DiagnosticCode::InvalidCharacter => "invalid_character",
            DiagnosticCode::UnclosedBrace => "unclosed_brace",
            DiagnosticCode::UnclosedBracket => "unclosed_bracket",
            DiagnosticCode::UnclosedString => "unclosed_string",

            DiagnosticCode::MissingSection => "missing_section",
            DiagnosticCode::InvalidSectionHeader => "invalid_section_header",
            DiagnosticCode::DuplicateSection => "duplicate_section",
            DiagnosticCode::EmptySection => "empty_section",
            DiagnosticCode::InvalidSectionContent => "invalid_section_content",
            DiagnosticCode::UnknownSectionType => "unknown_section_type",
            DiagnosticCode::DuplicateKey => "duplicate_key",

            DiagnosticCode::InvalidSyntax => "invalid_syntax",
            DiagnosticCode::UnexpectedToken => "unexpected_token",
            DiagnosticCode::MissingToken => "missing_token",
            DiagnosticCode::InvalidEscape => "invalid_escape",
            DiagnosticCode::InvalidUnicodeEscape => "invalid_unicode_escape",

            DiagnosticCode::UndefinedVariable => "undefined_variable",
            DiagnosticCode::UnusedVariable => "unused_variable",
            DiagnosticCode::TypeMismatch => "type_mismatch",
            DiagnosticCode::InvalidOperation => "invalid_operation",
            DiagnosticCode::UnknownFunction => "unknown_function",
            DiagnosticCode::InvalidArgumentCount => "invalid_argument_count",
            DiagnosticCode::InvalidArgumentType => "invalid_argument_type",

            DiagnosticCode::MissingRequiredField => "missing_required_field",
            DiagnosticCode::InvalidFieldValue => "invalid_field_value",
            DiagnosticCode::ValidationError => "validation_error",
            DiagnosticCode::ConstraintViolation => "constraint_violation",

            DiagnosticCode::UndefinedSymbol => "undefined_symbol",
            DiagnosticCode::UnusedSymbol => "unused_symbol",
            DiagnosticCode::DeprecatedSymbol => "deprecated_symbol",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    pub fn at_line(line: usize) -> Self {
        Self {
            start: Position::new(line, 0),
            end: Position::new(line, usize::MAX),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticRelatedInformation {
    pub location: DiagnosticLocation,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticLocation {
    pub file: String,
    pub range: Range,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related_information: Vec<DiagnosticRelatedInformation>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

impl Diagnostic {
    pub fn error(code: DiagnosticCode, message: impl Into<String>, range: Range) -> Self {
        Self {
            code,
            severity: DiagnosticSeverity::Error,
            message: message.into(),
            range,
            file: None,
            source: Some("grpctestify".to_string()),
            related_information: Vec::new(),
            suggestions: Vec::new(),
            context: None,
        }
    }

    pub fn warning(code: DiagnosticCode, message: impl Into<String>, range: Range) -> Self {
        Self {
            code,
            severity: DiagnosticSeverity::Warning,
            message: message.into(),
            range,
            file: None,
            source: Some("grpctestify".to_string()),
            related_information: Vec::new(),
            suggestions: Vec::new(),
            context: None,
        }
    }

    pub fn info(code: DiagnosticCode, message: impl Into<String>, range: Range) -> Self {
        Self {
            code,
            severity: DiagnosticSeverity::Information,
            message: message.into(),
            range,
            file: None,
            source: Some("grpctestify".to_string()),
            related_information: Vec::new(),
            suggestions: Vec::new(),
            context: None,
        }
    }

    pub fn hint(code: DiagnosticCode, message: impl Into<String>, range: Range) -> Self {
        Self {
            code,
            severity: DiagnosticSeverity::Hint,
            message: message.into(),
            range,
            file: None,
            source: Some("grpctestify".to_string()),
            related_information: Vec::new(),
            suggestions: Vec::new(),
            context: None,
        }
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestions.push(suggestion.into());
        self
    }

    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.suggestions.extend(suggestions);
        self
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    pub fn with_related_info(
        mut self,
        location: DiagnosticLocation,
        message: impl Into<String>,
    ) -> Self {
        self.related_information.push(DiagnosticRelatedInformation {
            location,
            message: message.into(),
        });
        self
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DiagnosticCollection {
    pub diagnostics: Vec<Diagnostic>,
}

impl DiagnosticCollection {
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn error(&mut self, code: DiagnosticCode, message: impl Into<String>, range: Range) {
        self.push(Diagnostic::error(code, message, range));
    }

    pub fn warning(&mut self, code: DiagnosticCode, message: impl Into<String>, range: Range) {
        self.push(Diagnostic::warning(code, message, range));
    }

    pub fn info(&mut self, code: DiagnosticCode, message: impl Into<String>, range: Range) {
        self.push(Diagnostic::info(code, message, range));
    }

    pub fn hint(&mut self, code: DiagnosticCode, message: impl Into<String>, range: Range) {
        self.push(Diagnostic::hint(code, message, range));
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error)
    }

    #[must_use]
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Warning)
    }

    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_code_as_str() {
        assert_eq!(DiagnosticCode::JsonParseError.as_str(), "json_parse_error");
        assert_eq!(DiagnosticCode::MissingSection.as_str(), "missing_section");
        assert_eq!(DiagnosticCode::InvalidSyntax.as_str(), "invalid_syntax");
        assert_eq!(
            DiagnosticCode::UndefinedVariable.as_str(),
            "undefined_variable"
        );
        assert_eq!(
            DiagnosticCode::MissingRequiredField.as_str(),
            "missing_required_field"
        );
        assert_eq!(DiagnosticCode::UndefinedSymbol.as_str(), "undefined_symbol");
    }

    #[test]
    fn diagnostic_collection_new() {
        let coll = DiagnosticCollection::new();
        assert!(coll.is_empty());
    }

    #[test]
    fn diagnostic_collection_push() {
        let mut coll = DiagnosticCollection::new();
        let diag = Diagnostic::error(DiagnosticCode::JsonParseError, "err", Range::default());
        coll.push(diag);
        assert!(!coll.is_empty());
    }

    #[test]
    fn diagnostic_collection_error() {
        let mut coll = DiagnosticCollection::new();
        coll.error(DiagnosticCode::JsonParseError, "err", Range::default());
        assert!(coll.has_errors());
        assert!(!coll.has_warnings());
        assert_eq!(coll.errors().count(), 1);
    }

    #[test]
    fn diagnostic_collection_warning() {
        let mut coll = DiagnosticCollection::new();
        coll.warning(DiagnosticCode::DuplicateSection, "warn", Range::default());
        assert!(!coll.has_errors());
        assert!(coll.has_warnings());
        assert_eq!(coll.warnings().count(), 1);
    }

    #[test]
    fn diagnostic_collection_info() {
        let mut coll = DiagnosticCollection::new();
        coll.info(DiagnosticCode::EmptySection, "info", Range::default());
        assert!(!coll.is_empty());
    }

    #[test]
    fn diagnostic_collection_hint() {
        let mut coll = DiagnosticCollection::new();
        coll.hint(DiagnosticCode::UnusedVariable, "hint", Range::default());
        assert!(!coll.is_empty());
    }

    #[test]
    fn diagnostic_collection_default() {
        let coll = DiagnosticCollection::default();
        assert!(coll.is_empty());
    }

    #[test]
    fn diagnostic_collection_mixed_severities() {
        let mut coll = DiagnosticCollection::new();
        coll.error(DiagnosticCode::JsonParseError, "err", Range::default());
        coll.warning(DiagnosticCode::DuplicateSection, "warn", Range::default());
        assert!(coll.has_errors());
        assert!(coll.has_warnings());
        assert_eq!(coll.errors().count(), 1);
        assert_eq!(coll.warnings().count(), 1);
    }

    #[test]
    fn diagnostic_with_suggestions() {
        let diag = Diagnostic::error(DiagnosticCode::JsonParseError, "err", Range::default())
            .with_suggestions(vec!["fix1".into(), "fix2".into()]);
        assert_eq!(diag.suggestions.len(), 2);

        let diag2 = Diagnostic::error(DiagnosticCode::JsonParseError, "err", Range::default())
            .with_context("ctx")
            .with_suggestion("fix");
        assert_eq!(diag2.context, Some("ctx".into()));
    }

    #[test]
    fn diagnostic_file_and_source() {
        let diag = Diagnostic::error(DiagnosticCode::JsonParseError, "err", Range::default())
            .with_file("test.gctf");
        assert_eq!(diag.file, Some("test.gctf".into()));
        assert_eq!(diag.source, Some("grpctestify".into()));
    }

    #[test]
    fn diagnostic_with_related_info() {
        let loc = DiagnosticLocation {
            file: "ref.gctf".into(),
            range: Range::at_line(5),
        };
        let diag = Diagnostic::error(DiagnosticCode::JsonParseError, "err", Range::default())
            .with_related_info(loc, "related message");
        assert_eq!(diag.related_information.len(), 1);
        assert_eq!(diag.related_information[0].message, "related message");
    }
}
