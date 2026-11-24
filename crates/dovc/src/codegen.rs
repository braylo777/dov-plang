//! C11 code generation. Input must have passed `check`.
//!
//! Generic functions, structs and enums are monomorphized: each concrete instance gets its
//! own C function or type, named with a length-prefixed mangling of its type arguments.
//! Structs become C structs, enums become tagged unions, slices and strings are small
//! by-value descriptors over immutable heap data (see `runtime/dov_rt.h`).

use crate::ast::*;
use crate::check::{subst, Checked};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write;

/// Quote a string as a C literal. Non-printable and non-ASCII bytes become octal escapes.
pub fn c_str(s: &str) -> String {
    let mut out = String::from("\"");
    for b in s.bytes() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e if b != b'?' => out.push(b as char),
            _ => {
                let _ = write!(out, "\\{b:03o}");
            }
        }
    }
    out.push('"');
    out
}

/// Unique C tag for handler `h` of agent `a`. The agent name is length-prefixed so
/// `A_b` + `c` and `A` + `b_c` cannot collide (both would be `A_b_c`).
fn htag(a: &str, h: &str) -> String {
    format!("{}{a}_{h}", a.len())
}

/// Injective, identifier-safe spelling of a concrete type.
pub fn mangle(t: &Type) -> String {
    match t {
        Type::I64 => "l".into(),
        Type::Bool => "b".into(),
        Type::Str => "s".into(),
        Type::Unit => "u".into(),
        Type::Agent(n) => format!("A{}{n}", n.len()),
        Type::Slice(x) => format!("S{}", mangle(x)),
        Type::Struct(n, a) | Type::Enum(n, a) => {
            let mut s = format!("{}{n}", n.len());
            if !a.is_empty() {
                s.push('I');
                for x in a {
                    s.push_str(&mangle(x));
                }
                s.push('E');
            }
            s
        }
        Type::Named(..) | Type::Param(_) | Type::Infer(_) => unreachable!("not concrete: {t}"),
    }
}

/// C type of an `extern "C"` parameter or result.
fn c_abi_ty(t: &Type) -> &'static str {
    match t {
        Type::I64 => "long",
        Type::Bool => "bool",
        Type::Str => "const char *",
        _ => "void",
    }
}

#[derive(Clone)]
struct Deferred {
    body: Block,
    locals: Vec<HashMap<String, String>>,
}

struct Gen<'a> {
    ck: &'a Checked,
    fns: HashMap<&'a str, &'a FnDecl>,
    externs: HashSet<&'a str>,
    structs: HashMap<&'a str, &'a StructDecl>,
    enums: HashMap<&'a str, &'a EnumDecl>,
    /// Current output buffer (one function body at a time).
    out: String,
    /// Registered struct/enum instances: C name -> concrete type, in first-use order.
    types: Vec<(String, Type)>,
    type_set: HashSet<String>,
    /// Generic function instances still to emit.
    queue: VecDeque<(String, Vec<Type>)>,
    queued: HashSet<String>,
    protos: Vec<String>,
    /// Type-parameter substitution of the instance being emitted.
    map: HashMap<String, Type>,
    /// Scopes of locals: .dov name -> C name.
    locals: Vec<HashMap<String, String>>,
    /// Deferred blocks, one list per open block.
    defers: Vec<Vec<Deferred>>,
    /// Counter for fresh C names (shadowing bindings, temporaries).
    fresh: usize,
    /// Field names of the agent whose handler is being emitted.
    fields: HashSet<String>,
    indent: usize,
}

pub fn generate(ck: &Checked) -> String {
    let mut g = Gen {
        ck,
        fns: HashMap::new(),
        externs: HashSet::new(),
        structs: HashMap::new(),
        enums: HashMap::new(),
        out: String::new(),
        types: Vec::new(),
        type_set: HashSet::new(),
        queue: VecDeque::new(),
        queued: HashSet::new(),
        protos: Vec::new(),
        map: HashMap::new(),
        locals: Vec::new(),
        defers: Vec::new(),
        fresh: 0,
        fields: HashSet::new(),
        indent: 0,
    };
    for it in &ck.prog.items {
        match it {
            Item::Fn(f) => {
                g.fns.insert(&f.name, f);
            }
            Item::Extern(e) => {
                g.externs.insert(&e.name);
            }
            Item::Struct(s) => {
                g.structs.insert(&s.name, s);
            }
            Item::Enum(e) => {
                g.enums.insert(&e.name, e);
            }
            _ => {}
        }
    }
    g.program()
}

