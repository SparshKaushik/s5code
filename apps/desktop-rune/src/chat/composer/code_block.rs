//! Code blocks in the prompt (composer-code-block.ts, composer-code-languages.ts,
//! ComposerCodeBlockLanguagePicker): Enter on a "```lang" line opens one, Enter after a closing
//! fence ends it, Tab indents inside it, and a send writes each as a fenced block. The editor
//! draws no widgets inline, so the language picker heads the composer and lists in a popover.
//! A draft keeps one info string per block, in block order (`Draft::code_langs`).

use std::ops::Range;

use rune::{Align, Block, Cursor, El, Side, TextEdit, col, icon, row, text};

use super::{Draft, act, draft, draft_mut, keyed};
use crate::App;
use crate::icons;
use crate::theme::{self, T, radius};

/// CODE_BLOCK_LANGUAGES.
const LANGUAGES: &[(&str, &str)] = &[
    ("", "Plain text"),
    ("asm", "Assembly"),
    ("bash", "Bash"),
    ("c", "C"),
    ("cpp", "C++"),
    ("csharp", "C#"),
    ("css", "CSS"),
    ("dart", "Dart"),
    ("diff", "Diff"),
    ("dockerfile", "Dockerfile"),
    ("elixir", "Elixir"),
    ("go", "Go"),
    ("graphql", "GraphQL"),
    ("haskell", "Haskell"),
    ("html", "HTML"),
    ("java", "Java"),
    ("javascript", "JavaScript"),
    ("json", "JSON"),
    ("jsx", "JSX"),
    ("kotlin", "Kotlin"),
    ("lua", "Lua"),
    ("markdown", "Markdown"),
    ("php", "PHP"),
    ("powershell", "PowerShell"),
    ("python", "Python"),
    ("r", "R"),
    ("ruby", "Ruby"),
    ("rust", "Rust"),
    ("scala", "Scala"),
    ("scss", "SCSS"),
    ("sql", "SQL"),
    ("svelte", "Svelte"),
    ("swift", "Swift"),
    ("toml", "TOML"),
    ("tsx", "TSX"),
    ("typescript", "TypeScript"),
    ("vue", "Vue"),
    ("xml", "XML"),
    ("yaml", "YAML"),
    ("zig", "Zig"),
];

/// LANGUAGE_ALIASES.
const ALIASES: &[(&str, &str)] = &[
    ("assembly", "asm"),
    ("cs", "csharp"),
    ("c++", "cpp"),
    ("js", "javascript"),
    ("md", "markdown"),
    ("plaintext", ""),
    ("py", "python"),
    ("rb", "ruby"),
    ("rs", "rust"),
    ("s", "asm"),
    ("sh", "bash"),
    ("shell", "bash"),
    ("text", ""),
    ("ts", "typescript"),
    ("txt", ""),
    ("yml", "yaml"),
];

const INDENT: &str = "  ";

/// codeLanguageEntry: `(id, label)`.
fn entry(language: &str) -> Option<(&'static str, &'static str)> {
    let lower = language.to_lowercase();
    let id = ALIASES.iter().find(|(alias, _)| *alias == lower).map_or(lower.as_str(), |(_, id)| id);
    LANGUAGES.iter().copied().find(|(known, _)| *known == id)
}

/// codeLanguageLabel.
fn label(language: &str) -> String {
    entry(language).map_or_else(|| language.to_owned(), |(_, label)| label.to_owned())
}

/// languageOfInfoString: the info string's first word.
fn language_of(info: &str) -> &str {
    info.split_whitespace().next().unwrap_or("")
}

/// withInfoStringLanguage: plain text clears the language unless trailing metadata would be read
/// as one.
fn with_language(info: &str, language: &str) -> String {
    let leading = &info[..info.len() - info.trim_start().len()];
    let rest = &info[leading.len() + language_of(info).len()..];
    if !language.is_empty() {
        format!("{leading}{language}{rest}")
    } else if !rest.trim().is_empty() {
        format!("{leading}text{rest}")
    } else {
        String::new()
    }
}

/// parseOpeningFence: three or more backticks or tildes, then the info string.
fn opening_fence(line: &str) -> Option<&str> {
    let fence = line.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let info = line.trim_start_matches(fence);
    (line.len() - info.len() >= 3 && !(fence == '`' && info.contains('`'))).then_some(info)
}

/// isClosingFence's run: the fence character and its length, when the line is only a fence.
fn closing_run(line: &str) -> Option<(char, usize)> {
    let fence = line.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let rest = line.trim_start_matches(fence);
    let rest = rest.strip_suffix('\r').unwrap_or(rest);
    let len = line.len() - line.trim_start_matches(fence).len();
    (len >= 3 && rest.chars().all(|c| matches!(c, ' ' | '\t'))).then_some((fence, len))
}

