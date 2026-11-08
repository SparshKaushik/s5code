//! Terminal sessions shared by the drawer and the right panel, keyed by thread and terminal id,
//! and the viewport that draws one. Wire payloads match the web's `terminal.*` RPCs.

use std::time::{Duration, Instant};

use super::vt::{self, MouseButton, Palette, Vt, Wheel};
use crate::{
    App, data,
    theme::{self, T},
};
use rune::{Color, El, KeyEvent, Phase, TextEdit, Tx, el, text, text_input};
use serde_json::{Value, json};

/// The web terminal's blink period; it blinks only while focused.
const BLINK: Duration = Duration::from_millis(500);
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
    cursor_on: bool,
    button: Option<MouseButton>,
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
            cursor_on: true,
            button: None,
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
        self.cursor_on = true;
    }
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

#[derive(Default)]
pub struct Ui {
    sessions: Vec<Session>,
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
        let on = if blinking {
            let elapsed = session.blink_origin.elapsed();
            let phase = elapsed.as_millis() / BLINK.as_millis();
            let remaining = BLINK.as_millis() - elapsed.as_millis() % BLINK.as_millis();
            tx.wake_after(Duration::from_millis(remaining as u64));
            phase % 2 == 0
        } else {
            true
        };
        if on != session.cursor_on {
            session.cursor_on = on;
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

fn key(app: &mut App, thread: &str, id: &str, e: &KeyEvent) -> bool {
    if super::drawer::shortcut(app, thread, id, e) {
        return true;
    }
    let Some(session) = app.ui.panel.terminal.get(thread, id) else {
        return false;
    };
    match session.vt.key(e) {
        Some(data) => {
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
    let m = metrics(app);
    let (cols, rows) = app
        .ui
        .panel
        .terminal
        .get(thread, id)
        .map_or((80, 24), |s| s.vt.size());
    (
        (((x - PADDING) / m.cell_w).floor().max(0.) as usize).min(cols - 1),
        (((y - PADDING) / m.cell_h).floor().max(0.) as usize).min(rows - 1),
    )
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
        .role("textbox")
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
    let focused = session.input.focused;
    if let Some(c) = cursor.filter(|_| session.cursor_on) {
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
        grid = grid.child(mark);
        if let (true, vt::CursorKind::Block, Some(glyph)) = (focused, c.kind, &c.glyph) {
            let mut font = m.font;
            if glyph.bold {
                font = theme::weight(font, 700.);
            }
            grid = grid.child(
                text(glyph.text.clone())
                    .font(font)
                    .color(t.c.terminal_background)
                    .absolute(cx, cy),
            );
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
        .on_input_key(move |s: &mut App, e, _| key(s, &t2, &i2, e))
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
        .on_drag(move |s: &mut App, d, _| {
            if d.phase == Phase::Down {
                focus(s, &t5, &i5);
                super::drawer::activate(s, &t5, &i5);
            }
            let (col, row) = cell_at(s, &t5, &i5, d.x, d.y);
            let Some(session) = s.ui.panel.terminal.get_mut(&t5, &i5) else {
                return;
            };
            let mods = rune::Mods::default();
            if !session.vt.reports_mouse(false) {
                return;
            }
            let report = match d.phase {
                Phase::Down => {
                    session.button = Some(MouseButton::Left);
                    session.vt.mouse(MouseButton::Left, true, false, col, row, mods)
                }
                Phase::Move => session.vt.mouse(MouseButton::Left, true, true, col, row, mods),
                Phase::Up => {
                    session.button = None;
                    session.vt.mouse(MouseButton::Left, false, false, col, row, mods)
                }
            };
            if let Some(data) = report {
                send(s, &t5, &i5, data);
            }
        })
        .on_pointer_move_local(move |s: &mut App, p, _| {
            let (col, row) = cell_at(s, &t6, &i6, p.x, p.y);
            let Some(session) = s.ui.panel.terminal.get(&t6, &i6) else {
                return;
            };
            if session.button.is_some() {
                return;
            }
            if let Some(data) = session
                .vt
                .mouse(MouseButton::None, false, true, col, row, rune::Mods::default())
            {
                send(s, &t6, &i6, data);
            }
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
    fn terminal_ids_and_labels_follow_the_web() {
        let ids = ["term-1".to_string(), "term-3".to_string()];
        assert_eq!(next_id(&ids), "term-2");
        assert_eq!(next_id(&[]), "term-1");
        assert_eq!(label_for("term-12"), "Terminal 12");
        assert_eq!(label_for("custom"), "custom");
    }
}
