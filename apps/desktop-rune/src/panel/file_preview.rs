//! FilePreviewPanel's preview modes around the editor: what a file is (packages/shared
//! filePreview), the rendered markdown, table and notebook views, and AttachmentFilePreview's
//! "No preview for this file" for formats Rune cannot draw (audio, video, PDF, Office, fonts,
//! archives). Mermaid stays source: no existing crate lays out its diagrams.

use rune::{El, col, el, row, text};
use serde_json::Value;

use crate::theme::{self, AlphaMul, T};
use crate::{App, kit};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Text,
    Markdown,
    Mermaid,
    Html,
    Table(char),
    Notebook,
    Image,
    Audio,
    Video,
    Pdf,
    Office,
    Font,
    Archive,
}

/// `filePreviewKind` for a workspace path, by its extension as the web reads a generic MIME type.
pub fn kind(path: &str) -> Kind {
    let lower = path.to_lowercase();
    let name = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
    match ext {
        "avif" | "bmp" | "gif" | "ico" | "heic" | "heif" | "jpeg" | "jpg" | "png" | "svg" | "tif" | "tiff" | "webp" => Kind::Image,
        "avi" | "m4v" | "mkv" | "mov" | "mp4" | "ogv" | "webm" => Kind::Video,
        "mp3" | "wav" | "ogg" | "oga" | "flac" | "aac" | "m4a" | "opus" | "aiff" | "weba" => Kind::Audio,
        "pdf" => Kind::Pdf,
        "htm" | "html" => Kind::Html,
        "md" | "mdx" | "markdown" | "mdown" | "mkd" => Kind::Markdown,
        "mmd" | "mermaid" => Kind::Mermaid,
        "csv" => Kind::Table(','),
        "tsv" => Kind::Table('\t'),
        "ipynb" => Kind::Notebook,
        "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" | "epub" => Kind::Office,
        "ttf" | "otf" | "woff" | "woff2" => Kind::Font,
        "zip" | "jar" | "tar" | "gz" | "tgz" | "sqlite" | "sqlite3" | "db" => Kind::Archive,
        _ => Kind::Text,
    }
}

impl Kind {
    /// Read as text through `projects.readFile`.
    pub fn reads_text(self) -> bool {
        matches!(self, Kind::Text | Kind::Markdown | Kind::Mermaid | Kind::Html | Kind::Table(_) | Kind::Notebook)
    }
    /// `isBrowserPreviewFile`: the preview browser can open it.
    pub fn opens_in_browser(self) -> bool {
        matches!(self, Kind::Html | Kind::Pdf)
    }
    /// The rendered/source toggle, for the modes Rune can draw rendered.
    pub fn rendered_mode(self) -> Option<&'static str> {
        match self {
            Kind::Markdown => Some("markdown"),
            Kind::Table(_) => Some("table"),
            _ => None,
        }
    }
    /// Shown as "No preview for this file" with Open externally and Save.
    pub fn unpreviewable(self) -> bool {
        matches!(self, Kind::Audio | Kind::Video | Kind::Pdf | Kind::Office | Kind::Font | Kind::Archive)
    }
}

/// `renderedToggleLabel`.
pub fn toggle_label(mode: &str, rendered: bool) -> &'static str {
    match (mode, rendered) {
        ("mermaid", true) => "Show Mermaid source",
        ("mermaid", false) => "Show rendered diagram",
        ("markdown", true) => "Show markdown source",
        ("markdown", false) => "Show rendered markdown",
        ("table", true) => "Show source",
        ("table", false) => "Show table",
        (_, true) => "Show HTML source",
        (_, false) => "Show rendered page",
    }
}

#[derive(Debug, PartialEq)]
pub struct Table {
    pub rows: Vec<Vec<String>>,
    pub truncated: bool,
}

