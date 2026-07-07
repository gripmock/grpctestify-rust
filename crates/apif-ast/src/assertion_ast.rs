//! AST nodes for assertion expressions.
//!
//! Pipeline: `text → tokenize_assertion() → Vec<Token> → parse_assertion() → AssertionExpr`
//!
//! The tokenizer (`super::tokenizer`) is the single source of tokens with exact byte positions.
//!
//! Precedence (low to high):
//!   pipe (`| not`)
//!   or
//!   xor
//!   and
//!   binary (==, !=, >, <, contains, matches, …)
//!   unary (!, not, not not, !!)
//!   atom (literal, @plugin, .path, paren)

use serde::{Deserialize, Serialize};

use crate::tokenizer::{TokenKind, tokenize_assertion};

/// A complete assertion expression (top-level).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AssertionExpr {
    Binary {
        op: BinaryOp,
        left: Box<AssertionExpr>,
        right: Box<AssertionExpr>,
    },
    Not(Box<AssertionExpr>),
    NotNot(Box<AssertionExpr>),
    And {
        left: Box<AssertionExpr>,
        right: Box<AssertionExpr>,
    },
    Or {
        left: Box<AssertionExpr>,
        right: Box<AssertionExpr>,
    },
    Xor {
        left: Box<AssertionExpr>,
        right: Box<AssertionExpr>,
    },
    IfThenElse {
        condition: Box<AssertionExpr>,
        then_branch: Box<AssertionExpr>,
        else_branch: Box<AssertionExpr>,
    },
    Paren(Box<AssertionExpr>),
    Atom(Expr),
    Raw(String),
}

/// Atomic expressions (leaf nodes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    JqPath(String),
    PluginCall {
        name: String,
        args: Vec<AssertionExpr>,
    },
    Literal(Literal),
    Variable(String),
    RegExp {
        pattern: String,
        flags: String,
    },
    Json(String),
    Yaml(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    Bool(bool),
    Number(String),
    Str(String),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    Eq,
    Ne,
    Gt,
    Lt,
    Ge,
    Le,
    Contains,
    Matches,
    StartsWith,
    EndsWith,
}

impl BinaryOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Gt => ">",
            Self::Lt => "<",
            Self::Ge => ">=",
            Self::Le => "<=",
            Self::Contains => "contains",
            Self::Matches => "matches",
            Self::StartsWith => "startsWith",
            Self::EndsWith => "endsWith",
        }
    }
    fn try_parse(s: &str) -> Option<Self> {
        match s {
            "==" => Some(Self::Eq),
            "!=" => Some(Self::Ne),
            ">" => Some(Self::Gt),
            "<" => Some(Self::Lt),
            ">=" => Some(Self::Ge),
            "<=" => Some(Self::Le),
            "contains" => Some(Self::Contains),
            "matches" => Some(Self::Matches),
            "startsWith" | "startswith" => Some(Self::StartsWith),
            "endsWith" | "endswith" => Some(Self::EndsWith),
            _ => None,
        }
    }
}

// ─── Display ───────────────────────────────────────────────────────────

impl std::fmt::Display for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::JqPath(p) => write!(f, "{}", p),
            Self::PluginCall { name, args } => {
                write!(f, "@{}(", name)?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", a)?;
                }
                write!(f, ")")
            }
            Self::Literal(Literal::Bool(b)) => write!(f, "{}", b),
            Self::Literal(Literal::Number(n)) => write!(f, "{}", n),
            Self::Literal(Literal::Str(s)) => write!(f, "\"{}\"", s),
            Self::Literal(Literal::Null) => write!(f, "null"),
            Self::RegExp { pattern, flags } => {
                write!(f, "/{}/", pattern)?;
                if !flags.is_empty() {
                    write!(f, "{}", flags)?;
                }
                Ok(())
            }
            Self::Json(s) => write!(f, "{}", s),
            Self::Yaml(s) => write!(f, "{}", s),
            Self::Variable(n) => write!(f, "{{{{{}}}}}", n),
        }
    }
}

impl std::fmt::Display for AssertionExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt_assertion(self, f, 0)
    }
}