impl<'a> Gen<'a> {
    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn take(&mut self) -> String {
        std::mem::take(&mut self.out)
    }

    fn tmp(&mut self, what: &str) -> String {
        self.fresh += 1;
        format!("dov_{what}{}", self.fresh)
    }

    // ---------- types ----------

    fn concrete(&self, t: &Type) -> Type {
        subst(t, &self.map)
    }

    /// Type of expression `e` in the current instance.
    fn ty_of(&self, e: &Expr) -> Type {
        self.concrete(&self.ck.info.types[&e.pos])
    }

    /// C type for a type of the current instance; registers struct and enum instances.
    fn cty(&mut self, t: &Type) -> String {
        let t = self.concrete(t);
        match &t {
            Type::I64 => "int64_t".into(),
            Type::Bool => "bool".into(),
            Type::Str => "dov_str".into(),
            Type::Unit => "void".into(),
            Type::Agent(a) => format!("dov_h_{a}"),
            Type::Slice(x) => {
                self.cty(x);
                "dov_slice".into()
            }
            Type::Struct(n, args) | Type::Enum(n, args) => {
                let name = format!("dov_t_{}", mangle(&t));
                if self.type_set.insert(name.clone()) {
                    self.types.push((name.clone(), t.clone()));
                    for m in self.members(n, args, matches!(t, Type::Enum(..))) {
                        self.cty(&m);
                    }
                }
                name
            }
            Type::Named(..) | Type::Param(_) | Type::Infer(_) => unreachable!("not concrete: {t}"),
        }
    }

    /// Concrete field/payload types of a struct or enum instance.
    fn members(&self, n: &str, args: &[Type], is_enum: bool) -> Vec<Type> {
        if is_enum {
            let e = self.enums[n];
            let map = e
                .tparams
                .iter()
                .cloned()
                .zip(args.iter().cloned())
                .collect();
            e.variants
                .iter()
                .flat_map(|v| v.fields.iter().map(|t| subst(t, &map)).collect::<Vec<_>>())
                .collect()
        } else {
            let s = self.structs[n];
            let map = s
                .tparams
                .iter()
                .cloned()
                .zip(args.iter().cloned())
                .collect();
            s.fields.iter().map(|f| subst(&f.ty, &map)).collect()
        }
    }

    fn variants(&self, t: &Type) -> Vec<(String, Vec<Type>)> {
        let Type::Enum(n, args) = t else {
            unreachable!("not an enum: {t}")
        };
        let e = self.enums[n.as_str()];
        let map = e
            .tparams
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        e.variants
            .iter()
            .map(|v| {
                (
                    v.name.clone(),
                    v.fields.iter().map(|t| subst(t, &map)).collect(),
                )
            })
            .collect()
    }

    /// Struct and enum definitions in dependency order (by-value members first).
    fn type_defs(&mut self) -> String {
        let mut out = String::new();
        // Emitting a definition can register no new types: every member type was
        // registered with its parent. So `types` is final here.
        let all: Vec<(String, Type)> = self.types.clone();
        for (name, _) in &all {
            let _ = writeln!(out, "typedef struct {name} {name};");
        }
        let mut done = HashSet::new();
        for (name, t) in &all {
            self.emit_type(name, t, &all, &mut done, &mut out);
        }
        out
    }

    fn emit_type(
        &mut self,
        name: &str,
        t: &Type,
        all: &[(String, Type)],
        done: &mut HashSet<String>,
        out: &mut String,
    ) {
        if !done.insert(name.to_string()) {
            return;
        }
        let (n, args, is_enum) = match t {
            Type::Struct(n, a) => (n, a, false),
            Type::Enum(n, a) => (n, a, true),
            _ => unreachable!(),
        };
        for m in self.members(n, args, is_enum) {
            if let Type::Struct(..) | Type::Enum(..) = m {
                let dep = format!("dov_t_{}", mangle(&m));
                if let Some((dn, dt)) = all.iter().find(|(x, _)| *x == dep) {
                    self.emit_type(dn, dt, all, done, out);
                }
            }
        }
        let _ = writeln!(out, "struct {name} {{ /* {t} */");
        if is_enum {
            let _ = writeln!(out, "    int32_t tag;");
            let vs = self.variants(t);
            if vs.iter().any(|(_, ps)| !ps.is_empty()) {
                let _ = writeln!(out, "    union {{");
                for (v, ps) in &vs {
                    if ps.is_empty() {
                        continue;
                    }
                    let _ = write!(out, "        struct {{");
                    for (i, p) in ps.iter().enumerate() {
                        let c = self.cty(p);
                        let _ = write!(out, " {c} a{i};");
                    }
                    let _ = writeln!(out, " }} v_{v};");
                }
                let _ = writeln!(out, "    }} u;");
            }
        } else {
            let s = self.structs[n.as_str()];
            if s.fields.is_empty() {
                let _ = writeln!(out, "    char dov_unused;");
            }
            let map = s
                .tparams
                .iter()
                .cloned()
                .zip(args.iter().cloned())
                .collect();
            for f in &s.fields {
                let c = self.cty(&subst(&f.ty, &map));
                let _ = writeln!(out, "    {c} f_{};", f.name);
            }
        }
        let _ = writeln!(out, "}};");
    }

