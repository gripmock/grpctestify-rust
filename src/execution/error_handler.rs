// Error handling for test execution

use prost::Message;
use prost_types::Any;
use serde_json::{Map, Value, json};

#[derive(Clone, PartialEq, Message)]
struct GoogleRpcStatus {
    #[prost(int32, tag = "1")]
    code: i32,
    #[prost(string, tag = "2")]
    message: String,
    #[prost(message, repeated, tag = "3")]
    details: Vec<Any>,
}

#[derive(Clone, PartialEq, Message)]
struct GoogleRpcErrorInfo {
    #[prost(string, tag = "1")]
    reason: String,
    #[prost(string, tag = "2")]
    domain: String,
    #[prost(map = "string, string", tag = "3")]
    metadata: std::collections::HashMap<String, String>,
}

#[derive(Clone, PartialEq, Message)]
struct GoogleRpcBadRequest {
    #[prost(message, repeated, tag = "1")]
    field_violations: Vec<GoogleRpcBadRequestFieldViolation>,
}

#[derive(Clone, PartialEq, Message)]
struct GoogleRpcBadRequestFieldViolation {
    #[prost(string, tag = "1")]
    field: String,
    #[prost(string, tag = "2")]
    description: String,
}

/// Error handler for gRPC test execution
pub struct ErrorHandler;

impl ErrorHandler {
    /// Check if error matches expected error
    pub fn error_matches_expected(error_text: &str, expected: &Value) -> bool {
        Self::message_matches_error_text(error_text, expected)
            && Self::code_matches_error_text(error_text, expected)
            && expected.get("details").is_none()
    }

    /// Check if tonic::Status matches expected error JSON (supports details)
    pub fn status_matches_expected(status: &tonic::Status, expected: &Value) -> bool {
        if !Self::message_matches_status(status, expected)
            || !Self::code_matches_status(status, expected)
        {
            return false;
        }

        let expects_details = expected.get("details").is_some();
        if !expects_details {
            return status.details().is_empty();
        }

        let Some(actual_details) = Self::decode_status_details(status.details()) else {
            return false;
        };

        Self::compare_details(expected, actual_details).is_none()
    }

    /// Convert tonic::Status details to JSON array
    pub fn status_details_json(status: &tonic::Status) -> Value {
        match Self::decode_status_details(status.details()) {
            Some(details) => Value::Array(details),
            None => Value::Null,
        }
    }

