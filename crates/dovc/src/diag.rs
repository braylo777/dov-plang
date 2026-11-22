//! Diagnostics: every compiler error carries a source position and a short hint.
//!
//! Rendered form (first line is the classic `file:line:col:` shape editors parse):
//!
//! ```text
//! demo.dov:4:5: error: cannot assign to immutable binding `x`
//!   |
//! 4 |     x = 2;
//!   |     ^
//!   = hint: declare it with `var x` to allow mutation
//! ```

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub pos: Pos,
    pub msg: String,
    pub hint: String,
}

impl Diag {
    pub fn new(pos: Pos, msg: impl Into<String>, hint: impl Into<String>) -> Self {
        Diag {
            pos,
            msg: msg.into(),
            hint: hint.into(),
        }
    }

    /// One-line form: `file:line:col: error: msg`.
    pub fn headline(&self, file: &str) -> String {
        format!(
            "{}:{}:{}: error: {}",
            file, self.pos.line, self.pos.col, self.msg
        )
    }

    /// Full form: headline, the source line with a caret under the column, and the hint.
    pub fn render(&self, file: &str, src: &str) -> String {
        let line_no = self.pos.line.max(1);
        let text = src.lines().nth(line_no as usize - 1).unwrap_or("");
        let gutter = line_no.to_string().len();
        let pad = " ".repeat(gutter);
        // Keep tabs so the caret lines up with the source as the terminal shows it.
        let caret_pad: String = text
            .chars()
            .take(self.pos.col.saturating_sub(1) as usize)
            .map(|c| if c == '\t' { '\t' } else { ' ' })
            .collect();
        let mut out = self.headline(file);
        out.push_str(&format!(
            "\n{pad} |\n{line_no} | {text}\n{pad} | {caret_pad}^"
        ));
        if !self.hint.is_empty() {
            out.push_str(&format!("\n{pad} = hint: {}", self.hint));
        }
        out
    }
}

pub type Result<T> = std::result::Result<T, Diag>;

/// Closest name within edit distance 2 (case-sensitive), for "did you mean" hints.
pub fn closest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let mut best: Option<(usize, &str)> = None;
    for c in candidates {
        let d = distance(name, c);
        let limit = if name.chars().count() <= 3 { 1 } else { 2 };
        if d <= limit && best.map_or(true, |(bd, bc)| d < bd || (d == bd && c < bc)) {
            best = Some((d, c));
        }
    }
    best.map(|(_, c)| c)
}

fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_has_line_caret_and_hint() {
        let d = Diag::new(Pos { line: 2, col: 5 }, "boom", "try this");
        let r = d.render("f.dov", "fn main() {\n    x = 2;\n}\n");
        assert_eq!(
            r,
            "f.dov:2:5: error: boom\n  |\n2 |     x = 2;\n  |     ^\n  = hint: try this"
        );
    }

    #[test]
    fn render_keeps_tabs_and_survives_eof_line() {
        let d = Diag::new(Pos { line: 1, col: 3 }, "m", "h");
        assert!(d.render("f", "\tab").contains("\n  | \t ^"));
        let d = Diag::new(Pos { line: 9, col: 1 }, "m", "h");
        assert!(d.render("f", "x").contains("9 | \n  | ^"));
    }

    #[test]
    fn closest_names() {
        assert_eq!(closest("totl", ["total", "name"]), Some("total"));
        assert_eq!(closest("zzz", ["total"]), None);
        assert_eq!(closest("ab", ["xy"]), None);
    }
}
