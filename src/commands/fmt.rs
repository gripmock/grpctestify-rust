#![allow(clippy::unwrap_used, clippy::expect_used)] // audited safe

use anyhow::Result;
use tracing::{debug, error, warn};

use crate::cli::args::{Cli, FmtArgs};
use crate::optimizer;
use crate::optimizer::OptimizeLevel;
use crate::parser;
use crate::semantics;
use crate::utils::FileUtils;

/// Normalize assertion lines:
/// - convert `#` comments to `//`
/// - normalize `:TypeName` spacing to stuck-together form (`.x:number` not `.x : number`)
fn normalize_assertion_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(|line| normalize_numeric_literals(&normalize_assertion_line(line), true))
        .collect()
}

fn normalize_assertion_line(line: &str) -> String {
    let line = normalize_hash_comment_line(line).unwrap_or_else(|| line.to_string());
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.is_empty() {
        return line;
    }
    // The tokenizer drops escape backslashes, so canonicalising an assertion
    // that carries one would rewrite the literal into a different string.
    // Try to parse as assertion expression for canonical formatting
    let expr = crate::parser::assertion_ast::parse_assertion(trimmed);
    if matches!(&expr, crate::parser::assertion_ast::AssertionExpr::Raw(_)) {
        return line;
    }
    let serialized = crate::parser::assertion_ast::assertion_to_string(&expr);
    // Round-trip guard: only rewrite to the canonical form when that form
    // parses back to itself. Otherwise the parser can't re-read what we
    // emit and a second format pass would change the output. This is the
    // case for `if..then..else..end`, which serializes to a `? :` ternary
    // the parser does not accept back (root cause: apif-ast
    // assertion_to_string/parse_assertion asymmetry).
    let reparsed = crate::parser::assertion_ast::parse_assertion(&serialized);
    let stable = !matches!(
        &reparsed,
        crate::parser::assertion_ast::AssertionExpr::Raw(_)
    ) && crate::parser::assertion_ast::assertion_to_string(&reparsed) == serialized;
    if !stable {
        return line;
    }
    // Preserve original indentation
    let indent_len = line.len() - trimmed.len();
    let indent = &line[..indent_len];
    format!("{}{}", indent, serialized)
}

fn normalize_hash_comment_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }

    let indent_len = line.len() - trimmed.len();
    let indent = &line[..indent_len];
    let rest = trimmed.trim_start_matches('#').trim_start();

    if rest.is_empty() {
        Some(format!("{}//", indent))
    } else {
        Some(format!("{}// {}", indent, rest))
    }
}