    /// Convert tonic::Status to JSON object for diff output
    pub fn status_to_json(status: &tonic::Status) -> Value {
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

    /// Returns a human-readable mismatch reason for tonic::Status comparison
    pub fn status_mismatch_reason(status: &tonic::Status, expected: &Value) -> Option<String> {
        if !Self::message_matches_status(status, expected) {
            let actual = status.message();
            if let Some(expected_msg) = expected.get("message").and_then(|v| v.as_str()) {
                return Some(format!(
                    "message mismatch: expected to contain '{}', got '{}'",
                    expected_msg, actual
                ));
            }
            if expected.is_string()
                && let Some(s) = expected.as_str()
            {
                return Some(format!(
                    "message mismatch: expected to contain '{}', got '{}'",
                    s, actual
                ));
            }
            return Some("message mismatch".to_string());
        }

        if !Self::code_matches_status(status, expected) {
            if let Some(code) = expected.get("code").and_then(|v| v.as_i64()) {
                let expected_name = Self::grpc_code_name_from_numeric(code).unwrap_or("Unknown");
                return Some(format!(
                    "code mismatch: expected {} ({}), got {} ({:?})",
                    code,
                    expected_name,
                    status.code() as i64,
                    status.code()
                ));
            }
            return Some("code mismatch".to_string());
        }

        let actual_details = match Self::decode_status_details(status.details()) {
            Some(details) => details,
            None => return Some("cannot decode gRPC status details".to_string()),
        };

        Self::compare_details(expected, actual_details)
    }

    fn message_matches_error_text(error_text: &str, expected: &Value) -> bool {
        // Check message
        if let Some(expected_msg) = expected.get("message").and_then(|v| v.as_str()) {
            if !error_text.contains(expected_msg) {
                return false;
            }
        } else if expected.is_string()
            && let Some(s) = expected.as_str()
            && !error_text.contains(s)
        {
            return false;
        }

        true
    }

    fn message_matches_status(status: &tonic::Status, expected: &Value) -> bool {
        if let Some(expected_msg) = expected.get("message").and_then(|v| v.as_str()) {
            return status.message().contains(expected_msg);
        }

        if expected.is_string()
            && let Some(s) = expected.as_str()
        {
            return status.message().contains(s);
        }

        true
    }

    fn code_matches_error_text(error_text: &str, expected: &Value) -> bool {
        // Check code
        if let Some(code) = expected.get("code").and_then(|v| v.as_i64())
            && let Some(code_name) = Self::grpc_code_name_from_numeric(code)
        {
            let status_marker = format!("status: {}", code_name);
            if !error_text.contains(&status_marker)
                && !error_text.contains(&format!("code: {}", code))
            {
                return false;
            }
        }

        true
    }

    fn code_matches_status(status: &tonic::Status, expected: &Value) -> bool {
        if let Some(code) = expected.get("code").and_then(|v| v.as_i64()) {
            return status.code() as i64 == code;
        }
        true
    }

    fn compare_details(expected: &Value, actual_details: Vec<Value>) -> Option<String> {
        if let Some(expected_details) = expected.get("details") {
            let expected_array = match expected_details.as_array() {
                Some(array) => array,
                None => return Some("expected ERROR.details must be an array".to_string()),
            };

            if expected_array.len() != actual_details.len() {
                return Some(format!(
                    "details mismatch: expected {} item(s), got {}",
                    expected_array.len(),
                    actual_details.len()
                ));
            }

            for (idx, (exp, act)) in expected_array.iter().zip(actual_details.iter()).enumerate() {
                if exp != act {
                    return Some(format!(
                        "details mismatch at index {}: expected {} but got {}",
                        idx, exp, act
                    ));
                }
            }

            return None;
        }

        if expected.is_object() && !actual_details.is_empty() {
            return Some(format!(
                "backend returned details, but ERROR.details is missing in gctf; actual details: {}",
                Value::Array(actual_details)
            ));
        }

        None
    }

    fn decode_status_details(raw: &[u8]) -> Option<Vec<Value>> {
        if raw.is_empty() {
            return Some(Vec::new());
        }

        let status = GoogleRpcStatus::decode(raw).ok()?;
        Some(status.details.into_iter().map(Self::any_to_json).collect())
    }

    fn any_to_json(any: Any) -> Value {
        let type_url = any.type_url;
        let value = any.value;

        if type_url.ends_with("/google.rpc.ErrorInfo") {
            if let Ok(info) = GoogleRpcErrorInfo::decode(value.as_slice()) {
                let mut metadata = Map::new();
                for (k, v) in info.metadata {
                    metadata.insert(k, Value::String(v));
                }

                return json!({
                    "@type": type_url,
                    "reason": info.reason,
                    "domain": info.domain,
                    "metadata": metadata,
                });
            }

            return json!({
                "@type": type_url,
                "_decodeError": "failed to decode ErrorInfo",
                "_valueHex": Self::hex_encode(value.as_slice()),
            });
        }

        if type_url.ends_with("/google.rpc.BadRequest") {
            if let Ok(bad_request) = GoogleRpcBadRequest::decode(value.as_slice()) {
                let field_violations = bad_request
                    .field_violations
                    .into_iter()
                    .map(|violation| {
                        json!({
                            "field": violation.field,
                            "description": violation.description,
                        })
                    })
                    .collect::<Vec<_>>();

                return json!({
                    "@type": type_url,
                    "fieldViolations": field_violations,
                });
            }

            return json!({
                "@type": type_url,
                "_decodeError": "failed to decode BadRequest",
                "_valueHex": Self::hex_encode(value.as_slice()),
            });
        }

        json!({
            "@type": type_url,
            "_valueHex": Self::hex_encode(value.as_slice()),
        })
    }

    fn hex_encode(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            output.push(HEX[(byte >> 4) as usize] as char);
            output.push(HEX[(byte & 0x0f) as usize] as char);
        }
        output
    }

