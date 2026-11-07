use crate::{
    App, data, icons, kit,
    theme::{self, T},
};
use rune::{El, Key, KeyEvent, ScrollSpec, TextEdit, TextStyle, Tx, col, el, icon, row, text, text_input};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// The web's `FileSaveCoordinator` debounce.
const DEBOUNCE: Duration = Duration::from_millis(500);
const CODE: TextStyle = theme::mono(TextStyle::new(13., 400.).lh(20.));

/// A write in flight (`id`), or queued behind one to the same file (`None`).
struct Write {
    id: Option<u64>,
    cwd: String,
    path: String,
    contents: String,
}

pub struct Ui {
    entries: Vec<Value>,
    directory: String,
    listing: Option<u64>,
    selected: String,
    cwd: String,
    reading: Option<u64>,
    editor: TextEdit,
    /// What the server has, as last read or confirmed.
    saved: String,
    /// The newest contents handed to a write, so an unchanged draft isn't written twice.
    sent: String,
    truncated: bool,
    numbers: String,
    columns: usize,
    due: Option<Instant>,
    writes: Vec<Write>,
    /// Drafts whose write failed, by (cwd, path): reopening the file resumes them.
    unsaved: HashMap<(String, String), String>,
    save_error: Option<String>,
    error: Option<String>,
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            entries: vec![],
            directory: String::new(),
            listing: None,
            selected: String::new(),
            cwd: String::new(),
            reading: None,
            editor: TextEdit::new(""),
            saved: String::new(),
            sent: String::new(),
            truncated: false,
            numbers: String::new(),
            columns: 0,
            due: None,
            writes: vec![],
            unsaved: HashMap::new(),
            save_error: None,
            error: None,
        }
    }
}

pub fn load(app: &mut App, directory: String) {
    let Some((_, cwd)) = super::context(app) else {
        return;
    };
    let request = app.src.net().map(|n| {
        n.request(
            "projects.listEntries",
            json!({"cwd":cwd,"directoryPath":directory}),
        )
    });
    app.ui.panel.files.directory = directory;
    app.ui.panel.files.listing = request;
    app.ui.panel.files.error = None;
}

fn read(app: &mut App, path: String) {
    flush(app);
    let Some((_, cwd)) = super::context(app) else {
        return;
    };
    let request = app
        .src
        .net()
        .map(|n| n.request("projects.readFile", json!({"cwd":cwd,"relativePath":path})));
    let ui = &mut app.ui.panel.files;
    ui.selected = path;
    ui.cwd = cwd;
    ui.reading = request;
    ui.error = None;
    ui.save_error = None;
    show(ui, String::new(), false);
}

/// Opens `path` (relative to the thread's workspace) in the Files surface, listing its folder.
pub fn open(app: &mut App, path: String) {
    let folder = path
        .rsplit_once('/')
        .map(|(p, _)| p.to_string())
        .unwrap_or_default();
    if app.ui.panel.files.directory != folder || app.ui.panel.files.entries.is_empty() {
        load(app, folder);
    }
    read(app, path);
}

fn close_file(app: &mut App) {
    flush(app);
    let ui = &mut app.ui.panel.files;
    ui.selected.clear();
    ui.reading = None;
    ui.save_error = None;
    show(ui, String::new(), false);
}

/// Puts server contents in the editor, resuming a draft that failed to save.
fn show(ui: &mut Ui, contents: String, truncated: bool) {
    let draft = ui.unsaved.remove(&(ui.cwd.clone(), ui.selected.clone()));
    ui.truncated = truncated;
    ui.sent = contents.clone();
    ui.editor = TextEdit::new(draft.unwrap_or_else(|| contents.clone()));
    ui.saved = contents;
    ui.due = None;
    measure(ui);
}

fn measure(ui: &mut Ui) {
    let text = &ui.editor.text;
    let lines = text.split('\n').count();
    let digits = lines.to_string().len();
    ui.numbers = (1..=lines)
        .map(|n| format!("{n:>digits$}"))
        .collect::<Vec<_>>()
        .join("\n");
    ui.columns = text
        .split('\n')
        .map(|line| line.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum())
        .max()
        .unwrap_or(0);
}