/// The prompt's code blocks as line ranges: runs of `Block::Code` lines.
fn runs(blocks: &[Block]) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        if *b != Block::Code {
            continue;
        }
        match out.last_mut() {
            Some(run) if run.end == i => run.end = i + 1,
            _ => out.push(i..i + 1),
        }
    }
    out
}

/// The caret is in a code block.
pub fn in_block(edit: &TextEdit) -> bool {
    edit.blocks().get(edit.line_of_byte(edit.range().0)) == Some(&Block::Code)
}

/// The block the picker sets: the one at the caret, else the last; with the block count.
fn target(edit: &TextEdit) -> Option<(usize, usize)> {
    let runs = runs(edit.blocks());
    let line = edit.line_of_byte(edit.focus);
    let n = runs.iter().position(|r| r.contains(&line)).or(runs.len().checked_sub(1))?;
    Some((n, runs.len()))
}

/// convertCodeFenceOnEnter: Enter at the end of a "```lang" line turns it into an empty block.
pub fn open(edit: &mut TextEdit, langs: &mut Vec<String>) -> bool {
    let (a, b) = edit.range();
    let line = edit.line_of_byte(a);
    let range = edit.line_range(line);
    let pilled = edit.pills().any(|(r, _)| r.start < range.end && r.end > range.start);
    if a != b || a != range.end || pilled || edit.blocks().get(line).is_some_and(|b| *b != Block::Paragraph) {
        return false;
    }
    let Some(info) = opening_fence(&edit.text[range.clone()]).map(str::to_owned) else { return false };
    let before = runs(edit.blocks());
    edit.undo_group(|e| {
        e.set_selection(range.start, range.end);
        e.insert("");
        e.set_block(Block::Code);
    });
    // A fence right beside a block joins it, which keeps that block's language.
    if runs(edit.blocks()).len() > before.len() {
        langs.resize(before.len(), String::new());
        langs.insert(before.iter().filter(|r| r.end <= line).count(), info);
    }
    true
}

/// exitCodeBlockOnClosingFence: Enter after a closing fence in a block ends the block there.
pub fn close(edit: &mut TextEdit) -> bool {
    let (a, b) = edit.range();
    let line = edit.line_of_byte(a);
    let range = edit.line_range(line);
    if a != b || a != range.end || closing_run(&edit.text[range.clone()]).is_none() {
        return false;
    }
    let Some(run) = runs(edit.blocks()).into_iter().find(|r| r.contains(&line)) else { return false };
    let end = edit.line_range(run.end - 1).end - range.len();
    edit.undo_group(|e| {
        e.set_selection(range.start, range.end);
        e.insert("");
        e.set_selection(range.start, end);
        e.set_block(Block::Paragraph);
        e.set_caret(range.start);
    });
    true
}

/// indentedNewlineInCodeBlock: Enter keeps the line's indent; false for an unindented line.
pub fn indented_newline(edit: &mut TextEdit) -> bool {
    let (a, _) = edit.range();
    let start = edit.line_range(edit.line_of_byte(a)).start;
    let before = &edit.text[start..a];
    let indent = &before[..before.len() - before.trim_start_matches([' ', '\t']).len()];
    if indent.is_empty() {
        return false;
    }
    let inserted = format!("\n{indent}");
    edit.insert(&inserted);
    true
}

/// indentCodeBlock: Tab indents the caret or each selected line by two spaces; Shift+Tab
/// outdents them.
pub fn indent(edit: &mut TextEdit, out: bool) -> bool {
    let (a, b) = edit.range();
    if a == b && !out {
        edit.insert(INDENT);
        return true;
    }
    let (first, last) = (edit.line_of_byte(a), edit.line_of_byte(b));
    edit.undo_group(|e| {
        for line in (first..=last).rev() {
            if e.blocks().get(line) != Some(&Block::Code) {
                continue;
            }
            let r = e.line_range(line);
            let (cut, empty) = {
                let t = &e.text[r.clone()];
                (if t.starts_with(INDENT) { 2 } else { usize::from(t.starts_with([' ', '\t'])) }, t.is_empty())
            };
            if out && cut > 0 {
                e.set_selection(r.start, r.start + cut);
                e.insert("");
            } else if !out && !empty {
                e.set_caret(r.start);
                e.insert(INDENT);
            }
        }
        let (start, end) = (e.line_range(first).start, e.line_range(last).end);
        e.set_selection(start, end);
    });
    true
}

/// The code lines of a draft, for storing it.
pub fn lines(edit: &TextEdit) -> Vec<usize> {
    edit.blocks().iter().enumerate().filter(|(_, b)| **b == Block::Code).map(|(i, _)| i).collect()
}

