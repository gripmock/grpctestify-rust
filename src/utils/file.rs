use std::path::Path;

use anyhow::Result;

use apif_utils::trailing_blank_line_count;

use crate::grpc::GrpcResponse;
use crate::parser::GctfDocument;
use crate::parser::ast::{InlineOptions, SectionContent, SectionType};

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

        let remaining = response.messages.len().saturating_sub(msg_idx);

        if section.section_type == SectionType::Response && remaining > 0 {
            let with_asserts = section.inline_options.with_asserts;
            // Preserve the message count the section originally declared:
            // a streaming (JsonLines) section keeps all of its messages,
            // while `with_asserts` sections capture every remaining message.
            let expected_count = if with_asserts {
                remaining
            } else {
                match &section.content {
                    SectionContent::JsonLines(values) => values.len().max(1),
                    _ => 1,
                }
            };

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
        } else if section.section_type == SectionType::Error && response.error.is_some() {
            new_lines.push(format!(
                "--- ERROR{} ---",
                format_inline_options(&section.inline_options)
            ));
            if let Some(error_msg) = &response.error {
                new_lines.push(error_msg.clone());
            }
            current_line = section_end;
        } else {
            // Non-snapshot section, or a RESPONSE/ERROR section with no
            // captured data — keep the original content untouched.
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

    let mut new_content = new_lines.join("\n");
    if content.ends_with('\n') {
        new_content.push('\n');
    }

    write_atomic(path, &new_content)?;
    Ok(())
}

/// A file's permission bits, if it exists. `None` on a platform without them
/// or when the file is new.
#[cfg(unix)]
pub(crate) fn file_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).ok().map(|m| m.permissions().mode())
}

#[cfg(not(unix))]
pub(crate) fn file_mode(_path: &Path) -> Option<u32> {
    None
}

/// Put `mode` back on `path` after an atomic replace. `NamedTempFile` is 0600
/// by design, so without this, rewriting a file in place would quietly strip
/// group/other read access it had before.
#[cfg(unix)]
pub(crate) fn restore_mode(path: &Path, mode: Option<u32>) {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
}

#[cfg(not(unix))]
pub(crate) fn restore_mode(_path: &Path, _mode: Option<u32>) {}

/// Writes `content` to a temp file in the same directory, then atomically
/// renames it over `path` so a crash mid-write cannot corrupt the file.
pub(crate) fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    // The old `.<file>.<pid>.tmp` was pre-creatable by anyone who can write the
    // directory, and `fs::write` follows a symlink planted there.
    let mut tmp = tempfile::Builder::new()
        .prefix(".grpctestify-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    let existing_mode = file_mode(path);
    use std::io::Write;
    tmp.write_all(content.as_bytes())?;
    tmp.flush()?;
    // Close the handle before renaming: Windows refuses to move a file that is
    // still open, which is what `persist` does.
    let (file, tmp_path) = tmp.keep().map_err(|e| e.error)?;
    drop(file);
    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    restore_mode(path, existing_mode);
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

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_update_test_file() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let mut doc = crate::parser::GctfDocument::new("test.gctf".to_string());
        use crate::parser::ast::{
            InlineOptions, Section, SectionContent, SectionSpan, SectionType,
        };
        use serde_json::json;

        doc.sections.push(Section {
            section_type: SectionType::Endpoint,
            content: SectionContent::Single("Service/Method".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "Service/Method".to_string(),
            start_line: 1,
            end_line: 1,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(json!({"result": "old"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"old\"}".to_string(),
            start_line: 2,
            end_line: 3,
            attributes: Vec::new(),
            span: SectionSpan::default(),
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
        update_test_file(temp_file.path(), &doc, &response).expect("update_test_file failed");
        let updated = std::fs::read_to_string(temp_file.path()).unwrap();
        assert!(updated.contains("\"result\": \"new\""));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn update_test_file_with_parsed_zero_based_sections() {
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
        update_test_file(temp_file.path(), &doc, &response).expect("update_test_file failed");
        let updated = std::fs::read_to_string(temp_file.path()).unwrap();
        assert!(updated.contains("\"result\": \"new\""));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn update_test_file_updates_jsonlines_response_count() {
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
        update_test_file(temp_file.path(), &doc, &response).expect("update_test_file failed");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn update_test_file_preserves_streaming_message_count() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let temp_file = NamedTempFile::new().unwrap();
        // A single RESPONSE section with two streamed messages (JsonLines).
        let content = "--- ENDPOINT ---\nService/Method\n\n--- RESPONSE ---\n{\"index\": 0}\n{\"index\": 1}\n";
        std::fs::write(temp_file.path(), content).unwrap();
        let doc = crate::parser::parse_gctf(temp_file.path()).unwrap();
        let response = crate::grpc::GrpcResponse {
            headers: std::collections::HashMap::new(),
            trailers: std::collections::HashMap::new(),
            messages: vec![
                serde_json::json!({"index": 10}),
                serde_json::json!({"index": 11}),
            ],
            error: None,
        };
        update_test_file(temp_file.path(), &doc, &response).expect("update_test_file failed");
        let updated = std::fs::read_to_string(temp_file.path()).unwrap();
        // Both streamed messages must survive the rewrite, not just the first.
        assert!(updated.contains("\"index\": 10"), "updated: {updated}");
        assert!(updated.contains("\"index\": 11"), "updated: {updated}");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn update_test_file_empty_response_preserves_original_content() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let temp_file = NamedTempFile::new().unwrap();
        let content = "--- ENDPOINT ---\nService/Method\n\n--- RESPONSE ---\n{\"result\": \"old\"}\n\n--- ERROR ---\n{\"code\": 5}\n";
        std::fs::write(temp_file.path(), content).unwrap();
        let doc = crate::parser::parse_gctf(temp_file.path()).unwrap();
        // Nothing captured (e.g. server down) — snapshot must not be emptied.
        let response = crate::grpc::GrpcResponse {
            headers: HashMap::new(),
            trailers: HashMap::new(),
            messages: vec![],
            error: None,
        };
        update_test_file(temp_file.path(), &doc, &response).expect("update_test_file failed");
        let updated = std::fs::read_to_string(temp_file.path()).unwrap();
        assert!(
            updated.contains("\"result\": \"old\""),
            "updated: {updated}"
        );
        assert!(updated.contains("\"code\": 5"), "updated: {updated}");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn update_test_file_preserves_trailing_newline_and_no_temp_leftover() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snapshot.gctf");
        let content =
            "--- ENDPOINT ---\nService/Method\n\n--- RESPONSE ---\n{\"result\": \"old\"}\n";
        std::fs::write(&path, content).unwrap();
        let doc = crate::parser::parse_gctf(&path).unwrap();
        let response = crate::grpc::GrpcResponse {
            headers: HashMap::new(),
            trailers: HashMap::new(),
            messages: vec![serde_json::json!({"result": "new"})],
            error: None,
        };
        update_test_file(&path, &doc, &response).expect("update_test_file must succeed");
        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.contains("\"result\": \"new\""));
        assert!(
            updated.ends_with('\n'),
            "trailing newline must be preserved"
        );
        // Atomic write must not leave temp files behind.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files left behind: {leftovers:?}"
        );
    }
}
