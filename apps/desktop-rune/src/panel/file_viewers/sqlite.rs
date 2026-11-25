use std::sync::Arc;

use rune::{El, col, el, row, text};
use serde_json::Value;

use super::{Content, Job, TempAsset, action, bar, byte_size, divider, grouped, loading, note, run};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons, kit};

const PAGE: usize = 50;

pub(super) struct Database {
    file: Arc<TempAsset>,
    size: u64,
    tables: Vec<Table>,
    table: usize,
    page: usize,
    select_open: bool,
    rows: Option<Result<Rows, String>>,
    job: Option<Job<Rows>>,
}

struct Table {
    name: String,
    count: usize,
}

struct Rows {
    columns: Vec<String>,
    values: Vec<Vec<String>>,
}

fn identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn query(file: &TempAsset, sql: &str) -> Result<Vec<Value>, String> {
    let sql = format!("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; {sql}");
    let out = run("sqlite3", &["-batch", "-safe", "-readonly", "-json", "-init", "/dev/null"],
        file.path(), &[&sql], 8 * 1024 * 1024)?;
    if out.is_empty() { return Ok(Vec::new()) }
    serde_json::from_slice(&out).map_err(|_| "Could not read the database preview.".to_owned())
}

fn read_rows(file: &TempAsset, name: &str, page: usize) -> Result<Rows, String> {
    let table = identifier(name);
    let columns: Vec<_> = query(file, &format!("PRAGMA table_xinfo({table});"))?.iter()
        .filter(|row| row["hidden"] != 1).filter_map(|row| row["name"].as_str().map(str::to_owned)).take(50).collect();
    if columns.is_empty() { return Ok(Rows { columns, values: Vec::new() }) }
    let fields = columns.iter().enumerate().map(|(index, name)| {
        let field = identifier(name);
        format!("CASE WHEN typeof({field}) = 'blob' THEN '[blob: ' || length({field}) || ' bytes]' ELSE substr(CAST({field} AS TEXT), 1, 1024) END AS c{index}")
    }).collect::<Vec<_>>().join(", ");
    let data = query(file, &format!("SELECT {fields} FROM {table} LIMIT {PAGE} OFFSET {};", page.saturating_mul(PAGE)))?;
    let values = data.iter().map(|row| (0..columns.len()).map(|i| row[format!("c{i}")].as_str().unwrap_or("NULL").to_owned()).collect()).collect();
    Ok(Rows { columns, values })
}

impl Database {
    pub(super) fn open(bytes: &[u8]) -> Result<Self, String> {
        let file = Arc::new(TempAsset::write("preview.sqlite", bytes)?);
        let raw = query(&file, "SELECT name FROM sqlite_schema WHERE type='table' AND substr(name,1,7) != 'sqlite_' AND sql NOT LIKE 'CREATE VIRTUAL TABLE%' ORDER BY name LIMIT 101;")?;
        if raw.len() > 100 { return Err("Database previews are limited to 100 tables.".into()) }
        let names: Vec<_> = raw.iter().filter_map(|v| v["name"].as_str()).collect();
        let counts = if names.is_empty() { Vec::new() } else {
            let queries = names.iter().map(|name| format!("SELECT count(*) AS count FROM {}", identifier(name))).collect::<Vec<_>>().join(" UNION ALL ");
            query(&file, &queries)?
        };
        let tables: Vec<_> = names.into_iter().zip(counts).map(|(name, row)| Table {
            name: name.to_owned(), count: row["count"].as_u64().unwrap_or(0) as usize,
        }).collect();
        let rows = tables.first().map(|t| read_rows(&file, &t.name, 0));
        Ok(Self { file, size: bytes.len() as u64, tables, table: 0, page: 0, select_open: false, rows, job: None })
    }

    pub(in crate::panel::file_viewers) fn fixture() -> Result<Self, String> {
        Ok(Self {
            file: Arc::new(TempAsset::write("preview.sqlite", &[])?),
            size: 24_576,
            tables: vec![Table { name: "work".into(), count: 52 }],
            table: 0, page: 0, select_open: false, job: None,
            rows: Some(Ok(Rows { columns: vec!["id".into(), "machine".into(), "status".into()],
                values: (1..=50).map(|i| vec![i.to_string(), format!("Worker {i}"), "ready".into()]).collect() })),
        })
    }

    pub(super) fn poll(&mut self) -> bool {
        if let Some(result) = self.job.as_ref().and_then(Job::take) {
            self.job = None;
            self.rows = Some(result);
            return true;
        }
        false
    }

