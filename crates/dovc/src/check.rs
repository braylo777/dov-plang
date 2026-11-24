//! Semantic checks: name resolution, types (with generics), exhaustiveness, immutability.
//!
//! `check` first resolves every written type (`Type::Named`) into its real form, then checks
//! all bodies. It returns the resolved program and an `Info` table for codegen. Generic bodies
//! are checked once with opaque `Type::Param`s; codegen substitutes concrete types per instance.

use crate::ast::*;
use crate::diag::{closest, Diag, Pos, Result};
use std::collections::{HashMap, HashSet};

/// What codegen needs to know about expressions.
#[derive(Debug, Default, Clone)]
pub struct Info {
    /// Type of every expression, keyed by its start position (may contain `Param`).
    pub types: HashMap<Pos, Type>,
    /// Type arguments of each call to a generic function, keyed by the call position.
    pub targs: HashMap<Pos, Vec<Type>>,
}

/// Built-in functions. They cannot be redefined.
pub const BUILTINS: &[&str] = &[
    "print", "panic", "len", "append", "concat", "to_str", "char_of",
];

/// C11 keywords (plus the C23 ones compilers already reserve). `extern "C"` names are
/// emitted unprefixed, so these cannot be used for them.
const C_KEYWORDS: &[&str] = &[
    "auto",
    "break",
    "case",
    "char",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extern",
    "float",
    "for",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "register",
    "restrict",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "typedef",
    "union",
    "unsigned",
    "void",
    "volatile",
    "while",
    "_Alignas",
    "_Alignof",
    "_Atomic",
    "_Bool",
    "_Complex",
    "_Generic",
    "_Imaginary",
    "_Noreturn",
    "_Static_assert",
    "_Thread_local",
    "alignas",
    "alignof",
    "constexpr",
    "nullptr",
    "static_assert",
    "thread_local",
    "typeof",
    "typeof_unqual",
];

/// Would an unprefixed extern name collide with an identifier in the emitted C?
fn clashes_with_generated_c(n: &str) -> bool {
    const EXACT: &[&str] = &[
        "self",
        "args",
        "m",
        "NULL",
        "bool",
        "true",
        "false",
        "size_t",
        "ptrdiff_t",
        "wchar_t",
        "max_align_t",
        "offsetof",
        "intptr_t",
        "uintptr_t",
        "intmax_t",
        "uintmax_t",
        "__bool_true_false_are_defined",
    ];
    let b = n.as_bytes();
    let v_numbered = b.len() > 1 && b[0] == b'v' && b[1].is_ascii_digit();
    let stdint_type = (n.starts_with("int") || n.starts_with("uint")) && n.ends_with("_t");
    let stdint_macro = [
        "INT",
        "UINT",
        "PTRDIFF_",
        "SIZE_",
        "WCHAR_",
        "SIG_ATOMIC_",
        "WINT_",
    ]
    .iter()
    .any(|p| n.starts_with(p));
    EXACT.contains(&n)
        || n.starts_with("v_")
        || n.starts_with("f_")
        || n.starts_with("DOV_")
        || v_numbered
        || stdint_type
        || stdint_macro
}

pub struct FnSig {
    pub tparams: Vec<String>,
    pub params: Vec<Type>,
    pub ret: Type,
    pub is_extern: bool,
}

pub struct StructSig {
    pub tparams: Vec<String>,
    pub fields: Vec<(String, Type)>,
}

pub struct EnumSig {
    pub tparams: Vec<String>,
    pub variants: Vec<(String, Vec<Type>)>,
}

struct AgentSig {
    fields: HashMap<String, (Type, bool)>,
    handlers: HashMap<String, Vec<Type>>,
}

/// Enums whose variants may be written without `Enum::` (the prelude).
const BARE_ENUMS: &[&str] = &["Option", "Result"];

#[derive(Clone, PartialEq)]
enum Ctx {
    Fn,
    Handler(String),
}

struct Checker {
    fns: HashMap<String, FnSig>,
    structs: HashMap<String, StructSig>,
    enums: HashMap<String, EnumSig>,
    agents: HashMap<String, AgentSig>,
    /// bare variant name -> enum name (prelude enums only)
    bare: HashMap<String, String>,
    /// name -> (type, mutable, is_parameter)
    scopes: Vec<HashMap<String, (Type, bool, bool)>>,
    ctx: Ctx,
    ret: Type,
    tparams: Vec<String>,
    in_defer: usize,
    infer: Vec<Option<Type>>,
    info: Info,
    /// Positions recorded in `info` since the last `finish_infer`.
    pending: Vec<Pos>,
    pending_targs: Vec<Pos>,
    /// Display names of the source files (for "already defined at" messages).
    files: Vec<String>,
}

/// The checked program: resolved items, expression types and the item tables.
pub struct Checked {
    pub prog: Program,
    pub info: Info,
    pub fns: HashMap<String, FnSig>,
    pub structs: HashMap<String, StructSig>,
    pub enums: HashMap<String, EnumSig>,
    /// bare variant name -> enum name
    pub bare: HashMap<String, String>,
}

pub fn check(prog: &Program) -> Result<Checked> {
    check_files(prog, &[])
}

/// Check with the display names of the source files (indexed by `Pos::file`).
pub fn check_files(prog: &Program, files: &[String]) -> Result<Checked> {
    let mut c = Checker {
        fns: HashMap::new(),
        structs: HashMap::new(),
        enums: HashMap::new(),
        agents: HashMap::new(),
        bare: HashMap::new(),
        scopes: Vec::new(),
        ctx: Ctx::Fn,
        ret: Type::Unit,
        tparams: Vec::new(),
        in_defer: 0,
        infer: Vec::new(),
        info: Info::default(),
        pending: Vec::new(),
        pending_targs: Vec::new(),
        files: files.to_vec(),
    };
    let resolved = c.collect(prog)?;
    c.bodies(&resolved)?;
    Ok(Checked {
        prog: resolved,
        info: c.info,
        fns: c.fns,
        structs: c.structs,
        enums: c.enums,
        bare: c.bare,
    })
}

/// Does every path through `b` end in `return`?
fn returns(b: &Block) -> bool {
    b.iter().any(|s| match &s.kind {
        StmtKind::Return(_) => true,
        StmtKind::Expr(Expr {
            kind: ExprKind::Call(n, _),
            ..
        }) => n == "panic",
        StmtKind::If {
            then, els: Some(e), ..
        } => returns(then) && returns(e),
        StmtKind::Match { arms, .. } => !arms.is_empty() && arms.iter().all(|a| returns(&a.body)),
        _ => false,
    })
}

pub fn subst(t: &Type, map: &HashMap<String, Type>) -> Type {
    match t {
        Type::Param(n) => map.get(n).cloned().unwrap_or_else(|| t.clone()),
        Type::Struct(n, a) => Type::Struct(n.clone(), a.iter().map(|x| subst(x, map)).collect()),
        Type::Enum(n, a) => Type::Enum(n.clone(), a.iter().map(|x| subst(x, map)).collect()),
        Type::Slice(x) => Type::Slice(Box::new(subst(x, map))),
        other => other.clone(),
    }
}

