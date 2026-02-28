// Diagnostic builder and common diagnostic helpers

use super::types::*;

/// Builder for creating diagnostics with fluent API
pub struct DiagnosticBuilder {
    code: DiagnosticCode,
    severity: DiagnosticSeverity,
    message: String,
    range: Range,
    file: Option<String>,
    source: Option<String>,
    related_information: Vec<DiagnosticRelatedInformation>,
    suggestions: Vec<String>,
    context: Option<String>,
}

impl DiagnosticBuilder {
    /// Create a new error diagnostic builder
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

    /// Create a new warning diagnostic builder
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

    /// Create a new information diagnostic builder
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

    /// Create a new hint diagnostic builder
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
        file: impl Into<String>,
        range: Range,
        message: impl Into<String>,
    ) -> Self {
        self.related_information.push(DiagnosticRelatedInformation {
            location: DiagnosticLocation {
                file: file.into(),
                range,
            },
            message: message.into(),
        });
        self
    }

    /// Build the diagnostic
    pub fn build(self) -> Diagnostic {
        Diagnostic {
            code: self.code,
            severity: self.severity,
            message: self.message,
            range: self.range,
            file: self.file,
            source: self.source,
            related_information: self.related_information,
            suggestions: self.suggestions,
            context: self.context,
        }
    }
}

/// Common diagnostic helpers for GCTF files
pub struct GctfDiagnostics;

impl GctfDiagnostics {
    // Parse errors

    pub fn json_parse_error(line: usize, column: usize, error: &str) -> Diagnostic {
        DiagnosticBuilder::error(
            DiagnosticCode::JsonParseError,
            format!("Failed to parse JSON: {}", error),
            Range::new(Position::new(line, column), Position::new(line, column)),
        )
        .with_suggestion("Check for syntax errors in JSON")
        .with_suggestion("Ensure all braces and brackets are closed")
        .build()
    }

    pub fn json5_parse_error(line: usize, column: usize, error: &str) -> Diagnostic {
        DiagnosticBuilder::error(
            DiagnosticCode::Json5ParseError,
            format!("Failed to parse JSON5: {}", error),
            Range::new(Position::new(line, column), Position::new(line, column)),
        )
        .with_suggestion("Check for syntax errors in JSON5")
        .with_suggestion("JSON5 allows unquoted keys and trailing commas")
        .build()
    }

    pub fn unclosed_brace(line: usize, column: usize) -> Diagnostic {
        DiagnosticBuilder::error(
            DiagnosticCode::UnclosedBrace,
            "Unclosed brace '{'",
            Range::new(Position::new(line, column), Position::new(line, column + 1)),
        )
        .with_suggestion("Add closing brace '}'")
        .build()
    }

    pub fn unclosed_bracket(line: usize, column: usize) -> Diagnostic {
        DiagnosticBuilder::error(
            DiagnosticCode::UnclosedBracket,
            "Unclosed bracket '['",
            Range::new(Position::new(line, column), Position::new(line, column + 1)),
        )
        .with_suggestion("Add closing bracket ']'")
        .build()
    }

    pub fn unclosed_string(line: usize, column: usize) -> Diagnostic {
        DiagnosticBuilder::error(
            DiagnosticCode::UnclosedString,
            "Unclosed string literal",
            Range::new(Position::new(line, column), Position::new(line, column + 1)),
        )
        .with_suggestion("Add closing quote '\"'")
        .build()
    }

    pub fn invalid_escape(line: usize, column: usize, escape_char: char) -> Diagnostic {
        DiagnosticBuilder::error(
            DiagnosticCode::InvalidEscape,
            format!("Invalid escape sequence '\\{}'", escape_char),
            Range::new(Position::new(line, column), Position::new(line, column + 2)),
        )
        .with_suggestion(
            "Valid escape sequences: \\n, \\t, \\r, \\\\, \\\", \\/, \\b, \\f, \\uXXXX",
        )
        .build()
    }

    // Section errors

    pub fn missing_section(section_name: &str) -> Diagnostic {
        DiagnosticBuilder::error(
            DiagnosticCode::MissingSection,
            format!("Missing required section: {}", section_name),
            Range::default(),
        )
        .with_suggestion(format!("Add --- {} --- section", section_name))
        .build()
    }

