//! S5 Code's desktop client, prototyped on Rune: the sidebar v2, the draft home page and chat,
//! the right panel's browser preview and the settings dialog. Turns run through `opencode serve`.

mod browser;
mod chat;
mod host;
mod icons;
mod opencode;
mod panel;
mod settings;
mod sidebar;
mod theme;
mod ui;

use std::cell::Cell;
use std::time::{SystemTime, UNIX_EPOCH};

use rune::{App as RuneApp, AppSpec, Cx, El, Key, KeyEvent, TextEdit, Tx, Waker, row};

use crate::browser::Browser;
use crate::opencode::{Client, Event, Message, Part};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Appearance {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    Draft(usize),
    Thread(u64),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Idle,
    Working(u64),
    Done,
    Failed,
}

pub struct Project {
    pub name: String,
    pub dir: String,
    pub color: rune::Color,
}

pub struct Thread {
    pub id: u64,
    pub project: usize,
    pub session: Option<String>,
    pub title: String,
    pub updated_ms: u64,
    pub status: Status,
    pub messages: Vec<Message>,
    pub loaded: bool,
    pub branch: Option<String>,
    pub diff: Option<(u32, u32)>,
}

/// A thread row on its way somewhere: where it started and where the pointer has it now.
#[derive(Clone)]
pub struct DragRow {
    pub id: u64,
    pub from_pinned: bool,
    pub press_top: f32,
    pub top: f32,
    pub started: bool,
    pub before: (Vec<u64>, Vec<u64>),
}

pub struct Prefs {
    pub thread_notifications: String,
    pub in_app_notifications: bool,
    pub project_grouping: bool,
    pub project_order: String,
    pub working_section: bool,
    pub unpin_confirm: bool,
    pub archive_confirm: bool,
    pub delete_confirm: bool,
    pub quit_shortcut: String,
    pub time_format: String,
    pub reduce_motion: bool,
}

pub struct App {
    pub appearance: Appearance,
    pub projects: Vec<Project>,
    pub threads: Vec<Thread>,
    pub pinned: Vec<u64>,
    pub active: Vec<u64>,
    pub settled: Vec<u64>,
    pub route: Route,
    pub search: TextEdit,
    pub composer: TextEdit,
    pub drag: Option<DragRow>,
    pub working_open: bool,
    pub settled_open: bool,
    pub settings: Option<usize>,
    pub settings_search: TextEdit,
    pub prefs: Prefs,
    pub open_select: Option<&'static str>,
    pub panel_open: bool,
    pub browser: Browser,
    pub page: browser::Status,
    pub url: TextEdit,
    pub page_focused: bool,
    pub browsing: bool,
    pub surface: Cell<(u32, u32)>,
    pub oc: Client,
    pub connected: Option<bool>,
    pub model: String,
    pub next_id: u64,
    pub now_ms: u64,
}

pub const MODELS: &[(&str, &str)] = &[
    ("big-pickle", "Big Pickle"),
    ("glm-5.3-flash-free", "GLM-5.3 Flash"),
    ("deepseek-v4.1-flash-free", "DeepSeek V4.1 Flash"),
    ("nemotron-3-ultra-free", "Nemotron 3 Ultra"),
    ("mimo-v2.6-flash-free", "MiMo V2.6 Flash"),
];

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn projects() -> Vec<Project> {
    let colors = ["#3b82f6", "#f97316", "#10b981", "#a855f7", "#ec4899"];
    host::projects(&host::opencode_url())
        .into_iter()
        .enumerate()
        .map(|(i, (name, dir))| Project { name, dir, color: rune::color::hex(colors[i % colors.len()]).unwrap_or(rune::Color::WHITE) })
        .collect()
}

impl App {
    fn new() -> App {
        App {
            appearance: Appearance::Dark,
            projects: projects(),
            threads: Vec::new(),
            pinned: Vec::new(),
            active: Vec::new(),
            settled: Vec::new(),
            route: Route::Draft(0),
            search: TextEdit::new(""),
            composer: TextEdit::new(""),
            drag: None,
            working_open: true,
            settled_open: false,
            settings: None,
            settings_search: TextEdit::new(""),
            prefs: Prefs {
                thread_notifications: "Notifications with sound".into(),
                in_app_notifications: true,
                project_grouping: true,
                project_order: "Recent activity".into(),
                working_section: true,
                unpin_confirm: false,
                archive_confirm: true,
                delete_confirm: true,
                quit_shortcut: "Hold to quit".into(),
                time_format: "System default".into(),
                reduce_motion: false,
            },
            open_select: None,
            panel_open: false,
            browser: Browser::default(),
            page: browser::Status::default(),
            url: TextEdit::new(""),
            page_focused: false,
            browsing: false,
            surface: Cell::new((0, 0)),
            oc: Client::new(host::opencode_url()),
            connected: None,
            model: MODELS[0].0.into(),
            next_id: 1,
            now_ms: now_ms(),
        }
    }

