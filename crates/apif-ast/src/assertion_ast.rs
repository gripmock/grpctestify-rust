use serde::{Deserialize, Serialize};

use crate::tokenizer::{TokenKind, tokenize_assertion};

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
    As(Box<Expr>, String),
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
            Self::Literal(Literal::Str(s)) => {
                let mut buf = String::new();
                write_escaped_string_literal(&mut buf, s);
                write!(f, "{buf}")
            }
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
            Self::Variable(n) => write!(f, "${}", n),
            Self::As(inner, type_name) => write!(f, "{}:{}", inner, type_name),
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

const MAX_PARSE_DEPTH: usize = 256;

pub fn dangling_operator(raw: &str) -> Option<String> {
    let tokens = crate::tokenizer::tokenize_assertion(raw);
    let last = tokens.last()?;
    let spelled = match &last.kind {
        crate::tokenizer::TokenKind::Op(s) => s.clone(),
        crate::tokenizer::TokenKind::Ident(s) if is_bin_op_keyword(s) => s.clone(),
        crate::tokenizer::TokenKind::Pipe => return Some("|".to_string()),
        _ => return None,
    };
    BinaryOp::try_parse(&spelled).map(|_| spelled)
}

pub fn reads_nothing(raw: &str) -> bool {
    use crate::tokenizer::TokenKind;
    let tokens = crate::tokenizer::tokenize_assertion(raw);
    if tokens.is_empty() {
        return false;
    }
    !tokens.iter().any(|t| match &t.kind {
        TokenKind::Dot | TokenKind::At | TokenKind::VarDelim | TokenKind::LBracket => true,
        TokenKind::Ident(name) => !matches!(
            name.as_str(),
            "true" | "false" | "null" | "and" | "or" | "not" | "xor"
        ),
        _ => false,
    })
}

pub fn parse_assertion(raw: &str) -> AssertionExpr {
    let tokens = tokenize_assertion(raw);
    if tokens.is_empty() {
        return AssertionExpr::Raw(raw.to_string());
    }
    let mut pos = 0;
    let expr = parse_pipe(&tokens, &mut pos, 0);
    if pos >= tokens.len() {
        expr
    } else {
        AssertionExpr::Raw(raw.to_string())
    }
}

fn parse_pipe(ts: &[crate::tokenizer::Token], p: &mut usize, d: usize) -> AssertionExpr {
    if d > MAX_PARSE_DEPTH {
        return AssertionExpr::Raw(String::new());
    }
    let mut expr = parse_or(ts, p, d);
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

fn parse_or(ts: &[crate::tokenizer::Token], p: &mut usize, d: usize) -> AssertionExpr {
    let mut left = parse_xor(ts, p, d);
    while *p < ts.len() && is_keyword(ts, *p, "or") {
        *p += 1;
        let right = parse_xor(ts, p, d);
        left = AssertionExpr::Or {
            left: Box::new(left),
            right: Box::new(right),
        };
    }
    left
}

fn parse_xor(ts: &[crate::tokenizer::Token], p: &mut usize, d: usize) -> AssertionExpr {
    let mut left = parse_and(ts, p, d);
    while *p < ts.len() && is_keyword(ts, *p, "xor") {
        *p += 1;
        let right = parse_and(ts, p, d);
        left = AssertionExpr::Xor {
            left: Box::new(left),
            right: Box::new(right),
        };
    }
    left
}

fn parse_and(ts: &[crate::tokenizer::Token], p: &mut usize, d: usize) -> AssertionExpr {
    let mut left = parse_bin(ts, p, d);
    while *p < ts.len() && is_keyword(ts, *p, "and") {
        *p += 1;
        let right = parse_bin(ts, p, d);
        left = AssertionExpr::And {
            left: Box::new(left),
            right: Box::new(right),
        };
    }
    left
}

fn parse_bin(ts: &[crate::tokenizer::Token], p: &mut usize, d: usize) -> AssertionExpr {
    let mut left = parse_unary(ts, p, d);
    loop {
        let op = match ts.get(*p).map(|t| &t.kind) {
            Some(TokenKind::Op(s)) => match BinaryOp::try_parse(s) {
                Some(op) => op,
                None => break,
            },
            Some(TokenKind::Ident(s)) if is_bin_op_keyword(s) => match BinaryOp::try_parse(s) {
                Some(op) => op,
                None => break,
            },
            None => break,
            _ => break,
        };

        if *p + 1 >= ts.len() {
            break;
        }
        *p += 1;
        let right = parse_unary(ts, p, d);
        left = AssertionExpr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        };
    }
    left
}