    // ---------- program ----------

    fn fn_cname(name: &str, targs: &[Type]) -> String {
        if targs.is_empty() {
            format!("dov_fn_{name}")
        } else {
            let m: String = targs.iter().map(mangle).collect();
            format!("dov_fn_{name}__{m}")
        }
    }

    fn params(&mut self, ps: &[Param], prefix: &str) -> String {
        let mut v: Vec<String> = Vec::new();
        if !prefix.is_empty() {
            v.push(prefix.to_string());
        }
        for p in ps {
            let c = self.cty(&p.ty);
            v.push(format!("{c} v_{}", p.name));
        }
        if v.is_empty() {
            "void".into()
        } else {
            v.join(", ")
        }
    }

    fn program(&mut self) -> String {
        let items = &self.ck.prog.items;
        let agents: Vec<&AgentDecl> = items
            .iter()
            .filter_map(|i| {
                if let Item::Agent(a) = i {
                    Some(a)
                } else {
                    None
                }
            })
            .collect();

        // Agent state, message structs, trampolines and spawn functions.
        let mut agent_defs = String::new();
        let mut agent_code = String::new();
        for a in &agents {
            let _ = writeln!(agent_defs, "struct dov_agent_{} {{", a.name);
            if a.fields.is_empty() {
                let _ = writeln!(agent_defs, "    char dov_unused;");
            }
            for f in &a.fields {
                let c = self.cty(f.ty.as_ref().expect("checked field type"));
                let _ = writeln!(agent_defs, "    {c} f_{};", f.name);
            }
            let _ = writeln!(agent_defs, "}};");
            self.protos.push(format!(
                "DOV_UNUSED static dov_h_{0} dov_spawn_{0}(void);",
                a.name
            ));
            for h in &a.handlers {
                let tag = htag(&a.name, &h.name);
                let selfp = format!("dov_h_{} self", a.name);
                let ps = self.params(&h.params, &selfp);
                self.protos
                    .push(format!("DOV_UNUSED static void dov_on_{tag}({ps});"));
                if !h.params.is_empty() {
                    let _ = writeln!(agent_code, "typedef struct {{");
                    for (i, p) in h.params.iter().enumerate() {
                        let c = self.cty(&p.ty);
                        let _ = writeln!(agent_code, "    {c} a{i};");
                    }
                    let _ = writeln!(agent_code, "}} dov_msg_{tag};");
                }
                let _ = writeln!(
                    agent_code,
                    "DOV_UNUSED static void dov_tramp_{tag}(void *self, void *args) {{"
                );
                if h.params.is_empty() {
                    let _ = writeln!(agent_code, "    (void)args;");
                    let _ = writeln!(agent_code, "    dov_on_{tag}((dov_h_{})self);", a.name);
                } else {
                    let _ = writeln!(agent_code, "    dov_msg_{tag} *m = (dov_msg_{tag} *)args;");
                    let args: Vec<String> =
                        (0..h.params.len()).map(|i| format!("m->a{i}")).collect();
                    let _ = writeln!(
                        agent_code,
                        "    dov_on_{tag}((dov_h_{})self, {});",
                        a.name,
                        args.join(", ")
                    );
                }
                let _ = writeln!(agent_code, "}}");
            }
            self.map = HashMap::new();
            self.locals = vec![HashMap::new()];
            self.defers = vec![Vec::new()];
            self.fields = HashSet::new();
            self.indent = 1;
            self.line(&format!(
                "dov_h_{0} self = (dov_h_{0})dov_alloc(sizeof *self);",
                a.name
            ));
            for f in &a.fields {
                let v = self.expr(&f.init);
                self.line(&format!("self->f_{} = {v};", f.name));
            }
            self.line("return self;");
            self.indent = 0;
            let body = self.take();
            let _ = writeln!(
                agent_code,
                "static dov_h_{0} dov_spawn_{0}(void) {{\n{body}}}",
                a.name
            );
        }

        // Function bodies: every non-generic fn, every handler, then generic instances.
        let mut bodies = String::new();
        for it in items {
            if let Item::Fn(f) = it {
                if f.tparams.is_empty() {
                    self.queued.insert(Self::fn_cname(&f.name, &[]));
                    let b = self.function(f, &[]);
                    bodies.push_str(&b);
                }
            }
        }
        for a in &agents {
            for h in &a.handlers {
                let b = self.handler(a, h);
                bodies.push_str(&b);
            }
        }
        while let Some((name, targs)) = self.queue.pop_front() {
            let f = self.fns[name.as_str()];
            let b = self.function(f, &targs);
            bodies.push_str(&b);
        }

        let mut out = String::new();
        out.push_str(concat!(
            "/* generated by dovc ",
            env!("CARGO_PKG_VERSION"),
            " — .dov pLang */\n#include \"dov_rt.h\"\n\n"
        ));
        for a in &agents {
            let _ = writeln!(
                out,
                "typedef struct dov_agent_{0} dov_agent_{0};\ntypedef dov_agent_{0} *dov_h_{0};",
                a.name
            );
        }
        let defs = self.type_defs();
        out.push_str(&defs);
        out.push_str(&agent_defs);
        for it in items {
            if let Item::Extern(e) = it {
                let ps: Vec<&str> = e.params.iter().map(|p| c_abi_ty(&p.ty)).collect();
                let ps = if ps.is_empty() {
                    "void".into()
                } else {
                    ps.join(", ")
                };
                let _ = writeln!(out, "{} {}({ps});", c_abi_ty(&e.ret), e.name);
            }
        }
        for p in &self.protos {
            out.push_str(p);
            out.push('\n');
        }
        out.push('\n');
        out.push_str(&agent_code);
        out.push('\n');
        out.push_str(&bodies);
        out.push_str("\nint main(void) {\n    dov_fn_main();\n    dov_run();\n    return 0;\n}\n");
        out
    }