/// `1000000` -> `1_000_000`, `1_00` -> `100`, `1.000000` -> `1.0`. Only a run
/// that is a number on both ends, so identifiers, UUIDs, hex, exponents,
/// string contents and comment prose are left as authored.
/// Canonicalise numeric literals. `group_integers` is false inside JSON
/// payloads: `1_000_000` is neither valid JSON nor JSON5, and a body is read by
/// other tools too. Fraction trimming (`1.000000` -> `1.0`) applies either way —
/// it keeps the value and the type.
fn normalize_numeric_literals(text: &str, group_integers: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '"' || c == '\'' {
            let quote = c;
            out.push(c);
            while let Some(next) = chars.next() {
                out.push(next);
                if next == '\\' {
                    if let Some(escaped) = chars.next() {
                        out.push(escaped);
                    }
                } else if next == quote {
                    break;
                }
            }
            continue;
        }

        // `// 1000` must stay `// 1000`.
        if c == '#' || (c == '/' && chars.peek() == Some(&'/')) {
            out.push(c);
            for next in chars.by_ref() {
                out.push(next);
                if next == '\n' {
                    break;
                }
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'*') {
            out.push(c);
            let mut prev = '\0';
            for next in chars.by_ref() {
                out.push(next);
                if prev == '*' && next == '/' {
                    break;
                }
                prev = next;
            }
            continue;
        }

        let is_number_start = c.is_ascii_digit()
            && out
                .chars()
                .next_back()
                .is_none_or(|p| !(p.is_alphanumeric() || p == '_' || p == '.'));

        if is_number_start {
            let mut int_part = String::new();
            int_part.push(c);
            while let Some(&next) = chars.peek() {
                if next.is_ascii_digit() || next == '_' {
                    int_part.push(next);
                    chars.next();
                } else {
                    break;
                }
            }

            let mut frac_part: Option<String> = None;
            if chars.peek() == Some(&'.') {
                let mut lookahead = chars.clone();
                lookahead.next();
                if lookahead.peek().is_some_and(|c| c.is_ascii_digit()) {
                    chars.next();
                    let mut frac = String::new();
                    while let Some(&next) = chars.peek() {
                        if next.is_ascii_digit() || next == '_' {
                            frac.push(next);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    frac_part = Some(frac);
                }
            }

            // Glued to a letter, `_` or `-` means it's part of a larger token:
            // a UUID, hex, an exponent tail. Not ours to regroup.
            let is_number_end = chars
                .peek()
                .is_none_or(|n| !(n.is_alphanumeric() || *n == '_' || *n == '-'));
            if !is_number_end {
                out.push_str(&int_part);
                if let Some(frac) = frac_part {
                    out.push('.');
                    out.push_str(&frac);
                }
                continue;
            }

            if group_integers {
                out.push_str(&group_digits(&int_part));
            } else {
                // Strip separators rather than pass them through: a payload
                // written by an older release carries `1_000_000`, which no
                // JSON parser outside this tool accepts.
                out.extend(int_part.chars().filter(|c| *c != '_'));
            }
            if let Some(frac) = frac_part {
                out.push('.');
                out.push_str(&trim_fraction(&frac));
            }
            continue;
        }

        out.push(c);
    }

    out
}

/// `1.000000` -> `1.0`, never `1` — losing the `.0` retypes a float as an int.
fn trim_fraction(raw: &str) -> String {
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    let trimmed = digits.trim_end_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn group_digits(raw: &str) -> String {
    let digits: Vec<char> = raw.chars().filter(char::is_ascii_digit).collect();
    let n = digits.len();
    if n <= 3 {
        return digits.into_iter().collect();
    }
    let mut grouped = String::with_capacity(n + n / 3);
    for (i, ch) in digits.iter().enumerate() {
        if i != 0 && (n - i).is_multiple_of(3) {
            grouped.push('_');
        }
        grouped.push(*ch);
    }
    grouped
}

fn format_json_with_comments(raw: &str) -> Vec<String> {
    fn push_indent(buf: &mut String, indent: usize) {
        for _ in 0..indent {
            buf.push_str("  ");
        }
    }

    // Move to a fresh, correctly-indented line. Any pending indentation on an
    // otherwise-empty line is stripped first, so we never emit a blank line
    // (which would also make a second format pass non-idempotent).
    fn newline_indent(buf: &mut String, indent: usize) {
        while buf.ends_with(' ') {
            buf.pop();
        }
        if !buf.ends_with('\n') {
            buf.push('\n');
        }
        push_indent(buf, indent);
    }

    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    let mut indent = 0usize;
    // JSON5, so `'…'` is a string too — otherwise a `#`, `//` or `:` inside one
    // reads as syntax and the value gets rewritten.
    let mut string_quote: Option<char> = None;
    let mut escaped = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut saw_newline_gap = false;
    // Break owed after `{`/`[`/`,`, emitted by the next token. Emitting it
    // eagerly moved `"a": 1, // note` onto the next line and split `{}` in two.
    let mut pending_break = false;
    // A `/* … */` ended mid-line; without a space the next value fuses onto it.
    let mut need_space = false;
    // Offset of a `,` with no value after it yet. JSON5 allows it before the
    // bracket, canonical JSON doesn't, and a comment may sit in between.
    let mut dangling_comma: Option<usize> = None;
    // A commented-out member sitting after that comma. The comma is then load
    // bearing: it is what lets the line be uncommented without also having to
    // put a comma back, so it must survive.
    let mut comma_holds_a_commented_out_member = false;

    macro_rules! open_value {
        () => {
            if saw_newline_gap || pending_break {
                newline_indent(&mut out, indent);
            } else if need_space && !out.ends_with(' ') && !out.ends_with('\n') {
                out.push(' ');
            }
            saw_newline_gap = false;
            need_space = false;
            dangling_comma = None;
            comma_holds_a_commented_out_member = false;
        };
    }

    while let Some(ch) = chars.next() {
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
                // The next token starts a fresh line — including a following
                // comment, which must not fold up onto this one.
                saw_newline_gap = true;
                pending_break = false;
            } else if ch != '\r' {
                out.push(ch);
            }
            continue;
        }

        if in_block_comment {
            out.push(ch);
            if ch == '*' && chars.next_if_eq(&'/').is_some() {
                out.push('/');
                in_block_comment = false;
                need_space = true;
            }
            continue;
        }

        if let Some(quote) = string_quote {
            saw_newline_gap = false;
            if escaped {
                escaped = false;
                // `\'` means nothing once the quotes are doubled.
                if !(quote == '\'' && ch == '\'') {
                    out.push('\\');
                }
                out.push(ch);
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == quote {
                string_quote = None;
                out.push('"');
                continue;
            }
            if ch == '"' {
                out.push('\\');
            }
            out.push(ch);
            continue;
        }

        // Quoting style carries no meaning; the contents do, and are untouched.
        if ch == '"' || ch == '\'' {
            open_value!();
            pending_break = false;
            string_quote = Some(ch);
            out.push('"');
            continue;
        }

        let slash_comment_kind = if ch == '/' {
            chars.next_if_map(|next| match next {
                '/' | '*' => Ok(next),
                _ => Err(next),
            })
        } else {
            None
        };

        if ch == '#' || slash_comment_kind.is_some() {
            // Only a newline in the *source* moves a comment to its own line;
            // a break merely owed by `{`/`[`/`,` must not.
            if saw_newline_gap {
                // A comment on its own line after a comma is a commented-out
                // member, not a note about the value before it.
                if dangling_comma.is_some() {
                    comma_holds_a_commented_out_member = true;
                }
                newline_indent(&mut out, indent);
                pending_break = false;
            } else if !out.is_empty() && !out.ends_with('\n') && !out.ends_with(' ') {
                out.push(' ');
            }

            need_space = false;
            let is_block = slash_comment_kind == Some('*');
            if is_block {
                out.push('/');
                out.push('*');
                in_block_comment = true;
            } else {
                out.push('/');
                out.push('/');
                in_line_comment = true;
                pending_break = false;
            }
            saw_newline_gap = false;
            continue;
        }

        match ch {
            '{' | '[' => {
                open_value!();
                out.push(ch);
                indent += 1;
                pending_break = true;
            }
            '}' | ']' => {
                // By offset, not `ends_with`: a comment may sit in between.
                if let Some(pos) = dangling_comma.take()
                    && !comma_holds_a_commented_out_member
                {
                    out.remove(pos);
                }
                comma_holds_a_commented_out_member = false;
                let empty = pending_break
                    && ((ch == '}' && out.ends_with('{')) || (ch == ']' && out.ends_with('[')));
                indent = indent.saturating_sub(1);
                pending_break = false;
                saw_newline_gap = false;
                need_space = false;
                if empty {
                    out.push(ch);
                    continue;
                }
                while out.ends_with(' ') {
                    out.pop();
                }
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                push_indent(&mut out, indent);
                out.push(ch);
            }
            ',' => {
                dangling_comma = Some(out.len());
                comma_holds_a_commented_out_member = false;
                out.push(',');
                saw_newline_gap = false;
                need_space = false;
                pending_break = true;
            }
            ':' => {
                out.push(':');
                out.push(' ');
                saw_newline_gap = false;
                need_space = false;
            }
            c if c.is_whitespace() => {
                if c == '\n' {
                    saw_newline_gap = true;
                }
            }
            _ => {
                open_value!();
                pending_break = false;

                // JSON5 identifiers are Unicode, not ASCII: `имя: 4` is as
                // valid an unquoted key as `name: 4` and must be quoted too.
                if ch.is_alphabetic() || ch == '_' || ch == '$' {
                    let mut word = String::from(ch);
                    while let Some(&next) = chars.peek() {
                        if next.is_alphanumeric() || next == '_' || next == '$' {
                            word.push(next);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    // In key position it's JSON5's unquoted key. Anywhere else
                    // it's a literal (`true`, `NaN`, an exponent tail).
                    let mut lookahead = chars.clone();
                    while lookahead.peek().is_some_and(|c| c.is_whitespace()) {
                        lookahead.next();
                    }
                    if lookahead.peek() == Some(&':') {
                        out.push('"');
                        out.push_str(&word);
                        out.push('"');
                    } else {
                        out.push_str(&word);
                    }
                    continue;
                }

                out.push(ch);
            }
        }
    }

    normalize_numeric_literals(&out, false)
        .lines()
        .map(str::to_string)
        .collect()
}

fn ensure_single_section_separator(output: &mut Vec<String>, has_next_section: bool) {
    if !has_next_section {
        return;
    }

    while output.last().is_some_and(|line| line.trim().is_empty()) {
        output.pop();
    }

    output.push(String::new());
}

fn canonical_line_ending() -> &'static str {
    "\n"
}

fn normalize_eol_for_compare(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains("\r\n") {
        std::borrow::Cow::Owned(s.replace("\r\n", "\n"))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

fn normalize_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(|line| normalize_hash_comment_line(line).unwrap_or_else(|| line.to_string()))
        .collect()
}

fn format_non_json_section_lines(raw: &str) -> Vec<String> {
    normalize_lines(raw)
}

/// Special formatter for EXTRACT sections.
/// Preserves `name:Type = .jq.path` syntax without breaking the `:Type` annotation.
fn format_extract_section(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in normalize_lines(raw) {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.is_empty() {
            out.push(line);
            continue;
        }

        // Try to parse as extract line with full type info
        if let Some((name, type_opt, value)) =
            crate::parser::gctf_tokenizer::tokenize_extract_line_full(trimmed)
        {
            let formatted = if let Some(tn) = type_opt {
                format!("{}:{} = {}", name, tn, value)
            } else {
                format!("{} = {}", name, value)
            };
            // Preserve original indentation
            let indent_len = line.len() - trimmed.len();
            out.push(format!("{}{}", &line[..indent_len], formatted));
        } else {
            out.push(line);
        }
    }
    out
}

/// A key line plus whatever was written above it, so sorting moves a comment
/// with the key it documents instead of stranding or deleting it.
struct KeyBlock {
    sort_key: String,
    lines: Vec<String>,
}

/// Comments and unparsable lines ride onto the next key; the remainder comes
/// back as the section's tail. Nothing is dropped.
fn collect_key_blocks(
    lines: &[String],
    mut render: impl FnMut(&str, &str) -> (String, String),
) -> (Vec<KeyBlock>, Vec<String>) {
    let mut blocks: Vec<KeyBlock> = Vec::new();
    let mut pending: Vec<String> = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match trimmed.split_once(':') {
            Some((key, value)) if !trimmed.starts_with("//") => {
                let (sort_key, text) = render(key.trim(), value.trim());
                let mut block = std::mem::take(&mut pending);
                block.push(text);
                blocks.push(KeyBlock {
                    sort_key,
                    lines: block,
                });
            }
            _ => pending.push(trimmed.to_string()),
        }
    }

    (blocks, pending)
}

fn flatten_key_blocks(blocks: Vec<KeyBlock>, tail: Vec<String>) -> Vec<String> {
    blocks
        .into_iter()
        .flat_map(|b| b.lines)
        .chain(tail)
        .collect()
}

fn format_key_values_section(raw: &str, sort_keys: bool) -> Vec<String> {
    let lines = normalize_lines(raw);
    let (mut blocks, tail) = collect_key_blocks(&lines, |key, value| {
        let sort_key = if sort_keys {
            key.to_lowercase()
        } else {
            String::new()
        };
        (sort_key, format!("{}: {}", key, value))
    });

    if sort_keys {
        blocks.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    }

    flatten_key_blocks(blocks, tail)
}

/// Like `format_key_values_section`, but indented lines stay glued to the
/// preceding key instead of being sorted as their own entries — needed for
/// `sources:`'s nested YAML list.
fn format_bench_section(raw: &str) -> Vec<String> {
    let lines = normalize_lines(raw);

    struct Item {
        rank: usize,
        key: String,
        text: Vec<String>,
    }
    let mut items: Vec<Item> = Vec::new();
    let mut pending: Vec<String> = Vec::new();

    for line in &lines {
        let is_continuation = line.starts_with(' ') || line.starts_with('\t');
        if is_continuation && let Some(item) = items.last_mut() {
            item.text.push(line.clone());
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match trimmed.split_once(':') {
            Some((key, value)) if !trimmed.starts_with("//") => {
                let key = key.trim();
                let value = value.trim();
                let text = if value.is_empty() {
                    format!("{key}:")
                } else {
                    format!("{key}: {value}")
                };
                let mut block = std::mem::take(&mut pending);
                block.push(text);
                items.push(Item {
                    rank: apif_source_row::schema::bench_key_rank(key),
                    key: key.to_lowercase(),
                    text: block,
                });
            }
            _ => pending.push(trimmed.to_string()),
        }
    }

    // Canonical order (mode → profile → ... → sources), matching `check`'s
    // reference order and the other BENCH serializer in apif-parser — not
    // alphabetical, so e.g. `mode` sorts before `duration`.
    items.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.key.cmp(&b.key)));
    items
        .into_iter()
        .flat_map(|item| item.text)
        .chain(pending)
        .collect()
}

fn format_options_section(raw: &str) -> Vec<String> {
    let lines = normalize_lines(raw);
    let (mut blocks, tail) = collect_key_blocks(&lines, |key, value| {
        let normalized_key = crate::parser::canonical_key_spelling(key);
        (
            normalized_key.to_ascii_lowercase(),
            format!("{}: {}", normalized_key, value),
        )
    });

    blocks.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    flatten_key_blocks(blocks, tail)
}

fn trim_trailing_blank_lines(lines: &mut Vec<String>) {
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
}

fn format_section_lines(section: &crate::parser::ast::Section) -> Vec<String> {
    let mut lines = match (&section.section_type, &section.content) {
        // Re-indented from source text, never re-serialized: a `serde_json`
        // round-trip rewrote `0xFF` -> `255`, `1e3` -> `1000.0`,
        // `Infinity`/`NaN` -> `null`, and dropped a duplicate key.
        (
            crate::parser::ast::SectionType::Request
            | crate::parser::ast::SectionType::Error
            | crate::parser::ast::SectionType::Response,
            crate::parser::ast::SectionContent::Json(_)
            | crate::parser::ast::SectionContent::JsonLines(_),
        ) => format_json_with_comments(&section.raw_content),
        // ASSERTS section — normalize type annotation spacing and comments
        (crate::parser::ast::SectionType::Asserts, _) => {
            return normalize_assertion_lines(&section.raw_content);
        }
        // META/DATASET are YAML — `#` is its comment marker, so preserve
        // lines verbatim (rewriting `#` to `//` would corrupt the YAML).
        (crate::parser::ast::SectionType::Meta | crate::parser::ast::SectionType::Dataset, _) => {
            section.raw_content.lines().map(str::to_string).collect()
        }
        (
            crate::parser::ast::SectionType::Options,
            crate::parser::ast::SectionContent::KeyValues(_),
        ) => format_options_section(&section.raw_content),
        (
            crate::parser::ast::SectionType::Bench,
            crate::parser::ast::SectionContent::KeyValues(_),
        ) => format_bench_section(&section.raw_content),
        (_, crate::parser::ast::SectionContent::KeyValues(_)) => {
            format_key_values_section(&section.raw_content, true)
        }
        (_, crate::parser::ast::SectionContent::Extract(_)) => {
            format_extract_section(&section.raw_content)
        }
        _ => format_non_json_section_lines(&section.raw_content),
    };

    trim_trailing_blank_lines(&mut lines);
    lines
}

/// One section's rendered lines (leading comments + attributes + header +
/// body), kept together as a unit so reordering preamble sections below
/// carries each section's comments/attributes along with it.
struct SectionBlock {
    preamble_rank: Option<usize>,
    lines: Vec<String>,
}

/// Move a trailing run of blank/`//`-comment lines off the end of each block
/// onto the front of the next one — see the call site for why.
/// Only re-homes comments up to `last_movable`, i.e. within the preamble
/// (plus its boundary with the first body section) — reordering never
/// touches body sections, so their existing comment placement is untouched.
fn rehome_trailing_comments(blocks: &mut [SectionBlock], last_movable: usize) {
    for i in 0..last_movable.min(blocks.len().saturating_sub(1)) {
        let split_at = {
            let lines = &blocks[i].lines;
            let mut idx = lines.len();
            while idx > 0 {
                let trimmed = lines[idx - 1].trim();
                if trimmed.is_empty() || trimmed.starts_with("//") {
                    idx -= 1;
                } else {
                    break;
                }
            }
            idx
        };
        if split_at < blocks[i].lines.len() {
            let mut tail = blocks[i].lines.split_off(split_at);
            // Only the comment text itself needs to travel —
            // `ensure_single_section_separator` supplies the blank line
            // between blocks at assembly time.
            while tail.first().is_some_and(|l| l.trim().is_empty()) {
                tail.remove(0);
            }
            if tail.is_empty() {
                continue;
            }
            let next = &mut blocks[i + 1].lines;
            next.splice(0..0, tail);
        }
    }
}

/// Format a GCTF document chain via AST. Walks all sections in order.
fn format_gctf_chain(head: &crate::parser::GctfDocument, source: &str) -> String {
    let eol = canonical_line_ending();
    let lines: Vec<&str> = source.lines().collect();
    let mut blocks: Vec<SectionBlock> = Vec::new();
    let mut current_line = 0usize;

    // Walk every section across all documents in the chain, in source order.
    for doc in head.iter_chain() {
        for section in &doc.sections {
            let attr_count = section.attributes.len();
            let attr_line_start = section.start_line.saturating_sub(attr_count);

            let mut block_lines = Vec::new();

            // Interleave comments between previous section end and attribute
            // lines. Blank lines are deliberately dropped, not copied —
            // inter-block spacing is always synthesized at assembly time by
            // `ensure_single_section_separator`, and every section's own
            // rendered content is already blank-trimmed
            // (`format_section_lines`'s `trim_trailing_blank_lines`), so a
            // raw blank line here would just double up with that separator.
            while current_line < attr_line_start && current_line < lines.len() {
                if !lines[current_line].trim().is_empty() {
                    block_lines.push(
                        normalize_hash_comment_line(lines[current_line])
                            .unwrap_or_else(|| lines[current_line].to_string()),
                    );
                }
                current_line += 1;
            }

            // Emit attributes before section header
            for attr in &section.attributes {
                block_lines.push(attr.format_directive());
            }

            // Normal section
            block_lines.push(section.format_header());
            block_lines.extend(format_section_lines(section));

            current_line = section.end_line.min(lines.len());

            blocks.push(SectionBlock {
                preamble_rank: section.section_type.preamble_rank(),
                lines: block_lines,
            });
        }
    }

    let first_body_idx = blocks
        .iter()
        .position(|b| b.preamble_rank.is_none())
        .unwrap_or(blocks.len());

    // The parser attributes a comment sitting between two sections to the
    // PRECEDING section's content (`extract_section_content` in
    // apif-parser), not the section it visually precedes. Reordering
    // preamble sections would otherwise drag such a comment along with the
    // wrong neighbor, so move a trailing run of blank/comment lines off the
    // end of each preamble block onto the front of the next one first.
    // Bounded to the preamble (+ its boundary with the first body section) —
    // body-to-body comment placement is untouched.
    rehome_trailing_comments(&mut blocks, first_body_idx);

    // Reorder only the leading run of preamble sections (META→BENCH→DATASET→
    // ADDRESS→ENDPOINT→TLS→PROTO→OPTIONS) into canonical order; body sections
    // keep their original order and position. A stable sort keeps each
    // block's comments/attributes glued to its section.
    blocks[..first_body_idx].sort_by_key(|b| b.preamble_rank.unwrap());

    let mut output: Vec<String> = Vec::new();
    for block in &blocks {
        output.extend(block.lines.iter().cloned());
        ensure_single_section_separator(&mut output, true);
    }

    // Trailing file lines
    while current_line < lines.len() {
        output.push(
            normalize_hash_comment_line(lines[current_line])
                .unwrap_or_else(|| lines[current_line].to_string()),
        );
        current_line += 1;
    }

    let mut rendered = output.join(eol);
    if !rendered.ends_with(eol) {
        rendered.push_str(eol);
    }
    rendered
}

/// Format GCTF content with Safe optimizer level (default).
pub fn format_gctf_content(source: &str, file_name: &str) -> Result<String> {
    format_gctf_content_with_level(source, file_name, optimizer::OptimizeLevel::Safe)
}

/// Format GCTF content with explicit optimizer level.
pub fn format_gctf_content_with_level(
    source: &str,
    file_name: &str,
    level: OptimizeLevel,
) -> Result<String> {
    let doc = parser::parse_gctf_from_str(source, file_name)?;

    // Apply optimizer rewrites before formatting
    let eol = canonical_line_ending();
    let source_after_optimizer = apply_optimizer_rewrites(&doc, source, eol, level);

    // Re-parse after optimizer to get updated raw_content
    let doc_after = parser::parse_gctf_from_str(&source_after_optimizer, file_name)?;
    Ok(format_gctf_chain(&doc_after, &source_after_optimizer))
}

/// Apply optimizer rewrites to source lines
fn apply_optimizer_rewrites(
    doc: &crate::parser::GctfDocument,
    source: &str,
    eol: &str,
    level: OptimizeLevel,
) -> String {
    let hints = optimizer::collect_assertion_optimizations(doc, level);
    if hints.is_empty() {
        return source.to_string();
    }

    let mut lines: Vec<String> = source.lines().map(str::to_string).collect();
    for hint in hints {
        let line_idx = hint.line.saturating_sub(1);
        if line_idx >= lines.len() {
            continue;
        }
        let line = &mut lines[line_idx];
        if let Some(start) = line.find(&hint.before) {
            let end = start + hint.before.len();
            line.replace_range(start..end, &hint.after);
        }
    }

    let mut rewritten = lines.join(eol);
    if source.ends_with('\n') {
        rewritten.push_str(eol);
    }
    rewritten
}

pub async fn handle_fmt(args: &FmtArgs, cli: &Cli) -> Result<()> {
    let level = cli.optimize_level(optimizer::OptimizeLevel::Safe);
    let mut files = Vec::new();
    let mut has_error = false;
    let mut files_needing_format = 0usize;
    let mut files_written = 0usize;
    // See `check.rs::rhai_plugin_names` — `PLUGIN_SIGNATURES` has no way to
    // know about a script from the convention directories, so without this
    // every valid `.rhai`-backed assertion would hard-fail `fmt` as an
    // unknown plugin.
    let rhai_plugin_names = crate::commands::check::rhai_plugin_names();
    apif_optimizer::register_extra_boolean_plugins(
        crate::commands::check::rhai_boolean_plugin_names(),
    );
    crate::parser::register_extra_inline_option_keys(
        crate::plugins::rhai_plugin::load_all_inline_option_keys(),
    );

    for path in &args.files {
        if path.is_dir() {
            files.extend(FileUtils::collect_test_files(path, &[]));
        } else if path.is_file() {
            files.push(path.clone());
        } else {
            error!("Path not found: {}", path.display());
            has_error = true;
        }
    }

    if files.is_empty() {
        if !has_error {
            warn!("No .gctf files found to format");
        }
        return Ok(());
    }

    let total_files = files.len();
    for file in files {
        let original = match std::fs::read_to_string(&file) {
            Ok(content) => content,
            Err(e) => {
                error!("Failed to read {}: {}", file.display(), e);
                has_error = true;
                continue;
            }
        };

        let file_name = file.to_string_lossy();
        let doc = match parser::parse_gctf_from_str(&original, &file_name) {
            Ok(doc) => doc,
            Err(e) => {
                error!("{}:1: [PARSE_ERROR] {}", file.display(), e);
                has_error = true;
                continue;
            }
        };

        // Validate each document in the chain
        let mut chain_has_error = false;
        for d in doc.iter_chain() {
            if let Err(e) = parser::validate_document(d) {
                error!("{}:1: [VALIDATION_ERROR] {}", file.display(), e);
                chain_has_error = true;
            }
            // detached: the semantic passes below are chain-aware internally too
            let single = d.detached();
            for mismatch in semantics::collect_assertion_type_mismatches(&single) {
                error!(
                    "{}:{}: [{}] {}",
                    file.display(),
                    mismatch.line,
                    mismatch.rule_id,
                    mismatch.message
                );
                chain_has_error = true;
            }
            for unknown in
                semantics::collect_unknown_plugin_calls_with_extra(&single, &rhai_plugin_names)
            {
                error!(
                    "{}:{}: [{}] {}",
                    file.display(),
                    unknown.line,
                    unknown.rule_id,
                    unknown.message
                );
                chain_has_error = true;
            }
            // Suppress SEM_D001 in fmt — Safe-level optimizer auto-fixes them
            for dep in semantics::collect_deprecated_plugin_calls(&single) {
                debug!(
                    "{}:{}: [{}] {}",
                    file.display(),
                    dep.line,
                    dep.rule_id,
                    dep.message
                );
            }
            for constant in semantics::collect_constant_assertions(&single) {
                error!(
                    "{}:{}: [{}] {}",
                    file.display(),
                    constant.line,
                    constant.rule_id,
                    constant.message
                );
                chain_has_error = true;
            }
            for dup in semantics::collect_duplicate_assertions(&single) {
                error!(
                    "{}:{}: [{}] {}",
                    file.display(),
                    dup.line,
                    dup.rule_id,
                    dup.message
                );
                chain_has_error = true;
            }
            for redundant in semantics::collect_redundant_response_assertions(&single) {
                error!(
                    "{}:{}: [{}] {}",
                    file.display(),
                    redundant.line,
                    redundant.rule_id,
                    redundant.message
                );
                chain_has_error = true;
            }
        }
        if chain_has_error {
            has_error = true;
            continue;
        }

        // Use the format function which handles multi-document automatically
        let formatted = match format_gctf_content_with_level(&original, &file_name, level) {
            Ok(f) => f,
            Err(e) => {
                error!("{}:1: [FORMAT_ERROR] {}", file.display(), e);
                has_error = true;
                continue;
            }
        };

        let formatted_cmp = normalize_eol_for_compare(&formatted);
        let original_cmp = normalize_eol_for_compare(&original);
        let changed = formatted_cmp != original_cmp;

        if args.write {
            if changed {
                if let Err(e) = crate::utils::file::write_atomic(&file, &formatted) {
                    error!("Failed to write {}: {}", file.display(), e);
                    has_error = true;
                } else {
                    println!(
                        "{} {} {}",
                        crate::report::style::pass_icon(),
                        file.display(),
                        crate::report::style::dim_style().apply_to("· formatted")
                    );
                    files_written += 1;
                }
            }
        } else if changed {
            println!(
                "{} {}:1: [FORMAT_NEEDED] File is not formatted",
                crate::report::style::fail_icon(),
                file.display()
            );
            println!("    hint: Run `grpctestify fmt -w {}`", file.display());
            has_error = true;
            files_needing_format += 1;
        } else {
            println!(
                "{} {} ... OK",
                crate::report::style::pass_icon(),
                file.display()
            );
        }
    }

    print_fmt_summary(args.write, total_files, files_written, files_needing_format);

    if has_error {
        return Err(anyhow::anyhow!("Formatting failed with errors"));
    }

    Ok(())
}

fn print_fmt_summary(write: bool, total: usize, written: usize, needing: usize) {
    use crate::report::style::{dim_style, fail_icon, fail_style, pass_icon, pass_style};
    println!();
    if write {
        let unchanged = total.saturating_sub(written);
        println!(
            "{} {} {}",
            pass_icon(),
            pass_style().apply_to("Formatted"),
            dim_style().apply_to(format!(
                "· {written} changed · {unchanged} already formatted"
            ))
        );
    } else if needing == 0 {
        println!(
            "{} {} {}",
            pass_icon(),
            pass_style().apply_to("All formatted"),
            dim_style().apply_to(format!("· {total} files"))
        );
    } else {
        println!(
            "{} {} {}",
            fail_icon(),
            fail_style().apply_to("Needs formatting"),
            dim_style().apply_to(format!(
                "· {needing}/{total} files · run `grpctestify fmt -w`"
            ))
        );
    }
}

#[cfg(test)]
mod tests {
    use super::format_gctf_content;
    use crate::utils::file::write_atomic;

    fn to_crlf(input: &str) -> String {
        input.replace('\n', "\r\n")
    }

    const HDR: &str = "--- ENDPOINT ---\ntest.Service/Method\n\n--- REQUEST ---\n{}\n\n";

    /// Assert `fmt(fmt(x)) == fmt(x)` — the core formatter idempotency property.
    fn assert_idempotent(src: &str) -> String {
        let once = format_gctf_content(src, "t.gctf").unwrap();
        let twice = format_gctf_content(&once, "t.gctf").unwrap();
        assert_eq!(
            once, twice,
            "not idempotent\n--- once ---\n{}\n--- twice ---\n{}",
            once, twice
        );
        once
    }

    // JSON5 means the whole JSON5 grammar, not its ASCII subset: a Unicode
    // unquoted key is as valid as an ASCII one, and the numeric forms JSON5
    // adds are the author's notation, not ours to re-mint.
    #[test]
    fn fmt_json5_grammar_round_trip() {
        let body = concat!(
            "{\n",
            "  \u{438}\u{43c}\u{44f}: 1,\n",
            "  $d: 2,\n",
            "  plus: +1.5,\n",
            "  dot: .5,\n",
            "  trail: 5.,\n",
            "  hex: 0X1F,\n",
            "  inf: -Infinity,\n",
            "  esc: 'it\\'s \"q\"',\n",
            "  arr: [1, 2,],\n",
            "}\n"
        );
        let src = format!("{HDR}--- RESPONSE ---\n{body}");
        let out = assert_idempotent(&src);
        for expected in [
            "\"\u{438}\u{43c}\u{44f}\": 1",
            "\"$d\": 2",
            "\"plus\": +1.5",
            "\"dot\": .5",
            "\"trail\": 5.",
            "\"hex\": 0X1F",
            "\"inf\": -Infinity",
            "\"esc\": \"it's \\\"q\\\"\"",
        ] {
            assert!(out.contains(expected), "missing {expected}: {out}");
        }
        assert!(!out.contains(",\n  ]"), "trailing comma must go: {out}");
    }

    // Regression: an empty EXTRACT section was deleted outright as a "no-op".
    // It is not one — `inspect`, `explain` and `Workflow::extractions` all
    // report the section, so dropping it changed what those commands saw.
    #[test]
    fn fmt_keeps_an_empty_extract_section() {
        let src = format!("{HDR}--- EXTRACT ---\n\n--- ASSERTS ---\n.a == 1\n");
        let out = assert_idempotent(&src);
        assert!(
            out.contains("--- EXTRACT ---"),
            "section must survive: {out}"
        );
    }

    // Regression: a `//` comment on the same line as an opening brace used to
    // gain a spurious blank line on the SECOND format pass, then be relocated
    // onto its own line. It belongs where the author put it.
    #[cfg_attr(miri, ignore)]
    #[test]
    fn fmt_json_comment_after_brace_idempotent() {
        let src = format!("{}--- RESPONSE ---\n{{ // opener\n  \"a\": 1\n}}\n", HDR);
        let out = assert_idempotent(&src);
        assert!(
            out.contains("{ // opener\n  \"a\": 1\n}"),
            "comment must stay on the brace line: {out}"
        );
        assert!(!out.contains("{\n  \n"), "no spurious blank line: {out}");
    }

    // Regression: a comment trailing a comma was pushed onto the following
    // line, re-attaching it to the *next* array element.
    #[test]
    fn fmt_json_trailing_comment_stays_on_its_line() {
        let src = format!(
            "{HDR}--- RESPONSE ---\n{{\n  \"ids\": [\n    \"a\", // first\n    \"b\" // second\n  ]\n}}\n"
        );
        let out = assert_idempotent(&src);
        assert!(
            out.contains("\"a\", // first"),
            "trailing comment must not move to the next line: {out}"
        );
        assert!(out.contains("\"b\" // second"), "{out}");
    }

    // Regression: consecutive comment lines were folded onto one physical line.
    #[test]
    fn fmt_json_consecutive_comments_stay_separate() {
        let src = format!("{HDR}--- RESPONSE ---\n{{\n  // first\n  // second\n  \"a\": 1\n}}\n");
        let out = assert_idempotent(&src);
        assert!(
            out.contains("// first\n  // second\n"),
            "each comment keeps its own line: {out}"
        );
    }

    // Regression: an empty object/array was blown open across two lines.
    #[test]
    fn fmt_json_empty_containers_stay_compact() {
        let src = format!(
            "{HDR}--- RESPONSE ---\n{{\n  \"o\": {{}},\n  \"a\": [], // none\n  \"b\": 1\n}}\n"
        );
        let out = assert_idempotent(&src);
        assert!(
            out.contains("\"o\": {},"),
            "empty object stays compact: {out}"
        );
        assert!(out.contains("\"a\": [], // none"), "{out}");
    }

    // Regression: digit grouping ran over comment prose, rewriting `// 1000`
    // to `// 1_000`.
    #[test]
    fn fmt_does_not_group_digits_inside_comments() {
        let src = format!("{HDR}--- RESPONSE ---\n{{\n  \"a\": 1000000 // limit is 1000000\n}}\n");
        let out = assert_idempotent(&src);
        assert!(out.contains("\"a\": 1000000 // limit is 1000000"), "{out}");
    }

    // Regression: a UUID's digit runs were regrouped (`00000000-0000-...` ->
    // `00_000_000-0000-...`). Only a run that is a number end to end is one.
    #[test]
    fn fmt_does_not_group_digits_inside_uuids_or_literals() {
        let src = format!(
            "{HDR}--- RESPONSE ---\n{{\n  // 00000000-0000-0000-0000-000000000000\n  \"id\": 1,\n  \"hex\": 0xFF,\n  \"exp\": 1e3\n}}\n"
        );
        let out = assert_idempotent(&src);
        assert!(
            out.contains("// 00000000-0000-0000-0000-000000000000"),
            "UUID digits untouched: {out}"
        );
        assert!(
            out.contains("\"hex\": 0xFF"),
            "hex literal untouched: {out}"
        );
        assert!(out.contains("\"exp\": 1e3"), "exponent untouched: {out}");
    }

    // A redundant fraction normalizes to `1.0`, never to `1` — dropping the
    // fraction would retype a float as an integer.
    #[test]
    fn fmt_trailing_zero_fraction_keeps_one_digit() {
        let src = format!(
            "{HDR}--- RESPONSE ---\n{{\n  \"a\": 1.000000,\n  \"b\": 2.500, // note\n  \"c\": 3.0\n}}\n"
        );
        let out = assert_idempotent(&src);
        assert!(out.contains("\"a\": 1.0,"), "{out}");
        assert!(out.contains("\"b\": 2.5, // note"), "{out}");
        assert!(out.contains("\"c\": 3.0"), "{out}");
    }

    // Regression: `'…'` wasn't recognized, so a `:` or `#` inside one read as
    // syntax and rewrote the value. Quotes canonicalize; contents don't.
    #[test]
    fn fmt_single_quoted_string_contents_survive() {
        let src = format!("{HDR}--- RESPONSE ---\n{{\n  'url': 'http://x/y#z' // note\n}}\n");
        let out = assert_idempotent(&src);
        assert!(
            out.contains("\"url\": \"http://x/y#z\" // note"),
            "contents must survive, only the quotes change: {out}"
        );
    }

    // JSON5 escapes re-target when the quotes are doubled: `\'` loses its
    // backslash, a bare `"` gains one.
    #[test]
    fn fmt_single_quoted_string_requoting_escapes() {
        let src =
            format!("{HDR}--- RESPONSE ---\n{{\n  'a': 'it\\'s \"x\"', // note\n  'b': 1\n}}\n");
        let out = assert_idempotent(&src);
        assert!(out.contains(r#""a": "it's \"x\"", // note"#), "{out}");
    }

    // JSON5 trailing commas and unquoted keys canonicalize; the numbers next to
    // them do not.
    #[test]
    fn fmt_json5_canonicalizes_syntax_not_literals() {
        let src = format!(
            "{HDR}--- RESPONSE ---\n{{\n  name: 'World', // hi\n  meta: {{ id: 0xFF, }},\n  flag: true,\n}}\n"
        );
        let out = assert_idempotent(&src);
        assert!(out.contains("\"name\": \"World\", // hi"), "{out}");
        assert!(out.contains("\"id\": 0xFF"), "hex literal survives: {out}");
        assert!(out.contains("\"flag\": true"), "{out}");
        assert!(!out.contains(",\n}"), "no trailing comma left: {out}");
    }

    // Regression: `serde_json` round-tripping rewrote authored literals —
    // `0xFF` -> `255`, `1e3` -> `1000.0`, `Infinity`/`NaN` -> `null` — and
    // dropped the first of two duplicate keys, all silently.
    #[test]
    fn fmt_does_not_remint_json_literals() {
        let src = format!(
            "{HDR}--- RESPONSE ---\n{{\n  \"hex\": 0xFF,\n  \"exp\": 1e3,\n  \"inf\": Infinity,\n  \"nan\": NaN,\n  \"dup\": 1,\n  \"dup\": 2\n}}\n"
        );
        let out = assert_idempotent(&src);
        for expected in [
            "\"hex\": 0xFF",
            "\"exp\": 1e3",
            "\"inf\": Infinity",
            "\"nan\": NaN",
            "\"dup\": 1",
            "\"dup\": 2",
        ] {
            assert!(out.contains(expected), "lost {expected}: {out}");
        }
    }

    // Regression: an inline `/* … */` fused onto the following key
    // (`/* has */"a"`).
    #[test]
    fn fmt_inline_block_comment_keeps_a_space() {
        let src = format!("{HDR}--- RESPONSE ---\n{{\n  /* has */ \"a\": 1\n}}\n");
        let out = assert_idempotent(&src);
        assert!(out.contains("/* has */ \"a\": 1"), "{out}");
    }

    // Regression: a trailing comma was only dropped when it was the last
    // character — a comment after it hid it from the check.
    #[test]
    fn fmt_drops_trailing_comma_behind_a_comment() {
        let src = format!(
            "{HDR}--- RESPONSE ---\n{{\n  \"a\": [\n    1, // one\n    2, // two\n  ],\n}}\n"
        );
        let out = assert_idempotent(&src);
        assert!(out.contains("2 // two"), "trailing comma must go: {out}");
        assert!(!out.contains(",\n  ]"), "{out}");
        assert!(!out.contains(",\n}"), "{out}");
    }

    // Strings are opaque: a digit run inside one is data, not a literal.
    #[test]
    fn fmt_does_not_group_digits_inside_strings() {
        let src = format!(
            "{HDR}--- RESPONSE ---\n{{\n  \"units\": \"1000000\",\n  \"esc\": \"q\\\" 1000000 end\"\n}}\n"
        );
        let out = assert_idempotent(&src);
        assert!(out.contains("\"units\": \"1000000\""), "{out}");
        assert!(out.contains("1000000 end"), "{out}");
    }

    // Regression: key-value sections deleted every comment line outright.
    #[test]
    fn fmt_key_value_section_keeps_comments() {
        let src = "--- ENDPOINT ---\ntest.Service/Method\n\n--- REQUEST_HEADERS ---\n// staging only\nauthorization: Bearer x\nx-trace: 1\n\n--- REQUEST ---\n{}\n";
        let out = assert_idempotent(src);
        assert!(
            out.contains("// staging only\nauthorization: Bearer x"),
            "comment must ride along with the key it documents: {out}"
        );
    }

    // Regression: OPTIONS sorts its keys, and a comment must travel with the
    // key it was written above rather than being dropped or stranded.
    #[test]
    fn fmt_options_comment_travels_with_its_key() {
        let src = format!(
            "{HDR}--- OPTIONS ---\n// why we wait\ntimeout: 30\n// retry policy\nretries: 2\n"
        );
        let out = assert_idempotent(&src);
        assert!(out.contains("// retry policy\nretries: 2"), "{out}");
        assert!(out.contains("// why we wait\ntimeout: 30"), "{out}");
    }

    // Regression: a standalone `/* block */` comment used to be glued directly
    // onto the following key (`/* block */"a"`) because the value token did not
    // honor the pending newline.
    #[test]
    fn fmt_json_block_comment_not_glued() {
        let src = format!(
            "{}--- RESPONSE ---\n{{\n  /* block */\n  \"a\": 1\n}}\n",
            HDR
        );
        let out = assert_idempotent(&src);
        assert!(
            out.contains("/* block */\n  \"a\": 1"),
            "block comment must not be glued to the next key: {out}"
        );
    }

    // Regression: a `#` hash comment inside JSON re-indents onto its own line
    // as `//` without a spurious leading blank line.
    #[test]
    fn fmt_json_hash_comment_idempotent() {
        let src = format!("{}--- RESPONSE ---\n{{\n  # note\n  \"a\": 1\n}}\n", HDR);
        let out = assert_idempotent(&src);
        assert!(out.contains("{\n  // note\n  \"a\": 1\n}"), "{out}");
    }

    // Regression: META is YAML, whose comment marker is `#`. The formatter used
    // to rewrite `#` to `//`, corrupting the YAML. It must be preserved verbatim.
    #[test]
    fn group_digits_groups_in_threes_from_the_right() {
        assert_eq!(super::group_digits("100"), "100");
        assert_eq!(super::group_digits("1000"), "1_000");
        assert_eq!(super::group_digits("1234567"), "1_234_567");
        // Malformed grouping normalizes to the canonical form.
        assert_eq!(super::group_digits("1_00"), "100");
    }

    #[test]
    fn group_numeric_literals_leaves_strings_and_identifiers_alone() {
        let input = r#"{"amount": 1000000, "id_1_2": 3, "note": "order_1_000_2"}"#;
        let out = super::normalize_numeric_literals(input, true);
        assert_eq!(
            out,
            r#"{"amount": 1_000_000, "id_1_2": 3, "note": "order_1_000_2"}"#
        );
    }

    /// A payload is JSON that other tools also read, and `1_000_000` parses as
    /// neither JSON nor JSON5, so grouping stops at the section boundary.
    #[test]
    fn fmt_leaves_json_payload_numbers_ungrouped() {
        let src =
            format!("{HDR}--- RESPONSE ---\n{{\n  \"amount\": 1000000,\n  \"code\": 42\n}}\n");
        let out = assert_idempotent(&src);
        assert!(
            out.contains("\"amount\": 1000000"),
            "payload digits must stay ungrouped: {out}"
        );
        assert!(!out.contains("1_000_000"), "{out}");
        assert!(out.contains("\"code\": 42"), "{out}");
    }

    #[test]
    fn fmt_asserts_numeric_literal_grouped() {
        let src = format!("{HDR}--- ASSERTS ---\n.amount == 1000000\n");
        let out = assert_idempotent(&src);
        assert!(
            out.contains(".amount == 1_000_000"),
            "expected grouped ASSERTS literal: {out}"
        );
    }

    #[test]
    fn fmt_meta_yaml_hash_comment_preserved() {
        let src = "--- META ---\n# a comment\nname: demo\n\n--- ENDPOINT ---\ntest.Service/Method\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n";
        let out = assert_idempotent(src);
        assert!(
            out.contains("--- META ---\n# a comment\nname: demo"),
            "META `#` comment must stay `#`, not become `//`: {out}"
        );
        assert!(
            !out.contains("// a comment"),
            "must not convert to //: {out}"
        );
    }

    // Regression: `if..then..else..end` serializes to a `? :` ternary the parser
    // cannot read back, so rewriting to it broke idempotency. The formatter now
    // keeps the original form when the canonical form does not round-trip.
    #[test]
    fn fmt_asserts_ternary_idempotent() {
        let src = format!(
            "{}--- RESPONSE with_asserts ---\n{{}}\n\n--- ASSERTS ---\nif .x == 1 then true else false end\n",
            HDR
        );
        let out = assert_idempotent(&src);
        assert!(
            out.contains("if .x == 1 then true else false end"),
            "non-round-trippable assertion must be preserved verbatim: {out}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn write_atomic_roundtrip() {
        let dir = std::env::temp_dir().join(format!("fmt_atomic_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.gctf");
        std::fs::write(&path, "old contents\n").unwrap();
        write_atomic(&path, "new contents\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new contents\n");
        // No leftover temp files in the directory.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp file left behind: {leftovers:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fmt_hash_comments_to_slashes() {
        let source = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE ---
 # Protected behavior: even if a stub tries gripmock -> NOT_SERVING,
# runtime must ignore it and return real status.
{
  "status": "SERVING"
}
"#;

        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE ---
// Protected behavior: even if a stub tries gripmock -> NOT_SERVING,
// runtime must ignore it and return real status.
{
  "status": "SERVING"
}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_jsonlines_preserves_comments() {
        let source = r#"--- ENDPOINT ---
grpc.health.v1.Health/Watch

--- REQUEST ---
{
  "service": "examples.health.watch"
}

--- RESPONSE with_asserts=true ---
# Delay applies before first message
{
  "status": "NOT_SERVING"
}
// Then service recovers
{
  "status": "SERVING"
}

--- ASSERTS ---
@scope.message_count() == 2
"#;

        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
grpc.health.v1.Health/Watch

--- REQUEST ---
{
  "service": "examples.health.watch"
}

--- RESPONSE with_asserts ---
// Delay applies before first message
{
  "status": "NOT_SERVING"
}
// Then service recovers
{
  "status": "SERVING"
}

--- ASSERTS ---
@scope.message_count() == 2
"#;

        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_hash_inside_string_not_comment() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{
  "value": "abc#def"
}

--- RESPONSE ---
{
  "ok": true
}
"#;

        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{
  "value": "abc#def"
}

--- RESPONSE ---
{
  "ok": true
}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_collapses_extra_blank_lines() {
        let source = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE with_asserts=true ---
{
  "status": "NOT_SERVING"
}
{
  "status": "SERVING"
}


--- ASSERTS ---
@scope.message_count() == 2
"#;

        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE with_asserts ---
{
  "status": "NOT_SERVING"
}
{
  "status": "SERVING"
}

--- ASSERTS ---
@scope.message_count() == 2
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_inserts_blank_line_before_asserts() {
        let source = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE with_asserts=true ---
{
  "status": "NOT_SERVING"
}
{
  "status": "SERVING"
}
--- ASSERTS --- 
@scope.message_count() == 2
"#;

        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE with_asserts ---
{
  "status": "NOT_SERVING"
}
{
  "status": "SERVING"
}

--- ASSERTS ---
@scope.message_count() == 2
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_crlf_to_lf_between_sections() {
        let source_lf = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE with_asserts=true ---
{
  "status": "NOT_SERVING"
}
{
  "status": "SERVING"
}
--- ASSERTS ---
@scope.message_count() == 2
"#;
        let source = to_crlf(source_lf);

        let formatted = format_gctf_content(&source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE with_asserts ---
{
  "status": "NOT_SERVING"
}
{
  "status": "SERVING"
}

--- ASSERTS ---
@scope.message_count() == 2
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_ends_with_newline() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        assert!(formatted.ends_with('\n'));
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_crlf_to_lf_and_ends_with_newline() {
        let source_lf = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let source = to_crlf(source_lf);
        let formatted = format_gctf_content(&source, "test.gctf").unwrap();
        assert!(formatted.ends_with('\n'));
        assert!(!formatted.contains("\r\n"));
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_address() {
        let source = r#"--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_endpoint() {
        let source = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_request() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{
  "id": 123,
  "name": "test"
}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{
  "id": 123,
  "name": "test"
}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_response_with_inline_option_with_asserts() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE with_asserts=true ---
{
  "status": "ok"
}

--- ASSERTS ---
.status == "ok""#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE with_asserts ---
{
  "status": "ok"
}

--- ASSERTS ---
.status == "ok"
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_response_with_inline_option_partial() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE partial=true ---
{
  "id": 1,
  "name": "test",
  "extra": "ignored"
}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE partial ---
{
  "id": 1,
  "name": "test",
  "extra": "ignored"
}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_response_with_inline_option_tolerance() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE tolerance=0.01 ---
{
  "value": 3.1415926
}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE tolerance=0.01 ---
{
  "value": 3.1415926
}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_response_with_inline_option_redact() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE redact=["token","secret"] ---
{
  "token": "abc123",
  "secret": "xyz789",
  "public": "visible"
}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE redact=["secret","token"] ---
{
  "token": "abc123",
  "secret": "xyz789",
  "public": "visible"
}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_response_with_inline_option_unordered_arrays() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE unordered_arrays=true ---
{
  "items": [3, 1, 2]
}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE unordered_arrays ---
{
  "items": [
    3,
    1,
    2
  ]
}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_response_with_multiple_inline_options() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE with_asserts=true partial=true tolerance=0.1 ---
{
  "status": "ok"
}

--- ASSERTS ---
.status == "ok"
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE partial tolerance=0.1 with_asserts ---
{
  "status": "ok"
}

--- ASSERTS ---
.status == "ok"
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_error() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- ERROR ---
{
  "code": 3,
  "message": "Invalid argument"
}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- ERROR ---
{
  "code": 3,
  "message": "Invalid argument"
}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_error_with_inline_options() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- ERROR with_asserts=true ---
{
  "code": 3
}

--- ASSERTS ---
.code == 3
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- ERROR with_asserts ---
{
  "code": 3
}

--- ASSERTS ---
.code == 3
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_error_with_partial_and_with_asserts_options() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- ERROR with_asserts=true partial=true ---
{
  "code": 5
}

--- ASSERTS ---
.code == 5
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- ERROR partial with_asserts ---
{
  "code": 5
}

--- ASSERTS ---
.code == 5
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_request_headers() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST_HEADERS ---
Content-Type: application/json
Authorization: Bearer token123

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST_HEADERS ---
Authorization: Bearer token123
Content-Type: application/json

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_asserts() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE with_asserts=true ---
{
  "status": "ok",
  "count": 42
}

--- ASSERTS ---
.status == "ok"
.count == 42
.count > 10
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE with_asserts ---
{
  "status": "ok",
  "count": 42
}

--- ASSERTS ---
.status == "ok"
.count == 42
.count > 10
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_proto() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- PROTO ---
files: service.proto
import_path: /proto

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- PROTO ---
files: service.proto
import_path: /proto

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_tls() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- TLS ---
insecure: false
ca_cert: /path/to/ca.crt
server_name: example.com

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- TLS ---
ca_cert: /path/to/ca.crt
insecure: false
server_name: example.com

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_options() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- OPTIONS ---
timeout: 30
dry_run: true
retry_count: 3

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- OPTIONS ---
dry_run: true
retry_count: 3
timeout: 30

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_section_extract() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{
  "id": 123,
  "token": "abc456"
}

--- EXTRACT ---
user_id: .id
auth_token: .token
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{
  "id": 123,
  "token": "abc456"
}

--- EXTRACT ---
user_id: .id
auth_token: .token
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_extract_with_type_annotation() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{"price": 42}

--- EXTRACT ---
total:number = .price
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{
  "price": 42
}

--- EXTRACT ---
total:number = .price
"#;
        assert_eq!(
            formatted, expected,
            "Type annotation in EXTRACT should be preserved as total:number = .price"
        );
    }

    #[test]
    fn fmt_all_sections_in_order() {
        let source = r#"--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST_HEADERS ---
Content-Type: application/json
Authorization: Bearer token

--- PROTO ---
files: health.proto

--- TLS ---
insecure: false

--- OPTIONS ---
timeout: 10

--- REQUEST ---
{
  "service": "grpc.health.v1.Health"
}

--- RESPONSE ---
{
  "status": "SERVING"
}

--- ERROR ---
{
  "code": 5,
  "message": "Service not found"
}

--- ASSERTS ---
.status == "SERVING"

--- EXTRACT ---
status_code: .status
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST_HEADERS ---
Authorization: Bearer token
Content-Type: application/json

--- PROTO ---
files: health.proto

--- TLS ---
insecure: false

--- OPTIONS ---
timeout: 10

--- REQUEST ---
{
  "service": "grpc.health.v1.Health"
}

--- RESPONSE ---
{
  "status": "SERVING"
}

--- ERROR ---
{
  "code": 5,
  "message": "Service not found"
}

--- ASSERTS ---
.status == "SERVING"

--- EXTRACT ---
status_code: .status
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_inline_options_all_boolean_combinations() {
        let combinations = [
            ("", ""),
            ("with_asserts=true", "with_asserts"),
            ("partial=true", "partial"),
            ("unordered_arrays=true", "unordered_arrays"),
            ("with_asserts=true partial=true", "partial with_asserts"),
            (
                "with_asserts=true unordered_arrays=true",
                "unordered_arrays with_asserts",
            ),
            (
                "partial=true unordered_arrays=true",
                "partial unordered_arrays",
            ),
            (
                "with_asserts=true partial=true unordered_arrays=true",
                "partial unordered_arrays with_asserts",
            ),
        ];

        for (input_opts, expected_opts) in combinations {
            let source = format!(
                r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{{}}

--- RESPONSE{} ---
{{"status": "ok"}}
"#,
                if input_opts.is_empty() {
                    String::new()
                } else {
                    format!(" {}", input_opts)
                }
            );

            let expected = format!(
                r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{{}}

--- RESPONSE{} ---
{{
  "status": "ok"
}}
"#,
                if expected_opts.is_empty() {
                    String::new()
                } else {
                    format!(" {}", expected_opts)
                }
            );

            let formatted = format_gctf_content(&source, "test.gctf").unwrap();
            assert_eq!(formatted, expected, "Failed for options: {}", input_opts);
        }
    }

    #[test]
    fn fmt_detached_comments_preserved() {
        let source = r#"// This is a detached comment before the endpoint
--- ENDPOINT ---
test.Service/Method

// Another detached comment
--- REQUEST ---
{}

// Comment before response
--- RESPONSE ---
{}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        // Preamble/body boundary (ENDPOINT → REQUEST) is re-homed for
        // reorder safety, so that comment now attaches directly to REQUEST;
        // body-to-body (REQUEST → RESPONSE) is untouched.
        let expected = r#"// This is a detached comment before the endpoint
--- ENDPOINT ---
test.Service/Method

// Another detached comment
--- REQUEST ---
{}
// Comment before response

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn fmt_attribute_between_sections_no_duplicated_or_phantom_lines() {
        // Regression: a `#[attr]` line between two sections used to make the
        // *preceding* section's parsed `end_line` overshoot past it, which
        // made `format_gctf_chain`'s block-reassembly re-copy a stray raw
        // line (extra blank line, or in the worst case a duplicated content
        // line) around the attribute boundary.
        let source = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{}

#[timeout(5)]
--- RESPONSE ---
{"status": "SERVING"}
"#;
        let once = format_gctf_content(source, "test.gctf").unwrap();
        let twice = format_gctf_content(&once, "test.gctf").unwrap();
        assert_eq!(once, twice, "fmt must be idempotent: {once}");
        assert_eq!(
            once.matches("timeout").count(),
            1,
            "attribute must appear exactly once: {once}"
        );
        assert_eq!(
            once.matches("SERVING").count(),
            1,
            "RESPONSE body must not be duplicated: {once}"
        );
    }

    #[test]
    fn fmt_reorders_scrambled_preamble_to_canonical() {
        let source = r#"--- META ---
name: my test

--- ENDPOINT ---
pkg.Svc/Method

--- ADDRESS ---
localhost:4770

--- BENCH ---
mode: fixed

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let positions: Vec<usize> = ["META", "BENCH", "ADDRESS", "ENDPOINT"]
            .iter()
            .map(|s| formatted.find(&format!("--- {s} ---")).unwrap())
            .collect();
        let mut sorted = positions.clone();
        sorted.sort();
        assert_eq!(
            positions, sorted,
            "sections not in canonical order: {formatted}"
        );
    }

    #[test]
    fn fmt_reorders_dataset_before_address_and_endpoint() {
        let source = r#"--- OPTIONS ---
timeout: 5

--- ADDRESS ---
localhost:4770

--- DATASET ---
- id: '1'

--- ENDPOINT ---
pkg.Svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let positions: Vec<usize> = ["DATASET", "ADDRESS", "ENDPOINT", "OPTIONS"]
            .iter()
            .map(|s| formatted.find(&format!("--- {s} ---")).unwrap())
            .collect();
        let mut sorted = positions.clone();
        sorted.sort();
        assert_eq!(
            positions, sorted,
            "DATASET must sort before ADDRESS/ENDPOINT/OPTIONS: {formatted}"
        );
    }

    #[test]
    fn fmt_preserves_hash_comments_inside_dataset() {
        let source = r#"--- ENDPOINT ---
pkg.Svc/Method

--- DATASET ---
# real users only
- id: '1'  # ada

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        assert!(
            formatted.contains("# real users only"),
            "YAML comment must survive fmt, not become `//`: {formatted}"
        );
        assert!(
            formatted.contains("# ada"),
            "trailing YAML comment must survive fmt: {formatted}"
        );
        assert!(!formatted.contains("// real users only"));
    }

    #[test]
    fn fmt_reorder_is_idempotent() {
        let source = r#"--- ENDPOINT ---
pkg.Svc/Method

--- ADDRESS ---
localhost:4770

--- META ---
name: my test

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let once = format_gctf_content(source, "test.gctf").unwrap();
        let twice = format_gctf_content(&once, "test.gctf").unwrap();
        assert_eq!(once, twice);
    }

    // Regression: a comment documenting a specific preamble section must
    // travel with that section when reordering moves it, not stay behind
    // with whatever section now precedes it (the parser attributes a
    // trailing comment to the preceding section's raw content, not the one
    // it visually precedes).
    #[test]
    fn fmt_reorder_carries_comment_with_its_section() {
        let source = r#"--- META ---
name: my test

--- ENDPOINT ---
pkg.Svc/Method

--- ADDRESS ---
localhost:4770

// documents the bench profile
--- BENCH ---
mode: fixed

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let comment_pos = formatted.find("// documents the bench profile").unwrap();
        let bench_pos = formatted.find("--- BENCH ---").unwrap();
        let address_pos = formatted.find("--- ADDRESS ---").unwrap();
        assert!(
            comment_pos < bench_pos && bench_pos < comment_pos + 40,
            "comment must sit directly above BENCH: {formatted}"
        );
        assert!(
            bench_pos < address_pos,
            "BENCH must now precede ADDRESS: {formatted}"
        );
    }

    // Regression: BENCH.sources: is a nested YAML list — formatting must not
    // scatter it into sorted, unindented top-level keys.
    #[test]
    fn fmt_bench_sources_nested_yaml_survives() {
        let source = r#"--- BENCH ---
duration: 30s
mode: fixed
sources:
  - name: users
    file: data/users.csv
  - name: orders
    file: data/orders.csv

--- ENDPOINT ---
pkg.Svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        assert!(
            formatted.contains("sources:\n  - name: users\n    file: data/users.csv\n  - name: orders\n    file: data/orders.csv\n"),
            "sources: list must stay intact and indented: {formatted}"
        );
        let twice = format_gctf_content(&formatted, "test.gctf").unwrap();
        assert_eq!(formatted, twice, "must be idempotent");
    }

    // Regression: BENCH keys must sort by the canonical `bench_key_rank`
    // order (matching `check` and the other BENCH serializer in
    // apif-parser), not alphabetically — `mode` before `duration`, etc.
    #[test]
    fn fmt_bench_keys_sort_canonically_not_alphabetically() {
        let source = r#"--- BENCH ---
duration: 30s
concurrency: 16
mode: fixed
requests: 5000
profile: smoke

--- ENDPOINT ---
pkg.Svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let positions: Vec<usize> = ["mode", "profile", "concurrency", "requests", "duration"]
            .iter()
            .map(|k| formatted.find(&format!("{k}:")).unwrap())
            .collect();
        let mut sorted = positions.clone();
        sorted.sort();
        assert_eq!(
            positions, sorted,
            "BENCH keys not in canonical order: {formatted}"
        );
    }
}
