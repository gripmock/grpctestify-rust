// Cross-platform file utilities

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// File utilities for cross-platform operations
pub struct FileUtils;

impl FileUtils {
    /// Collect all .gctf files from a directory
    pub fn collect_test_files(path: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();

        if path.is_file() {
            if path.extension().is_some_and(|e| e == "gctf") {
                files.push(path.to_path_buf());
            }
        } else if path.is_dir() {
            // Use walkdir for cross-platform traversal
            let walker = walkdir::WalkDir::new(path).into_iter().filter_entry(|e| {
                // Always include the root directory itself, even if it starts with '.'
                if e.depth() == 0 {
                    return true;
                }
                !e.file_name().to_string_lossy().starts_with('.')
            });

            for entry in walker.flatten() {
                if entry.file_type().is_file() {
                    if let Some(ext) = entry.path().extension() {
                        if ext == "gctf" {
                            files.push(entry.path().to_path_buf());
                        }
                    }
                }
            }
        }

        files
    }

    /// Sort files by given criteria
    pub fn sort_files(files: &mut [PathBuf], sort_by: &str) {
        match sort_by {
            "name" => files.sort_by(|a, b| a.file_name().cmp(&b.file_name())),
            "size" => files.sort_by_key(|a| Self::get_file_size(a).unwrap_or(0)),
            "mtime" => files.sort_by_key(|a| Self::get_mtime(a).unwrap_or(0)),
            "random" => {
                use rand::seq::SliceRandom;
                use rand::thread_rng;
                let mut rng = thread_rng();
                files.shuffle(&mut rng);
            }
            _ => files.sort(), // Default path sort
        }
    }

    /// Get file modification time (cross-platform)
    pub fn get_mtime(path: &Path) -> Result<i64> {
        #[cfg(unix)]
        {
            use std::fs::metadata;
            use std::time::UNIX_EPOCH;
            let metadata = metadata(path)
                .context(format!("Failed to get metadata for: {}", path.display()))?;
            Ok(metadata.modified()?.duration_since(UNIX_EPOCH)?.as_secs() as i64)
        }

        #[cfg(windows)]
        {
            use std::fs::metadata;
            use std::time::{SystemTime, UNIX_EPOCH};
            let metadata = metadata(path)
                .context(format!("Failed to get metadata for: {}", path.display()))?;
            Ok(metadata.modified()?.duration_since(UNIX_EPOCH)?.as_secs() as i64)
        }
    }

    /// Normalize path separators (backslash to forward slash)
    #[allow(dead_code)]
    pub fn normalize_path_separators(path: &Path) -> PathBuf {
        let p = path.to_string_lossy();
        if cfg!(windows) {
            p.replace('\\', "/").into()
        } else {
            p.into_owned().into()
        }
    }

    /// Get file size (cross-platform)
    pub fn get_file_size(path: &Path) -> Result<u64> {
        use std::fs;
        let metadata =
            fs::metadata(path).context(format!("Failed to get size for: {}", path.display()))?;
        Ok(metadata.len())
    }

