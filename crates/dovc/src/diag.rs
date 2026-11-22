//! Diagnostics: every compiler error carries a source position.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub pos: Pos,
    pub msg: String,
}

impl Diag {
    pub fn new(pos: Pos, msg: impl Into<String>) -> Self {
        Diag {
            pos,
            msg: msg.into(),
        }
    }

    /// Render as `file:line:col: error: msg`.
    pub fn render(&self, file: &str) -> String {
        format!(
            "{}:{}:{}: error: {}",
            file, self.pos.line, self.pos.col, self.msg
        )
    }
}

pub type Result<T> = std::result::Result<T, Diag>;
