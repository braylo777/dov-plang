//! Semantic checks: name resolution, types, and immutability.
//!
//! The checker returns a table of expression types keyed by source position.
//! Every expression starts at a distinct token, so the position is a unique key.

use crate::ast::*;
use crate::diag::{Diag, Pos, Result};
use std::collections::HashMap;

pub type Types = HashMap<Pos, Type>;

struct FnSig {
    params: Vec<Type>,
    ret: Type,
}

struct AgentSig {
    fields: HashMap<String, (Type, bool)>,
    handlers: HashMap<String, Vec<Type>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Ctx<'a> {
    Fn,
    Handler(&'a str),
}

struct Checker<'a> {
    fns: HashMap<String, FnSig>,
    agents: HashMap<String, AgentSig>,
    scopes: Vec<HashMap<String, (Type, bool)>>,
    ctx: Ctx<'a>,
    ret: Type,
    types: Types,
}

pub fn check(prog: &Program) -> Result<Types> {
    let mut c = Checker {
        fns: HashMap::new(),
        agents: HashMap::new(),
        scopes: Vec::new(),
        ctx: Ctx::Fn,
        ret: Type::Unit,
        types: HashMap::new(),
    };
    c.collect(prog)?;
    c.bodies(prog)?;
    Ok(c.types)
}

/// Does every path through `b` end in `return`?
fn returns(b: &Block) -> bool {
    b.iter().any(|s| match &s.kind {
        StmtKind::Return(_) => true,
        StmtKind::If {
            then, els: Some(e), ..
        } => returns(then) && returns(e),
        _ => false,
    })
}

impl<'a> Checker<'a> {
    fn known_type(&self, t: &Type, pos: Pos) -> Result<()> {
        if let Type::Agent(n) = t {
            if !self.agents.contains_key(n) {
                return Err(Diag::new(pos, format!("unknown type `{n}`")));
            }
        }
        Ok(())
    }

    fn collect(&mut self, prog: &Program) -> Result<()> {
        let mut seen: HashMap<&str, Pos> = HashMap::new();
        for item in &prog.items {
            let (name, pos) = match item {
                Item::Fn(f) => (f.name.as_str(), f.pos),
                Item::Extern(e) => (e.name.as_str(), e.pos),
                Item::Agent(a) => (a.name.as_str(), a.pos),
            };
            if name == "print" {
                return Err(Diag::new(
                    pos,
                    "`print` is a built-in and cannot be redefined",
                ));
            }
            if matches!(name, "i64" | "bool" | "str") {
                return Err(Diag::new(pos, format!("`{name}` is a built-in type name")));
            }
            if let Some(prev) = seen.insert(name, pos) {
                return Err(Diag::new(
                    pos,
                    format!("`{name}` is already defined at {}:{}", prev.line, prev.col),
                ));
            }
        }
        // Agents first so that agent types can appear in function signatures.
        for item in &prog.items {
            if let Item::Agent(a) = item {
                self.agents.insert(
                    a.name.clone(),
                    AgentSig {
                        fields: HashMap::new(),
                        handlers: HashMap::new(),
                    },
                );
            }
        }
        for item in &prog.items {
            match item {
                Item::Fn(f) => {
                    for p in &f.params {
                        self.known_type(&p.ty, p.pos)?;
                        if p.ty == Type::Unit {
                            return Err(Diag::new(p.pos, "parameters cannot have type `()`"));
                        }
                    }
                    self.known_type(&f.ret, f.pos)?;
                    if f.name == "main" && (!f.params.is_empty() || f.ret != Type::Unit) {
                        return Err(Diag::new(
                            f.pos,
                            "`main` takes no parameters and returns `()`",
                        ));
                    }
                    self.fns.insert(
                        f.name.clone(),
                        FnSig {
                            params: f.params.iter().map(|p| p.ty.clone()).collect(),
                            ret: f.ret.clone(),
                        },
                    );
                }
                Item::Extern(e) => {
                    if e.name.starts_with("dov_") || e.name == "main" {
                        return Err(Diag::new(
                            e.pos,
                            format!("extern name `{}` is reserved", e.name),
                        ));
                    }
                    for p in &e.params {
                        if !matches!(p.ty, Type::I64 | Type::Bool | Type::Str) {
                            return Err(Diag::new(
                                p.pos,
                                format!(
                                    "extern \"C\" parameters must be i64, bool or str, not `{}`",
                                    p.ty
                                ),
                            ));
                        }
                    }
                    if matches!(e.ret, Type::Agent(_)) {
                        return Err(Diag::new(
                            e.pos,
                            "extern \"C\" functions cannot return agent handles",
                        ));
                    }
                    self.fns.insert(
                        e.name.clone(),
                        FnSig {
                            params: e.params.iter().map(|p| p.ty.clone()).collect(),
                            ret: e.ret.clone(),
                        },
                    );
                }
                Item::Agent(_) => {}
            }
        }
        if !self.fns.contains_key("main") {
            return Err(Diag::new(
                Pos { line: 1, col: 1 },
                "program has no `fn main()`",
            ));
        }
        // Agent fields and handler signatures.
        for item in &prog.items {
            if let Item::Agent(a) = item {
                let mut fields = HashMap::new();
                for f in &a.fields {
                    // Field initializers see no locals and no other fields.
                    self.scopes = vec![HashMap::new()];
                    self.ctx = Ctx::Fn;
                    let t = self.expr(&f.init)?;
                    if let Some(ann) = &f.ty {
                        self.known_type(ann, f.pos)?;
                        if *ann != t {
                            return Err(Diag::new(
                                f.init.pos,
                                format!(
                                    "field `{}` is declared `{ann}` but its value is `{t}`",
                                    f.name
                                ),
                            ));
                        }
                    }
                    if t == Type::Unit {
                        return Err(Diag::new(f.pos, "fields cannot have type `()`"));
                    }
                    if fields.insert(f.name.clone(), (t, f.mutable)).is_some() {
                        return Err(Diag::new(f.pos, format!("duplicate field `{}`", f.name)));
                    }
                }
                let mut handlers = HashMap::new();
                for h in &a.handlers {
                    for p in &h.params {
                        self.known_type(&p.ty, p.pos)?;
                    }
                    let tys = h.params.iter().map(|p| p.ty.clone()).collect();
                    if handlers.insert(h.name.clone(), tys).is_some() {
                        return Err(Diag::new(h.pos, format!("duplicate handler `{}`", h.name)));
                    }
                }
                let sig = self.agents.get_mut(&a.name).unwrap();
                sig.fields = fields;
                sig.handlers = handlers;
            }
        }
        Ok(())
    }

