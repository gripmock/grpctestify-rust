use std::path::{Path, PathBuf};

use apif_report::{HtmlReporter, JsonReporter, JunitReporter, Reporter, YamlReporter};
use apif_state::TestResults;

use crate::report::AllureReporter;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Junit,
    Yaml,
    Html,
    Allure,
}

impl Format {
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "junit" | "xml" => Some(Self::Junit),
            "yaml" | "yml" => Some(Self::Yaml),
            "html" => Some(Self::Html),
            "allure" => Some(Self::Allure),
            _ => None,
        }
    }

    pub fn file_name(self) -> &'static str {
        match self {
            Self::Json => "report.json",
            Self::Junit => "junit.xml",
            Self::Yaml => "report.yaml",
            Self::Html => "report.html",
            Self::Allure => "allure",
        }
    }

    pub fn is_directory(self) -> bool {
        matches!(self, Self::Allure)
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Junit => "application/xml",
            Self::Yaml => "application/yaml",
            Self::Html => "text/html; charset=utf-8",
            Self::Allure => "application/octet-stream",
        }
    }

    fn reporter(self, path: PathBuf) -> Box<dyn Reporter> {
        match self {
            Self::Json => Box::new(JsonReporter::new(path)),
            Self::Junit => Box::new(JunitReporter::new(path)),
            Self::Yaml => Box::new(YamlReporter::new(path)),
            Self::Html => Box::new(HtmlReporter::new(path)),
            Self::Allure => Box::new(AllureReporter::new(path)),
        }
    }
}

pub fn dir_for(root: &Path, job_id: &str) -> PathBuf {
    let dot = root.join(".grpctestify");
    if dot.is_dir() {
        dot.join("reports").join(job_id)
    } else {
        root.join("grpctestify-reports").join(job_id)
    }
}

pub const KEEP_RUNS: usize = 20;

fn ignore_self(reports_root: &Path) {
    let marker = reports_root.join(".gitignore");
    if marker.exists() {
        return;
    }
    let _ = std::fs::write(marker, "# Written by `grpctestify play`; not source.\n*\n");
}

fn prune(reports_root: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(reports_root) else {
        return;
    };
    let mut runs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| {
            let modified = e.metadata().and_then(|m| m.modified()).ok()?;
            Some((modified, e.path()))
        })
        .collect();
    if runs.len() <= keep {
        return;
    }
    runs.sort_by_key(|run| std::cmp::Reverse(run.0));
    for (_, path) in runs.into_iter().skip(keep) {
        let _ = std::fs::remove_dir_all(path);
    }
}

pub fn write(root: &Path, job_id: &str, formats: &[Format], results: &TestResults) -> Vec<String> {
    if formats.is_empty() {
        return vec![];
    }

    let dir = dir_for(root, job_id);
    if std::fs::create_dir_all(&dir).is_err() {
        return vec![];
    }
    if let Some(reports_root) = dir.parent() {
        ignore_self(reports_root);
        prune(reports_root, KEEP_RUNS);
    }

    let mut written = Vec::new();
    for format in formats {
        let path = dir.join(format.file_name());
        let reporter = format.reporter(path);
        if format.is_directory() {
            for result in results.all() {
                reporter.on_test_end(&result.name, result);
            }
        }
        if reporter.on_suite_end(results).is_ok() {
            written.push(format.file_name().to_string());
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use apif_state::TestResult;

    fn results() -> TestResults {
        let mut results = TestResults::new();
        results.add(TestResult::pass("auth/login.gctf", 12, Some(8)));
        results.add(TestResult::fail(
            "feed/crud.gctf",
            "assertion failed".to_string(),
            7,
            Some(5),
        ));
        results
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn the_reports_directory_says_it_is_not_source() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "j-1", &[Format::Json], &results());
        let marker = dir.path().join("grpctestify-reports").join(".gitignore");
        let held = std::fs::read_to_string(&marker).expect("a .gitignore beside the runs");
        assert!(held.contains('*'), "ignores everything under it: {held}");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn only_the_last_runs_are_kept() {
        let dir = tempfile::tempdir().expect("tempdir");
        for i in 0..(KEEP_RUNS + 5) {
            write(dir.path(), &format!("j-{i}"), &[Format::Json], &results());
        }
        let kept = std::fs::read_dir(dir.path().join("grpctestify-reports"))
            .expect("reports root")
            .filter_map(Result::ok)
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .count();
        assert!(kept <= KEEP_RUNS, "kept {kept} of {}", KEEP_RUNS + 5);
        assert!(
            dir.path()
                .join("grpctestify-reports")
                .join(format!("j-{}", KEEP_RUNS + 4))
                .is_dir(),
            "the newest run survives"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn allure_is_a_directory_of_results_and_holds_the_tests() {
        let dir = tempfile::tempdir().expect("tempdir");
        let written = write(dir.path(), "j-allure", &[Format::Allure], &results());
        assert_eq!(written, vec!["allure".to_string()]);

        let out = dir
            .path()
            .join("grpctestify-reports")
            .join("j-allure")
            .join("allure");
        assert!(out.is_dir(), "allure writes a directory, not a file");
        let files: Vec<String> = std::fs::read_dir(&out)
            .expect("the allure directory")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            files.iter().filter(|n| n.ends_with("-result.json")).count(),
            2,
            "one result per test, got {files:?}"
        );
    }

    #[test]
    fn only_allure_writes_a_directory() {
        assert!(Format::Allure.is_directory());
        for format in [Format::Json, Format::Junit, Format::Yaml, Format::Html] {
            assert!(!format.is_directory(), "{} is one file", format.file_name());
        }
    }

    #[test]
    fn a_format_is_read_the_way_the_cli_spells_it() {
        assert!(matches!(Format::parse("json"), Some(Format::Json)));
        assert!(matches!(Format::parse("JUnit"), Some(Format::Junit)));
        assert!(matches!(Format::parse("yml"), Some(Format::Yaml)));
        assert!(matches!(Format::parse("allure"), Some(Format::Allure)));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn every_requested_format_lands_in_the_jobs_own_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let written = write(
            dir.path(),
            "j-1",
            &[Format::Json, Format::Junit, Format::Yaml],
            &results(),
        );
        assert_eq!(written.len(), 3);

        let reports = dir_for(dir.path(), "j-1");
        for name in ["report.json", "junit.xml", "report.yaml"] {
            let path = reports.join(name);
            assert!(path.exists(), "{name} was reported as written");
            assert!(
                std::fs::metadata(&path).expect("metadata").len() > 0,
                "{name} is empty"
            );
        }

        let json = std::fs::read_to_string(reports.join("report.json")).expect("read");
        assert!(json.contains("auth/login.gctf"), "the results are in it");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_report_does_not_turn_a_folder_into_a_project() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "j-3", &[Format::Json], &results());
        assert!(
            !dir.path().join(".grpctestify").exists(),
            "creating .grpctestify is what makes this a project — a report must not"
        );
        assert!(dir.path().join("grpctestify-reports").join("j-3").is_dir());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_project_keeps_its_reports_where_the_cli_writes_them() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join(".grpctestify")).expect("project");
        write(dir.path(), "j-4", &[Format::Json], &results());
        assert!(
            dir.path()
                .join(".grpctestify/reports/j-4/report.json")
                .exists()
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn asking_for_nothing_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(write(dir.path(), "j-2", &[], &results()).is_empty());
        assert!(!dir_for(dir.path(), "j-2").exists());
        assert!(!dir.path().join("grpctestify-reports").exists());
    }
}