/// A stored draft's code lines back on its text.
pub fn restore(edit: &mut TextEdit, lines: &[usize]) {
    if lines.is_empty() {
        return;
    }
    let mut blocks = vec![Block::Paragraph; edit.text.split('\n').count()];
    for &i in lines {
        if let Some(b) = blocks.get_mut(i) {
            *b = Block::Code;
        }
    }
    edit.set_blocks(blocks);
}

/// The prompt as web's serializer writes it (codeBlockSource): each block as ```` ```info ````,
/// its code, then a closing fence, the fence longer than any fence in the code. `None` when the
/// prompt has no block.
pub fn fenced(edit: &TextEdit, infos: &[String]) -> Option<TextEdit> {
    let runs = runs(edit.blocks());
    if runs.is_empty() {
        return None;
    }
    let mut out = edit.clone();
    out.clear_history();
    out.set_blocks(Vec::new());
    for (n, run) in runs.iter().enumerate().rev() {
        let (start, end) = (out.line_range(run.start).start, out.line_range(run.end - 1).end);
        let longest = out.text[start..end].split('\n').filter_map(closing_run).filter(|(c, _)| *c == '`').map(|(_, len)| len).max();
        let fence = "`".repeat(longest.map_or(3, |len| len + 1));
        let info = infos.get(n).map_or("", String::as_str);
        let opening = if start == end { format!("{fence}{info}") } else { format!("{fence}{info}\n") };
        out.set_caret(end);
        out.insert(&format!("\n{fence}"));
        out.set_caret(start);
        out.insert(&opening);
    }
    Some(out)
}

/// LanguageIcon: the language's file icon where there is one.
fn glyph(language: &str, t: &T) -> El {
    match language {
        "typescript" | "javascript" | "json" | "bash" | "python" | "rust" | "tsx" | "jsx" | "css" | "html" | "markdown" => {
            crate::chat::timeline::language_icons::language(language, t)
        }
        _ => icon(&icons::CODE, 14.0).color(t.c.muted_foreground),
    }
}

/// The picker's trigger, for the block at the caret (else the last), above the editor.
pub fn header(s: &App, t: &T, d: &Draft) -> Option<El> {
    let (n, _) = target(&d.edit)?;
    let language = language_of(d.code_langs.get(n).map_or("", String::as_str));
    let shown = label(language);
    let trigger = row()
        .key(keyed(s, "composer-code-language"))
        .h(24.0)
        .px(6.0)
        .gap(6.0)
        .center_y()
        .radius(radius::MD)
        .hover(t.c.accent)
        .cursor(Cursor::Pointer)
        .role("button")
        .label(format!("Code language: {shown}"))
        .child(glyph(language, t))
        .child(text(shown).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().max_w(160.0))
        .child(icon(&icons::CHEVRON_DOWN, 12.0).color(t.c.muted_foreground).opacity(0.6))
        .on_press(act(s, |s: &mut App| s.ui.chat.composer.code_menu = !s.ui.chat.composer.code_menu));
    Some(row().w_full().px(10.0).pt(10.0).child(trigger))
}

/// The picker's list over the composer, the current language checked.
pub fn menu(s: &App, t: &T) -> Option<El> {
    if !s.ui.chat.composer.code_menu {
        return None;
    }
    let d = draft(s)?;
    let (n, _) = target(&d.edit)?;
    let language = language_of(d.code_langs.get(n).map_or("", String::as_str));
    let current = entry(language).map_or(language, |(id, _)| id).to_owned();
    let mut items: Vec<(String, String)> = LANGUAGES.iter().map(|(id, label)| ((*id).to_owned(), (*label).to_owned())).collect();
    if !LANGUAGES.iter().any(|(id, _)| *id == current) {
        items.insert(0, (current.clone(), current.clone()));
    }
    let rows = items.into_iter().map(|(id, name)| {
        let checked = id == current;
        let key = format!("composer-code-language-{}", if id.is_empty() { "plain" } else { id.as_str() });
        let mut item = row()
            .key(key)
            .w_full()
            .h(28.0)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(radius::MD)
            .hover(t.c.accent)
            .cursor(Cursor::Pointer)
            .role("menuitemradio")
            .child(glyph(&id, t))
            .child(text(name).font(theme::TEXT_SM).color(t.c.foreground).truncate().grow(1.0).shrink().min_w(0.0));
        if checked {
            item = item.child(icon(&icons::CHECK, 14.0).color(t.c.foreground));
        }
        item.on_press(act(s, move |s: &mut App| pick(s, &id)))
    });
    let list = col().w_full().max_h(280.0).scroll(rune::ScrollSpec { stick_bottom: false }).children(rows);
    let card = super::picker::menu_card(t, &keyed(s, "composer-code-language-menu")).w(224.0);
    Some(card.anchored(keyed(s, "composer-code-language"), Side::Top, Align::Start, 4.0).child(list).modal(true, act(s, |s: &mut App| s.ui.chat.composer.code_menu = false)))
}

