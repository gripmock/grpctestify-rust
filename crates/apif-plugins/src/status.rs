use anyhow::Result;
use serde_json::Value;

use crate::{
    ArgTypeInfo, Plugin, PluginContext, PluginPurity, PluginResult, PluginSignature, TypeInfo,
};
use apif_assert::engine::AssertionResult;

pub const STATUS_HEADER: &str = ":status";

#[derive(Debug, Clone, Default)]
pub struct StatusPlugin;

impl Plugin for StatusPlugin {
    fn name(&self) -> &'static str {
        "status"
    }

    fn description(&self) -> &'static str {
        "The HTTP status code of the response"
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::Number,
            arg_types: &[] as &[ArgTypeInfo],
            purity: PluginPurity::ContextDependent,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: false,
            arg_names: &[],
            replacement: None,
        }
    }

    fn execute(&self, args: &[Value], context: &PluginContext) -> Result<PluginResult> {
        if !args.is_empty() {
            return Ok(PluginResult::Assertion(AssertionResult::fail(
                "@status takes no arguments",
            )));
        }

        let code = context
            .headers
            .and_then(|headers| headers.get(STATUS_HEADER))
            .and_then(|value| value.parse::<u64>().ok());

        Ok(match code {
            Some(code) => PluginResult::Value(Value::from(code)),
            None => PluginResult::Value(Value::Null),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn context<'a>(response: &'a Value, headers: &'a HashMap<String, String>) -> PluginContext<'a> {
        PluginContext::new(response).with_headers(Some(headers))
    }

    #[test]
    fn reads_the_status_a_response_arrived_with() {
        let mut headers = HashMap::new();
        headers.insert(STATUS_HEADER.to_string(), "201".to_string());
        let body = Value::Null;
        match StatusPlugin
            .execute(&[], &context(&body, &headers))
            .expect("runs")
        {
            PluginResult::Value(v) => assert_eq!(v, Value::from(201u64)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn is_null_where_there_is_no_http_response() {
        let headers = HashMap::new();
        let body = Value::Null;
        match StatusPlugin
            .execute(&[], &context(&body, &headers))
            .expect("runs")
        {
            PluginResult::Value(v) => assert_eq!(v, Value::Null),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn takes_no_arguments() {
        let headers = HashMap::new();
        let body = Value::Null;
        match StatusPlugin
            .execute(&[Value::from(200)], &context(&body, &headers))
            .expect("runs")
        {
            PluginResult::Assertion(AssertionResult::Fail { message, .. }) => {
                assert!(message.contains("no arguments"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }
}
