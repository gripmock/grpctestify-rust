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

    /// Check if file exists and is readable
    #[allow(dead_code)]
    pub fn is_readable(path: &Path) -> bool {
        path.exists() && path.is_file()
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

    /// Read file content
    #[allow(dead_code)]
    pub fn read_file(path: &Path) -> Result<String> {
        std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))
    }

    /// Write file content
    #[allow(dead_code)]
    pub fn write_file(path: &Path, content: &str) -> Result<()> {
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write file: {}", path.display()))
    }

    /// Check if file has .gctf extension
    #[allow(dead_code)]
    pub fn is_gctf_file(path: &Path) -> bool {
        path.extension().is_some_and(|e| e == "gctf")
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

    #[test]
    fn test_collect_test_files_single() {
        let file = tempfile::Builder::new().suffix(".gctf").tempfile().unwrap();
        let path = file.path();

        let files = FileUtils::collect_test_files(path);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], path);
    }

    #[test]
    fn test_collect_test_files_directory() {
        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("test.gctf");
        std::fs::write(&test_file, "test").unwrap();

        let files = FileUtils::collect_test_files(dir.path());
        assert_eq!(files.len(), 1);
        // assert_eq!(files[0], test_file); // Order is not guaranteed by walkdir or file system
        assert!(files.contains(&test_file));
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
}
