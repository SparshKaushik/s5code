//! The per-thread terminal drawer under the chat (web `ThreadTerminalDrawer`), and the same
//! layout as the right panel's Terminal surface: groups of up to four split panes, a sidebar
//! once there are two terminals, and a confirm before a terminal is closed.

use std::collections::HashMap;

use super::terminal;
use crate::{
    App, Route, icons, kit,
    theme::{self, AlphaMul, T},
};
use rune::{Color, Cx, El, KeyEvent, Phase, col, el, icon, row, text};
use serde_json::{Map, Value, json};

pub const DEFAULT_HEIGHT: f32 = 280.;
const MIN_HEIGHT: f32 = 180.;
pub const MAX_PER_GROUP: usize = 4;

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub id: String,
    pub ids: Vec<String>,
    pub vertical: bool,
    active: String,
}

/// One owner's terminals (`ThreadTerminalUiState`'s groups), in the web store's semantics.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Groups {
    pub groups: Vec<Group>,
    pub active: String,
}

impl Groups {
    pub fn ids(&self) -> impl Iterator<Item = &String> {
        self.groups.iter().flat_map(|g| &g.ids)
    }
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
    fn active_group(&self) -> Option<usize> {
        self.groups.iter().position(|g| g.ids.contains(&self.active))
    }
    /// The group on screen: the active terminal's.
    pub fn visible(&self) -> Option<&Group> {
        self.active_group().map(|i| &self.groups[i])
    }
    pub fn split_full(&self) -> bool {
        self.visible().is_some_and(|g| g.ids.len() >= MAX_PER_GROUP)
    }
    pub fn add(&mut self, id: &str) {
        let mut group = format!("group-{id}");
        while self.groups.iter().any(|g| g.id == group) {
            group.push('-');
        }
        self.groups.push(Group {
            id: group,
            ids: vec![id.into()],
            vertical: false,
            active: id.into(),
        });
        self.active = id.into();
    }
    /// Inserts after the active terminal in its group; refused once the group holds four.
    pub fn split(&mut self, id: &str, vertical: bool) -> bool {
        let Some(index) = self.active_group() else {
            self.add(id);
            return true;
        };
        let group = &mut self.groups[index];
        if group.ids.len() >= MAX_PER_GROUP {
            return false;
        }
        let anchor = group.ids.iter().position(|v| *v == self.active);
        group.ids.insert(anchor.map_or(group.ids.len(), |a| a + 1), id.into());
        group.vertical = vertical;
        group.active = id.into();
        self.active = id.into();
        true
    }
    pub fn activate(&mut self, id: &str) -> bool {
        let Some(group) = self.groups.iter_mut().find(|g| g.ids.iter().any(|v| v == id)) else { return false };
        group.active = id.into();
        self.active = id.into();
        true
    }
    /// The next active terminal is the group neighbour after, else before, else the nearest.
    pub fn remove(&mut self, id: &str) {
        let all: Vec<String> = self.ids().cloned().collect();
        let Some(closed) = all.iter().position(|v| v == id) else {
            return;
        };
        let remaining: Vec<&String> = all.iter().filter(|v| *v != id).collect();
        if self.active == id {
            let group = self.groups.iter().find(|g| g.ids.iter().any(|v| v == id));
            let next = group.and_then(|g| {
                let at = g.ids.iter().position(|v| v == id)?;
                g.ids.get(at + 1).or(at.checked_sub(1).and_then(|b| g.ids.get(b)))
            });
            self.active = next
                .or(remaining
                    .get(closed.min(remaining.len().saturating_sub(1)))
                    .copied())
                .cloned()
                .unwrap_or_default();
        }
        for group in &mut self.groups {
            group.ids.retain(|v| v != id);
            if group.active == id {
                group.active = group.ids.iter().find(|v| **v == self.active).or_else(|| group.ids.first()).cloned().unwrap_or_default();
            }
        }
        self.groups.retain(|g| !g.ids.is_empty());
    }
}

impl Groups {
    fn to_json(&self) -> Value {
        let groups: Vec<Value> = self
            .groups
            .iter()
            .map(|g| {
                let mut v = json!({"id": g.id, "terminalIds": g.ids, "activeTerminalId": g.active});
                if g.vertical {
                    v["splitDirection"] = json!("vertical");
                }
                v
            })
            .collect();
        json!({"terminalGroups": groups, "activeTerminalId": self.active})
    }
    /// `normalizeTerminalGroups`: an id lives in one group, groups hold one to four, and the
    /// active id falls back to the first.
    fn from_json(v: &Value) -> Groups {
        let mut out = Groups::default();
        let mut seen = std::collections::HashSet::new();
        for g in v["terminalGroups"].as_array().into_iter().flatten() {
            let ids: Vec<String> = g["terminalIds"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty() && seen.insert(id.to_string()))
                .take(MAX_PER_GROUP)
                .map(String::from)
                .collect();
            let Some(first) = ids.first() else { continue };
            let mut id = g["id"].as_str().map(str::trim).filter(|v| !v.is_empty()).map(String::from).unwrap_or_else(|| format!("group-{first}"));
            while out.groups.iter().any(|g| g.id == id) {
                id.push('-');
            }
            let active = g["activeTerminalId"].as_str().filter(|a| ids.iter().any(|id| id == *a)).unwrap_or(first).to_owned();
            out.groups.push(Group { id, ids, active, vertical: g["splitDirection"] == "vertical" });
        }
        let active = v["activeTerminalId"].as_str().unwrap_or_default();
        if !out.activate(active) {
            let first = out.ids().next().cloned().unwrap_or_default();
            out.active = first;
        }
        out
    }
}

