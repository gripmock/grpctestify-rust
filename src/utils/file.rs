use std::fs;
use std::path::Path;

use anyhow::Result;

use apif_utils::trailing_blank_line_count;

use crate::grpc::GrpcResponse;
use crate::parser::GctfDocument;
use crate::parser::ast::{InlineOptions, SectionType};

// Re-export base FileUtils from crate — all shared methods live there
pub use apif_utils::file_utils::FileUtils;

/// Snapshot update — write actual server response back to .gctf file.
/// Local because it depends on `GrpcResponse` and `GctfDocument`.
pub fn update_test_file(
    path: &Path,
    document: &GctfDocument,
    response: &GrpcResponse,
) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let mut new_lines: Vec<String> = Vec::new();
    let mut current_line = 0;
    let mut msg_idx = 0;

    for section in &document.sections {
        let section_start = section.start_line;
        let section_end = section.end_line;

        while current_line < section_start && current_line < lines.len() {
            new_lines.push(lines[current_line].to_string());
            current_line += 1;
        }

        if section.section_type == SectionType::Response {
            let with_asserts = section.inline_options.with_asserts;
            let remaining = response.messages.len().saturating_sub(msg_idx);
            let expected_count = if with_asserts { remaining } else { 1 };

            new_lines.push(format!(
                "--- RESPONSE{} ---",
                format_inline_options(&section.inline_options)
            ));

            let content_start = new_lines.len();

            for idx in 0..expected_count {
                if let Some(msg) = response.messages.get(msg_idx + idx) {
                    let response_json = serde_json::to_string_pretty(msg)?;
                    if expected_count > 1 && idx > 0 {
                        new_lines.push(String::new());
                    }
                    for line in response_json.lines() {
                        new_lines.push(line.to_string());
                    }
                }
            }

            let blank_count = trailing_blank_line_count(&lines, content_start, section.end_line);
            for _ in 0..blank_count {
                new_lines.push(String::new());
            }

            msg_idx += expected_count.min(remaining);
            current_line = section_end;
        } else if section.section_type == SectionType::Error {
            new_lines.push(format!(
                "--- ERROR{} ---",
                format_inline_options(&section.inline_options)
            ));
            if let Some(error_msg) = &response.error {
                new_lines.push(error_msg.clone());
            } else {
                new_lines.push("{}".to_string());
            }
            current_line = section_end;
        } else {
            while current_line < section_end && current_line < lines.len() {
                new_lines.push(lines[current_line].to_string());
                current_line += 1;
            }
        }
    }

    while current_line < lines.len() {
        new_lines.push(lines[current_line].to_string());
        current_line += 1;
    }

    let new_content = new_lines.join("\n");
    fs::write(path, new_content)?;
    Ok(())
}

fn format_inline_options(options: &InlineOptions) -> String {
    let mut parts = Vec::new();
    if options.with_asserts {
        parts.push("with_asserts".to_string());
    }
    if options.partial {
        parts.push("partial".to_string());
    }
    if let Some(tol) = &options.tolerance {
        parts.push(format!("tolerance={}", tol));
    }
    if options.unordered_arrays {
        parts.push("unordered_arrays".to_string());
    }
    if !options.redact.is_empty() {
        parts.push(format!("redact=[{}]", options.redact.join(",")));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" {}", parts.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::polyfill::runtime;
    use std::collections::HashMap;
    use tempfile::NamedTempFile;

    #[test]
    fn test_update_test_file() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let mut doc = crate::parser::GctfDocument::new("test.gctf".to_string());
        use crate::parser::ast::{InlineOptions, Section, SectionContent, SectionType};
        use serde_json::json;

        doc.sections.push(Section {
            section_type: SectionType::Endpoint,
            content: SectionContent::Single("Service/Method".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "Service/Method".to_string(),
            start_line: 1,
            end_line: 1,
            attributes: Vec::new(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(json!({"result": "old"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"old\"}".to_string(),
            start_line: 2,
            end_line: 3,
            attributes: Vec::new(),
        });

        let response = crate::grpc::GrpcResponse {
            headers: HashMap::new(),
            trailers: HashMap::new(),
            messages: vec![json!({"result": "new"})],
            error: None,
        };

        let temp_file = NamedTempFile::new().unwrap();
        let content =
            "--- ENDPOINT ---\nService/Method\n\n--- RESPONSE ---\n{\"result\": \"old\"}\n";
        std::fs::write(temp_file.path(), content).unwrap();
        assert!(update_test_file(temp_file.path(), &doc, &response).is_ok());
        let updated = std::fs::read_to_string(temp_file.path()).unwrap();
        assert!(updated.contains("\"result\": \"new\""));
    }

    #[test]
    fn test_update_test_file_with_parsed_zero_based_sections() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let temp_file = NamedTempFile::new().unwrap();
        let content =
            "--- ENDPOINT ---\nService/Method\n\n--- RESPONSE ---\n{\"result\": \"old\"}\n";
        std::fs::write(temp_file.path(), content).unwrap();
        let doc = crate::parser::parse_gctf(temp_file.path()).unwrap();
        let response = crate::grpc::GrpcResponse {
            headers: HashMap::new(),
            trailers: HashMap::new(),
            messages: vec![serde_json::json!({"result": "new"})],
            error: None,
        };
        assert!(update_test_file(temp_file.path(), &doc, &response).is_ok());
        let updated = std::fs::read_to_string(temp_file.path()).unwrap();
        assert!(updated.contains("\"result\": \"new\""));
    }

    #[test]
    fn test_update_test_file_updates_jsonlines_response_count() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let temp_file = NamedTempFile::new().unwrap();
        let content = "--- ENDPOINT ---\nService/Method\n\n--- RESPONSE with_asserts ---\n{\"status\": \"old\"}\n";
        std::fs::write(temp_file.path(), content).unwrap();
        let doc = crate::parser::parse_gctf(temp_file.path()).unwrap();
        let response = crate::grpc::GrpcResponse {
            headers: HashMap::new(),
            trailers: HashMap::new(),
            messages: vec![serde_json::json!({"status": "ok"})],
            error: None,
        };
        assert!(update_test_file(temp_file.path(), &doc, &response).is_ok());
    }
}
