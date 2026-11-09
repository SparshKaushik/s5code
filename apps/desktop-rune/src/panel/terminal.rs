//! Terminal sessions shared by the drawer and the right panel, keyed by thread and terminal id,
//! and the viewport that draws one. Wire payloads match the web's `terminal.*` RPCs.

use std::time::{Duration, Instant};

use super::vt::{self, Link, MouseButton, Palette, SelectBy, Vt, Wheel, links};
use crate::{
    App, data, kit,
    theme::{self, T},
};
use rune::{Color, Cursor, El, Key, KeyEvent, Phase, TextEdit, Tx, el, text, text_input};
use serde_json::{Value, json};

/// The web terminal's blink period; it blinks only while focused.
const BLINK: Duration = Duration::from_millis(500);
/// Blinks on the compositor's wall clock, so an idle focused terminal repaints its cursor a few
/// times a second without rebuilding the view.
const BLINK_CYCLE: rune::Cycle = rune::Cycle {
    period: 1.,
    phase: 0.,
    stops: &[(0., 1.), (0.5, 1.), (0.5001, 0.), (1., 0.)],
    still: 1.,
    fps: 4.,
};
const RESIZE_SETTLE: Duration = Duration::from_millis(150);
const PADDING: f32 = 4.;

pub struct Session {
    pub thread: String,
    pub id: String,
    pub vt: Vt,
    payload: Option<Value>,
    sequence: Option<u64>,
    pub status: String,
    pub label: String,
    /// Hidden field that owns keyboard focus, IME and paste for this terminal.
    pub input: TextEdit,
    blink_origin: Instant,
    /// The cursor is handed to [`BLINK_CYCLE`]; it stays solid for a beat after input.
    cursor_blinks: bool,
    button: Option<MouseButton>,
    /// What the primary button's press is doing until it lifts.
    press: Option<Press>,
    /// The link under the pointer, underlined and opened by a plain click.
    pub hover: Option<Link>,
    /// The grid the viewport fits, re-sent after an attach snapshot brings the server's own.
    fit: Option<(usize, usize)>,
    /// Set once the drawer has acted on the session ending.
    pub ended: bool,
    exit_noted: bool,
    /// When the PTY hears the settled grid (`notifyResize` in the web's ghostty surface).
    notify_at: Option<Instant>,
    /// A foreground process holds the shell (`hasRunningSubprocess`).
    pub running: bool,
    attached: bool,
    /// Input typed once the attach snapshot lands, for a project script run in a new terminal.
    queued: Option<String>,
}

impl Session {
    fn new(thread: &str, id: &str) -> Self {
        Self {
            thread: thread.into(),
            id: id.into(),
            vt: Vt::new(80, 24),
            payload: None,
            sequence: None,
            status: String::new(),
            label: String::new(),
            input: TextEdit::new(""),
            blink_origin: Instant::now(),
            cursor_blinks: false,
            button: None,
            press: None,
            hover: None,
            fit: None,
            ended: false,
            exit_noted: false,
            notify_at: None,
            running: false,
            attached: false,
            queued: None,
        }
    }

    /// The session's own label, else "Terminal N" from its id (`getTerminalLabel`).
    pub fn title(&self) -> String {
        let label = self.label.trim();
        if label.is_empty() {
            label_for(&self.id)
        } else {
            label.into()
        }
    }

    fn wake_cursor(&mut self) {
        self.blink_origin = Instant::now();
        self.cursor_blinks = false;
    }
}

/// The selection as a terminal context chip in the thread's composer (ThreadTerminalDrawer).
fn add_to_chat(s: &mut App, thread: &str, id: &str) {
    let Some(session) = s.ui.panel.terminal.get(thread, id) else { return };
    let (Some(text), Some((start, end))) = (session.vt.selection_text(), session.vt.selection_lines()) else { return };
    let label = session.title();
    for session in &mut s.ui.panel.terminal.sessions {
        session.input.focused = false;
    }
    crate::chat::composer::add_terminal_context(s, thread, id, &label, start, end, &text);
}

pub fn label_for(id: &str) -> String {
    match number(id) {
        Some(n) => format!("Terminal {n}"),
        None => id.into(),
    }
}

fn number(id: &str) -> Option<&str> {
    let rest = id
        .strip_prefix("terminal-")
        .or_else(|| id.strip_prefix("term-"))?;
    let digits = rest.split('-').next()?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then_some(digits)
}

/// The smallest unused `term-N` (`nextTerminalId`).
pub fn next_id<'a>(existing: impl IntoIterator<Item = &'a String>) -> String {
    let used: Vec<&str> = existing.into_iter().filter_map(|id| number(id)).collect();
    let n = (1..)
        .find(|n: &usize| !used.contains(&n.to_string().as_str()))
        .unwrap();
    format!("term-{n}")
}

/// A primary-button gesture on the grid (the ghostty surface's pointer handling).
#[derive(Clone, Debug, PartialEq)]
enum Press {
    /// Reported to a program that reads the mouse.
    Report,
    /// On a link: a release opens it, a drag past 4 points selects instead.
    Link(Link),
    Select { by: SelectBy, moved: bool },
}

/// The terminal's right-click menu (`terminalContextMenuItems`), at a window point.
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    thread: String,
    id: String,
    at: (f32, f32),
}

#[derive(Default)]
pub struct Ui {
    sessions: Vec<Session>,
    pub menu: Option<Menu>,
    pending: Vec<u64>,
    /// `terminal.close` requests, to type `exit` when one fails.
    closing: Vec<(u64, String, String)>,
    pub error: Option<String>,
    /// The palette last drawn with, for colour queries answered outside the view.
    palette: std::cell::Cell<Option<Palette>>,
    /// Edits for a field whose session closed under it.
    spare: TextEdit,
}

