//! Lexer: turns `.dov` source text into tokens.

use crate::diag::{Diag, Pos, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    Int(i64),
    Str(String),
    Ident(String),
    // keywords
    Fn,
    Let,
    Var,
    If,
    Else,
    While,
    Return,
    Agent,
    On,
    Spawn,
    Send,
    Extern,
    True,
    False,
    // punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Semi,
    Colon,
    Dot,
    Arrow,
    Assign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub pos: Pos,
    /// Length in source characters (tokens never span lines).
    pub len: u32,
}

/// A `// ...` line comment. `text` starts with `//` and has no trailing whitespace.
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    pub text: String,
    pub pos: Pos,
}

/// The source spelling of a token (used by `dovc fmt` and in messages).
pub fn spell(t: &Tok) -> String {
    match t {
        Tok::Int(v) => v.to_string(),
        Tok::Str(s) => quote(s),
        Tok::Ident(s) => s.clone(),
        Tok::Fn => "fn".into(),
        Tok::Let => "let".into(),
        Tok::Var => "var".into(),
        Tok::If => "if".into(),
        Tok::Else => "else".into(),
        Tok::While => "while".into(),
        Tok::Return => "return".into(),
        Tok::Agent => "agent".into(),
        Tok::On => "on".into(),
        Tok::Spawn => "spawn".into(),
        Tok::Send => "send".into(),
        Tok::Extern => "extern".into(),
        Tok::True => "true".into(),
        Tok::False => "false".into(),
        Tok::LParen => "(".into(),
        Tok::RParen => ")".into(),
        Tok::LBrace => "{".into(),
        Tok::RBrace => "}".into(),
        Tok::Comma => ",".into(),
        Tok::Semi => ";".into(),
        Tok::Colon => ":".into(),
        Tok::Dot => ".".into(),
        Tok::Arrow => "->".into(),
        Tok::Assign => "=".into(),
        Tok::Plus => "+".into(),
        Tok::Minus => "-".into(),
        Tok::Star => "*".into(),
        Tok::Slash => "/".into(),
        Tok::Percent => "%".into(),
        Tok::Eq => "==".into(),
        Tok::Ne => "!=".into(),
        Tok::Lt => "<".into(),
        Tok::Le => "<=".into(),
        Tok::Gt => ">".into(),
        Tok::Ge => ">=".into(),
        Tok::AndAnd => "&&".into(),
        Tok::OrOr => "||".into(),
        Tok::Bang => "!".into(),
        Tok::Eof => "".into(),
    }
}

/// Quote a string as a `.dov` literal (inverse of the lexer's escapes).
pub fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Is `t` a keyword (so it cannot be used as a name)?
pub fn is_keyword(t: &Tok) -> bool {
    matches!(
        t,
        Tok::Fn
            | Tok::Let
            | Tok::Var
            | Tok::If
            | Tok::Else
            | Tok::While
            | Tok::Return
            | Tok::Agent
            | Tok::On
            | Tok::Spawn
            | Tok::Send
            | Tok::Extern
            | Tok::True
            | Tok::False
    )
}

fn keyword(s: &str) -> Option<Tok> {
    Some(match s {
        "fn" => Tok::Fn,
        "let" => Tok::Let,
        "var" => Tok::Var,
        "if" => Tok::If,
        "else" => Tok::Else,
        "while" => Tok::While,
        "return" => Tok::Return,
        "agent" => Tok::Agent,
        "on" => Tok::On,
        "spawn" => Tok::Spawn,
        "send" => Tok::Send,
        "extern" => Tok::Extern,
        "true" => Tok::True,
        "false" => Tok::False,
        _ => return None,
    })
}

pub fn lex(src: &str) -> Result<Vec<Token>> {
    Ok(lex_with_comments(src)?.0)
}

