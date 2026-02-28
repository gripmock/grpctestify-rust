// Error handling for test execution

use serde_json::Value;

/// Error handler for gRPC test execution
pub struct ErrorHandler;

impl ErrorHandler {
    /// Check if error matches expected error
    pub fn error_matches_expected(error_text: &str, expected: &Value) -> bool {
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
    use serde_json::json;

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
}