fn zip_map(names: &[String], args: &[Type]) -> HashMap<String, Type> {
    names.iter().cloned().zip(args.iter().cloned()).collect()
}

impl Checker {
    // ---------- types ----------

    fn resolve(&self, t: &Type, tps: &[String], pos: Pos) -> Result<Type> {
        Ok(match t {
            Type::Named(n, args) => {
                let arity = |want: usize, what: &str| -> Result<()> {
                    if args.len() != want {
                        return Err(Diag::new(
                            pos,
                            format!(
                                "{what} `{n}` takes {want} type argument(s) but got {}",
                                args.len()
                            ),
                            format!("write `{n}` with exactly {want} type argument(s)"),
                        ));
                    }
                    Ok(())
                };
                let rargs = args
                    .iter()
                    .map(|a| self.resolve(a, tps, pos))
                    .collect::<Result<Vec<_>>>()?;
                if tps.contains(n) {
                    arity(0, "type parameter")?;
                    Type::Param(n.clone())
                } else if self.agents.contains_key(n) {
                    arity(0, "agent")?;
                    Type::Agent(n.clone())
                } else if let Some(s) = self.structs.get(n) {
                    arity(s.tparams.len(), "struct")?;
                    Type::Struct(n.clone(), rargs)
                } else if let Some(e) = self.enums.get(n) {
                    arity(e.tparams.len(), "enum")?;
                    Type::Enum(n.clone(), rargs)
                } else {
                    let names = self
                        .agents
                        .keys()
                        .chain(self.structs.keys())
                        .chain(self.enums.keys())
                        .map(|s| s.as_str())
                        .chain(tps.iter().map(|s| s.as_str()));
                    let hint = match closest(n, names.chain(["i64", "bool", "str"])) {
                        Some(c) => format!("did you mean `{c}`?"),
                        None => "types are `i64`, `bool`, `str`, `()`, `[T]`, or a declared struct, enum or agent".into(),
                    };
                    return Err(Diag::new(pos, format!("unknown type `{n}`"), hint));
                }
            }
            Type::Slice(x) => Type::Slice(Box::new(self.resolve(x, tps, pos)?)),
            other => other.clone(),
        })
    }

    fn fresh(&mut self) -> Type {
        self.infer.push(None);
        Type::Infer(self.infer.len() as u32 - 1)
    }

    /// Replace solved inference variables.
    fn zonk(&self, t: &Type) -> Type {
        match t {
            Type::Infer(i) => match &self.infer[*i as usize] {
                Some(x) => self.zonk(x),
                None => t.clone(),
            },
            Type::Struct(n, a) => Type::Struct(n.clone(), a.iter().map(|x| self.zonk(x)).collect()),
            Type::Enum(n, a) => Type::Enum(n.clone(), a.iter().map(|x| self.zonk(x)).collect()),
            Type::Slice(x) => Type::Slice(Box::new(self.zonk(x))),
            other => other.clone(),
        }
    }

    fn has_infer(t: &Type) -> bool {
        match t {
            Type::Infer(_) => true,
            Type::Struct(_, a) | Type::Enum(_, a) => a.iter().any(Self::has_infer),
            Type::Slice(x) => Self::has_infer(x),
            _ => false,
        }
    }

    fn occurs(&self, v: u32, t: &Type) -> bool {
        match self.zonk(t) {
            Type::Infer(i) => i == v,
            Type::Struct(_, a) | Type::Enum(_, a) => a.iter().any(|x| self.occurs(v, x)),
            Type::Slice(x) => self.occurs(v, &x),
            _ => false,
        }
    }

    /// Make `a` and `b` equal by solving inference variables. Returns false on mismatch.
    fn unify(&mut self, a: &Type, b: &Type) -> bool {
        let (a, b) = (self.zonk(a), self.zonk(b));
        match (&a, &b) {
            (Type::Infer(i), Type::Infer(j)) if i == j => true,
            (Type::Infer(i), t) | (t, Type::Infer(i)) => {
                if self.occurs(*i, t) {
                    return false;
                }
                self.infer[*i as usize] = Some(t.clone());
                true
            }
            (Type::Struct(n, x), Type::Struct(m, y)) | (Type::Enum(n, x), Type::Enum(m, y)) => {
                n == m && x.len() == y.len() && x.iter().zip(y).all(|(p, q)| self.unify(p, q))
            }
            (Type::Slice(x), Type::Slice(y)) => self.unify(x, y),
            _ => a == b,
        }
    }

    // ---------- declarations ----------

