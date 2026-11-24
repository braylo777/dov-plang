//! Recursive-descent parser: tokens -> AST (v0.3 grammar, see docs/SPEC.md §2).

use crate::ast::*;
use crate::diag::{Diag, Pos, Result};
use crate::lexer::{is_keyword, spell, Tok, Token};
use std::collections::HashSet;

/// Layout facts the formatter needs and cannot see in the tokens alone.
#[derive(Debug, Default, Clone)]
pub struct Layout {
    /// `<` / `>` tokens that bracket type parameters or arguments (no spaces around them).
    pub angles: HashSet<Pos>,
    /// `{` / `}` of struct literals (kept on one line).
    pub inline_braces: HashSet<Pos>,
    /// `,` that end a line (struct fields, enum variants).
    pub break_commas: HashSet<Pos>,
}

pub struct Parser {
    toks: Vec<Token>,
    i: usize,
    /// Inside `if`/`while`/`match` heads a `Name {` is a block, not a struct literal.
    no_struct: bool,
    layout: Layout,
}

pub fn parse(toks: Vec<Token>) -> Result<Program> {
    Ok(parse_with_layout(toks)?.0)
}

pub fn parse_with_layout(toks: Vec<Token>) -> Result<(Program, Layout)> {
    let mut p = Parser {
        toks,
        i: 0,
        no_struct: false,
        layout: Layout::default(),
    };
    let prog = p.program()?;
    Ok((prog, p.layout))
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Int(v) => format!("integer `{v}`"),
        Tok::Str(_) => "string literal".into(),
        Tok::Ident(s) => format!("`{s}`"),
        Tok::Eof => "end of file".into(),
        k if is_keyword(k) => format!("keyword `{}`", spell(k)),
        other => format!("`{}`", spell(other)),
    }
}

