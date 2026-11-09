use crate::{
    App, data, icons, kit,
    theme::{self, AlphaMul, T},
};
use rune::{El, Key, KeyEvent, ScrollSpec, TextEdit, TextStyle, Tx, col, el, icon, row, text, text_input};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

/// The web's `FileSaveCoordinator` debounce.
const DEBOUNCE: Duration = Duration::from_millis(500);
/// The web's `PROJECT_PATH_SEARCH_DEBOUNCE_MS`-style pause before a search goes out.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);
const SEARCH_LIMIT: usize = 200;
const CODE: TextStyle = theme::mono(TextStyle::new(13., 400.).lh(20.));

/// A write in flight (`id`), or queued behind one to the same file (`None`).
struct Write {
    id: Option<u64>,
    cwd: String,
    path: String,
    contents: String,
}

pub struct Ui {
    /// Each loaded folder's entries (`projects.listEntries`), by folder path ("" is the root).
    dirs: HashMap<String, Vec<Value>>,
    /// Folder listings in flight.
    listings: Vec<(u64, String)>,
    expanded: HashSet<String>,
    /// Expand all: folders that load while it is on open too.
    expand_all: bool,
    query: TextEdit,
    search_due: Option<Instant>,
    searching: Option<(u64, String)>,
    /// `projects.searchEntries` for the query: matches and whether more exist.
    found: Option<(String, Vec<Value>, bool)>,
    /// A row's right-click menu: the path and where it opened.
    menu: Option<(String, (f32, f32))>,
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
    /// The search match to mark and centre in the open file.
    line: Option<usize>,
    image: Option<Image>,
}

/// An image file shown as a picture, from a served asset URL rather than `projects.readFile`.
struct Image {
    request: Option<u64>,
    url: Option<String>,
    pixels: rune::Pixels,
    failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// `WORKSPACE_IMAGE_PREVIEW_EXTENSIONS` (packages/shared/src/mediaPreview.ts).
const IMAGE_EXTENSIONS: [&str; 13] = [".avif", ".bmp", ".gif", ".ico", ".heic", ".heif", ".jpeg", ".jpg", ".png", ".svg", ".tif", ".tiff", ".webp"];

fn is_image(path: &str) -> bool {
    let path = path.to_lowercase();
    IMAGE_EXTENSIONS.iter().any(|e| path.ends_with(e))
}

/// `filePreviewReadErrorMessage`, from the `ProjectReadFileError` in a failed reply's cause.
fn read_error(cause: &Value) -> String {
    let error = cause.as_array().and_then(|c| c.iter().find(|r| r["_tag"] == "Fail")).map_or(cause, |r| &r["error"]);
    match error["failure"].as_str() {
        Some("path_not_file") => "The path is a directory or special file, not a regular file.".into(),
        Some("binary_file") => "The file is binary and cannot be displayed as text.".into(),
        Some("workspace_path_outside_root") => "The requested path is outside the workspace.".into(),
        Some("resolved_path_outside_root") => "The path resolves to a location outside the workspace.".into(),
        Some("operation_failed") if error["operation"] == "realpath-workspace-root" => "The workspace folder could not be accessed.".into(),
        Some("operation_failed") => "The file could not be accessed or read. It may be missing or inaccessible.".into(),
        _ => crate::shell::palette::error_message(cause, "The file could not be read."),
    }
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            dirs: HashMap::new(),
            listings: vec![],
            expanded: HashSet::new(),
            expand_all: false,
            query: TextEdit::new(""),
            search_due: None,
            searching: None,
            found: None,
            menu: None,
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
            line: None,
            image: None,
        }
    }
}

/// Lists one folder's entries into the tree.
pub fn load(app: &mut App, directory: String) {
    let Some(cwd) = super::workspace(app) else {
        return;
    };
    let ui = &mut app.ui.panel.files;
    if ui.listings.iter().any(|(_, d)| *d == directory) {
        return;
    }
    if let Some(id) = app.src.net().map(|n| n.request("projects.listEntries", json!({"cwd":cwd,"directoryPath":directory}))) {
        app.ui.panel.files.listings.push((id, directory));
    }
    app.ui.panel.files.error = None;
}