fn changed(app: &mut App, tx: &mut Tx) {
    let ui = &mut app.ui.panel.files;
    measure(ui);
    ui.due = Some(Instant::now() + DEBOUNCE);
    tx.wake_after(DEBOUNCE);
}

/// Writes the open file's draft now instead of after the debounce.
pub fn flush(app: &mut App) {
    let ui = &mut app.ui.panel.files;
    ui.due = None;
    if ui.selected.is_empty() || ui.reading.is_some() || ui.truncated || ui.editor.text == ui.sent {
        return;
    }
    ui.sent = ui.editor.text.clone();
    let write = Write {
        id: None,
        cwd: ui.cwd.clone(),
        path: ui.selected.clone(),
        contents: ui.sent.clone(),
    };
    queue(app, write);
}

/// One ordered write queue per file, like the web's save sessions.
fn queue(app: &mut App, mut write: Write) {
    let ui = &mut app.ui.panel.files;
    let same = |w: &Write| w.cwd == write.cwd && w.path == write.path;
    ui.writes.retain(|w| !(w.id.is_none() && same(w)));
    if !ui.writes.iter().any(same) {
        let payload = json!({"cwd":write.cwd,"relativePath":write.path,"contents":write.contents});
        let Some(id) = app.src.net().map(|n| n.request("projects.writeFile", payload)) else {
            return;
        };
        write.id = Some(id);
    }
    app.ui.panel.files.writes.push(write);
}

/// Leaves the current thread's files, still finishing their writes.
pub fn reset(app: &mut App) {
    flush(app);
    let old = std::mem::take(&mut app.ui.panel.files);
    app.ui.panel.files.writes = old.writes;
    app.ui.panel.files.unsaved = old.unsaved;
}

/// An edit not yet confirmed by the server: the Files tab shows a dot.
pub fn pending(app: &App) -> bool {
    let ui = &app.ui.panel.files;
    !ui.writes.is_empty() || !ui.unsaved.is_empty() || ui.editor.text != ui.saved
}

pub fn poll(app: &mut App, tx: &mut Tx) -> bool {
    let mut changed = false;
    let ui = &mut app.ui.panel.files;
    if let Some(id) = ui.listing
        && let Some(reply) = app.src.net().and_then(|n| n.take_reply(id))
    {
        ui.listing = None;
        match reply {
            Ok(v) => ui.entries = v["entries"].as_array().cloned().unwrap_or_default(),
            Err(e) => ui.error = Some(e.to_string()),
        }
        changed = true;
    }
    if let Some(id) = ui.reading
        && let Some(reply) = app.src.net().and_then(|n| n.take_reply(id))
    {
        ui.reading = None;
        match reply {
            Ok(v) => {
                show(ui, data::s(&v, "contents").into(), v["truncated"] == true);
                if ui.editor.text != ui.saved {
                    ui.due = Some(Instant::now() + DEBOUNCE);
                    tx.wake_after(DEBOUNCE);
                }
            }
            Err(e) => ui.error = Some(e.to_string()),
        }
        changed = true;
    }
    let mut done = vec![];
    for (i, w) in ui.writes.iter().enumerate() {
        if let Some(id) = w.id
            && let Some(reply) = app.src.net().and_then(|n| n.take_reply(id))
        {
            done.push((i, reply));
        }
    }
    for (i, reply) in done.into_iter().rev() {
        let w = ui.writes.remove(i);
        let open = w.cwd == ui.cwd && w.path == ui.selected;
        let key = (w.cwd.clone(), w.path.clone());
        match reply {
            Ok(_) => {
                ui.unsaved.remove(&key);
                if open {
                    ui.saved = w.contents;
                    ui.save_error = None;
                }
            }
            Err(e) => {
                if open {
                    ui.sent = ui.saved.clone();
                    ui.save_error = Some(format!("Couldn't save {}: {e}", w.path));
                } else {
                    ui.unsaved.insert(key, w.contents);
                }
            }
        }
        changed = true;
    }
    let next: Vec<usize> = (0..ui.writes.len())
        .filter(|&i| {
            let w = &ui.writes[i];
            w.id.is_none()
                && !ui.writes.iter().any(|o| o.id.is_some() && o.cwd == w.cwd && o.path == w.path)
        })
        .collect();
    for i in next.into_iter().rev() {
        let w = app.ui.panel.files.writes.remove(i);
        queue(app, w);
    }
    if app.ui.panel.files.due.is_some_and(|due| Instant::now() >= due) {
        flush(app);
        changed = true;
    }
    changed
}