pub struct Thread {
    pub open: bool,
    pub height: f32,
    pub groups: Groups,
}
impl Default for Thread {
    fn default() -> Self {
        Self {
            open: false,
            height: DEFAULT_HEIGHT,
            groups: Groups::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Confirm {
    One(Owner, String, String),
    /// Closing the right panel's Terminal surface stops all of its terminals.
    Panel(String),
    Groups { thread: String, groups: Vec<String> },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owner {
    Drawer,
    Panel,
}

#[derive(Default)]
pub struct Ui {
    pub threads: HashMap<String, Thread>,
    /// The right panel's Terminal surface, per thread.
    pub panel: HashMap<String, Groups>,
    /// The terminals waiting on the close confirm.
    confirm: Option<Confirm>,
    /// A new-thread draft's thread id, by draft key, so its terminals carry into the thread.
    drafts: HashMap<String, String>,
    /// What `terminal-state.json` holds, once read (the web's `t3code:terminal-state:v1`).
    saved: Option<Value>,
    /// A height drag in progress: saved when it ends, not on every frame.
    resizing: bool,
}

const STATE_FILE: &str = "terminal-state.json";

impl Ui {
    fn to_json(&self) -> Value {
        let threads: Map<String, Value> = self
            .threads
            .iter()
            .filter(|(_, t)| !t.groups.is_empty())
            .map(|(id, t)| {
                let mut v = t.groups.to_json();
                v["terminalOpen"] = json!(t.open);
                v["terminalHeight"] = json!(t.height);
                (id.clone(), v)
            })
            .collect();
        let panel: Map<String, Value> =
            self.panel.iter().filter(|(_, g)| !g.is_empty()).map(|(id, g)| (id.clone(), g.to_json())).collect();
        json!({"version": 1, "terminalUiStateByThreadKey": threads, "panelTerminalsByThreadKey": panel})
    }
    fn restore(&mut self, v: &Value) {
        for (id, t) in v["terminalUiStateByThreadKey"].as_object().into_iter().flatten() {
            let groups = Groups::from_json(t);
            if !groups.is_empty() {
                let height = t["terminalHeight"].as_f64().map_or(DEFAULT_HEIGHT, |h| h as f32).max(MIN_HEIGHT);
                self.threads.insert(id.clone(), Thread { open: t["terminalOpen"] == true, height, groups });
            }
        }
        for (id, g) in v["panelTerminalsByThreadKey"].as_object().into_iter().flatten() {
            let groups = Groups::from_json(g);
            if !groups.is_empty() {
                self.panel.insert(id.clone(), groups);
            }
        }
    }
}

/// Reads the saved layouts once, then writes them whenever they change (a drag, once it ends).
pub(super) fn persist(app: &mut App) {
    if cfg!(test) {
        return;
    }
    let path = app.client_settings.sibling(STATE_FILE);
    let ui = &mut app.ui.panel.drawer;
    if ui.saved.is_none() {
        if let Some(v) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
            ui.restore(&v);
        }
        ui.saved = Some(ui.to_json());
        return;
    }
    if ui.resizing {
        return;
    }
    let next = ui.to_json();
    if ui.saved.as_ref() == Some(&next) {
        return;
    }
    if super::write_state(&path, &next) {
        ui.saved = Some(next);
    }
}

/// A restored layout has no sessions yet: an open drawer attaches its terminals again.
fn reattach(app: &mut App) {
    if app.src.net().is_none() {
        return;
    }
    let Some(thread) = current_thread(app) else {
        return;
    };
    let Some(state) = app.ui.panel.drawer.threads.get(&thread).filter(|t| t.open) else {
        return;
    };
    let missing: Vec<String> =
        state.groups.ids().filter(|id| app.ui.panel.terminal.get(&thread, id).is_none()).cloned().collect();
    for id in missing {
        start(app, &thread, &id, false);
    }
}

impl Ui {
    fn groups(&self, owner: Owner, thread: &str) -> Option<&Groups> {
        match owner {
            Owner::Drawer => self.threads.get(thread).map(|t| &t.groups),
            Owner::Panel => self.panel.get(thread),
        }
    }
    fn groups_mut(&mut self, owner: Owner, thread: &str) -> &mut Groups {
        match owner {
            Owner::Drawer => &mut self.threads.entry(thread.into()).or_default().groups,
            Owner::Panel => self.panel.entry(thread.into()).or_default(),
        }
    }
    pub(super) fn owner_of(&self, thread: &str, id: &str) -> Option<Owner> {
        [Owner::Drawer, Owner::Panel]
            .into_iter()
            .find(|&o| self.groups(o, thread).is_some_and(|g| g.ids().any(|v| v == id)))
    }
    pub fn is_open(&self, thread: &str) -> bool {
        self.threads.get(thread).is_some_and(|t| t.open)
    }
}

/// The current thread (or draft), its terminal cwd, its worktree and its project's root.
fn launch_context(app: &App) -> Option<(String, String, Option<String>, Option<String>)> {
    let thread = current_thread(app)?;
    let cwd = super::workspace(app)?;
    let worktree = match app.state().threads.iter().find(|v| v["id"] == thread.as_str()) {
        Some(v) => v["worktreePath"].as_str().filter(|v| !v.is_empty()).map(String::from),
        // A draft runs in the worktree it picked ("New thread on {branch}", "Previous worktree").
        None => crate::chat::details::draft_worktree(app).map(|(path, _)| path),
    };
    Some((thread, cwd, worktree, project_root(app)))
}

fn project_root(app: &App) -> Option<String> {
    let project = match &app.route {
        Route::Thread(id) => {
            let thread = app.state().threads.iter().find(|v| v["id"] == id.as_str())?;
            app.state().projects.iter().find(|p| p["id"] == thread["projectId"])?
        }
        Route::Home { .. } => crate::chat::home::draft_project(app)?,
        _ => return None,
    };
    Some(crate::data::s(project, "workspaceRoot").to_string()).filter(|r| !r.is_empty())
}

/// The current thread's active terminal for `owner`, which `workspace.focusNext` focuses.
pub(super) fn active_terminal(app: &App, owner: Owner) -> Option<(String, String)> {
    let thread = current_thread(app)?;
    let ui = &app.ui.panel.drawer;
    if owner == Owner::Drawer && !ui.is_open(&thread) {
        return None;
    }
    let active = ui.groups(owner, &thread)?.active.clone();
    (!active.is_empty()).then_some((thread, active))
}

/// The thread on screen, or the new-thread draft's once it has a terminal.
fn current_thread(app: &App) -> Option<String> {
    match &app.route {
        Route::Thread(id) => Some(id.clone()),
        Route::Home { .. } => app.ui.panel.drawer.drafts.get(&crate::chat::composer::draft_key(app, &app.route)).cloned(),
        _ => None,
    }
}

pub(super) fn composer_key(app: &App, thread: &str) -> String {
    app.ui.panel.drawer.drafts.iter()
        .find(|(_, id)| id.as_str() == thread)
        .map_or_else(|| thread.to_owned(), |(key, _)| key.clone())
}

pub(super) fn can_operate(app: &App) -> bool {
    crate::chat::details::grants(app, "terminal:operate")
}

/// The current thread; a draft gets the thread id it will be created with (web's draft terminals).
fn current_or_draft(app: &mut App) -> Option<String> {
    if let Some(thread) = current_thread(app) {
        return Some(thread);
    }
    if !matches!(app.route, Route::Home { .. }) || super::workspace(app).is_none() {
        return None;
    }
    let project = crate::data::s(crate::chat::home::draft_project(app)?, "id").to_string();
    Some(draft_thread(app, &project))
}

/// The thread id the project's draft will be created with, made on first need, so its
/// terminals and a pull request worktree prepared for it carry into the thread.
pub fn draft_thread(app: &mut App, project: &str) -> String {
    let key = crate::chat::composer::home_key(project);
    app.ui.panel.drawer.drafts.entry(key).or_insert_with(|| format!("thread:project:{project}:{}", s5_net::uuid())).clone()
}

/// The id a draft's terminals run under becomes the new thread's, once.
pub fn take_draft_thread(app: &mut App, project: &str) -> Option<String> {
    app.ui.panel.drawer.drafts.remove(&crate::chat::composer::home_key(project))
}

/// Every id the thread's drawer and panel hold, so a new one never shares a PTY.
fn allocate(app: &App, thread: &str) -> String {
    let ui = &app.ui.panel.drawer;
    let ids: Vec<String> = [Owner::Drawer, Owner::Panel]
        .into_iter()
        .filter_map(|o| ui.groups(o, thread))
        .flat_map(|g| g.ids().cloned())
        .chain(terminal::known(app, thread).into_iter().map(|(id, _, _)| id))
        .collect();
    terminal::next_id(&ids)
}

fn start(app: &mut App, thread: &str, id: &str, focus: bool) {
    if let Some((_, cwd, worktree, root)) = launch_context(app).filter(|(t, ..)| t == thread) {
        terminal::ensure(app, thread, id, &terminal::Launch { cwd: &cwd, worktree: worktree.as_deref(), root: root.as_deref(), size: None });
    }
    if focus {
        terminal::focus(app, thread, id);
    }
}

/// `terminal.toggle`: opening an empty drawer prefers an existing server terminal.
pub fn toggle(app: &mut App) {
    let Some(thread) = current_or_draft(app) else {
        return;
    };
    if !app.ui.panel.drawer.is_open(&thread)
        && !crate::chat::details::grants(app, "terminal:read")
        && !can_operate(app)
    {
        return;
    }
    if app.ui.panel.drawer.is_open(&thread) {
        app.ui.panel.drawer.threads.get_mut(&thread).unwrap().open = false;
    } else {
        reconcile(app);
        let operate = can_operate(app);
        let default = allocate(app, &thread);
        let state = app.ui.panel.drawer.threads.entry(thread.clone()).or_default();
        state.open = true;
        if state.groups.is_empty() && operate {
            state.groups.add(&default);
        }
        let active = state.groups.active.clone();
        if !active.is_empty() {
            start(app, &thread, &active, true);
        }
    }
    sync_topbar(app);
}

pub fn sync_topbar(app: &mut App) {
    app.ui.chat.topbar.terminal_open =
        current_thread(app).is_some_and(|t| app.ui.panel.drawer.is_open(&t));
}

/// Opens the drawer on a terminal the server started, such as a setup script's
/// (`storeEnsureTerminal` with open and active, then focus).
pub fn show(app: &mut App, id: &str) {
    let Some(thread) = current_thread(app) else {
        return;
    };
    let drawer = &mut app.ui.panel.drawer;
    let owner = drawer.owner_of(&thread, id);
    if owner != Some(Owner::Panel) {
        let state = drawer.threads.entry(thread.clone()).or_default();
        state.open = true;
        if owner.is_none() {
            state.groups.add(id);
        }
    }
    activate(app, &thread, id);
    start(app, &thread, id, true);
    sync_topbar(app);
}

pub fn new_terminal(app: &mut App, owner: Owner) {
    if !can_operate(app) {
        return;
    }
    let Some(thread) = current_or_draft(app) else {
        return;
    };
    let id = allocate(app, &thread);
    app.ui.panel.drawer.groups_mut(owner, &thread).add(&id);
    if owner == Owner::Drawer {
        app.ui.panel.drawer.threads.get_mut(&thread).unwrap().open = true;
    }
    start(app, &thread, &id, true);
    if owner == Owner::Panel {
        super::sync_terminal_tabs(app);
        if let Some(group) = app.ui.panel.drawer.panel.get(&thread).and_then(Groups::visible).map(|g| g.id.clone()) {
            super::tabs::open(app, super::tabs::Target::Terminal(group));
        }
    }
    sync_topbar(app);
}

/// Types `input` into an idle drawer terminal, the active one first, else a new one
/// (`findReusableProjectScriptTerminal`); panel-owned terminals are never reused.
/// A new script terminal's size until the drawer measures it (`SCRIPT_TERMINAL_COLS`/`ROWS`).
const SCRIPT_TERMINAL_SIZE: (u16, u16) = (120, 30);

/// `findReusableProjectScriptTerminal`: the active drawer terminal when idle, else the first idle
/// one; right-panel terminals and ones running a process are never reused.
fn reusable_script_terminal(drawer: &[String], active: &str, panel: &[String], running: &[String]) -> Option<String> {
    let idle: Vec<&String> = drawer.iter().filter(|id| !panel.contains(id) && !running.contains(id)).collect();
    idle.iter().find(|id| id.as_str() == active).or(idle.first()).map(|id| id.to_string())
}

pub fn run(app: &mut App, cwd: &str, worktree: Option<&str>, input: String) {
    if !can_operate(app) {
        return;
    }
    let Some(thread) = current_or_draft(app) else {
        return;
    };
    let ui = &app.ui.panel;
    let panel: Vec<String> = ui.drawer.panel.get(&thread).map(|g| g.ids().cloned().collect()).unwrap_or_default();
    let known = terminal::known(app, &thread);
    let mut drawer: Vec<String> = ui.drawer.threads.get(&thread).map(|t| t.groups.ids().cloned().collect()).unwrap_or_default();
    // Server sessions cover drawer terminals this client has not opened yet, such as a worktree setup shell.
    for (id, ..) in &known {
        if !drawer.contains(id) {
            drawer.push(id.clone());
        }
    }
    let running: Vec<String> = known.iter().filter(|k| k.2).map(|k| k.0.clone()).chain(drawer.iter().filter(|id| ui.terminal.get(&thread, id).is_some_and(|s| s.running)).cloned()).collect();
    let active = ui.drawer.threads.get(&thread).map(|t| t.groups.active.clone()).unwrap_or_default();
    // Reading back a reused terminal's scrollback needs terminal:read.
    let reusable = crate::chat::details::grants(app, "terminal:read").then(|| reusable_script_terminal(&drawer, &active, &panel, &running)).flatten();
    let size = reusable.is_none().then_some(SCRIPT_TERMINAL_SIZE);
    let id = reusable.unwrap_or_else(|| allocate(app, &thread));
    let state = app.ui.panel.drawer.threads.entry(thread.clone()).or_default();
    state.open = true;
    if !state.groups.ids().any(|v| *v == id) {
        state.groups.add(&id);
    }
    state.groups.activate(&id);
    let root = project_root(app);
    terminal::run(app, &thread, &id, &terminal::Launch { cwd, worktree, root: root.as_deref(), size }, input);
    terminal::focus(app, &thread, &id);
    sync_topbar(app);
}

pub fn split(app: &mut App, owner: Owner, vertical: bool) {
    if !can_operate(app) {
        return;
    }
    let Some(thread) = current_or_draft(app) else {
        return;
    };
    let id = allocate(app, &thread);
    if !app
        .ui
        .panel
        .drawer
        .groups_mut(owner, &thread)
        .split(&id, vertical)
    {
        return;
    }
    if owner == Owner::Drawer {
        app.ui.panel.drawer.threads.get_mut(&thread).unwrap().open = true;
    }
    start(app, &thread, &id, true);
    sync_topbar(app);
}

/// The right panel's Terminal surface shows its own terminals, started on first open.
pub fn open_panel(app: &mut App) {
    let Some(thread) = current_or_draft(app) else {
        return;
    };
    let empty = app
        .ui
        .panel
        .drawer
        .groups(Owner::Panel, &thread)
        .is_none_or(Groups::is_empty);
    if empty {
        if !can_operate(app) {
            return;
        }
        let id = allocate(app, &thread);
        app.ui.panel.drawer.groups_mut(Owner::Panel, &thread).add(&id);
    }
    let ids: Vec<String> = app.ui.panel.drawer.panel[&thread].ids().cloned().collect();
    for id in &ids {
        start(app, &thread, id, false);
    }
}

pub(super) fn panel_groups(app: &App) -> Option<&Groups> {
    app.ui.panel.drawer.panel.get(&current_thread(app)?)
}

pub(super) fn group_title(app: &App, group: &str) -> String {
    let Some(thread) = current_thread(app) else { return "Terminal".into() };
    let Some(groups) = app.ui.panel.drawer.panel.get(&thread) else { return "Terminal".into() };
    groups.groups.iter().find(|g| g.id == group).and_then(|g| g.ids.first())
        .map_or_else(|| "Terminal".into(), |id| terminal::title(app, &thread, id))
}

pub(super) fn activate_group(app: &mut App, group: &str) {
    let Some(thread) = current_thread(app) else { return };
    let Some(groups) = app.ui.panel.drawer.panel.get_mut(&thread) else { return };
    let Some(g) = groups.groups.iter().find(|g| g.id == group) else { return };
    let id = g.active.clone();
    groups.activate(&id);
    open_panel(app);
    terminal::focus(app, &thread, &id);
}

pub(super) fn request_close_groups(app: &mut App, groups: Vec<String>) {
    if !can_operate(app) {
        return;
    }
    if !groups.is_empty() && let Some(thread) = current_thread(app) {
        app.ui.panel.drawer.confirm = Some(Confirm::Groups { thread, groups });
    }
}

fn group_ids(ui: &Ui, thread: &str, selected: &[String]) -> Vec<String> {
    ui.panel.get(thread).into_iter().flat_map(|layout| &layout.groups)
        .filter(|group| selected.contains(&group.id))
        .flat_map(|group| group.ids.iter().cloned())
        .collect()
}

/// An activated Terminal surface takes the keyboard (web's `focusTerminal`).
pub fn focus_panel(app: &mut App) {
    let Some(thread) = current_thread(app) else {
        return;
    };
    let Some(groups) = app.ui.panel.drawer.groups(Owner::Panel, &thread) else {
        return;
    };
    let active = Some(&groups.active).filter(|a| groups.ids().any(|id| id == *a)).or_else(|| groups.ids().next()).cloned();
    if let Some(id) = active {
        terminal::focus(app, &thread, &id);
    }
}

/// Closing the panel's Terminal surface asks first, as it stops every terminal in it.
pub fn request_close_panel(app: &mut App) {
    if !can_operate(app) {
        return;
    }
    match current_thread(app).filter(|t| app.ui.panel.drawer.panel.get(t).is_some_and(|g| !g.is_empty())) {
        Some(thread) => app.ui.panel.drawer.confirm = Some(Confirm::Panel(thread)),
        None => {
            close_panel(app);
            super::remove_surface(app, super::Surface::Terminal);
        }
    }
}

/// Closing the panel's Terminal surface stops its terminals.
fn close_panel(app: &mut App) {
    let Some(thread) = current_thread(app) else {
        return;
    };
    if let Some(groups) = app.ui.panel.drawer.panel.remove(&thread) {
        for id in groups.ids() {
            terminal::close(app, &thread, id);
        }
    }
}

pub fn activate(app: &mut App, thread: &str, id: &str) {
    if let Some(owner) = app.ui.panel.drawer.owner_of(thread, id) {
        app.ui.panel.drawer.groups_mut(owner, thread).activate(id);
    }
}

fn select(app: &mut App, owner: Owner, id: &str) {
    let Some(thread) = current_thread(app) else {
        return;
    };
    app.ui.panel.drawer.groups_mut(owner, &thread).activate(id);
    terminal::focus(app, &thread, id);
}

pub fn request_close(app: &mut App, owner: Owner, id: &str) {
    if !can_operate(app) {
        return;
    }
    if let Some(thread) = current_thread(app) {
        app.ui.panel.drawer.confirm = Some(Confirm::One(owner, thread, id.into()));
    }
}

fn close_now(app: &mut App, owner: Owner, thread: &str, id: &str) {
    terminal::close(app, thread, id);
    let groups = app.ui.panel.drawer.groups_mut(owner, thread);
    groups.remove(id);
    let next = groups.active.clone();
    let empty = groups.is_empty();
    match owner {
        // Closing the last terminal resets the drawer to its default, closed state.
        Owner::Drawer if empty => {
            app.ui.panel.drawer.threads.remove(thread);
        }
        Owner::Panel if empty => {
            app.ui.panel.drawer.panel.remove(thread);
            super::remove_surface(app, super::Surface::Terminal);
        }
        _ => terminal::focus(app, thread, &next),
    }
    sync_topbar(app);
}

/// The process ended: the web prints its exit line, then closes the terminal.
pub fn session_ended(app: &mut App, thread: &str, id: &str) {
    if let Some(owner) = app.ui.panel.drawer.owner_of(thread, id) {
        close_now(app, owner, thread, id);
    }
}

/// Chords a focused terminal handles before the program sees the key (`when: terminalFocus`).
pub fn shortcut(app: &mut App, thread: &str, id: &str, e: &KeyEvent) -> bool {
    // The details card's menus don't take focus, so Escape must close them from here.
    if e.key == rune::Key::Escape && app.ui.chat.details.menu.take().is_some() {
        return true;
    }
    let owner = app
        .ui
        .panel
        .drawer
        .owner_of(thread, id)
        .unwrap_or(Owner::Drawer);
    let command = [
        "terminal.toggle",
        "terminal.split",
        "terminal.splitVertical",
        "terminal.new",
        "terminal.close",
        "rightPanel.toggle",
    ]
    .into_iter()
    .find(|c| super::shortcut(app, e, c));
    if !can_operate(app)
        && matches!(command, Some("terminal.split" | "terminal.splitVertical" | "terminal.new" | "terminal.close"))
    {
        return false;
    }
    match command {
        Some("terminal.toggle") => toggle(app),
        Some("terminal.split") => split(app, owner, false),
        Some("terminal.splitVertical") => split(app, owner, true),
        Some("terminal.new") => new_terminal(app, owner),
        Some("terminal.close") => request_close(app, owner, id),
        Some(_) => super::toggle_panel(app),
        None => return false,
    }
    true
}

/// Keeps the topbar toggle in step with the drawer of the thread on screen.
pub fn poll(app: &mut App) -> bool {
    persist(app);
    let reconciled = reconcile(app);
    reattach(app);
    let before = app.ui.chat.topbar.terminal_open;
    sync_topbar(app);
    reconciled || before != app.ui.chat.topbar.terminal_open
}

/// The drawer follows the server's terminals for the thread (another client's, an agent's),
/// except the panel's own.
fn reconcile(app: &mut App) -> bool {
    let Some(thread) = current_thread(app) else { return false };
    let ui = &app.ui.panel.drawer;
    let panel: Vec<String> = ui.panel.get(&thread).map(|g| g.ids().cloned().collect()).unwrap_or_default();
    let server: Vec<String> = terminal::known(app, &thread).into_iter().map(|(id, ..)| id).filter(|id| !panel.contains(id)).collect();
    if server.is_empty() && !ui.threads.contains_key(&thread) {
        return false;
    }
    let changed = reconcile_ids(&mut app.ui.panel.drawer.threads.entry(thread.clone()).or_default().groups, &server);
    if app.ui.panel.drawer.threads.get(&thread).is_some_and(|t| t.groups.is_empty() && !t.open) {
        app.ui.panel.drawer.threads.remove(&thread);
    }
    changed
}

/// `reconcileThreadTerminalSessionIds`, skipped while the server lags a terminal just opened
/// (its ids a strict subset of ours).
fn reconcile_ids(groups: &mut Groups, server: &[String]) -> bool {
    let client: Vec<String> = groups.ids().cloned().collect();
    let lagging = server.len() < client.len() && server.iter().all(|id| client.contains(id));
    let same = server.len() == client.len() && server.iter().all(|id| client.contains(id));
    if same || lagging {
        return false;
    }
    for id in client.iter().filter(|id| !server.contains(id)) {
        groups.remove(id);
    }
    for id in server.iter().filter(|id| !client.contains(id)) {
        let active = std::mem::take(&mut groups.active);
        groups.add(id);
        groups.active = if active.is_empty() { id.clone() } else { active };
    }
    true
}

fn action(
    t: &T,
    label: &str,
    glyph: &'static rune::Icon,
    disabled: bool,
    edge: bool,
    f: impl Fn(&mut App) + 'static,
) -> El {
    let mut b = row()
        .h_full()
        .px(4.)
        .center_y()
        .role("button")
        .label(label)
        .help(label)
        .focusable(true)
        .disabled(disabled)
        .child(
            icon(glyph, 13.)
                .size(13., 13.)
                .color(t.c.foreground.alpha_mul(0.9)),
        );
    if disabled {
        b = b.opacity(0.64);
    } else {
        b = b
            .hover(if edge { t.c.accent.alpha_mul(0.7) } else { t.c.accent })
            .on_press(f);
    }
    b
}

/// `label (shortcut)` when `command` has a shortcut, as web's terminal action labels.
fn with_shortcut(app: &App, label: &str, command: &str) -> String {
    match crate::shell::palette::shortcut_label(app, command) {
        shortcut if shortcut.is_empty() => label.to_owned(),
        shortcut => format!("{label} ({shortcut})"),
    }
}

fn actions(app: &App, t: &T, owner: Owner, full: bool, sidebar: bool) -> El {
    // Every action needs terminal:operate, as web's `canOperateTerminal`.
    let operate = can_operate(app);
    let split_label = |base: &str, command: &str| {
        if full {
            format!("{base} (max {MAX_PER_GROUP} per group)")
        } else {
            with_shortcut(app, base, command)
        }
    };
    let buttons = [
        action(
            t,
            &split_label("Split Terminal Horizontally", "terminal.split"),
            &icons::SQUARE_SPLIT_HORIZONTAL,
            full || !operate,
            sidebar,
            move |s| split(s, owner, false),
        ),
        action(
            t,
            &split_label("Split Terminal Vertically", "terminal.splitVertical"),
            &icons::SQUARE_SPLIT_VERTICAL,
            full || !operate,
            sidebar,
            move |s| split(s, owner, true),
        ),
        action(t, &with_shortcut(app, "New Terminal", "terminal.new"), &icons::PLUS, !operate, sidebar, move |s| {
            new_terminal(s, owner)
        }),
        action(t, &with_shortcut(app, "Close Terminal", "terminal.close"), &icons::TRASH_2, !operate, sidebar, move |s| {
            if let Some(thread) = current_thread(s)
                && let Some(active) = s
                    .ui
                    .panel
                    .drawer
                    .groups(owner, &thread)
                    .map(|g| g.active.clone())
            {
                request_close(s, owner, &active);
            }
        }),
    ];
    let mut group = row().h_full().center_y();
    for (i, button) in buttons.into_iter().enumerate() {
        if i > 0 {
            group = group.child(if sidebar {
                el().w(1.).h_full().fill(t.c.border.alpha_mul(0.7))
            } else {
                el().w(1.).h(16.).fill(t.c.border.alpha_mul(0.8))
            });
        }
        group = group.child(if sidebar { button } else { button.h(21.) });
    }
    group
}

fn panes(app: &App, t: &T, thread: &str, groups: &Groups) -> El {
    let Some(group) = groups.visible() else {
        return el().grow(1.);
    };
    let mut panes = if group.vertical { col() } else { row() }
        .grow(1.)
        .min_w(0.)
        .min_h(0.)
        .h_full();
    for (i, id) in group.ids.iter().enumerate() {
        let active = *id == groups.active;
        if i > 0 {
            let line = if active {
                t.c.border
            } else {
                t.c.border.alpha_mul(0.7)
            };
            panes = panes.child(if group.vertical {
                el().h(1.).w_full().fill(line)
            } else {
                el().w(1.).h_full().fill(line)
            });
        }
        panes = panes.child(
            col()
                .grow(1.)
                .min_w(0.)
                .min_h(0.)
                .child(terminal::viewport(app, t, thread, id)),
        );
    }
    panes
}

fn sidebar(app: &App, t: &T, owner: Owner, thread: &str, groups: &Groups) -> El {
    let show_headers = groups.groups.len() > 1 || groups.groups.iter().any(|g| g.ids.len() > 1);
    let mut list = col().w_full().px(4.).py(4.);
    for group in &groups.groups {
        let group_active = group.ids.contains(&groups.active);
        let mut block = col().w_full().pb(2.);
        if show_headers {
            let (label, glyph) = match (group.ids.len() > 1, group.vertical) {
                (false, _) => ("Single", &icons::SQUARE),
                (true, true) => ("Stacked", &icons::SQUARE_SPLIT_VERTICAL),
                (true, false) => ("Side by side", &icons::SQUARE_SPLIT_HORIZONTAL),
            };
            let fg = if group_active {
                t.c.foreground
            } else {
                t.c.muted_foreground
            };
            let target = if group_active {
                groups.active.clone()
            } else {
                group.ids[0].clone()
            };
            let mut header = row()
                .w_full()
                .h(22.)
                .px(6.)
                .gap(4.)
                .center_y()
                .radius(4.)
                .role("button")
                .label(label)
                .focusable(true)
                .child(icon(glyph, 12.).size(12., 12.).color(fg))
                .child(text(label).font(theme::TEXT_2XS).color(fg).grow(1.).truncate())
                .child(
                    text(group.ids.len().to_string())
                        .font(theme::TEXT_3XS)
                        .color(t.c.muted_foreground.alpha_mul(0.7)),
                )
                .on_press(move |s: &mut App| select(s, owner, &target));
            header = if group_active {
                header.fill(t.c.accent.alpha_mul(0.5))
            } else {
                header.hover(t.c.accent.alpha_mul(0.4))
            };
            block = block.child(header);
        }
        let mut rows = col().w_full().gap(2.);
        for id in &group.ids {
            let active = *id == groups.active;
            let label = terminal::title(app, thread, id);
            let fg = if active {
                t.c.foreground
            } else {
                t.c.muted_foreground
            };
            let (close_id, select_id) = (id.clone(), id.clone());
            let close_label = if active { with_shortcut(app, &format!("Close {label}"), "terminal.close") } else { format!("Close {label}") };
            let operable = can_operate(app);
            let close = if operable {
                row()
                    .size(16., 16.)
                    .center()
                    .role("button")
                    .label(close_label.clone())
                    .help(close_label)
                    .focusable(true)
                    .children(close_layers().into_iter().map(|layer| match layer {
                        CloseLayer::Backdrop => el().size(16., 16.).pin(Some(0.), Some(0.), None, None).radius(2.).hover(t.c.muted),
                        CloseLayer::Glyph { glyph, rest, hovered } => {
                            let color = if std::ptr::eq(glyph, &icons::X) { t.c.foreground } else { fg };
                            icon(glyph, 12.).size(12., 12.).color(color).group_opacity(rest, hovered).pin(Some(2.), Some(2.), None, None)
                        }
                    }))
                    .on_press(move |s: &mut App| request_close(s, owner, &close_id))
            } else {
                icon(&icons::SQUARE_TERMINAL, 12.).size(12., 12.).color(fg)
            };
            let mut entry = row()
                .w_full()
                .h(24.)
                .gap(2.)
                .pl(6.)
                .pr(8.)
                .center_y()
                .radius(6.)
                .hover_group()
                .child(close)
                .child(
                    row()
                        .grow(1.)
                        .min_w(0.)
                        .gap(4.)
                        .center_y()
                        .role("button")
                        .label(label.clone())
                        .focusable(true)
                        .child(text(label).font(theme::TEXT_XS).color(fg).truncate())
                        .on_press(move |s: &mut App| select(s, owner, &select_id)),
                );
            entry = if active {
                entry.fill(t.c.accent)
            } else {
                entry.hover(t.c.accent.alpha_mul(0.6))
            };
            rows = rows.child(entry);
        }
        list = list.child(block.child(rows));
    }
    col()
        .w(144.)
        .min_w(144.)
        .h_full()
        .border(1., t.c.border.alpha_mul(0.7))
        .fill(t.c.muted.alpha_mul(0.1))
        .child(
            row()
                .h(22.)
                .w_full()
                .justify_end()
                .child(actions(app, t, owner, groups.split_full(), true)),
        )
        .child(el().h(1.).w_full().fill(t.c.border.alpha_mul(0.7)))
        .child(
            col()
                .grow(1.)
                .min_h(0.)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                })
                .child(list),
        )
}

/// A layer of web's PanelTabCloseButton, bottom first: a 16 box whose terminal glyph turns into an
/// X while the row is hovered. Rune hands a group-opacity glyph to its nearest ancestor with a hover
/// fill, so the button's own fill is a sibling backdrop and the glyphs follow the row's hover group.
enum CloseLayer {
    Backdrop,
    Glyph { glyph: &'static rune::Icon, rest: f32, hovered: f32 },
}

fn close_layers() -> [CloseLayer; 3] {
    [
        CloseLayer::Backdrop,
        CloseLayer::Glyph { glyph: &icons::SQUARE_TERMINAL, rest: 1., hovered: 0. },
        CloseLayer::Glyph { glyph: &icons::X, rest: 0., hovered: 1. },
    ]
}

/// The drawer or panel body for one owner's terminals.
pub fn surface(app: &App, t: &T, owner: Owner, width: f32) -> El {
    let thread = current_thread(app).unwrap_or_default();
    let empty = Groups::default();
    let all = app.ui.panel.drawer.groups(owner, &thread).unwrap_or(&empty);
    let scoped;
    let groups = if owner == Owner::Panel && matches!(super::active_target(app), Some(super::tabs::Target::Terminal(_))) {
        scoped = Groups { groups: all.visible().cloned().into_iter().collect(), active: all.active.clone() };
        &scoped
    } else { all };
    if groups.is_empty() {
        return col()
            .grow(1.)
            .min_h(0.)
            .w_full()
            .center()
            .gap(12.)
            .px(16.)
            .py(24.)
            .child(
                text("No terminal sessions for this thread yet.")
                    .font(theme::TEXT_SM)
                    .color(t.c.muted_foreground),
            )
            .child(
                kit::Button::new(with_shortcut(app, "New Terminal", "terminal.new"))
                    .variant("outline")
                    .size("xs")
                    .disabled(!can_operate(app))
                    .on_press(move |s: &mut App| new_terminal(s, owner))
                    .build(t),
            );
    }
    let has_sidebar = groups.ids().count() > 1;
    let mut body = row()
        .grow(1.)
        .min_h(0.)
        .w_full()
        .fill(t.c.terminal_background)
        .child(panes(app, t, &thread, groups));
    if has_sidebar {
        body = body
            .gap(6.)
            .child(sidebar(app, t, owner, &thread, groups));
    }
    let mut out = col().grow(1.).min_h(0.).w_full().child(body);
    if !has_sidebar {
        // 4 buttons of 21 points, 3 dividers and the border.
        let floating_w = 4. * 21. + 3. + 2.;
        out = out.child(
            actions(app, t, owner, groups.split_full(), false)
                .absolute((width - 8. - floating_w).max(0.), 8.)
                .h(23.)
                .radius(6.)
                .clip_rounded(6.)
                .border(1., t.c.border.alpha_mul(0.8))
                .fill(t.c.background)
                .shadow(1., 2., Color::BLACK.alpha_mul(0.05)),
        );
    }
    out
}

/// The drawer, docked under the chat body when open for the thread on screen.
pub fn view(app: &App, cx: &mut Cx<App>, t: &T) -> Option<El> {
    let thread = current_thread(app)?;
    let state = app.ui.panel.drawer.threads.get(&thread).filter(|s| s.open)?;
    let column = cx.layout_with("main", |r| r[3]).unwrap_or(900.);
    let max = (column * 0.75).floor().max(MIN_HEIGHT);
    let height = state.height.clamp(MIN_HEIGHT, max);
    let width = cx.layout_with("terminal-drawer", |r| r[2]).unwrap_or(1000.);
    Some(
        col()
            .key("terminal-drawer")
            .role("complementary")
            .label("Terminal")
            .w_full()
            .h(height)
            .fill(t.c.background)
            .child(el().h(1.).w_full().fill(t.c.border.alpha_mul(0.8)))
            .child(surface(app, t, Owner::Drawer, width))
            .child(
                el().absolute(0., 0.)
                    .w_full()
                    .h(6.)
                    .cursor(rune::Cursor::ResizeRow)
                    .on_drag(move |s: &mut App, d, _| {
                        let Some(state) = current_thread(s)
                            .and_then(|t| s.ui.panel.drawer.threads.get_mut(&t))
                        else {
                            return;
                        };
                        if d.phase == Phase::Down {
                            d.data = height;
                        }
                        if !d.cancelled() {
                            state.height = (d.data - d.dy).clamp(MIN_HEIGHT, max);
                        }
                        s.ui.panel.drawer.resizing = d.phase != Phase::Up && !d.cancelled();
                    }),
            ),
    )
}

pub fn overlays(app: &App, t: &T, out: &mut Vec<El>) {
    let Some(confirm) = app.ui.panel.drawer.confirm.clone() else {
        return;
    };
    let labels: Vec<String> = match &confirm {
        Confirm::One(_, thread, id) => vec![terminal::title(app, thread, id)],
        Confirm::Groups { thread, groups } => group_ids(&app.ui.panel.drawer, thread, groups).into_iter()
            .map(|id| terminal::title(app, thread, &id)).collect(),
        Confirm::Panel(thread) => app.ui.panel.drawer.panel.get(thread).map_or_else(Vec::new, |g| {
            let others = g.ids().filter(|id| **id != g.active).map(|id| terminal::title(app, thread, id));
            std::iter::once(terminal::title(app, thread, &g.active)).chain(others).collect()
        }),
    };
    let (title, description) = close_message(&labels);
    let cancel = kit::Button::new("Cancel")
        .variant("outline")
        .on_press(|s: &mut App| s.ui.panel.drawer.confirm = None)
        .build(t);
    let confirm = kit::Button::new("Confirm")
        .variant("destructive")
        .disabled(!can_operate(app))
        .on_press(|s: &mut App| match s.ui.panel.drawer.confirm.take().filter(|_| can_operate(s)) {
            Some(Confirm::One(owner, thread, id)) => close_now(s, owner, &thread, &id),
            Some(Confirm::Groups { thread, groups }) if current_thread(s).as_deref() == Some(thread.as_str()) => {
                let ids = group_ids(&s.ui.panel.drawer, &thread, &groups);
                for id in ids { close_now(s, Owner::Panel, &thread, &id); }
                super::sync_terminal_tabs(s);
            }
            Some(Confirm::Panel(thread)) if current_thread(s).as_deref() == Some(thread.as_str()) => {
                close_panel(s);
                super::remove_surface(s, super::Surface::Terminal);
            }
            _ => {}
        })
        .build(t);
    kit::Dialog::new("terminal-close", title)
        .alert()
        .width(448.)
        .open(true)
        .description(description)
        .child(row().w_full().gap(8.).justify_end().child(cancel).child(confirm))
        .on_close(|s: &mut App| s.ui.panel.drawer.confirm = None)
        .build(t, out);
}

/// `confirmTerminalClose`'s wording for one terminal or several.
fn close_message(labels: &[String]) -> (String, String) {
    match labels {
        [one] => (format!("Close terminal \"{one}\"?"), "This stops the running process and clears its history.".into()),
        _ => {
            let quoted: Vec<String> = labels.iter().map(|l| format!("\"{l}\"")).collect();
            (format!("Close {} terminals?", labels.len()), format!("This stops their running processes and clears their histories: {}.", quoted.join(", ")))
        }
    }
}

/// Seeds a thread's drawer from recorded snapshots for headless scenes.
pub fn fixture(app: &mut App, owner: Owner, groups: &[(&[(&str, &str)], bool)], active: &str) {
    let Some(thread) = current_thread(app) else {
        return;
    };
    let mut state = Groups::default();
    for (terminals, vertical) in groups {
        let mut ids = vec![];
        for (id, recorded) in terminals.iter() {
            terminal::fixture(app, &thread, id, recorded);
            ids.push(id.to_string());
        }
        state.groups.push(Group {
            id: format!("group-{}", ids[0]),
            active: if ids.iter().any(|id| id == active) { active.to_owned() } else { ids[0].clone() },
            ids,
            vertical: *vertical,
        });
    }
    state.active = active.into();
    match owner {
        Owner::Drawer => {
            let entry = app.ui.panel.drawer.threads.entry(thread).or_default();
            entry.open = true;
            entry.groups = state;
        }
        Owner::Panel => {
            app.ui.panel.drawer.panel.insert(thread, state);
        }
    }
    sync_topbar(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hovered_tab_row_swaps_its_terminal_glyph_for_the_close_x() {
        let shown = |layers: &[CloseLayer], row_hovered: bool| -> Vec<&'static str> {
            layers
                .iter()
                .filter_map(|l| match l {
                    CloseLayer::Glyph { glyph, rest, hovered } => {
                        let opacity = if row_hovered { *hovered } else { *rest };
                        (opacity > 0.).then_some(glyph.name)
                    }
                    CloseLayer::Backdrop => None,
                })
                .collect()
        };
        let layers = close_layers();
        assert_eq!(shown(&layers, false), [icons::SQUARE_TERMINAL.name]);
        assert_eq!(shown(&layers, true), [icons::X.name], "the row's hover reaches the glyphs");
        assert!(matches!(layers[0], CloseLayer::Backdrop));
    }

    #[test]
    fn a_script_reuses_the_idle_active_drawer_terminal_else_the_first_idle_one() {
        let v = |ids: &[&str]| ids.iter().map(|i| i.to_string()).collect::<Vec<_>>();
        let drawer = v(&["term-1", "term-2", "term-3"]);
        assert_eq!(reusable_script_terminal(&drawer, "term-2", &[], &[]).as_deref(), Some("term-2"));
        assert_eq!(reusable_script_terminal(&drawer, "term-2", &[], &v(&["term-2"])).as_deref(), Some("term-1"), "a running terminal is skipped");
        assert_eq!(reusable_script_terminal(&drawer, "term-1", &v(&["term-1"]), &v(&["term-2"])).as_deref(), Some("term-3"), "the right panel's terminal is skipped");
        assert_eq!(reusable_script_terminal(&drawer, "term-1", &v(&["term-1"]), &v(&["term-2", "term-3"])), None);
    }

    #[test]
    fn layouts_survive_a_restart_and_bad_state_is_normalized() {
        let mut ui = Ui::default();
        let thread = ui.threads.entry("t1".into()).or_default();
        thread.open = true;
        thread.height = 333.;
        thread.groups.add("term-1");
        thread.groups.split("term-2", true);
        thread.groups.add("term-3");
        thread.groups.activate("term-2");
        ui.panel.entry("t2".into()).or_default().add("term-1");
        ui.threads.entry("empty".into()).or_default();

        let saved = ui.to_json();
        assert!(saved["terminalUiStateByThreadKey"].get("empty").is_none(), "{saved}");
        let stored = &saved["terminalUiStateByThreadKey"]["t1"]["terminalGroups"];
        assert_eq!(stored[0]["activeTerminalId"], "term-2");
        assert_eq!(stored[1]["activeTerminalId"], "term-3");
        let mut back = Ui::default();
        back.restore(&saved);
        let t1 = &back.threads["t1"];
        assert!(t1.open && t1.height == 333.);
        assert_eq!(t1.groups, ui.threads["t1"].groups);
        assert!(t1.groups.groups[0].vertical);
        assert_eq!(back.panel["t2"], ui.panel["t2"]);

        let mut odd = Ui::default();
        odd.restore(&json!({"terminalUiStateByThreadKey": {"t": {
            "terminalGroups": [
                {"id": "g", "terminalIds": ["a", " a ", "b", "c", "d", "e"]},
                {"id": "g", "terminalIds": ["b", "f"]},
                {"id": "", "terminalIds": []},
            ],
            "activeTerminalId": "gone",
        }}}));
        let t = &odd.threads["t"];
        assert_eq!(ids(&t.groups), [vec!["a", "b", "c", "d"], vec!["f"]]);
        assert_eq!(t.groups.groups[1].id, "g-");
        assert_eq!(t.groups.active, "a");
        assert!(!t.open && t.height == DEFAULT_HEIGHT);
    }

    fn ids(g: &Groups) -> Vec<Vec<&str>> {
        g.groups
            .iter()
            .map(|g| g.ids.iter().map(String::as_str).collect())
            .collect()
    }

    #[test]
    fn split_inserts_after_the_active_terminal_up_to_four() {
        let mut g = Groups::default();
        g.add("term-1");
        assert!(g.split("term-2", false));
        g.activate("term-1");
        assert!(g.split("term-3", true));
        assert_eq!(ids(&g), vec![vec!["term-1", "term-3", "term-2"]]);
        assert!(g.groups[0].vertical);
        assert!(g.split("term-4", false));
        assert!(!g.groups[0].vertical, "a horizontal split resets the direction");
        assert!(g.split_full());
        assert!(!g.split("term-5", false));
        assert_eq!(g.active, "term-4");
    }

    #[test]
    fn new_terminals_get_their_own_group() {
        let mut g = Groups::default();
        g.add("term-1");
        g.add("term-2");
        assert_eq!(ids(&g), vec![vec!["term-1"], vec!["term-2"]]);
        assert_eq!(g.visible().unwrap().ids, vec!["term-2".to_string()]);
    }

    #[test]
    fn closing_moves_focus_to_the_group_neighbour() {
        let mut g = Groups::default();
        g.add("term-1");
        g.split("term-2", false);
        g.split("term-3", false);
        g.add("term-4");
        g.activate("term-2");
        g.remove("term-2");
        assert_eq!(g.active, "term-3", "the neighbour after");
        g.remove("term-3");
        assert_eq!(g.active, "term-1", "else the one before");
        g.activate("term-1");
        g.remove("term-1");
        assert_eq!(g.active, "term-4", "else the nearest remaining");
        assert_eq!(ids(&g), vec![vec!["term-4"]]);
        g.remove("term-4");
        assert!(g.is_empty());
    }

    #[test]
    fn the_drawer_follows_the_server_terminals_unless_it_is_ahead() {
        let s = |ids: &[&str]| ids.iter().map(|v| v.to_string()).collect::<Vec<_>>();
        let mut g = Groups::default();
        g.add("term-1");
        g.split("term-2", true);
        assert!(!reconcile_ids(&mut g, &s(&["term-1"])), "the server hasn't heard of term-2 yet");
        assert!(!reconcile_ids(&mut g, &s(&[])));
        assert!(!reconcile_ids(&mut g, &s(&["term-2", "term-1"])));
        assert!(reconcile_ids(&mut g, &s(&["term-1", "term-2", "term-3"])), "another client opened term-3");
        assert_eq!(ids(&g), vec![vec!["term-1", "term-2"], vec!["term-3"]]);
        assert_eq!(g.active, "term-2", "focus stays put");
        assert!(!reconcile_ids(&mut g, &s(&["term-1", "term-3"])), "a strict subset reads as lag; term-2's own exit closes it");
        assert!(reconcile_ids(&mut g, &s(&["term-1", "term-3", "term-4"])), "term-2 ended elsewhere, term-4 opened");
        assert_eq!(ids(&g), vec![vec!["term-1"], vec!["term-3"], vec!["term-4"]]);
        let mut empty = Groups::default();
        assert!(reconcile_ids(&mut empty, &s(&["term-5"])));
        assert_eq!(empty.active, "term-5");
    }

    #[test]
    fn a_draft_terminal_runs_in_the_drafts_worktree_and_carries_into_its_thread() {
        let mut app = App::new();
        let p = app.state().projects.iter().next().cloned().unwrap();
        let (project, root) = (crate::data::s(&p, "id").to_string(), crate::data::s(&p, "workspaceRoot").to_string());
        app.navigate(Route::Home { project_id: Some(project.clone()) });
        assert_eq!(current_thread(&app), None, "no id until a terminal needs one");
        let thread = current_or_draft(&mut app).unwrap();
        assert_eq!(composer_key(&app, &thread), crate::chat::composer::home_key(&project));
        assert_eq!(composer_key(&app, "other-thread"), "other-thread");
        assert!(thread.starts_with(&format!("thread:project:{project}:")), "{thread}");
        assert_eq!(launch_context(&app), Some((thread.clone(), root.clone(), None, Some(root.clone()))));
        app.ui.chat.home.workspace = Some((project.clone(), json!({"type": "existing_worktree", "worktreePath": "/w/b", "branch": "feat/b"})));
        assert_eq!(launch_context(&app), Some((thread.clone(), "/w/b".into(), Some("/w/b".into()), Some(root))), "the picked worktree");
        assert_eq!(take_draft_thread(&mut app, &project), Some(thread), "the thread is created with the terminals' id");
        assert_eq!(current_thread(&app), None);
    }

    #[test]
    fn read_only_connections_cannot_change_the_drawer_layout() {
        let mut app = App::new();
        app.navigate(Route::Thread("t".into()));
        crate::chat::details::set_session(&mut app, Some(Ok(json!({"authenticated": true, "scopes": ["terminal:read"]}))));
        toggle(&mut app);
        assert!(app.ui.panel.drawer.groups(Owner::Drawer, "t").unwrap().is_empty());
        terminal::known_fixture(&mut app, "t", &["term-1", "term-2"]);
        reconcile(&mut app);
        new_terminal(&mut app, Owner::Drawer);
        split(&mut app, Owner::Drawer, false);
        request_close(&mut app, Owner::Drawer, "term-1");
        let drawer = &app.ui.panel.drawer;
        assert!(drawer.is_open("t"));
        assert_eq!(drawer.groups(Owner::Drawer, "t").unwrap().ids().count(), 2);
        assert!(drawer.confirm.is_none());
        terminal::known_fixture(&mut app, "t", &["term-3"]);
        assert_eq!(allocate(&app, "t"), "term-4");
    }

    #[test]
    fn closing_several_terminals_names_them_all() {
        let one = close_message(&["vim".into()]);
        assert_eq!(one.0, "Close terminal \"vim\"?");
        let two = close_message(&["Terminal 2".into(), "vim".into()]);
        assert_eq!(two.0, "Close 2 terminals?");
        assert_eq!(two.1, "This stops their running processes and clears their histories: \"Terminal 2\", \"vim\".");
    }
}
