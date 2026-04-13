//! AST nodes for assertion expressions.
//!
//! All assertion expressions are parsed into structured AST nodes
//! using a recursive descent parser with proper tokenization.

use serde::{Deserialize, Serialize};

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
    IfThenElse {
        condition: Box<AssertionExpr>,
        then_branch: Box<AssertionExpr>,
        else_branch: Box<AssertionExpr>,
    },
    Paren(Box<AssertionExpr>),
    Atom(Expr),
    Raw(String),
}

/// An atomic expression inside an assertion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    JqPath(String),
    PluginCall {
        name: String,
        args: Vec<AssertionExpr>,
    },
    Literal(Literal),
    Variable(String),
    /// JavaScript-style regex literal: `/pattern/flags`
    RegExp {
        pattern: String,
        flags: String,
    },
    /// Inline JSON literal: `{"key": "value"}`
    Json(String),
    /// Inline YAML literal: `key: value` (parsed when in extract context)
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
    pub fn try_parse(s: &str) -> Option<Self> {
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

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Id(String),
    Str(String),
    Num(String),
    Op(String),
    RegExpLit(String, String), // (pattern, flags)
    At,
    LP,
    RP,
    LB,
    RB,
    LBr,
    RBr,
    Dot,
    Comma,
    Bang,
    Slash,
    If,
    Then,
    Else,
    End,
    And,
    Or,
    Not,
    Var,
}

fn tokenize(s: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        match cs[i] {
            ' ' | '\t' | '\n' | '\r' => {
                i += 1;
            }
            '@' => {
                out.push(Tok::At);
                i += 1;
            }
            '(' => {
                out.push(Tok::LP);
                i += 1;
            }
            ')' => {
                out.push(Tok::RP);
                i += 1;
            }
            '[' => {
                out.push(Tok::LB);
                i += 1;
            }
            ']' => {
                out.push(Tok::RB);
                i += 1;
            }
            '{' => {
                if i + 1 < cs.len() && cs[i + 1] == '{' {
                    out.push(Tok::Var);
                    i += 2;
                } else {
                    out.push(Tok::LBr);
                    i += 1;
                }
            }
            '}' => {
                if i + 1 < cs.len() && cs[i + 1] == '}' {
                    out.push(Tok::Var);
                    i += 2;
                } else {
                    out.push(Tok::RBr);
                    i += 1;
                }
            }
            '.' => {
                out.push(Tok::Dot);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '!' => {
                if i + 1 < cs.len() && cs[i + 1] == '=' {
                    out.push(Tok::Op("!=".into()));
                    i += 2;
                } else {
                    out.push(Tok::Bang);
                    i += 1;
                }
            }
            '/' => {
                // Check if this looks like a regex literal: /pattern/flags
                let mut j = i + 1;
                let mut is_regex = true;
                let mut pattern = String::new();
                let mut flags = String::new();
                let mut escaped = false;
                let mut in_char_class = false;

                while j < cs.len() {
                    if escaped {
                        pattern.push(cs[j]);
                        escaped = false;
                        j += 1;
                    } else if cs[j] == '\\' {
                        pattern.push(cs[j]);
                        escaped = true;
                        j += 1;
                    } else if cs[j] == '[' {
                        in_char_class = true;
                        pattern.push(cs[j]);
                        j += 1;
                    } else if cs[j] == ']' {
                        in_char_class = false;
                        pattern.push(cs[j]);
                        j += 1;
                    } else if cs[j] == '/' && !in_char_class {
                        // Found closing /
                        j += 1;
                        // Collect flags (g, i, m, s, u, y)
                        let mut rx_flags = String::new();
                        while j < cs.len()
                            && cs[j].is_ascii_alphabetic()
                            && "gimsuy".contains(cs[j])
                        {
                            rx_flags.push(cs[j]);
                            j += 1;
                        }
                        // Make sure this isn't followed by alphanumeric (which would make it a path)
                        if j < cs.len()
                            && (cs[j].is_alphanumeric() || cs[j] == '_')
                            && !rx_flags.is_empty()
                        {
                            is_regex = false;
                        }
                        flags = rx_flags;
                        break;
                    } else if cs[j] == '\n' || cs[j] == '\r' || cs[j] == ' ' || cs[j] == '\t' {
                        // Newline or space before closing / — not a regex
                        is_regex = false;
                        break;
                    } else {
                        pattern.push(cs[j]);
                        j += 1;
                    }
                }

                if is_regex && !pattern.is_empty() && j > i + 1 {
                    out.push(Tok::RegExpLit(pattern, flags));
                    i = j;
                } else {
                    out.push(Tok::Slash);
                    i += 1;
                }
            }
            '=' => {
                if i + 1 < cs.len() && cs[i + 1] == '=' {
                    out.push(Tok::Op("==".into()));
                    i += 2;
                } else {
                    i += 1;
                }
            }
            '>' => {
                if i + 1 < cs.len() && cs[i + 1] == '=' {
                    out.push(Tok::Op(">=".into()));
                    i += 2;
                } else {
                    out.push(Tok::Op(">".into()));
                    i += 1;
                }
            }
            '<' => {
                if i + 1 < cs.len() && cs[i + 1] == '=' {
                    out.push(Tok::Op("<=".into()));
                    i += 2;
                } else {
                    out.push(Tok::Op("<".into()));
                    i += 1;
                }
            }
            '"' => {
                let mut v = String::new();
                i += 1;
                while i < cs.len() && cs[i] != '"' {
                    if cs[i] == '\\' && i + 1 < cs.len() {
                        i += 1;
                        v.push(cs[i]);
                    } else {
                        v.push(cs[i]);
                    }
                    i += 1;
                }
                if i < cs.len() {
                    i += 1;
                }
                out.push(Tok::Str(v));
            }
            c if c.is_ascii_digit() => {
                let mut v = String::new();
                while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                    v.push(cs[i]);
                    i += 1;
                }
                out.push(Tok::Num(v));
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut v = String::new();
                while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                    v.push(cs[i]);
                    i += 1;
                }
                out.push(match v.as_str() {
                    "if" => Tok::If,
                    "then" => Tok::Then,
                    "else" => Tok::Else,
                    "end" => Tok::End,
                    "and" => Tok::And,
                    "or" => Tok::Or,
                    "not" => Tok::Not,
                    _ => Tok::Id(v),
                });
            }
            _ => {
                i += 1;
            }
        }
    }
    out
}