/// Sets the picked language on the block, keeping the rest of its info string, and puts the caret
/// back in the prompt.
fn pick(s: &mut App, language: &str) {
    s.ui.chat.composer.code_menu = false;
    let d = draft_mut(s);
    if let Some((n, total)) = target(&d.edit) {
        d.code_langs.resize(total, String::new());
        let info = with_language(&d.code_langs[n], language);
        d.code_langs[n] = info;
    }
    d.edit.focused = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field holding `text` with the caret at its end.
    fn typed(text: &str) -> TextEdit {
        let mut e = TextEdit::new(text);
        e.set_caret(text.len());
        e
    }

    #[test]
    fn a_fence_and_enter_open_a_block_that_sends_fenced_with_its_language() {
        let mut langs = Vec::new();
        let mut e = typed("Why does this panic?\n```rust");
        assert!(open(&mut e, &mut langs), "FAILED: Enter on a fence opens a block");
        assert_eq!((e.text.as_str(), langs.as_slice()), ("Why does this panic?\n", &["rust".to_owned()][..]));
        e.insert("fn main() {");
        assert!(!close(&mut e) && !indented_newline(&mut e), "FAILED: code lines are not fences");
        e.insert("\n    let x = 1;");
        assert!(indented_newline(&mut e), "FAILED: Enter keeps the indent");
        e.insert("x\n}\n```");
        assert!(close(&mut e), "FAILED: a closing fence ends the block");
        e.insert("Thanks");
        assert_eq!(e.blocks()[1..5], [Block::Code; 4]);
        assert_eq!(e.blocks()[5], Block::Paragraph);
        let sent = fenced(&e, &langs).unwrap();
        assert_eq!(sent.text, "Why does this panic?\n```rust\nfn main() {\n    let x = 1;\n    x\n}\n```\nThanks");

        // The picker swaps the language and keeps trailing metadata; plain text clears it.
        assert_eq!(with_language("rust title=main.rs", "python"), "python title=main.rs");
        assert_eq!(with_language("rust title=main.rs", ""), "text title=main.rs");
        assert_eq!(with_language("rust", ""), "");
        assert_eq!(fenced(&e, &[String::new()]).unwrap().text, "Why does this panic?\n```\nfn main() {\n    let x = 1;\n    x\n}\n```\nThanks");
        assert!(fenced(&typed("no code"), &langs).is_none());
    }

    #[test]
    fn fences_inside_code_lengthen_the_outer_fence_and_empty_blocks_stay_closed() {
        let mut e = TextEdit::new("```md\nsay ```\n```\nok");
        e.set_blocks(vec![Block::Code, Block::Code, Block::Code, Block::Paragraph]);
        assert_eq!(fenced(&e, &["md".to_owned()]).unwrap().text, "````md\n```md\nsay ```\n```\n````\nok");
        let mut empty = TextEdit::new("");
        empty.set_blocks(vec![Block::Code]);
        assert_eq!(fenced(&empty, &["go".to_owned()]).unwrap().text, "```go\n```");
    }

    #[test]
    fn languages_resolve_aliases_and_only_real_fences_open_blocks() {
        assert_eq!(label("ts"), "TypeScript");
        assert_eq!(label("PY"), "Python");
        assert_eq!(label(""), "Plain text");
        assert_eq!(label("brainfuck"), "brainfuck");
        assert_eq!(opening_fence("```ts title"), Some("ts title"));
        assert_eq!(opening_fence("~~~"), Some(""));
        assert_eq!(opening_fence("``"), None);
        assert_eq!(opening_fence("```a`b"), None);
        assert_eq!(closing_run("```  "), Some(('`', 3)));
        assert_eq!(closing_run("```x"), None);
        let mut langs = vec!["stale".to_owned()];
        let mut e = typed("``");
        assert!(!open(&mut e, &mut langs));
        let mut e = typed("```");
        assert!(open(&mut e, &mut langs));
        assert_eq!(langs, vec![String::new()], "FAILED: a new block drops languages left from cleared ones");
    }

    #[test]
    fn tab_indents_and_shift_tab_outdents_code_lines() {
        let mut e = TextEdit::new("a\n  b\nc");
        e.set_blocks(vec![Block::Code, Block::Code, Block::Paragraph]);
        e.set_caret(1);
        assert!(indent(&mut e, false));
        assert_eq!(e.text, "a  \n  b\nc");
        e.set_selection(0, 6);
        assert!(indent(&mut e, true));
        assert_eq!(e.text, "a  \nb\nc");
        assert_eq!(lines(&e), vec![0, 1]);
        let mut back = TextEdit::new(e.text.clone());
        restore(&mut back, &lines(&e));
        assert_eq!(back.blocks(), e.blocks());
    }
}