impl Ui {
    /// A terminal holds keyboard focus (`terminalFocus`).
    pub fn focused(&self) -> bool {
        self.sessions.iter().any(|s| s.input.focused)
    }
    pub fn get(&self, thread: &str, id: &str) -> Option<&Session> {
        self.sessions
            .iter()
            .find(|s| s.thread == thread && s.id == id)
    }
    fn get_mut(&mut self, thread: &str, id: &str) -> Option<&mut Session> {
        self.sessions
            .iter_mut()
            .find(|s| s.thread == thread && s.id == id)
    }
}

/// Attaches (opening when missing) the session for a terminal id, once.
pub fn ensure(app: &mut App, thread: &str, id: &str, cwd: &str, worktree: Option<&str>) {
    if app.ui.panel.terminal.get(thread, id).is_some() {
        return;
    }
    let mut session = Session::new(thread, id);
    let payload = attach_payload(thread, id, cwd, worktree);
    if let Some(net) = app.src.net() {
        net.watch_resource("terminal.attach", payload.clone());
        session.payload = Some(payload);
    }
    app.ui.panel.terminal.sessions.push(session);
}

/// Types `data` into the terminal, attaching it first when needed (`runProjectScript`).
pub fn run(app: &mut App, thread: &str, id: &str, cwd: &str, worktree: Option<&str>, data: String) {
    ensure(app, thread, id, cwd, worktree);
    match app.ui.panel.terminal.get_mut(thread, id) {
        Some(session) if !session.attached => session.queued = Some(data),
        _ => send(app, thread, id, data),
    }
}

/// No `resume`, like the web's first attach: the server answers a resumed attach to an id it
/// closed with `closed`, so a new terminal reusing that id would end as soon as it opened.
fn attach_payload(thread: &str, id: &str, cwd: &str, worktree: Option<&str>) -> Value {
    let mut payload = json!({"threadId":thread,"terminalId":id,"cwd":cwd});
    if let Some(path) = worktree {
        payload["worktreePath"] = json!(path);
    }
    payload
}

/// Stops the process and deletes its history; a failed close falls back to typing `exit`.
pub fn close(app: &mut App, thread: &str, id: &str) {
    let ui = &mut app.ui.panel.terminal;
    let Some(index) = ui
        .sessions
        .iter()
        .position(|s| s.thread == thread && s.id == id)
    else {
        return;
    };
    let session = ui.sessions.remove(index);
    if let Some(net) = app.src.net() {
        if let Some(payload) = session.payload {
            net.unwatch_resource("terminal.attach", payload);
        }
        let request = net.request(
            "terminal.close",
            json!({"threadId":thread,"terminalId":id,"deleteHistory":true}),
        );
        ui.pending.push(request);
        ui.closing.push((request, thread.to_string(), id.to_string()));
    }
}

pub fn focus(app: &mut App, thread: &str, id: &str) {
    for session in &mut app.ui.panel.terminal.sessions {
        session.input.focused = session.thread == thread && session.id == id;
    }
}

fn send(app: &mut App, thread: &str, id: &str, data: String) {
    if data.is_empty() {
        return;
    }
    if let Some(session) = app.ui.panel.terminal.get_mut(thread, id) {
        session.vt.scroll_to_bottom();
        session.wake_cursor();
    }
    let Some(net) = app.src.net() else { return };
    // The server caps one write at 64 KiB.
    let mut rest = data.as_str();
    while !rest.is_empty() {
        let mut end = rest.len().min(65_536);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        let request = net.request(
            "terminal.write",
            json!({"threadId":thread,"terminalId":id,"data":&rest[..end]}),
        );
        app.ui.panel.terminal.pending.push(request);
        rest = &rest[end..];
    }
}

fn resize(app: &mut App, thread: &str, id: &str, cols: usize, rows: usize) {
    let Some(session) = app.ui.panel.terminal.get_mut(thread, id) else {
        return;
    };
    session.fit = Some((cols, rows));
    // Recorded sessions keep the grid they were captured at.
    if session.vt.size() == (cols, rows) || session.payload.is_none() {
        return;
    }
    // The grid reflows now; the shell only hears settled sizes so a drag doesn't reprint its prompt.
    session.vt.resize(cols, rows);
    session.notify_at = Some(Instant::now() + RESIZE_SETTLE);
}

pub fn palette(t: &T) -> Palette {
    let rgb = |c: Color| {
        [c.r, c.g, c.b].map(|v| (rune::color::srgb_encode(v) * 255.).round() as u8)
    };
    Palette {
        foreground: rgb(t.c.terminal_foreground),
        background: rgb(t.c.terminal_background),
        ansi: if t.dark { vt::DARK_ANSI } else { vt::LIGHT_ANSI },
    }
}

