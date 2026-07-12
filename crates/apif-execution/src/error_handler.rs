use prost::Message;
use prost_types::Any;
use serde_json::{Map, Value};

#[derive(Clone, PartialEq, Message)]
struct GoogleRpcStatus {
    #[prost(int32, tag = "1")]
    code: i32,
    #[prost(string, tag = "2")]
    message: String,
    #[prost(message, repeated, tag = "3")]
    details: Vec<Any>,
}

pub struct ErrorHandler;

impl ErrorHandler {
    pub fn status_matches_expected(
        status: &apif_grpc_transport::GrpcError,
        expected: &Value,
    ) -> bool {
        Self::status_matches_expected_with_options(status, expected, false)
    }
    pub fn status_matches_expected_with_options(
        status: &apif_grpc_transport::GrpcError,
        expected: &Value,
        partial: bool,
    ) -> bool {
        if !partial && !Self::status_has_no_unexpected_top_level_fields(status, expected) {
            return false;
        }
        if partial {
            return Self::is_subset_match(&Self::status_to_json(status), expected);
        }
        if !Self::message_matches_status(status, expected)
            || !Self::code_matches_status(status, expected)
        {
            return false;
        }
        if !expected.get("details").is_some() {
            return status.details().is_empty();
        }
        let Some(actual) = Self::decode_status_details(status.details()) else {
            return false;
        };
        Self::compare_details(expected, actual).is_none()
    }
    pub fn status_details_json(status: &apif_grpc_transport::GrpcError) -> Value {
        match Self::decode_status_details(status.details()) {
            Some(d) => Value::Array(d),
            None => Value::Null,
        }
    }
    pub fn status_to_json(status: &apif_grpc_transport::GrpcError) -> Value {
        let mut obj = Map::with_capacity(3);
        obj.insert("code".into(), Value::from(status.code() as i64));
        obj.insert("message".into(), Value::from(status.message()));
        if let Some(details) = Self::decode_status_details(status.details())
            && !details.is_empty()
        {
            obj.insert("details".into(), Value::Array(details));
        }
        Value::Object(obj)
    }
    pub fn status_mismatch_reason(
        status: &apif_grpc_transport::GrpcError,
        expected: &Value,
    ) -> Option<String> {
        Self::status_mismatch_reason_with_options(status, expected, false)
    }
    pub fn status_mismatch_reason_with_options(
        status: &apif_grpc_transport::GrpcError,
        expected: &Value,
        partial: bool,
    ) -> Option<String> {
        if !partial
            && let Some(r) = Self::status_unexpected_top_level_field_reason(status, expected)
        {
            return Some(r);
        }
        if partial {
            let actual = Self::status_to_json(status);
            return Self::partial_mismatch_reason(&actual, expected);
        }
        let mut reasons = Vec::new();
        if !Self::code_matches_status(status, expected) {
            reasons.push(format!("code mismatch"));
        }
        if !Self::message_matches_status(status, expected) {
            reasons.push(format!("message mismatch"));
        }
        if reasons.is_empty()
            && let Some(actual) = Self::decode_status_details(status.details())
        {
            if let Some(r) = Self::details_mismatch_reason(expected, actual) {
                reasons.push(r);
            }
        }
        if reasons.is_empty() {
            None
        } else {
            Some(reasons.join("; "))
        }
    }
    fn status_has_no_unexpected_top_level_fields(
        status: &apif_grpc_transport::GrpcError,
        expected: &Value,
    ) -> bool {
        Self::status_unexpected_top_level_field_reason(status, expected).is_none()
    }
    fn status_unexpected_top_level_field_reason(
        status: &apif_grpc_transport::GrpcError,
        expected: &Value,
    ) -> Option<String> {
        let actual = Self::status_to_json(status);
        let Some(actual_obj) = actual.as_object() else {
            return None;
        };
        let Some(expected_obj) = expected.as_object() else {
            return None;
        };
        for (k, _) in actual_obj {
            if !expected_obj.contains_key(k) {
                return Some(format!("Unexpected field '{}' in gRPC status JSON", k));
            }
        }
        None
    }
    fn message_matches_status(status: &apif_grpc_transport::GrpcError, expected: &Value) -> bool {
        expected
            .get("message")
            .map(|m| m.as_str())
            .flatten()
            .map(|exp_msg| status.message().contains(exp_msg))
            .unwrap_or(true)
    }
    fn code_matches_status(status: &apif_grpc_transport::GrpcError, expected: &Value) -> bool {
        expected
            .get("code")
            .and_then(|c| c.as_i64())
            .map(|exp_code| status.code() as i64 == exp_code)
            .unwrap_or(true)
    }
    pub fn grpc_code_name_from_numeric(code: i64) -> Option<&'static str> {
        Some(match code {
            0 => "OK",
            1 => "CANCELLED",
            2 => "UNKNOWN",
            3 => "INVALID_ARGUMENT",
            4 => "DEADLINE_EXCEEDED",
            5 => "NOT_FOUND",
            6 => "ALREADY_EXISTS",
            7 => "PERMISSION_DENIED",
            8 => "RESOURCE_EXHAUSTED",
            9 => "FAILED_PRECONDITION",
            10 => "ABORTED",
            11 => "OUT_OF_RANGE",
            12 => "UNIMPLEMENTED",
            13 => "INTERNAL",
            14 => "UNAVAILABLE",
            15 => "DATA_LOSS",
            16 => "UNAUTHENTICATED",
            _ => return None,
        })
    }
    pub fn error_matches_expected(error_text: &str, expected: &Value) -> bool {
        if let Value::Object(obj) = expected {
            if let Some(code_val) = obj.get("code").and_then(|c| c.as_i64()) {
                if let Some(msg_val) = obj.get("message").and_then(|m| m.as_str()) {
                    return error_text.contains(&format!("code={}", code_val))
                        && error_text.contains(msg_val);
                }
            }
        }
        false
    }

    fn decode_status_details(details: &[u8]) -> Option<Vec<Value>> {
        if details.is_empty() {
            return None;
        }
        let Ok(status) = GoogleRpcStatus::decode(details) else {
            return None;
        };
        let mut items = Vec::new();
        for any in status.details {
            if let Ok(msg) = GoogleRpcErrorInfo::decode(any.value.as_slice()) {
                let mut obj = Map::new();
                obj.insert("reason".into(), Value::String(msg.reason));
                obj.insert("domain".into(), Value::String(msg.domain));
                let meta: Map<String, Value> = msg
                    .metadata
                    .into_iter()
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect();
                obj.insert("metadata".into(), Value::Object(meta));
                items.push(Value::Object(obj));
            }
        }
        if items.is_empty() { None } else { Some(items) }
    }
    fn compare_details(expected: &Value, actual: Vec<Value>) -> Option<String> {
        let expected_details = expected.get("details").and_then(|d| d.as_array())?;
        if expected_details.len() != actual.len() {
            return Some(format!(
                "details count mismatch: expected {} got {}",
                expected_details.len(),
                actual.len()
            ));
        }
        None
    }
    fn details_mismatch_reason(expected: &Value, actual: Vec<Value>) -> Option<String> {
        Self::compare_details(expected, actual)
    }
    fn is_subset_match(actual: &Value, expected: &Value) -> bool {
        match (actual, expected) {
            (Value::Object(a), Value::Object(e)) => e.iter().all(|(k, v)| {
                a.get(k)
                    .map(|av| Self::is_subset_match(av, v))
                    .unwrap_or(false)
            }),
            (Value::Array(a), Value::Array(e)) => e
                .iter()
                .all(|ev| a.iter().any(|av| Self::is_subset_match(av, ev))),
            _ => actual == expected,
        }
    }
    fn partial_mismatch_reason(actual: &Value, expected: &Value) -> Option<String> {
        if Self::is_subset_match(actual, expected) {
            None
        } else {
            Some("partial match failed".to_string())
        }
    }
}

#[derive(Clone, PartialEq, Message)]
struct GoogleRpcErrorInfo {
    #[prost(string, tag = "1")]
    reason: String,
    #[prost(string, tag = "2")]
    domain: String,
    #[prost(map(string, string), tag = "3")]
    metadata: std::collections::HashMap<String, String>,
}
