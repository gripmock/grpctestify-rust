use crate::TestStatus;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TestMeta {
    pub name: Option<String>,
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
}

impl TestMeta {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.summary.is_none()
            && self.tags.is_empty()
            && self.owner.is_none()
            && self.links.is_empty()
    }

    pub fn from_file_meta(file_meta: &apif_ast::FileMeta) -> Self {
        Self {
            name: file_meta.name.clone(),
            summary: file_meta.summary.clone(),
            tags: file_meta.tags.clone(),
            owner: file_meta.owner.clone(),
            links: file_meta.links.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConfigSummary {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset_rows: Option<usize>,
    #[serde(skip_serializing_if = "is_false")]
    pub tls: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub proto_files: Vec<String>,
    #[serde(skip_serializing_if = "is_zero")]
    pub chain_steps: usize,
}

impl ConfigSummary {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
            && self.dataset_rows.is_none()
            && !self.tls
            && self.proto_files.is_empty()
            && self.chain_steps == 0
    }

    pub fn from_document(doc: &apif_ast::GctfDocument) -> Self {
        let mut summary = Self::default();

        for step in doc.iter_chain() {
            summary.chain_steps += 1;
            for section in &step.sections {
                let name = section.section_type.as_str().to_string();
                if !summary.sections.contains(&name) {
                    summary.sections.push(name);
                }
                match (section.section_type, &section.content) {
                    (apif_ast::SectionType::Dataset, apif_ast::SectionContent::Rows(rows)) => {
                        summary.dataset_rows = Some(summary.dataset_rows.unwrap_or(0) + rows.len());
                    }
                    (apif_ast::SectionType::Tls, _) => summary.tls = true,
                    (apif_ast::SectionType::Proto, apif_ast::SectionContent::KeyValues(kv)) => {
                        if let Some(files) = kv.get("files") {
                            for f in files.split(',') {
                                let f = f.trim().to_string();
                                if !f.is_empty() && !summary.proto_files.contains(&f) {
                                    summary.proto_files.push(f);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        summary
    }
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AssertionRecord {
    pub line: usize,
    pub expression: String,
    pub passed: bool,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct CapturedExchange {
    pub headers: BTreeMap<String, String>,
    pub trailers: BTreeMap<String, String>,
    pub response: Vec<serde_json::Value>,
    pub truncated: bool,
}

impl CapturedExchange {
    const MAX_MESSAGES: usize = 20;
    const MAX_BYTES: usize = 256 * 1024;

    #[must_use]
    pub fn capture(
        headers: HashMap<String, String>,
        trailers: HashMap<String, String>,
        messages: Vec<serde_json::Value>,
    ) -> Self {
        let headers = headers.into_iter().collect();
        let trailers = trailers.into_iter().collect();
        let mut response = Vec::new();
        let mut total_bytes = 0usize;
        let mut truncated = false;

        for msg in messages {
            if response.len() >= Self::MAX_MESSAGES {
                truncated = true;
                break;
            }
            let size = serde_json::to_string(&msg).map(|s| s.len()).unwrap_or(0);
            if total_bytes.saturating_add(size) > Self::MAX_BYTES {
                truncated = true;
                break;
            }
            total_bytes += size;
            response.push(msg);
        }

        Self {
            headers,
            trailers,
            response,
            truncated,
        }
    }
}

fn family_of(name: &str) -> String {
    apif_ast::Family::of(name).ext().to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TestResult {
    pub name: String,
    pub family: String,
    pub status: TestStatus,
    pub duration_ms: u64,
    pub call_duration_ms: Option<u64>,
    pub error_message: Option<String>,
    pub execution_time: i64,
    #[serde(default, skip_serializing_if = "TestMeta::is_empty")]
    pub meta: TestMeta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assertions: Vec<AssertionRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exchange: Option<CapturedExchange>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub retried: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub document_durations_ms: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_params: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "ConfigSummary::is_empty")]
    pub config_summary: ConfigSummary,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl TestResult {
    pub fn pass(name: impl Into<String>, duration_ms: u64, call_duration_ms: Option<u64>) -> Self {
        let name = name.into();
        Self {
            family: family_of(&name),
            name,
            status: TestStatus::Pass,
            duration_ms,
            call_duration_ms,
            error_message: None,
            execution_time: apif_cfg_runtime::now_timestamp(),
            meta: TestMeta::default(),
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
        }
    }

    pub fn pass_with_meta(
        name: impl Into<String>,
        duration_ms: u64,
        call_duration_ms: Option<u64>,
        meta: TestMeta,
    ) -> Self {
        let name = name.into();
        Self {
            family: family_of(&name),
            name,
            status: TestStatus::Pass,
            duration_ms,
            call_duration_ms,
            error_message: None,
            execution_time: apif_cfg_runtime::now_timestamp(),
            meta,
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
        }
    }

    pub fn fail(
        name: impl Into<String>,
        error_message: String,
        duration_ms: u64,
        call_duration_ms: Option<u64>,
    ) -> Self {
        let name = name.into();
        Self {
            family: family_of(&name),
            name,
            status: TestStatus::Fail,
            duration_ms,
            call_duration_ms,
            error_message: Some(error_message),
            execution_time: apif_cfg_runtime::now_timestamp(),
            meta: TestMeta::default(),
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
        }
    }

    pub fn fail_with_meta(
        name: impl Into<String>,
        error_message: String,
        duration_ms: u64,
        call_duration_ms: Option<u64>,
        meta: TestMeta,
    ) -> Self {
        let name = name.into();
        Self {
            family: family_of(&name),
            name,
            status: TestStatus::Fail,
            duration_ms,
            call_duration_ms,
            error_message: Some(error_message),
            execution_time: apif_cfg_runtime::now_timestamp(),
            meta,
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
        }
    }

    #[must_use]
    pub fn with_assertions(mut self, assertions: Vec<AssertionRecord>) -> Self {
        self.assertions = assertions;
        self
    }

    #[must_use]
    pub fn with_exchange(mut self, exchange: Option<CapturedExchange>) -> Self {
        self.exchange = exchange;
        self
    }

    #[must_use]
    pub fn with_retried(mut self, retried: bool) -> Self {
        self.retried = retried;
        self
    }

    #[must_use]
    pub fn with_document_durations(mut self, document_durations_ms: Vec<u64>) -> Self {
        self.document_durations_ms = document_durations_ms;
        self
    }

    #[must_use]
    pub fn with_row_params(mut self, row_params: Vec<(String, String)>) -> Self {
        self.row_params = row_params;
        self
    }

    #[must_use]
    pub fn with_config_summary(mut self, config_summary: ConfigSummary) -> Self {
        self.config_summary = config_summary;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_pass() {
        let result = TestResult::pass("test.gctf", 100, Some(50));
        assert_eq!(result.name, "test.gctf");
        assert_eq!(result.status, TestStatus::Pass);
        assert_eq!(result.duration_ms, 100);
        assert_eq!(result.call_duration_ms, Some(50));
        assert!(result.error_message.is_none());
    }

    #[test]
    fn result_retried_defaults_false_and_is_settable() {
        let result = TestResult::pass("test.gctf", 100, Some(50));
        assert!(!result.retried);

        let flaky = TestResult::pass("test.gctf", 100, Some(50)).with_retried(true);
        assert!(flaky.retried);
    }

    #[test]
    fn result_retried_false_not_serialized() {
        let result = TestResult::pass("test.gctf", 100, Some(50));
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            !json.contains("retried"),
            "false retried should be omitted, not written as retried:false: {json}"
        );

        let flaky = TestResult::pass("test.gctf", 100, Some(50)).with_retried(true);
        let json = serde_json::to_string(&flaky).unwrap();
        assert!(json.contains("\"retried\":true"), "json: {json}");
    }

    #[test]
    fn result_document_durations_settable_and_empty_omitted() {
        let result = TestResult::pass("test.gctf", 100, Some(50));
        assert!(result.document_durations_ms.is_empty());
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            !json.contains("document_durations_ms"),
            "empty vec should be omitted: {json}"
        );

        let chained =
            TestResult::pass("test.gctf", 100, Some(50)).with_document_durations(vec![10, 20]);
        assert_eq!(chained.document_durations_ms, vec![10, 20]);
        let json = serde_json::to_string(&chained).unwrap();
        assert!(
            json.contains("\"document_durations_ms\":[10,20]"),
            "json: {json}"
        );
    }

    #[test]
    fn result_pass_no_grpc() {
        let result = TestResult::pass("test.gctf", 100, None);
        assert_eq!(result.name, "test.gctf");
        assert_eq!(result.status, TestStatus::Pass);
        assert!(result.call_duration_ms.is_none());
    }

    #[test]
    fn result_fail() {
        let result = TestResult::fail("test.gctf", "error message".to_string(), 100, Some(50));
        assert_eq!(result.name, "test.gctf");
        assert_eq!(result.status, TestStatus::Fail);
        assert_eq!(result.duration_ms, 100);
        assert_eq!(result.call_duration_ms, Some(50));
        assert_eq!(result.error_message, Some("error message".to_string()));
    }

    #[test]
    fn result_clone() {
        let result = TestResult::pass("test.gctf", 100, Some(50));
        let cloned = result.clone();
        assert_eq!(result.name, cloned.name);
        assert_eq!(result.status, cloned.status);
    }

    #[test]
    fn result_debug() {
        let result = TestResult::pass("test.gctf", 100, Some(50));
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("test.gctf"));
        assert!(debug_str.contains("Pass"));
    }

    #[test]
    fn result_pass_with_meta() {
        let meta = TestMeta {
            name: Some("display".into()),
            summary: Some("test summary".into()),
            tags: vec!["api".into()],
            owner: Some("team".into()),
            links: vec!["http://link".into()],
        };
        let result = TestResult::pass_with_meta("test.gctf", 100, Some(50), meta.clone());
        assert_eq!(result.status, TestStatus::Pass);
        assert_eq!(result.meta, meta);
    }

    #[test]
    fn result_fail_with_meta() {
        let meta = TestMeta {
            name: Some("display".into()),
            ..Default::default()
        };
        let result = TestResult::fail_with_meta(
            "test.gctf",
            "error message".to_string(),
            100,
            Some(50),
            meta.clone(),
        );
        assert_eq!(result.status, TestStatus::Fail);
        assert_eq!(result.error_message, Some("error message".to_string()));
        assert_eq!(result.meta, meta);
    }

    #[test]
    fn meta_is_empty() {
        let meta = TestMeta::default();
        assert!(meta.is_empty());

        let meta = TestMeta {
            name: Some("test".into()),
            ..Default::default()
        };
        assert!(!meta.is_empty());
    }

    #[test]
    fn meta_is_empty_checks_owner_and_links() {
        let meta = TestMeta {
            owner: Some("team-x".into()),
            ..Default::default()
        };
        assert!(!meta.is_empty(), "owner alone must not count as empty");

        let meta = TestMeta {
            links: vec!["http://example.com".into()],
            ..Default::default()
        };
        assert!(!meta.is_empty(), "links alone must not count as empty");
    }

    #[test]
    fn meta_from_file_meta() {
        let file_meta = apif_ast::FileMeta {
            name: Some("test.gctf".into()),
            summary: Some("desc".into()),
            tags: vec!["tag".into()],
            owner: Some("me".into()),
            links: vec!["http://link".into()],
        };
        let meta = TestMeta::from_file_meta(&file_meta);
        assert_eq!(meta.name, Some("test.gctf".into()));
        assert_eq!(meta.summary, Some("desc".into()));
        assert_eq!(meta.tags, vec!["tag"]);
        assert_eq!(meta.links, vec!["http://link"]);
    }

    #[test]
    fn result_serialization() {
        let result = TestResult::pass("test.gctf", 100, Some(50));
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("Pass"));
        assert!(json.contains("test.gctf"));
    }

    fn section(
        section_type: apif_ast::SectionType,
        content: apif_ast::SectionContent,
    ) -> apif_ast::Section {
        apif_ast::Section {
            section_type,
            content,
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 1,
            end_line: 1,
            attributes: Vec::new(),
            span: apif_ast::SectionSpan::default(),
        }
    }

    #[test]
    fn config_summary_is_empty_by_default() {
        assert!(ConfigSummary::default().is_empty());
    }

    #[test]
    fn config_summary_from_document_collects_sections_dataset_tls_and_proto() {
        let mut doc = apif_ast::GctfDocument::new("t.gctf".to_string());
        doc.sections.push(section(
            apif_ast::SectionType::Address,
            apif_ast::SectionContent::Single("localhost:50051".into()),
        ));
        doc.sections.push(section(
            apif_ast::SectionType::Dataset,
            apif_ast::SectionContent::Rows(vec![
                serde_json::json!({"id": 1}),
                serde_json::json!({"id": 2}),
            ]),
        ));
        doc.sections.push(section(
            apif_ast::SectionType::Tls,
            apif_ast::SectionContent::KeyValues(apif_ast::OrderedStringMap::new()),
        ));
        doc.sections.push(section(
            apif_ast::SectionType::Proto,
            apif_ast::SectionContent::KeyValues(apif_ast::OrderedStringMap::from([(
                "files".to_string(),
                "a.proto, b.proto".to_string(),
            )])),
        ));

        let summary = ConfigSummary::from_document(&doc);
        assert_eq!(summary.sections, vec!["ADDRESS", "DATASET", "TLS", "PROTO"]);
        assert_eq!(summary.dataset_rows, Some(2));
        assert!(summary.tls);
        assert_eq!(summary.proto_files, vec!["a.proto", "b.proto"]);
        assert_eq!(summary.chain_steps, 1);
        assert!(!summary.is_empty());
    }

    #[test]
    fn config_summary_from_document_counts_chain_steps() {
        let mut first = apif_ast::GctfDocument::new("t.gctf".to_string());
        first.sections.push(section(
            apif_ast::SectionType::Endpoint,
            apif_ast::SectionContent::Single("svc.Svc/A".into()),
        ));
        let mut second = apif_ast::GctfDocument::new("t.gctf".to_string());
        second.sections.push(section(
            apif_ast::SectionType::Endpoint,
            apif_ast::SectionContent::Single("svc.Svc/B".into()),
        ));
        first.next_document = Some(Box::new(second));

        let summary = ConfigSummary::from_document(&first);
        assert_eq!(summary.chain_steps, 2);
        assert_eq!(summary.sections, vec!["ENDPOINT"]);
    }

    #[test]
    fn config_summary_is_omitted_from_json_when_empty() {
        let result = TestResult::pass("test.gctf", 100, Some(50));
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            !json.contains("config_summary"),
            "empty config_summary must be skipped: {json}"
        );
    }

    #[test]
    fn config_summary_appears_in_json_when_populated() {
        let summary = ConfigSummary {
            sections: vec!["ADDRESS".to_string()],
            tls: true,
            ..Default::default()
        };
        let result = TestResult::pass("test.gctf", 100, Some(50)).with_config_summary(summary);
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"config_summary\""));
        assert!(json.contains("\"tls\":true"));
    }
}
