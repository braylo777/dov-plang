//! `dovc fmt`: the canonical layout for `.dov` source.
//!
//! The formatter works on the token stream plus comments, so it never drops a
//! comment and never changes meaning: after formatting, the output is re-lexed
//! and must give the same tokens and comments as the input.
//!
//! Rules (see docs/SPEC.md, "Canonical format"):
//! - 4-space indent per `{` level; one statement, field or handler per line.
//! - `{` ends its line; `}` starts its own line; `} else {` stays together; empty blocks are `{}`.
//! - One space around binary operators, `=` and `->`; one after `,` and `:`; none inside `( )`,
//!   before `(` of a call or parameter list, around `.`, or after unary `-`/`!`.
//! - Blank lines from the source are kept, collapsed to one; none at the start or end of a block.
//!   Top-level items that end in `}` are followed by one blank line.
//! - Comments stay where they were: own-line comments on their own line at the current indent,
//!   trailing comments after one space at the end of the line.
//! - Literals keep their source spelling (`1_000`, escapes). Lines are never wrapped.
//! - Output ends with exactly one newline; an empty file stays empty.

use crate::diag::{Diag, Pos, Result};
use crate::lexer::{lex_with_comments, Comment, Tok, Token};
use crate::parser;

enum Elem<'a> {
    Tok(&'a Token),
    Comment(&'a Comment),
}

impl Elem<'_> {
    fn pos(&self) -> Pos {
        match self {
            Elem::Tok(t) => t.pos,
            Elem::Comment(c) => c.pos,
        }
    }
    fn is_tok(&self, t: &Tok) -> bool {
        matches!(self, Elem::Tok(x) if x.tok == *t)
    }
}

/// Format `.dov` source. The source must lex and parse (it need not type-check).
pub fn format(src: &str) -> Result<String> {
    let (toks, comments) = lex_with_comments(src)?;
    parser::parse(toks.clone())?;
    let out = layout(src, &toks, &comments);
    // Safety net: formatting must not change tokens or comments.
    let same = match lex_with_comments(&out) {
        Ok((t2, c2)) => {
            t2.len() == toks.len()
                && t2.iter().zip(&toks).all(|(a, b)| a.tok == b.tok)
                && c2.len() == comments.len()
                && c2.iter().zip(&comments).all(|(a, b)| a.text == b.text)
        }
        Err(_) => false,
    };
    if !same {
        return Err(Diag::new(
            Pos {
                file: 0,
                line: 1,
                col: 1,
            },
            "internal error: formatting would change this program",
            "please report this file as a dovc bug; the file was not changed",
        ));
    }
    Ok(out)
}

/// The exact source text of a token (keeps `1_000` and string escapes as written).
fn source_text(lines: &[Vec<char>], t: &Token) -> String {
    let line = &lines[t.pos.line as usize - 1];
    let start = t.pos.col as usize - 1;
    line[start..start + t.len as usize].iter().collect()
}

/// Tokens after which a `-` is binary (they end an operand).
fn ends_operand(t: &Tok) -> bool {
    matches!(
        t,
        Tok::Ident(_) | Tok::Int(_) | Tok::Str(_) | Tok::True | Tok::False | Tok::RParen
    )
}

fn space_between(prev: &Tok, prev_unary: bool, cur: &Tok) -> bool {
    if matches!(
        cur,
        Tok::RParen | Tok::Comma | Tok::Semi | Tok::Dot | Tok::Colon
    ) {
        return false;
    }
    // Keep `- -x` apart so it never reads like a `--` decrement.
    if prev_unary && *prev == Tok::Minus && *cur == Tok::Minus {
        return true;
    }
    if matches!(prev, Tok::LParen | Tok::Dot) || prev_unary {
        return false;
    }
    if *cur == Tok::LParen {
        return !matches!(prev, Tok::Ident(_));
    }
    true
}

struct Out {
    text: String,
    cur: String,
    depth: usize,
    /// Indent of the current line, fixed when its first text is written.
    line_depth: usize,
}

impl Out {
    fn push(&mut self, s: &str) {
        if self.cur.is_empty() {
            self.line_depth = self.depth;
        }
        self.cur.push_str(s);
    }
    fn newline(&mut self) {
        if !self.cur.is_empty() {
            for _ in 0..self.line_depth {
                self.text.push_str("    ");
            }
            self.text.push_str(self.cur.trim_end());
            self.text.push('\n');
            self.cur.clear();
        }
    }
}

