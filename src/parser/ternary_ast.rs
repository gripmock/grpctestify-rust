// AST nodes for ternary expressions in EXTRACT section
// Uses ternary_to_jq from ternary.rs for conversion

use crate::parser::ternary::ternary_to_jq;
use serde::{Deserialize, Serialize};

/// Extract value type
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExtractValue {
    /// Simple JQ path: .user.id
    Simple(String),
    /// JQ expression: .items | length
    JqExpr(String),
    /// Ternary expression (raw string): .status == 200 ? "OK" : "Error"
    Ternary(String),
}

impl ExtractValue {
    /// Parse extract value string into AST
    pub fn parse(value: &str) -> Self {
        if is_ternary(value) {
            ExtractValue::Ternary(value.to_string())
        } else if value.contains('|') {
            ExtractValue::JqExpr(value.to_string())
        } else {
            ExtractValue::Simple(value.to_string())
        }
    }

    /// Convert to JQ syntax
    pub fn to_jq(&self) -> String {
        match self {
            ExtractValue::Simple(path) => path.clone(),
            ExtractValue::JqExpr(expr) => expr.clone(),
            ExtractValue::Ternary(raw) => ternary_to_jq(raw),
        }
    }
}

/// Check if string contains top-level ternary operator
fn is_ternary(value: &str) -> bool {
    find_top_level_char(value, '?').is_some() && find_top_level_char(value, ':').is_some()
}

/// Find character that's not inside quotes, parentheses, or brackets
fn find_top_level_char(expr: &str, target: char) -> Option<usize> {
    let mut in_quotes = false;
    let mut quote_char = None;
    let mut paren_depth = 0;
    let mut bracket_depth = 0;

    for (i, c) in expr.char_indices() {
        match c {
            '\'' | '"' => {
                if !in_quotes {
                    in_quotes = true;
                    quote_char = Some(c);
                } else if Some(c) == quote_char {
                    in_quotes = false;
                    quote_char = None;
                }
            }
            '(' | '{' => paren_depth += 1,
            ')' | '}' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            _ if c == target && !in_quotes && paren_depth == 0 && bracket_depth == 0 => {
                return Some(i);
            }
            _ => {}
        }
    }

    None
}

/// Extract variable definition
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractVar {
    pub name: String,
    pub value: ExtractValue,
}

impl ExtractVar {
    /// Parse "name = value" into ExtractVar
    pub fn parse(line: &str) -> Option<Self> {
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            return None;
        }

        let eq_pos = find_top_level_char(line, '=')?;

        Some(Self {
            name: line[..eq_pos].trim().to_string(),
            value: ExtractValue::parse(line[eq_pos + 1..].trim()),
        })
    }

    /// Convert to JQ syntax
    pub fn to_jq(&self) -> String {
        format!("{} = {}", self.name, self.value.to_jq())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_value_simple() {
        let value = ExtractValue::parse(".user.id");
        assert!(matches!(value, ExtractValue::Simple(_)));
        assert_eq!(value.to_jq(), ".user.id");
    }

    #[test]
    fn test_extract_value_jq() {
        let value = ExtractValue::parse(".items | length");
        assert!(matches!(value, ExtractValue::JqExpr(_)));
        assert_eq!(value.to_jq(), ".items | length");
    }

    #[test]
    fn test_extract_value_ternary() {
        let value = ExtractValue::parse(".status == 200 ? \"OK\" : \"Error\"");
        assert!(matches!(value, ExtractValue::Ternary(_)));
        assert_eq!(
            value.to_jq(),
            "if .status == 200 then \"OK\" else \"Error\" end"
        );
    }

    #[test]
    fn test_extract_value_ternary_with_jq() {
        let value = ExtractValue::parse("(.items | length) > 0 ? \"yes\" : \"no\"");
        assert!(matches!(value, ExtractValue::Ternary(_)));
        assert!(value.to_jq().starts_with("if"));
    }

    #[test]
    fn test_extract_var_parse() {
        let var = ExtractVar::parse("status = .status == 200 ? \"OK\" : \"Error\"").unwrap();
        assert_eq!(var.name, "status");
        assert!(matches!(var.value, ExtractValue::Ternary(_)));
        assert_eq!(
            var.to_jq(),
            "status = if .status == 200 then \"OK\" else \"Error\" end"
        );
    }

    #[test]
    fn test_extract_var_simple() {
        let var = ExtractVar::parse("token = .access_token").unwrap();
        assert_eq!(var.name, "token");
        assert!(matches!(var.value, ExtractValue::Simple(_)));
    }

    #[test]
    fn test_extract_var_jq() {
        let var = ExtractVar::parse("count = .items | length").unwrap();
        assert_eq!(var.name, "count");
        assert!(matches!(var.value, ExtractValue::JqExpr(_)));
    }

    #[test]
    fn test_extract_var_skip_comment() {
        let var = ExtractVar::parse("# this is a comment");
        assert!(var.is_none());
    }

    #[test]
    fn test_extract_var_skip_empty() {
        let var = ExtractVar::parse("");
        assert!(var.is_none());
    }

    #[test]
    fn test_find_top_level_char() {
        assert_eq!(find_top_level_char("a ? b : c", '?'), Some(2));
        assert_eq!(find_top_level_char("a ? b : c", ':'), Some(6));
    }

    #[test]
    fn test_find_top_level_in_quotes() {
        let result = find_top_level_char(".text == \"a ? b\" ? \"yes\" : \"no\"", '?');
        assert_eq!(result, Some(17));
    }

    #[test]
    fn test_find_top_level_in_parens() {
        assert_eq!(
            find_top_level_char("(.a > 0 ? \"yes\" : \"no\") : \"other\"", '?'),
            None
        );
    }

    #[test]
    fn test_extract_var_nested_ternary() {
        let var = ExtractVar::parse(
            "size = .count == 0 ? \"empty\" : (.count > 10 ? \"large\" : \"small\")",
        )
        .unwrap();
        assert_eq!(var.name, "size");
        assert!(matches!(var.value, ExtractValue::Ternary(_)));
        // Parentheses are preserved in output (valid jq syntax)
        assert_eq!(
            var.to_jq(),
            "size = if .count == 0 then \"empty\" else (if .count > 10 then \"large\" else \"small\" end) end"
        );
    }

    #[test]
    fn test_extract_var_with_header_plugin() {
        let var = ExtractVar::parse("request_id = @header(\"x-request-id\") != null ? @header(\"x-request-id\") : \"unknown\"").unwrap();
        assert_eq!(var.name, "request_id");
        assert!(matches!(var.value, ExtractValue::Ternary(_)));
    }
}