    pub fn thread(&self, id: u64) -> Option<&Thread> {
        self.threads.iter().find(|t| t.id == id)
    }

    pub fn thread_mut(&mut self, id: u64) -> Option<&mut Thread> {
        self.threads.iter_mut().find(|t| t.id == id)
    }

    pub fn current_project(&self) -> usize {
        match self.route {
            Route::Draft(p) => p,
            Route::Thread(id) => self.thread(id).map_or(0, |t| t.project),
        }
    }

    pub fn is_working(&self, id: u64) -> bool {
        self.thread(id).is_some_and(|t| matches!(t.status, Status::Working(_)))
    }

    /// Active rows the list shows: working threads fold into the Working shelf when it's on.
    pub fn visible_active(&self) -> Vec<u64> {
        self.active.iter().copied().filter(|&id| !(self.prefs.working_section && self.is_working(id))).collect()
    }

    pub fn working(&self) -> Vec<u64> {
        if !self.prefs.working_section {
            return Vec::new();
        }
        self.active.iter().copied().filter(|&id| self.is_working(id)).collect()
    }

    pub fn open_thread(&mut self, id: u64) {
        self.route = Route::Thread(id);
        self.page_focused = false;
        let Some(t) = self.thread_mut(id) else { return };
        if t.status == Status::Done {
            t.status = Status::Idle;
        }
        if !t.loaded
            && let Some(session) = t.session.clone()
        {
            t.loaded = true;
            let project = t.project;
            let dir = self.projects[project].dir.clone();
            self.oc.load(id, session, dir);
        }
    }

    pub fn new_draft(&mut self, project: usize) {
        self.route = Route::Draft(project);
        self.composer = TextEdit::new("");
        self.composer.focused = true;
    }

    pub fn send(&mut self, tx: &mut Tx) {
        let text = self.composer.text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let (id, project, session) = match self.route {
            Route::Thread(id) => {
                let Some(t) = self.thread(id) else { return };
                if matches!(t.status, Status::Working(_)) {
                    return;
                }
                (id, t.project, t.session.clone())
            }
            Route::Draft(project) => {
                let id = self.next_id;
                self.next_id += 1;
                let title: String = text.chars().take(48).collect();
                self.threads.push(Thread {
                    id,
                    project,
                    session: None,
                    title,
                    updated_ms: now_ms(),
                    status: Status::Idle,
                    messages: Vec::new(),
                    loaded: true,
                    branch: Some("main".into()),
                    diff: None,
                });
                self.active.insert(0, id);
                self.route = Route::Thread(id);
                (id, project, None)
            }
        };
        let now = now_ms();
        if let Some(t) = self.thread_mut(id) {
            t.status = Status::Working(now);
            t.updated_ms = now;
            t.messages.push(Message { id: format!("local-{now}"), user: true, parts: vec![Part::Text(text.clone())], done: true, error: None });
        }
        if let Some(i) = self.active.iter().position(|&a| a == id) {
            let a = self.active.remove(i);
            self.active.insert(0, a);
        }
        self.composer = TextEdit::new("");
        self.composer.focused = true;
        let dir = self.projects[project].dir.clone();
        self.oc.prompt(id, session, dir, ("opencode".into(), self.model.clone()), text);
        tx.scroll_to_bottom("transcript");
    }