/// `parseDelimitedPreview`: quoted cells, doubled quotes, CRLF, a leading BOM, and limits on
/// rows, columns and cell length (in characters where the web counts UTF-16 units).
pub fn parse_delimited(text: &str, delimiter: char, max_rows: usize, max_columns: usize, max_cell: usize) -> Table {
    let chars: Vec<char> = text.chars().collect();
    let (mut rows, mut row, mut cell) = (Vec::new(), Vec::new(), String::new());
    let (mut cell_len, mut quoted, mut truncated) = (0usize, false, false);
    let mut i = usize::from(chars.first() == Some(&'\u{feff}'));
    let mut row_start = i;
    let end_cell = |row: &mut Vec<String>, cell: &mut String, cell_len: &mut usize, truncated: &mut bool| {
        if *cell_len > max_cell {
            *cell = cell.chars().take(max_cell).collect();
            *truncated = true;
        }
        if row.len() < max_columns {
            row.push(std::mem::take(cell));
        } else {
            *truncated = true;
            cell.clear();
        }
        *cell_len = 0;
    };
    while i < chars.len() {
        let ch = chars[i];
        if ch == '"' {
            if quoted && chars.get(i + 1) == Some(&'"') {
                if cell_len <= max_cell {
                    cell.push('"');
                    cell_len += 1;
                } else {
                    truncated = true;
                }
                i += 2;
                continue;
            }
            if quoted || cell.is_empty() {
                quoted = !quoted;
                i += 1;
                continue;
            }
        }
        if !quoted && (ch == delimiter || ch == '\n' || ch == '\r') {
            end_cell(&mut row, &mut cell, &mut cell_len, &mut truncated);
            if ch != delimiter {
                rows.push(std::mem::take(&mut row));
                if ch == '\r' && chars.get(i + 1) == Some(&'\n') {
                    i += 1;
                }
                row_start = i + 1;
                if rows.len() == max_rows {
                    return Table { rows, truncated: truncated || i + 1 < chars.len() };
                }
            }
        } else if cell_len <= max_cell {
            cell.push(ch);
            cell_len += 1;
        } else {
            truncated = true;
        }
        i += 1;
    }
    if row_start < chars.len() {
        end_cell(&mut row, &mut cell, &mut cell_len, &mut truncated);
        rows.push(row);
    }
    Table { rows, truncated: truncated || quoted }
}

#[derive(Debug, PartialEq)]
pub struct Notebook {
    pub language: String,
    pub cells: Vec<Cell>,
}

#[derive(Debug, PartialEq)]
pub struct Cell {
    pub kind: &'static str,
    pub source: String,
    pub outputs: Vec<String>,
}

fn notebook_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