fn on_key(s: &mut App, e: &KeyEvent, _: &mut Tx) -> bool {
    if matches!(e.key, Key::Escape) {
        s.ui.panel.files.editor.focused = false;
        return true;
    }
    false
}

fn editor(ui: &Ui, t: &T) -> El {
    let lang = ui.selected.rsplit('.').next();
    let spans = rune_kit::syntax::SyntaxTheme::neutral(t.dark)
        .spans(&rune_kit::syntax::highlight(lang, &ui.editor.text), 0);
    let cell = rune::text::advance("MMMMMMMMMM", CODE) / 10.;
    let width = (ui.columns + 2) as f32 * cell;
    let code = if ui.truncated {
        text(&ui.editor.text)
            .font(CODE)
            .color(t.c.contrast_foreground)
            .spans(spans)
    } else {
        text_input(&ui.editor, CODE)
            .key("file-editor")
            .label(&ui.selected)
            .color(t.c.contrast_foreground)
            .caret(t.c.contrast_foreground, t.c.primary.alpha_mul(0.3))
            .spans(spans)
            .on_edit(|s: &mut App| &mut s.ui.panel.files.editor)
            .on_input_change(changed)
            .on_submit(|s: &mut App, tx| {
                s.ui.panel.files.editor.insert("\n");
                changed(s, tx);
            })
            .on_input_key(on_key)
    };
    el().grow(1.).min_h(0.).min_w(0.).scroll_xy().child(
        row()
            .py(8.)
            .child(
                text(&ui.numbers)
                    .font(CODE)
                    .color(t.c.muted_foreground)
                    .pl(12.)
                    .pr(16.),
            )
            .child(code.min_w(width).grow(1.).pr(12.)),
    )
}