    fn apply(&mut self, e: Event) {
        match e {
            Event::Connected(ok) => self.connected = Some(ok),
            Event::Sessions { project, sessions } => {
                for (n, s) in sessions.into_iter().enumerate() {
                    if self.threads.iter().any(|t| t.session.as_deref() == Some(&s.id)) {
                        continue;
                    }
                    let id = self.next_id;
                    self.next_id += 1;
                    let seed = id as u32;
                    self.threads.push(Thread {
                        id,
                        project,
                        session: Some(s.id),
                        title: s.title,
                        updated_ms: s.updated_ms,
                        status: Status::Idle,
                        messages: Vec::new(),
                        loaded: false,
                        branch: Some(["main", "feat/sidebar-v2", "fix/oauth-redirect", "t3/rune-port"][seed as usize % 4].into()),
                        diff: (seed % 3 != 0).then_some((seed * 37 % 240 + 4, seed * 13 % 90)),
                    });
                    if n < 2 { self.active.push(id) } else { self.settled.push(id) }
                }
                let recency: std::collections::HashMap<u64, u64> = self.threads.iter().map(|t| (t.id, t.updated_ms)).collect();
                self.active.sort_by_key(|id| std::cmp::Reverse(recency.get(id).copied().unwrap_or(0)));
                // The demo opens with one thread pinned, as a returning user would have.
                if self.pinned.is_empty() && self.active.len() > 3 {
                    let id = self.active.remove(self.active.len() - 1);
                    self.pinned.push(id);
                }
            }
            Event::Created { thread, session } => {
                if let Some(t) = self.thread_mut(thread) {
                    t.session = Some(session);
                }
            }
            Event::Messages { thread, messages, busy } => {
                let selected = self.route == Route::Thread(thread);
                if let Some(t) = self.thread_mut(thread) {
                    // Keep the optimistic prompt until the server has the user message.
                    if !messages.is_empty() {
                        t.messages = messages;
                    }
                    t.updated_ms = now_ms();
                    if !busy && matches!(t.status, Status::Working(_)) {
                        let failed = t.messages.last().is_some_and(|m| m.error.is_some());
                        t.status = if failed { Status::Failed } else if selected { Status::Idle } else { Status::Done };
                    }
                }
            }
            Event::Failed { thread, error } => {
                if let Some(t) = self.thread_mut(thread) {
                    t.status = Status::Failed;
                    t.messages.push(Message { id: format!("err-{}", now_ms()), user: false, parts: Vec::new(), done: true, error: Some(error) });
                }
            }
        }
    }
}

fn setup(s: &mut App, waker: Waker) {
    s.oc.set_waker(waker.clone());
    s.oc.ping();
    for (i, p) in s.projects.iter().enumerate() {
        s.oc.list_sessions(i, p.dir.clone());
    }
    s.browser.start(waker);
}

fn poll(s: &mut App, _: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    for e in s.oc.drain() {
        s.apply(e);
        changed = true;
    }
    let page = s.browser.status.lock().unwrap().clone();
    if page != s.page {
        if page.url != s.page.url && !s.url.focused {
            s.url = TextEdit::new(display_url(&page.url));
        }
        s.page = page;
        changed = true;
    }
    // Working rows show a live duration, so tick once a second while any thread works.
    if s.threads.iter().any(|t| matches!(t.status, Status::Working(_))) {
        tx.wake_after(std::time::Duration::from_millis(1000));
        let now = now_ms();
        if now / 1000 != s.now_ms / 1000 {
            s.now_ms = now;
            changed = true;
        }
    } else if changed {
        s.now_ms = now_ms();
    }
    changed
}

pub fn display_url(u: &str) -> String {
    if u == "about:blank" { String::new() } else { u.to_string() }
}

fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    match &e.key {
        Key::Command(',') => {
            s.settings = Some(0);
            true
        }
        Key::Command('n') => {
            s.new_draft(s.current_project());
            true
        }
        Key::Command('b') => {
            s.panel_open = !s.panel_open;
            true
        }
        Key::Escape if s.settings.is_some() => {
            s.settings = None;
            true
        }
        _ if s.page_focused && s.panel_open && tx.focused().is_none() => panel::forward_key(s, e),
        _ => false,
    }
}

pub fn is_dark(s: &App, cx: &Cx<App>) -> bool {
    match s.appearance {
        Appearance::System => cx.system_dark(),
        Appearance::Light => false,
        Appearance::Dark => true,
    }
}

fn view(s: &App, cx: &mut Cx<App>) -> El {
    let dark = is_dark(s, cx);
    cx.reduced_motion(s.prefs.reduce_motion);
    let p = theme::palette(dark);
    let k = theme::kit(dark);
    let mut layer = Vec::new();
    let mut body = row().key("shell").w_full().h_full().fill(p.bg).child(sidebar::view(s, p, k)).child(chat::view(s, cx, p, k, &mut layer));
    if s.panel_open {
        body = body.child(panel::view(s, cx, p, k));
    }
    if let Some(section) = s.settings {
        layer.extend(settings::view(s, section, p, k));
    }
    rune_kit::root(body, layer)
}

fn scene_thread(s: &mut App) {
    s.panel_open = false;
}

fn main() {
    let spec = AppSpec {
        min_size: Some((960.0, 600.0)),
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        scenes: &[("thread", scene_thread), ("settings", |s| s.settings = Some(0)), ("browser", |s| s.panel_open = true)],
        ..AppSpec::new("S5 Code", (1440.0, 900.0), App::new, view)
    };
    let server = host::ensure_opencode(&host::opencode_url());
    RuneApp::new(spec).run();
    browser::shutdown();
    if let Some(mut child) = server {
        let _ = child.kill();
    }
}
