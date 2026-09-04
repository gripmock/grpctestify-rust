use crate::ternary::ternary_to_jq;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExtractValue {
    Simple(String),
    JqExpr(String),
    Ternary(String),
}

impl ExtractValue {
    pub fn parse(value: &str) -> Self {
        if is_ternary(value) {
            ExtractValue::Ternary(value.to_string())
        } else if value.contains('|') {
            ExtractValue::JqExpr(value.to_string())
        } else {
            ExtractValue::Simple(value.to_string())
        }
    }

    pub fn to_jq(&self) -> String {
        match self {
            ExtractValue::Simple(path) => path.clone(),
            ExtractValue::JqExpr(expr) => expr.clone(),
            ExtractValue::Ternary(raw) => ternary_to_jq(raw),
        }
    }
}

fn is_ternary(value: &str) -> bool {
    find_top_level_char(value, '?').is_some() && find_top_level_char(value, ':').is_some()
}

fn find_top_level_char(expr: &str, target: char) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut paren_depth = 0;
    let mut bracket_depth = 0;

    for (i, c) in expr.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '(' | '{' => paren_depth += 1,
            ')' | '}' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            _ if c == target && paren_depth == 0 && bracket_depth == 0 => {
                return Some(i);
            }
            _ => {}
        }
    }

    None
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractVar {
    pub name: String,
    pub value: ExtractValue,
}

impl ExtractVar {
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

    pub fn parse_raw(name: &str, value: &str) -> Option<Self> {
        if name.is_empty() || value.is_empty() {
            return None;
        }
        Some(Self {
            name: name.to_string(),
            value: ExtractValue::parse(value),
        })
    }

    pub fn to_jq(&self) -> String {
        format!("{} = {}", self.name, self.value.to_jq())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_value_simple() {
        let value = ExtractValue::parse(".user.id");
        assert!(matches!(value, ExtractValue::Simple(_)));
        assert_eq!(value.to_jq(), ".user.id");
    }

    #[test]
    fn extract_value_jq() {
        let value = ExtractValue::parse(".items | length");
        assert!(matches!(value, ExtractValue::JqExpr(_)));
        assert_eq!(value.to_jq(), ".items | length");
    }

    #[test]
    fn extract_value_ternary() {
        let value = ExtractValue::parse(".status == 200 ? \"OK\" : \"Error\"");
        assert!(matches!(value, ExtractValue::Ternary(_)));
        assert_eq!(
            value.to_jq(),
            "if .status == 200 then \"OK\" else \"Error\" end"
        );
    }

    #[test]
    fn extract_value_ternary_with_jq() {
        let value = ExtractValue::parse("(.items | length) > 0 ? \"yes\" : \"no\"");
        assert!(matches!(value, ExtractValue::Ternary(_)));
        assert!(value.to_jq().starts_with("if"));
    }

    #[test]
    fn extract_var_parse() {
        let var = ExtractVar::parse("status = .status == 200 ? \"OK\" : \"Error\"").unwrap();
        assert_eq!(var.name, "status");
        assert!(matches!(var.value, ExtractValue::Ternary(_)));
        assert_eq!(
            var.to_jq(),
            "status = if .status == 200 then \"OK\" else \"Error\" end"
        );
    }

    #[test]
    fn extract_var_simple() {
        let var = ExtractVar::parse("token = .access_token").unwrap();
        assert_eq!(var.name, "token");
        assert!(matches!(var.value, ExtractValue::Simple(_)));
    }

    #[test]
    fn extract_var_jq() {
        let var = ExtractVar::parse("count = .items | length").unwrap();
        assert_eq!(var.name, "count");
        assert!(matches!(var.value, ExtractValue::JqExpr(_)));
    }

    #[test]
    fn extract_var_skip_comment() {
        let var = ExtractVar::parse("# this is a comment");
        assert!(var.is_none());
    }

    #[test]
    fn extract_var_skip_empty() {
        let var = ExtractVar::parse("");
        assert!(var.is_none());
    }

    #[test]
    fn test_find_top_level_char() {
        assert_eq!(find_top_level_char("a ? b : c", '?'), Some(2));
        assert_eq!(find_top_level_char("a ? b : c", ':'), Some(6));
    }

    #[test]
    fn find_top_level_in_quotes() {
        let result = find_top_level_char(".text == \"a ? b\" ? \"yes\" : \"no\"", '?');
        assert_eq!(result, Some(17));
    }

    #[test]
    fn find_top_level_ignores_brackets_inside_string_literals() {
        assert_eq!(
            find_top_level_char(r#".a == "(" ? "yes" : "no""#, '?'),
            Some(10)
        );
        assert_eq!(
            find_top_level_char(r#".a == "[" ? "yes" : "no""#, '?'),
            Some(10)
        );
    }

    #[test]
    fn find_top_level_respects_escaped_quotes() {
        assert_eq!(find_top_level_char(r#".a == "x \" ? y : z""#, '?'), None);
        assert_eq!(
            find_top_level_char(r#".a == "say \"hi\"" ? 1 : 2"#, '?'),
            Some(19)
        );
    }

    #[test]
    fn find_top_level_in_parens() {
        assert_eq!(
            find_top_level_char("(.a > 0 ? \"yes\" : \"no\") : \"other\"", '?'),
            None
        );
    }

    #[test]
    fn extract_var_nested_ternary() {
        let var = ExtractVar::parse(
            "size = .count == 0 ? \"empty\" : (.count > 10 ? \"large\" : \"small\")",
        )
        .unwrap();
        assert_eq!(var.name, "size");
        assert!(matches!(var.value, ExtractValue::Ternary(_)));
        assert_eq!(
            var.to_jq(),
            "size = if .count == 0 then \"empty\" else (if .count > 10 then \"large\" else \"small\" end) end"
        );
    }

    #[test]
    fn extract_var_with_header_plugin() {
        let var = ExtractVar::parse("request_id = @header(\"x-request-id\") != null ? @header(\"x-request-id\") : \"unknown\"").unwrap();
        assert_eq!(var.name, "request_id");
        assert!(matches!(var.value, ExtractValue::Ternary(_)));
    }
}