/// Default hint when `want` was expected.
fn hint_for(want: &Tok) -> &'static str {
    match want {
        Tok::Semi => "end the statement with `;`",
        Tok::RParen => "close the list with `)`; separate items with `,`",
        Tok::RBracket => "close with `]`; separate items with `,`",
        Tok::LParen => "add `(`: function, handler and call argument lists need parentheses",
        Tok::LBrace => "add `{` to start the block",
        Tok::RBrace => "add `}` to close the block",
        Tok::Colon => "give it a type, e.g. `n: i64`",
        Tok::Assign => "give it an initial value with `=`, e.g. `let x = 1;`",
        Tok::Dot => "write `send <handle>.<handler>(<args>);`",
        Tok::Fn => "write `extern \"C\" fn name(params) -> type;`",
        Tok::FatArrow => "write each arm as `pattern => { ... }`",
        Tok::Gt => "close the type list with `>`",
        _ => "check the syntax near here",
    }
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.i].tok
    }
    fn peek_at(&self, n: usize) -> &Tok {
        &self.toks[(self.i + n).min(self.toks.len() - 1)].tok
    }
    fn pos(&self) -> Pos {
        self.toks[self.i].pos
    }
    fn advance(&mut self) -> Token {
        let t = self.toks[self.i].clone();
        if self.i < self.toks.len() - 1 {
            self.i += 1;
        }
        t
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == t {
            self.advance();
            true
        } else {
            false
        }
    }
    /// End of the previous token: where a missing `;` belongs.
    fn prev_end(&self) -> Pos {
        if self.i == 0 {
            return self.pos();
        }
        let t = &self.toks[self.i - 1];
        Pos {
            file: t.pos.file,
            line: t.pos.line,
            col: t.pos.col + t.len,
        }
    }
    fn expect(&mut self, t: Tok, what: &str) -> Result<Pos> {
        if *self.peek() == t {
            Ok(self.advance().pos)
        } else {
            let pos = if t == Tok::Semi {
                self.prev_end()
            } else {
                self.pos()
            };
            Err(Diag::new(
                pos,
                format!("expected {what}, found {}", describe(self.peek())),
                hint_for(&t),
            ))
        }
    }
    fn ident(&mut self, what: &str) -> Result<(String, Pos)> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                let p = self.advance().pos;
                Ok((s, p))
            }
            other => {
                let hint = if is_keyword(&other) {
                    format!("`{}` is a keyword; choose another name", spell(&other))
                } else {
                    "names start with a letter or `_`, then letters, digits or `_`".into()
                };
                Err(Diag::new(
                    self.pos(),
                    format!("expected {what}, found {}", describe(&other)),
                    hint,
                ))
            }
        }
    }
    /// Run `f` with the struct-literal restriction set to `on`.
    fn with_no_struct<T>(&mut self, on: bool, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let saved = self.no_struct;
        self.no_struct = on;
        let r = f(self);
        self.no_struct = saved;
        r
    }

    fn program(&mut self) -> Result<Program> {
        let mut items = Vec::new();
        while *self.peek() != Tok::Eof {
            items.push(match self.peek() {
                Tok::Fn => Item::Fn(self.fn_decl()?),
                Tok::Extern => Item::Extern(self.extern_fn()?),
                Tok::Agent => Item::Agent(self.agent_decl()?),
                Tok::Struct => Item::Struct(self.struct_decl()?),
                Tok::Enum => Item::Enum(self.enum_decl()?),
                Tok::Import => {
                    let pos = self.advance().pos;
                    let path = match self.peek().clone() {
                        Tok::Str(s) => {
                            self.advance();
                            s
                        }
                        other => {
                            return Err(Diag::new(
                                self.pos(),
                                format!("expected a file path string after `import`, found {}", describe(&other)),
                                "write `import \"other.dov\";` (the path is relative to this file)",
                            ))
                        }
                    };
                    self.expect(Tok::Semi, "`;` after import")?;
                    Item::Import(path, pos)
                }
                other => {
                    return Err(Diag::new(
                        self.pos(),
                        format!(
                            "expected `fn`, `extern`, `agent`, `struct`, `enum` or `import`, found {}",
                            describe(other)
                        ),
                        "only declarations may appear at the top level; put statements inside `fn main() { ... }`",
                    ))
                }
            });
        }
        Ok(Program { items })
    }

    /// `<T, U>` after a declaration name.
    fn tparams(&mut self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        if *self.peek() != Tok::Lt {
            return Ok(out);
        }
        let lt = self.advance().pos;
        self.layout.angles.insert(lt);
        loop {
            let (n, _) = self.ident("a type parameter name")?;
            out.push(n);
            if !self.eat(&Tok::Comma) || *self.peek() == Tok::Gt {
                break;
            }
        }
        let gt = self.expect(Tok::Gt, "`>` after type parameters")?;
        self.layout.angles.insert(gt);
        Ok(out)
    }

    fn ty(&mut self) -> Result<Type> {
        if self.eat(&Tok::LParen) {
            self.expect(Tok::RParen, "`)` in unit type `()`")?;
            return Ok(Type::Unit);
        }
        if self.eat(&Tok::LBracket) {
            let t = self.ty()?;
            self.expect(Tok::RBracket, "`]` after slice element type")?;
            return Ok(Type::Slice(Box::new(t)));
        }
        let (name, _) = self.ident("a type")?;
        let mut args = Vec::new();
        if *self.peek() == Tok::Lt {
            let lt = self.advance().pos;
            self.layout.angles.insert(lt);
            loop {
                args.push(self.ty()?);
                if !self.eat(&Tok::Comma) || *self.peek() == Tok::Gt {
                    break;
                }
            }
            let gt = self.expect(Tok::Gt, "`>` after type arguments")?;
            self.layout.angles.insert(gt);
        }
        Ok(match (name.as_str(), args.is_empty()) {
            ("i64", true) => Type::I64,
            ("bool", true) => Type::Bool,
            ("str", true) => Type::Str,
            _ => Type::Named(name, args),
        })
    }

    fn params(&mut self) -> Result<Vec<Param>> {
        self.expect(Tok::LParen, "`(`")?;
        let mut ps = Vec::new();
        while *self.peek() != Tok::RParen {
            let (name, pos) = self.ident("a parameter name")?;
            self.expect(Tok::Colon, "`:` after parameter name")?;
            let ty = self.ty()?;
            ps.push(Param { name, ty, pos });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RParen, "`)`")?;
        Ok(ps)
    }

    fn ret_ty(&mut self) -> Result<Type> {
        if self.eat(&Tok::Arrow) {
            self.ty()
        } else {
            Ok(Type::Unit)
        }
    }

    fn fn_decl(&mut self) -> Result<FnDecl> {
        let pos = self.expect(Tok::Fn, "`fn`")?;
        let (name, _) = self.ident("a function name")?;
        let tparams = self.tparams()?;
        let params = self.params()?;
        let ret = self.ret_ty()?;
        let body = self.block()?;
        Ok(FnDecl {
            name,
            tparams,
            params,
            ret,
            body,
            pos,
        })
    }

    fn extern_fn(&mut self) -> Result<ExternFn> {
        let pos = self.expect(Tok::Extern, "`extern`")?;
        match self.peek().clone() {
            Tok::Str(abi) if abi == "C" => {
                self.advance();
            }
            Tok::Str(abi) => {
                return Err(Diag::new(
                    self.pos(),
                    format!("unsupported ABI \"{abi}\"; only \"C\" is supported"),
                    "write `extern \"C\" fn ...`",
                ))
            }
            _ => {
                return Err(Diag::new(
                    self.pos(),
                    format!(
                        "expected ABI string \"C\" after `extern`, found {}",
                        describe(self.peek())
                    ),
                    "write `extern \"C\" fn ...`",
                ))
            }
        }
        self.expect(Tok::Fn, "`fn`")?;
        let (name, _) = self.ident("a function name")?;
        let params = self.params()?;
        let ret = self.ret_ty()?;
        self.expect(Tok::Semi, "`;` after extern declaration")?;
        Ok(ExternFn {
            name,
            params,
            ret,
            pos,
        })
    }

    fn struct_decl(&mut self) -> Result<StructDecl> {
        let pos = self.expect(Tok::Struct, "`struct`")?;
        let (name, _) = self.ident("a struct name")?;
        let tparams = self.tparams()?;
        self.expect(Tok::LBrace, "`{`")?;
        let mut fields = Vec::new();
        while *self.peek() != Tok::RBrace {
            let (fname, fpos) = self.ident("a field name")?;
            self.expect(Tok::Colon, "`:` after field name")?;
            let ty = self.ty()?;
            fields.push(Param {
                name: fname,
                ty,
                pos: fpos,
            });
            if *self.peek() == Tok::Comma {
                let c = self.advance().pos;
                self.layout.break_commas.insert(c);
            } else {
                break;
            }
        }
        self.expect(Tok::RBrace, "`}` (separate fields with `,`)")?;
        Ok(StructDecl {
            name,
            tparams,
            fields,
            pos,
        })
    }

    fn enum_decl(&mut self) -> Result<EnumDecl> {
        let pos = self.expect(Tok::Enum, "`enum`")?;
        let (name, _) = self.ident("an enum name")?;
        let tparams = self.tparams()?;
        self.expect(Tok::LBrace, "`{`")?;
        let mut variants = Vec::new();
        while *self.peek() != Tok::RBrace {
            let (vname, vpos) = self.ident("a variant name")?;
            let mut fields = Vec::new();
            if self.eat(&Tok::LParen) {
                while *self.peek() != Tok::RParen {
                    fields.push(self.ty()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RParen, "`)` after variant payload types")?;
            }
            variants.push(Variant {
                name: vname,
                fields,
                pos: vpos,
            });
            if *self.peek() == Tok::Comma {
                let c = self.advance().pos;
                self.layout.break_commas.insert(c);
            } else {
                break;
            }
        }
        self.expect(Tok::RBrace, "`}` (separate variants with `,`)")?;
        Ok(EnumDecl {
            name,
            tparams,
            variants,
            pos,
        })
    }

    fn agent_decl(&mut self) -> Result<AgentDecl> {
        let pos = self.expect(Tok::Agent, "`agent`")?;
        let (name, _) = self.ident("an agent name")?;
        self.expect(Tok::LBrace, "`{`")?;
        let (mut fields, mut handlers) = (Vec::new(), Vec::new());
        loop {
            match self.peek() {
                Tok::RBrace => break,
                Tok::Let | Tok::Var => {
                    let fpos = self.pos();
                    let mutable = self.advance().tok == Tok::Var;
                    let (fname, _) = self.ident("a field name")?;
                    let ty = if self.eat(&Tok::Colon) {
                        Some(self.ty()?)
                    } else {
                        None
                    };
                    self.expect(Tok::Assign, "`=` (agent fields need an initial value)")?;
                    let init = self.expr()?;
                    self.expect(Tok::Semi, "`;`")?;
                    fields.push(Field {
                        name: fname,
                        mutable,
                        ty,
                        init,
                        pos: fpos,
                    });
                }
                Tok::On => {
                    let hpos = self.advance().pos;
                    let (hname, _) = self.ident("a handler name")?;
                    let params = self.params()?;
                    let body = self.block()?;
                    handlers.push(Handler {
                        name: hname,
                        params,
                        body,
                        pos: hpos,
                    });
                }
                other => {
                    return Err(Diag::new(
                        self.pos(),
                        format!(
                            "expected `let`, `var`, `on` or `}}` in agent body, found {}",
                            describe(other)
                        ),
                        "an agent holds only fields (`let`/`var name = value;`) and handlers (`on name(params) { ... }`)",
                    ))
                }
            }
        }
        self.expect(Tok::RBrace, "`}`")?;
        Ok(AgentDecl {
            name,
            fields,
            handlers,
            pos,
        })
    }

    fn block(&mut self) -> Result<Block> {
        self.expect(Tok::LBrace, "`{`")?;
        self.with_no_struct(false, |p| {
            let mut stmts = Vec::new();
            while *p.peek() != Tok::RBrace {
                if *p.peek() == Tok::Eof {
                    return Err(Diag::new(
                        p.pos(),
                        "expected `}`, found end of file",
                        "a block is not closed; add the missing `}`",
                    ));
                }
                stmts.push(p.stmt()?);
            }
            p.expect(Tok::RBrace, "`}`")?;
            Ok(stmts)
        })
    }

    fn stmt(&mut self) -> Result<Stmt> {
        let pos = self.pos();
        let kind = match self.peek().clone() {
            Tok::Let | Tok::Var => {
                let mutable = self.advance().tok == Tok::Var;
                let (name, _) = self.ident("a variable name")?;
                let ty = if self.eat(&Tok::Colon) {
                    Some(self.ty()?)
                } else {
                    None
                };
                self.expect(Tok::Assign, "`=`")?;
                let init = self.expr()?;
                self.expect(Tok::Semi, "`;`")?;
                StmtKind::Let {
                    name,
                    mutable,
                    ty,
                    init,
                }
            }
            Tok::If => self.if_stmt()?,
            Tok::While => {
                self.advance();
                let cond = self.with_no_struct(true, |p| p.expr())?;
                let body = self.block()?;
                StmtKind::While { cond, body }
            }
            Tok::Return => {
                self.advance();
                let v = if *self.peek() == Tok::Semi {
                    None
                } else {
                    Some(self.expr()?)
                };
                self.expect(Tok::Semi, "`;`")?;
                StmtKind::Return(v)
            }
            Tok::Send => {
                self.advance();
                let target = self.postfix()?;
                self.expect(Tok::Dot, "`.` then a handler name after `send <agent>`")?;
                let (handler, handler_pos) = self.ident("a handler name")?;
                let args = self.args()?;
                self.expect(Tok::Semi, "`;`")?;
                StmtKind::Send {
                    target,
                    handler,
                    handler_pos,
                    args,
                }
            }
            Tok::Match => {
                self.advance();
                let scrut = self.with_no_struct(true, |p| p.expr())?;
                self.expect(Tok::LBrace, "`{` after the `match` value")?;
                let mut arms = Vec::new();
                while *self.peek() != Tok::RBrace {
                    if *self.peek() == Tok::Eof {
                        return Err(Diag::new(
                            self.pos(),
                            "expected `}`, found end of file",
                            "the `match` is not closed; add the missing `}`",
                        ));
                    }
                    let apos = self.pos();
                    let pat = self.pattern()?;
                    self.expect(Tok::FatArrow, "`=>` after the pattern")?;
                    let body = self.block()?;
                    arms.push(Arm {
                        pat,
                        body,
                        pos: apos,
                    });
                }
                self.expect(Tok::RBrace, "`}`")?;
                StmtKind::Match { scrut, arms }
            }
            Tok::Defer => {
                self.advance();
                let body = if *self.peek() == Tok::LBrace {
                    self.block()?
                } else {
                    vec![self.stmt()?]
                };
                StmtKind::Defer(body)
            }
            _ => {
                let e = self.expr()?;
                if self.eat(&Tok::Assign) {
                    let value = self.expr()?;
                    self.expect(Tok::Semi, "`;`")?;
                    StmtKind::Assign { target: e, value }
                } else {
                    self.expect(Tok::Semi, "`;`")?;
                    StmtKind::Expr(e)
                }
            }
        };
        Ok(Stmt { kind, pos })
    }

    fn pattern(&mut self) -> Result<Pattern> {
        let pos = self.pos();
        Ok(match self.peek().clone() {
            Tok::Int(v) => {
                self.advance();
                Pattern::Int(v)
            }
            Tok::Minus if matches!(self.peek_at(1), Tok::Int(_)) => {
                self.advance();
                let Tok::Int(v) = self.advance().tok else {
                    unreachable!()
                };
                Pattern::Int(-v)
            }
            Tok::Str(s) => {
                self.advance();
                Pattern::Str(s)
            }
            Tok::True => {
                self.advance();
                Pattern::Bool(true)
            }
            Tok::False => {
                self.advance();
                Pattern::Bool(false)
            }
            Tok::Ident(n) if n == "_" => {
                self.advance();
                Pattern::Wild
            }
            Tok::Ident(first) => {
                self.advance();
                let (enum_name, variant) = if self.eat(&Tok::ColonColon) {
                    let (v, _) = self.ident("a variant name after `::`")?;
                    (Some(first), v)
                } else {
                    (None, first)
                };
                let mut binds = Vec::new();
                if self.eat(&Tok::LParen) {
                    while *self.peek() != Tok::RParen {
                        let (b, bpos) = self.ident("a binding name or `_`")?;
                        binds.push(if b == "_" { None } else { Some((b, bpos)) });
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(Tok::RParen, "`)` after the bindings")?;
                }
                Pattern::Variant {
                    enum_name,
                    variant,
                    binds,
                }
            }
            other => {
                return Err(Diag::new(
                    pos,
                    format!("expected a pattern, found {}", describe(&other)),
                    "a pattern is `_`, a literal, or a variant like `Some(x)` / `Shape::Circle(r)`",
                ))
            }
        })
    }

    fn if_stmt(&mut self) -> Result<StmtKind> {
        self.expect(Tok::If, "`if`")?;
        let cond = self.with_no_struct(true, |p| p.expr())?;
        let then = self.block()?;
        let els = if self.eat(&Tok::Else) {
            if *self.peek() == Tok::If {
                let pos = self.pos();
                Some(vec![Stmt {
                    kind: self.if_stmt()?,
                    pos,
                }])
            } else {
                Some(self.block()?)
            }
        } else {
            None
        };
        Ok(StmtKind::If { cond, then, els })
    }

    fn args(&mut self) -> Result<Vec<Expr>> {
        self.expect(Tok::LParen, "`(`")?;
        self.with_no_struct(false, |p| {
            let mut a = Vec::new();
            while *p.peek() != Tok::RParen {
                a.push(p.expr()?);
                if !p.eat(&Tok::Comma) {
                    break;
                }
            }
            p.expect(Tok::RParen, "`)`")?;
            Ok(a)
        })
    }

    pub fn expr(&mut self) -> Result<Expr> {
        self.binary(0)
    }

    fn binop(t: &Tok) -> Option<(BinOp, u8)> {
        Some(match t {
            Tok::OrOr => (BinOp::Or, 1),
            Tok::AndAnd => (BinOp::And, 2),
            Tok::Eq => (BinOp::Eq, 3),
            Tok::Ne => (BinOp::Ne, 3),
            Tok::Lt => (BinOp::Lt, 4),
            Tok::Le => (BinOp::Le, 4),
            Tok::Gt => (BinOp::Gt, 4),
            Tok::Ge => (BinOp::Ge, 4),
            Tok::Plus => (BinOp::Add, 5),
            Tok::Minus => (BinOp::Sub, 5),
            Tok::Star => (BinOp::Mul, 6),
            Tok::Slash => (BinOp::Div, 6),
            Tok::Percent => (BinOp::Rem, 6),
            _ => return None,
        })
    }

    /// Precedence climbing; all binary operators are left-associative.
    fn binary(&mut self, min: u8) -> Result<Expr> {
        let mut lhs = self.unary()?;
        while let Some((op, prec)) = Self::binop(self.peek()) {
            if prec <= min {
                break;
            }
            let pos = self.advance().pos;
            let rhs = self.binary(prec)?;
            lhs = Expr {
                kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)),
                pos,
            };
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr> {
        let pos = self.pos();
        if self.eat(&Tok::Minus) {
            let e = self.unary()?;
            return Ok(Expr {
                kind: ExprKind::Unary(UnOp::Neg, Box::new(e)),
                pos,
            });
        }
        if self.eat(&Tok::Bang) {
            let e = self.unary()?;
            return Ok(Expr {
                kind: ExprKind::Unary(UnOp::Not, Box::new(e)),
                pos,
            });
        }
        self.postfix()
    }

    /// A primary followed by `.field`, `[i]` or `[a..b]`. A `.name(` is left for `send`.
    fn postfix(&mut self) -> Result<Expr> {
        let mut e = self.primary()?;
        loop {
            match self.peek() {
                Tok::Dot
                    if matches!(self.peek_at(1), Tok::Ident(_))
                        && *self.peek_at(2) != Tok::LParen =>
                {
                    let pos = self.advance().pos;
                    let (f, _) = self.ident("a field name")?;
                    e = Expr {
                        kind: ExprKind::Field(Box::new(e), f),
                        pos,
                    };
                }
                Tok::LBracket => {
                    let pos = self.advance().pos;
                    let (a, b) = self.with_no_struct(false, |p| {
                        let a = p.expr()?;
                        let b = if p.eat(&Tok::DotDot) {
                            Some(p.expr()?)
                        } else {
                            None
                        };
                        p.expect(Tok::RBracket, "`]`")?;
                        Ok((a, b))
                    })?;
                    e = Expr {
                        kind: match b {
                            None => ExprKind::Index(Box::new(e), Box::new(a)),
                            Some(b) => ExprKind::Range(Box::new(e), Box::new(a), Box::new(b)),
                        },
                        pos,
                    };
                }
                _ => return Ok(e),
            }
        }
    }

    /// Is the parser at `{ }` or `{ name :`, the start of a struct literal body?
    fn struct_lit_ahead(&self) -> bool {
        *self.peek() == Tok::LBrace
            && (*self.peek_at(1) == Tok::RBrace
                || (matches!(self.peek_at(1), Tok::Ident(_)) && *self.peek_at(2) == Tok::Colon))
    }

    fn primary(&mut self) -> Result<Expr> {
        let pos = self.pos();
        let kind = match self.peek().clone() {
            Tok::Int(v) => {
                self.advance();
                ExprKind::Int(v)
            }
            Tok::Str(s) => {
                self.advance();
                ExprKind::Str(s)
            }
            Tok::True => {
                self.advance();
                ExprKind::Bool(true)
            }
            Tok::False => {
                self.advance();
                ExprKind::Bool(false)
            }
            Tok::Spawn => {
                self.advance();
                let (name, _) = self.ident("an agent name after `spawn`")?;
                ExprKind::Spawn(name)
            }
            Tok::LParen => {
                self.advance();
                let e = self.with_no_struct(false, |p| p.expr())?;
                self.expect(Tok::RParen, "`)`")?;
                return Ok(e);
            }
            Tok::LBracket => {
                self.advance();
                let items = self.with_no_struct(false, |p| {
                    let mut items = Vec::new();
                    while *p.peek() != Tok::RBracket {
                        items.push(p.expr()?);
                        if !p.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    p.expect(Tok::RBracket, "`]`")?;
                    Ok(items)
                })?;
                ExprKind::Array(items)
            }
            Tok::Ident(name) => {
                self.advance();
                if self.eat(&Tok::ColonColon) {
                    let (v, _) = self.ident("a variant name after `::`")?;
                    let args = if *self.peek() == Tok::LParen {
                        self.args()?
                    } else {
                        Vec::new()
                    };
                    ExprKind::Variant(name, v, args)
                } else if *self.peek() == Tok::LParen {
                    ExprKind::Call(name, self.args()?)
                } else if !self.no_struct && self.struct_lit_ahead() {
                    let open = self.advance().pos;
                    self.layout.inline_braces.insert(open);
                    let fields = self.with_no_struct(false, |p| {
                        let mut fields = Vec::new();
                        while *p.peek() != Tok::RBrace {
                            let (f, _) = p.ident("a field name")?;
                            p.expect(Tok::Colon, "`:` after the field name")?;
                            fields.push((f, p.expr()?));
                            if !p.eat(&Tok::Comma) {
                                break;
                            }
                        }
                        Ok(fields)
                    })?;
                    let close = self.expect(Tok::RBrace, "`}` after the struct fields")?;
                    self.layout.inline_braces.insert(close);
                    ExprKind::StructLit(name, fields)
                } else {
                    ExprKind::Var(name)
                }
            }
            other => return Err(Diag::new(
                pos,
                format!("expected an expression, found {}", describe(&other)),
                "an expression is a literal, a name, a call, `spawn Agent`, `[...]`, or `( ... )`",
            )),
        };
        Ok(Expr { kind, pos })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn p(src: &str) -> Result<Program> {
        parse(lex(src)?)
    }

    fn expr(src: &str) -> Expr {
        let prog = p(&format!("fn main() {{ {src}; }}")).unwrap();
        match &prog.items[0] {
            Item::Fn(f) => match &f.body[0].kind {
                StmtKind::Expr(e) => e.clone(),
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    fn show(e: &Expr) -> String {
        match &e.kind {
            ExprKind::Int(v) => v.to_string(),
            ExprKind::Var(n) => n.clone(),
            ExprKind::Binary(op, l, r) => format!("({:?} {} {})", op, show(l), show(r)),
            ExprKind::Unary(op, x) => format!("({:?} {})", op, show(x)),
            _ => "?".into(),
        }
    }

    #[test]
    fn precedence() {
        assert_eq!(show(&expr("1 + 2 * 3")), "(Add 1 (Mul 2 3))");
        assert_eq!(show(&expr("1 - 2 - 3")), "(Sub (Sub 1 2) 3)");
        assert_eq!(show(&expr("a < b && c || d")), "(Or (And (Lt a b) c) d)");
        assert_eq!(show(&expr("-a * (b + c)")), "(Mul (Neg a) (Add b c))");
    }

    #[test]
    fn agent_decl() {
        let prog =
            p("agent C { var n: i64 = 0; let k = 2; on add(x: i64) { n = n + x; } }").unwrap();
        match &prog.items[0] {
            Item::Agent(a) => {
                assert_eq!(a.fields.len(), 2);
                assert!(a.fields[0].mutable && !a.fields[1].mutable);
                assert_eq!(a.handlers[0].name, "add");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn send_and_spawn() {
        let prog = p("fn main() { let c = spawn C; send c.add(1, 2); }").unwrap();
        match &prog.items[0] {
            Item::Fn(f) => assert!(
                matches!(&f.body[1].kind, StmtKind::Send { handler, args, .. } if handler == "add" && args.len() == 2)
            ),
            _ => panic!(),
        }
    }

    #[test]
    fn extern_c() {
        let prog = p(r#"extern "C" fn labs(x: i64) -> i64;"#).unwrap();
        assert!(
            matches!(&prog.items[0], Item::Extern(e) if e.name == "labs" && e.ret == Type::I64)
        );
        assert!(p(r#"extern "Rust" fn f();"#).is_err());
    }

    #[test]
    fn else_if_chain() {
        let prog = p("fn main() { if a { } else if b { } else { } }").unwrap();
        match &prog.items[0] {
            Item::Fn(f) => assert!(
                matches!(&f.body[0].kind, StmtKind::If { els: Some(e), .. } if matches!(e[0].kind, StmtKind::If { .. }))
            ),
            _ => panic!(),
        }
    }

    #[test]
    fn syntax_errors_have_positions() {
        let e = p("fn main() {\n  let x = ;\n}").unwrap_err();
        assert_eq!(e.pos.line, 2);
        assert!(e.msg.contains("expected an expression"));
    }
}