    fn bodies(&mut self, prog: &'a Program) -> Result<()> {
        for item in &prog.items {
            match item {
                Item::Fn(f) => {
                    self.ctx = Ctx::Fn;
                    self.ret = f.ret.clone();
                    self.scopes = vec![HashMap::new()];
                    self.params(&f.params)?;
                    self.block(&f.body)?;
                    if f.ret != Type::Unit && !returns(&f.body) {
                        return Err(Diag::new(
                            f.pos,
                            format!(
                                "function `{}` must return `{}` on every path",
                                f.name, f.ret
                            ),
                        ));
                    }
                }
                Item::Agent(a) => {
                    for h in &a.handlers {
                        self.ctx = Ctx::Handler(&a.name);
                        self.ret = Type::Unit;
                        self.scopes = vec![HashMap::new()];
                        self.params(&h.params)?;
                        self.block(&h.body)?;
                    }
                }
                Item::Extern(_) => {}
            }
        }
        Ok(())
    }

    fn params(&mut self, ps: &[Param]) -> Result<()> {
        for p in ps {
            // Parameters are immutable bindings.
            self.declare(&p.name, p.ty.clone(), false, p.pos)?;
        }
        Ok(())
    }

    fn declare(&mut self, name: &str, t: Type, mutable: bool, pos: Pos) -> Result<()> {
        let scope = self.scopes.last_mut().unwrap();
        if scope.contains_key(name) {
            return Err(Diag::new(
                pos,
                format!("`{name}` is already declared in this scope"),
            ));
        }
        scope.insert(name.to_string(), (t, mutable));
        Ok(())
    }

    /// Returns (type, mutable, is_field).
    fn lookup(&self, name: &str) -> Option<(Type, bool, bool)> {
        for s in self.scopes.iter().rev() {
            if let Some((t, m)) = s.get(name) {
                return Some((t.clone(), *m, false));
            }
        }
        if let Ctx::Handler(a) = self.ctx {
            if let Some((t, m)) = self.agents[a].fields.get(name) {
                return Some((t.clone(), *m, true));
            }
        }
        None
    }

    fn block(&mut self, b: &Block) -> Result<()> {
        self.scopes.push(HashMap::new());
        for s in b {
            self.stmt(s)?;
        }
        self.scopes.pop();
        Ok(())
    }