    fn function(&mut self, f: &FnDecl, targs: &[Type]) -> String {
        self.map = f
            .tparams
            .iter()
            .cloned()
            .zip(targs.iter().cloned())
            .collect();
        let name = Self::fn_cname(&f.name, targs);
        let ret = self.cty(&f.ret);
        let ps = self.params(&f.params, "");
        self.protos
            .push(format!("DOV_UNUSED static {ret} {name}({ps});"));
        self.fields = HashSet::new();
        self.locals = vec![Self::param_scope(&f.params)];
        self.defers = Vec::new();
        self.indent = 0;
        self.line(&format!("static {ret} {name}({ps}) {{"));
        self.indent = 1;
        for p in &f.params {
            self.line(&format!("(void)v_{};", p.name));
        }
        self.block_body(&f.body);
        if f.ret != Type::Unit {
            self.line(&format!(
                "dov_panic(\"missing return in `{}`\", {});",
                f.name, f.pos.line
            ));
        }
        self.indent = 0;
        self.line("}");
        self.take()
    }

    fn handler(&mut self, a: &AgentDecl, h: &Handler) -> String {
        self.map = HashMap::new();
        self.fields = a.fields.iter().map(|f| f.name.clone()).collect();
        self.locals = vec![Self::param_scope(&h.params)];
        self.defers = Vec::new();
        let selfp = format!("dov_h_{} self", a.name);
        let ps = self.params(&h.params, &selfp);
        self.indent = 0;
        self.line(&format!(
            "static void dov_on_{}({ps}) {{",
            htag(&a.name, &h.name)
        ));
        self.indent = 1;
        self.line("(void)self;");
        for p in &h.params {
            self.line(&format!("(void)v_{};", p.name));
        }
        self.block_body(&h.body);
        self.indent = 0;
        self.line("}");
        self.take()
    }

    fn param_scope(ps: &[Param]) -> HashMap<String, String> {
        ps.iter()
            .map(|p| (p.name.clone(), format!("v_{}", p.name)))
            .collect()
    }