fn layout(src: &str, toks: &[Token], comments: &[Comment]) -> String {
    let lines: Vec<Vec<char>> = src.split('\n').map(|l| l.chars().collect()).collect();
    let mut elems: Vec<Elem> = toks
        .iter()
        .filter(|t| t.tok != Tok::Eof)
        .map(Elem::Tok)
        .chain(comments.iter().map(Elem::Comment))
        .collect();
    elems.sort_by_key(|e| (e.pos().line, e.pos().col));

    let mut o = Out {
        text: String::new(),
        cur: String::new(),
        depth: 0,
        line_depth: 0,
    };
    let mut prev: Option<Tok> = None; // last token written
    let mut prev_unary = false;
    let mut last_line = 0u32; // source line of the last element written
    let mut last_tok_line = 0u32; // source line of the last token written
    let mut pending_nl = false;
    let mut force_blank = false;
    let mut skip_next = false;
    let mut last_was_comment = false;

    for (k, e) in elems.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        let line = e.pos().line;
        // A comment on the same source line as the previous token trails that line.
        if let Elem::Comment(c) = e {
            if line == last_tok_line && !o.cur.is_empty() {
                o.push(" ");
                o.push(&c.text);
                last_line = line;
                pending_nl = true;
                continue;
            }
        }
        if pending_nl {
            o.newline();
            pending_nl = false;
        }
        if let Elem::Comment(_) = e {
            o.newline(); // an own-line comment inside an expression breaks the line
        }
        if o.cur.is_empty() {
            let after_open = prev == Some(Tok::LBrace);
            // A `fn` or `agent` right after a top-level `extern ...;` gets a blank line.
            let after_extern = o.depth == 0
                && prev == Some(Tok::Semi)
                && !last_was_comment
                && (e.is_tok(&Tok::Fn) || e.is_tok(&Tok::Agent));
            let wants_blank =
                force_blank || after_extern || (last_line != 0 && line > last_line + 1);
            if wants_blank && !o.text.is_empty() && !after_open && !e.is_tok(&Tok::RBrace) {
                o.text.push('\n');
            }
            force_blank = false;
        }
        last_line = line;
        last_was_comment = matches!(e, Elem::Comment(_));

        let t = match e {
            Elem::Comment(c) => {
                o.push(&c.text);
                pending_nl = true;
                continue;
            }
            Elem::Tok(t) => *t,
        };
        last_tok_line = line;
        let text = source_text(&lines, t);
        let next = elems.get(k + 1);

        match &t.tok {
            Tok::LBrace if next.is_some_and(|n| n.is_tok(&Tok::RBrace)) => {
                if !o.cur.is_empty() {
                    o.push(" ");
                }
                o.push("{}");
                skip_next = true;
                last_tok_line = next.unwrap().pos().line;
                last_line = last_tok_line;
                prev = Some(Tok::RBrace);
                close(&mut o, elems.get(k + 2), &mut pending_nl, &mut force_blank);
            }
            Tok::LBrace => {
                if !o.cur.is_empty() {
                    o.push(" ");
                }
                o.push("{");
                o.depth += 1;
                pending_nl = true;
                prev = Some(Tok::LBrace);
            }
            Tok::RBrace => {
                o.newline();
                o.depth = o.depth.saturating_sub(1);
                o.push("}");
                prev = Some(Tok::RBrace);
                close(&mut o, next, &mut pending_nl, &mut force_blank);
            }
            tok => {
                let unary = match tok {
                    Tok::Bang => true,
                    Tok::Minus => !prev.as_ref().is_some_and(ends_operand),
                    _ => false,
                };
                if !o.cur.is_empty()
                    && prev
                        .as_ref()
                        .is_some_and(|p| space_between(p, prev_unary, tok))
                {
                    o.push(" ");
                }
                o.push(&text);
                prev_unary = unary;
                prev = Some(tok.clone());
                if *tok == Tok::Semi {
                    pending_nl = true;
                }
                continue;
            }
        }
        prev_unary = false;
    }
    o.newline();
    o.text
}

/// After a `}`: stay on the line for `else`, otherwise end the line. A top-level
/// item that ends in `}` gets one blank line after it.
fn close(o: &mut Out, next: Option<&Elem>, pending_nl: &mut bool, force_blank: &mut bool) {
    if next.is_some_and(|n| n.is_tok(&Tok::Else)) {
        return;
    }
    *pending_nl = true;
    if o.depth == 0 && next.is_some() {
        *force_blank = true;
    }
}

#[cfg(test)]
mod tests {
    use super::format;

    fn f(s: &str) -> String {
        format(s).unwrap()
    }

    #[test]
    fn basic_layout() {
        assert_eq!(
            f("fn main(){let x=1+2*3;print(x,-x,!true);}"),
            "fn main() {\n    let x = 1 + 2 * 3;\n    print(x, -x, !true);\n}\n"
        );
    }

    #[test]
    fn agents_else_and_empty_blocks() {
        let src = "agent C { var n: i64 = 0; on add(k: i64) { if k < 0 { return; } else { n = n - k; } } on z() {} }\nfn main() { }";
        assert_eq!(
            f(src),
            "agent C {\n    var n: i64 = 0;\n    on add(k: i64) {\n        if k < 0 {\n            return;\n        } else {\n            n = n - k;\n        }\n    }\n    on z() {}\n}\n\nfn main() {}\n"
        );
    }

    #[test]
    fn keeps_comments_literals_and_one_blank_line() {
        let src = "// top\nextern \"C\" fn labs(x: i64) -> i64; // libc\n\n\n\nfn main() {\n\n    // own line\n    print(1_000, \"a\\tb\");   // trailing\n\n}\n";
        assert_eq!(
            f(src),
            "// top\nextern \"C\" fn labs(x: i64) -> i64; // libc\n\nfn main() {\n    // own line\n    print(1_000, \"a\\tb\"); // trailing\n}\n"
        );
    }

    #[test]
    fn idempotent_and_rejects_bad_syntax() {
        let once = f("fn  main ( ) { var a=-(1) ; a = a-1 ; send spawn A.go( a ,2 ) ; }");
        assert_eq!(f(&once), once);
        assert!(format("fn main( {").is_err());
        assert_eq!(f(""), "");
    }
}
