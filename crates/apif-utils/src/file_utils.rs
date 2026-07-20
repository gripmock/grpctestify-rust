use anyhow::Result;
use std::path::{Path, PathBuf};

/// File utilities for cross-platform operations
pub struct FileUtils;

impl FileUtils {
    /// Collect all .gctf files from a directory, optionally excluding patterns.
    /// Uses `ignore` crate which respects `.gitignore` and `.ignore` files.
    pub fn collect_test_files(path: &Path, exclude_patterns: &[String]) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let walker = ignore::WalkBuilder::new(path)
            .git_global(true)
            .git_ignore(true)
            .git_exclude(true)
            .build();
        for entry in walker.flatten() {
            let p = entry.path();
            if p.extension()
                .is_some_and(|ext| ext == "gctf" || ext == "apif")
                && !is_excluded(p, exclude_patterns)
            {
                files.push(p.to_path_buf());
            }
        }
        files
    }

    /// Sort files by name or modification time
    pub fn sort_files(files: &mut [PathBuf], sort_by: &str) {
        match sort_by {
            "name" => files.sort_by(|a, b| a.file_name().cmp(&b.file_name())),
            "mtime" | "time" => files.sort_by(|a, b| {
                Self::get_mtime(b)
                    .unwrap_or(0)
                    .cmp(&Self::get_mtime(a).unwrap_or(0))
            }),
            "random" => {
                use rand::SeedableRng;
                use rand::rngs::StdRng;
                use rand::seq::SliceRandom;
                let mut rng = StdRng::from_rng(&mut rand::rng());
                files.shuffle(&mut rng);
            }
            _ => {}
        }
    }

    /// Get file modification time as Unix timestamp
    pub fn get_mtime(path: &Path) -> Result<i64> {
        let metadata = std::fs::metadata(path)?;
        let mtime = metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        Ok(mtime)
    }

    /// Get file size in bytes
    pub fn get_file_size(path: &Path) -> Result<u64> {
        let metadata = std::fs::metadata(path)?;
        Ok(metadata.len())
    }

    /// Resolve a relative path against a base file's directory
    pub fn resolve_relative_path(base_file_path: &Path, relative_path: &str) -> PathBuf {
        let base_dir = base_file_path.parent().unwrap_or(Path::new("."));
        base_dir.join(relative_path)
    }
}

fn is_excluded(path: &Path, exclude_patterns: &[String]) -> bool {
    if exclude_patterns.is_empty() {
        return false;
    }
    let path_str = path.to_string_lossy();
    exclude_patterns.iter().any(|pattern| {
        if let Ok(glob) = globset::Glob::new(pattern).map(|g| g.compile_matcher()) {
            // A glob like `smoke*` is whole-path anchored, so it would only
            // match a bare filename, never `dir/smoke1.gctf`. Match it against
            // the full path AND every path component (including the basename)
            // so patterns such as `smoke*` exclude matching files anywhere.
            glob.is_match(path_str.as_ref())
                || path
                    .components()
                    .any(|c| glob.is_match(c.as_os_str().to_string_lossy().as_ref()))
        } else {
            path_str.contains(pattern.as_str())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn test_collect_test_files_empty_dir() {
        let dir = std::env::temp_dir().join("gctf_test_empty");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let files = FileUtils::collect_test_files(&dir, &[]);
        assert!(files.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn test_collect_test_files_with_gctf() {
        let dir = std::env::temp_dir().join("gctf_test_files");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("test.gctf"), "content").unwrap();
        fs::write(dir.join("other.txt"), "content").unwrap();
        let files = FileUtils::collect_test_files(&dir, &[]);
        assert_eq!(files.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_relative_path() {
        let base = Path::new("/home/user/tests/test.gctf");
        let resolved = FileUtils::resolve_relative_path(base, "data/file.csv");
        assert_eq!(resolved, Path::new("/home/user/tests/data/file.csv"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn test_get_file_size() {
        let dir = std::env::temp_dir().join("gctf_test_size");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("test.gctf");
        fs::write(&file, "hello").unwrap();
        assert_eq!(FileUtils::get_file_size(&file).unwrap(), 5);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_is_excluded() {
        assert!(!is_excluded(Path::new("test.gctf"), &[]));
        assert!(is_excluded(Path::new("test.gctf"), &["test*".into()]));
        assert!(!is_excluded(Path::new("other.gctf"), &["test*".into()]));
        // Glob pattern matching
        assert!(is_excluded(Path::new("test.gctf"), &["*.gctf".into()]));
        assert!(!is_excluded(Path::new("test.gctf"), &["*.txt".into()]));
    }

    // Bug 7: `smoke*` (not `**/smoke*`) must exclude matching files anywhere,
    // matching against each path component / basename, not just the full path.
    #[test]
    fn test_is_excluded_matches_basename_glob() {
        assert!(is_excluded(
            Path::new("tests/smoke_login.gctf"),
            &["smoke*".into()]
        ));
        assert!(is_excluded(
            Path::new("a/b/c/smoke1.gctf"),
            &["smoke*".into()]
        ));
        assert!(!is_excluded(
            Path::new("tests/regular.gctf"),
            &["smoke*".into()]
        ));
        // Full-path anchored globs still work.
        assert!(is_excluded(Path::new("tests/x.gctf"), &["tests/*".into()]));
        // Excluding by directory component.
        assert!(is_excluded(
            Path::new("fixtures/skip/x.gctf"),
            &["skip".into()]
        ));
    }

    #[test]
    fn test_sort_files_by_name() {
        let mut files = vec![
            PathBuf::from("b.gctf"),
            PathBuf::from("a.gctf"),
            PathBuf::from("c.gctf"),
        ];
        FileUtils::sort_files(&mut files, "name");
        assert_eq!(files[0].file_name().unwrap(), "a.gctf");
        assert_eq!(files[1].file_name().unwrap(), "b.gctf");
        assert_eq!(files[2].file_name().unwrap(), "c.gctf");
    }

    #[test]
    fn test_sort_files_unsupported() {
        let mut files = vec![PathBuf::from("b.gctf"), PathBuf::from("a.gctf")];
        FileUtils::sort_files(&mut files, "unsupported");
        // Should stay in original order
        assert_eq!(files[0].file_name().unwrap(), "b.gctf");
    }

    #[test]
    fn test_get_mtime_nonexistent() {
        let result = FileUtils::get_mtime(Path::new("/nonexistent/path.gctf"));
        assert!(result.is_err());
    }
}