fn fmt_assertion(e: &AssertionExpr, f: &mut std::fmt::Formatter<'_>, prec: u8) -> std::fmt::Result {
    match e {
        AssertionExpr::Or { left, right } => {
            if prec > 1 {
                write!(f, "(")?;
            }
            fmt_assertion(left, f, 1)?;
            write!(f, " or ")?;
            fmt_assertion(right, f, 1)?;
            if prec > 1 {
                write!(f, ")")?;
            }
            Ok(())
        }
        AssertionExpr::Xor { left, right } => {
            if prec > 1 {
                write!(f, "(")?;
            }
            fmt_assertion(left, f, 1)?;
            write!(f, " xor ")?;
            fmt_assertion(right, f, 1)?;
            if prec > 1 {
                write!(f, ")")?;
            }
            Ok(())
        }
        AssertionExpr::And { left, right } => {
            if prec > 2 {
                write!(f, "(")?;
            }
            fmt_assertion(left, f, 2)?;
            write!(f, " and ")?;
            fmt_assertion(right, f, 2)?;
            if prec > 2 {
                write!(f, ")")?;
            }
            Ok(())
        }
        AssertionExpr::Binary { op, left, right } => {
            if prec > 3 {
                write!(f, "(")?;
            }
            fmt_assertion(left, f, 3)?;
            write!(f, " {} ", op.as_str())?;
            fmt_assertion(right, f, 3)?;
            if prec > 3 {
                write!(f, ")")?;
            }
            Ok(())
        }
        AssertionExpr::Not(inner) => {
            write!(f, "!")?;
            fmt_assertion(inner, f, 4)
        }
        AssertionExpr::NotNot(inner) => {
            write!(f, "not not ")?;
            fmt_assertion(inner, f, 4)
        }
        AssertionExpr::IfThenElse {
            condition,
            then_branch,
            else_branch,
        } => {
            write!(f, "(")?;
            fmt_assertion(condition, f, 0)?;
            write!(f, " ? ")?;
            fmt_assertion(then_branch, f, 0)?;
            write!(f, " : ")?;
            fmt_assertion(else_branch, f, 0)?;
            write!(f, ")")
        }
        AssertionExpr::Paren(inner) => {
            write!(f, "(")?;
            fmt_assertion(inner, f, 0)?;
            write!(f, ")")
        }
        AssertionExpr::Atom(e) => write!(f, "{}", e),
        AssertionExpr::Raw(s) => write!(f, "{}", s),
    }
}

// ─── Parser ────────────────────────────────────────────────────────────

/// Parse a raw assertion string into an AST.
/// Falls back to `Raw` if parsing fails.
pub fn parse_assertion(raw: &str) -> AssertionExpr {
    let tokens = tokenize_assertion(raw);
    if tokens.is_empty() {
        return AssertionExpr::Raw(raw.to_string());
    }
    let mut pos = 0;
    let expr = parse_pipe(&tokens, &mut pos);
    if pos >= tokens.len() {
        expr
    } else {
        AssertionExpr::Raw(raw.to_string())
    }
}

fn parse_pipe(ts: &[crate::tokenizer::Token], p: &mut usize) -> AssertionExpr {
    let mut expr = parse_or(ts, p);
    while *p < ts.len() {
        if !matches!(ts[*p].kind, TokenKind::Pipe) {
            break;
        }
        *p += 1;
        if *p < ts.len() && is_keyword(ts, *p, "not") {
            *p += 1;
            if *p < ts.len() && is_keyword(ts, *p, "not") {
                *p += 1;
            } else {
                expr = AssertionExpr::Not(Box::new(expr));
            }
        } else {
            break;
        }
    }
    expr
}

fn parse_or(ts: &[crate::tokenizer::Token], p: &mut usize) -> AssertionExpr {
    let mut left = parse_xor(ts, p);
    while *p < ts.len() && is_keyword(ts, *p, "or") {
        *p += 1;
        let right = parse_xor(ts, p);
        left = AssertionExpr::Or {
            left: Box::new(left),
            right: Box::new(right),
        };
    }
    left
}

fn parse_xor(ts: &[crate::tokenizer::Token], p: &mut usize) -> AssertionExpr {
    let mut left = parse_and(ts, p);
    while *p < ts.len() && is_keyword(ts, *p, "xor") {
        *p += 1;
        let right = parse_and(ts, p);
        left = AssertionExpr::Xor {
            left: Box::new(left),
            right: Box::new(right),
        };
    }
    left
}

fn parse_and(ts: &[crate::tokenizer::Token], p: &mut usize) -> AssertionExpr {
    let mut left = parse_bin(ts, p);
    while *p < ts.len() && is_keyword(ts, *p, "and") {
        *p += 1;
        let right = parse_bin(ts, p);
        left = AssertionExpr::And {
            left: Box::new(left),
            right: Box::new(right),
        };
    }
    left
}

fn parse_bin(ts: &[crate::tokenizer::Token], p: &mut usize) -> AssertionExpr {
    let mut left = parse_unary(ts, p);
    loop {
        let op = match ts.get(*p).map(|t| &t.kind) {
            Some(TokenKind::Op(s)) => match s.as_str() {
                "==" => BinaryOp::Eq,
                "!=" => BinaryOp::Ne,
                ">" => BinaryOp::Gt,
                "<" => BinaryOp::Lt,
                ">=" => BinaryOp::Ge,
                "<=" => BinaryOp::Le,
                "contains" => BinaryOp::Contains,
                "matches" => BinaryOp::Matches,
                "startsWith" | "startswith" => BinaryOp::StartsWith,
                "endsWith" | "endswith" => BinaryOp::EndsWith,
                _ => BinaryOp::EndsWith,
            },
            Some(TokenKind::Ident(s)) if is_bin_op_keyword(s) => {
                BinaryOp::try_parse(s).unwrap_or(BinaryOp::EndsWith)
            }
            None => break,
            _ => break,
        };

        *p += 1;
        let right = parse_unary(ts, p);
        left = AssertionExpr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        };
    }
    left
}