    /// Get gRPC code name from numeric code
    pub fn grpc_code_name_from_numeric(code: i64) -> Option<&'static str> {
        match code {
            0 => Some("OK"),
            1 => Some("Cancelled"),
            2 => Some("Unknown"),
            3 => Some("InvalidArgument"),
            4 => Some("DeadlineExceeded"),
            5 => Some("NotFound"),
            6 => Some("AlreadyExists"),
            7 => Some("PermissionDenied"),
            8 => Some("ResourceExhausted"),
            9 => Some("FailedPrecondition"),
            10 => Some("Aborted"),
            11 => Some("OutOfRange"),
            12 => Some("Unimplemented"),
            13 => Some("Internal"),
            14 => Some("Unavailable"),
            15 => Some("DataLoss"),
            16 => Some("Unauthenticated"),
            _ => None,
        }
    }

    /// Format error message for display
    pub fn format_error_message(_error_text: &str, expected: &Value) -> String {
        let mut parts = Vec::new();

        if let Some(msg) = expected.get("message").and_then(|v| v.as_str()) {
            parts.push(format!("expected message: {}", msg));
        }

        if let Some(code) = expected.get("code").and_then(|v| v.as_i64()) {
            if let Some(code_name) = Self::grpc_code_name_from_numeric(code) {
                parts.push(format!("expected code: {} ({})", code, code_name));
            } else {
                parts.push(format!("expected code: {}", code));
            }
        }

        if expected.get("details").is_some() {
            parts.push("expected details".to_string());
        }

        if parts.is_empty() {
            "error expected".to_string()
        } else {
            parts.join(", ")
        }
    }

    /// Check if error text contains expected code
    pub fn error_contains_code(error_text: &str, expected_code: i64) -> bool {
        if let Some(code_name) = Self::grpc_code_name_from_numeric(expected_code) {
            error_text.contains(&format!("status: {}", code_name))
                || error_text.contains(&format!("code: {}", expected_code))
        } else {
            error_text.contains(&format!("code: {}", expected_code))
        }
    }

    /// Check if error text contains expected message
    pub fn error_contains_message(error_text: &str, expected_message: &str) -> bool {
        error_text.contains(expected_message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;
    use serde_json::json;
    use tonic::Code;

    fn status_with_details() -> tonic::Status {
        let error_info = GoogleRpcErrorInfo {
            reason: "API_DISABLED".to_string(),
            domain: "your.service.com".to_string(),
            metadata: std::collections::HashMap::from([
                ("service".to_string(), "your.service.com".to_string()),
                ("consumer".to_string(), "projects/123".to_string()),
            ]),
        };

        let bad_request = GoogleRpcBadRequest {
            field_violations: vec![GoogleRpcBadRequestFieldViolation {
                field: "name".to_string(),
                description: "Name must be at least 3 characters".to_string(),
            }],
        };

        let status_proto = GoogleRpcStatus {
            code: Code::InvalidArgument as i32,
            message: "Invalid argument provided".to_string(),
            details: vec![
                Any {
                    type_url: "type.googleapis.com/google.rpc.ErrorInfo".to_string(),
                    value: error_info.encode_to_vec(),
                },
                Any {
                    type_url: "type.googleapis.com/google.rpc.BadRequest".to_string(),
                    value: bad_request.encode_to_vec(),
                },
            ],
        };

        tonic::Status::with_details(
            Code::InvalidArgument,
            "Invalid argument provided",
            status_proto.encode_to_vec().into(),
        )
    }

    fn status_without_details() -> tonic::Status {
        tonic::Status::new(Code::InvalidArgument, "Invalid argument provided")
    }

    #[test]
    fn test_error_matches_expected_message() {
        let error_text = "Error: status: NotFound, message: Resource not found";
        let expected = json!({"message": "Resource not found"});

        assert!(ErrorHandler::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn test_error_matches_expected_code() {
        let error_text = "Error: status: NotFound, message: Resource not found";
        let expected = json!({"code": 5});

        assert!(ErrorHandler::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn test_error_matches_expected_both() {
        let error_text = "Error: status: NotFound, message: Resource not found";
        let expected = json!({"code": 5, "message": "Resource not found"});

        assert!(ErrorHandler::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn test_error_matches_expected_wrong_message() {
        let error_text = "Error: status: NotFound, message: Resource not found";
        let expected = json!({"message": "Wrong message"});

        assert!(!ErrorHandler::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn test_error_matches_expected_wrong_code() {
        let error_text = "Error: status: NotFound, message: Resource not found";
        let expected = json!({"code": 3});

        assert!(!ErrorHandler::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn test_grpc_code_name_from_numeric() {
        assert_eq!(ErrorHandler::grpc_code_name_from_numeric(0), Some("OK"));
        assert_eq!(
            ErrorHandler::grpc_code_name_from_numeric(5),
            Some("NotFound")
        );
        assert_eq!(
            ErrorHandler::grpc_code_name_from_numeric(13),
            Some("Internal")
        );
        assert_eq!(ErrorHandler::grpc_code_name_from_numeric(999), None);
    }

    #[test]
    fn test_format_error_message() {
        let expected = json!({"code": 5, "message": "Resource not found"});
        let formatted = ErrorHandler::format_error_message("", &expected);

        assert!(formatted.contains("expected message: Resource not found"));
        assert!(formatted.contains("expected code: 5 (NotFound)"));
    }

    #[test]
    fn test_error_contains_code() {
        let error_text = "Error: status: NotFound, code: 5";

        assert!(ErrorHandler::error_contains_code(error_text, 5));
        assert!(!ErrorHandler::error_contains_code(error_text, 3));
    }

    #[test]
    fn test_error_contains_message() {
        let error_text = "Error: Resource not found";

        assert!(ErrorHandler::error_contains_message(
            error_text,
            "Resource not found"
        ));
        assert!(!ErrorHandler::error_contains_message(
            error_text,
            "Wrong message"
        ));
    }

    #[test]
    fn test_error_matches_expected_string() {
        let error_text = "Error: Resource not found";
        let expected = json!("Resource not found");

        assert!(ErrorHandler::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn test_status_matches_expected_with_details() {
        let status = status_with_details();
        let expected = json!({
            "code": 3,
            "message": "Invalid argument provided",
            "details": [
                {
                    "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                    "reason": "API_DISABLED",
                    "domain": "your.service.com",
                    "metadata": {
                        "service": "your.service.com",
                        "consumer": "projects/123"
                    }
                },
                {
                    "@type": "type.googleapis.com/google.rpc.BadRequest",
                    "fieldViolations": [
                        {
                            "field": "name",
                            "description": "Name must be at least 3 characters"
                        }
                    ]
                }
            ]
        });

        assert!(ErrorHandler::status_matches_expected(&status, &expected));
    }

    #[test]
    fn test_status_matches_expected_with_wrong_details() {
        let status = status_with_details();
        let expected = json!({
            "details": [
                {
                    "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                    "reason": "WRONG_REASON"
                }
            ]
        });

        assert!(!ErrorHandler::status_matches_expected(&status, &expected));
    }

    #[test]
    fn test_status_matches_expected_fails_when_actual_has_unexpected_details() {
        let status = status_with_details();
        let expected = json!({
            "code": 3,
            "message": "Invalid argument provided"
        });

        assert!(!ErrorHandler::status_matches_expected(&status, &expected));
    }

    #[test]
    fn test_status_matches_expected_fails_when_expected_requires_details() {
        let status = status_without_details();
        let expected = json!({
            "code": 3,
            "message": "Invalid argument provided",
            "details": [
                {
                    "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                    "reason": "API_DISABLED"
                }
            ]
        });

        assert!(!ErrorHandler::status_matches_expected(&status, &expected));
    }

    #[test]
    fn test_status_matches_expected_passes_when_no_details_on_both_sides() {
        let status = status_without_details();
        let expected = json!({
            "code": 3,
            "message": "Invalid argument provided"
        });

        assert!(ErrorHandler::status_matches_expected(&status, &expected));
    }

    #[test]
    fn test_status_mismatch_reason_for_unexpected_details() {
        let status = status_with_details();
        let expected = json!({
            "code": 3,
            "message": "Invalid argument provided"
        });

        let reason = ErrorHandler::status_mismatch_reason(&status, &expected).unwrap();
        assert!(reason.contains("ERROR.details is missing"));
        assert!(reason.contains("actual details"));
        assert!(reason.contains("type.googleapis.com/google.rpc.ErrorInfo"));
    }

    #[test]
    fn test_status_mismatch_reason_for_missing_required_details() {
        let status = status_without_details();
        let expected = json!({
            "code": 3,
            "message": "Invalid argument provided",
            "details": [
                {"@type": "type.googleapis.com/google.rpc.ErrorInfo"}
            ]
        });

        let reason = ErrorHandler::status_mismatch_reason(&status, &expected).unwrap();
        assert!(reason.contains("details mismatch"));
    }

    #[test]
    fn test_status_to_json_contains_details() {
        let status = status_with_details();
        let json = ErrorHandler::status_to_json(&status);
        assert_eq!(json["code"], 3);
        assert_eq!(json["message"], "Invalid argument provided");
        assert!(json.get("details").is_some());
    }

    #[test]
    fn test_status_matches_expected_fails_when_details_field_is_missing_in_expected_object() {
        let status = status_with_details();
        let expected = json!({
            "code": 3,
            "message": "Invalid argument provided",
            "details": [
                {
                    "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                    "reason": "API_DISABLED",
                    "domain": "your.service.com",
                    "metadata": {
                        "service": "your.service.com",
                        "consumer": "projects/123"
                    }
                },
                {
                    "@type": "type.googleapis.com/google.rpc.BadRequest",
                    "fieldViolations": [
                        {
                            "description": "Name must be at least 3 characters"
                        }
                    ]
                }
            ]
        });

        assert!(!ErrorHandler::status_matches_expected(&status, &expected));
        let reason = ErrorHandler::status_mismatch_reason(&status, &expected).unwrap();
        assert!(reason.contains("details mismatch at index 1"));
    }
}