/// Opens or closes a folder, listing it the first time.
fn toggle_dir(app: &mut App, path: String) {
    let ui = &mut app.ui.panel.files;
    if ui.expanded.remove(&path) {
        ui.expand_all = false;
        return;
    }
    ui.expanded.insert(path.clone());
    if !ui.dirs.contains_key(&path) {
        load(app, path);
    }
}

/// `toggleAllDirectories`: collapses when every known folder is open, else opens them all.
fn toggle_all(app: &mut App) {
    let ui = &mut app.ui.panel.files;
    let known = known_dirs(&ui.dirs);
    if ui.expand_all || known.iter().all(|d| ui.expanded.contains(d)) {
        ui.expand_all = false;
        ui.expanded.clear();
        return;
    }
    ui.expand_all = true;
    let missing: Vec<String> = known.iter().filter(|d| !ui.dirs.contains_key(*d)).cloned().collect();
    ui.expanded.extend(known);
    for dir in missing {
        load(app, dir);
    }
}

fn known_dirs(dirs: &HashMap<String, Vec<Value>>) -> Vec<String> {
    dirs.values().flatten().filter(|e| e["kind"] == "directory").map(|e| data::s(e, "path").to_string()).collect()
}

/// One visible tree row.
#[derive(Debug, PartialEq)]
struct Row {
    path: String,
    depth: usize,
    dir: bool,
    open: bool,
}

/// The tree's visible rows: folders first, then names, descending into open, loaded folders.
fn rows(dirs: &HashMap<String, Vec<Value>>, expanded: &dyn Fn(&str) -> bool) -> Vec<Row> {
    fn walk(dirs: &HashMap<String, Vec<Value>>, expanded: &dyn Fn(&str) -> bool, dir: &str, depth: usize, out: &mut Vec<Row>) {
        let Some(entries) = dirs.get(dir) else { return };
        let mut entries: Vec<(&str, bool)> = entries.iter().map(|e| (data::s(e, "path"), e["kind"] == "directory")).collect();
        entries.sort_by_key(|(path, dir)| (!dir, name(path).to_lowercase()));
        for (path, is_dir) in entries {
            let open = is_dir && expanded(path);
            out.push(Row { path: path.to_string(), depth, dir: is_dir, open });
            if open {
                walk(dirs, expanded, path, depth + 1, out);
            }
        }
    }
    let mut out = vec![];
    walk(dirs, expanded, "", 0, &mut out);
    out
}

/// Search matches as a tree (`buildTreeEntries`): each match under its folders, all open.
fn search_tree(found: &[Value]) -> HashMap<String, Vec<Value>> {
    let mut dirs: HashMap<String, Vec<Value>> = HashMap::new();
    let mut seen = HashSet::new();
    let mut add = |path: &str, kind: &str, dirs: &mut HashMap<String, Vec<Value>>| {
        if seen.insert(path.to_string()) {
            let parent = path.rsplit_once('/').map_or("", |(p, _)| p).to_string();
            dirs.entry(parent).or_default().push(json!({"path": path, "kind": kind}));
        }
    };
    for entry in found {
        let path = data::s(entry, "path");
        let mut at = 0;
        while let Some(i) = path[at..].find('/') {
            add(&path[..at + i], "directory", &mut dirs);
            at += i + 1;
        }
        add(path, if entry["kind"] == "directory" { "directory" } else { "file" }, &mut dirs);
    }
    dirs
}

fn name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `serializeComposerFileLink`: `[name](path)` with the label escaped and the path encoded.
fn file_link(path: &str) -> String {
    let label = name(path).replace('\\', "\\\\").replace('[', "\\[").replace(']', "\\]");
    let mut dest = String::new();
    for b in path.bytes() {
        match b {
            b'(' | b')' | b'#' | b'?' | b'\\' => dest.push_str(&format!("%{b:02X}")),
            b if b.is_ascii_alphanumeric() || b"-_.!~*';:@&=+$,/".contains(&b) => dest.push(b as char),
            b => dest.push_str(&format!("%{b:02X}")),
        }
    }
    format!("[{label}]({dest})")
}

