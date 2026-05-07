// Cross-platform file utilities

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::utils::gctf_style::trailing_blank_line_count;

/// File utilities for cross-platform operations
pub struct FileUtils;

impl FileUtils {
    /// Collect all .gctf files from a directory, optionally excluding patterns
    pub fn collect_test_files(path: &Path, exclude_patterns: &[String]) -> Vec<PathBuf> {
        let mut files = Vec::new();

        if path.is_file() {
            if path.extension().is_some_and(|e| e == "gctf")
                && !Self::is_excluded(path, exclude_patterns)
            {
                files.push(path.to_path_buf());
            }
        } else if path.is_dir() {
            // Use walkdir for cross-platform traversal
            let walker = walkdir::WalkDir::new(path).into_iter().filter_entry(|e| {
                // Always include the root directory itself, even if it starts with '.'
                if e.depth() == 0 {
                    return true;
                }
                if e.file_name().to_string_lossy().starts_with('.') {
                    return false;
                }
                // Check if this entry should be excluded
                !Self::is_excluded(e.path(), exclude_patterns)
            });

            for entry in walker.flatten() {
                if entry.file_type().is_file()
                    && let Some(ext) = entry.path().extension()
                    && ext == "gctf"
                    && !Self::is_excluded(entry.path(), exclude_patterns)
                {
                    files.push(entry.path().to_path_buf());
                }
            }
        }

        files
    }

    /// Check if a path matches any of the exclude patterns
    fn is_excluded(path: &Path, exclude_patterns: &[String]) -> bool {
        if exclude_patterns.is_empty() {
            return false;
        }

        let path_str = path.to_string_lossy();
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();

        for pattern in exclude_patterns {
            // Try glob matching
            if let Ok(glob) = globset::Glob::new(pattern) {
                let matcher = glob.compile_matcher();
                if matcher.is_match(path)
                    || matcher.is_match(&*path_str)
                    || matcher.is_match(&*file_name)
                {
                    return true;
                }
            }
            // Simple string contains check for basic patterns
            if path_str.contains(pattern) || file_name.contains(pattern) {
                return true;
            }
        }

        false
    }

    /// Sort files by given criteria
    pub fn sort_files(files: &mut [PathBuf], sort_by: &str) {
        match sort_by {
            "name" => files.sort_by(|a, b| a.file_name().cmp(&b.file_name())),
            "size" => files.sort_by_key(|a| Self::get_file_size(a).unwrap_or(0)),
            "mtime" => files.sort_by_key(|a| Self::get_mtime(a).unwrap_or(0)),
            "random" => {
                use rand::seq::SliceRandom;
                let mut rng = rand::thread_rng();
                files.shuffle(&mut rng);
            }
            _ => files.sort(), // Default path sort
        }
    }

    /// Get file modification time (cross-platform)
    pub fn get_mtime(path: &Path) -> Result<i64> {
        use std::fs::metadata;
        use std::time::UNIX_EPOCH;
        let metadata =
            metadata(path).context(format!("Failed to get metadata for: {}", path.display()))?;
        Ok(metadata.modified()?.duration_since(UNIX_EPOCH)?.as_secs() as i64)
    }

    /// Get file size (cross-platform)
    pub fn get_file_size(path: &Path) -> Result<u64> {
        use std::fs;
        let metadata =
            fs::metadata(path).context(format!("Failed to get size for: {}", path.display()))?;
        Ok(metadata.len())
    }

    /// Resolve a path relative to a base file path
    pub fn resolve_relative_path(base_file_path: &Path, relative_path: &str) -> PathBuf {
        let path = Path::new(relative_path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            // Get parent directory of the base file
            let base_dir = base_file_path.parent().unwrap_or_else(|| Path::new("."));
            base_dir.join(path)
        }
    }

