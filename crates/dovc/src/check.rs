//! Semantic checks: name resolution, types, and immutability.
//!
//! The checker returns a table of expression types keyed by source position.
//! Every expression starts at a distinct token, so the position is a unique key.

use crate::ast::*;
use crate::diag::{closest, Diag, Pos, Result};
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
    /// name -> (type, mutable, is_parameter)
    scopes: Vec<HashMap<String, (Type, bool, bool)>>,
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
                let names = self.agents.keys().map(|s| s.as_str());
                let hint = match closest(n, names.chain(["i64", "bool", "str"])) {
                    Some(c) => format!("did you mean `{c}`?"),
                    None => "types are `i64`, `bool`, `str`, `()` or an agent name".into(),
                };
                return Err(Diag::new(pos, format!("unknown type `{n}`"), hint));
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
                    "choose another name, e.g. `show`",
                ));
            }
            if matches!(name, "i64" | "bool" | "str") {
                return Err(Diag::new(
                    pos,
                    format!("`{name}` is a built-in type name"),
                    "choose another name for this declaration",
                ));
            }
            if let Some(prev) = seen.insert(name, pos) {
                return Err(Diag::new(
                    pos,
                    format!("`{name}` is already defined at {}:{}", prev.line, prev.col),
                    "functions, externs and agents share one namespace; rename one of them",
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
                            return Err(Diag::new(
                                p.pos,
                                "parameters cannot have type `()`",
                                "remove the parameter; `()` carries no value",
                            ));
                        }
                    }
                    self.known_type(&f.ret, f.pos)?;
                    if f.name == "main" && (!f.params.is_empty() || f.ret != Type::Unit) {
                        return Err(Diag::new(
                            f.pos,
                            "`main` takes no parameters and returns `()`",
                            "declare it as `fn main() { ... }`",
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
                            "`main` and names that start with `dov_` belong to the compiler and runtime",
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
                                "C maps i64 to `long`, bool to `bool`, str to `const char *`; agent handles stay inside .dov",
                            ));
                        }
                    }
                    if matches!(e.ret, Type::Agent(_)) {
                        return Err(Diag::new(
                            e.pos,
                            "extern \"C\" functions cannot return agent handles",
                            "return i64, bool, str or `()` from C",
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
                "add `fn main() { ... }`; the program starts there",
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
                                format!("change the annotation to `{t}`, or give it a value of type `{ann}`"),
                            ));
                        }
                    }
                    if t == Type::Unit {
                        return Err(Diag::new(
                            f.pos,
                            "fields cannot have type `()`",
                            "a field needs a value of type i64, bool, str or an agent handle",
                        ));
                    }
                    if fields.insert(f.name.clone(), (t, f.mutable)).is_some() {
                        return Err(Diag::new(
                            f.pos,
                            format!("duplicate field `{}`", f.name),
                            "each field in an agent needs its own name",
                        ));
                    }
                }
                let mut handlers = HashMap::new();
                for h in &a.handlers {
                    for p in &h.params {
                        self.known_type(&p.ty, p.pos)?;
                        if p.ty == Type::Unit {
                            return Err(Diag::new(
                                p.pos,
                                "parameters cannot have type `()`",
                                "remove the parameter; `()` carries no value",
                            ));
                        }
                    }
                    let tys = h.params.iter().map(|p| p.ty.clone()).collect();
                    if handlers.insert(h.name.clone(), tys).is_some() {
                        return Err(Diag::new(
                            h.pos,
                            format!("duplicate handler `{}`", h.name),
                            "each handler in an agent needs its own name; there is no overloading",
                        ));
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
                            "end the body with `return <value>;` or return in both `if`/`else` branches",
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
            self.scopes.last_mut().unwrap().get_mut(&p.name).unwrap().2 = true;
        }
        Ok(())
    }

    fn declare(&mut self, name: &str, t: Type, mutable: bool, pos: Pos) -> Result<()> {
        let scope = self.scopes.last_mut().unwrap();
        if scope.contains_key(name) {
            return Err(Diag::new(
                pos,
                format!("`{name}` is already declared in this scope"),
                "use a new name, or declare it with `var` once and assign to it",
            ));
        }
        scope.insert(name.to_string(), (t, mutable, false));
        Ok(())
    }

    /// Returns (type, mutable, what) where `what` is "binding", "parameter" or "field".
    fn lookup(&self, name: &str) -> Option<(Type, bool, &'static str)> {
        for s in self.scopes.iter().rev() {
            if let Some((t, m, is_param)) = s.get(name) {
                let what = if *is_param { "parameter" } else { "binding" };
                return Some((t.clone(), *m, what));
            }
        }
        if let Ctx::Handler(a) = self.ctx {
            if let Some((t, m)) = self.agents[a].fields.get(name) {
                return Some((t.clone(), *m, "field"));
            }
        }
        None
    }

    fn unknown_var(&self, name: &str, pos: Pos) -> Diag {
        let mut names: Vec<&str> = self
            .scopes
            .iter()
            .flat_map(|s| s.keys().map(|k| k.as_str()))
            .collect();
        if let Ctx::Handler(a) = self.ctx {
            names.extend(self.agents[a].fields.keys().map(|k| k.as_str()));
        }
        let hint = match closest(name, names) {
            Some(c) => format!("did you mean `{c}`?"),
            None => format!("declare it first with `let {name} = ...;`"),
        };
        Diag::new(pos, format!("unknown variable `{name}`"), hint)
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
        self.expect_h(
            e,
            want,
            what,
            "there are no implicit conversions; change the value to the expected type",
        )
    }

    fn expect_h(&mut self, e: &Expr, want: &Type, what: &str, hint: &str) -> Result<()> {
        let got = self.expr(e)?;
        if got != *want {
            return Err(Diag::new(
                e.pos,
                format!("{what}: expected `{want}`, found `{got}`"),
                hint,
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
                            format!("change the annotation to `{t}`, or give it a value of type `{ann}`"),
                        ));
                    }
                }
                if t == Type::Unit {
                    return Err(Diag::new(
                        init.pos,
                        format!("cannot bind `{name}` to a `()` value"),
                        "call it as a statement instead, e.g. `print(...);`",
                    ));
                }
                self.declare(name, t, *mutable, s.pos)?;
            }
            StmtKind::Assign { name, value } => {
                let (t, mutable, what) = self
                    .lookup(name)
                    .ok_or_else(|| self.unknown_var(name, s.pos))?;
                if !mutable {
                    let hint = match what {
                        "parameter" => format!("parameters are immutable; copy it first: `var {name}2 = {name};`"),
                        "field" => format!("declare the field with `var {name}` to allow mutation"),
                        _ => format!("declare it with `var {name}` instead of `let {name}` to allow mutation"),
                    };
                    return Err(Diag::new(
                        s.pos,
                        format!("cannot assign to immutable {what} `{name}`"),
                        hint,
                    ));
                }
                self.expect(value, &t, &format!("assignment to `{name}`"))?;
            }
            StmtKind::If { cond, then, els } => {
                self.expect_h(cond, &Type::Bool, "`if` condition", "conditions must be `bool`; compare explicitly, e.g. `n != 0`")?;
                self.block(then)?;
                if let Some(e) = els {
                    self.block(e)?;
                }
            }
            StmtKind::While { cond, body } => {
                self.expect_h(cond, &Type::Bool, "`while` condition", "conditions must be `bool`; compare explicitly, e.g. `n != 0`")?;
                self.block(body)?;
            }
            StmtKind::Return(v) => {
                let ret = self.ret.clone();
                match v {
                    None if ret == Type::Unit => {}
                    None => {
                        return Err(Diag::new(
                            s.pos,
                            format!("`return` needs a `{ret}` value"),
                            format!("write `return <{ret} value>;`"),
                        ))
                    }
                    Some(e) if ret == Type::Unit => {
                        let hint = if matches!(self.ctx, Ctx::Handler(_)) {
                            "handlers return nothing; write `return;`"
                        } else {
                            "this function returns `()`: write `return;`, or declare a return type such as `-> i64`"
                        };
                        self.expect_h(e, &ret, "return value", hint)?
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
                handler_pos,
                args,
            } => {
                let t = self.expr(target)?;
                let Type::Agent(a) = t else {
                    return Err(Diag::new(
                        target.pos,
                        format!("`send` needs an agent handle, found `{t}`"),
                        "get a handle with `let h = spawn AgentName;`, then `send h.handler(...);`",
                    ));
                };
                let hs = &self.agents[&a].handlers;
                let params = hs.get(handler).cloned().ok_or_else(|| {
                    let hint = match closest(handler, hs.keys().map(|k| k.as_str())) {
                        Some(c) => format!("did you mean `{c}`?"),
                        None if hs.is_empty() => format!("`{a}` has no handlers; add `on {handler}(...) {{ ... }}`"),
                        None => {
                            let mut names: Vec<&str> = hs.keys().map(|k| k.as_str()).collect();
                            names.sort();
                            format!("`{a}` handles: {}", names.join(", "))
                        }
                    };
                    Diag::new(*handler_pos, format!("agent `{a}` has no handler `{handler}`"), hint)
                })?;
                self.args(args, &params, &format!("{a}.{handler}"), *handler_pos)?;
            }
        }
        Ok(())
    }

    fn args(&mut self, args: &[Expr], params: &[Type], what: &str, pos: Pos) -> Result<()> {
        if args.len() != params.len() {
            let list: Vec<String> = params.iter().map(|t| t.to_string()).collect();
            return Err(Diag::new(
                pos,
                format!(
                    "`{what}` takes {} argument(s) but got {}",
                    params.len(),
                    args.len()
                ),
                format!("expected `{what}({})`", list.join(", ")),
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
            ExprKind::Var(n) => self.lookup(n).ok_or_else(|| self.unknown_var(n, e.pos))?.0,
            ExprKind::Unary(UnOp::Neg, x) => {
                self.expect_h(x, &Type::I64, "operand of `-`", "unary `-` takes an i64")?;
                Type::I64
            }
            ExprKind::Unary(UnOp::Not, x) => {
                self.expect_h(
                    x,
                    &Type::Bool,
                    "operand of `!`",
                    "`!` takes a bool; for numbers compare instead, e.g. `n == 0`",
                )?;
                Type::Bool
            }
            ExprKind::Binary(op, l, r) => {
                use BinOp::*;
                match op {
                    Add | Sub | Mul | Div | Rem => {
                        let h = "arithmetic works on i64 only; there is no string `+` in v0";
                        self.expect_h(l, &Type::I64, "left operand", h)?;
                        self.expect_h(r, &Type::I64, "right operand", h)?;
                        Type::I64
                    }
                    Lt | Le | Gt | Ge => {
                        let h = "`<`, `<=`, `>` and `>=` compare i64 values only";
                        self.expect_h(l, &Type::I64, "left operand", h)?;
                        self.expect_h(r, &Type::I64, "right operand", h)?;
                        Type::Bool
                    }
                    And | Or => {
                        let h = "`&&` and `||` take bool operands; compare first, e.g. `n > 0`";
                        self.expect_h(l, &Type::Bool, "left operand", h)?;
                        self.expect_h(r, &Type::Bool, "right operand", h)?;
                        Type::Bool
                    }
                    Eq | Ne => {
                        let lt = self.expr(l)?;
                        if !matches!(lt, Type::I64 | Type::Bool | Type::Str) {
                            return Err(Diag::new(
                                l.pos,
                                format!("cannot compare values of type `{lt}`"),
                                "`==` and `!=` work on i64, bool and str only",
                            ));
                        }
                        self.expect_h(
                            r,
                            &lt,
                            "right operand",
                            "both sides of `==`/`!=` must have the same type",
                        )?;
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
                            format!("`print` cannot show a value of type `{t}`"),
                            "`print` takes i64, bool and str arguments",
                        ));
                    }
                }
                Type::Unit
            }
            ExprKind::Call(name, args) => {
                let sig = self.fns.get(name).ok_or_else(|| {
                    let names = self.fns.keys().map(|k| k.as_str());
                    let hint = match closest(name, names.chain(["print"])) {
                        Some(c) => format!("did you mean `{c}`?"),
                        None => format!("declare it with `fn {name}(...) {{ ... }}` or `extern \"C\" fn {name}(...);`"),
                    };
                    Diag::new(e.pos, format!("unknown function `{name}`"), hint)
                })?;
                let (params, ret) = (sig.params.clone(), sig.ret.clone());
                if name == "main" {
                    return Err(Diag::new(
                        e.pos,
                        "`main` cannot be called",
                        "move the shared code into another function and call that",
                    ));
                }
                self.args(args, &params, name, e.pos)?;
                ret
            }
            ExprKind::Spawn(a) => {
                if !self.agents.contains_key(a) {
                    let hint = match closest(a, self.agents.keys().map(|k| k.as_str())) {
                        Some(c) => format!("did you mean `{c}`?"),
                        None => format!("declare it with `agent {a} {{ ... }}`"),
                    };
                    return Err(Diag::new(e.pos, format!("unknown agent `{a}`"), hint));
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
        assert!(err("fn f(a: i64) { a = 2; } fn main() {}").contains("immutable parameter `a`"));
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
