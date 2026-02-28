// LSP go-to-definition for variables

use crate::parser;
use tower_lsp::lsp_types::*;

/// Variable definition location
#[derive(Debug, Clone)]
pub struct VariableLocation {
    pub name: String,
    pub line: u32,
    pub character: u32,
    pub uri: String,
}

/// Find variable definition in document
pub fn find_variable_definition(
    content: &str,
    position: Position,
    uri: &str,
) -> Option<VariableLocation> {
    // Get the word at cursor
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;

    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];
    let char_idx = position.character as usize;

    if char_idx >= line.len() {
        return None;
    }

    // Check if we're on a variable reference {{ var_name }}
    if let Some(var_name) = extract_variable_at_position(line, char_idx) {
        // Find the definition in EXTRACT section
        return find_variable_in_extract(content, &var_name, uri);
    }

    None
}

/// Extract variable name from {{ var_name }} at position
pub fn extract_variable_at_position(line: &str, char_idx: usize) -> Option<String> {
    // Look for {{ before position
    let before = &line[..char_idx];
    let after = &line[char_idx..];

    // Find nearest {{ before position
    let open_start = before.rfind("{{")?;

    // Find }} after position
    let close_end = after.find("}}")?;

    // Extract variable name
    let var_content = &line[open_start + 2..char_idx + close_end];
    let var_name = var_content.trim();

    // Validate variable name (alphanumeric + underscore)
    if var_name.chars().all(|c| c.is_alphanumeric() || c == '_') && !var_name.is_empty() {
        Some(var_name.to_string())
    } else {
        None
    }
}

/// Find variable definition in EXTRACT section
fn find_variable_in_extract(content: &str, var_name: &str, uri: &str) -> Option<VariableLocation> {
    let doc = parser::parse_gctf_from_str(content, "lsp-document.gctf").ok()?;

    for section in &doc.sections {
        if section.section_type != parser::ast::SectionType::Extract {
            continue;
        }

        for (idx, line) in section.raw_content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }

            if let Some(extract_var) = parser::ExtractVar::parse(trimmed)
                && extract_var.name == var_name
            {
                let line_num = section.start_line as u32 + idx as u32 + 1;
                let character = line
                    .find(&extract_var.name)
                    .map(|pos| pos as u32)
                    .unwrap_or(0);

                return Some(VariableLocation {
                    name: var_name.to_string(),
                    line: line_num,
                    character,
                    uri: uri.to_string(),
                });
            }
        }
    }

    None
}

/// Convert VariableLocation to LSP Location
pub fn variable_location_to_lsp(loc: &VariableLocation) -> Location {
    Location {
        uri: Url::parse(&loc.uri).unwrap_or_else(|_| Url::parse("file:///unknown").unwrap()),
        range: Range {
            start: Position {
                line: loc.line,
                character: loc.character,
            },
            end: Position {
                line: loc.line,
                character: loc.character + loc.name.len() as u32,
            },
        },
    }
}

/// Find all variable references in document
pub fn find_variable_references(content: &str, var_name: &str, uri: &str) -> Vec<Location> {
    let mut references = Vec::new();

    if let Ok(doc) = parser::parse_gctf_from_str(content, "lsp-document.gctf") {
        for section in &doc.sections {
            for (idx, line) in section.raw_content.lines().enumerate() {
                // Find all {{ var_name }} occurrences
                let mut search_start = 0;
                while let Some(var_start) = line[search_start..].find("{{") {
                    let abs_start = search_start + var_start;
                    if let Some(var_end) = line[abs_start..].find("}}") {
                        let var_content = line[abs_start + 2..abs_start + var_end].trim();
                        if var_content == var_name {
                            references.push(Location {
                                uri: Url::parse(uri)
                                    .unwrap_or_else(|_| Url::parse("file:///unknown").unwrap()),
                                range: Range {
                                    start: Position {
                                        line: section.start_line as u32 + idx as u32 + 1,
                                        character: abs_start as u32,
                                    },
                                    end: Position {
                                        line: section.start_line as u32 + idx as u32 + 1,
                                        character: (abs_start + var_end + 2) as u32,
                                    },
                                },
                            });
                        }
                        search_start = abs_start + var_end + 2;
                    } else {
                        break;
                    }
                }
            }
        }
        return references;
    }

    // Fallback for parse failures: keep previous behavior on raw lines.
    for (line_idx, line) in content.lines().enumerate() {
        let mut search_start = 0;
        while let Some(var_start) = line[search_start..].find("{{") {
            let abs_start = search_start + var_start;
            if let Some(var_end) = line[abs_start..].find("}}") {
                let var_content = line[abs_start + 2..abs_start + var_end].trim();
                if var_content == var_name {
                    references.push(Location {
                        uri: Url::parse(uri)
                            .unwrap_or_else(|_| Url::parse("file:///unknown").unwrap()),
                        range: Range {
                            start: Position {
                                line: line_idx as u32,
                                character: abs_start as u32,
                            },
                            end: Position {
                                line: line_idx as u32,
                                character: (abs_start + var_end + 2) as u32,
                            },
                        },
                    });
                }
                search_start = abs_start + var_end + 2;
            } else {
                break;
            }
        }
    }

    references
}

