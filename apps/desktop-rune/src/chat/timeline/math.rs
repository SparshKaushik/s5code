//! Render math (ChatMarkdown's `remark-math`, micromark-extension-llm-math as patched): where
//! the formulas are. `katex` typesets them; until it has, or without Chrome, each reads as
//! MarkdownMath's fallback, its TeX source `\(tex\)` or `\[⏎tex⏎\]`, escaped so Markdown keeps
//! it literal.
use std::borrow::Cow;

/// A paragraph of prose as math sees it.
#[derive(Debug, PartialEq)]
pub enum Block {
    /// Markdown with no formulas.
    Text(String),
    /// A display formula on lines of its own.
    Display(String),
    /// Markdown holding inline formulas.
    Inline(Vec<Inline>),
}

#[derive(Debug, PartialEq)]
pub enum Inline {
    Text(String),
    Math { tex: String, display: bool },
}

/// Prose Markdown with every formula made literal TeX source; display blocks become paragraphs.
pub fn literal(source: &str) -> Cow<'_, str> {
    if !may_contain(source) {
        return Cow::Borrowed(source);
    }
    Cow::Owned(blocks(source).iter().map(Block::literal).collect::<Vec<_>>().join("\n\n"))
}

/// `source`'s paragraphs, display formulas split out as paragraphs of their own.
pub fn blocks(source: &str) -> Vec<Block> {
    source.split("\n\n").flat_map(paragraph).collect()
}

impl Block {
    /// The block as Markdown, its formulas literal TeX source.
    pub fn literal(&self) -> String {
        match self {
            Block::Text(text) => text.clone(),
            Block::Display(tex) => display(tex),
            Block::Inline(parts) => parts
                .iter()
                .map(|part| match part {
                    Inline::Text(text) => text.clone(),
                    Inline::Math { tex, display: true } => escape(&format!("\\[{tex}\\]")),
                    Inline::Math { tex, display: false } => escape(&format!("\\({tex}\\)")),
                })
                .collect(),
        }
    }
}

/// A closed ```` ```math ```` fence: one display formula.
pub fn display(tex: &str) -> String {
    escape(&format!("\\[\n{}\n\\]", tex.trim()))
}

/// `mayContainMath`: messages without these skip math parsing.
pub fn may_contain(text: &str) -> bool {
    text.contains('$') || text.contains("\\(") || text.contains("\\[")
}

/// Markdown that reads as `text`: ASCII punctuation escaped, newlines hard breaks, and each
/// line's leading spaces kept (`whitespace-pre-wrap`).
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    let mut line_start = true;
    for c in text.chars() {
        match c {
            '\r' => {}
            '\n' => {
                out.push_str("\\\n");
                line_start = true;
                continue;
            }
            ' ' if line_start => out.push('\u{a0}'),
            c if c.is_ascii_punctuation() => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
        line_start &= c == ' ';
    }
    out
}