fn parse_unary(ts: &[crate::tokenizer::Token], p: &mut usize) -> AssertionExpr {
    if *p >= ts.len() {
        return AssertionExpr::Raw(String::new());
    }
    match &ts[*p].kind {
        TokenKind::Bang => {
            *p += 1;
            if *p < ts.len() && matches!(ts[*p].kind, TokenKind::Bang) {
                *p += 1;
                let inner = parse_unary(ts, p);
                AssertionExpr::NotNot(Box::new(inner))
            } else {
                let inner = parse_unary(ts, p);
                AssertionExpr::Not(Box::new(inner))
            }
        }
        TokenKind::Ident(s) if s == "not" => {
            *p += 1;
            if *p < ts.len() && matches!(ts[*p].kind, TokenKind::Ident(ref s2) if s2 == "not") {
                *p += 1;
                let inner = parse_unary(ts, p);
                AssertionExpr::NotNot(Box::new(inner))
            } else {
                let inner = parse_unary(ts, p);
                AssertionExpr::Not(Box::new(inner))
            }
        }
        TokenKind::Ident(s) if s == "if" => parse_if(ts, p),
        TokenKind::LParen => {
            *p += 1;
            let inner = parse_pipe(ts, p);
            if *p < ts.len() && matches!(ts[*p].kind, TokenKind::RParen) {
                *p += 1;
            }
            AssertionExpr::Paren(Box::new(inner))
        }
        _ => parse_atom(ts, p),
    }
}

fn parse_if(ts: &[crate::tokenizer::Token], p: &mut usize) -> AssertionExpr {
    *p += 1;
    let cond = parse_pipe(ts, p);
    if *p >= ts.len() || !is_keyword(ts, *p, "then") {
        return AssertionExpr::Raw("if..then missing".into());
    }
    *p += 1;
    let then_b = parse_pipe(ts, p);
    if *p >= ts.len() || !is_keyword(ts, *p, "else") {
        return AssertionExpr::Raw("if..else missing".into());
    }
    *p += 1;
    let else_b = parse_pipe(ts, p);
    if *p < ts.len() && is_keyword(ts, *p, "end") {
        *p += 1;
    }
    AssertionExpr::IfThenElse {
        condition: Box::new(cond),
        then_branch: Box::new(then_b),
        else_branch: Box::new(else_b),
    }
}

