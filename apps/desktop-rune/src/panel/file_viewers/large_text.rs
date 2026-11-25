use std::borrow::Cow;

use rune::{El, GrowthSpec, ScrollSpec, Tx, col, el, row, text};

use super::{bar, note};
use crate::panel::file_preview::{parse_delimited, table_number};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons, kit};

pub(super) const BYTE_LIMIT: usize = 256 * 1024 * 1024;
const ROW_LIMIT: usize = 5_000_000;
const RECORD_LIMIT: usize = 1024 * 1024;
const COLUMNS: usize = 1024;
const LINE_HEIGHT: f32 = 20.0;
const SCROLL: &str = "full-file-scroll";

fn row_key(_: &App, index: usize) -> Cow<'_, str> {
    Cow::Owned(format!("full-file-row-{index}"))
}

pub(super) struct LargeText {
    source: String,
    lines: Index,
    table: Option<(u8, Index)>,
}

struct Index {
    offsets: Vec<u32>,
    columns: usize,
}

impl Index {
    fn new(source: &str, delimiter: Option<u8>) -> Result<Self, String> {
        let bytes = source.as_bytes();
        let mut offsets = vec![0];
        let (mut at, mut columns, mut max_columns) = (0, 1, 1);
        let (mut quoted, mut field_start) = (false, true);
        if bytes.starts_with(b"\xef\xbb\xbf") {
            at = 3;
        }
        while at < bytes.len() {
            if at - *offsets.last().unwrap() as usize > RECORD_LIMIT {
                return Err("A line or record exceeds the 1 MB preview limit.".into());
            }
            let b = bytes[at];
            at += 1;
            if delimiter.is_some() && b == b'"' {
                if quoted && bytes.get(at) == Some(&b'"') {
                    at += 1;
                    continue;
                }
                if quoted || field_start {
                    quoted = !quoted;
                    field_start = false;
                    continue;
                }
            }
            if quoted {
                continue;
            }
            field_start = Some(b) == delimiter;
            if field_start {
                columns += 1;
            } else if b == b'\r' || b == b'\n' {
                if b == b'\r' && bytes.get(at) == Some(&b'\n') {
                    at += 1;
                }
                offsets.push(at as u32);
                max_columns = max_columns.max(columns);
                columns = 1;
                field_start = true;
                if offsets.len() > ROW_LIMIT + 1 {
                    return Err("Text previews support up to 5 million rows.".into());
                }
            }
        }
        if offsets.last().copied() != Some(bytes.len() as u32) {
            offsets.push(bytes.len() as u32);
        }
        if offsets.len() > ROW_LIMIT + 1 {
            return Err("Text previews support up to 5 million rows.".into());
        }
        Ok(Self { offsets, columns: max_columns.max(columns).min(COLUMNS) })
    }

    fn count(&self) -> usize {
        self.offsets.len() - 1
    }

    fn record<'a>(&self, source: &'a str, index: usize) -> &'a str {
        let Some(bounds) = self.offsets.get(index..=index + 1) else { return "" };
        source[bounds[0] as usize..bounds[1] as usize].trim_end_matches(['\r', '\n'])
    }
}

impl LargeText {
    pub(super) fn new(bytes: Vec<u8>, delimiter: Option<char>) -> Result<Self, String> {
        if bytes.contains(&0) {
            return Err("This file contains binary data.".into());
        }
        let source = String::from_utf8(bytes).map_err(|_| "This file is not valid UTF-8.")?;
        let lines = Index::new(&source, None)?;
        let table = delimiter.map(|separator| Index::new(&source, Some(separator as u8)).map(|index| (separator as u8, index))).transpose()?;
        Ok(Self { source, lines, table })
    }

    pub(super) fn contents(&self) -> &str {
        self.source.trim_start_matches('\u{feff}')
    }

    fn display_table(&self, rendered: bool) -> Option<(char, &Index)> {
        rendered.then(|| self.table.as_ref().map(|(separator, index)| (*separator as char, index))).flatten()
    }

    pub(super) fn reveal(&self, line: Option<usize>, tx: &mut Tx) {
        if let Some(line) = line {
            let line = line.saturating_sub(1).min(self.lines.count().saturating_sub(1));
            tx.scroll_to_y(SCROLL, (line as f32 - 2.0).max(0.0) * LINE_HEIGHT);
        }
    }