/// Parse a raw assertion string into an AssertionExpr AST.
pub fn parse_assertion(raw: &str) -> AssertionExpr {
    let tokens = tokenize(raw);
    if tokens.is_empty() {
        return AssertionExpr::Raw(raw.to_string());
    }
    let mut pos = 0;
    let expr = parse_or(&tokens, &mut pos);
    if pos >= tokens.len() {
        expr
    } else {
        AssertionExpr::Raw(raw.to_string())
    }
}

fn parse_or(ts: &[Tok], p: &mut usize) -> AssertionExpr {
    let mut left = parse_and(ts, p);
    while *p < ts.len() && ts[*p] == Tok::Or {
        *p += 1;
        let right = parse_and(ts, p);
        left = AssertionExpr::Or {
            left: Box::new(left),
            right: Box::new(right),
        };
    }
    left
}

fn parse_and(ts: &[Tok], p: &mut usize) -> AssertionExpr {
    let mut left = parse_bin(ts, p);
    while *p < ts.len() && ts[*p] == Tok::And {
        *p += 1;
        let right = parse_bin(ts, p);
        left = AssertionExpr::And {
            left: Box::new(left),
            right: Box::new(right),
        };
    }
    left
}

fn parse_bin(ts: &[Tok], p: &mut usize) -> AssertionExpr {
    let mut left = parse_unary(ts, p);
    loop {
        let op = match ts.get(*p) {
            Some(Tok::Op(s)) => s.clone(),
            Some(Tok::Id(s))
                if matches!(
                    s.as_str(),
                    "contains" | "matches" | "startsWith" | "endsWith" | "startswith" | "endswith"
                ) =>
            {
                s.clone()
            }
            _ => break,
        };
        *p += 1;
        let right = parse_unary(ts, p);
        let op = BinaryOp::try_parse(&op).unwrap_or(match op.as_str() {
            "contains" => BinaryOp::Contains,
            "matches" => BinaryOp::Matches,
            "startsWith" | "startswith" => BinaryOp::StartsWith,
            _ => BinaryOp::EndsWith,
        });
        left = AssertionExpr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        };
    }
    left
}