fn parse_atom(ts: &[crate::tokenizer::Token], p: &mut usize) -> AssertionExpr {
    if *p >= ts.len() {
        return AssertionExpr::Atom(Expr::JqPath(String::new()));
    }
    let expr = match &ts[*p].kind {
        TokenKind::StringLit(s) => {
            *p += 1;
            Expr::Literal(Literal::Str(s.clone()))
        }
        TokenKind::NumberLit(n) => {
            *p += 1;
            Expr::Literal(Literal::Number(n.clone()))
        }
        TokenKind::Ident(s) if s == "true" => {
            *p += 1;
            Expr::Literal(Literal::Bool(true))
        }
        TokenKind::Ident(s) if s == "false" => {
            *p += 1;
            Expr::Literal(Literal::Bool(false))
        }
        TokenKind::Ident(s) if s == "null" => {
            *p += 1;
            Expr::Literal(Literal::Null)
        }
        TokenKind::VarDelim => {
            *p += 1;
            let mut name = String::with_capacity(16);
            while *p < ts.len() && !matches!(ts[*p].kind, TokenKind::VarDelim) {
                if let TokenKind::Ident(s) = &ts[*p].kind {
                    if !name.is_empty() {
                        name.push(' ');
                    }
                    name.push_str(s);
                }
                *p += 1;
            }
            if *p < ts.len() {
                *p += 1;
            }
            Expr::Variable(name)
        }
        TokenKind::At => {
            *p += 1;
            let name = if *p < ts.len() {
                if let TokenKind::Ident(s) = &ts[*p].kind {
                    *p += 1;
                    s.clone()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let args = if *p < ts.len() && matches!(ts[*p].kind, TokenKind::LParen) {
                *p += 1;
                let mut args = Vec::with_capacity(4);
                while *p < ts.len() && !matches!(ts[*p].kind, TokenKind::RParen) {
                    let arg = parse_pipe(ts, p);
                    args.push(arg);
                    if *p < ts.len() && matches!(ts[*p].kind, TokenKind::Comma) {
                        *p += 1;
                    }
                }
                if *p < ts.len() {
                    *p += 1;
                }
                merge_hyphenated_args(args)
            } else {
                Vec::new()
            };
            Expr::PluginCall { name, args }
        }
        TokenKind::RegExpLit { pattern, flags } => {
            *p += 1;
            Expr::RegExp {
                pattern: pattern.clone(),
                flags: flags.clone(),
            }
        }
        TokenKind::LBrace => {
            *p += 1;
            let mut json = String::with_capacity(64);
            json.push('{');
            let mut depth = 1;
            while *p < ts.len() && depth > 0 {
                match &ts[*p].kind {
                    TokenKind::LBrace => {
                        depth += 1;
                        json.push('{');
                        *p += 1;
                    }
                    TokenKind::RBrace => {
                        depth -= 1;
                        if depth > 0 {
                            json.push('}');
                        }
                        *p += 1;
                    }
                    TokenKind::LParen => {
                        json.push('(');
                        *p += 1;
                    }
                    TokenKind::RParen => {
                        json.push(')');
                        *p += 1;
                    }
                    TokenKind::LBracket => {
                        json.push('[');
                        *p += 1;
                    }
                    TokenKind::RBracket => {
                        json.push(']');
                        *p += 1;
                    }
                    TokenKind::StringLit(s) => {
                        json.push('"');
                        json.push_str(s);
                        json.push('"');
                        *p += 1;
                    }
                    TokenKind::NumberLit(n) => {
                        json.push_str(n);
                        *p += 1;
                    }
                    TokenKind::Ident(s) => {
                        json.push_str(s);
                        *p += 1;
                    }
                    TokenKind::Comma => {
                        json.push(',');
                        *p += 1;
                    }
                    TokenKind::Op(s) => {
                        json.push_str(s);
                        *p += 1;
                    }
                    TokenKind::Dot => {
                        json.push('.');
                        *p += 1;
                    }
                    TokenKind::At => {
                        json.push('@');
                        *p += 1;
                    }
                    TokenKind::VarDelim => {
                        json.push_str("{{ }}");
                        *p += 1;
                    }
                    _ => {
                        *p += 1;
                    }
                }
            }
            json.push('}');
            Expr::Json(json)
        }
        TokenKind::Dot | TokenKind::Ident(_) => {
            let mut path = String::with_capacity(24);
            while *p < ts.len() {
                if let TokenKind::Ident(s) = &ts[*p].kind
                    && (is_bin_op_keyword(s) || is_keyword_token(&ts[*p].kind))
                {
                    break;
                }
                match &ts[*p].kind {
                    TokenKind::Dot => {
                        path.push('.');
                        *p += 1;
                    }
                    TokenKind::Ident(s) => {
                        path.push_str(s);
                        *p += 1;
                    }
                    TokenKind::LBracket => {
                        path.push('[');
                        *p += 1;
                        while *p < ts.len() && !matches!(ts[*p].kind, TokenKind::RBracket) {
                            if let TokenKind::NumberLit(n) = &ts[*p].kind {
                                path.push_str(n);
                            } else if let TokenKind::Ident(s) = &ts[*p].kind {
                                path.push_str(s);
                            }
                            *p += 1;
                        }
                        if *p < ts.len() {
                            path.push(']');
                            *p += 1;
                        }
                    }
                    _ => break,
                }
            }
            Expr::JqPath(path)
        }
        _ => {
            *p += 1;
            Expr::JqPath(String::new())
        }
    };
    AssertionExpr::Atom(expr)
}

fn is_keyword(ts: &[crate::tokenizer::Token], idx: usize, kw: &str) -> bool {
    matches!(ts.get(idx), Some(t) if matches!(&t.kind, TokenKind::Ident(s) if s == kw))
}

/// Merge consecutive bare JqPath args that were split by `-`.
/// e.g. `[JqPath("content"), JqPath("type")]` → `[JqPath("content-type")]`
fn merge_hyphenated_args(args: Vec<AssertionExpr>) -> Vec<AssertionExpr> {
    if args.len() <= 1 {
        return args;
    }

    let mut merged = Vec::with_capacity(args.len());

    let mut iter = args.into_iter().peekable();
    while let Some(current) = iter.next() {
        match current {
            AssertionExpr::Atom(Expr::JqPath(mut cur)) if !cur.contains('.') => {
                while let Some(AssertionExpr::Atom(Expr::JqPath(nxt))) = iter.peek() {
                    if nxt.contains('.') {
                        break;
                    }

                    let Some(AssertionExpr::Atom(Expr::JqPath(nxt_owned))) = iter.next() else {
                        break;
                    };

                    cur.push('-');
                    cur.push_str(&nxt_owned);
                }

                merged.push(AssertionExpr::Atom(Expr::JqPath(cur)));
            }
            other => merged.push(other),
        }
    }

    merged
}

fn is_bin_op_keyword(s: &str) -> bool {
    matches!(
        s,
        "contains" | "matches" | "startsWith" | "endsWith" | "startswith" | "endswith"
    )
}

fn is_keyword_token(k: &TokenKind) -> bool {
    matches!(
        k,
        TokenKind::Ident(s)
            if matches!(
                s.as_str(),
                "and" | "or" | "xor" | "contains" | "matches" | "startsWith"
                    | "endsWith" | "startswith" | "endswith"
            )
    )
}

// ─── Public helpers ─────────────────────────────────────────────────────

/// Convert AssertionExpr back to string (ternary for if-then-else).
pub fn assertion_to_string(expr: &AssertionExpr) -> String {
    let mut out = String::with_capacity(64);
    push_assertion(expr, &mut out, 0);
    out
}

fn push_expr(expr: &Expr, out: &mut String) {
    match expr {
        Expr::JqPath(p) => out.push_str(p),
        Expr::PluginCall { name, args } => {
            out.push('@');
            out.push_str(name);
            out.push('(');
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                push_assertion(a, out, 0);
            }
            out.push(')');
        }
        Expr::Literal(Literal::Bool(b)) => out.push_str(if *b { "true" } else { "false" }),
        Expr::Literal(Literal::Number(n)) => out.push_str(n),
        Expr::Literal(Literal::Str(s)) => {
            out.push('"');
            out.push_str(s);
            out.push('"');
        }
        Expr::Literal(Literal::Null) => out.push_str("null"),
        Expr::Variable(n) => {
            out.push_str("{{");
            out.push_str(n);
            out.push_str("}}");
        }
        Expr::RegExp { pattern, flags } => {
            out.push('/');
            out.push_str(pattern);
            out.push('/');
            out.push_str(flags);
        }
        Expr::Json(s) | Expr::Yaml(s) => out.push_str(s),
    }
}

fn push_assertion(expr: &AssertionExpr, out: &mut String, prec: u8) {
    match expr {
        AssertionExpr::Or { left, right } => {
            if prec > 1 {
                out.push('(');
            }
            push_assertion(left, out, 1);
            out.push_str(" or ");
            push_assertion(right, out, 1);
            if prec > 1 {
                out.push(')');
            }
        }
        AssertionExpr::Xor { left, right } => {
            if prec > 1 {
                out.push('(');
            }
            push_assertion(left, out, 1);
            out.push_str(" xor ");
            push_assertion(right, out, 1);
            if prec > 1 {
                out.push(')');
            }
        }
        AssertionExpr::And { left, right } => {
            if prec > 2 {
                out.push('(');
            }
            push_assertion(left, out, 2);
            out.push_str(" and ");
            push_assertion(right, out, 2);
            if prec > 2 {
                out.push(')');
            }
        }
        AssertionExpr::Binary { op, left, right } => {
            if prec > 3 {
                out.push('(');
            }
            push_assertion(left, out, 3);
            out.push(' ');
            out.push_str(op.as_str());
            out.push(' ');
            push_assertion(right, out, 3);
            if prec > 3 {
                out.push(')');
            }
        }
        AssertionExpr::Not(inner) => {
            out.push('!');
            push_assertion(inner, out, 4);
        }
        AssertionExpr::NotNot(inner) => {
            out.push_str("not not ");
            push_assertion(inner, out, 4);
        }
        AssertionExpr::IfThenElse {
            condition,
            then_branch,
            else_branch,
        } => {
            out.push('(');
            push_assertion(condition, out, 0);
            out.push_str(" ? ");
            push_assertion(then_branch, out, 0);
            out.push_str(" : ");
            push_assertion(else_branch, out, 0);
            out.push(')');
        }
        AssertionExpr::Paren(inner) => {
            out.push('(');
            push_assertion(inner, out, 0);
            out.push(')');
        }
        AssertionExpr::Atom(e) => push_expr(e, out),
        AssertionExpr::Raw(s) => out.push_str(s),
    }
}

/// Remove redundant parentheses.
pub fn remove_redundant_parens(expr: &AssertionExpr) -> AssertionExpr {
    match expr {
        AssertionExpr::Paren(inner) => remove_redundant_parens(inner),
        AssertionExpr::Binary { op, left, right } => AssertionExpr::Binary {
            op: *op,
            left: Box::new(remove_redundant_parens(left)),
            right: Box::new(remove_redundant_parens(right)),
        },
        AssertionExpr::Not(e) => AssertionExpr::Not(Box::new(remove_redundant_parens(e))),
        AssertionExpr::NotNot(e) => AssertionExpr::NotNot(Box::new(remove_redundant_parens(e))),
        AssertionExpr::And { left, right } => AssertionExpr::And {
            left: Box::new(remove_redundant_parens(left)),
            right: Box::new(remove_redundant_parens(right)),
        },
        AssertionExpr::Or { left, right } => AssertionExpr::Or {
            left: Box::new(remove_redundant_parens(left)),
            right: Box::new(remove_redundant_parens(right)),
        },
        AssertionExpr::Xor { left, right } => AssertionExpr::Xor {
            left: Box::new(remove_redundant_parens(left)),
            right: Box::new(remove_redundant_parens(right)),
        },
        AssertionExpr::IfThenElse {
            condition,
            then_branch,
            else_branch,
        } => AssertionExpr::IfThenElse {
            condition: Box::new(remove_redundant_parens(condition)),
            then_branch: Box::new(remove_redundant_parens(then_branch)),
            else_branch: Box::new(remove_redundant_parens(else_branch)),
        },
        _ => expr.clone(),
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(miri))]
    use std::hint::black_box;
    #[cfg(not(miri))]
    use std::time::Instant;

    #[cfg(not(miri))]
    fn bench_phase(name: &str, iterations: u32, mut f: impl FnMut()) {
        let start = Instant::now();
        for _ in 0..iterations {
            f();
        }
        let elapsed = start.elapsed();
        let per_call = elapsed / iterations;
        eprintln!(
            "{}: {} iterations in {:?} ({:?}/call)",
            name, iterations, elapsed, per_call
        );
    }

    #[test]
    #[cfg(not(miri))]
    fn perf_phase_breakdown_simple() {
        let expr = ".id == 42 and .active == true";

        bench_phase("ast_phase_simple_tokenize", 200_000, || {
            let tokens = tokenize_assertion(black_box(expr));
            black_box(tokens.len());
        });

        bench_phase("ast_phase_simple_parse_from_tokens", 200_000, || {
            let tokens = tokenize_assertion(black_box(expr));
            let mut pos = 0;
            let ast = parse_pipe(&tokens, &mut pos);
            black_box(matches!(ast, AssertionExpr::Raw(_)));
            black_box(pos);
        });

        bench_phase("ast_phase_simple_serialize", 200_000, || {
            let ast = parse_assertion(black_box(expr));
            let s = assertion_to_string(&ast);
            black_box(s.len());
        });
    }

    #[test]
    #[cfg(not(miri))]
    fn perf_phase_breakdown_complex() {
        let expr = "if @len(.items) > 0 then (@regex(.name, /foo.*/i) and .meta.version >= 2) else (.status == \"empty\" or {{ feature_flag }} == true) end";

        bench_phase("ast_phase_complex_tokenize", 100_000, || {
            let tokens = tokenize_assertion(black_box(expr));
            black_box(tokens.len());
        });

        bench_phase("ast_phase_complex_parse_from_tokens", 100_000, || {
            let tokens = tokenize_assertion(black_box(expr));
            let mut pos = 0;
            let ast = parse_pipe(&tokens, &mut pos);
            black_box(matches!(ast, AssertionExpr::Raw(_)));
            black_box(pos);
        });

        bench_phase("ast_phase_complex_serialize", 50_000, || {
            let ast = parse_assertion(black_box(expr));
            let s = assertion_to_string(&ast);
            black_box(s.len());
        });
    }

    #[test]
    fn test_parse_simple_equality() {
        let expr = parse_assertion(".id == 123");
        if let AssertionExpr::Binary { op, left, right } = expr {
            assert_eq!(op, BinaryOp::Eq);
            assert!(matches!(*left, AssertionExpr::Atom(Expr::JqPath(_))));
            assert!(matches!(
                *right,
                AssertionExpr::Atom(Expr::Literal(Literal::Number(_)))
            ));
        } else {
            panic!("Expected Binary, got: {:?}", expr);
        }
    }

    #[test]
    fn test_parse_plugin_call() {
        let expr = parse_assertion("@uuid(.user_id) == true");
        if let AssertionExpr::Binary { op, left, .. } = expr {
            assert_eq!(op, BinaryOp::Eq);
            if let AssertionExpr::Atom(Expr::PluginCall { name, args }) = &*left {
                assert_eq!(name, "uuid");
                assert_eq!(args.len(), 1);
            } else {
                panic!("Expected PluginCall");
            }
        } else {
            panic!("Expected Binary, got: {:?}", expr);
        }
    }

    #[test]
    fn test_parse_negation_bang() {
        let expr = parse_assertion("!@has_header(\"x\")");
        if let AssertionExpr::Not(inner) = expr {
            if let AssertionExpr::Atom(Expr::PluginCall { name, .. }) = &*inner {
                assert_eq!(name, "has_header");
            } else {
                panic!("Expected PluginCall inside Not");
            }
        } else {
            panic!("Expected Not, got: {:?}", expr);
        }
    }

    #[test]
    fn test_parse_pipe_not() {
        let expr = parse_assertion("@empty(.id) | not");
        if let AssertionExpr::Not(inner) = expr {
            if let AssertionExpr::Atom(Expr::PluginCall { name, .. }) = &*inner {
                assert_eq!(name, "empty");
            } else {
                panic!("Expected PluginCall inside Not, got: {:?}", inner);
            }
        } else {
            panic!("Expected Not, got: {:?}", expr);
        }
    }

    #[test]
    fn test_parse_pipe_not_not() {
        let expr = parse_assertion("@empty(.id) | not not");
        if let AssertionExpr::Atom(Expr::PluginCall { name, .. }) = expr {
            assert_eq!(name, "empty");
        } else {
            panic!(
                "Expected bare PluginCall (double negation cancels), got: {:?}",
                expr
            );
        }
    }

    #[test]
    fn test_parse_negation_not_keyword() {
        let expr = parse_assertion("not @empty(.id)");
        if let AssertionExpr::Not(inner) = expr {
            if let AssertionExpr::Atom(Expr::PluginCall { name, .. }) = &*inner {
                assert_eq!(name, "empty");
            } else {
                panic!("Expected PluginCall");
            }
        } else {
            panic!("Expected Not, got: {:?}", expr);
        }
    }

    #[test]
    fn test_parse_xor() {
        let expr = parse_assertion("@uuid(.id) xor @email(.name)");
        if let AssertionExpr::Xor { left, right, .. } = expr {
            assert!(matches!(
                *left,
                AssertionExpr::Atom(Expr::PluginCall { .. })
            ));
            assert!(matches!(
                *right,
                AssertionExpr::Atom(Expr::PluginCall { .. })
            ));
        } else {
            panic!("Expected Xor, got: {:?}", expr);
        }
    }

    #[test]
    fn test_parse_or() {
        let expr = parse_assertion("@uuid(.id) or @email(.name)");
        assert!(
            matches!(expr, AssertionExpr::Or { .. }),
            "Expected Or, got: {:?}",
            expr
        );
    }

    #[test]
    fn test_parse_and_or_xor_precedence() {
        let expr = parse_assertion("@a or @b xor @c and @d");
        assert!(
            matches!(expr, AssertionExpr::Or { .. }),
            "Expected Or, got: {:?}",
            expr
        );
    }

    #[test]
    fn test_parse_paren_or_in_and() {
        let expr = parse_assertion("(@a or @b) and @c");
        if let AssertionExpr::And { left, .. } = expr {
            assert!(
                matches!(*left, AssertionExpr::Paren(_)),
                "Left should be Paren(Or)"
            );
        } else {
            panic!("Expected And, got: {:?}", expr);
        }
    }

    #[test]
    fn test_parse_negated_paren_or() {
        let expr = parse_assertion("!(@empty(.id) or @uuid(.id))");
        if let AssertionExpr::Not(inner) = expr {
            if let AssertionExpr::Paren(or_expr) = &*inner {
                assert!(matches!(**or_expr, AssertionExpr::Or { .. }));
            } else {
                panic!("Expected Paren(Or), got: {:?}", inner);
            }
        } else {
            panic!("Expected Not, got: {:?}", expr);
        }
    }

    #[test]
    fn test_roundtrip_xor() {
        let expr = parse_assertion("@a() xor @b()");
        let s = assertion_to_string(&expr);
        assert!(s.contains(" xor "), "Should contain xor: {}", s);
    }

    #[test]
    fn test_roundtrip_pipe_not() {
        let expr = parse_assertion("@empty(.id) | not");
        let s = assertion_to_string(&expr);
        assert!(s.contains('!'), "Pipe not should serialize as !: {}", s);
    }

    #[test]
    fn test_contains() {
        let expr = parse_assertion(".name contains \"test\"");
        if let AssertionExpr::Binary { op, .. } = expr {
            assert_eq!(op, BinaryOp::Contains);
        } else {
            panic!("Expected Binary");
        }
    }

    #[test]
    fn test_startswith() {
        let expr = parse_assertion(".name startsWith \"te\"");
        if let AssertionExpr::Binary { op, .. } = expr {
            assert_eq!(op, BinaryOp::StartsWith);
        } else {
            panic!("Expected Binary");
        }
    }

    #[test]
    fn test_matches() {
        let expr = parse_assertion(".name matches \"^te.*t$\"");
        if let AssertionExpr::Binary { op, .. } = expr {
            assert_eq!(op, BinaryOp::Matches);
        } else {
            panic!("Expected Binary");
        }
    }

    #[test]
    fn test_if_then_else() {
        let expr = parse_assertion("if @len(.items) == 0 then true else false end");
        if let AssertionExpr::IfThenElse {
            condition,
            then_branch,
            else_branch,
        } = expr
        {
            assert!(matches!(*condition, AssertionExpr::Binary { .. }));
            assert!(matches!(
                *then_branch,
                AssertionExpr::Atom(Expr::Literal(Literal::Bool(true)))
            ));
            assert!(matches!(
                *else_branch,
                AssertionExpr::Atom(Expr::Literal(Literal::Bool(false)))
            ));
        } else {
            panic!("Expected IfThenElse");
        }
    }

    #[test]
    fn test_if_then_else_serializes_as_ternary() {
        let expr = parse_assertion("if .x == 0 then true else false end");
        let s = assertion_to_string(&expr);
        assert!(s.contains('?'), "Should contain ternary: {}", s);
        assert!(s.contains(':'), "Should contain colon: {}", s);
    }

    #[test]
    fn test_nested_if_serializes_correctly() {
        let expr =
            parse_assertion("if .a == 1 then if .b == 2 then \"A\" else \"B\" end else \"C\" end");
        if let AssertionExpr::IfThenElse {
            then_branch,
            else_branch,
            ..
        } = &expr
        {
            assert!(matches!(**then_branch, AssertionExpr::IfThenElse { .. }));
            assert!(matches!(
                **else_branch,
                AssertionExpr::Atom(Expr::Literal(Literal::Str(_)))
            ));
        } else {
            panic!("Expected IfThenElse");
        }
        let s = assertion_to_string(&expr);
        assert!(s.contains('('), "Nested ternary should have parens: {}", s);
    }

    #[test]
    fn test_remove_redundant_parens() {
        let expr = parse_assertion("((.x == 1))");
        let simplified = remove_redundant_parens(&expr);
        let s = assertion_to_string(&simplified);
        assert!(!s.starts_with("(("), "Should not have double parens: {}", s);
    }

    #[test]
    fn test_roundtrip_simple() {
        let original = ".id == 123";
        let expr = parse_assertion(original);
        assert_eq!(assertion_to_string(&expr), original);
    }

    #[test]
    fn test_roundtrip_with_plugin() {
        let original = "@len(.items) == 0";
        let expr = parse_assertion(original);
        assert_eq!(assertion_to_string(&expr), original);
    }

    #[test]
    fn test_parse_and_or() {
        assert!(matches!(
            parse_assertion(".x == 1 and .y == 2"),
            AssertionExpr::And { .. }
        ));
        assert!(matches!(
            parse_assertion(".x == 1 or .y == 2"),
            AssertionExpr::Or { .. }
        ));
    }

    #[test]
    fn test_parse_not_not() {
        if let AssertionExpr::NotNot(inner) = parse_assertion("not not .x") {
            assert!(matches!(*inner, AssertionExpr::Atom(Expr::JqPath(_))));
        } else {
            panic!("Expected NotNot");
        }
    }

    #[test]
    fn test_parse_double_bang() {
        if let AssertionExpr::NotNot(inner) = parse_assertion("!!.x") {
            assert!(matches!(*inner, AssertionExpr::Atom(Expr::JqPath(_))));
        } else {
            panic!("Expected NotNot");
        }
    }

    #[test]
    fn test_parse_regex_literal() {
        let expr = parse_assertion("@regex(.name, /^hello/i) == true");
        if let AssertionExpr::Binary { op, left, .. } = expr {
            assert_eq!(op, BinaryOp::Eq);
            if let AssertionExpr::Atom(Expr::PluginCall { name, args }) = &*left {
                assert_eq!(name, "regex");
                assert_eq!(args.len(), 2);
                if let AssertionExpr::Atom(a) = &args[1] {
                    if let Expr::RegExp { pattern, flags } = &a {
                        assert_eq!(pattern, "^hello");
                        assert_eq!(flags, "");
                    } else {
                        panic!("Expected RegExp");
                    }
                } else {
                    panic!("Expected Atom");
                }
            } else {
                panic!("Expected PluginCall");
            }
        } else {
            panic!("Expected Binary");
        }
    }

    #[test]
    fn test_parse_regex_serializes_correctly() {
        let expr = parse_assertion("@regex(.x, /\\d{4}/gi) == true");
        let s = assertion_to_string(&expr);
        assert!(s.contains("/\\d{4}/"), "Should contain regex: {}", s);
    }

    #[test]
    fn test_parse_json_literal() {
        let expr = parse_assertion("@json(.data) == {\"key\": \"value\"}");
        if let AssertionExpr::Binary { op, right, .. } = expr {
            assert_eq!(op, BinaryOp::Eq);
            if let AssertionExpr::Atom(Expr::Json(s)) = &*right {
                assert!(s.contains("\"key\""));
                assert!(s.contains("\"value\""));
            } else {
                panic!("Expected Json");
            }
        } else {
            panic!("Expected Binary");
        }
    }

    #[test]
    fn test_nested_ternary_roundtrip() {
        let original = "if .x == 1 then if .y == 2 then true else false end else false end";
        let expr = parse_assertion(original);
        let s = assertion_to_string(&expr);
        assert!(s.contains('?'), "Should contain ternary: {}", s);
        assert!(s.contains('('), "Should contain parens: {}", s);
    }
}
