// LSP variable definition integration tests

use grpctestify::lsp::variable_definition::{
    find_variable_definition, find_variable_references, get_all_variables,
};
use tower_lsp::lsp_types::Position;

#[test]
fn test_lsp_variable_definition_basic() {
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
    let position = Position {
        line: 14,
        character: 15,
    };

    let result = find_variable_definition(content, position, "file:///test.gctf");

    // Assert
    assert!(result.is_some());
    assert_eq!(result.unwrap().name, "auth_token");
}

#[test]
fn test_lsp_variable_definition_multiple_vars() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"token": "abc", "user_id": 42}

--- EXTRACT ---
auth_token = .token
user_id = .user_id

--- ASSERTS ---
@len({{ auth_token }}) > 0
.user_id == {{ user_id }}
"#;

    // Act
    let position = Position {
        line: 15,
        character: 15,
    };

    let result = find_variable_definition(content, position, "file:///test.gctf");

    // Assert
    assert!(result.is_some());
    assert_eq!(result.unwrap().name, "auth_token");
}

#[test]
fn test_lsp_variable_definition_not_on_variable() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"token": "abc"}

--- EXTRACT ---
auth_token = .token

--- ASSERTS ---
@len({{ auth_token }}) > 0
"#;

    // Act
    let position = Position {
        line: 0,
        character: 0,
    };

    let result = find_variable_definition(content, position, "file:///test.gctf");

    // Assert
    assert!(result.is_none());
}

#[test]
fn test_lsp_variable_references() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"token": "abc"}

--- EXTRACT ---
token = .token

--- ASSERTS ---
@len({{ token }}) > 0
{{ token }} != null
"#;

    // Act
    let refs = find_variable_references(content, "token", "file:///test.gctf");

    // Assert
    assert_eq!(refs.len(), 2);
}

#[test]
fn test_lsp_get_all_variables() {
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
"#;

    // Act
    let vars = get_all_variables(content);

    // Assert
    assert_eq!(vars.len(), 3);
    assert_eq!(vars[0].0, "auth_token");
    assert_eq!(vars[1].0, "user_id");
    assert_eq!(vars[2].0, "user_name");
}

#[test]
fn test_lsp_variable_definition_cross_section() {
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
    let position = Position {
        line: 17,
        character: 25,
    };

    let result = find_variable_definition(content, position, "file:///test.gctf");

    // Assert
    assert!(result.is_some());
    assert_eq!(result.unwrap().name, "token");
}

#[test]
fn test_lsp_get_all_variables_empty() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"result": "ok"}

--- ASSERTS ---
.result == "ok"
"#;

    // Act
    let vars = get_all_variables(content);

    // Assert
    assert!(vars.is_empty());
}

#[test]
fn test_lsp_variable_references_empty() {
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

--- ASSERTS ---
.result == "ok"
"#;

    // Act
    let refs = find_variable_references(content, "nonexistent", "file:///test.gctf");

    // Assert
    assert!(refs.is_empty());
}