fn parse_unary(ts: &[crate::tokenizer::Token], p: &mut usize, d: usize) -> AssertionExpr {
    if d > MAX_PARSE_DEPTH {
        return AssertionExpr::Raw(String::new());
    }
    if *p >= ts.len() {
        return AssertionExpr::Raw(String::new());
    }
    match &ts[*p].kind {
        TokenKind::Bang => {
            *p += 1;
            if *p < ts.len() && matches!(ts[*p].kind, TokenKind::Bang) {
                *p += 1;
                let inner = parse_unary(ts, p, d + 1);
                AssertionExpr::NotNot(Box::new(inner))
            } else {
                let inner = parse_unary(ts, p, d + 1);
                AssertionExpr::Not(Box::new(inner))
            }
        }
        TokenKind::Ident(s) if s == "not" => {
            *p += 1;
            if *p < ts.len() && matches!(ts[*p].kind, TokenKind::Ident(ref s2) if s2 == "not") {
                *p += 1;
                let inner = parse_unary(ts, p, d + 1);
                AssertionExpr::NotNot(Box::new(inner))
            } else {
                let inner = parse_unary(ts, p, d + 1);
                AssertionExpr::Not(Box::new(inner))
            }
        }
        TokenKind::Ident(s) if s == "if" => parse_if(ts, p, d),
        TokenKind::LParen => {
            *p += 1;
            let inner = parse_pipe(ts, p, d + 1);
            if *p < ts.len() && matches!(ts[*p].kind, TokenKind::RParen) {
                *p += 1;
            }
            AssertionExpr::Paren(Box::new(inner))
        }
        _ => parse_atom(ts, p, d),
    }
}

fn parse_if(ts: &[crate::tokenizer::Token], p: &mut usize, d: usize) -> AssertionExpr {
    *p += 1;
    let cond = parse_pipe(ts, p, d + 1);
    if *p >= ts.len() || !is_keyword(ts, *p, "then") {
        return AssertionExpr::Raw("if..then missing".into());
    }
    *p += 1;
    let then_b = parse_pipe(ts, p, d + 1);
    if *p >= ts.len() || !is_keyword(ts, *p, "else") {
        return AssertionExpr::Raw("if..else missing".into());
    }
    *p += 1;
    let else_b = parse_pipe(ts, p, d + 1);
    if *p < ts.len() && is_keyword(ts, *p, "end") {
        *p += 1;
    }
    AssertionExpr::IfThenElse {
        condition: Box::new(cond),
        then_branch: Box::new(then_b),
        else_branch: Box::new(else_b),
    }
}