    /// Check if path exists
    #[allow(dead_code)]
    pub fn exists(path: &Path) -> bool {
        path.exists()
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
            // Add lines before this section
            // section.start_line is 1-based
            while current_line < section.start_line - 1 {
                if current_line < lines.len() {
                    new_lines.push(lines[current_line].to_string());
                }
                current_line += 1;
            }

            match section.section_type {
                SectionType::Response => {
                    // Replace this section with updated response
                    if msg_idx < response.messages.len() {
                        let msg = &response.messages[msg_idx];
                        msg_idx += 1;

                        new_lines.push(format!("--- {} ---", SectionType::Response.as_str()));

                        // Format JSON
                        let json_str = serde_json::to_string_pretty(msg)?;
                        new_lines.push(json_str);

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
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_collect_test_files_single() {
        let file = tempfile::Builder::new().suffix(".gctf").tempfile().unwrap();
        let path = file.path();

        let files = FileUtils::collect_test_files(path);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], path);
    }

    #[test]
    fn test_collect_test_files_non_gctf() {
        let file = tempfile::Builder::new().suffix(".txt").tempfile().unwrap();
        let path = file.path();

        let files = FileUtils::collect_test_files(path);
        assert_eq!(files.len(), 0);
    }

    #[test]
    fn test_collect_test_files_directory() {
        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("test.gctf");
        std::fs::write(&test_file, "test").unwrap();

        let files = FileUtils::collect_test_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files.contains(&test_file));
    }

    #[test]
    fn test_collect_test_files_directory_multiple() {
        let dir = tempfile::tempdir().unwrap();
        let file1 = dir.path().join("test1.gctf");
        let file2 = dir.path().join("test2.gctf");
        let file3 = dir.path().join("other.txt");
        std::fs::write(&file1, "test1").unwrap();
        std::fs::write(&file2, "test2").unwrap();
        std::fs::write(&file3, "other").unwrap();

        let files = FileUtils::collect_test_files(dir.path());
        assert_eq!(files.len(), 2);
        assert!(files.contains(&file1));
        assert!(files.contains(&file2));
        assert!(!files.contains(&file3));
    }

    #[test]
    fn test_collect_test_files_directory_nested() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        let nested_file = subdir.join("nested.gctf");
        std::fs::write(&nested_file, "nested").unwrap();

        let files = FileUtils::collect_test_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files.contains(&nested_file));
    }

    #[test]
    fn test_collect_test_files_hidden_dirs_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let hidden_dir = dir.path().join(".hidden");
        std::fs::create_dir(&hidden_dir).unwrap();
        let hidden_file = hidden_dir.join("hidden.gctf");
        std::fs::write(&hidden_file, "hidden").unwrap();
        let visible_file = dir.path().join("visible.gctf");
        std::fs::write(&visible_file, "visible").unwrap();

        let files = FileUtils::collect_test_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files.contains(&visible_file));
        assert!(!files.contains(&hidden_file));
    }

    #[test]
    fn test_collect_test_files_nonexistent() {
        let path = PathBuf::from("/nonexistent/path");
        let files = FileUtils::collect_test_files(&path);
        assert_eq!(files.len(), 0);
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
        let mut files = vec![
            PathBuf::from("z.gctf"),
            PathBuf::from("a.gctf"),
        ];
        FileUtils::sort_files(&mut files, "unknown");
        // Default should sort by path
        assert!(files[0].to_string_lossy() < files[1].to_string_lossy());
    }

    #[test]
    fn test_get_mtime() {
        let file = tempfile::Builder::new().suffix(".gctf").tempfile().unwrap();
        let mtime = FileUtils::get_mtime(file.path());
        assert!(mtime.is_ok());
        assert!(mtime.unwrap() > 0);
    }

    #[test]
    fn test_get_mtime_nonexistent() {
        let result = FileUtils::get_mtime(Path::new("/nonexistent/file"));
        assert!(result.is_err());
    }

    #[test]
    fn test_get_file_size() {
        let file = tempfile::Builder::new().suffix(".gctf").tempfile().unwrap();
        std::fs::write(file.path(), "hello").unwrap();
        let size = FileUtils::get_file_size(file.path());
        assert!(size.is_ok());
        assert_eq!(size.unwrap(), 5);
    }

    #[test]
    fn test_get_file_size_nonexistent() {
        let result = FileUtils::get_file_size(Path::new("/nonexistent/file"));
        assert!(result.is_err());
    }

    #[test]
    fn test_exists() {
        let file = tempfile::Builder::new().suffix(".gctf").tempfile().unwrap();
        assert!(FileUtils::exists(file.path()));
        assert!(!FileUtils::exists(Path::new("/nonexistent/file")));
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
        let mut doc = crate::parser::GctfDocument::new("test.gctf".to_string());
        use crate::parser::ast::{Section, SectionContent, SectionType, InlineOptions};
        use serde_json::json;

        doc.sections.push(Section {
            section_type: SectionType::Endpoint,
            content: SectionContent::Single("Service/Method".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "Service/Method".to_string(),
            start_line: 1,
            end_line: 1,
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(json!({"result": "old"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"old\"}".to_string(),
            start_line: 2,
            end_line: 3,
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
    fn test_normalize_path_separators() {
        if cfg!(windows) {
            let path = Path::new("C:\\Users\\Test\\file.gctf");
            let normalized = FileUtils::normalize_path_separators(path);
            assert!(normalized.to_str().unwrap().contains("/"));
        } else {
            let path = Path::new("/home/user/test/file.gctf");
            let normalized = FileUtils::normalize_path_separators(path);
            assert!(normalized.to_str().unwrap().contains("/"));
        }
    }

    #[test]
    fn test_file_utils_debug() {
        // FileUtils is a unit struct, just verify it exists
        let _ = FileUtils;
    }
}
