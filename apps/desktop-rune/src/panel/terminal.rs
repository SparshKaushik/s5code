//! Terminal sessions shared by the drawer and the right panel, keyed by thread and terminal id,
//! and the viewport that draws one. Wire payloads match the web's `terminal.*` RPCs.

use std::collections::HashMap;
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
/// A double or triple click waits this long before its selection menu, so a third click can land.
const MULTI_CLICK_MENU: Duration = Duration::from_millis(500);
/// Middle-click pastes the terminal's own selection on X11-style desktops (`isMiddleClickPastePlatform`).
const MIDDLE_PASTE: bool = cfg!(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"));

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
    /// The PTY's grid, from the attach snapshot and later activity.
    grid: Option<(usize, usize)>,
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
    /// The grid's top-left in window points, to place the selection menu at a release.
    origin: (f32, f32),
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
            grid: None,
            ended: false,
            exit_noted: false,
            notify_at: None,
            running: false,
            attached: false,
            queued: None,
            origin: (0., 0.),
        }
    }

    /// The grid to draw: the viewport's fit, never past the PTY's own (`applyGrid` in the web's
    /// ghostty surface), so a wider view waits for the shell instead of reflowing ahead of it.
    fn drawn(&self) -> Option<(usize, usize)> {
        match (self.fit, self.grid) {
            (Some((c, r)), Some((gc, gr))) => Some((c.min(gc), r.min(gr))),
            (fit, grid) => fit.or(grid),
        }
    }

    fn apply_grid(&mut self) {
        if let Some(size) = self.drawn().filter(|size| *size != self.vt.size()) {
            self.vt.resize(size.0, size.1);
        }
    }

    /// The PTY hears the fit once it settles, unless it already has it.
    fn schedule_notify(&mut self) {
        self.notify_at = (self.fit.is_some() && self.fit != self.grid).then(|| Instant::now() + RESIZE_SETTLE);
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

    /// Input sent before the attach snapshot waits for it, or the shell starting up drops it;
    /// a recorded session (no attach) never waits.
    fn hold(&mut self, data: String) -> Option<String> {
        if self.payload.is_none() || self.attached {
            return Some(data);
        }
        self.queued.get_or_insert_default().push_str(&data);
        None
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
    /// The selection's own menu after a mouse selection: Add to chat and Copy only.
    selection: bool,
}

/// A selection menu waiting out a double or triple click.
struct Popup {
    thread: String,
    id: String,
    at: (f32, f32),
    due: Instant,
}

#[derive(Default)]
pub struct Ui {
    sessions: Vec<Session>,
    pub menu: Option<Menu>,
    /// Requests whose failure is written into their terminal, as web's `writeSystemMessage`.
    pending: Vec<(u64, String, String)>,
    /// `terminal.close` requests, to type `exit` when one fails.
    closing: Vec<(u64, String, String)>,
    /// The palette last drawn with, for colour queries answered outside the view.
    palette: std::cell::Cell<Option<Palette>>,
    /// Edits for a field whose session closed under it.
    spare: TextEdit,
    popup: Option<Popup>,
    /// Whether each absolute path a link names exists (`filesystem.getMetadata`); `None` while asked.
    paths: HashMap<String, Option<bool>>,
    path_requests: Vec<(u64, String)>,
    /// A path link pressed before its check came back: opens if it exists.
    open_when_known: Option<(String, String)>,
    /// Every terminal the server runs (`subscribeTerminalMetadata`'s summaries).
    known: Vec<Value>,
    /// Closed here and not yet gone from `known`, so the drawer doesn't bring them back.
    gone: Vec<(String, String)>,
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

/// Where a terminal starts: its cwd, the thread's worktree and the project root its scripts see.
pub struct Launch<'a> {
    pub cwd: &'a str,
    pub worktree: Option<&'a str>,
    pub root: Option<&'a str>,
}

/// Attaches (opening when missing) the session for a terminal id, once.
pub fn ensure(app: &mut App, thread: &str, id: &str, at: &Launch) {
    if app.ui.panel.terminal.get(thread, id).is_some() {
        return;
    }
    app.ui.panel.terminal.gone.retain(|(t, i)| !(t == thread && i == id));
    let mut session = Session::new(thread, id);
    let payload = attach_payload(thread, id, at);
    if let Some(net) = app.src.net() {
        net.watch_resource("terminal.attach", payload.clone());
        session.payload = Some(payload);
    }
    app.ui.panel.terminal.sessions.push(session);
}

/// Types `data` into the terminal, attaching it first when needed (`runProjectScript`).
pub fn run(app: &mut App, thread: &str, id: &str, at: &Launch, data: String) {
    ensure(app, thread, id, at);
    send(app, thread, id, data);
}

/// No `resume`, like the web's first attach: the server answers a resumed attach to an id it
/// closed with `closed`, so a new terminal reusing that id would end as soon as it opened.
/// The project's scripts read `T3CODE_PROJECT_ROOT` and `T3CODE_WORKTREE_PATH` (`projectScriptRuntimeEnv`).
fn attach_payload(thread: &str, id: &str, at: &Launch) -> Value {
    let mut payload = json!({"threadId":thread,"terminalId":id,"cwd":at.cwd});
    if let Some(path) = at.worktree {
        payload["worktreePath"] = json!(path);
    }
    if let Some(root) = at.root {
        payload["env"] = json!({"T3CODE_PROJECT_ROOT": root});
        if let Some(path) = at.worktree {
            payload["env"]["T3CODE_WORKTREE_PATH"] = json!(path);
        }
    }
    payload
}

/// Folds a `subscribeTerminalMetadata` event into the server's terminal list.
fn fold_known(known: &mut Vec<Value>, event: &Value) {
    let same = |a: &Value, b: &Value| a["threadId"] == b["threadId"] && a["terminalId"] == b["terminalId"];
    match data::s(event, "type") {
        "snapshot" => *known = event["terminals"].as_array().cloned().unwrap_or_default(),
        "upsert" => {
            let terminal = &event["terminal"];
            match known.iter_mut().find(|k| same(k, terminal)) {
                Some(k) => *k = terminal.clone(),
                None => known.push(terminal.clone()),
            }
        }
        "remove" => known.retain(|k| !same(k, event)),
        _ => {}
    }
}

/// The thread's terminals the server knows, in id order (`selectKnownTerminalSessions`).
pub fn known(app: &App, thread: &str) -> Vec<(String, String, bool)> {
    let mut out: Vec<(String, String, bool)> = app
        .ui
        .panel
        .terminal
        .known
        .iter()
        .filter(|k| data::s(k, "threadId") == thread)
        .filter(|k| !app.ui.panel.terminal.gone.iter().any(|(t, i)| t == thread && i == data::s(k, "terminalId")))
        .map(|k| (data::s(k, "terminalId").to_string(), data::s(k, "label").trim().to_string(), k["hasRunningSubprocess"] == true))
        .collect();
    out.sort_by_key(|(id, ..)| (number(id).and_then(|n| n.parse::<u64>().ok()).unwrap_or(u64::MAX), id.clone()));
    out
}

/// Seeds the server's terminal list for headless scenes.
pub fn known_fixture(app: &mut App, thread: &str, running: &[&str]) {
    app.ui.panel.terminal.known = running.iter().map(|id| json!({"threadId": thread, "terminalId": id, "label": "", "hasRunningSubprocess": true})).collect();
}

/// How many of the thread's terminals run a process (`useThreadRunningTerminalIds`).
pub fn running(app: &App, thread: &str) -> usize {
    known(app, thread).iter().filter(|(.., running)| *running).count()
}

/// A terminal's label: the attached session's, else the server's, else "Terminal N".
pub fn title(app: &App, thread: &str, id: &str) -> String {
    if let Some(session) = app.ui.panel.terminal.get(thread, id) {
        return session.title();
    }
    known(app, thread).into_iter().find(|(k, ..)| k == id).map(|(_, label, _)| label).filter(|l| !l.is_empty()).unwrap_or_else(|| label_for(id))
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
    ui.gone.push((thread.to_string(), id.to_string()));
    if let Some(net) = app.src.net() {
        if let Some(payload) = session.payload {
            net.unwatch_resource("terminal.attach", payload);
        }
        let request = net.request(
            "terminal.close",
            json!({"threadId":thread,"terminalId":id,"deleteHistory":true}),
        );
        ui.pending.push((request, thread.to_string(), id.to_string()));
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
    let data = match app.ui.panel.terminal.get_mut(thread, id) {
        Some(session) => {
            session.vt.scroll_to_bottom();
            session.wake_cursor();
            session.hold(data)
        }
        None => Some(data),
    };
    let (Some(data), Some(net)) = (data, app.src.net()) else { return };
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
        app.ui.panel.terminal.pending.push((request, thread.to_string(), id.to_string()));
        rest = &rest[end..];
    }
}

fn resize(app: &mut App, thread: &str, id: &str, cols: usize, rows: usize) {
    let Some(session) = app.ui.panel.terminal.get_mut(thread, id) else {
        return;
    };
    if session.fit == Some((cols, rows)) {
        return;
    }
    session.fit = Some((cols, rows));
    // Recorded sessions keep the grid they were captured at.
    if session.payload.is_none() {
        return;
    }
    // A narrower grid reflows now; the shell only hears settled sizes so a drag doesn't reprint
    // its prompt.
    session.apply_grid();
    session.schedule_notify();
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
    let mut changed = poll_links(app);
    if let Some(net) = app.src.net() {
        net.watch_resource("subscribeTerminalMetadata", json!({}));
        let events = net.take_resource_events("subscribeTerminalMetadata", json!({}));
        let ui = &mut app.ui.panel.terminal;
        for event in &events {
            fold_known(&mut ui.known, event);
            changed = true;
        }
        let known = &ui.known;
        ui.gone.retain(|(t, i)| known.iter().any(|k| data::s(k, "threadId") == t && data::s(k, "terminalId") == i));
    }
    if let Some(p) = app.ui.panel.terminal.popup.as_ref() {
        let now = Instant::now();
        if now >= p.due {
            let p = app.ui.panel.terminal.popup.take().unwrap();
            if app.ui.panel.terminal.get(&p.thread, &p.id).is_some_and(|s| s.vt.selection_text().is_some()) {
                app.ui.panel.terminal.menu = Some(Menu { thread: p.thread, id: p.id, at: p.at, selection: true });
                changed = true;
            }
        } else {
            tx.wake_after(p.due - now);
        }
    }
    let pending = std::mem::take(&mut app.ui.panel.terminal.pending);
    for (request, thread, id) in pending {
        match app.src.net().and_then(|net| net.take_reply(request)) {
            None => app.ui.panel.terminal.pending.push((request, thread, id)),
            Some(result) => {
                let closing = app
                    .ui
                    .panel
                    .terminal
                    .closing
                    .iter()
                    .position(|(r, ..)| *r == request);
                if let Some(index) = closing {
                    app.ui.panel.terminal.closing.remove(index);
                    if result.is_err() {
                        fallback_exit(app, &thread, &id);
                    }
                } else if let Err(error) = result
                    && let Some(session) = app.ui.panel.terminal.get_mut(&thread, &id)
                {
                    let message = data::s(&error, "message");
                    system_line(session, if message.is_empty() { "Terminal request failed" } else { message });
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
                if let Some((cols, rows)) = session.fit {
                    notify.push((session.thread.clone(), session.id.clone(), cols, rows));
                }
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
            app.ui.panel.terminal.pending.push((request, thread, id));
        }
    }
    for (thread, id, data) in writes {
        send(app, &thread, &id, data);
    }
    for (thread, id) in ended {
        super::drawer::session_ended(app, &thread, &id);
        changed = true;
    }
    changed
}

/// Path checks coming back; a path pressed while it was asked opens once it exists.
fn poll_links(app: &mut App) -> bool {
    let mut changed = false;
    for (request, path) in std::mem::take(&mut app.ui.panel.terminal.path_requests) {
        let Some(reply) = app.src.net().and_then(|net| net.take_reply(request)) else {
            app.ui.panel.terminal.path_requests.push((request, path));
            continue;
        };
        let exists = reply.is_ok_and(|v| matches!(v["entries"][0]["kind"].as_str(), Some("file" | "directory")));
        let ui = &mut app.ui.panel.terminal;
        ui.paths.insert(path.clone(), Some(exists));
        changed = true;
        if ui.open_when_known.as_ref().is_some_and(|(p, _)| *p == path)
            && let Some((_, target)) = ui.open_when_known.take()
            && exists
        {
            open_path(app, target);
        }
    }
    changed
}

fn fallback_exit(app: &mut App, thread: &str, id: &str) {
    if let Some(net) = app.src.net() {
        let request = net.request(
            "terminal.write",
            json!({"threadId":thread,"terminalId":id,"data":"exit\n"}),
        );
        app.ui.panel.terminal.pending.push((request, thread.to_string(), id.to_string()));
    }
}

fn apply(session: &mut Session, event: &Value) {
    if !event["error"].is_null() {
        fail(session, data::s(&event["error"], "message"));
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
            if let (Some(cols), Some(rows)) = (snapshot["cols"].as_u64(), snapshot["rows"].as_u64()) {
                session.grid = Some((cols as usize, rows as usize));
            }
            // History replays at the grid it will be drawn at, as web's surface does.
            let (cols, rows) = session.drawn().unwrap_or(session.vt.size());
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
            // A new attach hears the fit again (`resendSize`).
            session.schedule_notify();
            note_exit(session);
        }
        "activity" => {
            session.running = event["hasRunningSubprocess"] == true;
            if let Some(label) = event["label"].as_str() {
                session.label = label.into();
            }
            if let (Some(cols), Some(rows)) = (event["cols"].as_u64(), event["rows"].as_u64()) {
                session.grid = Some((cols as usize, rows as usize));
                session.apply_grid();
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
        "error" => fail(session, data::s(event, "message")),
        _ => {}
    }
    if sequence.is_some() {
        session.sequence = sequence;
    }
}

/// An attach or process error, written once into the terminal as web does.
fn fail(session: &mut Session, message: &str) {
    if session.status != message {
        session.status = message.into();
        system_line(session, message);
    }
}

fn system_line(session: &mut Session, message: &str) {
    session.vt.feed(&format!("\r\n\x1b[90m[terminal] {message}\x1b[0m\r\n"));
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
    system_line(session, message);
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

/// Shortcuts the web terminal turns into shell input itself: macOS Cmd+K clears (a form feed,
/// as Ctrl+L), Option or Alt+arrows move by word, and on macOS Cmd+arrows go to the line's ends
/// and Cmd+Backspace deletes to its start (`isTerminalClearShortcut`, `terminalNavigationShortcutData`).
fn shortcut_input(e: &KeyEvent, mac: bool) -> Option<&'static str> {
    let m = e.mods;
    if m.shift || m.ctrl {
        return None;
    }
    let meta = if mac { m.cmd } else { m.logo };
    let letter = match &e.key {
        Key::Command(c) => Some(c.to_ascii_lowercase()),
        Key::Text(t) => t.chars().next().map(|c| c.to_ascii_lowercase()),
        _ => None,
    };
    match (&e.key, letter) {
        (_, Some('k')) if mac && meta && !m.alt => Some("\x0c"),
        (Key::Left, _) if m.alt && !meta => Some("\x1bb"),
        (Key::Right, _) if m.alt && !meta => Some("\x1bf"),
        (Key::Left, _) if mac && meta && !m.alt => Some("\x01"),
        (Key::Right, _) if mac && meta && !m.alt => Some("\x05"),
        (Key::Backspace, _) if mac && meta && !m.alt => Some("\x15"),
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
    if crate::shell::quit::held_close(e, tx.key_repeat()) || super::drawer::shortcut(app, thread, id, e) {
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
    if let Some(data) = shortcut_input(e, cfg!(target_os = "macos")) {
        send(app, thread, id, data.to_string());
        return true;
    }
    let Some(session) = app.ui.panel.terminal.get_mut(thread, id) else {
        return false;
    };
    if session.vt.history_key(e, cfg!(target_os = "macos")) {
        return true;
    }
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
    let size = font_size(&app.client_settings.values);
    let cell_h = (size * 1.35).round().max(1.);
    let family = crate::settings::fonts::terminal_family(&app.client_settings.values);
    let font = rune::TextStyle::new(size, 400.).family(family).lh(cell_h);
    let cell_w = rune::text::advance("MMMMMMMMMM", font) / 10.;
    Metrics {
        font,
        cell_w: if cell_w > 0. { cell_w } else { size * 0.6 },
        cell_h,
    }
}

fn in_preview(settings: &serde_json::Map<String, Value>, url: &str, force_browser: bool) -> bool {
    let web = url.starts_with("http://") || url.starts_with("https://");
    web && !force_browser && settings.get("browserLinkTarget").and_then(Value::as_str) == Some("app")
}

/// The code font size, or the terminal's own under advanced typography
/// (`resolveTerminalFontSizePreference`), in units of the root's interface scale.
fn font_size(settings: &serde_json::Map<String, Value>) -> f32 {
    crate::settings::fonts::terminal_size(settings)
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

/// `resolvePathLinkTarget`: a path link's absolute path, relative ones joined to the terminal's
/// directory and `~/` to the home it sits in, then its `:line:col` again for the editor.
fn link_target(text: &str, cwd: &str) -> (String, String) {
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
    let absolute = if joined.starts_with('/') { format!("/{}", parts.join("/")) } else { joined.clone() };
    let target = format!("{absolute}{}", &text[path.len()..]);
    (absolute, target)
}

/// Whether a path link's file or folder exists, asking the server the first time it shows; a
/// URL always opens. Only absolute paths up to 512 bytes are checked (`useFileMetadata`).
/// Whether `link` is known to exist, else the absolute path still to ask about.
fn known_link(app: &App, link: &Link) -> Result<Option<bool>, String> {
    if link.kind == links::Kind::Url {
        return Ok(Some(true));
    }
    let Some((_, cwd)) = super::context(app) else { return Ok(None) };
    let (absolute, _) = link_target(&link.text, &cwd);
    if !absolute.starts_with('/') || absolute.len() > 512 {
        return Ok(Some(false));
    }
    app.ui.panel.terminal.paths.get(&absolute).copied().ok_or(absolute)
}

fn link_exists(app: &mut App, link: &Link) -> Option<bool> {
    let absolute = match known_link(app, link) {
        Ok(known) => return known,
        Err(absolute) => absolute,
    };
    let net = app.src.net()?;
    let request = net.request("filesystem.getMetadata", json!({"paths": [absolute]}));
    let ui = &mut app.ui.panel.terminal;
    ui.paths.insert(absolute.clone(), None);
    ui.path_requests.push((request, absolute));
    None
}

/// Opens a clicked link: URLs in the preview when links open in the app (Cmd or Ctrl still
/// picks the system browser), paths at their line in the preferred editor (`handleLinkActivate`,
/// `openTerminalLinkInPreview`, `openInPreferredEditor`).
fn open_link(app: &mut App, tx: &mut Tx, link: &Link, force_browser: bool) {
    if link.kind == links::Kind::Url {
        return open_web(app, tx, &link.text, force_browser);
    }
    let Some((_, cwd)) = super::context(app) else {
        return;
    };
    let (absolute, target) = link_target(&link.text, &cwd);
    match link_exists(app, link) {
        Some(true) => open_path(app, target),
        Some(false) => {}
        None => app.ui.panel.terminal.open_when_known = Some((absolute, target)),
    }
}

/// A web link in the preview when Settings send links there, else the system browser.
pub(super) fn open_web(app: &mut App, tx: &mut Tx, url: &str, force_browser: bool) {
    if in_preview(&app.client_settings.values, url, force_browser) && super::context(app).is_some() {
        super::open_preview(app, url);
    } else if let Err(e) = tx.open_url(url) {
        crate::shell::palette::toast(app, "Unable to open link", Some(&e.to_string()));
    }
}

fn open_path(app: &mut App, target: String) {
    let config = app.state().config.as_ref();
    let editor = config
        .and_then(|c| c["availableEditors"].as_array())
        .and_then(|v| v.first())
        .map(|e| e.as_str().map_or_else(|| data::s(e, "id").to_string(), str::to_string));
    let Some(editor) = editor else {
        return crate::shell::palette::toast_error(app, "Editor opening is unavailable.", None);
    };
    if let Some(net) = app.src.net() {
        net.request("shell.openInEditor", json!({"cwd": target, "editor": editor}));
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
    // A path underlines only once it is known to exist, as the web checks it on hover.
    let hover = session.hover.as_ref().filter(|link| known_link(app, link) == Ok(Some(true)));
    if let Some(link) = hover {
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
    let pointer = if hover.is_some() {
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
        .pointer_buttons([rune::PointerButton::Middle])
        .on_drag(move |s: &mut App, d, tx| drag(s, tx, &t5, &i5, d))
        .on_pointer_move_local(move |s: &mut App, p, _| {
            let (col, row) = cell_at(s, &t6, &i6, p.x, p.y);
            let Some(session) = s.ui.panel.terminal.get_mut(&t6, &i6) else {
                return;
            };
            let at = p.window();
            session.origin = (at.x - p.x, at.y - p.y);
            if session.button.is_some() || session.press.is_some() {
                return;
            }
            let report = session
                .vt
                .mouse(MouseButton::None, false, true, col, row, rune::Mods::default());
            let hover = if session.vt.reports_mouse(false) { None } else { session.vt.link_at(col, row) };
            if let Some(link) = &hover {
                link_exists(s, link);
            }
            let Some(session) = s.ui.panel.terminal.get_mut(&t6, &i6) else {
                return;
            };
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
            s.ui.panel.terminal.menu = Some(Menu { thread: t8.clone(), id: i8.clone(), at: (at.x, at.y), selection: false });
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
        let ui = &mut s.ui.panel.terminal;
        ui.popup = None;
        ui.menu = None;
    }
    if tx.button() == rune::PointerButton::Middle {
        return middle(s, tx, thread, id, d);
    }
    // A path not known to exist is plain text; one still being checked opens if it does.
    let link = if d.phase == Phase::Down {
        let (col, row) = cell_at(s, thread, id, d.x, d.y);
        let link = s.ui.panel.terminal.get(thread, id).and_then(|session| session.vt.link_at(col, row));
        link.filter(|link| link_exists(s, link) != Some(false))
    } else {
        None
    };
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
            } else if let (1, false, Some(link)) = (d.clicks, mods.shift, link) {
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
            Some(Press::Link(link)) if !d.cancelled() => open = Some((link, mods.ctrl || mods.cmd)),
            Some(Press::Select { by: SelectBy::Cell, moved: false }) => session.vt.clear_selection(),
            Some(Press::Select { .. }) if !d.cancelled() && session.vt.selection_text().is_some() => {
                let at = (session.origin.0 + d.x, session.origin.1 + d.y);
                select_menu(s, tx, thread, id, at, d.clicks >= 2);
            }
            _ => {}
        },
    }
    if let Some(data) = report {
        send(s, thread, id, data);
    }
    if let Some((link, force_browser)) = open {
        open_link(s, tx, &link, force_browser);
    }
}

/// A mouse selection offers Add to chat and Copy where it was released (`selectionActions.ts`);
/// after a double or triple click it waits, so a further click can extend it first.
fn select_menu(s: &mut App, tx: &mut Tx, thread: &str, id: &str, at: (f32, f32), multi: bool) {
    let ui = &mut s.ui.panel.terminal;
    if multi {
        ui.popup = Some(Popup { thread: thread.to_string(), id: id.to_string(), at, due: Instant::now() + MULTI_CLICK_MENU });
        tx.wake_after(MULTI_CLICK_MENU);
    } else {
        ui.menu = Some(Menu { thread: thread.to_string(), id: id.to_string(), at, selection: true });
    }
}

/// The middle button: reported to a program reading the mouse, else on X11-style desktops it
/// pastes the terminal's own selection, the only primary-selection buffer it can read.
fn middle(s: &mut App, tx: &mut Tx, thread: &str, id: &str, d: &mut rune::Drag) {
    let (col, row) = cell_at(s, thread, id, d.x, d.y);
    let mods = tx.mods();
    let Some(session) = s.ui.panel.terminal.get_mut(thread, id) else {
        return;
    };
    let data = if session.vt.reports_mouse(mods.shift || mods.ctrl || mods.cmd) {
        match d.phase {
            Phase::Down => session.vt.mouse(MouseButton::Middle, true, false, col, row, mods),
            Phase::Move => session.vt.mouse(MouseButton::Middle, true, true, col, row, mods),
            Phase::Up => session.vt.mouse(MouseButton::Middle, false, false, col, row, mods),
        }
    } else if MIDDLE_PASTE && d.phase == Phase::Down {
        session.vt.selection_text().map(|text| session.vt.paste(&text))
    } else {
        None
    };
    if let Some(data) = data {
        send(s, thread, id, data);
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
    ];
    let entries = if menu.selection {
        entries
    } else {
        entries.into_iter().chain(right_click_entries(item)).collect()
    };
    kit::Menu::new("terminal-menu")
        .native(true)
        .width(198.)
        .entries(entries)
        .open(true)
        .at(menu.at.0, menu.at.1)
        .on_close(|s: &mut App| s.ui.panel.terminal.menu = None)
        .overlay(&kit::theme(t), out);
}

fn right_click_entries(item: impl Fn(&str, fn(&mut App, &mut Tx, &str, &str)) -> kit::MenuItem) -> Vec<kit::MenuEntry> {
    vec![
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
    ]
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
    fn links_and_fonts_follow_the_settings() {
        let settings = |v: Value| v.as_object().cloned().unwrap();
        let app = settings(json!({"browserLinkTarget": "app"}));
        assert!(in_preview(&app, "https://example.com", false));
        assert!(!in_preview(&app, "https://example.com", true));
        assert!(!in_preview(&app, "file:///tmp/a", false));
        assert!(!in_preview(&settings(json!({"browserLinkTarget": "system"})), "https://example.com", false));
        assert_eq!(font_size(&settings(json!({}))), 13.);
        assert_eq!(font_size(&settings(json!({"fontSizeCode": 15, "fontSizeTerminal": 10}))), 15.);
        assert_eq!(font_size(&settings(json!({"typographyAdvanced": true, "fontSizeCode": 15, "fontSizeTerminal": 10}))), 10.);
    }

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
        assert!(rt.state.ui.panel.terminal.menu.as_ref().is_some_and(|m| m.selection), "a mouse selection offers Add to chat and Copy");
        rt.pointer_down(x(8.), y);
        rt.pointer_up(x(8.), y);
        rt.frame(0.);
        assert_eq!(selection(&rt), None, "a click clears the selection");
        assert!(rt.state.ui.panel.terminal.menu.is_none());
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
            &json!({"type":"activity","sequence":13,"cols":100,"rows":30,"label":"vim","hasRunningSubprocess":true}),
        );
        assert_eq!(session.vt.size(), (100, 30));
        assert_eq!((session.title().as_str(), session.running), ("vim", true), "activity renames the tab");
        // A wider view waits for the PTY's grid; a narrower one reflows at once (`applyGrid`).
        session.fit = Some((120, 40));
        session.apply_grid();
        assert_eq!(session.vt.size(), (100, 30));
        session.fit = Some((90, 20));
        session.apply_grid();
        assert_eq!(session.vt.size(), (90, 20));
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
        // An attach error is written once, however often the stream repeats it.
        let failed = json!({"error":{"message":"Terminal attach failed"}});
        apply(&mut session, &failed);
        apply(&mut session, &failed);
        assert!(session.vt.text().ends_with("[terminal] Terminal attach failed"));
        assert_eq!(session.vt.text().matches("Terminal attach failed").count(), 1);
    }

    #[test]
    fn input_before_the_attach_waits_for_it() {
        let mut session = Session::new("t", "term-1");
        assert_eq!(session.hold("ls".into()).as_deref(), Some("ls"), "a recorded session sends at once");
        session.payload = Some(json!({"threadId":"t","terminalId":"term-1","cwd":"/p"}));
        assert_eq!(session.hold("l".into()), None);
        assert_eq!(session.hold("s\r".into()), None);
        apply(&mut session, &json!({"type":"snapshot","snapshot":{"history":"$ ","sequence":1}}));
        assert_eq!(session.queued.take().as_deref(), Some("ls\r"), "the first keystroke is kept");
        assert_eq!(session.hold("x".into()).as_deref(), Some("x"));
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
    fn attach_opens_a_closed_id_again_with_the_project_env() {
        let payload = attach_payload("t", "term-2", &Launch { cwd: "/wt", worktree: Some("/wt"), root: Some("/repo") });
        assert!(payload.get("resume").is_none(), "{payload}");
        assert_eq!(payload["worktreePath"], "/wt");
        assert_eq!(payload["env"], json!({"T3CODE_PROJECT_ROOT": "/repo", "T3CODE_WORKTREE_PATH": "/wt"}));
        let local = attach_payload("t", "term-2", &Launch { cwd: "/repo", worktree: None, root: Some("/repo") });
        assert!(local.get("worktreePath").is_none());
        assert_eq!(local["env"], json!({"T3CODE_PROJECT_ROOT": "/repo"}));
    }

    #[test]
    fn server_terminals_fold_from_the_metadata_stream() {
        let summary = |thread: &str, id: &str, label: &str, running: bool| json!({"threadId": thread, "terminalId": id, "label": label, "hasRunningSubprocess": running});
        let mut known = vec![];
        fold_known(&mut known, &json!({"type": "snapshot", "terminals": [summary("t", "term-10", "", false), summary("t", "term-2", "vim", true), summary("u", "term-1", "", false)]}));
        fold_known(&mut known, &json!({"type": "upsert", "terminal": summary("t", "term-10", "npm run dev", true)}));
        fold_known(&mut known, &json!({"type": "upsert", "terminal": summary("t", "term-3", "", false)}));
        fold_known(&mut known, &json!({"type": "remove", "threadId": "u", "terminalId": "term-1"}));
        let mut app = App::new();
        app.ui.panel.terminal.known = known;
        let ids: Vec<String> = super::known(&app, "t").into_iter().map(|(id, ..)| id).collect();
        assert_eq!(ids, ["term-2", "term-3", "term-10"]);
        assert_eq!(running(&app, "t"), 2);
        assert!(super::known(&app, "u").is_empty());
        assert_eq!(title(&app, "t", "term-10"), "npm run dev");
        assert_eq!(title(&app, "t", "term-3"), "Terminal 3");
        app.ui.panel.terminal.gone.push(("t".into(), "term-2".into()));
        assert_eq!(super::known(&app, "t").len(), 2, "a terminal closed here waits for the server's remove");
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
    fn path_links_resolve_to_absolute_editor_targets() {
        let cwd = "/home/me/repo";
        let target = |text: &str| link_target(text, cwd);
        assert_eq!(target("src/main.rs:12:3"), ("/home/me/repo/src/main.rs".into(), "/home/me/repo/src/main.rs:12:3".into()));
        assert_eq!(target("./a/../b.ts").0, "/home/me/repo/b.ts");
        assert_eq!(target("~/notes.md:4").1, "/home/me/notes.md:4");
        assert_eq!(target("../other/z").0, "/home/me/other/z");
        assert_eq!(target("/etc/hosts").0, "/etc/hosts");
    }

    #[test]
    fn shell_shortcuts_follow_the_web_terminal() {
        let key = |k: Key, f: fn(&mut rune::Mods)| {
            let mut e = KeyEvent::from(k);
            f(&mut e.mods);
            e
        };
        let mac_cmd = |m: &mut rune::Mods| (m.cmd, m.logo) = (true, true);
        assert_eq!(shortcut_input(&key(Key::Command('k'), mac_cmd), true), Some("\x0c"));
        assert_eq!(shortcut_input(&key(Key::Command('k'), |m| (m.cmd, m.ctrl) = (true, true)), false), None, "Ctrl+K off macOS is the shell's");
        assert_eq!(shortcut_input(&key(Key::Left, |m| m.alt = true), true), Some("\x1bb"));
        assert_eq!(shortcut_input(&key(Key::Right, |m| m.alt = true), false), Some("\x1bf"));
        assert_eq!(shortcut_input(&key(Key::Left, mac_cmd), true), Some("\x01"));
        assert_eq!(shortcut_input(&key(Key::Right, mac_cmd), true), Some("\x05"));
        assert_eq!(shortcut_input(&key(Key::Backspace, mac_cmd), true), Some("\x15"));
        assert_eq!(shortcut_input(&key(Key::Left, |m| (m.alt, m.shift) = (true, true)), true), None, "Shift keeps the encoder's sequence");
        assert_eq!(shortcut_input(&key(Key::Left, |m| (m.logo, m.cmd) = (true, false)), false), None, "Super+arrow is the desktop's");
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