/// Drains attach streams, replies and IME text, and runs the focused cursor's blink.
pub fn poll(app: &mut App, tx: &mut Tx) -> bool {
    let mut changed = false;
    let pending = std::mem::take(&mut app.ui.panel.terminal.pending);
    for request in pending {
        match app.src.net().and_then(|net| net.take_reply(request)) {
            None => app.ui.panel.terminal.pending.push(request),
            Some(result) => {
                let closing = app
                    .ui
                    .panel
                    .terminal
                    .closing
                    .iter()
                    .position(|(r, ..)| *r == request);
                if let Some(index) = closing {
                    let (_, thread, id) = app.ui.panel.terminal.closing.remove(index);
                    if result.is_err() {
                        fallback_exit(app, &thread, &id);
                    }
                } else if let Err(error) = result {
                    app.ui.panel.terminal.error = Some(data::s(&error, "message").to_string());
                    changed = true;
                }
            }
        }
    }

    let palette = app.ui.panel.terminal.palette.get().unwrap_or(Palette {
        foreground: [0xe5, 0xe5, 0xe5],
        background: [0, 0, 0],
        ansi: vt::DARK_ANSI,
    });
    let mut writes = vec![];
    let mut ended = vec![];
    let mut refit = vec![];
    let mut notify = vec![];
    for index in 0..app.ui.panel.terminal.sessions.len() {
        let payload = app.ui.panel.terminal.sessions[index].payload.clone();
        let events = match (payload, app.src.net()) {
            (Some(payload), Some(net)) => net.take_resource_events("terminal.attach", payload),
            _ => vec![],
        };
        let session = &mut app.ui.panel.terminal.sessions[index];
        for event in &events {
            apply(session, event);
            changed = true;
            if data::s(event, "type") == "snapshot"
                && let Some((cols, rows)) = session.fit
            {
                refit.push((session.thread.clone(), session.id.clone(), cols, rows));
            }
            if session.attached
                && let Some(queued) = session.queued.take()
            {
                writes.push((session.thread.clone(), session.id.clone(), queued));
            }
        }
        if session.vt.sync_pending() {
            changed |= session.vt.flush_sync(false);
            tx.wake_after(Duration::from_millis(50));
        }
        for reply in session.vt.take_replies(&palette) {
            writes.push((session.thread.clone(), session.id.clone(), reply));
        }
        if !session.input.text.is_empty() {
            let text = std::mem::take(&mut session.input.text);
            let focused = session.input.focused;
            session.input = TextEdit::new("");
            session.input.focused = focused;
            writes.push((session.thread.clone(), session.id.clone(), text));
        }
        if session.exit_noted && !session.ended {
            session.ended = true;
            ended.push((session.thread.clone(), session.id.clone()));
        }
        let blinking = session.input.focused
            && session.vt.cursor_blinks()
            && !session.vt.scrolled_back();
        let elapsed = session.blink_origin.elapsed();
        if blinking && elapsed < BLINK {
            tx.wake_after(BLINK - elapsed);
        }
        let blinks = blinking && elapsed >= BLINK;
        if blinks != session.cursor_blinks {
            session.cursor_blinks = blinks;
            changed = true;
        }
        if let Some(at) = session.notify_at {
            let now = Instant::now();
            if now >= at {
                session.notify_at = None;
                let (cols, rows) = session.vt.size();
                notify.push((session.thread.clone(), session.id.clone(), cols, rows));
            } else {
                tx.wake_after(at - now);
            }
        }
    }
    for (thread, id, cols, rows) in notify {
        if let Some(net) = app.src.net() {
            let request = net.request(
                "terminal.resize",
                json!({"threadId":thread,"terminalId":id,"cols":cols,"rows":rows}),
            );
            app.ui.panel.terminal.pending.push(request);
        }
    }
    for (thread, id, data) in writes {
        send(app, &thread, &id, data);
    }
    for (thread, id, cols, rows) in refit {
        resize(app, &thread, &id, cols, rows);
    }
    for (thread, id) in ended {
        super::drawer::session_ended(app, &thread, &id);
        changed = true;
    }
    changed
}

fn fallback_exit(app: &mut App, thread: &str, id: &str) {
    if let Some(net) = app.src.net() {
        let request = net.request(
            "terminal.write",
            json!({"threadId":thread,"terminalId":id,"data":"exit\n"}),
        );
        app.ui.panel.terminal.pending.push(request);
    }
}

fn apply(session: &mut Session, event: &Value) {
    if !event["error"].is_null() {
        session.status = data::s(&event["error"], "message").into();
        return;
    }
    let sequence = event["sequence"].as_u64();
    let kind = data::s(event, "type");
    if kind != "snapshot" && sequence.is_some_and(|s| session.sequence.is_some_and(|old| s <= old))
    {
        return;
    }
    match kind {
        "snapshot" | "restarted" => {
            let snapshot = &event["snapshot"];
            let (cols, rows) = session.vt.size();
            let cols = snapshot["cols"].as_u64().map_or(cols, |v| v as usize);
            let rows = snapshot["rows"].as_u64().map_or(rows, |v| v as usize);
            session.vt = Vt::new(cols, rows);
            session.vt.feed(data::s(snapshot, "history"));
            session.vt.flush_sync(true);
            // Replayed history already answered its own queries.
            session.vt.take_replies(&Palette {
                foreground: [0; 3],
                background: [0; 3],
                ansi: vt::DARK_ANSI,
            });
            session.status = data::s(snapshot, "status").into();
            session.label = data::s(snapshot, "label").into();
            session.sequence = snapshot["sequence"].as_u64();
            session.exit_noted = false;
            session.ended = false;
            session.running = snapshot["hasRunningSubprocess"] == true;
            session.attached = true;
            note_exit(session);
        }
        "activity" => {
            session.running = event["hasRunningSubprocess"] == true;
            if let (Some(cols), Some(rows)) = (event["cols"].as_u64(), event["rows"].as_u64()) {
                session.vt.resize(cols as usize, rows as usize);
            }
        }
        "output" => session.vt.feed(data::s(event, "data")),
        "cleared" => {
            let (cols, rows) = session.vt.size();
            session.vt = Vt::new(cols, rows);
        }
        "exited" => {
            session.status = "exited".into();
            note_exit(session);
        }
        "closed" => {
            session.status = "closed".into();
            note_exit(session);
        }
        "error" => session.status = data::s(event, "message").into(),
        _ => {}
    }
    if sequence.is_some() {
        session.sequence = sequence;
    }
}

/// Writes the web's system line once when the process is gone.
fn note_exit(session: &mut Session) {
    if session.exit_noted || !matches!(session.status.as_str(), "exited" | "closed") {
        return;
    }
    session.exit_noted = true;
    let message = if session.status == "closed" {
        "Terminal closed"
    } else {
        "Process exited"
    };
    session
        .vt
        .feed(&format!("\r\n\x1b[90m[terminal] {message}\x1b[0m\r\n"));
}

