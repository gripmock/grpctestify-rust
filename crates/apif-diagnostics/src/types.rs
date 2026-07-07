// Diagnostic types for parse and validation errors
// Foundation for LSP diagnostics

use serde::Serialize;

/// Diagnostic severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    /// Critical error that prevents execution
    Error,
    /// Warning that might cause issues
    Warning,
    /// Informational message
    Information,
    /// Hint for improvement
    Hint,
}

/// Diagnostic error codes for categorization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    // Parse errors (1000-1999)
    JsonParseError = 1001,
    Json5ParseError = 1002,
    UnexpectedEndOfFile = 1003,
    InvalidCharacter = 1004,
    UnclosedBrace = 1005,
    UnclosedBracket = 1006,
    UnclosedString = 1007,

    // Section errors (2000-2999)
    MissingSection = 2001,
    InvalidSectionHeader = 2002,
    DuplicateSection = 2003,
    EmptySection = 2004,
    InvalidSectionContent = 2005,
    UnknownSectionType = 2006,

    // Syntax errors (3000-3999)
    InvalidSyntax = 3001,
    UnexpectedToken = 3002,
    MissingToken = 3003,
    InvalidEscape = 3004,
    InvalidUnicodeEscape = 3005,

    // Semantic errors (4000-4999)
    UndefinedVariable = 4001,
    UnusedVariable = 4002,
    TypeMismatch = 4003,
    InvalidOperation = 4004,
    UnknownFunction = 4005,
    InvalidArgumentCount = 4006,
    InvalidArgumentType = 4007,

    // Validation errors (5000-5999)
    MissingRequiredField = 5001,
    InvalidFieldValue = 5002,
    ValidationError = 5003,
    ConstraintViolation = 5004,

    // LSP-specific (6000-6999)
    UndefinedSymbol = 6001,
    UnusedSymbol = 6002,
    DeprecatedSymbol = 6003,
}

impl DiagnosticCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            // Parse errors
            DiagnosticCode::JsonParseError => "json_parse_error",
            DiagnosticCode::Json5ParseError => "json5_parse_error",
            DiagnosticCode::UnexpectedEndOfFile => "unexpected_eof",
            DiagnosticCode::InvalidCharacter => "invalid_character",
            DiagnosticCode::UnclosedBrace => "unclosed_brace",
            DiagnosticCode::UnclosedBracket => "unclosed_bracket",
            DiagnosticCode::UnclosedString => "unclosed_string",

            // Section errors
            DiagnosticCode::MissingSection => "missing_section",
            DiagnosticCode::InvalidSectionHeader => "invalid_section_header",
            DiagnosticCode::DuplicateSection => "duplicate_section",
            DiagnosticCode::EmptySection => "empty_section",
            DiagnosticCode::InvalidSectionContent => "invalid_section_content",
            DiagnosticCode::UnknownSectionType => "unknown_section_type",

            // Syntax errors
            DiagnosticCode::InvalidSyntax => "invalid_syntax",
            DiagnosticCode::UnexpectedToken => "unexpected_token",
            DiagnosticCode::MissingToken => "missing_token",
            DiagnosticCode::InvalidEscape => "invalid_escape",
            DiagnosticCode::InvalidUnicodeEscape => "invalid_unicode_escape",

            // Semantic errors
            DiagnosticCode::UndefinedVariable => "undefined_variable",
            DiagnosticCode::UnusedVariable => "unused_variable",
            DiagnosticCode::TypeMismatch => "type_mismatch",
            DiagnosticCode::InvalidOperation => "invalid_operation",
            DiagnosticCode::UnknownFunction => "unknown_function",
            DiagnosticCode::InvalidArgumentCount => "invalid_argument_count",
            DiagnosticCode::InvalidArgumentType => "invalid_argument_type",

            // Validation errors
            DiagnosticCode::MissingRequiredField => "missing_required_field",
            DiagnosticCode::InvalidFieldValue => "invalid_field_value",
            DiagnosticCode::ValidationError => "validation_error",
            DiagnosticCode::ConstraintViolation => "constraint_violation",

            // LSP-specific
            DiagnosticCode::UndefinedSymbol => "undefined_symbol",
            DiagnosticCode::UnusedSymbol => "unused_symbol",
            DiagnosticCode::DeprecatedSymbol => "deprecated_symbol",
        }
    }
}

/// Source position in the document
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

/// Source range in the document
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

/// Related diagnostic information
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticRelatedInformation {
    pub location: DiagnosticLocation,
    pub message: String,
}

/// Location of a diagnostic
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticLocation {
    pub file: String,
    pub range: Range,
}

/// Main diagnostic structure
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    /// Diagnostic code for categorization
    pub code: DiagnosticCode,
    /// Severity level
    pub severity: DiagnosticSeverity,
    /// Human-readable message
    pub message: String,
    /// Source location
    pub range: Range,
    /// Optional file path (defaults to current file)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Optional source of the diagnostic
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Optional related information
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related_information: Vec<DiagnosticRelatedInformation>,
    /// Optional suggestions for fixing
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,
    /// Optional context showing the problematic code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

impl Diagnostic {
    /// Create a new error diagnostic
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

    /// Create a new warning diagnostic
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

    /// Create a new information diagnostic
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

    /// Create a new hint diagnostic
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

    /// Set the file path
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    /// Add a suggestion
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestions.push(suggestion.into());
        self
    }

    /// Add multiple suggestions
    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.suggestions.extend(suggestions);
        self
    }

    /// Set context
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Add related information
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

/// Collection of diagnostics
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

    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error)
    }

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