    /// Update a test file with captured responses (Snapshot Mode)
    pub fn update_test_file(
        path: &Path,
        document: &crate::parser::GctfDocument,
        response: &crate::grpc::GrpcResponse,
    ) -> Result<()> {
        let content = std::fs::read_to_string(path)?;
        let lines: Vec<&str> = content.lines().collect();
        let mut new_lines = Vec::new();

        let mut current_line = 0;
        let mut msg_idx = 0;

        use crate::parser::ast::SectionType;

        // Iterate over existing sections
        for section in &document.sections {
            // Add lines before this section.
            // Parser stores 0-based line indexes for section boundaries.
            while current_line < section.start_line {
                if current_line < lines.len() {
                    new_lines.push(lines[current_line].to_string());
                }
                current_line += 1;
            }

            match section.section_type {
                SectionType::Response => {
                    // Replace this section with updated response
                    let expected_count = match &section.content {
                        crate::parser::ast::SectionContent::JsonLines(values) => {
                            values.len().max(1)
                        }
                        _ => 1,
                    };

                    let remaining = response.messages.len().saturating_sub(msg_idx);
                    let write_count = expected_count.min(remaining);

                    if write_count > 0 {
                        new_lines.push(section.format_header());

                        for idx in 0..write_count {
                            let msg = &response.messages[msg_idx + idx];
                            let json_str = serde_json::to_string_pretty(msg)?;
                            new_lines.push(json_str);
                        }
                        msg_idx += write_count;

                        let content_start = section.start_line.saturating_add(1);
                        let trailing_blanks =
                            trailing_blank_line_count(&lines, content_start, section.end_line);
                        for _ in 0..trailing_blanks {
                            new_lines.push(String::new());
                        }

                        // Skip original lines of this section
                        current_line = section.end_line;
                    } else {
                        // Copy original content if no message available
                        while current_line < section.end_line {
                            if current_line < lines.len() {
                                new_lines.push(lines[current_line].to_string());
                            }
                            current_line += 1;
                        }
                    }
                }
                _ => {
                    // Copy other sections as is
                    while current_line < section.end_line {
                        if current_line < lines.len() {
                            new_lines.push(lines[current_line].to_string());
                        }
                        current_line += 1;
                    }
                }
            }
        }

        // Copy remaining lines from original file
        while current_line < lines.len() {
            new_lines.push(lines[current_line].to_string());
            current_line += 1;
        }

        // Append remaining messages as new RESPONSE sections
        while msg_idx < response.messages.len() {
            let msg = &response.messages[msg_idx];
            msg_idx += 1;

            new_lines.push(String::new()); // Empty line separator
            new_lines.push(format!("--- {} ---", SectionType::Response.as_str()));
            let json_str = serde_json::to_string_pretty(msg)?;
            new_lines.push(json_str);
        }

        // If there was an error, append ERROR section?
        // Only if not already present? (We didn't check for ERROR section in loop above)
        // If there is an ERROR section, we should probably update it too?
        // But for simplicity, let's just append if error occurred and wasn't handled.
        // Or if we want to be robust, we should handle SectionType::Error in the loop.

        if let Some(err) = &response.error {
            // Check if we already have an Error section at the end? Hard to know without tracking.
            // Just append it.
            new_lines.push(String::new());
            new_lines.push(format!("--- {} ---", SectionType::Error.as_str()));
            new_lines.push(serde_json::to_string(err)?);
        }

        // Write back to file
        let new_content = new_lines.join("\n");
        // Ensure trailing newline
        let final_content = if new_content.ends_with('\n') {
            new_content
        } else {
            new_content + "\n"
        };

        std::fs::write(path, final_content)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::polyfill::runtime;
    use tempfile::NamedTempFile;

    #[test]
    fn test_collect_test_files_single() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let file = tempfile::Builder::new().suffix(".gctf").tempfile().unwrap();
        let path = file.path();

        let files = FileUtils::collect_test_files(path, &[]);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], path);
    }

    #[test]
    fn test_collect_test_files_non_gctf() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let file = tempfile::Builder::new().suffix(".txt").tempfile().unwrap();
        let path = file.path();

        let files = FileUtils::collect_test_files(path, &[]);
        assert_eq!(files.len(), 0);
    }

    #[test]
    fn test_collect_test_files_directory() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("test.gctf");
        std::fs::write(&test_file, "test").unwrap();

        let files = FileUtils::collect_test_files(dir.path(), &[]);
        assert_eq!(files.len(), 1);
        assert!(files.contains(&test_file));
    }

    #[test]
    fn test_collect_test_files_directory_multiple() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file1 = dir.path().join("test1.gctf");
        let file2 = dir.path().join("test2.gctf");
        let file3 = dir.path().join("other.txt");
        std::fs::write(&file1, "test1").unwrap();
        std::fs::write(&file2, "test2").unwrap();
        std::fs::write(&file3, "other").unwrap();

        let files = FileUtils::collect_test_files(dir.path(), &[]);
        assert_eq!(files.len(), 2);
        assert!(files.contains(&file1));
        assert!(files.contains(&file2));
        assert!(!files.contains(&file3));
    }

    #[test]
    fn test_collect_test_files_directory_nested() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        let nested_file = subdir.join("nested.gctf");
        std::fs::write(&nested_file, "nested").unwrap();

        let files = FileUtils::collect_test_files(dir.path(), &[]);
        assert_eq!(files.len(), 1);
        assert!(files.contains(&nested_file));
    }

    #[test]
    fn test_collect_test_files_exclude_directory() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let excluded_dir = dir.path().join("excluded");
        let included_dir = dir.path().join("included");
        std::fs::create_dir(&excluded_dir).unwrap();
        std::fs::create_dir(&included_dir).unwrap();

        let excluded_file = excluded_dir.join("excluded.gctf");
        let included_file = included_dir.join("included.gctf");
        std::fs::write(&excluded_file, "excluded").unwrap();
        std::fs::write(&included_file, "included").unwrap();

        // Exclude 'excluded' directory
        let files = FileUtils::collect_test_files(dir.path(), &["excluded".to_string()]);
        assert_eq!(files.len(), 1);
        assert!(files.contains(&included_file));
        assert!(!files.contains(&excluded_file));
    }

    #[test]
    fn test_collect_test_files_exclude_file() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file1 = dir.path().join("test1.gctf");
        let file2 = dir.path().join("test2.gctf");
        std::fs::write(&file1, "test1").unwrap();
        std::fs::write(&file2, "test2").unwrap();

        // Exclude test1.gctf
        let files = FileUtils::collect_test_files(dir.path(), &["test1.gctf".to_string()]);
        assert_eq!(files.len(), 1);
        assert!(files.contains(&file2));
        assert!(!files.contains(&file1));
    }

    #[test]
    fn test_collect_test_files_exclude_glob() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file1 = dir.path().join("test_001.gctf");
        let file2 = dir.path().join("test_002.gctf");
        let file3 = dir.path().join("prod.gctf");
        std::fs::write(&file1, "test1").unwrap();
        std::fs::write(&file2, "test2").unwrap();
        std::fs::write(&file3, "prod").unwrap();

        // Exclude files matching test_*.gctf
        let files = FileUtils::collect_test_files(dir.path(), &["test_*.gctf".to_string()]);
        assert_eq!(files.len(), 1);
        assert!(files.contains(&file3));
        assert!(!files.contains(&file1));
        assert!(!files.contains(&file2));
    }

    #[test]
    fn test_collect_test_files_exclude_multiple_patterns() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file1 = dir.path().join("test1.gctf");
        let file2 = dir.path().join("test2.gctf");
        let file3 = dir.path().join("prod.gctf");
        std::fs::write(&file1, "test1").unwrap();
        std::fs::write(&file2, "test2").unwrap();
        std::fs::write(&file3, "prod").unwrap();

        // Exclude with multiple patterns
        let files = FileUtils::collect_test_files(
            dir.path(),
            &["test1.gctf".to_string(), "test2.gctf".to_string()],
        );
        assert_eq!(files.len(), 1);
        assert!(files.contains(&file3));
        assert!(!files.contains(&file1));
        assert!(!files.contains(&file2));
    }

    #[test]
    fn test_collect_test_files_no_exclude() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file1 = dir.path().join("test1.gctf");
        let file2 = dir.path().join("test2.gctf");
        std::fs::write(&file1, "test1").unwrap();
        std::fs::write(&file2, "test2").unwrap();

        // Empty exclude list should include all files
        let files = FileUtils::collect_test_files(dir.path(), &[]);
        assert_eq!(files.len(), 2);
        assert!(files.contains(&file1));
        assert!(files.contains(&file2));
    }

    #[test]
    fn test_sort_files_by_name() {
        let mut files = vec![
            PathBuf::from("z.gctf"),
            PathBuf::from("a.gctf"),
            PathBuf::from("m.gctf"),
        ];
        FileUtils::sort_files(&mut files, "name");
        assert_eq!(files[0].file_name().unwrap(), "a.gctf");
        assert_eq!(files[1].file_name().unwrap(), "m.gctf");
        assert_eq!(files[2].file_name().unwrap(), "z.gctf");
    }

    #[test]
    fn test_sort_files_by_size() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small.gctf");
        let large = dir.path().join("large.gctf");
        std::fs::write(&small, "small").unwrap();
        std::fs::write(&large, "larger content").unwrap();

        let mut files = vec![large.clone(), small.clone()];
        FileUtils::sort_files(&mut files, "size");
        assert_eq!(files[0], small);
        assert_eq!(files[1], large);
    }

    #[test]
    fn test_sort_files_by_mtime() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.gctf");
        let new = dir.path().join("new.gctf");
        std::fs::write(&old, "old").unwrap();
        // Ensure different mtime by sleeping longer
        std::thread::sleep(std::time::Duration::from_secs(1));
        std::fs::write(&new, "new").unwrap();

        let mut files = vec![new.clone(), old.clone()];
        FileUtils::sort_files(&mut files, "mtime");
        // Older file should come first
        assert!(files[0].file_name() == Some(std::ffi::OsStr::new("old.gctf")));
        assert!(files[1].file_name() == Some(std::ffi::OsStr::new("new.gctf")));
    }

    #[test]
    fn test_sort_files_random() {
        let mut files = vec![
            PathBuf::from("a.gctf"),
            PathBuf::from("b.gctf"),
            PathBuf::from("c.gctf"),
        ];
        let original = files.clone();
        FileUtils::sort_files(&mut files, "random");
        // Random shuffle should still have same elements
        assert_eq!(files.len(), original.len());
        for f in &original {
            assert!(files.contains(f));
        }
    }

    #[test]
    fn test_sort_files_default() {
        let mut files = vec![PathBuf::from("z.gctf"), PathBuf::from("a.gctf")];
        FileUtils::sort_files(&mut files, "unknown");
        // Default should sort by path
        assert!(files[0].to_string_lossy() < files[1].to_string_lossy());
    }

    #[test]
    fn test_get_mtime() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let file = tempfile::Builder::new().suffix(".gctf").tempfile().unwrap();
        let mtime = FileUtils::get_mtime(file.path());
        assert!(mtime.is_ok());
        assert!(mtime.unwrap() > 0);
    }

    #[test]
    fn test_get_mtime_nonexistent() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let result = FileUtils::get_mtime(Path::new("/nonexistent/file"));
        assert!(result.is_err());
    }

    #[test]
    fn test_get_file_size() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let file = tempfile::Builder::new().suffix(".gctf").tempfile().unwrap();
        std::fs::write(file.path(), "hello").unwrap();
        let size = FileUtils::get_file_size(file.path());
        assert!(size.is_ok());
        assert_eq!(size.unwrap(), 5);
    }

    #[test]
    fn test_get_file_size_nonexistent() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }
        let result = FileUtils::get_file_size(Path::new("/nonexistent/file"));
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_relative_path_absolute() {
        let base = Path::new("/base/file.gctf");
        let result = FileUtils::resolve_relative_path(base, "/absolute/path.gctf");
        assert_eq!(result, PathBuf::from("/absolute/path.gctf"));
    }

    #[test]
    fn test_resolve_relative_path_relative() {
        let base = Path::new("/base/dir/file.gctf");
        let result = FileUtils::resolve_relative_path(base, "relative/path.gctf");
        assert_eq!(result, PathBuf::from("/base/dir/relative/path.gctf"));
    }

    #[test]
    fn test_resolve_relative_path_parent_missing() {
        let base = Path::new("file.gctf");
        let result = FileUtils::resolve_relative_path(base, "relative.gctf");
        assert_eq!(result, PathBuf::from("relative.gctf"));
    }

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
            headers: std::collections::HashMap::new(),
            trailers: std::collections::HashMap::new(),
            messages: vec![json!({"result": "new"})],
            error: None,
        };

        let temp_file = NamedTempFile::new().unwrap();
        let content = "--- ENDPOINT ---
