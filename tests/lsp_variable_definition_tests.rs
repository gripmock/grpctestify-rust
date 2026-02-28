// LSP variable go-to-definition and hover tests

use grpctestify::execution::ExecutionPlan;
use grpctestify::parser::parse_gctf_from_str;

#[test]
fn test_variable_definition_tracking() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"token": "abc123", "user_id": 42}

--- EXTRACT ---
auth_token = .token
user_id = .user_id

--- ASSERTS ---
@len({{ auth_token }}) > 0
.user_id == {{ user_id }}
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);

    // Assert
    assert_eq!(plan.extractions.len(), 1);
    assert!(!plan.extractions[0].variables.is_empty());
}

#[test]
fn test_variable_usage_in_request_headers() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
auth.AuthService/Login

--- REQUEST ---
{"username": "user", "password": "pass"}

--- RESPONSE ---
{"access_token": "eyJ..."}

--- EXTRACT ---
token = .access_token

--- ENDPOINT ---
users.UserService/GetProfile

--- REQUEST_HEADERS ---
Authorization: Bearer {{ token }}

--- REQUEST ---
{"user_id": 123}

--- RESPONSE ---
{"name": "User"}
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);

    // Assert
    assert_eq!(plan.extractions.len(), 1);
    assert!(plan.extractions[0].variables.contains_key("token"));
}

#[test]
fn test_variable_usage_in_request() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"next_id": 456}

--- EXTRACT ---
next_id = .next_id

--- ASSERTS ---
.next_id == 456
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with EXTRACT should parse successfully"
    );
}

#[test]
fn test_variable_usage_in_asserts() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"expected": 100}

--- RESPONSE ---
{"value": 100}

--- EXTRACT ---
expected_value = .value

--- ASSERTS ---
.value == {{ expected_value }}
@len({{ expected_value }}) > 0
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);

    // Assert
    assert_eq!(plan.extractions.len(), 1);
    assert!(plan.extractions[0].variables.contains_key("expected_value"));
}

#[test]
fn test_variable_with_ternary() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"status": 200}

--- EXTRACT ---
status_label = .status == 200 ? "OK" : "Error"

--- ASSERTS ---
.status_label == "OK"
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);

    // Assert
    assert_eq!(plan.extractions.len(), 1);
    assert!(plan.extractions[0].variables.contains_key("status_label"));
}

#[test]
fn test_variable_with_header_plugin() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"result": "ok"}

--- EXTRACT ---
request_id = @header("x-request-id")

--- ASSERTS ---
@len({{ request_id }}) > 0
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);

    // Assert
    assert_eq!(plan.extractions.len(), 1);
    assert!(plan.extractions[0].variables.contains_key("request_id"));
}

#[test]
fn test_variable_with_trailer_plugin() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"result": "ok"}

--- EXTRACT ---
checksum = @trailer("x-checksum")

--- ASSERTS ---
@len({{ checksum }}) > 0
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);

    // Assert
    assert_eq!(plan.extractions.len(), 1);
    assert!(plan.extractions[0].variables.contains_key("checksum"));
}

#[test]
fn test_multiple_variables_same_section() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"token": "abc", "user_id": 42, "name": "test"}

--- EXTRACT ---
auth_token = .token
user_id = .user_id
user_name = .name

--- ASSERTS ---
@len({{ auth_token }}) > 0
.user_id == {{ user_id }}
.name == "{{ user_name }}"
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);

    // Assert
    assert_eq!(plan.extractions.len(), 1);
    assert!(plan.extractions[0].variables.contains_key("auth_token"));
    assert!(plan.extractions[0].variables.contains_key("user_id"));
    assert!(plan.extractions[0].variables.contains_key("user_name"));
}

#[test]
fn test_variable_cross_section_usage() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
auth.AuthService/Login

--- REQUEST ---
{"username": "user", "password": "pass"}

--- RESPONSE ---
{"access_token": "eyJ...", "refresh_token": "ref..."}

--- EXTRACT ---
access_token = .access_token
refresh_token = .refresh_token

--- ASSERTS ---
@len({{ access_token }}) > 0
@len({{ refresh_token }}) > 0
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with multiple variables should parse successfully"
    );
}

#[test]
fn test_variable_with_jq_expression() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"items": [1, 2, 3]}

--- RESPONSE ---
{"items": [1, 2, 3]}

--- EXTRACT ---
item_count = .items | length
first_item = .items[0]

--- ASSERTS ---
.item_count == 3
.first_item == 1
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);

    // Assert
    assert_eq!(plan.extractions.len(), 1);
    assert!(plan.extractions[0].variables.contains_key("item_count"));
    assert!(plan.extractions[0].variables.contains_key("first_item"));
}