    pub(super) fn view(&self, app: &App, rendered: bool, wrap: bool, t: &T) -> El {
        let table = self.display_table(rendered);
        let index = table.map_or(&self.lines, |(_, index)| index);
        let count = index.count();
        let bytes = super::byte_size(self.source.len() as u64);
        let summary = if table.is_some() {
            format!("{} rows · {} columns · {bytes}", super::grouped(count.saturating_sub(1) as u64), index.columns)
        } else {
            format!("{} lines · {bytes}", super::grouped(count as u64))
        };
        let theme = *t;
        let has_heading = table.is_some() && count > 0;
        let list = col().w_full().growth(GrowthSpec::default()).selection_group(true)
            .virtual_rows(count - usize::from(has_heading), LINE_HEIGHT, row_key, move |app: &App, i| {
                let Some(super::Content::Large(file)) = super::active(app) else { return el().h(LINE_HEIGHT) };
                match file.display_table(rendered) {
                    Some((separator, index)) => file.table_row(separator, index, i + 1, &theme),
                    None => file.source_row(app, wrap, i, &theme),
                }
            }).virtual_rows_version(rendered);
        let content = match table {
            Some((separator, index)) if has_heading => col().min_w(56.0 + index.columns as f32 * 160.0)
                .child(self.table_row(separator, index, 0, t).sticky_top(0.0))
                .child(list),
            _ => list,
        };
        let viewport = el().key(SCROLL).grow(1.0).min_h(0.0).min_w(0.0);
        let viewport = if wrap && table.is_none() { viewport.scroll(ScrollSpec { stick_bottom: false }) } else { viewport.scroll_xy() };
        let full = bar(t).child(note("Full file", t).grow(1.0))
            .child(kit::icon_button(&icons::COPY, "Copy contents").variant("ghost").size("icon-xs").build(t)
                .on_press_tx(super::super::files::copy_contents).help("Copy contents"))
            .child(kit::icon_button(&icons::DOWNLOAD, "Save file").variant("ghost").size("icon-xs").build(t)
                .on_press_tx(super::super::files::save_as).help("Save file"));
        col().grow(1.0).min_h(0.0).min_w(0.0)
            .child(full).child(super::divider(t))
            .child(bar(t).child(note(summary, t)).role("status")).child(super::divider(t))
            .child(viewport.child(content))
    }

    fn table_row(&self, separator: char, index: &Index, i: usize, t: &T) -> El {
        let record = index.record(&self.source, i);
        let parsed = parse_delimited(record, separator, 1, COLUMNS, 8192);
        let cells = parsed.rows.into_iter().next().unwrap_or_default();
        let font = if i == 0 { theme::XS_MEDIUM } else { theme::TEXT_XS };
        row().w_full().min_h(29.0).fill(if i == 0 { t.c.background } else { rune::Color::TRANSPARENT })
            .child(table_number((i != 0).then_some(i), t))
            .children(cells.into_iter().map(|cell| row().w(160.0).px(12.0).py(6.0)
                .child(text(cell.clone()).font(font).color(t.c.foreground).truncate().help_when_truncated(cell).selectable(i != 0))))
    }

    fn source_row(&self, app: &App, wrap: bool, i: usize, t: &T) -> El {
        let actual = self.lines.record(&self.source, i).trim_start_matches('\u{feff}');
        let end = actual.floor_char_boundary(actual.len().min(16_384));
        let source = if end < actual.len() { format!("{}…", &actual[..end]) } else { actual.to_owned() };
        let focused = app.ui.panel.files.line.is_some_and(|line| line.clamp(1, self.lines.count().max(1)) == i + 1);
        let font = theme::mono(rune::TextStyle::new(t.code, 400.0).lh(LINE_HEIGHT));
        let content = text(source).font(font).color(t.c.foreground).selectable(true);
        let content = if wrap { content.grow(1.0).min_w(0.0).wrap_anywhere() } else { content.single_line() };
        row().w_full().min_h(LINE_HEIGHT)
            .fill(if focused { t.c.accent } else { rune::Color::TRANSPARENT })
            .child(row().w(62.0).pl(12.0).fill(t.c.background).sticky_left(0.0)
                .child(text((i + 1).to_string()).font(font).color(t.c.muted_foreground)))
            .child(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_and_table_indexes_keep_distinct_offsets_for_quoted_newlines() {
        let file = LargeText::new(b"kind,text\r\nold,\"a\r\nb\"\rnew,c\n".to_vec(), Some(',')).unwrap();
        assert_eq!(file.lines.count(), 4);
        assert_eq!(file.lines.record(&file.source, 2), "b\"");
        let (_, index) = file.display_table(true).unwrap();
        assert_eq!((index.count(), index.columns), (3, 2));
        assert_eq!(index.record(&file.source, 1), "old,\"a\r\nb\"");
        assert_eq!(index.record(&file.source, 2), "new,c");
        assert!(LargeText::new(b"invalid\0text".to_vec(), None).is_err());
    }
}