pub fn view(app: &App, t: &T) -> El {
    let ui = &app.ui.panel.files;
    let header = row()
        .h(40.)
        .px(8.)
        .gap(6.)
        .center_y()
        .child(
            kit::Button::new("Parent folder")
                .icon(&icons::ARROW_UP)
                .variant("ghost-muted")
                .size("icon-xs")
                .on_press(|s: &mut App| {
                    let path =
                        s.ui.panel
                            .files
                            .directory
                            .rsplit_once('/')
                            .map(|(p, _)| p.to_string())
                            .unwrap_or_default();
                    close_file(s);
                    load(s, path);
                })
                .build(t),
        )
        .child(
            text(if ui.selected.is_empty() {
                if ui.directory.is_empty() {
                    "Files"
                } else {
                    &ui.directory
                }
            } else {
                &ui.selected
            })
            .font(theme::TEXT_XS)
            .color(t.c.contrast_foreground)
            .grow(1.),
        )
        .child(
            kit::Button::new("Refresh files")
                .icon(&icons::REFRESH_CW)
                .variant("ghost-muted")
                .size("icon-xs")
                .on_press(|s: &mut App| load(s, s.ui.panel.files.directory.clone()))
                .build(t),
        );
    let content = if let Some(error) = &ui.error {
        scroll(super::notice(t, error))
    } else if !ui.selected.is_empty() {
        editor(ui, t)
    } else {
        let mut list = col().w_full().padding(8.).gap(2.);
        for entry in &ui.entries {
            let path = data::s(entry, "path").to_string();
            let directory = entry["kind"] == "directory";
            list = list.child(
                row()
                    .w_full()
                    .h(28.)
                    .px(8.)
                    .gap(8.)
                    .center_y()
                    .radius(4.)
                    .hover(t.c.accent)
                    .focusable(true)
                    .role("button")
                    .label(&path)
                    .child(
                        icon(
                            if directory {
                                &icons::FOLDER
                            } else {
                                &icons::FILE
                            },
                            14.,
                        )
                        .size(14., 14.)
                        .color(t.c.muted_foreground),
                    )
                    .child(
                        text(path.rsplit('/').next().unwrap_or(&path))
                            .font(theme::TEXT_XS)
                            .color(t.c.contrast_foreground),
                    )
                    .on_press(move |s: &mut App| {
                        if directory {
                            close_file(s);
                            load(s, path.clone())
                        } else {
                            read(s, path.clone())
                        }
                    }),
            );
        }
        scroll(list)
    };
    let mut surface = col()
        .grow(1.)
        .min_h(0.)
        .child(header)
        .child(el().h(1.).w_full().fill(t.c.border));
    if let Some(error) = &ui.save_error {
        surface = surface.child(
            row()
                .w_full()
                .px(12.)
                .py(6.)
                .fill(t.c.warning_surface)
                .child(
                    text(error)
                        .font(theme::TEXT_2XS)
                        .color(t.c.warning_foreground)
                        .wrap()
                        .grow(1.),
                ),
        );
        surface = surface.child(el().h(1.).w_full().fill(t.c.warning.alpha_mul(0.2)));
    }
    surface.child(content)
}

fn scroll(content: El) -> El {
    col()
        .grow(1.)
        .min_h(0.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(content)
}

pub fn fixture(app: &mut App) {
    let v: Value = serde_json::from_str(include_str!("../../fixtures/panel/files.json")).unwrap();
    app.ui.panel.files.entries = v["entries"].as_array().unwrap().clone();
}

/// A recorded file open in the editor; `edit` types into it, `error` fails its save.
pub fn edit_fixture(app: &mut App, edit: Option<&str>, error: bool) {
    fixture(app);
    let v: Value =
        serde_json::from_str(include_str!("../../fixtures/panel/file-edit.json")).unwrap();
    let ui = &mut app.ui.panel.files;
    ui.directory = "src".into();
    ui.selected = data::s(&v, "relativePath").into();
    ui.cwd = "/repo".into();
    show(ui, data::s(&v, "contents").into(), v["truncated"] == true);
    if let Some(edit) = edit {
        let at = ui.editor.text.find("  return").unwrap_or(0);
        ui.editor.text.insert_str(at, edit);
        ui.editor.focused = true;
        measure(ui);
    }
    if error {
        ui.save_error = Some(format!(
            "Couldn't save {}: Permission denied (os error 13)",
            ui.selected
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_numbers_and_width_follow_the_text() {
        let mut ui = Ui::default();
        show(&mut ui, "a\n\tbc\n".into(), false);
        assert_eq!(ui.numbers, "1\n2\n3");
        assert_eq!(ui.columns, 6);
        let mut ui = Ui::default();
        show(&mut ui, (0..10).map(|_| "x\n").collect(), false);
        assert!(ui.numbers.starts_with(" 1\n 2\n"), "{:?}", ui.numbers);
    }

    #[test]
    fn a_failed_draft_resumes_when_the_file_reopens() {
        let mut ui = Ui::default();
        ui.cwd = "/repo".into();
        ui.selected = "a.ts".into();
        ui.unsaved
            .insert(("/repo".into(), "a.ts".into()), "draft".into());
        show(&mut ui, "server".into(), false);
        assert_eq!(ui.editor.text, "draft");
        assert_eq!(ui.saved, "server");
        assert_eq!(ui.sent, "server");
        assert!(ui.unsaved.is_empty());
    }
}
