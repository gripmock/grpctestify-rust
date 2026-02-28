use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub file: String,
    pub range: DiagnosticRange,
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quick_fix: Option<QuickFix>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticRange {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickFix {
    pub title: String,
    pub edits: Vec<TextEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextEdit {
    pub range: DiagnosticRange,
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckReport {
    pub diagnostics: Vec<Diagnostic>,
    pub summary: CheckSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckSummary {
    pub total_files: usize,
    pub files_with_errors: usize,
    pub total_errors: usize,
    pub total_warnings: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectReport {
    pub file: String,
    pub parse_time_ms: f64,
    pub validation_time_ms: f64,
    pub ast: AstOverview,
    pub diagnostics: Vec<Diagnostic>,
    pub semantic_diagnostics: Vec<Diagnostic>,
    pub optimization_hints: Vec<Diagnostic>,
    pub inferred_rpc_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AstOverview {
    pub sections: Vec<SectionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionInfo {
    pub section_type: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_count: Option<usize>,
}

impl Diagnostic {
    pub fn error(file: &str, code: &str, message: &str, line: usize) -> Self {
        Self {
            file: file.to_string(),
            range: DiagnosticRange {
                start: Position { line, column: 1 },
                end: Position { line, column: 1000 },
            },
            severity: DiagnosticSeverity::Error,
            code: code.to_string(),
            message: message.to_string(),
            hint: None,
            quick_fix: None,
        }
    }

    pub fn warning(file: &str, code: &str, message: &str, line: usize) -> Self {
        Self {
            file: file.to_string(),
            range: DiagnosticRange {
                start: Position { line, column: 1 },
                end: Position { line, column: 1000 },
            },
            severity: DiagnosticSeverity::Warning,
            code: code.to_string(),
            message: message.to_string(),
            hint: None,
            quick_fix: None,
        }
    }

    pub fn info(file: &str, code: &str, message: &str, line: usize) -> Self {
        Self {
            file: file.to_string(),
            range: DiagnosticRange {
                start: Position { line, column: 1 },
                end: Position { line, column: 1000 },
            },
            severity: DiagnosticSeverity::Info,
            code: code.to_string(),
            message: message.to_string(),
            hint: None,
            quick_fix: None,
        }
    }

    pub fn hint(file: &str, code: &str, message: &str, line: usize) -> Self {
        Self {
            file: file.to_string(),
            range: DiagnosticRange {
                start: Position { line, column: 1 },
                end: Position { line, column: 1000 },
            },
            severity: DiagnosticSeverity::Hint,
            code: code.to_string(),
            message: message.to_string(),
            hint: None,
            quick_fix: None,
        }
    }

    pub fn with_hint(mut self, hint: &str) -> Self {
        self.hint = Some(hint.to_string());
        self
    }

    pub fn with_quick_fix(mut self, title: &str, range: DiagnosticRange, new_text: &str) -> Self {
        self.quick_fix = Some(QuickFix {
            title: title.to_string(),
            edits: vec![TextEdit {
                range,
                new_text: new_text.to_string(),
            }],
        });
        self
    }
}