Service/Method

--- RESPONSE ---
{\"result\": \"old\"}
";
        std::fs::write(temp_file.path(), content).unwrap();

        let result = FileUtils::update_test_file(temp_file.path(), &doc, &response);
        assert!(result.is_ok());

        let updated_content = std::fs::read_to_string(temp_file.path()).unwrap();
        assert!(updated_content.contains("\"result\": \"new\""));
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
            headers: std::collections::HashMap::new(),
            trailers: std::collections::HashMap::new(),
            messages: vec![serde_json::json!({"result": "new"})],
            error: None,
        };

        let result = FileUtils::update_test_file(temp_file.path(), &doc, &response);
        assert!(result.is_ok());

        let updated_content = std::fs::read_to_string(temp_file.path()).unwrap();
        assert!(updated_content.contains("\"result\": \"new\""));
    }

    #[test]
    fn test_update_test_file_preserves_response_inline_options() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }

        let temp_file = NamedTempFile::new().unwrap();
        let content = "--- ENDPOINT ---\nService/Method\n\n--- REQUEST ---\n{\"name\":\"World\"}\n\n--- RESPONSE partial=true tolerance=0.1 ---\n{\"result\": \"old\", \"extra\": 1}\n";
        std::fs::write(temp_file.path(), content).unwrap();

        let doc = crate::parser::parse_gctf(temp_file.path()).unwrap();
        let response = crate::grpc::GrpcResponse {
            headers: std::collections::HashMap::new(),
            trailers: std::collections::HashMap::new(),
            messages: vec![serde_json::json!({"result": "new"})],
            error: None,
        };

        let result = FileUtils::update_test_file(temp_file.path(), &doc, &response);
        assert!(result.is_ok());

        let updated_content = std::fs::read_to_string(temp_file.path()).unwrap();
        assert!(updated_content.contains("--- RESPONSE partial tolerance=0.1 ---"));
        assert!(updated_content.contains("\"result\": \"new\""));
    }

    #[test]
    fn test_update_test_file_matches_fmt_output() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }

        let temp_file = NamedTempFile::new().unwrap();
        let content = "--- ENDPOINT ---\nsvc.Greeter/SayHello\n\n--- REQUEST ---\n{}\n\n--- RESPONSE partial ---\n{\"result\":\"old\"}\n";
        std::fs::write(temp_file.path(), content).unwrap();

        let doc = crate::parser::parse_gctf(temp_file.path()).unwrap();
        let response = crate::grpc::GrpcResponse {
            headers: std::collections::HashMap::new(),
            trailers: std::collections::HashMap::new(),
            messages: vec![serde_json::json!({"result": "new"})],
            error: None,
        };

        FileUtils::update_test_file(temp_file.path(), &doc, &response).unwrap();
        let updated = std::fs::read_to_string(temp_file.path()).unwrap();

        let formatted = crate::commands::fmt::format_gctf_content(&updated, "temp.gctf").unwrap();
        assert_eq!(updated, formatted);
    }

    #[test]
    fn test_update_test_file_updates_jsonlines_response_count() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }

        let temp_file = NamedTempFile::new().unwrap();
        let content = "--- ENDPOINT ---\nsvc.Stream/Read\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{\"seq\":1}\n{\"seq\":2}\n";
        std::fs::write(temp_file.path(), content).unwrap();

        let doc = crate::parser::parse_gctf(temp_file.path()).unwrap();
        let response = crate::grpc::GrpcResponse {
            headers: std::collections::HashMap::new(),
            trailers: std::collections::HashMap::new(),
            messages: vec![
                serde_json::json!({"seq": 10, "ok": true}),
                serde_json::json!({"seq": 11, "ok": true}),
            ],
            error: None,
        };

        FileUtils::update_test_file(temp_file.path(), &doc, &response).unwrap();
        let updated = std::fs::read_to_string(temp_file.path()).unwrap();

        assert!(updated.contains("\"seq\": 10"));
        assert!(updated.contains("\"seq\": 11"));
        assert!(updated.contains("\"ok\": true"));
    }
}