fn search_changed(app: &mut App, tx: &mut Tx) {
    let ui = &mut app.ui.panel.files;
    if ui.query.text.trim().is_empty() {
        ui.search_due = None;
        ui.found = None;
    } else {
        ui.search_due = Some(Instant::now() + SEARCH_DEBOUNCE);
        tx.wake_after(SEARCH_DEBOUNCE);
    }
}

fn search(app: &mut App) {
    let Some(cwd) = super::workspace(app) else {
        return;
    };
    let query: String = app.ui.panel.files.query.text.trim().chars().take(256).collect();
    let request = app.src.net().map(|n| n.request("projects.searchEntries", json!({"cwd":cwd,"query":query,"limit":SEARCH_LIMIT})));
    let ui = &mut app.ui.panel.files;
    ui.search_due = None;
    ui.searching = request.map(|id| (id, query));
}

fn read(app: &mut App, path: String) {
    flush(app);
    let Some(cwd) = super::workspace(app) else {
        return;
    };
    let image = is_image(&path).then(|| {
        let absolute = format!("{}/{}", cwd.trim_end_matches('/'), path);
        let resource = match super::context(app) {
            Some((thread, _)) => json!({"_tag":"workspace-file","threadId":thread,"path":absolute}),
            None => json!({"_tag":"draft-workspace-file","cwd":cwd,"path":absolute}),
        };
        Image {
            request: app.src.net().map(|n| n.request("assets.createUrl", json!({"resource":resource}))),
            url: None,
            pixels: rune::Pixels::new(),
            failed: Default::default(),
        }
    });
    let request = if image.is_some() {
        None
    } else {
        app.src.net().map(|n| n.request("projects.readFile", json!({"cwd":cwd,"relativePath":path})))
    };
    let ui = &mut app.ui.panel.files;
    ui.image = image;
    ui.selected = path;
    ui.line = None;
    ui.cwd = cwd;
    ui.reading = request;
    ui.error = None;
    ui.save_error = None;
    show(ui, String::new(), false);
}

/// Opens `path` (relative to the workspace) in the Files surface, opening its folders in the tree
/// (`loadFileAncestors`), and marks 1-based `line` once it loads.
pub fn open(app: &mut App, path: String, line: Option<usize>) {
    let mut at = 0;
    let mut folders = vec![String::new()];
    while let Some(i) = path[at..].find('/') {
        folders.push(path[..at + i].to_string());
        at += i + 1;
    }
    for folder in folders {
        if !folder.is_empty() {
            app.ui.panel.files.expanded.insert(folder.clone());
        }
        if !app.ui.panel.files.dirs.contains_key(&folder) {
            load(app, folder);
        }
    }
    read(app, path);
    app.ui.panel.files.line = line;
}

/// The bytes of 1-based `line` in `text`, without its newline.
fn line_range(text: &str, line: usize) -> Option<std::ops::Range<usize>> {
    let mut start = 0;
    for (i, l) in text.split('\n').enumerate() {
        if i + 1 == line {
            return Some(start..start + l.len());
        }
        start += l.len() + 1;
    }
    None
}