/// Get all variables defined in document
pub fn get_all_variables(content: &str) -> Vec<(String, u32)> {
    let mut variables = Vec::new();

    if let Ok(doc) = parser::parse_gctf_from_str(content, "lsp-document.gctf") {
        for section in &doc.sections {
            if section.section_type != parser::ast::SectionType::Extract {
                continue;
            }

            for (idx, line) in section.raw_content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                    continue;
                }

                if let Some(extract_var) = parser::ExtractVar::parse(trimmed) {
                    variables.push((extract_var.name, section.start_line as u32 + idx as u32 + 1));
                }
            }
        }
    }

    variables
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_variable_at_position() {
        let line = "Authorization: Bearer {{ token }}";
        let result = extract_variable_at_position(line, 25);
        assert_eq!(result, Some("token".to_string()));
    }

    #[test]
    fn test_extract_variable_at_position_not_on_variable() {
        let line = "Authorization: Bearer {{ token }}";
        let result = extract_variable_at_position(line, 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_find_variable_in_extract() {
        let content = r#"
--- ENDPOINT ---
test.Service/Method

--- RESPONSE ---
{"token": "abc"}

--- EXTRACT ---
auth_token = .token
user_id = .user_id

--- ASSERTS ---
@len({{ auth_token }}) > 0
"#;

        let loc = find_variable_in_extract(content, "auth_token", "file:///test.gctf");
        assert!(loc.is_some());
        assert_eq!(loc.unwrap().name, "auth_token");
    }

    #[test]
    fn test_find_variable_not_found() {
        let content = r#"
--- ENDPOINT ---
test.Service/Method

--- RESPONSE ---
{"token": "abc"}

--- EXTRACT ---
auth_token = .token

--- ASSERTS ---
@len({{ auth_token }}) > 0
"#;

        let loc = find_variable_in_extract(content, "nonexistent", "file:///test.gctf");
        assert!(loc.is_none());
    }

    #[test]
    fn test_get_all_variables() {
        let content = r#"
--- ENDPOINT ---
test.Service/Method

--- RESPONSE ---
{"token": "abc", "id": 123}

--- EXTRACT ---
auth_token = .token
user_id = .user_id
user_name = .name

--- ASSERTS ---
@len({{ auth_token }}) > 0
"#;

        let vars = get_all_variables(content);
        assert_eq!(vars.len(), 3);
        assert_eq!(vars[0].0, "auth_token");
        assert_eq!(vars[1].0, "user_id");
        assert_eq!(vars[2].0, "user_name");
    }

    #[test]
    fn test_find_variable_references() {
        let content = r#"
--- ENDPOINT ---
test.Service/Method

--- RESPONSE ---
{"token": "abc"}

--- EXTRACT ---
token = .token

--- ASSERTS ---
@len({{ token }}) > 0
{{ token }} != null
"#;

        let refs = find_variable_references(content, "token", "file:///test.gctf");
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn test_variable_location_to_lsp() {
        let var_loc = VariableLocation {
            name: "test_var".to_string(),
            line: 10,
            character: 5,
            uri: "file:///test.gctf".to_string(),
        };

        let lsp_loc = variable_location_to_lsp(&var_loc);
        assert_eq!(lsp_loc.range.start.line, 10);
        assert_eq!(lsp_loc.range.start.character, 5);
    }

    #[test]
    fn test_find_variable_definition_full() {
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

        let position = Position {
            line: 14,
            character: 15,
        };

        let result = find_variable_definition(content, position, "file:///test.gctf");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "auth_token");
    }
}