fn paragraph(text: &str) -> Vec<Block> {
    let indented = text.trim_start_matches('\n').starts_with("    ") || text.trim_start_matches('\n').starts_with('\t');
    if !may_contain(text) || indented {
        return vec![Block::Text(text.to_owned())];
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    let (mut from, mut i) = (0, 0);
    while i < lines.len() {
        match flow(&lines[i..]) {
            Some((tex, used)) => {
                if from < i {
                    out.push(inline(&lines[from..i].join("\n")));
                }
                out.push(Block::Display(tex.trim().to_owned()));
                i += used;
                from = i;
            }
            None => i += 1,
        }
    }
    if from < lines.len() {
        out.push(inline(&lines[from..].join("\n")));
    }
    out
}

/// A display block opening at `lines[0]` (`$$` or `\[` fences, math-flow / math-tex-flow): its TeX
/// and how many lines it spans, or `None` when it isn't one or never closes.
fn flow(lines: &[&str]) -> Option<(String, usize)> {
    let first = lines[0];
    let body = first.trim_start_matches(' ');
    if first.len() - body.len() > 3 {
        return None;
    }
    let dollars = body.bytes().take_while(|b| *b == b'$').count();
    let (open, rest) = if dollars >= 2 {
        (dollars, &body[dollars..])
    } else if let Some(rest) = body.strip_prefix("\\[") {
        (0, rest)
    } else {
        return None;
    };
    let mut tex = String::new();
    for (n, line) in std::iter::once(rest).chain(lines[1..].iter().copied()).enumerate() {
        if n > 0 {
            tex.push('\n');
        }
        match close(line, open) {
            Close::At(end) => {
                tex.push_str(&line[..end]);
                return Some((tex, n + 1));
            }
            Close::Trailing => return None,
            Close::None => tex.push_str(line),
        }
    }
    None
}

enum Close {
    /// The fence closes at this byte, the rest of the line blank.
    At(usize),
    /// A closing fence with text after it: no block.
    Trailing,
    None,
}

/// The first closing fence in a block's line: `$` × `open` or more (an `open` of 0 means `\]`).
fn close(line: &str, open: usize) -> Close {
    let b = line.as_bytes();
    let (mut i, mut slashes) = (0, 0);
    while i < b.len() {
        let escaped = slashes % 2 == 1;
        let end = match b[i] {
            b'$' if open > 0 && !escaped => {
                let run = b[i..].iter().take_while(|c| **c == b'$').count();
                (run >= open).then_some(i + run)
            }
            b'\\' if open == 0 && b.get(i + 1) == Some(&b']') => Some(i + 2),
            _ => None,
        };
        if let Some(end) = end {
            return if line[end..].trim().is_empty() { Close::At(i) } else { Close::Trailing };
        }
        slashes = if b[i] == b'\\' { slashes + 1 } else { 0 };
        i += 1;
    }
    Close::None
}

/// Inline formulas (math-text, math-tex-text) split out, skipping code spans, escapes, link
/// destinations and autolinks as Markdown does.
fn inline(text: &str) -> Block {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let (mut copied, mut i) = (0, 0);
    while i < b.len() {
        let found = match b[i] {
            b'`' => {
                i = code_span(b, i);
                continue;
            }
            b'\\' if matches!(b.get(i + 1), Some(b'(' | b'[')) => tex_text(b, i),
            b'\\' => {
                i += if b.get(i + 1).is_some_and(u8::is_ascii_punctuation) { 2 } else { 1 };
                continue;
            }
            b'$' => dollar_text(b, i),
            b']' if b.get(i + 1) == Some(&b'(') => {
                i = destination(b, i + 1);
                continue;
            }
            b'<' => {
                i = autolink(b, i);
                continue;
            }
            _ => None,
        };
        match found {
            Some((tex, display, end)) => {
                if copied < i {
                    out.push(Inline::Text(text[copied..i].to_owned()));
                }
                out.push(Inline::Math { tex: text[tex].trim().to_owned(), display });
                copied = end;
                i = end;
            }
            // A failed `$` run can't open again from inside itself.
            None if b[i] == b'$' => i += b[i..].iter().take_while(|c| **c == b'$').count(),
            // Not a formula: `\(` or `\[` is an escape.
            None if b[i] == b'\\' => i += 2,
            None => i += 1,
        }
    }
    if out.is_empty() {
        return Block::Text(text.to_owned());
    }
    if copied < text.len() {
        out.push(Inline::Text(text[copied..].to_owned()));
    }
    Block::Inline(out)
}

type Found = Option<(std::ops::Range<usize>, bool, usize)>;

/// `$…$` or `$$…$$` at `at`: the price, shell-variable and URL rules of the patched math-text.
fn dollar_text(b: &[u8], at: usize) -> Found {
    let open = b[at..].iter().take_while(|c| **c == b'$').count();
    let start = at + open;
    let first = *b.get(start)?;
    if matches!(first, b'\n' | b'\r') {
        return None;
    }
    let open_ws = matches!(first, b' ' | b'\t' | b',');
    if open == 1 && open_ws {
        return None;
    }
    let (mut k, mut slashes, mut prev, mut before) = (start, 0, 0u8, 0u8);
    while k < b.len() {
        let c = b[k];
        if c == b'`' || (open == 1 && matches!(c, b'\n' | b'\r')) {
            return None;
        }
        if c == b'$' && slashes % 2 == 0 {
            let size = b[k..].iter().take_while(|c| **c == b'$').count();
            if size != open {
                k += size;
                slashes = 0;
                continue;
            }
            let before_close = b[k - 1];
            let next = b.get(k + size).copied();
            if matches!(before_close, b'\n' | b'\r') || next.is_some_and(|n| n.is_ascii_digit()) {
                return None;
            }
            if before_close == b',' && !open_ws && next.is_some_and(|n| !n.is_ascii_whitespace() && !b".,;:)]}".contains(&n)) {
                return None;
            }
            let close_ws = matches!(before_close, b' ' | b'\t') || (before_close == b',' && open_ws);
            return (open_ws == close_ws).then_some((start..k, open > 1, k + size));
        }
        if c == b'/' && prev == b'/' && before == b':' {
            return None;
        }
        before = prev;
        prev = c;
        slashes = if c == b'\\' { slashes + 1 } else { 0 };
        k += 1;
    }
    None
}

/// `\(…\)` or `\[…\]` at `at`; a bracket pair needs more than digits, commas and spaces.
fn tex_text(b: &[u8], at: usize) -> Found {
    let close = if b[at + 1] == b'(' { b')' } else { b']' };
    let (mut k, mut substantive) = (at + 2, false);
    while k < b.len() {
        match b[k] {
            b'`' => return None,
            b'\\' if b.get(k + 1) == Some(&close) => {
                return (close == b')' || substantive).then_some((at + 2..k, close == b']', k + 2));
            }
            b'\\' if b.get(k + 1) == Some(&b'\\') => k += 2,
            b'\\' => k += 1,
            c => {
                substantive |= !matches!(c, b' ' | b',' | b'\n' | b'\r') && !c.is_ascii_digit();
                k += 1;
            }
        }
    }
    None
}

/// Past a run of backticks: its whole code span when a run as long closes it.
fn code_span(b: &[u8], at: usize) -> usize {
    let n = b[at..].iter().take_while(|c| **c == b'`').count();
    let mut k = at + n;
    while k < b.len() {
        let run = b[k..].iter().take_while(|c| **c == b'`').count();
        if run == n {
            return k + run;
        }
        k += run.max(1);
    }
    at + n
}

/// Past a link's `(destination)`, parentheses balanced and escapes honoured.
fn destination(b: &[u8], open: usize) -> usize {
    let (mut depth, mut k) = (0, open);
    while k < b.len() {
        match b[k] {
            b'\\' => k += 1,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return k + 1;
                }
            }
            b'\n' => break,
            _ => {}
        }
        k += 1;
    }
    open
}