fn close_file(app: &mut App) {
    flush(app);
    let ui = &mut app.ui.panel.files;
    ui.selected.clear();
    ui.line = None;
    ui.image = None;
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
    for (id, dir) in app.ui.panel.files.listings.clone() {
        let Some(reply) = app.src.net().and_then(|n| n.take_reply(id)) else { continue };
        let ui = &mut app.ui.panel.files;
        ui.listings.retain(|(v, _)| *v != id);
        match reply {
            Ok(v) => {
                let entries = v["entries"].as_array().cloned().unwrap_or_default();
                let more: Vec<String> = if ui.expand_all {
                    entries.iter().filter(|e| e["kind"] == "directory").map(|e| data::s(e, "path").to_string()).collect()
                } else {
                    vec![]
                };
                ui.dirs.insert(dir, entries);
                ui.expanded.extend(more.iter().cloned());
                let unloaded: Vec<String> = more.into_iter().filter(|d| !ui.dirs.contains_key(d)).collect();
                for dir in unloaded {
                    load(app, dir);
                }
            }
            Err(e) => ui.error = Some(e.to_string()),
        }
        changed = true;
    }
    if let Some((id, query)) = app.ui.panel.files.searching.clone()
        && let Some(reply) = app.src.net().and_then(|n| n.take_reply(id))
    {
        let ui = &mut app.ui.panel.files;
        ui.searching = None;
        match reply {
            Ok(v) => ui.found = Some((query, v["entries"].as_array().cloned().unwrap_or_default(), v["truncated"] == true)),
            Err(e) => ui.error = Some(e.to_string()),
        }
        changed = true;
    }
    if app.ui.panel.files.searching.is_none() && app.ui.panel.files.search_due.is_some_and(|due| Instant::now() >= due) {
        search(app);
    }
    let ui = &mut app.ui.panel.files;
    if let Some(id) = ui.reading
        && let Some(reply) = app.src.net().and_then(|n| n.take_reply(id))
    {
        ui.reading = None;
        match reply {
            Ok(v) => {
                show(ui, data::s(&v, "contents").into(), v["truncated"] == true);
                if ui.line.is_some() {
                    tx.scroll_center("file-line");
                }
                if ui.editor.text != ui.saved {
                    ui.due = Some(Instant::now() + DEBOUNCE);
                    tx.wake_after(DEBOUNCE);
                }
            }
            Err(e) => ui.error = Some(read_error(&e)),
        }
        changed = true;
    }
    if let Some(image) = ui.image.as_mut()
        && let Some(id) = image.request
        && let Some(reply) = app.src.net().and_then(|n| n.take_reply(id))
    {
        image.request = None;
        match reply {
            Ok(v) => {
                let url = format!("{}{}", app.src.net().map(|n| n.base_url().trim_end_matches('/').to_owned()).unwrap_or_default(), data::s(&v, "relativeUrl"));
                image.url = Some(url.clone());
                let (pixels, failed, wake) = (image.pixels.clone(), image.failed.clone(), app.ui.panel.waker.clone());
                std::thread::spawn(move || {
                    if decode(&url, &pixels).is_none() {
                        failed.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    if let Some(wake) = wake {
                        wake();
                    }
                });
            }
            Err(_) => image.failed.store(true, std::sync::atomic::Ordering::Relaxed),
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

/// Relists the loaded folders, searches again and re-reads the open file
/// (`onRefreshSelectedFile`), keeping a draft.
fn refresh(s: &mut App) {
    let dirs: Vec<String> = s.ui.panel.files.dirs.keys().cloned().collect();
    for dir in if dirs.is_empty() { vec![String::new()] } else { dirs } {
        load(s, dir);
    }
    if !s.ui.panel.files.query.text.trim().is_empty() {
        search(s);
    }
    let ui = &s.ui.panel.files;
    if !ui.selected.is_empty() && ui.writes.is_empty() && ui.editor.text == ui.saved {
        let path = ui.selected.clone();
        read(s, path);
    }
}

fn on_key(s: &mut App, e: &KeyEvent, _: &mut Tx) -> bool {
    if matches!(e.key, Key::Escape) {
        s.ui.panel.files.editor.focused = false;
        return true;
    }
    false
}

/// PNG and JPEG decode here; other formats fail to the "Open in browser" fallback.
fn decode(url: &str, pixels: &rune::Pixels) -> Option<()> {
    use std::io::Read;
    let mut bytes = Vec::new();
    ureq::get(url).call().ok()?.into_reader().take(50 * 1024 * 1024).read_to_end(&mut bytes).ok()?;
    let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
    pixels.set(image.width(), image.height(), image.into_raw());
    Some(())
}

fn image_view(image: &Image, t: &T) -> El {
    if image.failed.load(std::sync::atomic::Ordering::Relaxed) {
        let mut notice = col().grow(1.).center().gap(12.).padding(24.).child(text("Unable to load workspace image.").font(theme::TEXT_XS).color(t.c.destructive_foreground));
        if let Some(url) = image.url.clone() {
            notice = notice.child(
                kit::Button::new("Open in browser")
                    .icon(&icons::EXTERNAL_LINK)
                    .variant("outline")
                    .size("xs")
                    .build(t)
                    .on_press_tx(move |_: &mut App, tx: &mut Tx| {
                        let _ = tx.open_url(&url);
                    }),
            );
        }
        return notice;
    }
    el().grow(1.).min_h(0.).min_w(0.).padding(16.).pixels_fit(&image.pixels, rune::SurfaceFit::Fit)
}

fn editor(ui: &Ui, t: &T) -> El {
    let lang = ui.selected.rsplit('.').next();
    let spans = rune_kit::syntax::SyntaxTheme::neutral(t.dark)
        .spans(&rune_kit::syntax::highlight(lang, &ui.editor.text), 0);
    let cell = rune::text::advance("MMMMMMMMMM", CODE) / 10.;
    let width = (ui.columns + 2) as f32 * cell;
    let mark = ui.line.and_then(|n| line_range(&ui.editor.text, n)).map(|range| rune::TextHighlight::new(range, t.c.primary.alpha_mul(0.25)).key("file-line"));
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
    let code = code.text_highlights(mark);
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

fn header(ui: &Ui, t: &T) -> El {
    let button = |label: &str, glyph: &'static rune::Icon| kit::Button::new(label).icon(glyph).variant("ghost-muted").size("icon-xs");
    let reload = button("Refresh workspace files", &icons::REFRESH_CW).on_press(refresh).build(t).help("Refresh files");
    let header = row().h(40.).px(8.).gap(4.).center_y();
    if !ui.selected.is_empty() {
        return header
            .child(button("Back to files", &icons::ARROW_LEFT).on_press(close_file).build(t).help("Back to files"))
            .child(text(&ui.selected).font(theme::TEXT_XS).color(t.c.contrast_foreground).single_line().truncate_middle().grow(1.).shrink().min_w(0.))
            .child(reload);
    }
    let search = text_input(&ui.query, theme::TEXT_SM)
        .key("files-search")
        .label("Search files")
        .single_line()
        .h(28.)
        .px(8.)
        .py(4.)
        .grow(1.)
        .min_w(0.)
        .radius(6.)
        .color(t.c.foreground)
        .placeholder("Search files", t.c.muted_foreground)
        .on_edit(|s: &mut App| &mut s.ui.panel.files.query)
        .on_input_change(search_changed)
        .on_input_key(|s: &mut App, e: &KeyEvent, tx: &mut Tx| {
            if e.key != Key::Escape {
                return false;
            }
            let ui = &mut s.ui.panel.files;
            ui.query = TextEdit::new("");
            search_changed(s, tx);
            true
        });
    let mut header = header.child(reload).child(search);
    let known = known_dirs(&ui.dirs);
    if !known.is_empty() {
        let all_open = ui.expand_all || known.iter().all(|d| ui.expanded.contains(d));
        let (label, glyph) = if all_open { ("Collapse all folders", &icons::CHEVRONS_DOWN_UP) } else { ("Expand all folders", &icons::CHEVRONS_UP_DOWN) };
        header = header.child(button(label, glyph).on_press(toggle_all).build(t).help(label));
    }
    header
}

fn tree(ui: &Ui, t: &T) -> El {
    let status = |label: &str| row().px(12.).py(4.).child(text(label.to_string()).font(theme::TEXT_XS).color(t.c.muted_foreground));
    let mut list = col().w_full().py(4.);
    let query = ui.query.text.trim();
    let (visible, loading) = if query.is_empty() {
        (rows(&ui.dirs, &|p| ui.expanded.contains(p)), !ui.dirs.contains_key(""))
    } else {
        match &ui.found {
            Some((_, found, truncated)) => {
                if *truncated {
                    list = list.child(status("More matches available. Refine your search."));
                }
                (rows(&search_tree(found), &|_| true), ui.searching.is_some() || ui.search_due.is_some())
            }
            None => (vec![], true),
        }
    };
    if loading {
        list = list.child(status("Loading files\u{2026}"));
    } else if visible.is_empty() && !query.is_empty() {
        list = list.child(status("No matching files."));
    }
    for r in visible {
        let selected = !r.dir && r.path == ui.selected;
        let glyph = match (r.dir, r.open) {
            (true, true) => &icons::FOLDER_OPEN,
            (true, false) => &icons::FOLDER,
            _ => &icons::FILE,
        };
        let mut line = row()
            .key(format!("files-row-{}", r.path))
            .w_full()
            .h(26.)
            .pl(8. + r.depth as f32 * 12.)
            .pr(8.)
            .gap(6.)
            .center_y()
            .cursor(rune::Cursor::Pointer)
            .focusable(true)
            .role(if r.dir { "button" } else { "link" })
            .label(&r.path)
            .hover(t.c.accent)
            .child(if r.dir {
                icon(if r.open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 12.).size(12., 12.).color(t.c.muted_foreground)
            } else {
                el().size(12., 12.)
            })
            .child(icon(glyph, 14.).size(14., 14.).color(t.c.muted_foreground))
            .child(text(name(&r.path).to_string()).font(theme::TEXT_XS).color(t.c.contrast_foreground).single_line().truncate().grow(1.).min_w(0.));
        if selected {
            line = line.fill(t.c.accent);
        }
        let (path, menu_path) = (r.path.clone(), r.path.clone());
        let dir = r.dir;
        line = line
            .on_press(move |s: &mut App| if dir { toggle_dir(s, path.clone()) } else { read(s, path.clone()) })
            .on_secondary_press(move |s: &mut App, at: rune::PointerPress, _: &mut Tx| {
                s.ui.panel.files.menu = Some((menu_path.clone(), (at.x, at.y)));
            });
        list = list.child(line);
    }
    scroll(list)
}

/// A row's right-click menu: Copy mention, Add to chat.
pub fn overlays(app: &App, t: &T, out: &mut Vec<El>) {
    let Some((path, (x, y))) = app.ui.panel.files.menu.clone() else {
        return;
    };
    let link = file_link(&path);
    let (copy_link, chat_link, copied) = (link.clone(), link, path.clone());
    let entries = vec![
        kit::MenuEntry::item(kit::MenuItem::new("Copy mention").on_select_tx(move |s: &mut App, tx: &mut Tx| {
            s.ui.panel.files.menu = None;
            tx.copy(copy_link.clone());
            crate::shell::palette::toast(s, "Mention copied", Some(&copied));
        })),
        kit::MenuEntry::item(kit::MenuItem::new("Add to chat").on_select(move |s: &mut App| {
            s.ui.panel.files.menu = None;
            crate::chat::composer::append_to_draft(s, &format!("{chat_link} "));
        })),
    ];
    kit::Menu::new("files-menu")
        .native(true)
        .width(198.)
        .entries(entries)
        .open(true)
        .at(x, y)
        .on_close(|s: &mut App| s.ui.panel.files.menu = None)
        .overlay(&kit::theme(t), out);
}

pub fn view(app: &App, t: &T) -> El {
    let ui = &app.ui.panel.files;
    let header = header(ui, t);
    let content = if let Some(error) = &ui.error {
        scroll(
            row()
                .w_full()
                .padding(16.)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .on_press(|s: &mut App| {
                    s.ui.panel.files.error = None;
                    refresh(s);
                })
                .child(text(format!("{error} Click to retry.")).font(theme::TEXT_XS).color(t.c.destructive).wrap().grow(1.)),
        )
    } else if let Some(image) = &ui.image {
        image_view(image, t)
    } else if !ui.selected.is_empty() {
        editor(ui, t)
    } else {
        tree(ui, t)
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
    app.ui.panel.files.dirs.insert(String::new(), v["entries"].as_array().unwrap().clone());
}

/// A recorded file open in the editor; `edit` types into it, `error` fails its save.
pub fn edit_fixture(app: &mut App, edit: Option<&str>, error: bool) {
    fixture(app);
    let v: Value =
        serde_json::from_str(include_str!("../../fixtures/panel/file-edit.json")).unwrap();
    let ui = &mut app.ui.panel.files;
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
    fn a_search_match_marks_its_whole_line() {
        let text = "one\ntwo words\n\nlast";
        assert_eq!(line_range(text, 1), Some(0..3));
        assert_eq!(&text[line_range(text, 2).unwrap()], "two words");
        assert_eq!(line_range(text, 3), Some(14..14));
        assert_eq!(&text[line_range(text, 4).unwrap()], "last");
        assert_eq!(line_range(text, 5), None);
    }

    #[test]
    fn images_preview_and_read_failures_read_like_the_web() {
        assert!(is_image("assets/Logo.PNG") && is_image("a.svg") && !is_image("png.ts"));
        let cause = json!([{"_tag":"Fail","error":{"_tag":"ProjectReadFileError","failure":"binary_file","message":"Failed to read workspace file 'a.bin'."}}]);
        assert_eq!(read_error(&cause), "The file is binary and cannot be displayed as text.");
        let cause = json!([{"_tag":"Fail","error":{"failure":"operation_failed","operation":"realpath-workspace-root","message":"x"}}]);
        assert_eq!(read_error(&cause), "The workspace folder could not be accessed.");
        assert_eq!(read_error(&json!({"message":"network worker stopped"})), "network worker stopped");
    }

    #[test]
    fn the_tree_lists_folders_first_and_search_shows_matches_under_their_folders() {
        let entries = |v: Value| v.as_array().unwrap().clone();
        let mut dirs = HashMap::new();
        dirs.insert(String::new(), entries(json!([{"path":"b.ts","kind":"file"},{"path":"src","kind":"directory"},{"path":"A.md","kind":"file"},{"path":"lib","kind":"directory"}])));
        dirs.insert("src".to_string(), entries(json!([{"path":"src/main.rs","kind":"file"}])));
        let view = |open: &[&str]| -> Vec<(String, usize)> { rows(&dirs, &|p| open.iter().any(|o| *o == p)).into_iter().map(|r| (r.path, r.depth)).collect() };
        let at = |p: &str, d: usize| (p.to_string(), d);
        assert_eq!(view(&[]), [at("lib", 0), at("src", 0), at("A.md", 0), at("b.ts", 0)]);
        // An open folder that hasn't loaded shows no children yet.
        assert_eq!(view(&["src", "lib"]), [at("lib", 0), at("src", 0), at("src/main.rs", 1), at("A.md", 0), at("b.ts", 0)]);
        assert_eq!(known_dirs(&dirs).len(), 2);

        let found = search_tree(&entries(json!([{"path":"src/a/x.rs","kind":"file"},{"path":"src/b.rs","kind":"file"}])));
        let rows: Vec<(String, usize, bool)> = rows(&found, &|_| true).into_iter().map(|r| (r.path, r.depth, r.dir)).collect();
        assert_eq!(
            rows,
            [("src".into(), 0, true), ("src/a".into(), 1, true), ("src/a/x.rs".into(), 2, false), ("src/b.rs".into(), 1, false)]
        );
    }

    #[test]
    fn mentions_match_the_web_file_link() {
        assert_eq!(file_link("src/main.rs"), "[main.rs](src/main.rs)");
        assert_eq!(file_link("docs/a b (1)#?.md"), "[a b (1)#?.md](docs/a%20b%20%281%29%23%3F.md)");
        assert_eq!(file_link("x/[n]é.md"), "[\\[n\\]é.md](x/%5Bn%5D%C3%A9.md)");
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
