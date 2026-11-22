//! Abstract syntax tree for `.dov` v0.1.

use crate::diag::Pos;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    I64,
    Bool,
    Str,
    Unit,
    /// Handle to an agent of the named kind. Handles are immutable and sendable.
    Agent(String),
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::I64 => write!(f, "i64"),
            Type::Bool => write!(f, "bool"),
            Type::Str => write!(f, "str"),
            Type::Unit => write!(f, "()"),
            Type::Agent(n) => write!(f, "{n}"),
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
    Assign {
        name: String,
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
}