/// Past an autolink `<scheme:…>`.
fn autolink(b: &[u8], at: usize) -> usize {
    let end = b[at + 1..].iter().position(|c| c.is_ascii_whitespace() || matches!(c, b'<' | b'>')).map(|p| at + 1 + p);
    match end {
        Some(end) if b[end] == b'>' && b[at + 1..end].contains(&b':') => end + 1,
        _ => at + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The formulas `literal` found, as the TeX source they now read as.
    fn shown(source: &str) -> String {
        let out = literal(source);
        pulldown_cmark::Parser::new(&out)
            .map(|e| match e {
                pulldown_cmark::Event::Text(t) => t.replace('\u{a0}', " "),
                pulldown_cmark::Event::Code(c) => format!("`{c}`"),
                pulldown_cmark::Event::HardBreak => "\n".to_owned(),
                pulldown_cmark::Event::SoftBreak => " ".to_owned(),
                pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Paragraph) => "\n\n".to_owned(),
                _ => String::new(),
            })
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    #[test]
    fn the_four_delimiters_read_as_tex_source() {
        assert_eq!(shown("Inline $x^2$ and \\(y_1\\), display $$a+b$$ and \\[c\\]."), "Inline \\(x^2\\) and \\(y_1\\), display \\[a+b\\] and \\[c\\].");
        assert_eq!(shown("$$\n\\frac{a}{b}\n$$"), "\\[\n\\frac{a}{b}\n\\]");
        assert_eq!(shown("Where\n\\[\nx \\\\\n  - y\n\\]\nholds."), "Where\n\n\\[\nx \\\\\n  - y\n\\]\n\nholds.");
        assert_eq!(shown("$$x^2\n+y^2$$"), "\\[\nx^2\n+y^2\n\\]");
        assert_eq!(shown("$$a$$ is first.\nSome prose.\n$$\nb\n$$"), "\\[a\\] is first. Some prose.\n\n\\[\nb\n\\]");
        assert_eq!(shown("Where $x = 1,$ with $$ b,$$."), "Where \\(x = 1,\\) with \\[b,\\].");
        assert_eq!(display("x_1 * y_2\n"), "\\\\\\[\\\nx\\_1 \\* y\\_2\\\n\\\\\\]");
    }

    #[test]
    fn blocks_split_display_formulas_out_and_keep_inline_ones_in_place() {
        let math = |tex: &str, display| Inline::Math { tex: tex.to_owned(), display };
        assert_eq!(
            blocks("Where $x$ holds.\n$$\na\n$$\n\nNo math here."),
            [Block::Inline(vec![Inline::Text("Where ".into()), math("x", false), Inline::Text(" holds.".into())]), Block::Display("a".into()), Block::Text("No math here.".into())]
        );
        assert_eq!(blocks("It costs $5."), [Block::Text("It costs $5.".into())]);
    }

    #[test]
    fn prices_shell_variables_code_and_links_stay_prose() {
        for prose in [
            "It costs $5 and $10.",
            "Budget $5-$10 per seat, or $20,000-$30,000 a year.",
            "Plans are $5/$10 monthly.",
            "Pay $5,$10 now.",
            "Run $browser then $deploy on it.",
            "Set $HOME,$PATH before running.",
            "Set $HOME, $PATH before running.",
            "Set $HOME,$(pwd) before running.",
            "Pay $1,000,$2,000 now.",
            "$$E=mc^2",
            "Use `$x$` in code.",
            "Set $TOKEN, then open [users](https://graph.microsoft.com/v1.0/users?$select=id).",
            "Set $TOKEN, then open <https://graph.microsoft.com/v1.0/users?$select=id>.",
            "Escaped \\$x\\$ stays.",
            "As shown in \\[1\\] and \\[2, 3\\].",
            "    $x$ is indented code",
        ] {
            assert_eq!(literal(prose), prose, "{prose}");
        }
        assert_eq!(shown("Price $5-$10 stays. The formula is $x_i$ and $5$."), "Price $5-$10 stays. The formula is \\(x_i\\) and \\(5\\).");
        assert_eq!(shown("Pay \\$$x$ now."), "Pay $\\(x\\) now.");
        assert!(matches!(literal("No formulas here."), Cow::Borrowed(_)));
    }
}