fn parse_unary(ts: &[Tok], p: &mut usize) -> AssertionExpr {
    if *p >= ts.len() {
        return AssertionExpr::Raw(String::new());
    }
    match &ts[*p] {
        Tok::Bang => {
            *p += 1;
            // Check for second bang
            if *p < ts.len() && matches!(ts[*p], Tok::Bang) {
                *p += 1;
                AssertionExpr::NotNot(Box::new(parse_atom(ts, p)))
            } else {
                AssertionExpr::Not(Box::new(parse_atom(ts, p)))
            }
        }
        Tok::Not => {
            *p += 1;
            if *p < ts.len() && ts[*p] == Tok::Not {
                *p += 1;
                AssertionExpr::NotNot(Box::new(parse_atom(ts, p)))
            } else {
                AssertionExpr::Not(Box::new(parse_atom(ts, p)))
            }
        }
        Tok::If => parse_if(ts, p),
        Tok::LP => {
            *p += 1;
            let inner = parse_or(ts, p);
            if *p < ts.len() && ts[*p] == Tok::RP {
                *p += 1;
            }
            AssertionExpr::Paren(Box::new(inner))
        }
        _ => parse_atom(ts, p),
    }
}

fn parse_if(ts: &[Tok], p: &mut usize) -> AssertionExpr {
    *p += 1;
    let cond = parse_or(ts, p);
    if *p >= ts.len() || ts[*p] != Tok::Then {
        return AssertionExpr::Raw("if..then missing".into());
    }
    *p += 1;
    let then_b = parse_or(ts, p);
    if *p >= ts.len() || ts[*p] != Tok::Else {
        return AssertionExpr::Raw("if..else missing".into());
    }
    *p += 1;
    let else_b = parse_or(ts, p);
    if *p >= ts.len() || ts[*p] != Tok::End {
        return AssertionExpr::Raw("if..end missing".into());
    }
    *p += 1;
    AssertionExpr::IfThenElse {
        condition: Box::new(cond),
        then_branch: Box::new(then_b),
        else_branch: Box::new(else_b),
    }
}