    fn expect(&mut self, e: &Expr, want: &Type, what: &str) -> Result<()> {
        let got = self.expr(e)?;
        if got != *want {
            return Err(Diag::new(
                e.pos,
                format!("{what}: expected `{want}`, found `{got}`"),
            ));
        }
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> Result<()> {
        match &s.kind {
            StmtKind::Let {
                name,
                mutable,
                ty,
                init,
            } => {
                let t = self.expr(init)?;
                if let Some(ann) = ty {
                    self.known_type(ann, s.pos)?;
                    if *ann != t {
                        return Err(Diag::new(
                            init.pos,
                            format!("`{name}` is declared `{ann}` but its value is `{t}`"),
                        ));
                    }
                }
                if t == Type::Unit {
                    return Err(Diag::new(
                        init.pos,
                        format!("cannot bind `{name}` to a `()` value"),
                    ));
                }
                self.declare(name, t, *mutable, s.pos)?;
            }
            StmtKind::Assign { name, value } => {
                let (t, mutable, is_field) = self
                    .lookup(name)
                    .ok_or_else(|| Diag::new(s.pos, format!("unknown variable `{name}`")))?;
                if !mutable {
                    let what = if is_field { "field" } else { "binding" };
                    return Err(Diag::new(
                        s.pos,
                        format!("cannot assign to immutable {what} `{name}`; declare it with `var` to allow mutation"),
                    ));
                }
                self.expect(value, &t, &format!("assignment to `{name}`"))?;
            }
            StmtKind::If { cond, then, els } => {
                self.expect(cond, &Type::Bool, "`if` condition")?;
                self.block(then)?;
                if let Some(e) = els {
                    self.block(e)?;
                }
            }
            StmtKind::While { cond, body } => {
                self.expect(cond, &Type::Bool, "`while` condition")?;
                self.block(body)?;
            }
            StmtKind::Return(v) => {
                let ret = self.ret.clone();
                match v {
                    None if ret == Type::Unit => {}
                    None => {
                        return Err(Diag::new(s.pos, format!("`return` needs a `{ret}` value")))
                    }
                    Some(e) => self.expect(e, &ret, "return value")?,
                }
            }
            StmtKind::Expr(e) => {
                self.expr(e)?;
            }
            StmtKind::Send {
                target,
                handler,
                args,
            } => {
                let t = self.expr(target)?;
                let Type::Agent(a) = t else {
                    return Err(Diag::new(
                        target.pos,
                        format!("`send` needs an agent handle, found `{t}`"),
                    ));
                };
                let params = self.agents[&a]
                    .handlers
                    .get(handler)
                    .cloned()
                    .ok_or_else(|| {
                        Diag::new(s.pos, format!("agent `{a}` has no handler `{handler}`"))
                    })?;
                self.args(args, &params, &format!("{a}.{handler}"), s.pos)?;
            }
        }
        Ok(())
    }

    fn args(&mut self, args: &[Expr], params: &[Type], what: &str, pos: Pos) -> Result<()> {
        if args.len() != params.len() {
            return Err(Diag::new(
                pos,
                format!(
                    "`{what}` takes {} argument(s), but {} were given",
                    params.len(),
                    args.len()
                ),
            ));
        }
        for (i, (a, p)) in args.iter().zip(params).enumerate() {
            self.expect(a, p, &format!("argument {} of `{what}`", i + 1))?;
        }
        Ok(())
    }

    fn expr(&mut self, e: &Expr) -> Result<Type> {
        let t = match &e.kind {
            ExprKind::Int(_) => Type::I64,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Str(_) => Type::Str,
            ExprKind::Var(n) => {
                self.lookup(n)
                    .ok_or_else(|| Diag::new(e.pos, format!("unknown variable `{n}`")))?
                    .0
            }
            ExprKind::Unary(UnOp::Neg, x) => {
                self.expect(x, &Type::I64, "operand of `-`")?;
                Type::I64
            }
            ExprKind::Unary(UnOp::Not, x) => {
                self.expect(x, &Type::Bool, "operand of `!`")?;
                Type::Bool
            }
            ExprKind::Binary(op, l, r) => {
                use BinOp::*;
                match op {
                    Add | Sub | Mul | Div | Rem => {
                        self.expect(l, &Type::I64, "left operand")?;
                        self.expect(r, &Type::I64, "right operand")?;
                        Type::I64
                    }
                    Lt | Le | Gt | Ge => {
                        self.expect(l, &Type::I64, "left operand")?;
                        self.expect(r, &Type::I64, "right operand")?;
                        Type::Bool
                    }
                    And | Or => {
                        self.expect(l, &Type::Bool, "left operand")?;
                        self.expect(r, &Type::Bool, "right operand")?;
                        Type::Bool
                    }
                    Eq | Ne => {
                        let lt = self.expr(l)?;
                        if !matches!(lt, Type::I64 | Type::Bool | Type::Str) {
                            return Err(Diag::new(
                                l.pos,
                                format!("cannot compare values of type `{lt}`"),
                            ));
                        }
                        self.expect(r, &lt, "right operand")?;
                        Type::Bool
                    }
                }
            }
            ExprKind::Call(name, args) if name == "print" => {
                for a in args {
                    let t = self.expr(a)?;
                    if !matches!(t, Type::I64 | Type::Bool | Type::Str) {
                        return Err(Diag::new(
                            a.pos,
                            format!("`print` cannot show a `{t}` value"),
                        ));
                    }
                }
                Type::Unit
            }
            ExprKind::Call(name, args) => {
                let sig = self
                    .fns
                    .get(name)
                    .ok_or_else(|| Diag::new(e.pos, format!("unknown function `{name}`")))?;
                let (params, ret) = (sig.params.clone(), sig.ret.clone());
                if name == "main" {
                    return Err(Diag::new(e.pos, "`main` cannot be called"));
                }
                self.args(args, &params, name, e.pos)?;
                ret
            }
            ExprKind::Spawn(a) => {
                if !self.agents.contains_key(a) {
                    return Err(Diag::new(e.pos, format!("unknown agent `{a}`")));
                }
                Type::Agent(a.clone())
            }
        };
        self.types.insert(e.pos, t.clone());
        Ok(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer::lex, parser::parse};

    fn chk(src: &str) -> Result<Types> {
        check(&parse(lex(src)?)?)
    }

    fn err(src: &str) -> String {
        chk(src).unwrap_err().msg
    }

    #[test]
    fn ok_program() {
        chk("fn add(a: i64, b: i64) -> i64 { return a + b; } fn main() { let x = add(1, 2); print(x); }").unwrap();
    }

    #[test]
    fn let_is_immutable() {
        assert!(err("fn main() { let x = 1; x = 2; }")
            .contains("cannot assign to immutable binding `x`"));
    }

    #[test]
    fn var_is_mutable() {
        chk("fn main() { var x = 1; x = x + 1; }").unwrap();
    }

    #[test]
    fn params_are_immutable() {
        assert!(err("fn f(a: i64) { a = 2; } fn main() {}").contains("immutable binding `a`"));
    }

    #[test]
    fn let_fields_are_immutable() {
        assert!(err("agent A { let k = 1; on h() { k = 2; } } fn main() {}")
            .contains("immutable field `k`"));
        chk("agent A { var k = 1; on h() { k = 2; } } fn main() {}").unwrap();
    }

    #[test]
    fn type_mismatch() {
        assert!(err("fn main() { let x: i64 = true; }")
            .contains("declared `i64` but its value is `bool`"));
        assert!(err("fn main() { let x = 1 + true; }").contains("expected `i64`, found `bool`"));
        assert!(err("fn main() { if 1 { } }").contains("`if` condition"));
    }

    #[test]
    fn missing_return() {
        assert!(err("fn f() -> i64 { } fn main() {}").contains("must return"));
        chk("fn f(b: bool) -> i64 { if b { return 1; } else { return 2; } } fn main() {}").unwrap();
    }

    #[test]
    fn send_checks() {
        let base = "agent C { on add(n: i64) {} } fn main() { let c = spawn C; ";
        chk(&format!("{base} send c.add(1); }}")).unwrap();
        assert!(err(&format!("{base} send c.nope(1); }}")).contains("no handler `nope`"));
        assert!(err(&format!("{base} send c.add(true); }}")).contains("argument 1"));
        assert!(err(&format!("{base} send c.add(); }}")).contains("takes 1 argument"));
        assert!(err("fn main() { let x = 1; send x.add(1); }").contains("needs an agent handle"));
    }

    #[test]
    fn names() {
        assert!(err("fn main() { print(y); }").contains("unknown variable `y`"));
        assert!(err("fn main() { f(); }").contains("unknown function `f`"));
        assert!(err("fn main() {} fn main() {}").contains("already defined"));
        assert!(err("fn f() {}").contains("no `fn main()`"));
        assert!(err("fn main() { let x = 1; let x = 2; }").contains("already declared"));
        chk("fn main() { let x = 1; if true { let x = 2; } }").unwrap();
    }

    #[test]
    fn extern_rules() {
        chk(r#"extern "C" fn labs(x: i64) -> i64; fn main() { print(labs(-3)); }"#).unwrap();
        assert!(err(r#"extern "C" fn dov_x(); fn main() {}"#).contains("reserved"));
    }

    #[test]
    fn records_types() {
        let t = chk("fn main() { let s = \"hi\"; }").unwrap();
        assert!(t.values().any(|t| *t == Type::Str));
    }
}
