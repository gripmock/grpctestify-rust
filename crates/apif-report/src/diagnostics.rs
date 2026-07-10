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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_runtime: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bench_resolved: Option<Vec<BenchResolvedOption>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchResolvedOption {
    pub key: String,
    pub value: String,
    pub source: String,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_error() {
        let d = Diagnostic::error("test.gctf", "E001", "error msg", 5);
        assert_eq!(d.severity, DiagnosticSeverity::Error);
        assert_eq!(d.file, "test.gctf");
        assert_eq!(d.code, "E001");
        assert_eq!(d.range.start.line, 5);
    }

    #[test]
    fn test_diagnostic_warning() {
        let d = Diagnostic::warning("test.gctf", "W001", "warning msg", 10);
        assert_eq!(d.severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn test_diagnostic_info() {
        let d = Diagnostic::info("test.gctf", "I001", "info msg", 15);
        assert_eq!(d.severity, DiagnosticSeverity::Info);
    }

    #[test]
    fn test_diagnostic_hint() {
        let d = Diagnostic::hint("test.gctf", "H001", "hint msg", 20);
        assert_eq!(d.severity, DiagnosticSeverity::Hint);
    }

    #[test]
    fn test_diagnostic_with_hint() {
        let d = Diagnostic::error("test.gctf", "E001", "msg", 1).with_hint("try fixing X");
        assert_eq!(d.hint, Some("try fixing X".into()));
    }

    #[test]
    fn test_diagnostic_serialization() {
        let d = Diagnostic::error("test.gctf", "E001", "msg", 5).with_hint("hint");
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("E001"));
        assert!(json.contains("hint"));
    }

    #[test]
    fn test_diagnostic_deserialization() {
        let json = r#"{
            "file": "test.gctf",
            "range": {
                "start": {"line": 1, "column": 1},
                "end": {"line": 1, "column": 1000}
            },
            "severity": "Error",
            "code": "E001",
            "message": "test"
        }"#;
        let d: Diagnostic = serde_json::from_str(json).unwrap();
        assert_eq!(d.file, "test.gctf");
        assert_eq!(d.severity, DiagnosticSeverity::Error);
        assert!(d.hint.is_none());
    }

    #[test]
    fn test_check_report() {
        let report = CheckReport {
            diagnostics: vec![Diagnostic::error("f.gctf", "E1", "msg", 1)],
            summary: CheckSummary {
                total_files: 1,
                files_with_errors: 1,
                total_errors: 1,
                total_warnings: 0,
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("total_files"));
    }

    #[test]
    fn test_inspect_report() {
        let report = InspectReport {
            file: "test.gctf".into(),
            parse_time_ms: 1.5,
            validation_time_ms: 0.5,
            ast: AstOverview { sections: vec![] },
            diagnostics: vec![],
            semantic_diagnostics: vec![],
            optimization_hints: vec![],
            inferred_rpc_mode: Some("unary".into()),
            effective_runtime: None,
            bench_resolved: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("test.gctf"));
        assert!(json.contains("unary"));
    }

    #[test]
    fn test_range_and_position() {
        let range = DiagnosticRange {
            start: Position { line: 1, column: 5 },
            end: Position {
                line: 1,
                column: 10,
            },
        };
        assert_eq!(range.start.line, 1);
        assert_eq!(range.end.column, 10);
    }
}