/// The terminal's own clipboard chords (`isTerminalCopyShortcut`, `isTerminalPasteShortcut`,
/// the surface's select-all), checked before the key reaches the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Clip {
    /// Copy the selection; a plain Ctrl+C off macOS also clears it so the next one interrupts.
    Copy { clear: bool },
    Paste,
    SelectAll,
}

fn clip(e: &KeyEvent, mac: bool, selection: bool) -> Option<Clip> {
    let m = e.mods;
    let letter = match &e.key {
        Key::Command(c) => Some(c.to_ascii_lowercase()),
        Key::Text(t) if m.ctrl || m.cmd => t.chars().next().map(|c| c.to_ascii_lowercase()),
        _ => None,
    };
    let primary = if mac { m.cmd && !m.ctrl } else { m.ctrl && !m.logo };
    match (letter, &e.key) {
        (Some('c'), _) if primary && selection && !m.alt => Some(Clip::Copy { clear: !mac && !m.shift }),
        (_, Key::Insert) if !mac && m.ctrl && !m.shift && selection => Some(Clip::Copy { clear: false }),
        (Some('v'), _) if primary && !m.alt && (mac || m.shift) => Some(Clip::Paste),
        (_, Key::Insert) if !mac && m.shift && !m.ctrl => Some(Clip::Paste),
        (Some('a'), _) if primary && !m.alt && (mac != m.shift) => Some(Clip::SelectAll),
        _ => None,
    }
}

fn copy_selection(app: &mut App, tx: &mut Tx, thread: &str, id: &str, clear: bool) {
    let Some(session) = app.ui.panel.terminal.get_mut(thread, id) else {
        return;
    };
    if let Some(text) = session.vt.selection_text() {
        tx.copy(text);
    }
    if clear {
        session.vt.clear_selection();
    }
}

fn paste_clipboard(tx: &mut Tx, thread: &str, id: &str) {
    let (thread, id) = (thread.to_string(), id.to_string());
    tx.read_clipboard(move |s: &mut App, content, _| {
        let text = content.text();
        let Some(session) = s.ui.panel.terminal.get(&thread, &id) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let data = session.vt.paste(text);
        send(s, &thread, &id, data);
    });
}

fn key(app: &mut App, tx: &mut Tx, thread: &str, id: &str, e: &KeyEvent) -> bool {
    if super::drawer::shortcut(app, thread, id, e) {
        return true;
    }
    let Some(session) = app.ui.panel.terminal.get(thread, id) else {
        return false;
    };
    let selection = session.vt.selection_text().is_some();
    match clip(e, cfg!(target_os = "macos"), selection) {
        Some(Clip::Copy { clear }) => {
            copy_selection(app, tx, thread, id, clear);
            return true;
        }
        Some(Clip::Paste) => {
            paste_clipboard(tx, thread, id);
            return true;
        }
        Some(Clip::SelectAll) => {
            if let Some(session) = app.ui.panel.terminal.get_mut(thread, id) {
                session.vt.select_all();
            }
            return true;
        }
        None => {}
    }
    let Some(session) = app.ui.panel.terminal.get_mut(thread, id) else {
        return false;
    };
    match session.vt.key(e) {
        Some(data) => {
            session.vt.clear_selection();
            send(app, thread, id, data);
            true
        }
        None => false,
    }
}

struct Metrics {
    font: rune::TextStyle,
    cell_w: f32,
    cell_h: f32,
}

fn metrics(app: &App) -> Metrics {
    let size = app.client_settings.values["fontSizeTerminal"]
        .as_f64()
        .unwrap_or(12.)
        .clamp(8., 20.) as f32;
    let cell_h = (size * 1.35).round().max(1.);
    let font = theme::mono(rune::TextStyle::new(size, 400.)).lh(cell_h);
    let cell_w = rune::text::advance("MMMMMMMMMM", font) / 10.;
    Metrics {
        font,
        cell_w: if cell_w > 0. { cell_w } else { size * 0.6 },
        cell_h,
    }
}

fn cell_at(app: &App, thread: &str, id: &str, x: f32, y: f32) -> (usize, usize) {
    let (col, row, _) = cell_side(app, thread, id, x, y);
    (col, row)
}

/// The cell under a point and whether the point is in its right half (the selection side).
fn cell_side(app: &App, thread: &str, id: &str, x: f32, y: f32) -> (usize, usize, bool) {
    let m = metrics(app);
    let (cols, rows) = app
        .ui
        .panel
        .terminal
        .get(thread, id)
        .map_or((80, 24), |s| s.vt.size());
    let fx = ((x - PADDING) / m.cell_w).max(0.);
    let col = (fx.floor() as usize).min(cols - 1);
    let right = fx >= cols as f32 || fx.fract() >= 0.5;
    (
        col,
        (((y - PADDING) / m.cell_h).floor().max(0.) as usize).min(rows - 1),
        right,
    )
}

/// `resolvePathLinkTarget`'s home for `~/`: the `/Users/<name>` or `/home/<name>` cwd sits in.
fn home_of(cwd: &str) -> Option<String> {
    let mut parts = cwd.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(""), Some(root @ ("Users" | "home")), Some(name)) if !name.is_empty() => Some(format!("/{root}/{name}")),
        _ => None,
    }
}

/// Where a path link points: inside the workspace (opened in the Files panel), else an
/// absolute path for the editor. Relative paths resolve against the terminal's directory.
#[derive(Debug, PartialEq)]
enum PathTarget {
    Workspace(String),
    Outside(String),
}