/// Lex and also keep the line comments (for the formatter).
pub fn lex_with_comments(src: &str) -> Result<(Vec<Token>, Vec<Comment>)> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut comments = Vec::new();
    let (mut i, mut line, mut col) = (0usize, 1u32, 1u32);

    macro_rules! bump {
        () => {{
            let c = chars[i];
            i += 1;
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            c
        }};
    }

    while i < chars.len() {
        let c = chars[i];
        let pos = Pos { line, col };
        let start = i;
        if c.is_whitespace() {
            bump!();
            continue;
        }
        // line comment
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            let mut text = String::new();
            while i < chars.len() && chars[i] != '\n' {
                text.push(bump!());
            }
            comments.push(Comment {
                text: text.trim_end().to_string(),
                pos,
            });
            continue;
        }
        if c.is_ascii_digit() {
            let mut s = String::new();
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                let d = bump!();
                if d != '_' {
                    s.push(d);
                }
            }
            let v: i64 = s.parse().map_err(|_| {
                Diag::new(
                    pos,
                    format!("integer literal `{s}` does not fit in i64"),
                    "i64 holds -9223372036854775808 to 9223372036854775807; write the minimum as `-9223372036854775807 - 1`",
                )
            })?;
            out.push(Token {
                tok: Tok::Int(v),
                pos,
                len: (i - start) as u32,
            });
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let mut s = String::new();
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                s.push(bump!());
            }
            let tok = keyword(&s).unwrap_or(Tok::Ident(s));
            out.push(Token {
                tok,
                pos,
                len: (i - start) as u32,
            });
            continue;
        }
        if c == '"' {
            bump!();
            let mut s = String::new();
            loop {
                if i >= chars.len() || chars[i] == '\n' {
                    return Err(Diag::new(
                        pos,
                        "unterminated string literal",
                        "close the string with `\"` on the same line; use `\\n` for a newline",
                    ));
                }
                let ch = bump!();
                match ch {
                    '"' => break,
                    '\\' => {
                        if i >= chars.len() {
                            return Err(Diag::new(
                                pos,
                                "unterminated string literal",
                                "close the string with `\"` on the same line",
                            ));
                        }
                        let e = bump!();
                        s.push(match e {
                            'n' => '\n',
                            't' => '\t',
                            '\\' => '\\',
                            '"' => '"',
                            '0' => '\0',
                            other => {
                                return Err(Diag::new(
                                    Pos { line, col: col - 1 },
                                    format!("unknown escape `\\{other}`"),
                                    "valid escapes are \\n \\t \\\\ \\\" and \\0",
                                ))
                            }
                        });
                    }
                    other => s.push(other),
                }
            }
            out.push(Token {
                tok: Tok::Str(s),
                pos,
                len: (i - start) as u32,
            });
            continue;
        }
        let next = chars.get(i + 1).copied();
        let (tok, len) = match (c, next) {
            ('-', Some('>')) => (Tok::Arrow, 2),
            ('=', Some('=')) => (Tok::Eq, 2),
            ('!', Some('=')) => (Tok::Ne, 2),
            ('<', Some('=')) => (Tok::Le, 2),
            ('>', Some('=')) => (Tok::Ge, 2),
            ('&', Some('&')) => (Tok::AndAnd, 2),
            ('|', Some('|')) => (Tok::OrOr, 2),
            ('(', _) => (Tok::LParen, 1),
            (')', _) => (Tok::RParen, 1),
            ('{', _) => (Tok::LBrace, 1),
            ('}', _) => (Tok::RBrace, 1),
            (',', _) => (Tok::Comma, 1),
            (';', _) => (Tok::Semi, 1),
            (':', _) => (Tok::Colon, 1),
            ('.', _) => (Tok::Dot, 1),
            ('=', _) => (Tok::Assign, 1),
            ('+', _) => (Tok::Plus, 1),
            ('-', _) => (Tok::Minus, 1),
            ('*', _) => (Tok::Star, 1),
            ('/', _) => (Tok::Slash, 1),
            ('%', _) => (Tok::Percent, 1),
            ('<', _) => (Tok::Lt, 1),
            ('>', _) => (Tok::Gt, 1),
            ('!', _) => (Tok::Bang, 1),
            _ => {
                let hint = if c.is_alphabetic() {
                    "names use ASCII letters, digits and `_` only (non-ASCII text belongs in strings)"
                } else if c == '&' || c == '|' {
                    "logical operators are `&&` and `||`; there are no bitwise operators in v0"
                } else {
                    "remove this character; it is not part of .dov syntax"
                };
                return Err(Diag::new(pos, format!("unexpected character `{c}`"), hint));
            }
        };
        for _ in 0..len {
            bump!();
        }
        out.push(Token {
            tok,
            pos,
            len: len as u32,
        });
    }
    out.push(Token {
        tok: Tok::Eof,
        pos: Pos { line, col },
        len: 0,
    });
    Ok((out, comments))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        lex(s).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn keywords_and_idents() {
        assert_eq!(
            toks("let var agent foo"),
            vec![
                Tok::Let,
                Tok::Var,
                Tok::Agent,
                Tok::Ident("foo".into()),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn numbers_with_underscores() {
        assert_eq!(toks("1_000"), vec![Tok::Int(1000), Tok::Eof]);
    }

    #[test]
    fn two_char_ops() {
        assert_eq!(
            toks("-> == != <= >= && ||"),
            vec![
                Tok::Arrow,
                Tok::Eq,
                Tok::Ne,
                Tok::Le,
                Tok::Ge,
                Tok::AndAnd,
                Tok::OrOr,
                Tok::Eof
            ]
        );
    }

    #[test]
    fn strings_and_escapes() {
        assert_eq!(toks(r#""a\nb""#), vec![Tok::Str("a\nb".into()), Tok::Eof]);
    }

    #[test]
    fn comments_skipped() {
        assert_eq!(toks("// hi\n42"), vec![Tok::Int(42), Tok::Eof]);
    }

    #[test]
    fn positions() {
        let t = lex("a\n  b").unwrap();
        assert_eq!(t[1].pos, Pos { line: 2, col: 3 });
    }

    #[test]
    fn errors() {
        assert!(lex("\"abc").is_err());
        assert!(lex("@").is_err());
        assert!(lex("99999999999999999999").is_err());
    }
}