    // ---------- statements ----------

    /// Statements of a block at the current indent, with its own scope and defers.
    fn block_body(&mut self, b: &Block) {
        self.locals.push(HashMap::new());
        self.defers.push(Vec::new());
        for s in b {
            self.stmt(s);
        }
        let mut ds = self.defers.pop().unwrap();
        ds.reverse();
        self.run_defers(&ds);
        self.locals.pop();
    }

    fn block(&mut self, b: &Block) {
        self.indent += 1;
        self.block_body(b);
        self.indent -= 1;
    }

    /// Emit deferred blocks in the given order, each with the scope it was written in.
    fn run_defers(&mut self, ds: &[Deferred]) {
        for d in ds {
            let saved_locals = std::mem::replace(&mut self.locals, d.locals.clone());
            let saved_defers = std::mem::take(&mut self.defers);
            self.line("{");
            self.block(&d.body);
            self.line("}");
            self.defers = saved_defers;
            self.locals = saved_locals;
        }
    }

    fn bind(&mut self, name: &str) -> String {
        // A shadowing binding gets its own C name. In C the new name is already in
        // scope inside its own initializer, so `let x = x + 1;` would read itself.
        // `v<N>_x` cannot clash with a user name, which is always `v_<name>`.
        let cname = if self.locals.iter().any(|s| s.contains_key(name)) {
            self.fresh += 1;
            format!("v{}_{name}", self.fresh)
        } else {
            format!("v_{name}")
        };
        self.locals
            .last_mut()
            .unwrap()
            .insert(name.to_string(), cname.clone());
        cname
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let {
                name,
                mutable,
                init,
                ..
            } => {
                let t = self.ty_of(init);
                let ct = self.cty(&t);
                let v = self.expr(init);
                let cname = self.bind(name);
                // `let` becomes a C const object, so C checks immutability a second time.
                let decl = if *mutable {
                    format!("{ct} {cname}")
                } else {
                    format!("const {ct} {cname}")
                };
                self.line(&format!("{decl} = {v};"));
                // An unused binding is legal .dov; keep strict C (-Wunused-variable) quiet.
                self.line(&format!("(void){cname};"));
            }
            StmtKind::Assign { target, value } => {
                let t = self.place(target);
                let v = self.expr(value);
                self.line(&format!("{t} = {v};"));
            }
            StmtKind::If { cond, then, els } => {
                let c = Self::cond(self.expr(cond));
                self.line(&format!("if ({c}) {{"));
                self.block(then);
                if let Some(e) = els {
                    self.line("} else {");
                    self.block(e);
                }
                self.line("}");
            }
            StmtKind::While { cond, body } => {
                let c = Self::cond(self.expr(cond));
                self.line(&format!("while ({c}) {{"));
                self.block(body);
                self.line("}");
            }
            StmtKind::Return(v) => {
                // Every enclosing block's defers, innermost and latest first.
                let all: Vec<Deferred> = self
                    .defers
                    .iter()
                    .rev()
                    .flat_map(|l| l.iter().rev().cloned())
                    .collect();
                match v {
                    None if all.is_empty() => self.line("return;"),
                    None => {
                        self.line("{");
                        self.indent += 1;
                        self.run_defers(&all);
                        self.line("return;");
                        self.indent -= 1;
                        self.line("}");
                    }
                    Some(e) if all.is_empty() => {
                        let v = self.expr(e);
                        self.line(&format!("return {v};"));
                    }
                    Some(e) => {
                        let t = self.ty_of(e);
                        let ct = self.cty(&t);
                        let v = self.expr(e);
                        let r = self.tmp("ret");
                        self.line("{");
                        self.indent += 1;
                        self.line(&format!("const {ct} {r} = {v};"));
                        self.run_defers(&all);
                        self.line(&format!("return {r};"));
                        self.indent -= 1;
                        self.line("}");
                    }
                }
            }
            StmtKind::Expr(e) => {
                let v = self.expr(e);
                self.line(&format!("(void)({v});"));
            }
            StmtKind::Send {
                target,
                handler,
                args,
                ..
            } => {
                let Type::Agent(a) = self.ty_of(target) else {
                    unreachable!("checked")
                };
                let tag = htag(&a, handler);
                let t = self.expr(target);
                self.line("{");
                if args.is_empty() {
                    self.line(&format!("    dov_send({t}, dov_tramp_{tag}, NULL);"));
                } else {
                    self.line(&format!(
                        "    dov_msg_{tag} *m = (dov_msg_{tag} *)dov_alloc(sizeof *m);"
                    ));
                    for (i, arg) in args.iter().enumerate() {
                        let v = self.expr(arg);
                        self.line(&format!("    m->a{i} = {v};"));
                    }
                    self.line(&format!("    dov_send({t}, dov_tramp_{tag}, m);"));
                }
                self.line("}");
            }
            StmtKind::Match { scrut, arms } => self.match_stmt(scrut, arms),
            StmtKind::Defer(b) => {
                let d = Deferred {
                    body: b.clone(),
                    locals: self.locals.clone(),
                };
                self.defers.last_mut().unwrap().push(d);
            }
        }
    }

    /// An assignable C lvalue for a name or field chain.
    fn place(&mut self, e: &Expr) -> String {
        match &e.kind {
            ExprKind::Var(n) => self.var(n),
            ExprKind::Field(b, f) => format!("{}.f_{f}", self.place(b)),
            _ => unreachable!("checked place"),
        }
    }

    fn match_stmt(&mut self, scrut: &Expr, arms: &[Arm]) {
        let st = self.ty_of(scrut);
        let ct = self.cty(&st);
        let v = self.expr(scrut);
        let m = self.tmp("m");
        self.line("{");
        self.indent += 1;
        self.line(&format!("const {ct} {m} = {v};"));
        self.line(&format!("(void){m};"));
        if let Type::Enum(..) = st {
            let vs = self.variants(&st);
            self.line(&format!("switch ({m}.tag) {{"));
            for arm in arms {
                self.locals.push(HashMap::new());
                match &arm.pat {
                    Pattern::Variant { variant, binds, .. } => {
                        let idx = vs.iter().position(|(n, _)| n == variant).unwrap();
                        self.line(&format!("case {idx}: {{"));
                        self.indent += 1;
                        let payload = vs[idx].1.clone();
                        for (i, (b, t)) in binds.iter().zip(&payload).enumerate() {
                            if let Some((name, _)) = b {
                                let c = self.cty(t);
                                let cname = self.bind(name);
                                self.line(&format!("const {c} {cname} = {m}.u.v_{variant}.a{i};"));
                                self.line(&format!("(void){cname};"));
                            }
                        }
                    }
                    _ => {
                        self.line("default: {");
                        self.indent += 1;
                    }
                }
                self.block_body(&arm.body);
                self.line("break;");
                self.indent -= 1;
                self.line("}");
                self.locals.pop();
            }
            self.line("}");
        } else {
            for (k, arm) in arms.iter().enumerate() {
                let test = match &arm.pat {
                    Pattern::Int(i) => Some(format!("{m} == INT64_C({i})")),
                    Pattern::Bool(b) => Some(format!("{m} == {b}")),
                    Pattern::Str(s) => Some(format!("dov_str_eq({m}, {})", Self::str_lit(s))),
                    _ => None,
                };
                let head = match (k, test) {
                    (0, Some(t)) => format!("if ({t}) {{"),
                    (_, Some(t)) => format!("}} else if ({t}) {{"),
                    (0, None) => "{".into(),
                    (_, None) => "} else {".into(),
                };
                self.line(&head);
                self.block(&arm.body);
            }
            if !arms.is_empty() {
                self.line("}");
            }
        }
        self.indent -= 1;
        self.line("}");
    }

    /// Drop one redundant outer paren pair so clang's -Wparentheses-equality stays quiet.
    fn cond(c: String) -> String {
        if c.starts_with('(') && c.ends_with(')') {
            let inner = &c[1..c.len() - 1];
            let mut depth = 0i32;
            let balanced = inner.chars().all(|ch| {
                match ch {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                depth >= 0
            });
            if balanced && depth == 0 && !inner.contains('"') {
                return inner.to_string();
            }
        }
        c
    }

    fn var(&self, name: &str) -> String {
        if let Some(c) = self.locals.iter().rev().find_map(|s| s.get(name)) {
            c.clone()
        } else if self.fields.contains(name) {
            format!("self->f_{name}")
        } else {
            format!("v_{name}")
        }
    }

    fn str_lit(s: &str) -> String {
        format!("DOV_S({}, {})", c_str(s), s.len())
    }

    // ---------- expressions ----------

    fn variant(&mut self, t: &Type, v: &str, args: &[Expr]) -> String {
        let ct = self.cty(t);
        let vs = self.variants(t);
        let idx = vs.iter().position(|(n, _)| n == v).unwrap();
        if args.is_empty() {
            return format!("(({ct}){{ .tag = {idx} }})");
        }
        let vals: Vec<String> = args
            .iter()
            .enumerate()
            .map(|(i, a)| format!(".a{i} = {}", self.expr(a)))
            .collect();
        format!(
            "(({ct}){{ .tag = {idx}, .u.v_{v} = {{ {} }} }})",
            vals.join(", ")
        )
    }

    fn expr(&mut self, e: &Expr) -> String {
        let line = e.pos.line;
        match &e.kind {
            ExprKind::Int(v) => format!("INT64_C({v})"),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Str(s) => Self::str_lit(s),
            ExprKind::Var(n) => {
                let known =
                    self.locals.iter().any(|s| s.contains_key(n)) || self.fields.contains(n);
                if !known && self.ck.bare.contains_key(n) {
                    let t = self.ty_of(e);
                    self.variant(&t, n, &[])
                } else {
                    self.var(n)
                }
            }
            ExprKind::Unary(UnOp::Neg, x) => format!("dov_neg({}, {line})", self.expr(x)),
            ExprKind::Unary(UnOp::Not, x) => format!("(!{})", self.expr(x)),
            ExprKind::Binary(op, l, r) => {
                let (a, b) = (self.expr(l), self.expr(r));
                match op {
                    BinOp::Add => format!("dov_add({a}, {b}, {line})"),
                    BinOp::Sub => format!("dov_sub({a}, {b}, {line})"),
                    BinOp::Mul => format!("dov_mul({a}, {b}, {line})"),
                    BinOp::Div => format!("dov_div({a}, {b}, {line})"),
                    BinOp::Rem => format!("dov_rem({a}, {b}, {line})"),
                    BinOp::Eq | BinOp::Ne => {
                        let neg = *op == BinOp::Ne;
                        if self.ty_of(l) == Type::Str {
                            format!("({}dov_str_eq({a}, {b}))", if neg { "!" } else { "" })
                        } else {
                            format!("({a} {} {b})", if neg { "!=" } else { "==" })
                        }
                    }
                    BinOp::Lt => format!("({a} < {b})"),
                    BinOp::Le => format!("({a} <= {b})"),
                    BinOp::Gt => format!("({a} > {b})"),
                    BinOp::Ge => format!("({a} >= {b})"),
                    BinOp::And => format!("({a} && {b})"),
                    BinOp::Or => format!("({a} || {b})"),
                }
            }
            ExprKind::Call(name, args) if self.fns.contains_key(name.as_str()) => {
                let vs: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
                let targs: Vec<Type> = match self.ck.info.targs.get(&e.pos) {
                    Some(ts) => ts.iter().map(|t| self.concrete(t)).collect(),
                    None => Vec::new(),
                };
                let cname = Self::fn_cname(name, &targs);
                if !targs.is_empty() && self.queued.insert(cname.clone()) {
                    self.queue.push_back((name.clone(), targs));
                }
                format!("{cname}({})", vs.join(", "))
            }
            ExprKind::Call(name, args) if self.externs.contains(name.as_str()) => {
                let vs: Vec<String> = args
                    .iter()
                    .map(|a| {
                        let v = self.expr(a);
                        if self.ty_of(a) == Type::Str {
                            format!("({v}).p")
                        } else {
                            v
                        }
                    })
                    .collect();
                let call = format!("{name}({})", vs.join(", "));
                match self.ty_of(e) {
                    Type::Str => format!("dov_str_from_c({call})"),
                    Type::I64 => format!("((int64_t){call})"),
                    _ => call,
                }
            }
            ExprKind::Call(name, args) if self.ck.bare.contains_key(name.as_str()) => {
                let t = self.ty_of(e);
                self.variant(&t, name, args)
            }
            ExprKind::Call(name, args) => self.builtin(name, args, e),
            ExprKind::Spawn(a) => format!("dov_spawn_{a}()"),
            ExprKind::StructLit(_, fields) => {
                let t = self.ty_of(e);
                let ct = self.cty(&t);
                if fields.is_empty() {
                    return format!("(({ct}){{ 0 }})");
                }
                let vals: Vec<String> = fields
                    .iter()
                    .map(|(f, v)| format!(".f_{f} = {}", self.expr(v)))
                    .collect();
                format!("(({ct}){{ {} }})", vals.join(", "))
            }
            ExprKind::Field(b, f) => format!("({}).f_{f}", self.expr(b)),
            ExprKind::Variant(_, v, args) => {
                let t = self.ty_of(e);
                self.variant(&t, v, args)
            }
            ExprKind::Array(items) => {
                let Type::Slice(el) = self.ty_of(e) else {
                    unreachable!()
                };
                let ct = self.cty(&el);
                if items.is_empty() {
                    return "dov_slice_empty()".into();
                }
                let vals: Vec<String> = items.iter().map(|i| self.expr(i)).collect();
                format!(
                    "dov_slice_lit({}, sizeof({ct}), ({ct}[]){{ {} }})",
                    items.len(),
                    vals.join(", ")
                )
            }
            ExprKind::Index(b, i) => {
                let bt = self.ty_of(b);
                let (bv, iv) = (self.expr(b), self.expr(i));
                match bt {
                    Type::Str => format!("dov_str_at({bv}, {iv}, {line})"),
                    Type::Slice(el) => {
                        let ct = self.cty(&el);
                        format!("DOV_AT({ct}, {bv}, {iv}, {line})")
                    }
                    _ => unreachable!(),
                }
            }
            ExprKind::Range(b, x, y) => {
                let bt = self.ty_of(b);
                let (bv, xv, yv) = (self.expr(b), self.expr(x), self.expr(y));
                if bt == Type::Str {
                    format!("dov_str_range({bv}, {xv}, {yv}, {line})")
                } else {
                    format!("dov_range({bv}, {xv}, {yv}, {line})")
                }
            }
        }
    }

    fn builtin(&mut self, name: &str, args: &[Expr], e: &Expr) -> String {
        let line = e.pos.line;
        match name {
            "print" => {
                let mut parts = Vec::new();
                for a in args {
                    let v = self.expr(a);
                    parts.push(match self.ty_of(a) {
                        Type::I64 => format!("dov_print_i64({v})"),
                        Type::Bool => format!("dov_print_bool({v})"),
                        _ => format!("dov_print_str({v})"),
                    });
                }
                parts.push("dov_print_nl()".into());
                format!("({})", parts.join(", "))
            }
            "panic" => format!("dov_panic_str({}, {line})", self.expr(&args[0])),
            "len" => format!("({}).len", self.expr(&args[0])),
            "append" => {
                let Type::Slice(el) = self.ty_of(e) else {
                    unreachable!()
                };
                let ct = self.cty(&el);
                let (xs, x) = (self.expr(&args[0]), self.expr(&args[1]));
                format!("dov_append({xs}, sizeof({ct}), ({ct}[]){{ {x} }})")
            }
            "concat" => {
                let t = self.ty_of(e);
                let (a, b) = (self.expr(&args[0]), self.expr(&args[1]));
                match t {
                    Type::Str => format!("dov_str_concat({a}, {b})"),
                    Type::Slice(el) => {
                        let ct = self.cty(&el);
                        format!("dov_concat({a}, {b}, sizeof({ct}))")
                    }
                    _ => unreachable!(),
                }
            }
            "to_str" => {
                let v = self.expr(&args[0]);
                match self.ty_of(&args[0]) {
                    Type::I64 => format!("dov_i64_to_str({v})"),
                    Type::Bool => format!("dov_bool_to_str({v})"),
                    _ => v,
                }
            }
            "char_of" => format!("dov_char_of({}, {line})", self.expr(&args[0])),
            _ => unreachable!("builtin {name}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_string_escapes() {
        assert_eq!(c_str("a\"b\\c\n"), r#""a\"b\\c\n""#);
        assert_eq!(c_str("é"), r#""\303\251""#);
        assert_eq!(c_str("??="), r#""\077\077=""#);
    }

    #[test]
    fn mangling_is_injective_on_samples() {
        let a = Type::Struct("A".into(), vec![Type::I64]);
        let b = Type::Struct("A".into(), vec![Type::Slice(Box::new(Type::I64))]);
        let c = Type::Enum("AI".into(), vec![]);
        let names: HashSet<String> = [&a, &b, &c].iter().map(|t| mangle(t)).collect();
        assert_eq!(names.len(), 3);
        assert_eq!(mangle(&a), "1AIlE");
    }
}