fn path_target(text: &str, cwd: &str) -> PathTarget {
    let path = links::strip_position(text);
    let joined = if let Some(rest) = path.strip_prefix("~/") {
        match home_of(cwd) {
            Some(home) => format!("{home}/{rest}"),
            None => path.to_string(),
        }
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{}/{path}", cwd.trim_end_matches('/'))
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    let absolute = format!("/{}", parts.join("/"));
    let root = format!("{}/", cwd.trim_end_matches('/'));
    match absolute.strip_prefix(&root) {
        Some(relative) if !relative.is_empty() && joined.starts_with('/') => PathTarget::Workspace(relative.to_string()),
        _ => PathTarget::Outside(absolute),
    }
}

/// Opens a clicked link: URLs in the browser, workspace files in the Files panel, other paths
/// in the preferred editor (`handleLinkActivate`).
fn open_link(app: &mut App, tx: &mut Tx, link: &Link) {
    if link.kind == links::Kind::Url {
        let _ = tx.open_url(&link.text);
        return;
    }
    let Some((_, cwd)) = super::context(app) else {
        return;
    };
    match path_target(&link.text, &cwd) {
        PathTarget::Workspace(relative) => super::open_file(app, relative),
        PathTarget::Outside(absolute) => {
            let config = app.state().config.as_ref();
            let editor = config
                .and_then(|c| c["availableEditors"].as_array())
                .and_then(|v| v.first())
                .map(|e| e.as_str().map_or_else(|| data::s(e, "id").to_string(), str::to_string));
            if let (Some(editor), Some(net)) = (editor, app.src.net()) {
                net.request("shell.openInEditor", json!({"cwd": absolute, "editor": editor}));
            }
        }
    }
}

/// One terminal's grid, sized to the space it's given.
pub fn viewport(app: &App, t: &T, thread: &str, id: &str) -> El {
    let m = metrics(app);
    let palette = palette(t);
    app.ui.panel.terminal.palette.set(Some(palette));
    let el_key = format!("terminal-{thread}-{id}");
    let mut grid = el()
        .key(el_key.clone())
        .grow(1.)
        .min_h(0.)
        .min_w(0.)
        .w_full()
        .h_full()
        .fill(t.c.terminal_background)
        .clip_rounded(0.)
        // Not "textbox": Rune hands a textbox's presses to its one field, starving `on_drag`.
        .role("group")
        .label("Terminal");
    let Some(session) = app.ui.panel.terminal.get(thread, id) else {
        return grid;
    };
    let (rows, cursor) = session.vt.frame(&palette);
    let rgb = |c: vt::Rgb8| {
        Color {
            r: rune::color::srgb_decode(c[0] as f32 / 255.),
            g: rune::color::srgb_decode(c[1] as f32 / 255.),
            b: rune::color::srgb_decode(c[2] as f32 / 255.),
            a: 1.,
        }
    };
    let x = |col: usize| PADDING + col as f32 * m.cell_w;
    let y = |row: usize| PADDING + row as f32 * m.cell_h;
    for (r, row) in rows.iter().enumerate() {
        for &(col, cells, color) in &row.fills {
            grid = grid.child(
                el().absolute(x(col), y(r))
                    .size(cells as f32 * m.cell_w, m.cell_h)
                    .fill(rgb(color)),
            );
        }
        for &(col, cells) in &row.selected {
            grid = grid.child(
                el().absolute(x(col), y(r))
                    .size(cells as f32 * m.cell_w, m.cell_h)
                    .fill(t.c.terminal_selection_background),
            );
        }
        for run in &row.runs {
            let mut font = m.font;
            if run.bold {
                font = theme::weight(font, 700.);
            }
            font.italic = run.italic;
            let fg = rgb(run.fg);
            grid = grid.child(
                text(run.text.clone())
                    .font(font)
                    .color(fg)
                    .absolute(x(run.col), y(r)),
            );
            let width = run.cells as f32 * m.cell_w;
            if run.underline {
                grid = grid.child(
                    el().absolute(x(run.col), y(r) + m.cell_h - 2.)
                        .size(width, 1.)
                        .fill(fg),
                );
            }
            if run.strike {
                grid = grid.child(
                    el().absolute(x(run.col), y(r) + (m.cell_h * 0.55).floor())
                        .size(width, 1.)
                        .fill(fg),
                );
            }
        }
    }
    if let Some(link) = &session.hover {
        for &(r, first, last) in &link.spans {
            grid = grid.child(
                el().absolute(x(first), y(r) + m.cell_h - 2.)
                    .size((last + 1 - first) as f32 * m.cell_w, 1.)
                    .fill(t.c.terminal_foreground),
            );
        }
    }
    let focused = session.input.focused;
    let blink = |e: El| if session.cursor_blinks { e.cycle(BLINK_CYCLE) } else { e };
    if let Some(c) = cursor {
        let (cx, cy) = (x(c.col), y(c.row));
        let color = t.c.terminal_cursor;
        let mark = match (focused, c.kind) {
            (false, _) | (true, vt::CursorKind::Hollow) => el()
                .absolute(cx, cy)
                .size(m.cell_w, m.cell_h)
                .border(1., color),
            (true, vt::CursorKind::Beam) => el().absolute(cx, cy).size(2., m.cell_h).fill(color),
            (true, vt::CursorKind::Underline) => el()
                .absolute(cx, cy + m.cell_h - 2.)
                .size(m.cell_w, 2.)
                .fill(color),
            (true, vt::CursorKind::Block) => el()
                .absolute(cx, cy)
                .size(m.cell_w, m.cell_h)
                .fill(color),
        };
        grid = grid.child(blink(mark));
        if let (true, vt::CursorKind::Block, Some(glyph)) = (focused, c.kind, &c.glyph) {
            let mut font = m.font;
            if glyph.bold {
                font = theme::weight(font, 700.);
            }
            grid = grid.child(blink(
                text(glyph.text.clone())
                    .font(font)
                    .color(t.c.terminal_background)
                    .absolute(cx, cy),
            ));
        }
    }
    let (cursor_col, cursor_row) = session.vt.cursor_point();
    let (t1, i1) = (thread.to_string(), id.to_string());
    let (t2, i2) = (t1.clone(), i1.clone());
    let (t3, i3) = (t1.clone(), i1.clone());
    let (t4, i4) = (t1.clone(), i1.clone());
    let (t5, i5) = (t1.clone(), i1.clone());
    let (t6, i6) = (t1.clone(), i1.clone());
    let (t7, i7) = (t1.clone(), i1.clone());
    let (t8, i8) = (t1.clone(), i1.clone());
    let (t9, i9) = (t1.clone(), i1.clone());
    let pointer = if session.hover.is_some() {
        Cursor::Pointer
    } else if session.vt.reports_mouse(false) {
        Cursor::Default
    } else {
        Cursor::Text
    };
    let input = text_input(&session.input, m.font)
        .single_line()
        .key(format!("{el_key}-input"))
        .label("Terminal input")
        .absolute(x(cursor_col), y(cursor_row.min(rows.len().saturating_sub(1))))
        .size(1., m.cell_h)
        // Not `opacity(0.)`: Rune never focuses a field it doesn't show.
        .color(Color::TRANSPARENT)
        .caret(Color::TRANSPARENT, Color::TRANSPARENT)
        .on_edit(move |s: &mut App| {
            let ui = &mut s.ui.panel.terminal;
            match ui
                .sessions
                .iter_mut()
                .find(|v| v.thread == t1 && v.id == i1)
            {
                Some(session) => &mut session.input,
                None => &mut ui.spare,
            }
        })
        .on_input_key(move |s: &mut App, e, tx| key(s, tx, &t2, &i2, e))
        .on_input_paste(move |s: &mut App, content, _| {
            let text = content.text();
            let Some(session) = s.ui.panel.terminal.get(&t3, &i3) else {
                return false;
            };
            let data = session.vt.paste(text);
            send(s, &t3, &i3, data);
            true
        });
    grid.child(input)
        .on_layout(move |s: &mut App, r| {
            let m = metrics(s);
            let cols = ((r[2] - 2. * PADDING) / m.cell_w).floor().clamp(2., 1000.) as usize;
            let rows = ((r[3] - 2. * PADDING) / m.cell_h).floor().clamp(1., 500.) as usize;
            resize(s, &t4, &i4, cols, rows);
        })
        .cursor(pointer)
        .on_drag(move |s: &mut App, d, tx| drag(s, tx, &t5, &i5, d))
        .on_pointer_move_local(move |s: &mut App, p, _| {
            let (col, row) = cell_at(s, &t6, &i6, p.x, p.y);
            let Some(session) = s.ui.panel.terminal.get_mut(&t6, &i6) else {
                return;
            };
            if session.button.is_some() || session.press.is_some() {
                return;
            }
            let report = session
                .vt
                .mouse(MouseButton::None, false, true, col, row, rune::Mods::default());
            let hover = if session.vt.reports_mouse(false) { None } else { session.vt.link_at(col, row) };
            if session.hover != hover {
                session.hover = hover;
            }
            if let Some(data) = report {
                send(s, &t6, &i6, data);
            }
        })
        .on_hover_change(move |s: &mut App, on, _| {
            if let (false, Some(session)) = (on, s.ui.panel.terminal.get_mut(&t9, &i9)) {
                session.hover = None;
            }
        })
        .on_secondary_press_local(move |s: &mut App, p, tx| {
            let (col, row) = cell_at(s, &t8, &i8, p.x, p.y);
            let mods = tx.mods();
            let Some(session) = s.ui.panel.terminal.get(&t8, &i8) else {
                return;
            };
            // A program reading the mouse gets the right button; Shift brings the menu back.
            if session.vt.reports_mouse(mods.shift || mods.ctrl || mods.cmd) {
                let press = session.vt.mouse(MouseButton::Right, true, false, col, row, mods);
                let release = session.vt.mouse(MouseButton::Right, false, false, col, row, mods);
                send(s, &t8, &i8, press.unwrap_or_default() + &release.unwrap_or_default());
                return;
            }
            let at = p.window();
            focus(s, &t8, &i8);
            s.ui.panel.terminal.menu = Some(Menu { thread: t8.clone(), id: i8.clone(), at: (at.x, at.y) });
        })
        .on_wheel(move |s: &mut App, w, _| {
            let m = metrics(s);
            let (col, row) = cell_at(s, &t7, &i7, w.x, w.y);
            let Some(session) = s.ui.panel.terminal.get_mut(&t7, &i7) else {
                return false;
            };
            match session.vt.wheel(w.dy / m.cell_h, col, row, w.mods) {
                Wheel::Send(data) => send(s, &t7, &i7, data),
                Wheel::Scrolled(_) => {}
            }
            true
        })
}

/// The primary button on the grid: mouse reports for programs that read the mouse (Shift,
/// Ctrl or Cmd select instead), a plain click on a link opens it, and otherwise a press
/// selects by cell, word (double click) or line (triple click) as it drags.
fn drag(s: &mut App, tx: &mut Tx, thread: &str, id: &str, d: &mut rune::Drag) {
    if d.phase == Phase::Down {
        focus(s, thread, id);
        super::drawer::activate(s, thread, id);
    }
    let (col, row, right) = cell_side(s, thread, id, d.x, d.y);
    let (c0, r0, right0) = cell_side(s, thread, id, d.start_x, d.start_y);
    let mods = tx.mods();
    let mut open = None;
    let mut report = None;
    let Some(session) = s.ui.panel.terminal.get_mut(thread, id) else {
        return;
    };
    match d.phase {
        Phase::Down => {
            session.press = Some(if session.vt.reports_mouse(mods.shift || mods.ctrl || mods.cmd) {
                session.button = Some(MouseButton::Left);
                report = session.vt.mouse(MouseButton::Left, true, false, col, row, mods);
                Press::Report
            } else if let (1, false, Some(link)) = (d.clicks, mods.shift, session.vt.link_at(col, row)) {
                Press::Link(link)
            } else {
                let by = match d.clicks {
                    0 | 1 => SelectBy::Cell,
                    2 => SelectBy::Word,
                    _ => SelectBy::Line,
                };
                session.hover = None;
                session.vt.select_start(col, row, right, by);
                Press::Select { by, moved: false }
            });
        }
        Phase::Move => match session.press.clone() {
            Some(Press::Report) => {
                report = session.vt.mouse(MouseButton::Left, true, true, col, row, mods);
            }
            Some(Press::Link(_)) if d.dx.hypot(d.dy) > 4. => {
                session.hover = None;
                session.vt.select_start(c0, r0, right0, SelectBy::Cell);
                session.vt.select_to(col, row, right);
                session.press = Some(Press::Select { by: SelectBy::Cell, moved: true });
            }
            Some(Press::Select { by, .. }) => {
                // Past the top or bottom edge, the view scrolls through history as it selects.
                if d.y < 0. {
                    session.vt.scroll_lines(1);
                } else if d.y > d.h {
                    session.vt.scroll_lines(-1);
                }
                session.vt.select_to(col, row, right);
                session.press = Some(Press::Select { by, moved: true });
            }
            _ => {}
        },
        Phase::Up => match session.press.take() {
            Some(Press::Report) => {
                session.button = None;
                report = session.vt.mouse(MouseButton::Left, false, false, col, row, mods);
            }
            Some(Press::Link(link)) if !d.cancelled() => open = Some(link),
            Some(Press::Select { by: SelectBy::Cell, moved: false }) => session.vt.clear_selection(),
            _ => {}
        },
    }
    if let Some(data) = report {
        send(s, thread, id, data);
    }
    if let Some(link) = open {
        open_link(s, tx, &link);
    }
}

/// The terminal's right-click menu: Add to chat, Copy, Paste, Select all, Jump to latest.
pub fn overlays(app: &App, t: &T, out: &mut Vec<El>) {
    let Some(menu) = &app.ui.panel.terminal.menu else {
        return;
    };
    let selection = app
        .ui
        .panel
        .terminal
        .get(&menu.thread, &menu.id)
        .is_some_and(|s| s.vt.selection_text().is_some());
    let item = |label: &str, act: fn(&mut App, &mut Tx, &str, &str)| {
        let (thread, id) = (menu.thread.clone(), menu.id.clone());
        kit::MenuItem::new(label).on_select_tx(move |s: &mut App, tx: &mut Tx| {
            s.ui.panel.terminal.menu = None;
            act(s, tx, &thread, &id);
            focus(s, &thread, &id);
        })
    };
    let entries = vec![
        kit::MenuEntry::item({
            // Not `item`: the caret goes to the composer, not back to the terminal.
            let (thread, id) = (menu.thread.clone(), menu.id.clone());
            kit::MenuItem::new("Add to chat")
                .on_select_tx(move |s: &mut App, _: &mut Tx| {
                    s.ui.panel.terminal.menu = None;
                    add_to_chat(s, &thread, &id);
                })
                .disabled(!selection)
        }),
        kit::MenuEntry::item(item("Copy", |s, tx, th, id| copy_selection(s, tx, th, id, false)).disabled(!selection)),
        kit::MenuEntry::item(item("Paste", |_, tx, th, id| paste_clipboard(tx, th, id))),
        kit::MenuEntry::item(item("Select all", |s, _, th, id| {
            if let Some(session) = s.ui.panel.terminal.get_mut(th, id) {
                session.vt.select_all();
            }
        })),
        kit::MenuEntry::item(item("Jump to latest", |s, _, th, id| {
            if let Some(session) = s.ui.panel.terminal.get_mut(th, id) {
                session.vt.scroll_to_bottom();
            }
        })),
    ];
    kit::Menu::new("terminal-menu")
        .native(true)
        .width(198.)
        .entries(entries)
        .open(true)
        .at(menu.at.0, menu.at.1)
        .on_close(|s: &mut App| s.ui.panel.terminal.menu = None)
        .overlay(&kit::theme(t), out);
}

/// Loads a recorded `terminal.attach` snapshot into a session for headless scenes.
pub fn fixture(app: &mut App, thread: &str, id: &str, recorded: &str) {
    let event: Value = serde_json::from_str(recorded).expect("terminal fixture is JSON");
    if app.ui.panel.terminal.get(thread, id).is_none() {
        app.ui.panel.terminal.sessions.push(Session::new(thread, id));
    }
    let session = app.ui.panel.terminal.get_mut(thread, id).unwrap();
    apply(session, &event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presses_on_the_grid_select_by_drag_and_a_click_clears() {
        fn view(s: &App, _: &mut rune::Cx<App>) -> El {
            el().size(600., 300.).child(viewport(s, &theme::theme(true), "t", "term-1"))
        }
        let mut app = App::new();
        fixture(
            &mut app,
            "t",
            "term-1",
            r#"{"type":"snapshot","snapshot":{"history":"hello brave world","cols":40,"rows":10,"sequence":1,"status":"running"}}"#,
        );
        let m = metrics(&app);
        let mut rt = rune::Runtime::new(app, view, (600., 300.), rune::Env::new(true, 1.));
        rt.frame(0.);
        let (x, y) = (|col: f32| PADDING + col * m.cell_w, PADDING + m.cell_h * 0.5);
        let selection = |rt: &rune::Runtime<App>| rt.state.ui.panel.terminal.get("t", "term-1").unwrap().vt.selection_text();
        rt.pointer_down(x(0.2), y);
        rt.pointer_move(x(2.), y);
        rt.pointer_move(x(4.8), y);
        rt.pointer_up(x(4.8), y);
        rt.frame(0.);
        assert_eq!(selection(&rt).as_deref(), Some("hello"));
        rt.pointer_down(x(8.), y);
        rt.pointer_up(x(8.), y);
        rt.frame(0.);
        assert_eq!(selection(&rt), None, "a click clears the selection");
    }

    #[test]
    fn snapshot_then_ordered_events_and_resume() {
        let mut session = Session::new("t", "term-1");
        apply(
            &mut session,
            &json!({"type":"snapshot","snapshot":{"history":"$ ","cols":80,"rows":24,"sequence":10,"status":"running"}}),
        );
        for seq in [11, 11, 9, 12] {
            apply(
                &mut session,
                &json!({"type":"output","sequence":seq,"data":"x"}),
            );
        }
        assert_eq!(session.vt.text(), "$ xx");
        apply(
            &mut session,
            &json!({"type":"activity","sequence":13,"cols":100,"rows":30}),
        );
        assert_eq!(session.vt.size(), (100, 30));
        apply(
            &mut session,
            &json!({"type":"snapshot","snapshot":{"history":"resumed","sequence":20}}),
        );
        assert_eq!(session.vt.text(), "resumed");
        apply(
            &mut session,
            &json!({"type":"exited","sequence":21,"exitCode":0}),
        );
        assert!(session.vt.text().ends_with("[terminal] Process exited"));
        assert!(session.exit_noted);
    }

    #[test]
    fn recorded_full_screen_programs_replay() {
        for (recorded, expect) in [
            (include_str!("../../fixtures/panel/terminal-vim.json"), "fn main"),
            (include_str!("../../fixtures/panel/terminal-htop.json"), "Load average"),
        ] {
            let mut session = Session::new("t", "term-1");
            apply(&mut session, &serde_json::from_str(recorded).unwrap());
            assert!(
                session.vt.mode().contains(alacritty_terminal::term::TermMode::ALT_SCREEN),
                "full-screen programs run on the alternate screen"
            );
            assert!(session.vt.text().contains(expect), "{}", session.vt.text());
        }
    }

    #[test]
    fn attach_opens_a_closed_id_again() {
        let payload = attach_payload("t", "term-2", "/repo", Some("/wt"));
        assert!(payload.get("resume").is_none(), "{payload}");
        assert_eq!(payload["worktreePath"], "/wt");
        assert!(attach_payload("t", "term-2", "/repo", None).get("worktreePath").is_none());
    }

    #[test]
    fn clipboard_chords_follow_the_platform() {
        let chord = |c: char, f: fn(&mut rune::Mods)| {
            let mut e = KeyEvent::from(Key::Command(c));
            f(&mut e.mods);
            e
        };
        let ctrl = chord('c', |m| (m.ctrl, m.cmd) = (true, true));
        assert_eq!(clip(&ctrl, false, true), Some(Clip::Copy { clear: true }));
        assert_eq!(clip(&ctrl, false, false), None, "no selection: Ctrl+C interrupts");
        let ctrl_shift = chord('C', |m| (m.ctrl, m.cmd, m.shift) = (true, true, true));
        assert_eq!(clip(&ctrl_shift, false, true), Some(Clip::Copy { clear: false }));
        let paste = chord('V', |m| (m.ctrl, m.cmd, m.shift) = (true, true, true));
        assert_eq!(clip(&paste, false, false), Some(Clip::Paste));
        let ctrl_v = chord('v', |m| (m.ctrl, m.cmd) = (true, true));
        assert_eq!(clip(&ctrl_v, false, false), None, "Ctrl+V is the program's");
        let cmd_c = chord('c', |m| m.cmd = true);
        assert_eq!(clip(&cmd_c, true, true), Some(Clip::Copy { clear: false }));
        assert_eq!(clip(&chord('v', |m| m.cmd = true), true, false), Some(Clip::Paste));
        assert_eq!(clip(&chord('a', |m| m.cmd = true), true, false), Some(Clip::SelectAll));
        let mac_ctrl_c = chord('c', |m| m.ctrl = true);
        assert_eq!(clip(&mac_ctrl_c, true, true), None, "Ctrl+C on macOS interrupts");
    }

    #[test]
    fn path_links_open_in_the_workspace_or_the_editor() {
        let cwd = "/home/me/repo";
        assert_eq!(path_target("src/main.rs:12:3", cwd), PathTarget::Workspace("src/main.rs".into()));
        assert_eq!(path_target("./a/../b.ts", cwd), PathTarget::Workspace("b.ts".into()));
        assert_eq!(path_target("/home/me/repo/x/y", cwd), PathTarget::Workspace("x/y".into()));
        assert_eq!(path_target("~/notes.md", cwd), PathTarget::Outside("/home/me/notes.md".into()));
        assert_eq!(path_target("../other/z", cwd), PathTarget::Outside("/home/me/other/z".into()));
        assert_eq!(path_target("/etc/hosts", cwd), PathTarget::Outside("/etc/hosts".into()));
    }

    #[test]
    fn terminal_ids_and_labels_follow_the_web() {
        let ids = ["term-1".to_string(), "term-3".to_string()];
        assert_eq!(next_id(&ids), "term-2");
        assert_eq!(next_id(&[]), "term-1");
        assert_eq!(label_for("term-12"), "Terminal 12");
        assert_eq!(label_for("custom"), "custom");
    }
}