fn parse_atom(ts: &[crate::tokenizer::Token], p: &mut usize, d: usize) -> AssertionExpr {
    if *p >= ts.len() {
        return AssertionExpr::Atom(Expr::JqPath(String::new()));
    }
    let mut expr = match &ts[*p].kind {
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
            while *p < ts.len() && !matches!(ts[*p].kind, TokenKind::VarDelim) {
                *p += 1;
            }
            if *p < ts.len() {
                *p += 1;
            }
            Expr::JqPath(String::new())
        }
        TokenKind::At => {
            *p += 1;
            let name = if *p < ts.len() {
                if let TokenKind::Ident(s) = &ts[*p].kind {
                    *p += 1;
                    let mut name = s.clone();
                    if *p + 1 < ts.len()
                        && matches!(&ts[*p].kind, TokenKind::Dot)
                        && matches!(&ts[*p + 1].kind, TokenKind::Ident(_))
                    {
                        *p += 1;
                        if let TokenKind::Ident(method) = &ts[*p].kind {
                            name.push('.');
                            name.push_str(method);
                            *p += 1;
                        }
                    }
                    name
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
                    let arg = parse_pipe(ts, p, d + 1);
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
        TokenKind::Op(op) if op == "-" => {
            *p += 1;
            if *p < ts.len()
                && let TokenKind::NumberLit(n) = &ts[*p].kind
            {
                let neg = format!("-{}", n);
                *p += 1;
                Expr::Literal(Literal::Number(neg))
            } else {
                Expr::JqPath("-".to_string())
            }
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
                    TokenKind::Colon => {
                        json.push(':');
                        *p += 1;
                    }
                    TokenKind::VarDelim => {
                        json.push_str("{{ ");
                        *p += 1;
                        while *p < ts.len() && !matches!(ts[*p].kind, TokenKind::VarDelim) {
                            if let TokenKind::Ident(s) = &ts[*p].kind {
                                json.push_str(s);
                            }
                            *p += 1;
                        }
                        if *p < ts.len() {
                            json.push_str(" }}");
                            *p += 1;
                        } else {
                            json.push_str(" }}");
                        }
                    }
                    _ => {
                        *p += 1;
                    }
                }
            }
            json.push('}');
            Expr::Json(json)
        }
        TokenKind::Ident(s) if s.starts_with('$') => {
            *p += 1;
            Expr::Variable(s[1..].to_string())
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
                    TokenKind::StringLit(s) => {
                        path.push('.');
                        path.push('"');
                        path.push_str(s);
                        path.push('"');
                        *p += 1;
                    }
                    TokenKind::Op(op) if op == "-" || op == ":" => {
                        path.push_str(op);
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
                            } else if let TokenKind::StringLit(s) = &ts[*p].kind {
                                path.push('"');
                                path.push_str(s);
                                path.push('"');
                            } else if let TokenKind::Op(op) = &ts[*p].kind
                                && (op == ":" || op == "-")
                            {
                                path.push_str(op);
                            } else if let TokenKind::Dot = &ts[*p].kind {
                                path.push('.');
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

    if *p + 1 < ts.len()
        && matches!(&ts[*p].kind, TokenKind::Colon)
        && let TokenKind::Ident(type_name) = &ts[*p + 1].kind
    {
        let name = type_name.clone();
        *p += 2;
        expr = Expr::As(Box::new(expr), name);
    }

    AssertionExpr::Atom(expr)
}

fn is_keyword(ts: &[crate::tokenizer::Token], idx: usize, kw: &str) -> bool {
    matches!(ts.get(idx), Some(t) if matches!(&t.kind, TokenKind::Ident(s) if s == kw))
}

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

pub fn write_escaped_string_literal(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\u{b}' => out.push_str("\\v"),
            '\0' => out.push_str("\\0"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
}

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
        Expr::Literal(Literal::Str(s)) => write_escaped_string_literal(out, s),
        Expr::Literal(Literal::Null) => out.push_str("null"),
        Expr::Variable(n) => {
            out.push('$');
            out.push_str(n);
        }
        Expr::RegExp { pattern, flags } => {
            out.push('/');
            out.push_str(pattern);
            out.push('/');
            out.push_str(flags);
        }
        Expr::Json(s) | Expr::Yaml(s) => out.push_str(s),
        Expr::As(inner, type_name) => {
            push_expr(inner, out);
            out.push(':');
            out.push_str(type_name);
        }
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
            out.push_str("if ");
            push_assertion(condition, out, 0);
            out.push_str(" then ");
            push_assertion(then_branch, out, 0);
            out.push_str(" else ");
            push_assertion(else_branch, out, 0);
            out.push_str(" end");
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
            let ast = parse_pipe(&tokens, &mut pos, 0);
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
            let ast = parse_pipe(&tokens, &mut pos, 0);
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
    fn parse_simple_equality() {
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
    fn parse_plugin_call() {
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
    fn parse_negation_bang() {
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
    fn parse_pipe_not() {
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
    fn parse_pipe_not_not() {
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
    fn parse_negation_not_keyword() {
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
    fn parse_and_or_xor_precedence() {
        let expr = parse_assertion("@a or @b xor @c and @d");
        assert!(
            matches!(expr, AssertionExpr::Or { .. }),
            "Expected Or, got: {:?}",
            expr
        );
    }

    #[test]
    fn parse_paren_or_in_and() {
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
    fn parse_negated_paren_or() {
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
    fn roundtrip_xor() {
        let expr = parse_assertion("@a() xor @b()");
        let s = assertion_to_string(&expr);
        assert!(s.contains(" xor "), "Should contain xor: {}", s);
    }

    #[test]
    fn roundtrip_pipe_not() {
        let expr = parse_assertion("@empty(.id) | not");
        let s = assertion_to_string(&expr);
        assert!(s.contains('!'), "Pipe not should serialize as !: {}", s);
    }

    #[test]
    fn contains() {
        let expr = parse_assertion(".name contains \"test\"");
        if let AssertionExpr::Binary { op, .. } = expr {
            assert_eq!(op, BinaryOp::Contains);
        } else {
            panic!("Expected Binary");
        }
    }

    #[test]
    fn startswith() {
        let expr = parse_assertion(".name startsWith \"te\"");
        if let AssertionExpr::Binary { op, .. } = expr {
            assert_eq!(op, BinaryOp::StartsWith);
        } else {
            panic!("Expected Binary");
        }
    }

    #[test]
    fn matches() {
        let expr = parse_assertion(".name matches \"^te.*t$\"");
        if let AssertionExpr::Binary { op, .. } = expr {
            assert_eq!(op, BinaryOp::Matches);
        } else {
            panic!("Expected Binary");
        }
    }

    #[test]
    fn if_then_else() {
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
    fn if_then_else_roundtrips() {
        let original = "if .x == 0 then true else false end";
        let expr = parse_assertion(original);
        let s = assertion_to_string(&expr);
        assert_eq!(s, original);
        assert!(matches!(
            parse_assertion(&s),
            AssertionExpr::IfThenElse { .. }
        ));
    }

    #[test]
    fn nested_if_serializes_correctly() {
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
        assert!(
            matches!(parse_assertion(&s), AssertionExpr::IfThenElse { .. }),
            "Nested if should re-parse to IfThenElse: {}",
            s
        );
    }

    #[test]
    fn test_remove_redundant_parens() {
        let expr = parse_assertion("((.x == 1))");
        let simplified = remove_redundant_parens(&expr);
        let s = assertion_to_string(&simplified);
        assert!(!s.starts_with("(("), "Should not have double parens: {}", s);
    }

    #[test]
    fn roundtrip_simple() {
        let original = ".id == 123";
        let expr = parse_assertion(original);
        assert_eq!(assertion_to_string(&expr), original);
    }

    #[test]
    fn roundtrip_with_plugin() {
        let original = "@len(.items) == 0";
        let expr = parse_assertion(original);
        assert_eq!(assertion_to_string(&expr), original);
    }

    #[test]
    fn parse_and_or() {
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
    fn parse_not_not() {
        if let AssertionExpr::NotNot(inner) = parse_assertion("not not .x") {
            assert!(matches!(*inner, AssertionExpr::Atom(Expr::JqPath(_))));
        } else {
            panic!("Expected NotNot");
        }
    }

    #[test]
    fn parse_double_bang() {
        if let AssertionExpr::NotNot(inner) = parse_assertion("!!.x") {
            assert!(matches!(*inner, AssertionExpr::Atom(Expr::JqPath(_))));
        } else {
            panic!("Expected NotNot");
        }
    }

    #[test]
    fn parse_regex_literal() {
        let expr = parse_assertion("@regex(.name, /^hello/i) == true");
        if let AssertionExpr::Binary { op, left, .. } = expr {
            assert_eq!(op, BinaryOp::Eq);
            if let AssertionExpr::Atom(Expr::PluginCall { name, args }) = &*left {
                assert_eq!(name, "regex");
                assert_eq!(args.len(), 2);
                if let AssertionExpr::Atom(a) = &args[1] {
                    if let Expr::RegExp { pattern, flags } = &a {
                        assert_eq!(pattern, "^hello");
                        assert_eq!(flags, "i");
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
    fn parse_trailing_colon_no_panic() {
        let expr = parse_assertion(".x:");
        assert_eq!(expr, AssertionExpr::Raw(".x:".to_string()));
    }

    #[test]
    fn parse_unknown_operator_falls_back_to_raw() {
        assert_eq!(
            parse_assertion("@len(.x) - 1 == 0"),
            AssertionExpr::Raw("@len(.x) - 1 == 0".to_string())
        );
        assert_eq!(
            parse_assertion("\"abc\" - \"c\""),
            AssertionExpr::Raw("\"abc\" - \"c\"".to_string())
        );
    }

    #[test]
    fn parse_lone_equals_falls_back_to_raw() {
        assert_eq!(
            parse_assertion(".x = 5"),
            AssertionExpr::Raw(".x = 5".to_string())
        );
    }

    #[test]
    fn parse_known_binary_operators_still_parse() {
        for (src, op) in [
            (".x == 1", BinaryOp::Eq),
            (".x != 1", BinaryOp::Ne),
            (".x > 1", BinaryOp::Gt),
            (".x < 1", BinaryOp::Lt),
            (".x >= 1", BinaryOp::Ge),
            (".x <= 1", BinaryOp::Le),
            (".x contains \"a\"", BinaryOp::Contains),
            (".x matches \"^a\"", BinaryOp::Matches),
            (".x startsWith \"a\"", BinaryOp::StartsWith),
            (".x endsWith \"a\"", BinaryOp::EndsWith),
        ] {
            match parse_assertion(src) {
                AssertionExpr::Binary { op: parsed, .. } => assert_eq!(parsed, op, "{}", src),
                other => panic!("Expected Binary for {}, got: {:?}", src, other),
            }
        }
    }

    #[test]
    fn parse_regex_serializes_correctly() {
        let expr = parse_assertion("@regex(.x, /\\d{4}/gi) == true");
        let s = assertion_to_string(&expr);
        assert!(s.contains("/\\d{4}/"), "Should contain regex: {}", s);
    }

    #[test]
    fn parse_json_literal() {
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
    fn nested_ternary_roundtrip() {
        let original = "if .x == 1 then if .y == 2 then true else false end else false end";
        let expr = parse_assertion(original);
        let s = assertion_to_string(&expr);
        assert_eq!(s, original, "nested if must round-trip");
        assert!(matches!(
            parse_assertion(&s),
            AssertionExpr::IfThenElse { .. }
        ));
    }

    #[test]
    fn parse_type_cast_number() {
        let expr = parse_assertion(".price:number >= 0");
        if let AssertionExpr::Binary { op, left, right } = expr {
            assert_eq!(op, BinaryOp::Ge);
            assert!(matches!(&*left, AssertionExpr::Atom(Expr::As(_, tn)) if tn == "number"));
            if let AssertionExpr::Atom(Expr::As(inner, tn)) = &*left {
                assert_eq!(tn, "number");
                assert!(matches!(&**inner, Expr::JqPath(p) if p == ".price"));
            }
            assert!(
                matches!(&*right, AssertionExpr::Atom(Expr::Literal(Literal::Number(n))) if n == "0")
            );
        } else {
            panic!("Expected Binary, got: {:?}", expr);
        }
    }

    #[test]
    fn parse_type_cast_string() {
        let expr = parse_assertion(".name:string contains \"hello\"");
        assert!(matches!(
            expr,
            AssertionExpr::Binary {
                op: BinaryOp::Contains,
                ..
            }
        ));
    }

    #[test]
    fn parse_type_cast_uint() {
        let expr = parse_assertion("@len(.items):uint > 0");
        if let AssertionExpr::Binary { op, left, .. } = expr {
            assert_eq!(op, BinaryOp::Gt);
            assert!(matches!(&*left, AssertionExpr::Atom(Expr::As(_, tn)) if tn == "uint"));
        } else {
            panic!("Expected Binary, got: {:?}", expr);
        }
    }

    #[test]
    fn parse_type_cast_bool() {
        let expr = parse_assertion(".active:bool == true");
        if let AssertionExpr::Binary { op, left, right } = expr {
            assert_eq!(op, BinaryOp::Eq);
            assert!(matches!(&*left, AssertionExpr::Atom(Expr::As(_, tn)) if tn == "bool"));
            assert!(matches!(
                &*right,
                AssertionExpr::Atom(Expr::Literal(Literal::Bool(true)))
            ));
        } else {
            panic!("Expected Binary, got: {:?}", expr);
        }
    }

    #[test]
    fn parse_type_cast_all_types_parsed() {
        let types = [
            "bool", "uint", "number", "string", "json", "yaml", "uuid", "email", "url", "ip",
        ];
        for type_name in &types {
            let raw = format!(".x:{} == 0", type_name);
            let expr = parse_assertion(&raw);
            assert!(
                matches!(&expr, AssertionExpr::Binary { left, .. }
                    if matches!(&**left, AssertionExpr::Atom(Expr::As(_, tn)) if tn == type_name)),
                "Failed for type: {}",
                type_name
            );
        }
    }

    #[test]
    fn parse_type_cast_roundtrip() {
        let original = ".price:number >= 0";
        let expr = parse_assertion(original);
        let s = assertion_to_string(&expr);
        assert_eq!(s, original);
    }

    #[test]
    fn parse_type_cast_compound() {
        let expr =
            parse_assertion(r#".ips_to_decorations["10.0.0.1"].environment == "production""#);
        assert_eq!(
            assertion_to_string(&expr),
            r#".ips_to_decorations["10.0.0.1"].environment == "production""#
        );

        let expr = parse_assertion(".items:json == {\"key\": \"value\"}");
        if let AssertionExpr::Binary { op, left, right } = expr {
            assert_eq!(op, BinaryOp::Eq);
            assert!(matches!(&*left, AssertionExpr::Atom(Expr::As(_, tn)) if tn == "json"));
            assert!(matches!(&*right, AssertionExpr::Atom(Expr::Json(_))));
        } else {
            panic!("Expected Binary, got: {:?}", expr);
        }
    }

    #[test]
    fn parse_type_cast_jq_path_preserved() {
        let expr = parse_assertion(".user.name:string == \"alice\"");
        if let AssertionExpr::Binary { left, .. } = &expr {
            if let AssertionExpr::Atom(Expr::As(inner, tn)) = &**left {
                assert_eq!(tn, "string");
                assert!(matches!(&**inner, Expr::JqPath(p) if p == ".user.name"));
            } else {
                panic!("Expected As, got: {:?}", left);
            }
        } else {
            panic!("Expected Binary, got: {:?}", expr);
        }
    }

    #[test]
    fn parse_type_cast_assertion_to_string_preserves_cast() {
        let cases = [
            ".x:number > 0",
            ".x:string == \"hello\"",
            ".x:bool == true",
            ".x:uint >= 5",
        ];
        for original in &cases {
            let expr = parse_assertion(original);
            let s = assertion_to_string(&expr);
            assert_eq!(s.as_str(), *original, "Roundtrip failed for: {}", original);
        }
    }

    #[test]
    fn bracket_with_template_string() {
        let expr = parse_assertion(r#".x["{{var}}"] == "val""#);
        if let AssertionExpr::Binary { left, .. } = &expr {
            if let AssertionExpr::Atom(Expr::JqPath(p)) = &**left {
                assert_eq!(p, r#".x["{{var}}"]"#);
            } else {
                panic!("Expected JqPath, got: {:?}", left);
            }
        } else {
            panic!("Expected Binary, got: {:?}", expr);
        }
        let s = assertion_to_string(&expr);
        assert_eq!(s, r#".x["{{var}}"] == "val""#);
    }

    #[test]
    fn bracket_with_var_index() {
        let expr = parse_assertion(r#".x[.idx] == 0"#);
        if let AssertionExpr::Binary { left, .. } = &expr {
            if let AssertionExpr::Atom(Expr::JqPath(p)) = &**left {
                assert_eq!(p, ".x[.idx]", "path mismatch, full expr: {:?}", expr);
            } else {
                panic!("Expected JqPath, got: {:?}", left);
            }
        } else {
            panic!("Expected Binary, got: {:?}", expr);
        }
        let s = assertion_to_string(&expr);
        assert_eq!(s, ".x[.idx] == 0");
    }

    #[test]
    fn type_method_syntax() {
        let expr = parse_assertion(r#"@url.scheme(.webhook) == "https""#);
        if let AssertionExpr::Binary { left, .. } = &expr {
            if let AssertionExpr::Atom(Expr::PluginCall { name, args }) = &**left {
                assert_eq!(name, "url.scheme");
                assert_eq!(args.len(), 1);
            } else {
                panic!("Expected PluginCall, got: {:?}", left);
            }
        } else {
            panic!("Expected Binary, got: {:?}", expr);
        }
        let s = assertion_to_string(&expr);
        assert_eq!(s, r#"@url.scheme(.webhook) == "https""#);
    }

    #[test]
    fn type_method_roundtrip() {
        let cases = [
            "@email.domain(.x) == \"example.com\"",
            "@json.key(.config, \"timeout\") == 30",
            "@regexp.test(.pattern, \"input\")",
        ];
        for original in &cases {
            let expr = parse_assertion(original);
            let s = assertion_to_string(&expr);
            assert_eq!(s.as_str(), *original, "Roundtrip failed for: {}", original);
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn deeply_nested_parens_does_not_overflow() {
        let src = "(".repeat(100_000);
        let expr = parse_assertion(&src);
        assert_eq!(expr, AssertionExpr::Raw(src));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn deeply_nested_bang_does_not_overflow() {
        let src = "!".repeat(100_000);
        let expr = parse_assertion(&src);
        assert_eq!(expr, AssertionExpr::Raw(src));
    }

    #[test]
    fn moderate_paren_nesting_still_parses() {
        let src = format!("{}.x == 1{}", "(".repeat(20), ")".repeat(20));
        let expr = parse_assertion(&src);
        assert!(
            !matches!(expr, AssertionExpr::Raw(_)),
            "expected structured parse, got Raw: {:?}",
            expr
        );
    }

    #[test]
    fn parse_type_cast_with_plugin() {
        let expr = parse_assertion("@len(.items):number >= 0");
        if let AssertionExpr::Binary { left, .. } = &expr {
            if let AssertionExpr::Atom(Expr::As(inner, tn)) = &**left {
                assert_eq!(tn, "number");
                assert!(matches!(&**inner, Expr::PluginCall { name, .. } if name == "len"));
            } else {
                panic!("Expected As(PluginCall), got: {:?}", left);
            }
        } else {
            panic!("Expected Binary, got: {:?}", expr);
        }
    }

    #[test]
    fn string_literals_round_trip_losslessly() {
        for src in [
            r#".msg == "a\nb""#,
            r#".p == "a\\b""#,
            r#".q == "tab\there""#,
            r#".m == "say \"hi\"""#,
            r#".plain == "abc""#,
        ] {
            let once = assertion_to_string(&parse_assertion(src));
            let twice = assertion_to_string(&parse_assertion(&once));
            assert_eq!(once, twice, "unstable render for {src}");
        }
    }

    #[test]
    fn escapes_decode_to_the_characters_they_denote() {
        let AssertionExpr::Binary { right, .. } = parse_assertion(r#".msg == "a\nb""#) else {
            panic!("expected a comparison");
        };
        let AssertionExpr::Atom(Expr::Literal(Literal::Str(value))) = &*right else {
            panic!("expected a string literal");
        };
        assert_eq!(value, "a\nb", "an escaped newline must be a real newline");
    }

    #[test]
    fn escapes_decode_to_the_same_values_json5_produces() {
        for (src, expected) in [
            (r#".a == "caf\u00e9""#, "café"),
            (r#".a == "x\bY""#, "x\u{8}Y"),
            (r#".a == "x\fY""#, "x\u{c}Y"),
            (r#".a == "x\vY""#, "x\u{b}Y"),
            (r#".a == "x\x41Y""#, "xAY"),
            (r#".a == "a\\b""#, "a\\b"),
            (r#".a == "tab\there""#, "tab\there"),
        ] {
            let AssertionExpr::Binary { right, .. } = parse_assertion(src) else {
                panic!("expected a comparison for {src}");
            };
            let AssertionExpr::Atom(Expr::Literal(Literal::Str(v))) = &*right else {
                panic!("expected a string literal for {src}");
            };
            assert_eq!(v, expected, "decoding {src}");
        }
    }

    #[test]
    fn every_decoded_string_re_renders_to_the_same_value() {
        for src in [
            r#".a == "caf\u00e9""#,
            r#".a == "x\bY""#,
            r#".a == "x\fY""#,
            r#".a == "a\\b""#,
            r#".a == "tab\there""#,
            r#".a == "say \"hi\"""#,
            r#".a == "nul\0end""#,
        ] {
            let first = assertion_to_string(&parse_assertion(src));
            let second = assertion_to_string(&parse_assertion(&first));
            assert_eq!(first, second, "unstable render for {src}");
        }
    }

    #[test]
    fn grouped_number_literals_parse_as_numbers() {
        let AssertionExpr::Binary { right, .. } = parse_assertion(".amount == 1_000_000") else {
            panic!("expected a comparison, got Raw — the separator broke the lexer");
        };
        let AssertionExpr::Atom(Expr::Literal(Literal::Number(n))) = &*right else {
            panic!("expected a number literal");
        };
        assert_eq!(n, "1000000");
    }

    #[test]
    fn a_trailing_underscore_is_not_part_of_a_number() {
        let ast = parse_assertion(".a == 1_");
        assert!(
            !matches!(&ast, AssertionExpr::Binary { right, .. }
                if matches!(&**right, AssertionExpr::Atom(Expr::Literal(Literal::Number(n))) if n == "1_")),
            "got {ast:?}"
        );
    }

    #[test]
    fn encoder_and_decoder_are_inverses_over_every_char_class() {
        let mut values: Vec<String> = (0u32..0x80)
            .filter_map(char::from_u32)
            .map(|c| format!("x{c}y"))
            .collect();
        values.extend(
            [
                "café",
                "日本",
                "emoji 🌍",
                "\u{7f}",
                "\u{9f}",
                "a\\b",
                "q\"q",
            ]
            .map(String::from),
        );

        for value in values {
            let mut rendered = String::new();
            write_escaped_string_literal(&mut rendered, &value);

            let AssertionExpr::Binary { right, .. } = parse_assertion(&format!(".a == {rendered}"))
            else {
                panic!("{rendered:?} did not re-parse as a comparison");
            };
            let AssertionExpr::Atom(Expr::Literal(Literal::Str(back))) = &*right else {
                panic!("{rendered:?} did not re-parse as a string literal");
            };
            assert_eq!(
                *back, value,
                "encode/decode disagree for {value:?} (rendered as {rendered})"
            );
        }
    }

    #[test]
    fn a_comparison_with_nothing_on_the_right_is_not_a_comparison() {
        assert_eq!(dangling_operator(".name ==").as_deref(), Some("=="));
        assert_eq!(dangling_operator(".n >=").as_deref(), Some(">="));
        assert_eq!(
            dangling_operator(".s contains").as_deref(),
            Some("contains")
        );
        assert_eq!(dangling_operator(".name == \"Ada\""), None);
        assert_eq!(dangling_operator(".name"), None);
        assert_eq!(dangling_operator(""), None);
        assert_eq!(dangling_operator(".name |").as_deref(), Some("|"));
        assert_eq!(dangling_operator(".name | length"), None);
        assert!(matches!(parse_assertion(".name =="), AssertionExpr::Raw(_)));
        assert!(!matches!(
            parse_assertion(".name == \"Ada\""),
            AssertionExpr::Raw(_)
        ));
    }

    #[test]
    fn an_assertion_that_cannot_read_the_answer_is_not_one() {
        assert!(reads_nothing("200"));
        assert!(reads_nothing("\"SERVING\""));
        assert!(reads_nothing("true"));
        assert!(reads_nothing("1 == 1"));
        assert!(!reads_nothing(".name"));
        assert!(!reads_nothing("@status() == 200"));
        assert!(!reads_nothing("$count > 0"));
        assert!(!reads_nothing("item_count == 2"));
        assert!(!reads_nothing("has_more == true"));
        assert!(reads_nothing("true and false"));
        assert!(!reads_nothing(".items[0] == 1"));
        assert!(!reads_nothing(""));
    }
}
