use apif_assert::{AssertionResult, JsonComparator};
use apif_ast::{InlineOptions, Section, SectionContent};
use serde_json::Value;

pub struct ResponseHandler {
    no_assert: bool,
}

impl ResponseHandler {
    pub fn new(no_assert: bool) -> Self {
        Self { no_assert }
    }

    pub fn validate_message(
        &self,
        actual: &Value,
        expected: &Value,
        options: &InlineOptions,
    ) -> Vec<String> {
        if self.no_assert {
            return vec![];
        }
        let diffs = JsonComparator::compare(actual, expected, options);
        diffs
            .iter()
            .filter_map(|d| match d {
                AssertionResult::Fail { message, .. } => Some(message.clone()),
                AssertionResult::Error(m) => Some(m.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn expected_values_for_section(section: &Section) -> Vec<Value> {
        match &section.content {
            SectionContent::Json(v) => vec![v.clone()],
            SectionContent::JsonLines(vals) => vals.clone(),
            _ => vec![],
        }
    }
}
