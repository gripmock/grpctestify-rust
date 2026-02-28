// LSP variable tests

use grpctestify::parser::parse_gctf_from_str;

#[test]
fn test_variable_basic() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"token": "abc123"}

--- EXTRACT ---
auth_token = .token

--- ASSERTS ---
@len({{ auth_token }}) > 0
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
fn test_variable_multiple() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"token": "abc", "user_id": 123, "name": "test"}

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
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with multiple variables should parse successfully"
    );
}

#[test]
fn test_variable_cross_request() {
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
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with cross-request variables should parse successfully"
    );
}

#[test]
fn test_variable_in_asserts() {
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
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with variable in ASSERTS should parse successfully"
    );
}

#[test]
fn test_variable_with_jq_functions() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"name": "test"}

--- RESPONSE ---
{"name": "TEST", "items": [1, 2, 3]}

--- EXTRACT ---
upper_name = .name | upper
item_count = .items | length

--- ASSERTS ---
@len({{ upper_name }}) > 0
.item_count == {{ item_count }}
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with JQ functions in EXTRACT should parse successfully"
    );
}

#[test]
fn test_variable_header_extraction() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"result": "ok"}

--- EXTRACT ---
result = .result
request_id = @header("x-request-id")

--- ASSERTS ---
.result == "ok"
@len({{ request_id }}) > 0
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with @header in EXTRACT should parse successfully"
    );
}

#[test]
fn test_variable_trailer_extraction() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"result": "ok"}

--- EXTRACT ---
result = .result
checksum = @trailer("x-checksum")

--- ASSERTS ---
.result == "ok"
@len({{ checksum }}) > 0
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with @trailer in EXTRACT should parse successfully"
    );
}