    fn collect(&mut self, prog: &Program) -> Result<Program> {
        let mut seen: HashMap<&str, Pos> = HashMap::new();
        for item in &prog.items {
            let Some((name, pos)) = item.name_pos() else {
                continue;
            };
            if BUILTINS.contains(&name) {
                return Err(Diag::new(
                    pos,
                    format!("`{name}` is a built-in and cannot be redefined"),
                    "choose another name",
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
                let at = if prev.file == pos.file {
                    format!("{}:{}", prev.line, prev.col)
                } else {
                    let f = self
                        .files
                        .get(prev.file as usize)
                        .cloned()
                        .unwrap_or_else(|| "another file".into());
                    format!("{f}:{}:{}", prev.line, prev.col)
                };
                return Err(Diag::new(
                    pos,
                    format!("`{name}` is already defined at {at}"),
                    "functions, externs, agents, structs and enums share one namespace; rename one of them",
                ));
            }
        }
        let dup_tparams = |tps: &[String], pos: Pos| -> Result<()> {
            let mut s = HashSet::new();
            for t in tps {
                if !s.insert(t) {
                    return Err(Diag::new(
                        pos,
                        format!("type parameter `{t}` is declared twice"),
                        "give each type parameter its own name",
                    ));
                }
            }
            Ok(())
        };
        // Register type names first so any type can mention any other.
        for item in &prog.items {
            match item {
                Item::Agent(a) => {
                    self.agents.insert(
                        a.name.clone(),
                        AgentSig {
                            fields: HashMap::new(),
                            handlers: HashMap::new(),
                        },
                    );
                }
                Item::Struct(s) => {
                    dup_tparams(&s.tparams, s.pos)?;
                    self.structs.insert(
                        s.name.clone(),
                        StructSig {
                            tparams: s.tparams.clone(),
                            fields: Vec::new(),
                        },
                    );
                }
                Item::Enum(e) => {
                    dup_tparams(&e.tparams, e.pos)?;
                    self.enums.insert(
                        e.name.clone(),
                        EnumSig {
                            tparams: e.tparams.clone(),
                            variants: Vec::new(),
                        },
                    );
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        // Struct and enum bodies.
        for item in &prog.items {
            match item {
                Item::Struct(s) => {
                    let mut fields = Vec::new();
                    let mut names = HashSet::new();
                    for f in &s.fields {
                        if !names.insert(&f.name) {
                            return Err(Diag::new(
                                f.pos,
                                format!("duplicate field `{}`", f.name),
                                "each field in a struct needs its own name",
                            ));
                        }
                        let t = self.resolve(&f.ty, &s.tparams, f.pos)?;
                        self.no_unit_or_agentless(&t, f.pos, "struct fields")?;
                        fields.push((f.name.clone(), t));
                    }
                    let mut rs = s.clone();
                    for (f, (_, t)) in rs.fields.iter_mut().zip(&fields) {
                        f.ty = t.clone();
                    }
                    self.structs.get_mut(&s.name).unwrap().fields = fields;
                    out.push(Item::Struct(rs));
                }
                Item::Enum(e) => {
                    let mut vs = Vec::new();
                    let mut names = HashSet::new();
                    let mut re = e.clone();
                    if e.variants.is_empty() {
                        return Err(Diag::new(
                            e.pos,
                            format!("enum `{}` has no variants", e.name),
                            "add at least one variant, e.g. `enum E { A, B(i64) }`",
                        ));
                    }
                    for (vi, v) in e.variants.iter().enumerate() {
                        if !names.insert(&v.name) {
                            return Err(Diag::new(
                                v.pos,
                                format!("duplicate variant `{}`", v.name),
                                "each variant in an enum needs its own name",
                            ));
                        }
                        let mut ts = Vec::new();
                        for t in &v.fields {
                            let t = self.resolve(t, &e.tparams, v.pos)?;
                            self.no_unit_or_agentless(&t, v.pos, "variant payloads")?;
                            ts.push(t);
                        }
                        re.variants[vi].fields = ts.clone();
                        vs.push((v.name.clone(), ts));
                    }
                    if BARE_ENUMS.contains(&e.name.as_str()) && e.pos.file == 0 {
                        for (v, _) in &vs {
                            self.bare.insert(v.clone(), e.name.clone());
                        }
                    }
                    self.enums.get_mut(&e.name).unwrap().variants = vs;
                    out.push(Item::Enum(re));
                }
                _ => {}
            }
        }
        self.no_value_cycles(prog)?;
        // Function signatures.
        for item in &prog.items {
            match item {
                Item::Fn(f) => {
                    dup_tparams(&f.tparams, f.pos)?;
                    let mut rf = f.clone();
                    for p in &mut rf.params {
                        p.ty = self.resolve(&p.ty, &f.tparams, p.pos)?;
                        if p.ty == Type::Unit {
                            return Err(Diag::new(
                                p.pos,
                                "parameters cannot have type `()`",
                                "remove the parameter; `()` carries no value",
                            ));
                        }
                    }
                    rf.ret = self.resolve(&f.ret, &f.tparams, f.pos)?;
                    if f.name == "main"
                        && (!f.params.is_empty() || rf.ret != Type::Unit || !f.tparams.is_empty())
                    {
                        return Err(Diag::new(
                            f.pos,
                            "`main` takes no parameters and returns `()`",
                            "declare it as `fn main() { ... }`",
                        ));
                    }
                    self.fns.insert(
                        f.name.clone(),
                        FnSig {
                            tparams: f.tparams.clone(),
                            params: rf.params.iter().map(|p| p.ty.clone()).collect(),
                            ret: rf.ret.clone(),
                            is_extern: false,
                        },
                    );
                    out.push(Item::Fn(rf));
                }
                Item::Extern(e) => {
                    if C_KEYWORDS.contains(&e.name.as_str()) {
                        return Err(Diag::new(
                            e.pos,
                            format!("extern name `{}` is a C keyword", e.name),
                            "extern names are emitted as-is into C; a C keyword cannot name a function",
                        ));
                    }
                    if clashes_with_generated_c(&e.name) {
                        return Err(Diag::new(
                            e.pos,
                            format!("extern name `{}` clashes with a name in the generated C", e.name),
                            "names used by dovc's C output or <stdint.h>/<stdbool.h>/<stddef.h> (e.g. `NULL`, `size_t`, `self`, `v_x`) cannot be extern names",
                        ));
                    }
                    if e.name.starts_with("dov_") || e.name == "main" {
                        return Err(Diag::new(
                            e.pos,
                            format!("extern name `{}` is reserved", e.name),
                            "`main` and names that start with `dov_` belong to the compiler and runtime",
                        ));
                    }
                    let mut re = e.clone();
                    for p in &mut re.params {
                        p.ty = self.resolve(&p.ty, &[], p.pos)?;
                        if !matches!(p.ty, Type::I64 | Type::Bool | Type::Str) {
                            return Err(Diag::new(
                                p.pos,
                                format!(
                                    "extern \"C\" parameters must be i64, bool or str, not `{}`",
                                    p.ty
                                ),
                                "C maps i64 to `long`, bool to `bool`, str to `const char *`; other types stay inside .dov",
                            ));
                        }
                    }
                    re.ret = self.resolve(&e.ret, &[], e.pos)?;
                    if !matches!(re.ret, Type::I64 | Type::Bool | Type::Str | Type::Unit) {
                        return Err(Diag::new(
                            e.pos,
                            format!("extern \"C\" functions cannot return `{}`", re.ret),
                            "return i64, bool, str or `()` from C",
                        ));
                    }
                    self.fns.insert(
                        e.name.clone(),
                        FnSig {
                            tparams: Vec::new(),
                            params: re.params.iter().map(|p| p.ty.clone()).collect(),
                            ret: re.ret.clone(),
                            is_extern: true,
                        },
                    );
                    out.push(Item::Extern(re));
                }
                _ => {}
            }
        }
        if !self.fns.contains_key("main") {
            // File 0 is the prelude when there is one; the main file follows it.
            let file = u16::from(self.files.len() > 1);
            return Err(Diag::new(
                Pos {
                    file,
                    line: 1,
                    col: 1,
                },
                "program has no `fn main()`",
                "add `fn main() { ... }`; the program starts there",
            ));
        }
        // Agent fields and handler signatures.
        for item in &prog.items {
            if let Item::Agent(a) = item {
                let mut ra = a.clone();
                let mut fields = HashMap::new();
                for (fi, f) in a.fields.iter().enumerate() {
                    // Field initializers see no locals and no other fields.
                    self.scopes = vec![HashMap::new()];
                    self.ctx = Ctx::Fn;
                    self.tparams = Vec::new();
                    let ann = match &f.ty {
                        Some(t) => Some(self.resolve(t, &[], f.pos)?),
                        None => None,
                    };
                    let t = self.expr(&f.init, ann.as_ref())?;
                    if let Some(ann) = &ann {
                        if !self.unify(ann, &t) {
                            let t = self.zonk(&t);
                            return Err(Diag::new(
                                f.init.pos,
                                format!("field `{}` is declared `{ann}` but its value is `{t}`", f.name),
                                format!("change the annotation to `{t}`, or give it a value of type `{ann}`"),
                            ));
                        }
                    }
                    self.finish_infer()?;
                    let t = self.zonk(&t);
                    if t == Type::Unit {
                        return Err(Diag::new(
                            f.pos,
                            "fields cannot have type `()`",
                            "a field needs a value such as an i64, bool, str, struct, enum, slice or agent handle",
                        ));
                    }
                    ra.fields[fi].ty = Some(t.clone());
                    if fields.insert(f.name.clone(), (t, f.mutable)).is_some() {
                        return Err(Diag::new(
                            f.pos,
                            format!("duplicate field `{}`", f.name),
                            "each field in an agent needs its own name",
                        ));
                    }
                }
                let mut handlers = HashMap::new();
                for (hi, h) in a.handlers.iter().enumerate() {
                    let mut tys = Vec::new();
                    for (pi, p) in h.params.iter().enumerate() {
                        let t = self.resolve(&p.ty, &[], p.pos)?;
                        if t == Type::Unit {
                            return Err(Diag::new(
                                p.pos,
                                "parameters cannot have type `()`",
                                "remove the parameter; `()` carries no value",
                            ));
                        }
                        ra.handlers[hi].params[pi].ty = t.clone();
                        tys.push(t);
                    }
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
                out.push(Item::Agent(ra));
            }
        }
        // Keep the source order of items (codegen relies on nothing else).
        let order: HashMap<(String, u8), usize> = prog
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| it.name_pos().map(|(n, _)| ((n.to_string(), kind(it)), i)))
            .collect();
        out.sort_by_key(|it| {
            it.name_pos()
                .map(|(n, _)| order[&(n.to_string(), kind(it))])
        });
        Ok(Program { items: out })
    }

    fn no_unit_or_agentless(&self, t: &Type, pos: Pos, what: &str) -> Result<()> {
        if *t == Type::Unit {
            return Err(Diag::new(
                pos,
                format!("{what} cannot have type `()`"),
                "remove it; `()` carries no value",
            ));
        }
        Ok(())
    }

    /// A struct or enum may not contain itself by value (only through a slice).
    fn no_value_cycles(&self, prog: &Program) -> Result<()> {
        fn deps(t: &Type, out: &mut Vec<String>) {
            match t {
                Type::Struct(n, a) | Type::Enum(n, a) => {
                    out.push(n.clone());
                    for x in a {
                        deps(x, out);
                    }
                }
                _ => {}
            }
        }
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        for (n, s) in &self.structs {
            let mut d = Vec::new();
            for (_, t) in &s.fields {
                deps(t, &mut d);
            }
            graph.insert(n.clone(), d);
        }
        for (n, e) in &self.enums {
            let mut d = Vec::new();
            for (_, ts) in &e.variants {
                for t in ts {
                    deps(t, &mut d);
                }
            }
            graph.insert(n.clone(), d);
        }
        for item in &prog.items {
            let (name, pos) = match item {
                Item::Struct(s) => (&s.name, s.pos),
                Item::Enum(e) => (&e.name, e.pos),
                _ => continue,
            };
            // DFS from `name`; reaching `name` again is a by-value cycle.
            let mut stack = graph[name].clone();
            let mut seen = HashSet::new();
            while let Some(n) = stack.pop() {
                if n == *name {
                    return Err(Diag::new(
                        pos,
                        format!(
                            "`{name}` contains itself by value, so it would have infinite size"
                        ),
                        format!("put the recursive part in a slice, e.g. `children: [{name}]`"),
                    ));
                }
                if seen.insert(n.clone()) {
                    if let Some(d) = graph.get(&n) {
                        stack.extend(d.iter().cloned());
                    }
                }
            }
        }
        Ok(())
    }
}

// ---------- bodies ----------

/// Variant names with their (substituted) payload types.
type Variants = Vec<(String, Vec<Type>)>;

const COND_HINT: &str = "conditions must be `bool`; compare explicitly, e.g. `n != 0`";

impl Checker {
    fn bodies(&mut self, prog: &Program) -> Result<()> {
        for item in &prog.items {
            match item {
                Item::Fn(f) => {
                    self.ctx = Ctx::Fn;
                    self.ret = f.ret.clone();
                    self.tparams = f.tparams.clone();
                    self.scopes = vec![HashMap::new()];
                    self.params(&f.params)?;
                    self.block(&f.body)?;
                    self.finish_infer()?;
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
                        self.ctx = Ctx::Handler(a.name.clone());
                        self.ret = Type::Unit;
                        self.tparams = Vec::new();
                        self.scopes = vec![HashMap::new()];
                        self.params(&h.params)?;
                        self.block(&h.body)?;
                        self.finish_infer()?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Zonk every type recorded since the last call; an unsolved variable is an error.
    fn finish_infer(&mut self) -> Result<()> {
        let pending = std::mem::take(&mut self.pending);
        for pos in pending {
            let t = self.zonk(&self.info.types[&pos]);
            if Self::has_infer(&t) {
                return Err(Diag::new(
                    pos,
                    format!("cannot infer the full type of this expression (`{t}`)"),
                    "add a type annotation, e.g. `let x: Option<i64> = None;` or `let xs: [str] = [];`",
                ));
            }
            self.info.types.insert(pos, t);
        }
        let pending = std::mem::take(&mut self.pending_targs);
        for pos in pending {
            let ts: Vec<Type> = self.info.targs[&pos].iter().map(|t| self.zonk(t)).collect();
            if let Some(t) = ts.iter().find(|t| Self::has_infer(t)) {
                return Err(Diag::new(
                    pos,
                    format!("cannot infer the type arguments of this call (`{t}`)"),
                    "pass arguments whose types are known, or annotate the binding it goes into",
                ));
            }
            self.info.targs.insert(pos, ts);
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
        if let Ctx::Handler(a) = &self.ctx {
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
        if let Ctx::Handler(a) = &self.ctx {
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

    fn expect(&mut self, e: &Expr, want: &Type, what: &str) -> Result<Type> {
        self.expect_h(
            e,
            want,
            what,
            "there are no implicit conversions; change the value to the expected type",
        )
    }

    fn expect_h(&mut self, e: &Expr, want: &Type, what: &str, hint: &str) -> Result<Type> {
        let got = self.expr(e, Some(want))?;
        if !self.unify(want, &got) {
            let (w, g) = (self.zonk(want), self.zonk(&got));
            return Err(Diag::new(
                e.pos,
                format!("{what}: expected `{w}`, found `{g}`"),
                hint,
            ));
        }
        Ok(got)
    }

    fn stmt(&mut self, s: &Stmt) -> Result<()> {
        match &s.kind {
            StmtKind::Let {
                name,
                mutable,
                ty,
                init,
            } => {
                let ann = match ty {
                    Some(t) => Some(self.resolve(t, &self.tparams.clone(), s.pos)?),
                    None => None,
                };
                let t = self.expr(init, ann.as_ref())?;
                if let Some(ann) = &ann {
                    if !self.unify(ann, &t) {
                        let t = self.zonk(&t);
                        return Err(Diag::new(
                            init.pos,
                            format!("`{name}` is declared `{ann}` but its value is `{t}`"),
                            format!("change the annotation to `{t}`, or give it a value of type `{ann}`"),
                        ));
                    }
                }
                let t = self.zonk(&t);
                if t == Type::Unit {
                    return Err(Diag::new(
                        init.pos,
                        format!("cannot bind `{name}` to a `()` value"),
                        "call it as a statement instead, e.g. `print(...);`",
                    ));
                }
                self.declare(name, t, *mutable, s.pos)?;
            }
            StmtKind::Assign { target, value } => self.assign(s.pos, target, value)?,
            StmtKind::If { cond, then, els } => {
                self.expect_h(cond, &Type::Bool, "`if` condition", COND_HINT)?;
                self.block(then)?;
                if let Some(e) = els {
                    self.block(e)?;
                }
            }
            StmtKind::While { cond, body } => {
                self.expect_h(cond, &Type::Bool, "`while` condition", COND_HINT)?;
                self.block(body)?;
            }
            StmtKind::Return(v) => {
                if self.in_defer > 0 {
                    return Err(Diag::new(
                        s.pos,
                        "`return` is not allowed inside `defer`",
                        "a deferred block runs while the function is already leaving; remove the `return`",
                    ));
                }
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
                        self.expect_h(e, &ret, "return value", hint)?;
                    }
                    Some(e) => {
                        self.expect(e, &ret, "return value")?;
                    }
                }
            }
            StmtKind::Expr(e) => {
                self.expr(e, None)?;
            }
            StmtKind::Send {
                target,
                handler,
                handler_pos,
                args,
            } => {
                let t = self.expr(target, None)?;
                let t = self.zonk(&t);
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
                        None if hs.is_empty() => {
                            format!("`{a}` has no handlers; add `on {handler}(...) {{ ... }}`")
                        }
                        None => {
                            let mut names: Vec<&str> = hs.keys().map(|k| k.as_str()).collect();
                            names.sort();
                            format!("`{a}` handles: {}", names.join(", "))
                        }
                    };
                    Diag::new(
                        *handler_pos,
                        format!("agent `{a}` has no handler `{handler}`"),
                        hint,
                    )
                })?;
                self.args(args, &params, &format!("{a}.{handler}"), *handler_pos)?;
            }
            StmtKind::Match { scrut, arms } => self.match_stmt(s.pos, scrut, arms)?,
            StmtKind::Defer(b) => {
                self.in_defer += 1;
                let r = self.block(b);
                self.in_defer -= 1;
                r?;
            }
        }
        Ok(())
    }

    fn assign(&mut self, pos: Pos, target: &Expr, value: &Expr) -> Result<()> {
        // Find the root name of a field chain `a.b.c`.
        let mut root = target;
        let mut through_field = false;
        loop {
            match &root.kind {
                ExprKind::Field(b, _) => {
                    root = b;
                    through_field = true;
                }
                ExprKind::Var(_) => break,
                ExprKind::Index(..) | ExprKind::Range(..) => {
                    return Err(Diag::new(
                        root.pos,
                        "cannot assign to an item of a slice or string",
                        "slice and string contents are immutable; build a new one with `append` or `concat`",
                    ))
                }
                _ => {
                    return Err(Diag::new(
                        root.pos,
                        "cannot assign to this expression",
                        "assign to a `var` binding, a `var` field, or a field of a `var` struct, e.g. `p.x = 1;`",
                    ))
                }
            }
        }
        let ExprKind::Var(name) = &root.kind else {
            unreachable!()
        };
        let (t, mutable, what) = self
            .lookup(name)
            .ok_or_else(|| self.unknown_var(name, pos))?;
        if !mutable {
            let hint = match what {
                "parameter" => {
                    format!("parameters are immutable; copy it first: `var {name}2 = {name};`")
                }
                "field" => format!("declare the field with `var {name}` to allow mutation"),
                _ => format!(
                    "declare it with `var {name}` instead of `let {name}` to allow mutation"
                ),
            };
            let msg = if through_field {
                format!("cannot assign to a field of immutable {what} `{name}`")
            } else {
                format!("cannot assign to immutable {what} `{name}`")
            };
            return Err(Diag::new(pos, msg, hint));
        }
        if through_field {
            let t = self.expr(target, None)?;
            let shown = field_path(target);
            self.expect(value, &t, &format!("assignment to `{shown}`"))?;
        } else {
            self.expect(value, &t, &format!("assignment to `{name}`"))?;
        }
        Ok(())
    }

    fn match_stmt(&mut self, pos: Pos, scrut: &Expr, arms: &[Arm]) -> Result<()> {
        let st = self.expr(scrut, None)?;
        let st = self.zonk(&st);
        let variants: Option<(String, Variants)> = match &st {
            Type::Enum(n, targs) => {
                let sig = &self.enums[n];
                let map = zip_map(&sig.tparams, targs);
                Some((
                    n.clone(),
                    sig.variants
                        .iter()
                        .map(|(v, ts)| (v.clone(), ts.iter().map(|t| subst(t, &map)).collect()))
                        .collect(),
                ))
            }
            Type::I64 | Type::Bool | Type::Str => None,
            other => {
                return Err(Diag::new(
                    scrut.pos,
                    format!("cannot `match` on a value of type `{other}`"),
                    "`match` works on enums, i64, bool and str",
                ))
            }
        };
        let mut wild = false;
        let mut seen_variants: HashSet<String> = HashSet::new();
        let mut seen_lits: HashSet<String> = HashSet::new();
        for arm in arms {
            if wild {
                return Err(Diag::new(
                    arm.pos,
                    "unreachable pattern: an earlier `_` arm already matches everything",
                    "remove this arm, or move the `_` arm last",
                ));
            }
            self.scopes.push(HashMap::new());
            match (&arm.pat, &variants) {
                (Pattern::Wild, _) => wild = true,
                (
                    Pattern::Variant {
                        enum_name,
                        variant,
                        binds,
                    },
                    Some((en, vs)),
                ) => {
                    if let Some(written) = enum_name {
                        if written != en {
                            return Err(Diag::new(
                                arm.pos,
                                format!("pattern `{written}::{variant}` does not match a value of type `{st}`"),
                                format!("use a variant of `{en}`"),
                            ));
                        }
                    }
                    let Some((_, payload)) = vs.iter().find(|(v, _)| v == variant) else {
                        let hint = match closest(variant, vs.iter().map(|(v, _)| v.as_str())) {
                            Some(c) => format!("did you mean `{c}`?"),
                            None => {
                                let names: Vec<&str> = vs.iter().map(|(v, _)| v.as_str()).collect();
                                format!("`{en}` has: {}", names.join(", "))
                            }
                        };
                        return Err(Diag::new(
                            arm.pos,
                            format!("enum `{en}` has no variant `{variant}`"),
                            hint,
                        ));
                    };
                    if binds.len() != payload.len() {
                        return Err(Diag::new(
                            arm.pos,
                            format!(
                                "variant `{variant}` has {} value(s) but the pattern binds {}",
                                payload.len(),
                                binds.len()
                            ),
                            "bind every value, using `_` for the ones you do not need",
                        ));
                    }
                    if !seen_variants.insert(variant.clone()) {
                        return Err(Diag::new(
                            arm.pos,
                            format!("unreachable pattern: `{variant}` is already matched above"),
                            "remove this arm or merge it with the earlier one",
                        ));
                    }
                    for (b, t) in binds.iter().zip(payload.clone()) {
                        if let Some((name, bpos)) = b {
                            self.declare(name, t, false, *bpos)?;
                        }
                    }
                }
                (Pattern::Variant { variant, .. }, None) => {
                    return Err(Diag::new(
                        arm.pos,
                        format!("pattern `{variant}` is a variant, but the value has type `{st}`"),
                        "match a value of this type with literals and `_`",
                    ));
                }
                (lit, vs) => {
                    let (lt, key) = match lit {
                        Pattern::Int(v) => (Type::I64, v.to_string()),
                        Pattern::Bool(b) => (Type::Bool, b.to_string()),
                        Pattern::Str(s) => (Type::Str, format!("{s:?}")),
                        _ => unreachable!(),
                    };
                    if vs.is_some() || lt != st {
                        return Err(Diag::new(
                            arm.pos,
                            format!("pattern type mismatch: expected `{st}`, found `{lt}`"),
                            "each pattern must have the type of the matched value",
                        ));
                    }
                    if !seen_lits.insert(key.clone()) {
                        return Err(Diag::new(
                            arm.pos,
                            format!("unreachable pattern: `{key}` is already matched above"),
                            "remove this arm or merge it with the earlier one",
                        ));
                    }
                }
            }
            let r = self.block(&arm.body);
            self.scopes.pop();
            r?;
        }
        if !wild {
            match &variants {
                Some((en, vs)) => {
                    let missing: Vec<String> = vs
                        .iter()
                        .filter(|(v, _)| !seen_variants.contains(v))
                        .map(|(v, _)| format!("`{v}`"))
                        .collect();
                    if !missing.is_empty() {
                        return Err(Diag::new(
                            pos,
                            format!("`match` on `{en}` does not cover {}", missing.join(", ")),
                            "add an arm for each missing variant, or a `_ => { ... }` arm",
                        ));
                    }
                }
                None if st == Type::Bool && seen_lits.len() == 2 => {}
                None => {
                    return Err(Diag::new(
                        pos,
                        format!("`match` on `{st}` does not cover every value"),
                        "add a `_ => { ... }` arm at the end",
                    ))
                }
            }
        }
        Ok(())
    }

    fn args(&mut self, args: &[Expr], params: &[Type], what: &str, pos: Pos) -> Result<()> {
        if args.len() != params.len() {
            let list: Vec<String> = params.iter().map(|t| self.zonk(t).to_string()).collect();
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

    fn arity(&self, args: &[Expr], n: usize, what: &str, sig: &str, pos: Pos) -> Result<()> {
        if args.len() != n {
            return Err(Diag::new(
                pos,
                format!("`{what}` takes {n} argument(s) but got {}", args.len()),
                format!("expected `{sig}`"),
            ));
        }
        Ok(())
    }

    /// Build a value of enum `en`, variant `v` (fresh type arguments, unified with `expected`).
    fn variant_value(
        &mut self,
        en: &str,
        v: &str,
        args: &[Expr],
        expected: Option<&Type>,
        pos: Pos,
        shown: &str,
    ) -> Result<Type> {
        let sig = &self.enums[en];
        let Some((_, payload)) = sig.variants.iter().find(|(n, _)| n == v) else {
            let hint = match closest(v, sig.variants.iter().map(|(n, _)| n.as_str())) {
                Some(c) => format!("did you mean `{en}::{c}`?"),
                None => {
                    let names: Vec<&str> = sig.variants.iter().map(|(n, _)| n.as_str()).collect();
                    format!("`{en}` has: {}", names.join(", "))
                }
            };
            return Err(Diag::new(
                pos,
                format!("enum `{en}` has no variant `{v}`"),
                hint,
            ));
        };
        let payload = payload.clone();
        let tps = sig.tparams.clone();
        let vars: Vec<Type> = tps.iter().map(|_| self.fresh()).collect();
        let map = zip_map(&tps, &vars);
        let t = Type::Enum(en.to_string(), vars);
        if let Some(x) = expected {
            self.unify(x, &t);
        }
        let ps: Vec<Type> = payload.iter().map(|p| subst(p, &map)).collect();
        if args.len() != ps.len() {
            return Err(Diag::new(
                pos,
                format!(
                    "variant `{shown}` takes {} value(s) but got {}",
                    ps.len(),
                    args.len()
                ),
                if ps.is_empty() {
                    format!("write `{shown}` without parentheses")
                } else {
                    format!("write `{shown}(...)` with {} value(s)", ps.len())
                },
            ));
        }
        for (i, (a, p)) in args.iter().zip(&ps).enumerate() {
            self.expect(a, p, &format!("value {} of `{shown}`", i + 1))?;
        }
        Ok(t)
    }

    fn expr(&mut self, e: &Expr, expected: Option<&Type>) -> Result<Type> {
        let t = self.expr_inner(e, expected)?;
        self.info.types.insert(e.pos, t.clone());
        self.pending.push(e.pos);
        Ok(t)
    }

    fn expr_inner(&mut self, e: &Expr, expected: Option<&Type>) -> Result<Type> {
        Ok(match &e.kind {
            ExprKind::Int(_) => Type::I64,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Str(_) => Type::Str,
            ExprKind::Var(n) => match self.lookup(n) {
                Some((t, _, _)) => t,
                None => match self.bare.get(n).cloned() {
                    Some(en) => self.variant_value(&en, n, &[], expected, e.pos, n)?,
                    None => return Err(self.unknown_var(n, e.pos)),
                },
            },
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
                        let h = "arithmetic works on i64 only; join strings with `concat(a, b)`";
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
                        let lt = self.expr(l, None)?;
                        let lt = self.zonk(&lt);
                        if !matches!(lt, Type::I64 | Type::Bool | Type::Str) {
                            return Err(Diag::new(
                                l.pos,
                                format!("cannot compare values of type `{lt}`"),
                                "`==` and `!=` work on i64, bool and str only; use `match` for enums",
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
            ExprKind::Call(name, args) if BUILTINS.contains(&name.as_str()) => {
                self.builtin(name, args, expected, e.pos)?
            }
            ExprKind::Call(name, args) => {
                if let Some(sig) = self.fns.get(name) {
                    let (tps, params, ret) =
                        (sig.tparams.clone(), sig.params.clone(), sig.ret.clone());
                    if name == "main" {
                        return Err(Diag::new(
                            e.pos,
                            "`main` cannot be called",
                            "move the shared code into another function and call that",
                        ));
                    }
                    if tps.is_empty() {
                        self.args(args, &params, name, e.pos)?;
                        ret
                    } else {
                        let vars: Vec<Type> = tps.iter().map(|_| self.fresh()).collect();
                        let map = zip_map(&tps, &vars);
                        let params: Vec<Type> = params.iter().map(|p| subst(p, &map)).collect();
                        let ret = subst(&ret, &map);
                        if let Some(x) = expected {
                            self.unify(x, &ret);
                        }
                        self.args(args, &params, name, e.pos)?;
                        self.info.targs.insert(e.pos, vars);
                        self.pending_targs.push(e.pos);
                        ret
                    }
                } else if let Some(en) = self.bare.get(name).cloned() {
                    self.variant_value(&en, name, args, expected, e.pos, name)?
                } else {
                    let names = self.fns.keys().map(|k| k.as_str());
                    let hint = match closest(name, names.chain(BUILTINS.iter().copied())) {
                        Some(c) => format!("did you mean `{c}`?"),
                        None => format!("declare it with `fn {name}(...) {{ ... }}` or `extern \"C\" fn {name}(...);`"),
                    };
                    return Err(Diag::new(e.pos, format!("unknown function `{name}`"), hint));
                }
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
            ExprKind::StructLit(name, fields) => {
                let Some(sig) = self.structs.get(name) else {
                    let hint = match closest(name, self.structs.keys().map(|k| k.as_str())) {
                        Some(c) => format!("did you mean `{c}`?"),
                        None => format!("declare it with `struct {name} {{ ... }}`"),
                    };
                    return Err(Diag::new(e.pos, format!("unknown struct `{name}`"), hint));
                };
                let tps = sig.tparams.clone();
                let decl = sig.fields.clone();
                let vars: Vec<Type> = tps.iter().map(|_| self.fresh()).collect();
                let map = zip_map(&tps, &vars);
                let t = Type::Struct(name.clone(), vars);
                if let Some(x) = expected {
                    self.unify(x, &t);
                }
                let mut seen = HashSet::new();
                for (f, v) in fields {
                    let Some((_, ft)) = decl.iter().find(|(n, _)| n == f) else {
                        let hint = match closest(f, decl.iter().map(|(n, _)| n.as_str())) {
                            Some(c) => format!("did you mean `{c}`?"),
                            None => {
                                let names: Vec<&str> =
                                    decl.iter().map(|(n, _)| n.as_str()).collect();
                                format!("`{name}` has fields: {}", names.join(", "))
                            }
                        };
                        return Err(Diag::new(
                            v.pos,
                            format!("struct `{name}` has no field `{f}`"),
                            hint,
                        ));
                    };
                    if !seen.insert(f.clone()) {
                        return Err(Diag::new(
                            v.pos,
                            format!("field `{f}` is given twice"),
                            "give each field once",
                        ));
                    }
                    let ft = subst(ft, &map);
                    self.expect(v, &ft, &format!("field `{name}.{f}`"))?;
                }
                let missing: Vec<String> = decl
                    .iter()
                    .filter(|(n, _)| !seen.contains(n))
                    .map(|(n, _)| format!("`{n}`"))
                    .collect();
                if !missing.is_empty() {
                    return Err(Diag::new(
                        e.pos,
                        format!(
                            "missing field(s) {} in `{name}` literal",
                            missing.join(", ")
                        ),
                        "a struct literal must give every field a value",
                    ));
                }
                t
            }
            ExprKind::Field(base, f) => {
                let bt = self.expr(base, None)?;
                let bt = self.zonk(&bt);
                let Type::Struct(sn, targs) = &bt else {
                    return Err(Diag::new(
                        e.pos,
                        format!("a value of type `{bt}` has no fields"),
                        "only struct values have fields; use `len(x)` for the length of a str or slice",
                    ));
                };
                let sig = &self.structs[sn];
                let Some((_, ft)) = sig.fields.iter().find(|(n, _)| n == f) else {
                    let hint = match closest(f, sig.fields.iter().map(|(n, _)| n.as_str())) {
                        Some(c) => format!("did you mean `{c}`?"),
                        None => {
                            let names: Vec<&str> =
                                sig.fields.iter().map(|(n, _)| n.as_str()).collect();
                            format!("`{sn}` has fields: {}", names.join(", "))
                        }
                    };
                    return Err(Diag::new(
                        e.pos,
                        format!("struct `{sn}` has no field `{f}`"),
                        hint,
                    ));
                };
                subst(ft, &zip_map(&sig.tparams, targs))
            }
            ExprKind::Variant(en, v, args) => {
                if !self.enums.contains_key(en) {
                    let hint = match closest(en, self.enums.keys().map(|k| k.as_str())) {
                        Some(c) => format!("did you mean `{c}`?"),
                        None => format!("declare it with `enum {en} {{ ... }}`"),
                    };
                    return Err(Diag::new(e.pos, format!("unknown enum `{en}`"), hint));
                }
                self.variant_value(en, v, args, expected, e.pos, &format!("{en}::{v}"))?
            }
            ExprKind::Array(items) => {
                let el = match expected.map(|x| self.zonk(x)) {
                    Some(Type::Slice(t)) => *t,
                    _ => self.fresh(),
                };
                for (i, it) in items.iter().enumerate() {
                    self.expect_h(
                        it,
                        &el,
                        &format!("slice item {}", i + 1),
                        "all items of a slice have the same type",
                    )?;
                }
                Type::Slice(Box::new(el))
            }
            ExprKind::Index(base, i) => {
                let bt = self.expr(base, None)?;
                let bt = self.zonk(&bt);
                self.expect_h(i, &Type::I64, "index", "indexes are i64")?;
                match bt {
                    Type::Str => Type::I64,
                    Type::Slice(t) => *t,
                    other => {
                        return Err(Diag::new(
                            e.pos,
                            format!("cannot index a value of type `{other}`"),
                            "only `str` and slices `[T]` can be indexed",
                        ))
                    }
                }
            }
            ExprKind::Range(base, a, b) => {
                let bt = self.expr(base, None)?;
                let bt = self.zonk(&bt);
                self.expect_h(a, &Type::I64, "range start", "range bounds are i64")?;
                self.expect_h(b, &Type::I64, "range end", "range bounds are i64")?;
                match bt {
                    Type::Str | Type::Slice(_) => bt,
                    other => {
                        return Err(Diag::new(
                            e.pos,
                            format!("cannot take a range of a value of type `{other}`"),
                            "only `str` and slices `[T]` support `x[a..b]`",
                        ))
                    }
                }
            }
        })
    }

    fn builtin(
        &mut self,
        name: &str,
        args: &[Expr],
        expected: Option<&Type>,
        pos: Pos,
    ) -> Result<Type> {
        Ok(match name {
            "print" => {
                for a in args {
                    let t = self.expr(a, None)?;
                    let t = self.zonk(&t);
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
            "panic" => {
                self.arity(args, 1, name, "panic(message: str)", pos)?;
                self.expect(&args[0], &Type::Str, "argument 1 of `panic`")?;
                Type::Unit
            }
            "len" => {
                self.arity(args, 1, name, "len(x: str or [T]) -> i64", pos)?;
                let t = self.expr(&args[0], None)?;
                let t = self.zonk(&t);
                if !matches!(t, Type::Str | Type::Slice(_)) {
                    return Err(Diag::new(
                        args[0].pos,
                        format!("`len` takes a str or a slice, found `{t}`"),
                        "`len` counts the bytes of a str or the items of a slice",
                    ));
                }
                Type::I64
            }
            "append" => {
                self.arity(args, 2, name, "append(xs: [T], x: T) -> [T]", pos)?;
                let t = self.expr(&args[0], expected)?;
                let el = match self.zonk(&t) {
                    Type::Slice(el) => *el,
                    Type::Infer(_) => {
                        let el = self.fresh();
                        self.unify(&t, &Type::Slice(Box::new(el.clone())));
                        el
                    }
                    other => {
                        return Err(Diag::new(
                            args[0].pos,
                            format!("argument 1 of `append`: expected a slice, found `{other}`"),
                            "`append(xs, x)` returns a new slice with `x` at the end",
                        ))
                    }
                };
                self.expect(&args[1], &el, "argument 2 of `append`")?;
                Type::Slice(Box::new(el))
            }
            "concat" => {
                self.arity(args, 2, name, "concat(a: str, b: str) -> str", pos)?;
                let t = self.expr(&args[0], expected)?;
                let t = self.zonk(&t);
                if !matches!(t, Type::Str | Type::Slice(_)) {
                    return Err(Diag::new(
                        args[0].pos,
                        format!("argument 1 of `concat`: expected a str or a slice, found `{t}`"),
                        "`concat` joins two strs or two slices of the same type",
                    ));
                }
                self.expect(&args[1], &t, "argument 2 of `concat`")?;
                self.zonk(&t)
            }
            "to_str" => {
                self.arity(args, 1, name, "to_str(x: i64, bool or str) -> str", pos)?;
                let t = self.expr(&args[0], None)?;
                let t = self.zonk(&t);
                if !matches!(t, Type::I64 | Type::Bool | Type::Str) {
                    return Err(Diag::new(
                        args[0].pos,
                        format!("`to_str` cannot convert a value of type `{t}`"),
                        "`to_str` takes i64, bool or str",
                    ));
                }
                Type::Str
            }
            "char_of" => {
                self.arity(args, 1, name, "char_of(byte: i64) -> str", pos)?;
                self.expect(&args[0], &Type::I64, "argument 1 of `char_of`")?;
                Type::Str
            }
            _ => unreachable!("builtin {name}"),
        })
    }
}

/// `a.b.c` as text (for messages).
fn field_path(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Field(b, f) => format!("{}.{f}", field_path(b)),
        ExprKind::Var(n) => n.clone(),
        _ => "...".into(),
    }
}

fn kind(it: &Item) -> u8 {
    match it {
        Item::Fn(_) => 0,
        Item::Extern(_) => 1,
        Item::Agent(_) => 2,
        Item::Struct(_) => 3,
        Item::Enum(_) => 4,
        Item::Import(..) => 5,
    }
}

#[cfg(test)]
mod tests {
    fn chk(src: &str) -> crate::diag::Result<()> {
        crate::check_source(src)
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
        chk("fn main() { var x = 1; x = x + 1; }").unwrap();
    }

    #[test]
    fn params_and_let_fields_are_immutable() {
        assert!(err("fn f(a: i64) { a = 2; } fn main() {}").contains("immutable parameter `a`"));
        assert!(err("agent A { let k = 1; on h() { k = 2; } } fn main() {}")
            .contains("immutable field `k`"));
        chk("agent A { var k = 1; on h() { k = 2; } } fn main() {}").unwrap();
    }

    #[test]
    fn type_mismatch() {
        assert!(err("fn main() { let x: i64 = true; }")
            .contains("declared `i64` but its value is `bool`"));
        assert!(err("fn main() { if 1 { } }").contains("`if` condition"));
    }

    #[test]
    fn return_paths() {
        assert!(err("fn f() -> i64 { } fn main() {}").contains("must return"));
        chk("fn f(b: bool) -> i64 { if b { return 1; } else { return 2; } } fn main() {}").unwrap();
        chk("enum E { A, B } fn f(e: E) -> i64 { match e { E::A => { return 1; } E::B => { return 2; } } } fn main() {}").unwrap();
        chk("fn f() -> i64 { panic(\"no\"); } fn main() {}").unwrap();
    }

    #[test]
    fn send_checks() {
        let base = "agent C { on add(n: i64) {} } fn main() { let c = spawn C; ";
        chk(&format!("{base} send c.add(1); }}")).unwrap();
        assert!(err(&format!("{base} send c.nope(1); }}")).contains("no handler `nope`"));
        assert!(err(&format!("{base} send c.add(true); }}")).contains("argument 1"));
        assert!(err("fn main() { let x = 1; send x.add(1); }").contains("needs an agent handle"));
    }

    #[test]
    fn struct_rules() {
        let s = "struct P { x: i64, y: i64 }";
        chk(&format!(
            "{s} fn main() {{ var p = P {{ x: 1, y: 2 }}; p.x = 3; }}"
        ))
        .unwrap();
        assert!(err(&format!(
            "{s} fn main() {{ let p = P {{ x: 1, y: 2 }}; p.x = 3; }}"
        ))
        .contains("field of immutable binding"));
        assert!(
            err(&format!("{s} fn main() {{ let p = P {{ x: 1 }}; }}")).contains("missing field")
        );
        assert!(err("struct A { b: B } struct B { a: A } fn main() {}").contains("by value"));
        chk("struct T { kids: [T] } fn main() {}").unwrap();
    }

    #[test]
    fn match_rules() {
        let e = "enum C { R, G }";
        assert!(err(&format!(
            "{e} fn main() {{ match C::R {{ C::R => {{}} }} }}"
        ))
        .contains("does not cover `G`"));
        assert!(err("fn main() { match true { true => {} } }").contains("every value"));
        chk("fn main() { match true { true => {} false => {} } }").unwrap();
    }

    #[test]
    fn generic_inference() {
        chk("fn id<T>(x: T) -> T { return x; } fn main() { let s: str = id(\"a\"); print(id(1) + 1, s); }").unwrap();
        assert!(err("fn main() { let x = None; }").contains("cannot infer"));
        chk("fn main() { let x: Option<i64> = None; var y = None; y = Some(true); }").unwrap();
        assert!(err("fn f<T>(a: T) -> T { return a + a; } fn main() {}").contains("found `T`"));
    }

    #[test]
    fn slices_and_strings() {
        chk("fn main() { let xs = append([1], 2); let s = concat(\"a\", \"b\"); print(xs[0], s[0], len(s[0..1])); }").unwrap();
        assert!(err("fn main() { var xs = [1]; xs[0] = 2; }").contains("slice or string"));
        assert!(err("fn main() { let xs = [1, true]; }").contains("slice item 2"));
    }

    #[test]
    fn defer_rules() {
        chk("fn main() { var n = 0; defer { n = n + 1; print(n); } }").unwrap();
        assert!(err("fn f() { defer { return; } } fn main() {}").contains("inside `defer`"));
    }

    #[test]
    fn names() {
        assert!(err("fn main() { print(y); }").contains("unknown variable `y`"));
        assert!(err("fn main() { f(); }").contains("unknown function `f`"));
        assert!(err("fn main() {} fn main() {}").contains("already defined"));
        assert!(err("fn f() {}").contains("no `fn main()`"));
        assert!(err("fn len() {} fn main() {}").contains("built-in"));
        assert!(err("fn is_some() {} fn main() {}").contains("<prelude>"));
        chk("fn main() { let x = 1; if true { let x = 2; } }").unwrap();
    }
}
