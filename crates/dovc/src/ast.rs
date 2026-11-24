//! Abstract syntax tree for `.dov` v0.3.

use crate::diag::Pos;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    I64,
    Bool,
    Str,
    Unit,
    /// As written in source: a name with optional type arguments. The checker resolves it.
    Named(String, Vec<Type>),
    /// Handle to an agent of the named kind. Handles are immutable and sendable.
    Agent(String),
    Struct(String, Vec<Type>),
    Enum(String, Vec<Type>),
    /// `[T]`: an immutable slice.
    Slice(Box<Type>),
    /// A type parameter of the generic item being checked.
    Param(String),
    /// An inference variable (checker only).
    Infer(u32),
}

impl Type {
    pub fn args(&self) -> &[Type] {
        match self {
            Type::Named(_, a) | Type::Struct(_, a) | Type::Enum(_, a) => a,
            _ => &[],
        }
    }
}

fn list(f: &mut fmt::Formatter<'_>, name: &str, args: &[Type]) -> fmt::Result {
    write!(f, "{name}")?;
    if !args.is_empty() {
        write!(f, "<")?;
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{a}")?;
        }
        write!(f, ">")?;
    }
    Ok(())
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::I64 => write!(f, "i64"),
            Type::Bool => write!(f, "bool"),
            Type::Str => write!(f, "str"),
            Type::Unit => write!(f, "()"),
            Type::Agent(n) | Type::Param(n) => write!(f, "{n}"),
            Type::Named(n, a) | Type::Struct(n, a) | Type::Enum(n, a) => list(f, n, a),
            Type::Slice(t) => write!(f, "[{t}]"),
            Type::Infer(_) => write!(f, "_"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Fn(FnDecl),
    Extern(ExternFn),
    Agent(AgentDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Import(String, Pos),
}

impl Item {
    pub fn name_pos(&self) -> Option<(&str, Pos)> {
        Some(match self {
            Item::Fn(f) => (&f.name, f.pos),
            Item::Extern(e) => (&e.name, e.pos),
            Item::Agent(a) => (&a.name, a.pos),
            Item::Struct(s) => (&s.name, s.pos),
            Item::Enum(e) => (&e.name, e.pos),
            Item::Import(..) => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct FnDecl {
    pub name: String,
    pub tparams: Vec<String>,
    pub params: Vec<Param>,
    pub ret: Type,
    pub body: Block,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct ExternFn {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Type,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub mutable: bool,
    pub ty: Option<Type>,
    pub init: Expr,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct Handler {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Block,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct AgentDecl {
    pub name: String,
    pub fields: Vec<Field>,
    pub handlers: Vec<Handler>,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: String,
    pub tparams: Vec<String>,
    pub fields: Vec<Param>,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<Type>,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub tparams: Vec<String>,
    pub variants: Vec<Variant>,
    pub pos: Pos,
}

pub type Block = Vec<Stmt>;

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Let {
        name: String,
        mutable: bool,
        ty: Option<Type>,
        init: Expr,
    },
    /// `place = value;` where place is a name or a field chain `a.b.c`.
    Assign {
        target: Expr,
        value: Expr,
    },
    If {
        cond: Expr,
        then: Block,
        els: Option<Block>,
    },
    While {
        cond: Expr,
        body: Block,
    },
    Return(Option<Expr>),
    Expr(Expr),
    Send {
        target: Expr,
        handler: String,
        handler_pos: Pos,
        args: Vec<Expr>,
    },
    Match {
        scrut: Expr,
        arms: Vec<Arm>,
    },
    Defer(Block),
}

#[derive(Debug, Clone)]
pub struct Arm {
    pub pat: Pattern,
    pub body: Block,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Wild,
    Int(i64),
    Bool(bool),
    Str(String),
    /// `E::V(a, _)` or `V(a, _)`; `None` binds are `_`.
    Variant {
        enum_name: Option<String>,
        variant: String,
        binds: Vec<Option<(String, Pos)>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    Bool(bool),
    Str(String),
    Var(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Spawn(String),
    /// `Name { f: e, ... }`
    StructLit(String, Vec<(String, Expr)>),
    /// `e.f`
    Field(Box<Expr>, String),
    /// `E::V` or `E::V(args)`
    Variant(String, String, Vec<Expr>),
    /// `[a, b, c]`
    Array(Vec<Expr>),
    /// `e[i]`
    Index(Box<Expr>, Box<Expr>),
    /// `e[a..b]`
    Range(Box<Expr>, Box<Expr>, Box<Expr>),
}