    pub fn invalid_section_header(line: usize, header: &str) -> Diagnostic {
        DiagnosticBuilder::error(
            DiagnosticCode::InvalidSectionHeader,
            format!("Invalid section header: {}", header),
            Range::at_line(line),
        )
        .with_suggestion("Section headers should be: --- SECTION_NAME ---")
        .build()
    }

    pub fn duplicate_section(section_name: &str, line: usize) -> Diagnostic {
        DiagnosticBuilder::warning(
            DiagnosticCode::DuplicateSection,
            format!("Duplicate section: {}", section_name),
            Range::at_line(line),
        )
        .with_suggestion("Remove duplicate section")
        .build()
    }

    pub fn empty_section(section_name: &str, line: usize) -> Diagnostic {
        DiagnosticBuilder::warning(
            DiagnosticCode::EmptySection,
            format!("Empty section: {}", section_name),
            Range::at_line(line),
        )
        .build()
    }

    pub fn unknown_section_type(line: usize, section_name: &str) -> Diagnostic {
        DiagnosticBuilder::warning(
            DiagnosticCode::UnknownSectionType,
            format!("Unknown section type: {}", section_name),
            Range::at_line(line),
        )
        .with_suggestion("Valid sections: ADDRESS, ENDPOINT, REQUEST, RESPONSE, ERROR, EXTRACT, ASSERTS, REQUEST_HEADERS, TLS, PROTO, OPTIONS")
        .build()
    }

    // Semantic errors

    pub fn undefined_variable(var_name: &str, line: usize, column: usize) -> Diagnostic {
        DiagnosticBuilder::error(
            DiagnosticCode::UndefinedVariable,
            format!("Undefined variable: {}", var_name),
            Range::new(
                Position::new(line, column),
                Position::new(line, column + var_name.len()),
            ),
        )
        .with_suggestion("Define variable in EXTRACT section before use")
        .build()
    }

    pub fn unused_variable(var_name: &str, line: usize) -> Diagnostic {
        DiagnosticBuilder::hint(
            DiagnosticCode::UnusedVariable,
            format!("Unused variable: {}", var_name),
            Range::at_line(line),
        )
        .with_suggestion("Remove unused variable or use it in subsequent sections")
        .build()
    }

    pub fn unknown_function(func_name: &str, line: usize, column: usize) -> Diagnostic {
        DiagnosticBuilder::error(
            DiagnosticCode::UnknownFunction,
            format!("Unknown function: {}", func_name),
            Range::new(
                Position::new(line, column),
                Position::new(line, column + func_name.len()),
            ),
        )
        .with_suggestion("Available functions: @uuid, @email, @ip, @phone, @url, @header, @trailer")
        .build()
    }

    // Validation errors

    pub fn with_asserts_without_asserts(line: usize) -> Diagnostic {
        DiagnosticBuilder::warning(
            DiagnosticCode::ValidationError,
            "with_asserts option set but no ASSERTS section follows",
            Range::at_line(line),
        )
        .with_suggestion("Add ASSERTS section after this RESPONSE")
        .build()
    }

    pub fn missing_endpoint() -> Diagnostic {
        DiagnosticBuilder::error(
            DiagnosticCode::MissingRequiredField,
            "Missing required ENDPOINT section",
            Range::default(),
        )
        .with_suggestion("Add --- ENDPOINT --- section with service/method")
        .build()
    }

    pub fn missing_request_or_error() -> Diagnostic {
        DiagnosticBuilder::warning(
            DiagnosticCode::MissingRequiredField,
            "No REQUEST or ERROR section found",
            Range::default(),
        )
        .with_suggestion("Add REQUEST section for normal calls or ERROR section for error testing")
        .build()
    }

    pub fn empty_request(line: usize) -> Diagnostic {
        DiagnosticBuilder::info(
            DiagnosticCode::EmptySection,
            "Empty REQUEST section will send empty JSON object {}",
            Range::at_line(line),
        )
        .build()
    }

    pub fn empty_extract(line: usize) -> Diagnostic {
        DiagnosticBuilder::warning(
            DiagnosticCode::EmptySection,
            "EXTRACT section has no variables",
            Range::at_line(line),
        )
        .build()
    }

    pub fn empty_asserts(line: usize) -> Diagnostic {
        DiagnosticBuilder::warning(
            DiagnosticCode::EmptySection,
            "ASSERTS section has no assertions",
            Range::at_line(line),
        )
        .build()
    }
}