fn parse_atom(ts: &[Tok], p: &mut usize) -> AssertionExpr {
    if *p >= ts.len() {
        return AssertionExpr::Atom(Expr::JqPath(String::new()));
    }
    let expr = match &ts[*p] {
        Tok::Str(s) => {
            *p += 1;
            Expr::Literal(Literal::Str(s.clone()))
        }
        Tok::Num(n) => {
            *p += 1;
            Expr::Literal(Literal::Number(n.clone()))
        }
        Tok::Id(s) if s == "true" => {
            *p += 1;
            Expr::Literal(Literal::Bool(true))
        }
        Tok::Id(s) if s == "false" => {
            *p += 1;
            Expr::Literal(Literal::Bool(false))
        }
        Tok::Id(s) if s == "null" => {
            *p += 1;
            Expr::Literal(Literal::Null)
        }
        Tok::Var => {
            *p += 1;
            let mut name = String::new();
            while *p < ts.len() && ts[*p] != Tok::Var {
                if let Tok::Id(s) = &ts[*p] {
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
        Tok::At => {
            *p += 1;
            let name = if *p < ts.len() {
                if let Tok::Id(s) = &ts[*p] {
                    *p += 1;
                    s.clone()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let args = if *p < ts.len() && ts[*p] == Tok::LP {
                *p += 1;
                let mut args = Vec::new();
                while *p < ts.len() && ts[*p] != Tok::RP {
                    args.push(parse_or(ts, p));
                    if *p < ts.len() && ts[*p] == Tok::Comma {
                        *p += 1;
                    }
                }
                if *p < ts.len() {
                    *p += 1;
                }
                args
            } else {
                Vec::new()
            };
            Expr::PluginCall { name, args }
        }
        Tok::RegExpLit(pat, flags) => {
            *p += 1;
            Expr::RegExp {
                pattern: pat.clone(),
                flags: flags.clone(),
            }
        }
        Tok::LBr => {
            // Parse as JSON literal: collect until matching RBr
            *p += 1;
            let mut json = String::from("{");
            let mut depth = 1;
            while *p < ts.len() && depth > 0 {
                match &ts[*p] {
                    Tok::LBr => {
                        depth += 1;
                        json.push('{');
                        *p += 1;
                    }
                    Tok::RBr => {
                        depth -= 1;
                        if depth > 0 {
                            json.push('}');
                        }
                        *p += 1;
                    }
                    Tok::LP => {
                        json.push('(');
                        *p += 1;
                    }
                    Tok::RP => {
                        json.push(')');
                        *p += 1;
                    }
                    Tok::LB => {
                        json.push('[');
                        *p += 1;
                    }
                    Tok::RB => {
                        json.push(']');
                        *p += 1;
                    }
                    Tok::Str(s) => {
                        json.push('"');
                        json.push_str(s);
                        json.push('"');
                        *p += 1;
                    }
                    Tok::Num(n) => {
                        json.push_str(n);
                        *p += 1;
                    }
                    Tok::Id(s) => {
                        json.push_str(s);
                        *p += 1;
                    }
                    Tok::Comma => {
                        json.push(',');
                        *p += 1;
                    }
                    Tok::Op(s) => {
                        json.push_str(s);
                        *p += 1;
                    }
                    Tok::Dot => {
                        json.push('.');
                        *p += 1;
                    }
                    Tok::At => {
                        json.push('@');
                        *p += 1;
                    }
                    Tok::Var => {
                        json.push_str("{{ }}");
                        *p += 1;
                    }
                    Tok::Slash => {
                        json.push('/');
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
        Tok::Dot | Tok::Id(_) => {
            let mut path = String::new();
            while *p < ts.len() {
                // Stop if we hit an operator keyword
                if let Tok::Id(s) = &ts[*p]
                    && matches!(
                        s.as_str(),
                        "contains"
                            | "matches"
                            | "startsWith"
                            | "endsWith"
                            | "startswith"
                            | "endswith"
                            | "and"
                            | "or"
                    )
                {
                    break;
                }
                match &ts[*p] {
                    Tok::Dot => {
                        path.push('.');
                        *p += 1;
                    }
                    Tok::Id(s) => {
                        path.push_str(s);
                        *p += 1;
                    }
                    Tok::LB => {
                        path.push('[');
                        *p += 1;
                        while *p < ts.len() && ts[*p] != Tok::RB {
                            if let Tok::Num(n) = &ts[*p] {
                                path.push_str(n);
                            } else if let Tok::Id(s) = &ts[*p] {
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

/// Display helpers: IfThenElse serializes as ternary `cond ? then : else`.
impl AssertionExpr {
    fn fmt_inner(&self, f: &mut std::fmt::Formatter<'_>, outer_prec: u8) -> std::fmt::Result {
        // Prec: or=1, and=2, bin=3, unary=4, atom=5
        match self {
            Self::Or { left, right } => {
                if outer_prec > 1 {
                    write!(f, "(")?;
                }
                left.fmt_inner(f, 1)?;
                write!(f, " or ")?;
                right.fmt_inner(f, 1)?;
                if outer_prec > 1 {
                    write!(f, ")")?;
                }
                Ok(())
            }
            Self::And { left, right } => {
                if outer_prec > 2 {
                    write!(f, "(")?;
                }
                left.fmt_inner(f, 2)?;
                write!(f, " and ")?;
                right.fmt_inner(f, 2)?;
                if outer_prec > 2 {
                    write!(f, ")")?;
                }
                Ok(())
            }
            Self::Binary { op, left, right } => {
                if outer_prec > 3 {
                    write!(f, "(")?;
                }
                left.fmt_inner(f, 3)?;
                write!(f, " {} ", op.as_str())?;
                right.fmt_inner(f, 3)?;
                if outer_prec > 3 {
                    write!(f, ")")?;
                }
                Ok(())
            }
            Self::Not(inner) => {
                write!(f, "!")?;
                inner.fmt_inner(f, 4)
            }
            Self::NotNot(inner) => {
                write!(f, "not not ")?;
                inner.fmt_inner(f, 4)
            }
            Self::IfThenElse {
                condition,
                then_branch,
                else_branch,
            } => {
                // Always parenthesize ternary for safety with nesting
                write!(f, "(")?;
                condition.fmt_inner(f, 0)?;
                write!(f, " ? ")?;
                then_branch.fmt_inner(f, 0)?;
                write!(f, " : ")?;
                else_branch.fmt_inner(f, 0)?;
                write!(f, ")")
            }
            Self::Paren(inner) => {
                write!(f, "(")?;
                inner.fmt_inner(f, 0)?;
                write!(f, ")")
            }
            Self::Atom(e) => std::fmt::Display::fmt(e, f),
            Self::Raw(s) => write!(f, "{}", s),
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
                    a.fmt_inner(f, 0)?;
                }
                write!(f, ")")
            }
            Self::Literal(Literal::Bool(b)) => write!(f, "{}", b),
            Self::Literal(Literal::Number(n)) => write!(f, "{}", n),
            Self::Literal(Literal::Str(s)) => write!(f, "\"{}\"", s),
            Self::Literal(Literal::Null) => write!(f, "null"),
            Self::Variable(n) => write!(f, "{{{{{}}}}}", n),
            Self::RegExp { pattern, flags } => {
                write!(f, "/{}/", pattern)?;
                if !flags.is_empty() {
                    write!(f, "{}", flags)?;
                }
                Ok(())
            }
            Self::Json(s) => write!(f, "{}", s),
            Self::Yaml(s) => write!(f, "{}", s),
        }
    }
}

/// Convert AssertionExpr back to string.
pub fn assertion_to_string(expr: &AssertionExpr) -> String {
    struct Wrap<'a>(&'a AssertionExpr);
    impl std::fmt::Display for Wrap<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.0.fmt_inner(f, 0)
        }
    }
    Wrap(expr).to_string()
}

/// Remove redundant parentheses from an assertion expression.
pub fn remove_redundant_parens(expr: &AssertionExpr) -> AssertionExpr {
    match expr {
        AssertionExpr::Paren(inner) => {
            let simplified = remove_redundant_parens(inner);
            match &simplified {
                AssertionExpr::Binary { .. }
                | AssertionExpr::And { .. }
                | AssertionExpr::Or { .. }
                | AssertionExpr::IfThenElse { .. }
                | AssertionExpr::Not(..)
                | AssertionExpr::NotNot(..)
                | AssertionExpr::Raw(_) => AssertionExpr::Paren(Box::new(simplified)),
                _ => simplified,
            }
        }
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
            panic!("Expected Binary, got {:?}", expr);
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
            panic!("Expected Binary");
        }
    }

    #[test]
    fn test_parse_negation() {
        let expr = parse_assertion("!@has_header(\"x\")");
        if let AssertionExpr::Not(inner) = expr {
            if let AssertionExpr::Atom(Expr::PluginCall { name, .. }) = &*inner {
                assert_eq!(name, "has_header");
            } else {
                panic!("Expected PluginCall");
            }
        } else {
            panic!("Expected Not");
        }
    }

    #[test]
    fn test_parse_contains() {
        let expr = parse_assertion(".name contains \"test\"");
        if let AssertionExpr::Binary { op, left, right } = expr {
            assert_eq!(op, BinaryOp::Contains);
            assert!(matches!(*left, AssertionExpr::Atom(Expr::JqPath(_))));
            assert!(matches!(
                *right,
                AssertionExpr::Atom(Expr::Literal(Literal::Str(_)))
            ));
        } else {
            panic!("Expected Binary");
        }
    }

    #[test]
    fn test_parse_startswith() {
        let expr = parse_assertion(".name startsWith \"te\"");
        if let AssertionExpr::Binary { op, .. } = expr {
            assert_eq!(op, BinaryOp::StartsWith);
        } else {
            panic!("Expected Binary");
        }
    }

    #[test]
    fn test_parse_matches() {
        let expr = parse_assertion(".name matches \"^te.*t$\"");
        if let AssertionExpr::Binary { op, .. } = expr {
            assert_eq!(op, BinaryOp::Matches);
        } else {
            panic!("Expected Binary");
        }
    }

    #[test]
    fn test_parse_if_then_else() {
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
            panic!("Expected IfThenElse, got {:?}", expr);
        }
    }

    #[test]
    fn test_if_then_else_serializes_as_ternary() {
        let expr = parse_assertion("if .x == 0 then true else false end");
        let s = assertion_to_string(&expr);
        assert!(s.contains('?'), "Should contain ternary operator: {}", s);
        assert!(s.contains(':'), "Should contain ternary colon: {}", s);
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
        let expr = parse_assertion(".x == 1 and .y == 2");
        assert!(matches!(expr, AssertionExpr::And { .. }));
        let expr = parse_assertion(".x == 1 or .y == 2");
        assert!(matches!(expr, AssertionExpr::Or { .. }));
    }

    #[test]
    fn test_parse_not_not() {
        let expr = parse_assertion("not not .x");
        if let AssertionExpr::NotNot(inner) = expr {
            assert!(matches!(*inner, AssertionExpr::Atom(Expr::JqPath(_))));
        } else {
            panic!("Expected NotNot, got {:?}", expr);
        }
    }

    #[test]
    fn test_parse_double_bang() {
        let expr = parse_assertion("!!.x");
        if let AssertionExpr::NotNot(inner) = expr {
            assert!(matches!(*inner, AssertionExpr::Atom(Expr::JqPath(_))));
        } else {
            panic!("Expected NotNot, got {:?}", expr);
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
                // Second arg should be a regex literal (including the ^ anchor)
                if let AssertionExpr::Atom(Expr::RegExp { pattern, flags }) = &args[1] {
                    assert_eq!(pattern, "^hello");
                    assert_eq!(flags, "i");
                } else {
                    panic!("Expected RegExp, got {:?}", args[1]);
                }
            } else {
                panic!("Expected PluginCall");
            }
        } else {
            panic!("Expected Binary, got {:?}", expr);
        }
    }

    #[test]
    fn test_parse_regex_serializes_correctly() {
        let expr = parse_assertion("@regex(.x, /\\d{4}/gi) == true");
        let s = assertion_to_string(&expr);
        assert!(
            s.contains("/\\d{4}/gi"),
            "Should contain regex literal: {}",
            s
        );
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
                panic!("Expected Json, got {:?}", right);
            }
        } else {
            panic!("Expected Binary, got {:?}", expr);
        }
    }

    #[test]
    fn test_nested_ternary_roundtrip() {
        let original = "if .x == 1 then if .y == 2 then true else false end else false end";
        let expr = parse_assertion(original);
        let s = assertion_to_string(&expr);
        // Should be serialized as nested ternary with proper parens
        assert!(s.contains('?'), "Should contain ternary operator: {}", s);
        assert!(s.contains('('), "Should contain parens for nesting: {}", s);
    }
}
