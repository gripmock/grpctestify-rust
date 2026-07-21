// LSP go-to-definition for variables

use crate::lsp::position::byte_to_utf16_col;
use crate::parser;
use std::collections::HashMap;
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
    // LSP `character` is a UTF-16 code-unit offset; convert to a byte index so
    // slicing does not panic on non-ASCII lines.
    let char_idx = crate::lsp::position::utf16_col_to_byte(line, position.character as usize);

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
pub fn variable_location_to_lsp(loc: &VariableLocation) -> Option<Location> {
    let uri = Url::parse(&loc.uri).ok()?;
    Some(Location {
        uri,
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
    })
}

/// Find all variable references in document
pub fn find_variable_references(content: &str, var_name: &str, uri: &str) -> Vec<Location> {
    let mut references = Vec::new();
    let Some(parsed_uri) = Url::parse(uri).ok() else {
        return references;
    };

    if let Ok(doc) = parser::parse_gctf_from_str(content, "lsp-document.gctf") {
        for section in &doc.sections {
            for (idx, line) in section.raw_content.lines().enumerate() {
                let line_num = section.start_line as u32 + idx as u32 + 1;
                for (byte_start, byte_end) in find_reference_spans(line, var_name) {
                    references.push(Location {
                        uri: parsed_uri.clone(),
                        range: Range {
                            start: Position {
                                line: line_num,
                                character: byte_to_utf16_col(line, byte_start) as u32,
                            },
                            end: Position {
                                line: line_num,
                                character: byte_to_utf16_col(line, byte_end) as u32,
                            },
                        },
                    });
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
                        uri: parsed_uri.clone(),
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

/// Find byte spans of references to `var_name` on a single line.
///
/// Matches both the `{{ var_name }}` form (whitespace-flexible inside the
/// braces) used in REQUEST/headers/JSON and the `$var_name` form used in
/// ASSERTS. The `$` form matches whole identifiers only, so `$token` does not
/// match `$token_extra`.
fn find_reference_spans(line: &str, var_name: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();

    // {{ var_name }} occurrences.
    let mut search_start = 0;
    while let Some(rel) = line[search_start..].find("{{") {
        let open = search_start + rel;
        if let Some(rel_close) = line[open + 2..].find("}}") {
            let close = open + 2 + rel_close; // byte index of the closing "}}"
            if line[open + 2..close].trim() == var_name {
                spans.push((open, close + 2));
            }
            search_start = close + 2;
        } else {
            break;
        }
    }

    // $var_name occurrences (whole identifier only).
    let dollar = format!("${}", var_name);
    let mut search_start = 0;
    while let Some(rel) = line[search_start..].find(&dollar) {
        let start = search_start + rel;
        let end = start + dollar.len();
        let boundary_ok = line[end..]
            .chars()
            .next()
            .map(|c| !c.is_alphanumeric() && c != '_')
            .unwrap_or(true);
        if boundary_ok {
            spans.push((start, end));
        }
        search_start = end;
    }

    spans
}

/// Build text edits to rename `var_name` to `new_name` across a document.
///
/// The rename (a) preserves the sigil form at each reference site
/// (`{{ name }}` stays braces, `$name` stays `$`), (b) matches whole
/// identifiers only, and (c) also renames the EXTRACT definition site.
pub fn build_rename_edits(
    content: &str,
    var_name: &str,
    new_name: &str,
    uri: &str,
) -> Option<HashMap<Url, Vec<TextEdit>>> {
    let parsed_uri = Url::parse(uri).ok()?;
    let doc = parser::parse_gctf_from_str(content, "lsp-document.gctf").ok()?;

    let mut edits: Vec<TextEdit> = Vec::new();

    for section in &doc.sections {
        let is_extract = section.section_type == parser::ast::SectionType::Extract;
        for (idx, line) in section.raw_content.lines().enumerate() {
            let line_num = section.start_line as u32 + idx as u32 + 1;

            // Rename the EXTRACT definition site (`name = <expr>`).
            if is_extract {
                let trimmed = line.trim_start();
                if !trimmed.is_empty()
                    && !trimmed.starts_with('#')
                    && !trimmed.starts_with("//")
                    && let Some(ev) = parser::ExtractVar::parse(trimmed)
                    && ev.name == var_name
                {
                    let name_byte = line.len() - trimmed.len();
                    let start = byte_to_utf16_col(line, name_byte) as u32;
                    let end = byte_to_utf16_col(line, name_byte + var_name.len()) as u32;
                    edits.push(TextEdit::new(
                        Range::new(Position::new(line_num, start), Position::new(line_num, end)),
                        new_name.to_string(),
                    ));
                }
            }

            // Rename references, preserving their sigil form.
            for (byte_start, byte_end) in find_reference_spans(line, var_name) {
                let replacement = if line[byte_start..].starts_with('$') {
                    format!("${}", new_name)
                } else {
                    format!("{{{{ {} }}}}", new_name)
                };
                let start = byte_to_utf16_col(line, byte_start) as u32;
                let end = byte_to_utf16_col(line, byte_end) as u32;
                edits.push(TextEdit::new(
                    Range::new(Position::new(line_num, start), Position::new(line_num, end)),
                    replacement,
                ));
            }
        }
    }

    if edits.is_empty() {
        return None;
    }

    let mut changes = HashMap::new();
    changes.insert(parsed_uri, edits);
    Some(changes)
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
    fn test_extract_variable_at_position_non_ascii_utf16() {
        // The `references` handler receives an LSP UTF-16 column and must
        // convert it to a byte offset before slicing; a raw UTF-16 index on a
        // non-ASCII line mislocates (or panics mid-multibyte).
        let line = "Заголовок: Bearer {{ token }}";
        // UTF-16 column pointing inside "{{ token }}" (each Cyrillic char is one
        // UTF-16 unit but two UTF-8 bytes).
        let utf16_col = line.chars().take_while(|&c| c != 't').count();
        let byte_idx = crate::lsp::position::utf16_col_to_byte(line, utf16_col);
        assert_eq!(
            extract_variable_at_position(line, byte_idx),
            Some("token".to_string())
        );
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
@len($auth_token) > 0
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
@len($auth_token) > 0
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
@len($auth_token) > 0
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
@len($token) > 0
$token != null
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

        let lsp_loc = variable_location_to_lsp(&var_loc).expect("valid file URI");
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

    /// Apply a set of single-line text edits to `content` (ASCII, so UTF-16
    /// columns equal byte offsets) and return the rewritten text.
    fn apply_edits(content: &str, edits: &[TextEdit]) -> String {
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        // Group edits per line and apply right-to-left so earlier ranges stay valid.
        let mut by_line: std::collections::HashMap<u32, Vec<&TextEdit>> = HashMap::new();
        for e in edits {
            by_line.entry(e.range.start.line).or_default().push(e);
        }
        for (line, mut es) in by_line {
            es.sort_by_key(|e| std::cmp::Reverse(e.range.start.character));
            let l = &mut lines[line as usize];
            for e in es {
                let start = e.range.start.character as usize;
                let end = e.range.end.character as usize;
                l.replace_range(start..end, &e.new_text);
            }
        }
        lines.join("\n")
    }

    #[test]
    fn test_build_rename_edits_whole_word_and_definition() {
        let content = r#"--- ENDPOINT ---
svc.M

--- REQUEST ---
{"a": "{{ token }}", "b": "{{ token_extra }}"}

--- RESPONSE ---
{}

--- EXTRACT ---
token = .a
token_extra = .b

--- ASSERTS ---
$token != null
$token_extra != null
@len($token) > 0
"#;

        let changes =
            build_rename_edits(content, "token", "renamed", "file:///t.gctf").expect("edits");
        let uri = Url::parse("file:///t.gctf").unwrap();
        let edits = changes.get(&uri).expect("edits for uri");

        // Every edit must target a whole `token` reference/definition, never
        // the `token_extra` identifier.
        let lines: Vec<&str> = content.lines().collect();
        for e in edits {
            let line = lines[e.range.start.line as usize];
            let original = &line[e.range.start.character as usize..e.range.end.character as usize];
            assert!(
                matches!(original, "token" | "{{ token }}" | "$token"),
                "edit targeted unexpected span: {:?}",
                original
            );
        }

        let rewritten = apply_edits(content, edits);
        // Definition renamed.
        assert!(rewritten.contains("renamed = .a"));
        // `{{ token }}` reference renamed, preserving braces sigil.
        assert!(rewritten.contains("{{ renamed }}"));
        // `$token` references renamed, preserving `$` sigil.
        assert!(rewritten.contains("$renamed != null"));
        assert!(rewritten.contains("@len($renamed) > 0"));
        // `token_extra` must be completely untouched (no prefix corruption).
        assert!(rewritten.contains("token_extra = .b"));
        assert!(rewritten.contains("{{ token_extra }}"));
        assert!(rewritten.contains("$token_extra != null"));
        assert!(!rewritten.contains("renamed_extra"));
        assert!(!rewritten.contains("$renamed_extra"));
    }

    #[test]
    fn test_find_variable_references_whole_word_dollar() {
        // `$token` must not match `$token_extra`.
        let content = r#"--- ENDPOINT ---
svc.M

--- EXTRACT ---
token = .a

--- ASSERTS ---
$token != null
$token_extra != null
"#;
        let refs = find_variable_references(content, "token", "file:///t.gctf");
        assert_eq!(refs.len(), 1, "only the whole-word $token should match");
    }
}