    fn select(&self) -> kit::Select {
        let names: Vec<_> = self.tables.iter().map(|table| format!("{} ({} rows)", table.name, grouped(table.count as u64))).collect();
        kit::Select::new("file-sqlite-table", &names).current(names.get(self.table).map(String::as_str))
            .label("Table").size("compact").width(144.0)
            .on_toggle(|app: &mut App| set_select(app, true))
            .on_close(|app: &mut App| set_select(app, false))
            .on_choose(|app: &mut App, name, _| {
                let Some(database) = db(app) else { return };
                let index = database.tables.iter().position(|table| name == format!("{} ({} rows)", table.name, grouped(table.count as u64)));
                if let Some(index) = index { change_page(app, index, 0); }
            })
    }

    pub(super) fn overlays(&self, t: &T, layers: &mut Vec<El>) {
        if self.select_open { self.select().open(true).build(&kit::theme(t), layers); }
    }

    pub(super) fn view(&self, t: &T) -> El {
        let Some(table) = self.tables.get(self.table) else {
            return note("This database has no readable tables.", t).padding(24.0);
        };
        let (page, index, count) = (self.page, self.table, table.count);
        let loading_rows = self.job.is_some();
        let shown = if count == 0 { "0 rows".into() } else {
            format!("{}–{} of {}", page * PAGE + 1, ((page + 1) * PAGE).min(count), grouped(count as u64))
        };
        let cols = self.rows.as_ref().and_then(|r| r.as_ref().ok()).map_or("…".into(), |rows| rows.columns.len().to_string());
        let header = bar(t).flex_wrap()
            .child(self.select().trigger(&kit::theme(t)))
            .child(action("Previous rows", &icons::CHEVRON_LEFT, loading_rows || page == 0, t).on_press(move |app: &mut App| change_page(app, index, page.saturating_sub(1))))
            .child(note(shown, t))
            .child(action("Next rows", &icons::CHEVRON_RIGHT, loading_rows || (page + 1) * PAGE >= count, t).on_press(move |app: &mut App| change_page(app, index, page + 1)));
        let summary = bar(t).child(note(format!("{} rows · {cols} columns · {}", grouped(count as u64), byte_size(self.size)), t));
        let notice = note("Read-only. Up to 50 columns and 1,024 characters per cell. Blob contents are omitted.", t).wrap().padding(8.0);
        let body = match self.rows.as_ref() {
            Some(Ok(rows)) => self.table(rows, t),
            Some(Err(error)) => super::failure(error, t),
            None => loading(t),
        };
        col().grow(1.0).min_w(0.0).min_h(0.0).child(header).child(divider(t))
            .child(summary).child(divider(t)).child(notice).child(divider(t)).child(body)
    }

    fn table(&self, rows: &Rows, t: &T) -> El {
        let cell = |content: &str, heading| {
            let font = if heading { theme::XS_MEDIUM } else { theme::TEXT_XS };
            row().w(128.0).padding(8.0).border(0.5, t.c.border.alpha_mul(0.6)).child(
                text(content.to_owned()).font(font).color(t.c.foreground).wrap_anywhere().selectable(!heading),
            )
        };
        let mut grid = col().child(row().fill(t.c.background).sticky_top(0.0)
            .children(rows.columns.iter().map(|name| cell(name, true))));
        if rows.values.is_empty() {
            grid = grid.child(note("This table is empty.", t).padding(24.0));
        }
        for (index, values) in rows.values.iter().enumerate() {
            let line = row().fill(if index % 2 == 1 { t.c.muted.alpha_mul(0.3) } else { rune::Color::TRANSPARENT });
            grid = grid.child(line.children(values.iter().map(|v| cell(v, false))));
        }
        el().grow(1.0).min_h(0.0).min_w(0.0).scroll_xy().child(grid)
    }
}

fn db(app: &mut App) -> Option<&mut Database> {
    match super::active_mut(app) {
        Some(Content::Sqlite(database)) => Some(database),
        _ => None,
    }
}

fn set_select(app: &mut App, open: bool) {
    if let Some(database) = db(app) { database.select_open = open; }
}

fn change_page(app: &mut App, table: usize, page: usize) {
    let wake = app.ui.panel.waker.clone();
    let Some(database) = db(app) else { return };
    let Some(found) = database.tables.get(table) else { return };
    let page = page.min(found.count.saturating_sub(1) / PAGE);
    let name = found.name.clone();
    database.table = table;
    database.page = page;
    database.select_open = false;
    database.rows = None;
    let file = database.file.clone();
    database.job = Some(Job::spawn(wake, move || read_rows(&file, &name, page)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_identifiers_cannot_escape_their_quotes() {
        assert_eq!(identifier("topic\"; SELECT 1"), "\"topic\"\"; SELECT 1\"");
    }
}