/// `openNotebook`: version 4 notebooks with up to 500 cells, saved text outputs only.
pub fn notebook(source: &str) -> Result<Notebook, String> {
    let v: Value = serde_json::from_str(source).map_err(|_| "This notebook is not valid JSON.".to_owned())?;
    let cells = v["cells"].as_array().filter(|c| v["nbformat"] == 4 && c.len() <= 500).ok_or("Preview supports version 4 notebooks with up to 500 cells.")?;
    let language: String = v["metadata"]["language_info"]["name"].as_str().unwrap_or("text").chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '-')).take(40).collect();
    let cells = cells
        .iter()
        .map(|cell| {
            let outputs = cell["outputs"].as_array().map_or(&[][..], Vec::as_slice);
            if outputs.len() > 100 {
                return Err("A notebook cell has too many outputs to preview.".to_owned());
            }
            Ok(Cell {
                kind: match cell["cell_type"].as_str() {
                    Some("markdown") => "markdown",
                    Some("code") => "code",
                    _ => "raw",
                },
                source: notebook_text(&cell["source"]),
                outputs: outputs
                    .iter()
                    .map(|o| {
                        let text = [&o["text"], &o["data"]["text/plain"], &o["traceback"]].into_iter().find(|v| !v.is_null()).map(notebook_text).unwrap_or_default();
                        strip_ansi(&text)
                    })
                    .filter(|t| !t.trim().is_empty())
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Notebook { language, cells })
}

/// Tracebacks carry terminal colour codes.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

// Views.

fn status(line: String, t: &T) -> El {
    row().w_full().px(12.).py(8.).child(text(line).font(theme::TEXT_XS).color(t.c.muted_foreground))
}

/// The markdown rendered as chat renders a message, in web's `max-w-4xl` reading column.
pub fn markdown(app: &App, path: &str, source: &str, t: &T) -> El {
    col().grow(1.).min_h(0.).scroll(rune::ScrollSpec { stick_bottom: false }).child(
        row().w_full().justify_center().child(
            col().w_full().max_w(896.).px(24.).py(20.).child(crate::chat::timeline::markdown::view(app, source, &format!("file-{path}"), t)),
        ),
    )
}

/// LargeTextViewer's table: "N rows · C columns", the first row as the header.
pub fn table(parsed: &Table, t: &T) -> El {
    let columns = parsed.rows.iter().map(Vec::len).max().unwrap_or(0);
    let body_rows = parsed.rows.len().saturating_sub(1);
    let mut summary = format!("{body_rows} rows · {columns} columns");
    if parsed.truncated {
        summary.push_str(" · Preview limited");
    }
    let cell_w = |c: usize| -> f32 {
        let longest = parsed.rows.iter().take(200).filter_map(|r| r.get(c)).map(|v| v.chars().count()).max().unwrap_or(0);
        (longest as f32 * 8. + 32.).clamp(64., 320.)
    };
    let widths: Vec<f32> = (0..columns).map(cell_w).collect();
    let mut grid = col();
    for (i, cells) in parsed.rows.iter().enumerate() {
        let header = i == 0;
        let mut line = row().fill(if header { t.c.muted.alpha_mul(0.5) } else { rune::Color::TRANSPARENT });
        for (c, w) in widths.iter().enumerate() {
            let value = cells.get(c).cloned().unwrap_or_default();
            let font = if header { theme::XS_MEDIUM } else { theme::TEXT_XS };
            line = line.child(
                el().w(*w).px(12.).py(6.).child(text(value.clone()).font(font).color(t.c.foreground).single_line().truncate().help_when_truncated(value)),
            );
        }
        grid = grid.child(line).child(el().h(1.).w_full().fill(t.c.border));
    }
    col()
        .grow(1.)
        .min_h(0.)
        .child(status(summary, t))
        .child(el().h(1.).w_full().fill(t.c.border))
        .child(el().grow(1.).min_h(0.).min_w(0.).scroll_xy().child(grid))
}

/// NotebookPreview: a count, the "never executed" notice, then each cell with its saved output.
pub fn notebook_view(app: &App, path: &str, nb: &Notebook, t: &T) -> El {
    let mut cells = col().w_full().max_w(896.).gap(20.).px(20.).py(24.);
    for (i, cell) in nb.cells.iter().enumerate() {
        let label = if cell.kind == "code" { nb.language.as_str() } else { cell.kind };
        let mut card = col()
            .w_full()
            .radius(6.)
            .border(1., t.c.border.alpha_mul(0.6))
            .clip_rounded(6.)
            .role("region")
            .label(format!("Cell {}", i + 1))
            .child(row().w_full().px(12.).py(6.).fill(t.c.muted.alpha_mul(0.3)).child(text(format!("{label} · {}", i + 1)).font(theme::TEXT_XS).color(t.c.muted_foreground)))
            .child(el().h(1.).w_full().fill(t.c.border.alpha_mul(0.6)));
        if cell.kind == "markdown" {
            card = card.child(col().w_full().px(16.).py(12.).child(crate::chat::timeline::markdown::view(app, &cell.source, &format!("nb-{path}-{i}"), t)));
        } else if !cell.source.trim().is_empty() {
            let lang = if cell.kind == "code" { Some(nb.language.as_str()) } else { None };
            let spans = rune_kit::syntax::SyntaxTheme::neutral(t.dark).spans(&rune_kit::syntax::highlight(lang, &cell.source), 0);
            card = card.child(row().w_full().px(12.).py(8.).child(text(cell.source.clone()).font(theme::mono(theme::TEXT_XS)).color(t.c.foreground).spans(spans).wrap().selectable(true)));
        }
        for output in &cell.outputs {
            card = card
                .child(el().h(1.).w_full().fill(t.c.border.alpha_mul(0.6)))
                .child(row().w_full().px(12.).py(8.).child(text(output.clone()).font(theme::mono(theme::TEXT_XS)).color(t.c.muted_foreground).wrap_anywhere().selectable(true)));
        }
        cells = cells.child(card);
    }
    col()
        .grow(1.)
        .min_h(0.)
        .child(status(format!("{} cells · {}", nb.cells.len(), nb.language), t))
        .child(el().h(1.).w_full().fill(t.c.border.alpha_mul(0.6)))
        .child(row().w_full().px(12.).py(6.).fill(t.c.muted.alpha_mul(0.3)).child(text("Saved outputs only. Code is never executed.").font(theme::TEXT_XS).color(t.c.muted_foreground)))
        .child(col().grow(1.).min_h(0.).scroll(rune::ScrollSpec { stick_bottom: false }).child(row().w_full().justify_center().child(cells)))
}

/// AttachmentFilePreview's unsupported state, or OfficeViewer's with the reason it failed;
/// Rune adds the way out web's browser has built in: open it in another app, or save it.
pub fn no_preview(name: &str, reason: Option<&str>, t: &T, open: impl Fn(&mut App) + 'static, save: impl Fn(&mut App, &mut rune::Tx) + 'static) -> El {
    let ext = name.rsplit_once('.').map(|(_, e)| e).filter(|e| !e.is_empty()).unwrap_or("this format");
    let detail = match reason {
        Some(reason) => format!("{reason} Save it or open it in another app."),
        None => format!("Save it to open in an app that supports {ext} files."),
    };
    col()
        .grow(1.)
        .min_h(0.)
        .center()
        .gap(4.)
        .px(24.)
        .role("status")
        .child(text("No preview for this file").font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(row().max_w(384.).child(text(detail).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()))
        .child(
            row()
                .pt(12.)
                .gap(8.)
                .child(kit::Button::new("Open externally").icon(&crate::icons::EXTERNAL_LINK).variant("outline").size("xs").on_press(open).build(t))
                .child(kit::Button::new("Save file").icon(&crate::icons::DOWNLOAD).variant("outline").size("xs").build(t).on_press_tx(save)),
        )
}

pub fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).find(|p| !p.is_empty()).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_files_as_the_web_does() {
        let cases = [
            ("README.md", Kind::Markdown),
            ("docs/guide.MDX", Kind::Markdown),
            ("flow.mmd", Kind::Mermaid),
            ("site/index.html", Kind::Html),
            ("data.csv", Kind::Table(',')),
            ("data.tsv", Kind::Table('\t')),
            ("analysis.ipynb", Kind::Notebook),
            ("logo.svg", Kind::Image),
            ("clip.mp4", Kind::Video),
            ("song.flac", Kind::Audio),
            ("paper.pdf", Kind::Pdf),
            ("deck.pptx", Kind::Office),
            ("Inter.woff2", Kind::Font),
            ("app.sqlite3", Kind::Archive),
            ("src/main.rs", Kind::Text),
            (".env", Kind::Text),
            ("Makefile", Kind::Text),
        ];
        for (path, want) in cases {
            assert_eq!(kind(path), want, "{path}");
        }
        assert!(Kind::Html.opens_in_browser() && Kind::Pdf.opens_in_browser() && !Kind::Markdown.opens_in_browser());
        assert!(Kind::Pdf.unpreviewable() && Kind::Audio.unpreviewable() && !Kind::Image.unpreviewable() && !Kind::Notebook.unpreviewable());
        assert_eq!(toggle_label("markdown", false), "Show rendered markdown");
        assert_eq!(toggle_label("table", true), "Show source");
        assert_eq!(file_name("a/b/c.txt"), "c.txt");
    }

    #[test]
    fn parses_delimited_text_like_parse_delimited_preview() {
        let t = parse_delimited("\u{feff}name,note\r\nAda,\"says \"\"hi\"\", ok\"\nBob,\"multi\nline\"\n", ',', 100, 30, 2000);
        assert_eq!(t.rows, vec![vec!["name", "note"], vec!["Ada", "says \"hi\", ok"], vec!["Bob", "multi\nline"]]);
        assert!(!t.truncated);
        let tabs = parse_delimited("a\tb\n1\t2", '\t', 100, 30, 2000);
        assert_eq!(tabs.rows, vec![vec!["a", "b"], vec!["1", "2"]]);
        let capped = parse_delimited("1\n2\n3\n", ',', 2, 30, 2000);
        assert_eq!((capped.rows.len(), capped.truncated), (2, true));
        let wide = parse_delimited("a,b,c\n", ',', 100, 2, 2000);
        assert_eq!((wide.rows[0].len(), wide.truncated), (2, true));
        let long = parse_delimited("abcdef\n", ',', 100, 30, 3);
        assert_eq!((long.rows[0][0].as_str(), long.truncated), ("abc", true));
        assert!(parse_delimited("\"open", ',', 100, 30, 2000).truncated, "an unclosed quote");
        assert_eq!(parse_delimited("", ',', 100, 30, 2000).rows, Vec::<Vec<String>>::new());
    }

    #[test]
    fn reads_version_4_notebooks_with_saved_text_outputs() {
        let source = r##"{"nbformat": 4, "metadata": {"language_info": {"name": "python 3!"}}, "cells": [
            {"cell_type": "markdown", "source": ["# Title\n", "Body"]},
            {"cell_type": "code", "source": "print(1)", "outputs": [
                {"output_type": "stream", "text": ["1\n"]},
                {"output_type": "execute_result", "data": {"text/plain": "2"}},
                {"output_type": "error", "traceback": ["\u001b[31mValueError\u001b[0m: bad"]},
                {"output_type": "display_data", "data": {"image/png": "abc"}}
            ]},
            {"cell_type": "raw", "source": "raw"}
        ]}"##;
        let nb = notebook(source).unwrap();
        assert_eq!(nb.language, "python3");
        assert_eq!(nb.cells.len(), 3);
        assert_eq!((nb.cells[0].kind, nb.cells[0].source.as_str()), ("markdown", "# Title\nBody"));
        assert_eq!(nb.cells[1].outputs, vec!["1\n", "2", "ValueError: bad"]);
        assert_eq!(nb.cells[2].kind, "raw");
        assert_eq!(notebook(r#"{"nbformat": 3, "cells": []}"#).unwrap_err(), "Preview supports version 4 notebooks with up to 500 cells.");
        assert!(notebook("{").is_err());
    }
}
