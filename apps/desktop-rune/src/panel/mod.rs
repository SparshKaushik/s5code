//! Workspace surfaces; wire payloads stay identical to the web client's RPCs.
pub(crate) mod agent;
pub(crate) mod stream;
pub mod browser;
pub(crate) mod closed;
pub(crate) mod device;
pub(crate) mod diff;
pub mod drawer;
mod favicon;
mod file_preview;
mod file_viewers;
pub const FILE_VIEWER_SCENES: &[(&str, fn(&mut App))] = file_viewers::SCENES;
mod files;
mod history;
pub(crate) mod html_frame;
pub(crate) mod mcp_app;
pub mod popout;
mod proactive;
pub(crate) mod pull_requests;
pub(crate) mod preview;
pub mod browser_import;
pub(crate) mod terminal;
mod tabs;
pub(crate) mod vt;

pub(crate) use device::Viewport as DeviceViewport;

use crate::{
    App, Route, data, icons, kit,
    theme::{self, AlphaMul, T},
};
use rune::{Cx, El, Key, KeyEvent, Phase, Tx, Waker, col, el, icon, row, text};
use serde_json::{Map, Value, json};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    Browser(browser::TabId),
    Terminal,
    Files,
    Diff,
    PullRequests,
    Device,
    Tab(usize),
}
impl Surface {
    fn title(self) -> &'static str {
        match self {
            Self::Browser(_) => "Browser",
            Self::Terminal => "Terminal",
            Self::Files => "Files",
            Self::Diff => "Diff",
            Self::PullRequests => "Pull requests",
            Self::Device => "Device",
            Self::Tab(_) => "",
        }
    }
    fn glyph(self) -> &'static rune::Icon {
        match self {
            Self::Browser(_) => &icons::GLOBE,
            Self::Terminal => &icons::SQUARE_TERMINAL,
            Self::Files => &icons::FILES,
            Self::Diff => &icons::FILE_DIFF,
            Self::PullRequests => &icons::LINK,
            Self::Device => &icons::SMARTPHONE,
            Self::Tab(_) => &icons::FILES,
        }
    }
}
pub struct Ui {
    width: f32,
    tabs: tabs::Tabs,
    surfaces: Vec<Surface>,
    active: Option<Surface>,
    pub diff: diff::Ui,
    pub(crate) pull_requests: pull_requests::Ui,
    files: files::Ui,
    /// The open attachment tab's format-aware viewer.
    attachment: Option<(String, file_viewers::Viewer)>,
    device: device::Ui,
    terminal: terminal::Ui,
    drawer: drawer::Ui,
    browser: preview::Ui,
    add_menu: bool,
    /// The + menu's Browser profiles submenu, and the launcher's profile menu.
    add_profiles: bool,
    launcher_profiles: bool,
    /// The tab strip's right-click menu: its tab and window point.
    tab_menu: Option<(Surface, (f32, f32))>,
    thread: String,
    pub(crate) closed: closed::Store,
    /// Each thread's surfaces, as web's right panel state is per thread; browser tabs stay
    /// alive while their thread is in the background.
    stash: std::collections::HashMap<String, Stash>,
    stash_key: String,
    /// What `right-panel.json` holds; `None` until the first poll reads it.
    saved: Option<Value>,
    pub waker: Option<Waker>,
    pub(crate) html_frames: html_frame::Frames,
    pub(crate) mcp_apps: mcp_app::Apps,
    proactive: proactive::Watch,
    history: history::History,
    /// Threads whose panel fills the chat column's room (`rightPanelState.maximized`).
    maximized: std::collections::HashSet<String>,
}
#[derive(Default)]
struct Stash {
    surfaces: Vec<Surface>,
    active: Option<Surface>,
    open: bool,
    pip: Option<browser::TabId>,
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            width: 540.,
            tabs: Default::default(),
            surfaces: vec![],
            active: None,
            diff: Default::default(),
            pull_requests: Default::default(),
            files: Default::default(),
            attachment: None,
            device: Default::default(),
            terminal: Default::default(),
            drawer: Default::default(),
            browser: Default::default(),
            add_menu: false,
            add_profiles: false,
            launcher_profiles: false,
            tab_menu: None,
            thread: String::new(),
            closed: Default::default(),
            stash: Default::default(),
            stash_key: String::new(),
            saved: None,
            waker: None,
            html_frames: Default::default(),
            mcp_apps: Default::default(),
            proactive: Default::default(),
            history: Default::default(),
            maximized: Default::default(),
        }
    }
}
fn context(app: &App) -> Option<(String, String)> {
    let Route::Thread(id) = &app.route else {
        return None;
    };
    let thread = app.state().threads.iter().find(|v| v["id"] == *id)?;
    let project = app
        .state()
        .projects
        .iter()
        .find(|v| v["id"] == thread["projectId"])?;
    let cwd = thread["worktreePath"]
        .as_str()
        .filter(|v| !v.is_empty())
        .unwrap_or(data::s(project, "workspaceRoot"));
    Some((id.clone(), cwd.to_string()))
}
/// The diff's repository: the thread's, or on a new-thread draft its project's, keyed by the draft.
fn diff_context(app: &App) -> Option<(String, String)> {
    if let Some(c) = context(app) {
        return Some(c);
    }
    let cwd = workspace(app)?;
    Some((crate::chat::composer::draft_key(app, &app.route), cwd))
}
/// A diff's file path inside the open workspace (`resolveDiffPathForWorkspace`), or `None` when
/// it lies outside. A checkout thread reads repository-relative paths against the project's
/// repository root; a worktree thread is its own root.
pub(crate) fn workspace_path(app: &App, file_path: &str) -> Option<String> {
    let cwd = workspace(app);
    let thread = match &app.route {
        Route::Thread(id) => app.state().threads.iter().find(|v| v["id"] == *id),
        _ => None,
    };
    let project = match thread {
        Some(t) => app.state().projects.iter().find(|p| p["id"] == t["projectId"]),
        None => crate::chat::home::draft_project(app),
    };
    let in_worktree = match thread {
        Some(t) => !data::s(t, "worktreePath").is_empty(),
        None => crate::chat::details::draft_worktree(app).is_some(),
    };
    let root = project.map(|p| data::s(&p["repositoryIdentity"], "rootPath")).filter(|r| !in_worktree && !r.is_empty());
    diff::file_menu::resolve_diff_path(file_path, cwd.as_deref(), root)
}
/// Diff is offered in Git repositories only; until the status arrives it stays available.
fn diff_available(app: &App) -> bool {
    diff_context(app).is_some() && diff::is_repo(app) != Some(false)
}
/// The changed-files card's right-click menu: the diff file menu, outside the panel.
pub fn file_menu(app: &mut App, path: String, at: (f32, f32)) {
    diff::file_menu::open(app, path, at);
}
/// The Files surface's root: the thread's workspace, or on a new-thread draft its project's.
fn workspace(app: &App) -> Option<String> {
    if let Some((_, cwd)) = context(app) {
        return Some(cwd);
    }
    let Route::Home { .. } = &app.route else { return None };
    if let Some((path, _)) = crate::chat::details::draft_worktree(app) {
        return Some(path);
    }
    crate::chat::home::draft_project(app).map(|p| data::s(p, "workspaceRoot").to_string()).filter(|cwd| !cwd.is_empty())
}
#[derive(Clone, Copy)]
enum Launcher {
    Surface(Surface),
    PullRequest,
    SideChat,
}

const LAUNCH: [(Launcher, &str); 8] = [
    (Launcher::Surface(Surface::Browser(0)), "B"),
    (Launcher::Surface(Surface::Terminal), "T"),
    (Launcher::Surface(Surface::Files), "F"),
    (Launcher::Surface(Surface::Diff), "D"),
    (Launcher::PullRequest, "P"),
    (Launcher::Surface(Surface::PullRequests), "L"),
    (Launcher::Surface(Surface::Device), "M"),
    (Launcher::SideChat, "S"),
];

impl Launcher {
    fn title(self) -> &'static str {
        match self {
            Self::Surface(Surface::PullRequests) => "Linked pull requests",
            Self::Surface(surface) => surface.title(),
            Self::PullRequest => "Pull request",
            Self::SideChat => "Side chat",
        }
    }

    fn glyph(self) -> &'static rune::Icon {
        match self {
            Self::Surface(surface) => surface.glyph(),
            Self::PullRequest => &icons::GIT_PULL_REQUEST_ARROW,
            Self::SideChat => &icons::MESSAGE_SQUARE,
        }
    }

    fn available(self, app: &App) -> bool {
        match self {
            Self::Surface(surface) => launchable(app, surface),
            Self::PullRequest => pull_requests::launcher_target(app).is_some(),
            Self::SideChat => crate::chat::side::disabled_reason(app).is_none(),
        }
    }

    fn open(self, app: &mut App) {
        match self {
            Self::Surface(surface) => add(app, surface),
            Self::PullRequest => {
                if let Some(entry) = pull_requests::launcher_target(app) { open_pull_request(app, entry); }
            }
            Self::SideChat => crate::chat::side::send_command(app, "/side", false),
        }
    }

    fn hint(self, app: &App) -> Option<&'static str> {
        if self.available(app) { return None; }
        match self {
            Self::Surface(Surface::Diff) if diff_context(app).is_some() => Some("Available for Git repositories."),
            Self::Surface(Surface::PullRequests) => Some("No linked pull requests available."),
            Self::SideChat => crate::chat::side::disabled_reason(app),
            _ => None,
        }
    }
}

fn add_in_profile(app: &mut App, profile: &str) {
    let tab = preview::open_in(app, profile);
    activate(app, Surface::Browser(tab));
}
fn add(app: &mut App, surface: Surface) {
    match surface {
        Surface::Browser(_) => {
            let tab = preview::open(app);
            activate(app, Surface::Browser(tab));
        }
        Surface::Terminal if app.ui.panel.surfaces.iter().any(|&s| tabs::is_terminal(app, s)) => drawer::new_terminal(app, drawer::Owner::Panel),
        _ => activate(app, surface),
    }
}
fn activate(app: &mut App, surface: Surface) {
    let previous = app.ui.panel.active;
    if previous != Some(surface) {
        if let Some(tabs::Target::SideChat(side)) = previous.and_then(|s| app.ui.panel.tabs.target(s)).cloned() {
            crate::chat::side::focus(app, &side, false);
        }
        tabs::finish_rename(app, true);
    }
    app.ui.shell.panel_open = true;
    if !app.ui.panel.surfaces.contains(&surface) {
        app.ui.panel.surfaces.push(surface);
    }
    app.ui.panel.active = Some(surface);
    match surface {
        Surface::Diff => diff::load(app),
        Surface::Files => {
            if previous != Some(surface) { files::back_to_tree(app); }
            files::load(app, "".into());
        },
        Surface::Terminal => {
            drawer::open_panel(app);
            drawer::focus_panel(app);
        }
        Surface::Device => device::browse(app),
        Surface::Tab(id) => tabs::activate(app, id, previous != Some(surface)),
        _ => {}
    }
}
/// The thread details card's "Changes" row (`openChangesFromThreadPanel`).
pub fn open_changes(app: &mut App) {
    diff::open_changes(app);
    activate(app, Surface::Diff);
}
/// A file_change item's "Open diff": its recorded patch in the diff surface.
pub fn open_tool_diff(app: &mut App, thread: String, item: String) {
    diff::open_tool_call(app, thread, item);
    activate(app, Surface::Diff);
}
/// The thread's linked pull requests: the sidebar badge, the palette and the toolbar's PR badge.
pub fn show_pull_requests(app: &mut App) {
    // A badge on another thread's row navigates first; that thread's panel takes the surface.
    switch_thread(app);
    activate(app, Surface::PullRequests);
}
/// The open panel shows the linked pull requests list.
pub fn showing_pull_requests(app: &App) -> bool {
    app.ui.shell.panel_open && app.ui.panel.active == Some(Surface::PullRequests)
}
/// The open panel shows a browser tab, as web's `threadShowingBrowser`.
pub fn showing_browser(app: &App) -> bool {
    app.ui.shell.panel_open && matches!(app.ui.panel.active, Some(Surface::Browser(_)))
}
/// The device state subscription, while a thread's panel has its Device surface.
pub fn device_wanted(app: &App) -> Option<(String, Value)> {
    app.ui.panel.surfaces.iter().any(|&s| tabs::is_device(app, s)).then(|| (device::STATE.to_owned(), json!({})))
}
/// Shows `url` in the browser surface (a project script's `autoOpenPreview`).
pub fn open_preview(app: &mut App, url: &str) {
    let tab = preview::open(app);
    activate(app, Surface::Browser(tab));
    preview::navigate(app, tab, url);
}
/// Web's "Open in browser" for an HTML or PDF file, absolute or workspace-relative.
pub fn open_file_in_preview(app: &mut App, path: &str) {
    preview::open_file(app, path);
}
pub fn previewable_file(path: &str) -> bool {
    preview::previewable_file(path)
}
/// `workspace.focusNext`'s panel (order 4) and terminal (order 5) areas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Region {
    Panel,
    Terminal,
}
enum Field {
    Bar(browser::TabId),
    Files,
    Terminal(String, String),
}
/// The text field a region focuses, as the web's area picks a preferred control. The diff has
/// none Rune can focus, so a panel showing it is passed over.
fn field(app: &App, region: Region) -> Option<Field> {
    let terminal = |owner| {
        let (thread, id) = drawer::active_terminal(app, owner)?;
        app.ui.panel.terminal.get(&thread, &id).map(|_| Field::Terminal(thread, id))
    };
    match region {
        Region::Terminal => terminal(drawer::Owner::Drawer),
        Region::Panel if !app.ui.shell.panel_open => None,
        Region::Panel => match app.ui.panel.active? {
            Surface::Browser(tab) => Some(Field::Bar(tab)),
            Surface::Files => files::field_focused(app).map(|_| Field::Files),
            Surface::Terminal => terminal(drawer::Owner::Panel),
            Surface::Tab(id) if matches!(app.ui.panel.tabs.get(id), Some(tabs::Target::File(_))) => files::field_focused(app).map(|_| Field::Files),
            Surface::Tab(id) if matches!(app.ui.panel.tabs.get(id), Some(tabs::Target::Terminal(_))) => terminal(drawer::Owner::Panel),
            _ => None,
        },
    }
}
pub fn region_available(app: &App, region: Region) -> bool {
    field(app, region).is_some()
}
pub fn region_focused(app: &App, region: Region) -> bool {
    match field(app, region) {
        Some(Field::Bar(tab)) => preview::bar_focused(app, tab),
        Some(Field::Files) => files::field_focused(app) == Some(true),
        Some(Field::Terminal(thread, id)) => app.ui.panel.terminal.get(&thread, &id).is_some_and(|s| s.input.focused),
        None => false,
    }
}
/// Focuses `region`'s field and lets go of the others; `None` lets go of them all.
pub fn focus_region(app: &mut App, region: Option<Region>) {
    let target = region.and_then(|r| field(app, r));
    preview::focus_bar(app, if let Some(Field::Bar(tab)) = target { Some(tab) } else { None });
    files::focus_field(app, matches!(target, Some(Field::Files)));
    match target {
        Some(Field::Terminal(thread, id)) => terminal::focus(app, &thread, &id),
        _ => terminal::focus(app, "", ""),
    }
}
/// The terminal holds the keyboard, so `!terminalFocus` shortcuts stand down.
pub fn terminal_active(app: &App) -> bool {
    app.ui.shell.panel_open && app.ui.panel.active.is_some_and(|s| tabs::is_terminal(app, s))
}
fn close(app: &mut App, surface: Surface) {
    if !app.ui.panel.surfaces.contains(&surface) { return; }
    if let Some(tabs::Target::Terminal(group)) = app.ui.panel.tabs.target(surface) {
        drawer::request_close_groups(app, vec![group.clone()]);
        return;
    }
    if surface == Surface::Terminal {
        drawer::request_close_panel(app);
        return;
    }
    let view = match surface {
        Surface::Files => Some(closed::View::Files(String::new())),
        Surface::Diff => Some(closed::View::Diff),
        Surface::PullRequests => Some(closed::View::PullRequests),
        Surface::Browser(tab) => preview::url(app, tab).map(closed::View::Browser),
        Surface::Tab(id) => app.ui.panel.tabs.get(id).and_then(tabs::Target::closed),
        Surface::Terminal | Surface::Device => None,
    };
    if let (Some(view), Some((thread, _))) = (view, context(app)) {
        app.ui.panel.closed.record(closed::Closed { thread, view });
    }
    match surface {
        Surface::Files => files::flush(app),
        Surface::Browser(tab) => preview::close(app, tab),
        Surface::Tab(id) => tabs::closed(app, id),
        _ => {}
    }
    remove_surface(app, surface);
}
/// `view.reopenClosed`: the newest closed view still worth reopening, on its thread or the pull
/// requests page, from either.
fn reopen_closed(app: &mut App, tx: &mut Tx) -> bool {
    let mut store = std::mem::take(&mut app.ui.panel.closed);
    let current = context(app).map(|(id, _)| id);
    let exists = |thread: &str| thread == closed::PAGE || app.state().threads.iter().any(|v| v["id"] == thread);
    let next = store.next(exists, |c| {
        if let closed::View::SideChat(side) = &c.view
            && !exists(side)
        {
            return true;
        }
        if let closed::View::PullRequest(entry) = &c.view
            && c.thread == closed::PAGE
        {
            return crate::pages::pr_tabs::showing(app, entry);
        }
        let panel = &app.ui.panel;
        let (surfaces, open) = if current.as_deref() == Some(c.thread.as_str()) {
            (panel.surfaces.as_slice(), app.ui.shell.panel_open)
        } else if let Some(stash) = panel.stash.get(&c.thread) {
            (stash.surfaces.as_slice(), stash.open)
        } else {
            return false;
        };
        open && surfaces.iter().any(|&surface| match surface {
            Surface::Diff => c.view == closed::View::Diff,
            Surface::PullRequests => c.view == closed::View::PullRequests,
            Surface::Files => c.view == closed::View::Files(String::new()),
            Surface::Tab(id) => panel.tabs.get(id).and_then(tabs::Target::closed).as_ref().is_some_and(|view| view.same_target(&c.view)),
            _ => false,
        })
    });
    app.ui.panel.closed = store;
    let c = match next {
        None => return false,
        Some(closed::Closed { view: closed::View::PullRequest(entry), thread }) if thread == closed::PAGE => {
            let tab = crate::pages::pr_tabs::restore(app, entry);
            tx.scroll_into_view_in(crate::pages::pr_tabs::STRIP, tab);
            return true;
        }
        Some(c) => c,
    };
    let parent = c.thread.clone();
    if current.as_deref() != Some(c.thread.as_str()) {
        app.navigate(Route::Thread(c.thread.clone()));
        // Swap in that thread's panel and switch its thread now, so the next poll neither
        // stashes nor resets what reopens here.
        switch_thread(app);
        let previous = std::mem::replace(&mut app.ui.panel.thread, c.thread);
        diff::reset(app, &previous);
        files::reset(app);
    }
    match c.view {
        closed::View::Diff => activate(app, Surface::Diff),
        closed::View::PullRequests => activate(app, Surface::PullRequests),
        closed::View::Files(path) if path.is_empty() => activate(app, Surface::Files),
        closed::View::Files(path) => open_file(app, path),
        closed::View::Browser(url) => open_preview(app, &url),
        closed::View::SideChat(side) => crate::chat::side::reopen(app, &parent, &side),
        closed::View::PullRequest(entry) => open_pull_request(app, entry),
        closed::View::Attachment(id) => open_attachment(app, &id),
        closed::View::Device { host, device, name, title } => {
            let id = tabs::open(app, tabs::Target::Device { host, device, name, title });
            tx.scroll_into_view_in(tabs::STRIP, tabs::key(Surface::Tab(id)));
        }
    }
    true
}
fn remove_surface(app: &mut App, surface: Surface) {
    let was_active = app.ui.panel.active == Some(surface);
    if app.ui.panel.tabs.rename.as_ref().is_some_and(|(id, _)| Surface::Tab(*id) == surface) {
        tabs::finish_rename(app, true);
    }
    let next = tabs::remove(&mut app.ui.panel.surfaces, surface);
    if was_active {
        app.ui.panel.active = None;
        if let Some(next) = next {
            activate(app, next);
        }
    }
}
pub(crate) fn open_side_chat(app: &mut App, parent: &str, side: &str) {
    switch_thread(app);
    let target = tabs::Target::SideChat(side.to_owned());
    if app.ui.panel.stash_key == parent {
        tabs::open(app, target);
    } else {
        let panel = &mut app.ui.panel;
        let surface = Surface::Tab(panel.tabs.intern(parent, target));
        let stash = panel.stash.entry(parent.to_owned()).or_default();
        if !stash.surfaces.contains(&surface) {
            stash.surfaces.push(surface);
        }
        stash.active = Some(surface);
        stash.open = true;
    }
}
pub(crate) fn active_side_chat(app: &App) -> Option<&str> {
    if !matches!(app.frame_route(), Route::Thread(parent) if parent == &app.ui.panel.stash_key) { return None; }
    match active_target(app)? {
        tabs::Target::SideChat(side) => Some(side),
        _ => None,
    }
}
pub(crate) fn close_side_chat(app: &mut App, side: &str, remember: bool) {
    if let Some(surface) = app.ui.panel.surfaces.iter().copied().find(|&surface| {
        matches!(app.ui.panel.tabs.target(surface), Some(tabs::Target::SideChat(id)) if id == side)
    }) {
        if remember { close(app, surface); } else { remove_surface(app, surface); }
    }
    for stash in app.ui.panel.stash.values_mut() {
        if let Some(surface) = stash.surfaces.iter().copied().find(|&surface| {
            matches!(app.ui.panel.tabs.target(surface), Some(tabs::Target::SideChat(id)) if id == side)
        }) {
            let next = tabs::remove(&mut stash.surfaces, surface);
            if stash.active == Some(surface) {
                stash.active = next;
            }
        }
    }
}
pub(crate) fn open_pull_request(app: &mut App, entry: Value) {
    tabs::open(app, tabs::Target::PullRequest(entry));
}
pub(crate) fn active_pull_request(app: &App) -> Option<&Value> {
    if !matches!(app.frame_route(), Route::Thread(_)) { return None; }
    match active_target(app)? {
        tabs::Target::PullRequest(entry) => Some(entry),
        _ => None,
    }
}
pub(crate) fn set_pull_request_section(app: &mut App, section: usize) {
    let Some(Surface::Tab(id)) = app.ui.panel.active.filter(|_| app.ui.shell.panel_open) else { return };
    if let Some(tabs::Target::PullRequest(entry)) = app.ui.panel.tabs.get_mut(id) {
        entry["detailTab"] = json!(section);
    }
}
pub(crate) fn open_attachment(app: &mut App, id: &str) {
    tabs::open(app, tabs::Target::Attachment(id.to_owned()));
}
fn active_target(app: &App) -> Option<&tabs::Target> {
    app.ui.shell.panel_open.then(|| app.ui.panel.active).flatten().and_then(|surface| app.ui.panel.tabs.target(surface))
}

/// The Files tree or a file's own tab is on screen.
pub(super) fn files_showing(app: &App) -> bool {
    app.ui.shell.panel_open && (app.ui.panel.active == Some(Surface::Files) || matches!(active_target(app), Some(tabs::Target::File(_))))
}

fn sync_terminal_tabs(app: &mut App) {
    let panel = &app.ui.panel;
    let has_tab = panel.surfaces.iter().any(|&surface| tabs::is_terminal(app, surface));
    if !has_tab { return; }
    let groups: Vec<String> = drawer::panel_groups(app).into_iter().flat_map(|g| &g.groups).map(|g| g.id.clone()).collect();
    let active = drawer::panel_groups(app).and_then(drawer::Groups::visible).map(|group| group.id.clone());
    let panel = &mut app.ui.panel;
    let scope = panel.stash_key.clone();
    let keyed: Vec<(String, Surface)> = groups.into_iter().map(|group| {
        let id = panel.tabs.intern(&scope, tabs::Target::Terminal(group.clone()));
        (group, Surface::Tab(id))
    }).collect();
    let old = panel.surfaces.iter().position(|&surface| surface == Surface::Terminal);
    if let Some(index) = old.filter(|_| !keyed.is_empty()) {
        panel.surfaces.splice(index..=index, keyed.iter().map(|(_, surface)| *surface));
        if panel.active == Some(Surface::Terminal) {
            panel.active = active.as_ref().and_then(|active| keyed.iter().find(|(group, _)| group == active)).map(|(_, s)| *s);
        }
    }
    let stale: Vec<Surface> = panel.surfaces.iter().copied().filter(|&surface| {
        matches!(panel.tabs.target(surface), Some(tabs::Target::Terminal(_))) && !keyed.iter().any(|(_, s)| *s == surface)
    }).collect();
    let active_closed = stale.contains(&panel.active.unwrap_or(Surface::Terminal));
    let mut next = None;
    for surface in stale {
        let neighbor = tabs::remove(&mut panel.surfaces, surface);
        if panel.active == Some(surface) { next = neighbor; }
    }
    if active_closed {
        app.ui.panel.active = None;
        if let Some(surface) = next.filter(|s| app.ui.panel.surfaces.contains(s)) {
            activate(app, surface);
        }
    }
}

/// Gives `thread`'s panel, on screen or put away, a browser tab, leaving the panel shut.
/// Whether that panel is the one on screen.
fn add_browser(app: &mut App, thread: &str, tab: browser::TabId, float: bool) -> bool {
    let surface = Surface::Browser(tab);
    let panel = &mut app.ui.panel;
    let (surfaces, active, pip) = if panel.stash_key == thread {
        (&mut panel.surfaces, &mut panel.active, &mut panel.browser.pip)
    } else {
        let stash = panel.stash.entry(thread.to_owned()).or_default();
        (&mut stash.surfaces, &mut stash.active, &mut stash.pip)
    };
    if !surfaces.contains(&surface) {
        surfaces.push(surface);
    }
    active.get_or_insert(surface);
    if float {
        *pip = Some(tab);
    }
    panel.stash_key == thread
}
/// Takes a browser tab off every thread's panel.
fn drop_browser(app: &mut App, tab: browser::TabId) {
    let surface = Surface::Browser(tab);
    remove_surface(app, surface);
    for stash in app.ui.panel.stash.values_mut() {
        stash.surfaces.retain(|v| *v != surface);
        if stash.active == Some(surface) {
            stash.active = stash.surfaces.last().copied();
        }
        if stash.pip == Some(tab) {
            stash.pip = None;
        }
    }
}
/// Opens a workspace file in the Files surface (the palette's "Go to file").
pub fn open_file(app: &mut App, path: String) {
    open_file_at(app, path, None);
}
/// Opens `path` in the Files surface and centres 1-based `line` once it loads.
pub fn open_file_at(app: &mut App, path: String, line: Option<usize>) {
    let path = tabs::file_path(&path);
    if path == "." || path.is_empty() {
        activate(app, Surface::Files);
        return;
    }
    let id = tabs::open(app, tabs::Target::File(path.clone()));
    app.ui.panel.surfaces.retain(|&surface| surface != Surface::Files);
    if let Some(line) = line {
        files::reveal_line(app, &path, line);
    }
    app.ui.panel.tabs.reveal = Some(Surface::Tab(id));
}
pub fn open_diff(app: &mut App, checkpoint: Value) {
    app.ui.chat.timeline.diff_target = Some(checkpoint.clone());
    let checkpoint = if checkpoint["appRunOrdinal"].is_null() {
        if let Route::Thread(id) = &app.route {
            app.state()
                .thread_views
                .get(id)
                .and_then(|v| v.projection["checkpoints"].as_array())
                .and_then(|rows| rows.iter().find(|v| v["id"] == checkpoint["checkpointId"]))
                .cloned()
                .unwrap_or(checkpoint)
        } else {
            checkpoint
        }
    } else {
        checkpoint
    };
    let turn = checkpoint["checkpointTurnCount"]
        .as_u64()
        .or(checkpoint["appRunOrdinal"].as_u64())
        .unwrap_or(1);
    diff::open_turn(app, turn);
    activate(app, Surface::Diff);
}
const STATE_FILE: &str = "right-panel.json";

impl Surface {
    /// Browser tabs are live pages, so only these surfaces outlive a restart.
    fn saved_id(self) -> Option<&'static str> {
        match self {
            Self::Terminal => Some("terminal"),
            Self::Files => Some("files"),
            Self::Diff => Some("diff"),
            Self::PullRequests => Some("pull-requests"),
            Self::Browser(_) | Self::Device | Self::Tab(_) => None,
        }
    }
    fn from_saved_id(id: &str) -> Option<Self> {
        [Self::Terminal, Self::Files, Self::Diff, Self::PullRequests].into_iter().find(|s| s.saved_id() == Some(id))
    }
}

impl Ui {
    /// Web's `t3code:right-panel-state`: each thread's open flag, surfaces and active surface.
    fn to_json(&self, open: bool) -> Value {
        let entry = |surfaces: &[Surface], active: Option<Surface>, open: bool| {
            let ids: Vec<Value> = surfaces.iter().filter_map(|&s| self.saved_surface(s)).collect();
            (!ids.is_empty()).then(|| json!({"isOpen": open, "activeSurfaceId": active.and_then(|s| self.saved_surface(s)), "surfaces": ids}))
        };
        let mut threads = Map::new();
        for (key, stash) in &self.stash {
            if let Some(v) = entry(&stash.surfaces, stash.active, stash.open).filter(|_| !key.is_empty()) {
                threads.insert(key.clone(), v);
            }
        }
        if let Some(v) = entry(&self.surfaces, self.active, open).filter(|_| !self.stash_key.is_empty()) {
            threads.insert(self.stash_key.clone(), v);
        }
        let mut saved = json!({"version": 1, "byThreadKey": threads});
        // Web keeps this in `t3code:diff-file-tree-width`; it outlives a restart for every thread.
        if let Some(w) = self.diff.tree_w {
            saved["diffFileTreeWidth"] = json!(w);
        }
        saved
    }

    fn saved_surface(&self, surface: Surface) -> Option<Value> {
        match surface {
            Surface::Tab(id) => self.tabs.get(id).map(tabs::Target::encode),
            _ => surface.saved_id().map(|id| json!(id)),
        }
    }
    fn restore_surface(&mut self, scope: &str, value: &Value) -> Option<Surface> {
        if let Some(id) = value.as_str() {
            return Surface::from_saved_id(id);
        }
        tabs::Target::decode(value).map(|target| Surface::Tab(self.tabs.intern(scope, target)))
    }
    /// Fills the stash, so the first switch to a thread brings its saved panel back.
    fn restore(&mut self, v: &Value) {
        if let Some(w) = v["diffFileTreeWidth"].as_f64() {
            self.diff.tree_w = Some(w as f32);
        }
        for (key, entry) in v["byThreadKey"].as_object().into_iter().flatten() {
            if key.is_empty() || *key == self.stash_key || self.stash.contains_key(key) {
                continue;
            }
            let mut surfaces = vec![];
            for surface in entry["surfaces"].as_array().into_iter().flatten().filter_map(|v| self.restore_surface(key, v)) {
                if !surfaces.contains(&surface) {
                    surfaces.push(surface);
                }
            }
            let Some(&last) = surfaces.last() else { continue };
            let active = self.restore_surface(key, &entry["activeSurfaceId"]).filter(|s| surfaces.contains(s)).unwrap_or(last);
            let open = entry["isOpen"].as_bool().unwrap_or(false);
            self.stash.insert(key.clone(), Stash { surfaces, active: Some(active), open, pip: None });
        }
    }
}

/// Writes `value` to `path` through a temporary file, so a crash never leaves half a file.
pub(crate) fn write_state(path: &std::path::Path, value: &Value) -> bool {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    serde_json::to_vec(value).ok().is_some_and(|bytes| {
        path.parent().is_none_or(|p| std::fs::create_dir_all(p).is_ok())
            && std::fs::write(&temporary, bytes).is_ok()
            && std::fs::rename(&temporary, path).is_ok()
    })
}

fn persist(app: &mut App) {
    if cfg!(test) {
        return;
    }
    let path = app.client_settings.sibling(STATE_FILE);
    let open = app.ui.shell.panel_open;
    let ui = &mut app.ui.panel;
    if ui.saved.is_none() {
        if let Some(v) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
            ui.restore(&v);
        }
        ui.saved = Some(ui.to_json(open));
        return;
    }
    let next = ui.to_json(open);
    if ui.saved.as_ref() != Some(&next) && write_state(&path, &next) {
        ui.saved = Some(next);
    }
}

pub(crate) fn flush_file(app: &mut App) {
    files::flush(app);
}

/// Puts the panel away with the thread it belonged to and brings back the next thread's.
fn switch_thread(app: &mut App) -> bool {
    let key = match app.frame_route() {
        Route::Thread(id) => id.clone(),
        _ => String::new(),
    };
    if app.ui.panel.stash_key == key {
        return false;
    }
    let previous = std::mem::replace(&mut app.ui.panel.stash_key, key.clone());
    let panel = &mut app.ui.panel;
    let saved = Stash {
        surfaces: std::mem::take(&mut panel.surfaces),
        active: panel.active.take(),
        open: app.ui.shell.panel_open,
        pip: panel.browser.pip.take(),
    };
    panel.stash.insert(previous, saved);
    let next = panel.stash.remove(&key).unwrap_or_default();
    panel.surfaces = next.surfaces;
    panel.active = next.active;
    panel.browser.pip = next.pip;
    panel.add_menu = false;
    panel.tabs.rename = None;
    panel.tabs.drag = None;
    panel.tabs.reveal = next.active;
    app.ui.shell.panel_open = next.open;
    true
}

/// A thread launched from the new-thread draft keeps the panel the draft had, as web's draft id
/// becomes the thread id.
pub fn adopt(app: &mut App, thread: &str) {
    let panel = &mut app.ui.panel;
    if panel.stash_key.is_empty() {
        panel.tabs.adopt_scope("", thread);
        panel.stash_key = thread.to_owned();
    } else if let Some(draft) = panel.stash.remove("") {
        panel.tabs.adopt_scope("", thread);
        panel.stash.insert(thread.to_owned(), draft);
    }
}

/// The turn's diff, opened and scrolled to `path` once it loads (a changed-files row).
pub fn open_diff_at(app: &mut App, checkpoint: Value, path: &str) {
    open_diff(app, checkpoint);
    app.ui.panel.diff.reveal = Some(path.to_owned());
}
pub fn poll(app: &mut App, tx: &mut Tx) -> bool {
    let mut context_changed = false;
    if matches!(app.src, crate::data::Source::Live(_)) {
        // Terminal layouts load first, so a restored Terminal surface reuses its saved ids.
        drawer::persist(app);
        persist(app);
        context_changed |= switch_thread(app);
        let thread = diff_context(app).map(|(id, _)| id).unwrap_or_default();
        if app.ui.panel.thread != thread {
            let previous = std::mem::replace(&mut app.ui.panel.thread, thread);
            diff::reset(app, &previous);
            files::reset(app);
            if app.ui.shell.panel_open {
                match app.ui.panel.active {
                    Some(Surface::Diff) => diff::load(app),
                    Some(Surface::Files) => files::load(app, String::new()),
                    Some(Surface::Tab(id)) => tabs::activate(app, id, true),
                    _ => {}
                }
            }
            context_changed = true;
        }
        if terminal_active(app) {
            drawer::open_panel(app);
        }
        context_changed |= proactive::poll(app);
    }

    tabs::poll_rename(app);
    if matches!(app.src, crate::data::Source::Live(_)) { sync_terminal_tabs(app); }
    if let Some(surface) = app.ui.panel.tabs.reveal.take() {
        tx.scroll_into_view_in(tabs::STRIP, tabs::key(surface));
    }
    pull_requests::want_avatars(app);
    let changed = diff::poll(app)
        | diff::reveal(app, tx)
        | files::poll(app, tx)
        | device::poll(app, tx)
        | terminal::poll(app, tx)
        | drawer::poll(app)
        | preview::poll(app, tx)
        | html_frame::poll(app)
        | mcp_app::poll(app, tx);
    changed | context_changed
}
/// Whether `e` resolves to `command` (`resolveShortcutCommand`): the last binding whose key
/// matches and whose `when` holds wins, so `mod+d` splits a focused terminal and toggles the
/// diff elsewhere.
pub(crate) fn shortcut(app: &App, e: &KeyEvent, command: &str) -> bool {
    let Some(bindings) = app.state().config.as_ref().and_then(|v| v["keybindings"].as_array()) else { return false };
    resolve(bindings, e, &|name: &str| when_context(app, name)) == Some(command)
}

fn resolve<'a>(bindings: &'a [serde_json::Value], e: &KeyEvent, context: &dyn Fn(&str) -> bool) -> Option<&'a str> {
    // Shift turns "[" into "{" on the key event; bindings name the unshifted key, and Space "space".
    let char_key = |c: char| match if e.mods.shift { crate::settings::unshift(c) } else { c } {
        ' ' => "space".to_string(),
        c => c.to_string(),
    };
    let key = match &e.key {
        Key::Command(c) => char_key(*c),
        Key::Text(v) if v.chars().count() == 1 => char_key(v.chars().next().unwrap_or(' ')),
        Key::Text(v) => v.clone(),
        Key::Home => "home".into(),
        Key::End => "end".into(),
        Key::PageUp => "pageup".into(),
        Key::PageDown => "pagedown".into(),
        Key::Function(n) => format!("f{n}"),
        Key::Enter => "enter".into(),
        Key::Tab => "tab".into(),
        Key::Escape => "escape".into(),
        Key::Backspace => "backspace".into(),
        Key::Delete => "delete".into(),
        Key::Up => "arrowup".into(),
        Key::Down => "arrowdown".into(),
        Key::Left => "arrowleft".into(),
        Key::Right => "arrowright".into(),
        _ => String::new(),
    };
    if key.is_empty() {
        return None;
    }
    let keys = event_keys(&key.to_lowercase(), physical_code(e).as_deref());
    bindings
        .iter()
        .rev()
        .find(|v| {
            let k = &v["shortcut"];
            (v["whenAst"].is_null() || when(&v["whenAst"], context))
                && k["key"].as_str().is_some_and(|v| keys.iter().any(|key| v.eq_ignore_ascii_case(key)))
                && modifiers_match(k, e.mods)
        })
        .and_then(|v| v["command"].as_str())
}

pub(crate) fn terminal_focused(app: &App) -> bool {
    app.ui.panel.terminal.focused()
}

thread_local! {
    /// What a key handler's `Tx` knew about the key the window is handling: the key, its
    /// physical code (winit's name, "KeyA") and whether a text field had focus.
    static KEY_CONTEXT: std::cell::RefCell<Option<(Key, Option<String>, bool)>> = const { std::cell::RefCell::new(None) };
}

/// Records the key's physical code and focus for the shortcut matcher, which handlers call
/// without their `Tx`.
pub fn note_key(e: &KeyEvent, tx: &Tx) {
    let context = (e.key.clone(), tx.physical_key().map(|code| format!("{code:?}")), tx.focused_editable());
    KEY_CONTEXT.with(|c| *c.borrow_mut() = Some(context));
}

/// The noted physical code, when it was noted for this key.
fn physical_code(e: &KeyEvent) -> Option<String> {
    KEY_CONTEXT.with(|c| c.borrow().as_ref().filter(|(key, _, _)| *key == e.key).and_then(|(_, code, _)| code.clone()))
}

/// `editableFocus` (`isEditableFocused`): a text field has focus.
fn editable_focus() -> bool {
    KEY_CONTEXT.with(|c| c.borrow().as_ref().is_some_and(|(_, _, focused)| *focused))
}

/// `EVENT_CODE_SHORTCUT_KEYS`: the US key at a physical position.
fn code_key(code: &str) -> Option<String> {
    if let Some(letter) = code.strip_prefix("Key").filter(|l| l.len() == 1) {
        return Some(letter.to_lowercase());
    }
    if let Some(digit) = code.strip_prefix("Digit").filter(|d| d.len() == 1) {
        return Some(digit.to_owned());
    }
    let key = match code {
        "Backquote" => "`",
        "Backslash" => "\\",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Comma" => ",",
        "Equal" => "=",
        "Minus" => "-",
        "Period" => ".",
        "Quote" => "'",
        "Semicolon" => ";",
        "Slash" => "/",
        _ => return None,
    };
    Some(key.to_owned())
}

/// `resolveEventKeys`: the typed key, and unless it is a Latin letter the key at its physical
/// position, so ⌘⇧7 on a German layout (typed "/") and ⌘К on a Russian one match ⌘⇧7 and ⌘K.
fn event_keys(typed: &str, code: Option<&str>) -> Vec<String> {
    let mut keys = vec![typed.to_owned()];
    let letter = typed.len() == 1 && typed.chars().all(|c| c.is_ascii_lowercase());
    if let Some(physical) = code.and_then(code_key).filter(|k| !letter && *k != typed) {
        keys.push(physical);
    }
    keys
}

/// `matchesKeybindingShortcutModifiers`: exactly the shortcut's modifiers are held.
pub(crate) fn modifiers_match(k: &serde_json::Value, mods: rune::Mods) -> bool {
    let yes = |name: &str| k[name].as_bool().unwrap_or(false);
    mods.cmd == (yes("modKey") || yes("metaKey") || (!cfg!(target_os = "macos") && yes("ctrlKey")))
        && mods.ctrl == (yes("ctrlKey") || (!cfg!(target_os = "macos") && yes("modKey")))
        && mods.shift == yes("shiftKey")
        && mods.alt == yes("altKey")
}

/// `evaluateWhenNode`: unknown identifiers are false.
fn when(node: &serde_json::Value, context: &dyn Fn(&str) -> bool) -> bool {
    match node["type"].as_str() {
        Some("identifier") => match node["name"].as_str() {
            Some("true") => true,
            Some("false") | None => false,
            Some(name) => context(name),
        },
        Some("not") => !when(&node["node"], context),
        Some("and") => when(&node["left"], context) && when(&node["right"], context),
        Some("or") => when(&node["left"], context) || when(&node["right"], context),
        _ => false,
    }
}

/// The `ShortcutMatchContext` keys this window can answer.
fn when_context(app: &App, name: &str) -> bool {
    match name {
        "isDesktop" => true,
        "terminalFocus" => app.ui.panel.terminal.focused(),
        "terminalOpen" => terminal_active(app) || context(app).is_some_and(|(thread, _)| app.ui.panel.drawer.is_open(&thread)),
        "rightPanelOpen" => app.ui.shell.panel_open,
        "previewOpen" => app.ui.shell.panel_open && matches!(app.ui.panel.active, Some(Surface::Browser(_))),
        "previewFocus" => app.ui.panel.browser.focused,
        "usagePageOpen" => matches!(app.route, crate::Route::Usage),
        "modelPickerOpen" => app.ui.chat.composer.picker.open,
        "composerFocus" => crate::chat::composer::focused(app),
        "turnRunning" => crate::chat::composer::running(app),
        "draftThreadRoute" => matches!(app.frame_route(), crate::Route::Home { .. }),
        // While the sidebar's undo notice claims mod+z it is not a text field's (AppSidebarLayout).
        "editableFocus" => editable_focus() && !(!app.ui.shell.sidebar.collapsed && app.ui.shell.sidebar.undos.claims_key(crate::data::now_ms())),
        _ => false,
    }
}
pub fn on_key(app: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if !app.ui.panel.closed.is_empty() && !tx.key_repeat() && shortcut(app, e, "view.reopenClosed") {
        return reopen_closed(app, tx);
    }
    if shortcut(app, e, "rightPanel.toggle") {
        toggle_panel(app);
        return true;
    }
    if app.ui.shell.panel_open && matches!(app.frame_route(), Route::Thread(_)) && shortcut(app, e, "rightPanel.toggleMaximized") {
        if !tx.key_repeat() {
            toggle_maximized(app);
        }
        return true;
    }
    if app.ui.shell.panel_open && shortcut(app, e, "rightPanel.new") {
        if let Some(side) = active_side_chat(app).map(str::to_owned) {
            crate::chat::side::focus(app, &side, false);
        }
        app.ui.panel.active = None;
        return true;
    }
    if app.ui.shell.panel_open && shortcut(app, e, "rightPanel.close") {
        if let Some(surface) = app.ui.panel.active {
            close(app, surface);
        } else {
            app.ui.shell.panel_open = false;
        }
        return true;
    }
    if shortcut(app, e, "terminal.toggle") {
        toggle_terminal(app);
        return true;
    }
    if diff_context(app).is_some() && shortcut(app, e, "diff.toggle") {
        if app.ui.shell.panel_open && app.ui.panel.active == Some(Surface::Diff) {
            app.ui.shell.panel_open = false;
        } else {
            activate(app, Surface::Diff);
        }
        return true;
    }
    if preview::on_key(app, e, tx) {
        return true;
    }
    if app.ui.shell.panel_open
        && app.ui.panel.active.is_none()
        && tx
            .focused()
            .is_none_or(|key| key.starts_with('#') || key.contains("panel-launcher"))
        && !e.mods.cmd
        && !e.mods.ctrl
        && !e.mods.alt
    {
        if let Key::Text(v) = &e.key {
            let key = v.to_uppercase();
            if let Some(&(surface, _)) = LAUNCH.iter().find(|(_, k)| *k == key) {
                if surface.available(app) {
                    surface.open(app);
                }
                return true;
            }
        }
    }
    false
}
/// The topbar button and `terminal.toggle`: the thread's bottom drawer.
pub fn toggle_terminal(app: &mut App) {
    drawer::toggle(app);
}
fn action(t: &T, label: &str, glyph: &'static rune::Icon) -> kit::Button {
    let _ = t;
    kit::Button::new(label)
        .icon(glyph)
        .variant("ghost-muted")
        .size("icon-xs")
}
/// Whether the launcher can open `surface` here.
fn launchable(app: &App, surface: Surface) -> bool {
    match surface {
        Surface::Browser(_) => true,
        Surface::Diff => diff_available(app),
        Surface::PullRequests => pull_requests::available(app),
        _ => context(app).is_some(),
    }
}
fn picker(app: &App, t: &T) -> El {
    let mut list = col().w(320.).gap(2.).child(
        text("Open a surface")
            .font(theme::SM_MEDIUM)
            .align(rune::Align::Center)
            .color(t.c.contrast_foreground)
            .mb(10.),
    );
    for (launcher, key) in LAUNCH {
        let available = launcher.available(app);
        let hint = launcher.hint(app);
        let title = launcher.title();
        list = list.child(
            row()
                .h(32.)
                .px(10.)
                .gap(10.)
                .center_y()
                .radius(8.)
                .hover(t.c.accent.alpha_mul(0.6))
                .focusable(true)
                .role("button")
                .label(title)
                .disabled(!available)
                .child(
                    icon(launcher.glyph(), 16.)
                        .size(16., 16.)
                        .color(t.c.contrast_foreground),
                )
                .child(
                    text(title)
                        .font(theme::TEXT_SM)
                        .color(t.c.contrast_foreground)
                        .grow(1.),
                )
                // Web's "Open browser in a profile" chevron.
                .children(matches!(launcher, Launcher::Surface(Surface::Browser(_))).then(|| {
                    el().key("panel-launcher-profiles").child(
                        kit::Button::new("Open browser in a profile")
                            .icon(&icons::CHEVRON_DOWN)
                            .variant("ghost-muted")
                            .size("icon-xs")
                            .build(t)
                            .on_press(|s: &mut App| s.ui.panel.launcher_profiles ^= true),
                    )
                }))
                .child(text(key).font(theme::MONO_XS).color(t.c.muted_foreground))
                .help(hint.unwrap_or_default())
                .on_press(move |s: &mut App| launcher.open(s)),
        );
    }
    col()
        .key("panel-launcher")
        .focus_group()
        .grow(1.)
        .min_h(0.)
        .center()
        .px(24.)
        .pb(52.)
        .child(list)
}
/// `RIGHT_PANEL_INLINE_LAYOUT_MEDIA_QUERY`: at this width or less the panel is a sheet.
const SHEET_BELOW: f32 = 980.;

/// RightPanelSheet's width for a window `viewport` wide, or None when the panel sits inline:
/// `min(42vw, 28rem)` at least 20rem, and `min(88vw, 24rem)` at 760 or less.
fn sheet_width(viewport: f32) -> Option<f32> {
    if viewport > SHEET_BELOW {
        None
    } else if viewport <= 760. {
        Some((viewport * 0.88).min(384.))
    } else {
        Some((viewport * 0.42).min(448.).max(320.))
    }
}

/// `rightPanelMaximized`: the thread's open panel, inline (not the narrow window's sheet),
/// maximized over the chat column.
pub fn maximized(app: &App, viewport: Option<f32>) -> bool {
    app.ui.shell.panel_open
        && viewport.and_then(sheet_width).is_none()
        && matches!(app.frame_route(), Route::Thread(id) if app.ui.panel.maximized.contains(id))
}

/// `toggleRightPanelMaximized`: only an open panel on a thread maximizes.
fn toggle_maximized(app: &mut App) {
    let Route::Thread(id) = app.frame_route().clone() else { return };
    if app.ui.shell.panel_open && !app.ui.panel.maximized.remove(&id) {
        app.ui.panel.maximized.insert(id);
    }
}

fn toggle_panel(app: &mut App) {
    if app.ui.shell.panel_open {
        close_panel(app);
    } else if let Some(surface) = app.ui.panel.active {
        app.ui.panel.active = None;
        activate(app, surface);
    } else {
        app.ui.shell.panel_open = true;
    }
}
fn close_panel(s: &mut App) {
    if let Some(side) = active_side_chat(s).map(str::to_owned) {
        crate::chat::side::focus(s, &side, false);
    }
    tabs::finish_rename(s, true);
    s.ui.shell.panel_open = false;
}

pub fn view(app: &App, cx: &mut Cx<App>, t: &T, overlays: &mut Vec<El>) -> El {
    let viewport = cx.layout_with("shell", |r| r[2]);
    let sheet = viewport.and_then(sheet_width);
    let width = sheet.unwrap_or_else(|| app.ui.panel.width.min(viewport.map_or(1000., |w| w * 0.7)));
    // Web's RightPanelTabs: 24pt tabs whose surface icon turns into the close button on hover.
    let mut tabs = row().h_full().gap(tabs::GAP).center_y().role("tablist");
    let items: Vec<_> = app.ui.panel.surfaces.iter().map(|&surface| {
        let title = match surface {
            Surface::Browser(tab) => preview::title(app, tab),
            Surface::Tab(id) => tabs::title(app, id),
            _ => surface.title().to_string(),
        };
        let audio = match surface {
            Surface::Browser(tab) => preview::audio(app, tab),
            _ => None,
        };
        let renaming = app.ui.panel.tabs.rename.as_ref().is_some_and(|(id, _)| Surface::Tab(*id) == surface);
        let width = tabs::tab_width(&title, audio.is_some(), renaming);
        (surface, title, audio, width)
    }).collect();
    let spans = tabs::spans(items.iter().map(|item| item.3));
    let control_count = if sheet.is_some() { 2. } else { 3. };
    let available = (width - 1. - 20. - control_count * 24. - (control_count + 1.) * tabs::GAP).max(0.);
    let (strip_width, max_scroll) = tabs::strip_width(&spans, available);
    for (surface, title, audio, tab_width) in items {
        let active = app.ui.panel.active == Some(surface);
        let fg = if active { t.c.contrast_foreground } else { t.c.muted_foreground };
        let mut glyph = el()
            .absolute(2., 2.)
            .size(12., 12.)
            .group_opacity(1., 0.)
            .child(
                match surface {
                    Surface::Browser(tab) => preview::url(app, tab).and_then(|url| favicon::icon(app, &url, 12., 2.)),
                    _ => None,
                }
                .unwrap_or_else(|| icon(tabs::glyph(app, surface), 12.).size(12., 12.).color(fg)),
            );
        if match app.ui.panel.tabs.target(surface) {
            Some(tabs::Target::File(path)) => files::pending_path(app, path),
            _ => surface == Surface::Files && files::pending(app),
        } {
            glyph = glyph.child(el().absolute(8., 8.).size(5., 5.).radius(2.5).fill(t.c.muted_foreground));
        }
        // The hover fill sits on the X's own layer: a `hover` element is the hover group of what
        // it contains, which would keep the glyph swap from following the tab's hover.
        let close_button = el()
            .size(16., 16.)
            .role("button")
            .label(format!("Close {title}"))
            .on_press(move |s: &mut App| close(s, surface))
            .child(glyph)
            .child(
                el().absolute(0., 0.)
                    .size(16., 16.)
                    .radius(4.)
                    .center()
                    .hover(t.c.muted)
                    .group_opacity(0., 1.)
                    .child(icon(&icons::X, 12.).size(12., 12.).color(fg)),
            );
        // Web's speaker on a playing browser tab; a press mutes or unmutes it.
        let speaker = match surface {
            Surface::Browser(tab) => audio.map(|muted| {
                el().size(16., 16.)
                    .radius(4.)
                    .center()
                    .hover(t.c.muted)
                    .role("button")
                    .label(format!("{} {title}", if muted { "Unmute" } else { "Mute" }))
                    .on_press(move |s: &mut App| preview::set_muted(s, tab, !muted))
                    .child(icon(if muted { &icons::VOLUME_OFF } else { &icons::VOLUME_2 }, 12.).size(12., 12.).color(fg))
            }),
            _ => None,
        };
        let tab_spans = spans.clone();
        let name = tabs::label(app, t, surface, title.clone(), fg);
        let tab = row()
                .key(tabs::key(surface))
                .h(24.)
                .w(tab_width)
                .pl(6.)
                .pr(8.)
                .gap(2.)
                .radius(6.)
                .center_y()
                .fill(if active { t.c.accent } else { rune::Color::TRANSPARENT })
                .hover(if active { t.c.accent } else { t.c.accent.alpha_mul(0.6) })
                .hover_group()
                .role("tab")
                .label(title.clone())
                .focusable(true)
                // Web closes a tab on a middle click.
                .pointer_buttons([rune::PointerButton::Middle])
                .on_drag(move |s: &mut App, d: &mut rune::Drag, tx: &mut Tx| tabs::drag(s, surface, d, tx, &tab_spans))
                .on_key(move |s: &mut App, e: &KeyEvent, _: &mut Tx| {
                    let activate_key = e.key == Key::Enter || matches!(&e.key, Key::Text(k) if k == " ");
                    if activate_key { activate(s, surface); }
                    activate_key
                })
                .on_secondary_press(move |s: &mut App, at: rune::PointerPress, _: &mut rune::Tx| s.ui.panel.tab_menu = Some((surface, (at.x, at.y))))
                .child(close_button)
                .children(speaker)
                .child(name);
        let tab = if matches!(app.ui.panel.tabs.target(surface), Some(tabs::Target::Device { .. })) {
            tab.on_double_press(move |s: &mut App| tabs::rename(s, surface))
        } else { tab };
        tabs = tabs.child(tabs::dragged(app, surface, tab));
    }
    let tabs = row().h(51.).gap(4.).pl(8.).pr(12.).center_y()
        .child(tabs::strip(app, tabs, strip_width))
        .children(tabs::overflow(app, t, max_scroll))
        .child(
            el().key("panel-add").child(
                action(t, "Add panel surface", &icons::PLUS)
                    .on_press(|s: &mut App| s.ui.panel.add_menu ^= true)
                    .build(t),
            ),
        )
        .child(el().grow(1.))
        .children(sheet.is_none().then(|| {
            // RightPanelMaximizeControl.
            let (glyph, label) = if maximized(app, viewport) { (&icons::MINIMIZE_2, "Restore panel size") } else { (&icons::MAXIMIZE_2, "Maximize panel") };
            action(t, label, glyph).on_press(toggle_maximized).build(t)
        }))
        .child(
            action(t, "Close right panel", &icons::PANEL_RIGHT)
                .on_press(close_panel)
                .build(t),
        );
    let content = match app.ui.panel.active {
        None => picker(app, t),
        Some(Surface::Diff) => diff::view(app, t, cx.layout_with("panel-diff-refs", |r| r[2])),
        Some(Surface::Files) => files::view(app, t, cx.layout_with("file-editor", |r| r[2]), cx.layout_with("file-image-viewport", |r| [r[2], r[3]])),
        Some(Surface::Tab(id)) => tabs::view(app, cx, t, id, width, overlays),
        Some(Surface::PullRequests) => pull_requests::view(app, t, overlays),
        Some(Surface::Device) => device::view(app, t, cx.layout_with("device-stage", |r| (r[2], r[3])), overlays),
        Some(Surface::Terminal) => col()
            .grow(1.)
            .min_h(0.)
            .child(drawer::surface(app, t, drawer::Owner::Panel, width - 1.)),
        // The app zoom magnifies the page too; Chrome renders at the pixels it ends up on.
        Some(Surface::Browser(tab)) => preview::view(app, t, tab, cx.env().scale * app.ui.desktop.zoom_factor()),
    };
    let edge = el().w(1.).h_full().fill(t.c.border);
    let edge = if sheet.is_some() {
        edge
    } else {
        edge.cursor(rune::Cursor::ResizeColumn).on_drag(|s: &mut App, d, _| {
            if d.phase == Phase::Down {
                d.data = s.ui.panel.width;
            }
            if !d.cancelled() {
                s.ui.panel.width = (d.data - d.dx).clamp(320., 1100.);
            }
        })
    };
    let panel = row().key("right-panel");
    let panel = if maximized(app, viewport) { panel.grow(1.).min_w(0.) } else { panel.w(width) };
    let panel = panel
        .h_full()
        .fill(t.c.background)
        .child(edge)
        .child(
            col()
                .grow(1.)
                .min_w(0.)
                .h_full()
                .child(tabs)
                .child(el().h(1.).w_full().fill(t.c.border))
                .child(content),
        );
    if sheet.is_none() {
        return panel;
    }
    // RightPanelSheet: over the whole window from the right, behind a scrim; a press outside or
    // Escape closes it, as web's Sheet does.
    el().key("right-panel-sheet")
        .pin(Some(0.), Some(0.), Some(0.), Some(0.))
        .child(kit::motion::scrim(t))
        .child(
            panel
                .role("dialog")
                .label("Right panel")
                .pin(None, Some(0.), Some(0.), Some(0.))
                .shadow(16., 48., rune::Color::BLACK.alpha(0.24))
                .modal(true, close_panel),
        )
}
fn scene(app: &mut App, surface: Option<Surface>) {
    (crate::shell::SCENES[1].1)(app);
    app.ui.shell.panel_open = true;
    if let Some(surface) = surface {
        activate(app, surface);
    }
}
fn browser_scene(app: &mut App, kind: &str) {
    scene(app, None);
    let tab = preview::scene(app, kind);
    activate(app, Surface::Browser(tab));
}
fn browser_page_scene(app: &mut App, path: &str, title: &str) {
    scene(app, None);
    if let Some(tab) = preview::scene_page(app, path, title) {
        activate(app, Surface::Browser(tab));
    }
}
pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("panel-picker", |s| scene(s, None)),
    ("browser-annotation", |s| browser_scene(s, "annotation")),
    ("browser-rails", |s| browser_scene(s, "rails")),
    ("browser-device-narrow", |s| {
        browser_scene(s, "rails");
        s.ui.panel.width = 320.;
    }),
    ("browser-stream-dialog", |s| browser_scene(s, "stream-dialog")),
    ("browser-host-setup", |s| browser_scene(s, "host-setup")),
    ("browser-more-menu", |s| browser_scene(s, "more")),
    ("browser-file-chooser", |s| browser_scene(s, "chooser")),
    ("browser-clipboard-prompt", |s| browser_scene(s, "clipboard-prompt")),
    ("browser-external-prompt", |s| browser_scene(s, "external-prompt")),
    ("browser-file-not-shown", |s| browser_scene(s, "file-not-shown")),
    ("browser-server-unreachable", |s| browser_scene(s, "server-unreachable")),
    ("browser-pdf-chrome", |s| browser_page_scene(s, "/reports/q3", "q3")),
    ("browser-auth-chrome", |s| browser_page_scene(s, "/staging", "401 Unauthorized")),
    ("browser-permissions-chrome", |s| browser_page_scene(s, "/permissions", "Permissions ready")),
    ("browser-title-chrome", |s| browser_page_scene(s, "/late-title", "Title set after load")),
    ("browser-popup-chrome", |s| {
        scene(s, None);
        if let Some(tab) = preview::scene_popup(s) {
            activate(s, Surface::Browser(tab));
        }
    }),
    ("panel-launcher-profiles", |s| {
        scene(s, None);
        s.client_settings.values.insert("browserProfiles".into(), json!([{ "id": "work", "name": "Work", "kind": "persistent" }]));
        s.ui.panel.launcher_profiles = true;
    }),
    ("panel-add-profiles", |s| {
        browser_scene(s, "rails");
        s.ui.panel.add_menu = true;
        s.ui.panel.add_profiles = true;
    }),
    ("panel-browser", |s| {
        scene(s, None);
        add(s, Surface::Browser(0));
    }),
    ("panel-diff-empty", |s| scene(s, Some(Surface::Diff))),
    ("panel-diff", |s| {
        scene(s, Some(Surface::Diff));
        diff::fixture(s);
    }),
    ("panel-diff-test-files-shown", |s| {
        scene(s, Some(Surface::Diff));
        diff::test_filter_fixture(s, false);
    }),
    ("panel-diff-test-files-hidden", |s| {
        scene(s, Some(Surface::Diff));
        diff::test_filter_fixture(s, true);
    }),
    ("panel-diff-tree", |s| {
        scene(s, Some(Surface::Diff));
        diff::tree_fixture(s);
    }),
    ("panel-diff-long", |s| {
        scene(s, Some(Surface::Diff));
        diff::long_fixture(s);
    }),
    ("panel-diff-maximized", |s| {
        scene(s, Some(Surface::Diff));
        diff::fixture(s);
        toggle_maximized(s);
    }),
    ("panel-diff-tree-split", |s| {
        scene(s, Some(Surface::Diff));
        diff::tree_fixture(s);
        s.ui.panel.diff.state.set_mode(diff::DiffMode::Split);
    }),
    ("panel-diff-review", |s| {
        scene(s, Some(Surface::Diff));
        diff::review_fixture(s);
    }),
    ("panel-diff-turns", |s| {
        scene(s, Some(Surface::Diff));
        diff::turns_fixture(s);
    }),
    ("git-commit", crate::chat::details::git::commit_scene),
    ("details-pr-conflict", crate::chat::details::pr::conflict_scene),
    ("panel-diff-split", |s| {
        scene(s, Some(Surface::Diff));
        diff::fixture(s);
        s.ui.panel.diff.state.set_mode(diff::DiffMode::Split);
    }),
    ("panel-files", |s| {
        scene(s, Some(Surface::Files));
        files::fixture(s);
    }),
    // A 1440-point window at 173% zoom is 833 points wide: under web's 980 breakpoint.
    ("panel-sheet", |s| {
        scene(s, Some(Surface::Files));
        files::fixture(s);
        s.ui.desktop.scene_zoom(3.0);
    }),
    ("panel-file-markdown", |s| {
        scene(s, Some(Surface::Files));
        files::preview_fixture(s, "README.md", include_str!("../../../../README.md"), true);
    }),
    ("panel-file-table", |s| {
        scene(s, Some(Surface::Files));
        files::preview_fixture(s, "data/models.csv", "provider,model,context,notes\nClaude,Opus 5.5,1000000,\"Default for \"\"hard\"\" turns\"\nClaude,Haiku 5.5,200000,Cheap replays\nCodex,Luna,400000,\nCursor,Auto,128000,\"Multi\nline\"\n", true);
    }),
    ("panel-file-mermaid", |s| {
        scene(s, Some(Surface::Files));
        files::preview_fixture(s, "docs/turn.mmd", "flowchart LR\n  Client -->|command| Orchestrator\n  Orchestrator -->|events| Sink[(Event sink)]\n  Sink --> Projections\n  Sink --> Worker[Effect worker]\n  Worker -->|result command| Orchestrator\n", true);
    }),
    ("panel-file-no-preview", |s| {
        scene(s, Some(Surface::Files));
        files::preview_fixture(s, "docs/spec.pdf", "", false);
    }),
    ("panel-device-list", |s| {
        scene(s, Some(Surface::Device));
        device::fixture(s, json!([]));
    }),
    ("panel-device-webrtc", |s| {
        scene(s, Some(Surface::Device));
        device::fixture(s, json!([{"hostId": "eas", "deviceId": "eas-16", "platform": "ios"}]));
    }),
    ("panel-device-stream", |s| {
        scene(s, Some(Surface::Device));
        device::fixture(s, json!([{"hostId": "local", "deviceId": "ios-16", "platform": "ios"}]));
    }),
    ("panel-device-menu", |s| {
        scene(s, Some(Surface::Device));
        device::fixture(s, json!([{"hostId": "local", "deviceId": "ios-16", "platform": "ios"}]));
        device::open_menu(s);
    }),
    ("panel-add-menu", |s| {
        scene(s, Some(Surface::Device));
        device::fixture(s, json!([]));
        s.ui.panel.add_menu = true;
    }),
    ("panel-device-android", |s| {
        scene(s, Some(Surface::Device));
        device::fixture(s, json!([{"hostId": "local", "deviceId": "pixel", "platform": "android"}]));
    }),
    ("panel-device-3d-model", |s| {
        scene(s, Some(Surface::Device));
        device::fixture(s, json!([{"hostId": "local", "deviceId": "ios-18", "platform": "ios"}]));
    }),
    ("panel-device-fold", |s| {
        scene(s, Some(Surface::Device));
        device::fixture(s, json!([{"hostId": "local", "deviceId": "fold", "platform": "android"}]));
        device::fold_fixture(s);
    }),
    ("panel-device-keyboard", |s| {
        scene(s, Some(Surface::Device));
        device::fixture(s, json!([{"hostId": "local", "deviceId": "ipad-pro", "platform": "ios"}]));
        device::keyboard_fixture(s);
    }),
    ("panel-device-duo", |s| {
        scene(s, Some(Surface::Device));
        device::fixture(s, json!([{"hostId": "local", "deviceId": "duo", "platform": "ios"}]));
        device::duo_fixture(s);
    }),
    ("panel-device-tools", |s| {
        scene(s, Some(Surface::Device));
        device::fixture(s, json!([{"hostId": "local", "deviceId": "ios-16", "platform": "ios"}]));
        device::open_tools(s);
    }),
    ("device-float", |s| {
        scene(s, Some(Surface::Device));
        device::fixture(s, json!([{"hostId": "local", "deviceId": "ios-16", "platform": "ios"}]));
        device::float_fixture(s);
    }),
    ("panel-device-tools-more", |s| {
        scene(s, Some(Surface::Device));
        device::fixture(s, json!([{"hostId": "local", "deviceId": "ios-16", "platform": "ios"}]));
        device::tools_more_fixture(s);
    }),
    ("panel-files-edit", |s| {
        scene(s, Some(Surface::Files));
        files::edit_fixture(s, None, false);
    }),
    ("panel-files-edit-dirty", |s| {
        scene(s, Some(Surface::Files));
        files::edit_fixture(s, Some("  // Reuse the running session first.\n"), false);
    }),
    ("panel-files-edit-error", |s| {
        scene(s, Some(Surface::Files));
        files::edit_fixture(s, Some("  // Reuse the running session first.\n"), true);
    }),
    ("panel-terminal", |s| panel_terminal(s, TERMINAL_DEV)),
    ("panel-terminal-read-only-menu", |s| {
        panel_terminal(s, TERMINAL_DEV);
        terminal::read_only_menu_fixture(s);
    }),
    ("panel-terminal-vim", |s| panel_terminal(s, TERMINAL_VIM)),
    ("panel-terminal-htop", |s| panel_terminal(s, TERMINAL_HTOP)),
    ("terminal-drawer", |s| {
        (crate::shell::SCENES[1].1)(s);
        drawer::fixture(s, drawer::Owner::Drawer, &[(&[("term-1", TERMINAL_DEV)], false)], "term-1");
    }),
    ("terminal-drawer-split", |s| {
        (crate::shell::SCENES[1].1)(s);
        drawer::fixture(
            s,
            drawer::Owner::Drawer,
            &[
                (&[("term-1", TERMINAL_DEV), ("term-2", TERMINAL_HTOP)], false),
                (&[("term-3", TERMINAL_VIM)], false),
            ],
            "term-2",
        );
    }),
    ("terminal-drawer-stacked", |s| {
        (crate::shell::SCENES[1].1)(s);
        drawer::fixture(
            s,
            drawer::Owner::Drawer,
            &[(&[("term-1", TERMINAL_DEV), ("term-2", TERMINAL_VIM)], true)],
            "term-1",
        );
    }),
    ("panel-diff-banners", |s| {
        scene(s, Some(Surface::Diff));
        diff::banners_fixture(s);
    }),
    ("panel-terminal-close", |s| {
        (crate::shell::SCENES[1].1)(s);
        drawer::fixture(s, drawer::Owner::Panel, &[(&[("term-1", TERMINAL_DEV), ("term-2", TERMINAL_VIM)], false)], "term-1");
        s.ui.shell.panel_open = true;
        s.ui.panel.surfaces.push(Surface::Terminal);
        s.ui.panel.active = Some(Surface::Terminal);
        close(s, Surface::Terminal);
    }),
    ("branch-mismatch", |s| {
        (crate::shell::SCENES[1].1)(s);
        let Route::Thread(id) = s.route.clone() else { return };
        if let data::Source::Fixture(state) = &mut s.src
            && let Some(thread) = state.threads.iter_mut().find(|v| v["id"] == id.as_str())
        {
            thread["branch"] = json!("feature/login-redirect");
            thread["worktreePath"] = Value::Null;
        }
        let key = crate::chat::composer::draft_key(s, &Route::Thread(id));
        s.ui.chat.composer.drafts.entry(key).or_default().edit.replace_text("Ship the redirect fix");
    }),
    ("sidebar-terminal-card", |s| {
        (crate::shell::SCENES[1].1)(s);
        let Route::Thread(id) = s.route.clone() else { return };
        terminal::known_fixture(s, &id, &["term-1", "term-2"]);
        s.ui.shell.sidebar.card = Some(id);
    }),
    ("details-previous-worktree", |s| {
        (crate::shell::SCENES[0].1)(s);
        // The shell's "home" scene is a fixture thread; the previous worktree is a draft's choice.
        s.navigate(Route::Home { project_id: None });
        let project = crate::chat::home::draft_project(s).map(|p| data::s(p, "id").to_string()).unwrap_or_default();
        if let data::Source::Fixture(state) = &mut s.src
            && let Some(thread) = state.threads.iter_mut().find(|v| v["projectId"] == project.as_str())
        {
            thread["worktreePath"] = json!("/home/dev/.t3/worktrees/demo/feat-login");
            thread["branch"] = json!("feat/login-redirect");
            thread["archivedAt"] = Value::Null;
        }
        s.ui.chat.details.menu = Some(crate::chat::details::Menu::RunContext);
    }),
    ("details-run-on", crate::chat::details::run_on::scene),
    ("composer-multiple-models", crate::chat::composer::fanout::scene),
    ("composer-fanout-uncertain", crate::chat::composer::fanout::scene_uncertain),
    ("composer-allow-retry", crate::chat::composer::fanout::scene_allow_retry),
    ("delete-worktree-too", crate::chat::details::cleanup::scene),
    ("script-editor-locked", crate::chat::details::scripts::scene_locked),
    ("details-editor-menu", crate::chat::details::scene_editor_menu),
    ("details-branch-menu", crate::chat::details::scene_branch_menu),
    ("details-branch-restore", crate::chat::details::scene_branch_restore),
    ("checkout-pr", |s| {
        (crate::shell::SCENES[1].1)(s);
        crate::chat::details::pr::scene(s);
    }),
    ("panel-pull-requests", |s| pull_requests_scene(s, pull_requests::fixture_links())),
    ("panel-pull-requests-empty", |s| pull_requests_scene(s, json!([]))),
    ("panel-pull-requests-menu", |s| {
        pull_requests_scene(s, pull_requests::fixture_links());
        s.ui.panel.pull_requests.menu = Some(("github.com/acme/app#20".into(), None));
    }),
    ("panel-tabs-files", |s| {
        scene(s, None);
        for path in ["src/main.rs", "src/settings.rs", "src/lib.rs"] {
            open_file(s, path.into());
        }
        files::preview_fixture(s, "src/lib.rs", "pub mod settings;\n", false);
    }),
    ("panel-tabs-terminal", |s| {
        panel_terminal(s, TERMINAL_DEV);
        drawer::fixture(s, drawer::Owner::Panel, &[(&[("term-1", TERMINAL_DEV), ("term-2", TERMINAL_VIM)], false), (&[("term-3", TERMINAL_HTOP)], false)], "term-1");
        sync_terminal_tabs(s);
    }),
    ("panel-tabs-overflow", |s| {
        scene(s, None);
        for path in [
            "src/orchestration/command-handler.rs", "src/orchestration/turn-projection.rs",
            "src/checkpoint/checkpoint-manager.rs", "src/browser/browser-session.rs",
            "src/settings/settings-persistence.rs", "src/preview/preview-attachment.rs",
        ] {
            open_file(s, path.into());
        }
        activate(s, s.ui.panel.surfaces[0]);
        files::preview_fixture(s, "src/orchestration/command-handler.rs", "pub mod commands;\n", false);
        s.ui.panel.tabs.reveal = None;
    }),
    ("panel-tabs-pull-request", |s| {
        scene(s, None);
        let Some(recorded) = crate::pages::fixture("pull-request-detail.json") else { return };
        let entry = recorded["detail"].clone();
        if entry["url"].is_string() {
            open_pull_request(s, entry);
            crate::pages::pr_detail::poll(s);
        }
    }),
    ("panel-tabs-device-rename", |s| {
        scene(s, None);
        device::fixture(s, json!([{"hostId":"local","deviceId":"ios-16","platform":"ios"}, {"hostId":"local","deviceId":"pixel","platform":"android"}]));
        tabs::open(s, tabs::Target::Device { host: "local".into(), device: "pixel".into(), name: "Pixel 9".into(), title: None });
        let id = tabs::open(s, tabs::Target::Device { host: "local".into(), device: "ios-16".into(), name: "iPhone 16 Pro".into(), title: Some("iOS test device".into()) });
        tabs::rename(s, Surface::Tab(id));
    }),
    ("terminal-drawer-close", |s| {
        (crate::shell::SCENES[1].1)(s);
        drawer::fixture(s, drawer::Owner::Drawer, &[(&[("term-1", TERMINAL_DEV)], false)], "term-1");
        drawer::request_close(s, drawer::Owner::Drawer, "term-1");
    }),
];

/// The thread scene with `links` as its linked pull requests, showing the list.
fn pull_requests_scene(app: &mut App, links: Value) {
    (crate::shell::SCENES[1].1)(app);
    let Route::Thread(id) = app.route.clone() else { return };
    if let data::Source::Fixture(state) = &mut app.src
        && let Some(thread) = state.threads.iter_mut().find(|v| v["id"] == id.as_str())
    {
        thread["pullRequests"] = links;
    }
    activate(app, Surface::PullRequests);
}

const TERMINAL_DEV: &str = include_str!("../../fixtures/panel/terminal.json");
const TERMINAL_VIM: &str = include_str!("../../fixtures/panel/terminal-vim.json");
const TERMINAL_HTOP: &str = include_str!("../../fixtures/panel/terminal-htop.json");

fn panel_terminal(app: &mut App, recorded: &str) {
    (crate::shell::SCENES[1].1)(app);
    drawer::fixture(app, drawer::Owner::Panel, &[(&[("term-1", recorded)], false)], "term-1");
    app.ui.shell.panel_open = true;
    if !app.ui.panel.surfaces.contains(&Surface::Terminal) {
        app.ui.panel.surfaces.push(Surface::Terminal);
    }
    app.ui.panel.active = Some(Surface::Terminal);
}

pub(super) fn notice(t: &T, message: &str) -> El {
    col().grow(1.).min_h(0.).center().px(20.).child(
        text(message)
            .font(theme::TEXT_XS)
            .color(t.c.muted_foreground.alpha_mul(0.7)),
    )
}

pub fn overlays(app: &App, t: &T) -> Vec<El> {
    let mut out = vec![];
    mcp_app::overlay(app, t, &mut out);
    preview::pip(app, t, &mut out);
    device::float(app, t, &mut out);
    if app.ui.shell.panel_open && app.ui.panel.active == Some(Surface::Diff) {
        out.extend(diff::overlays(app, t));
    }
    diff::file_menu::overlay(app, t, &mut out);
    terminal::overlays(app, t, &mut out);
    drawer::overlays(app, t, &mut out);
    if app.ui.shell.panel_open {
        if files_showing(app) {
            files::overlays(app, t, &mut out);
        } else if matches!(active_target(app), Some(tabs::Target::Attachment(_))) {
            file_viewers::overlays(app, t, &mut out);
        }
        preview::overlays(app, t, &mut out);
        if app.ui.panel.add_menu {
            add_menu(app, t, &mut out);
        }
        if app.ui.panel.launcher_profiles {
            launcher_profiles(app, t, &mut out);
        }
        if let Some((surface, at)) = app.ui.panel.tab_menu {
            tab_menu(app, t, surface, at, &mut out);
        }
    }
    out
}

/// Web's tab context menu: mute for a browser tab, then the close actions.
fn tab_menu(app: &App, t: &T, surface: Surface, at: (f32, f32), out: &mut Vec<El>) {
    let done = |s: &mut App| s.ui.panel.tab_menu = None;
    let surfaces = &app.ui.panel.surfaces;
    let Some(index) = surfaces.iter().position(|&s| s == surface) else { return };
    let mut menu = kit::Menu::new("panel-tab-menu").native(true).width(198.).open(true).at(at.0, at.1).on_close(done);
    if let Surface::Browser(tab) = surface {
        let muted = preview::muted(app, tab);
        menu = menu.item(kit::MenuItem::new(if muted { "Unmute tab" } else { "Mute tab" }).on_select(move |s: &mut App| {
            done(s);
            preview::set_muted(s, tab, !muted);
        }));
    }
    if matches!(app.ui.panel.tabs.target(surface), Some(tabs::Target::Device { .. })) {
        menu = menu.item(kit::MenuItem::new("Rename").on_select(move |s: &mut App| {
            done(s);
            tabs::rename(s, surface);
        }));
    }
    let close_where = move |keep: fn(usize, usize) -> bool| {
        move |s: &mut App| {
            done(s);
            let doomed: Vec<Surface> = s.ui.panel.surfaces.iter().enumerate().filter(|&(i, _)| !keep(i, index)).map(|(_, &v)| v).collect();
            let mut groups = Vec::new();
            for surface in doomed {
                if let Some(tabs::Target::Terminal(group)) = s.ui.panel.tabs.target(surface) {
                    groups.push(group.clone());
                } else {
                    close(s, surface);
                }
            }
            drawer::request_close_groups(s, groups);
        }
    };
    menu.item(kit::MenuItem::new("Close").on_select(move |s: &mut App| {
        done(s);
        close(s, surface);
    }))
    .item(kit::MenuItem::new("Close others").disabled(surfaces.len() <= 1).on_select(close_where(|i, index| i == index)))
    .item(kit::MenuItem::new("Close to the right").disabled(index + 1 >= surfaces.len()).on_select(close_where(|i, index| i <= index)))
    .item(kit::MenuItem::new("Close all").on_select(close_where(|_, _| false)))
    .overlay(&kit::theme(t), out);
}

/// One item per browser profile, each opening a tab in it.
fn profile_items(app: &App, close: fn(&mut App)) -> Vec<kit::MenuItem> {
    crate::settings::browser_profiles::pickable(app)
        .into_iter()
        .map(|(id, name)| {
            kit::MenuItem::new(name).on_select(move |s: &mut App| {
                close(s);
                add_in_profile(s, &id);
            })
        })
        .collect()
}

/// The launcher Browser row's profile menu.
fn launcher_profiles(app: &App, t: &T, out: &mut Vec<El>) {
    let mut menu = kit::Menu::new("panel-launcher-profiles-menu")
        .anchor("panel-launcher-profiles")
        .placement(kit::Placement::BottomEnd)
        .min_width(160.)
        .open(true)
        .on_close(|s: &mut App| s.ui.panel.launcher_profiles = false);
    for item in profile_items(app, |s| s.ui.panel.launcher_profiles = false) {
        menu = menu.item(item);
    }
    menu.overlay(&kit::theme(t), out);
}

/// The tab strip's + menu (web's add-surface menu).
fn add_menu(app: &App, t: &T, out: &mut Vec<El>) {
    let mut menu = kit::Menu::new("panel-add-menu")
        .anchor("panel-add")
        .placement(kit::Placement::BottomStart)
        .min_width(224.)
        .open(true)
        .on_close(|s: &mut App| {
            s.ui.panel.add_menu = false;
            s.ui.panel.add_profiles = false;
        });
    for (launcher, key) in LAUNCH {
        let mut item = kit::MenuItem::new(launcher.title())
            .icon(launcher.glyph())
            .shortcut(key)
            .disabled(!launcher.available(app))
            .on_select(move |s: &mut App| {
                s.ui.panel.add_menu = false;
                s.ui.panel.add_profiles = false;
                launcher.open(s);
            });
        // Web's Browser submenu: a new tab in any profile.
        if matches!(launcher, Launcher::Surface(Surface::Browser(_))) {
            let profiles = profile_items(app, |s| {
                s.ui.panel.add_menu = false;
                s.ui.panel.add_profiles = false;
            });
            item = item.submenu(profiles.into_iter().map(kit::MenuEntry::item).collect(), app.ui.panel.add_profiles).on_submenu_change(|s: &mut App, open| s.ui.panel.add_profiles = open);
        }
        menu = menu.item(item);
    }
    menu.overlay(&kit::theme(t), out);
}

#[cfg(test)]
mod sheet_tests {
    use super::*;

    #[test]
    fn the_panel_becomes_a_sheet_at_webs_breakpoint() {
        let near = |viewport: f32, want: f32| sheet_width(viewport).is_some_and(|w| (w - want).abs() < 0.01);
        assert_eq!(sheet_width(1280.), None);
        assert_eq!(sheet_width(981.), None);
        assert!(near(980., 411.6));
        assert!(near(800., 336.));
        assert!(near(761., 320.), "never narrower than 20rem above 760");
        assert!(near(760., 384.));
        assert!(near(400., 352.));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_launchers_profile_chevron_opens_its_menu_without_a_default_tab() {
        fn view(s: &App, _: &mut rune::Cx<App>) -> El {
            el().size(480., 640.).child(picker(s, &crate::theme::theme(true)))
        }
        let mut rt = rune::Runtime::new(App::new(), view, (480., 640.), rune::Env::new(true, 1.));
        rt.frame(0.);
        let chevron = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("panel-launcher-profiles")).expect("the Browser row's chevron").rect;
        let click = |rt: &mut rune::Runtime<App>, (x, y): (f32, f32)| {
            rt.pointer_move(x, y);
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(0.);
        };
        let y = chevron[1] + chevron[3] / 2.;
        click(&mut rt, (chevron[0] + chevron[2] / 2., y));
        assert!(rt.state.ui.panel.launcher_profiles, "the chevron opens the profile menu");
        assert!(rt.state.ui.panel.surfaces.is_empty(), "and opens no tab");
        // The rest of the row still opens a tab in the default profile.
        click(&mut rt, (chevron[0] - 120., y));
        assert!(matches!(rt.state.ui.panel.surfaces[..], [Surface::Browser(_)]), "the row opens one tab");
    }

    #[test]
    fn overflowing_tabs_render_both_arrows_without_scrolling_first() {
        fn view(s: &App, cx: &mut rune::Cx<App>) -> El {
            super::view(s, cx, &crate::theme::theme(true), &mut Vec::new())
        }
        let mut app = App::new();
        let scene = SCENES.iter().find(|(name, _)| *name == "panel-tabs-overflow").unwrap().1;
        scene(&mut app);
        let mut rt = rune::Runtime::new(app, view, (540., 640.), rune::Env::new(true, 1.));
        rt.frame(0.);
        let nodes = rt.elements();
        for key in ["panel-tabs-scroll-left", "panel-tabs-scroll-right"] {
            assert!(nodes.iter().any(|e| e.key.as_deref() == Some(key) && e.rect[2] > 0.), "{key} is present on the first frame");
        }
    }

    #[test]
    fn each_thread_gets_its_own_panel_back() {
        let mut app = App::new();
        let ids: Vec<String> = app.state().threads.iter().take(2).map(|v| data::s(v, "id").to_string()).collect();
        app.navigate(Route::Thread(ids[0].clone()));
        switch_thread(&mut app);
        app.ui.panel.surfaces = vec![Surface::Browser(1), Surface::Terminal];
        app.ui.panel.active = Some(Surface::Browser(1));
        app.ui.shell.panel_open = true;
        app.ui.panel.browser.pip = Some(1);

        app.navigate(Route::Thread(ids[1].clone()));
        assert!(switch_thread(&mut app));
        assert!(app.ui.panel.surfaces.is_empty() && app.ui.panel.active.is_none() && app.ui.panel.browser.pip.is_none());
        assert!(!app.ui.shell.panel_open);
        app.ui.panel.surfaces = vec![Surface::Diff];

        app.navigate(Route::Thread(ids[0].clone()));
        assert!(switch_thread(&mut app));
        assert!(app.ui.panel.surfaces == vec![Surface::Browser(1), Surface::Terminal]);
        assert!(app.ui.panel.active == Some(Surface::Browser(1)) && app.ui.shell.panel_open);
        assert_eq!(app.ui.panel.browser.pip, Some(1));
        assert!(!switch_thread(&mut app));

        // A restart brings each thread's surfaces back, less its browser tabs.
        app.ui.panel.diff.tree_w = Some(300.0);
        let saved = app.ui.panel.to_json(app.ui.shell.panel_open);
        let mut restarted = App::new();
        restarted.ui.panel.restore(&saved);
        assert_eq!(restarted.ui.panel.diff.tree_w, Some(300.0), "the Changes tree keeps its width");
        restarted.ui.panel.restore(&json!({"byThreadKey": {"bad": {"surfaces": ["browser:1", 7]}, "dupes": {"surfaces": ["diff", "files", "diff"], "activeSurfaceId": "terminal"}}}));
        restarted.navigate(Route::Thread(ids[0].clone()));
        assert!(switch_thread(&mut restarted));
        assert!(restarted.ui.panel.surfaces == vec![Surface::Terminal] && restarted.ui.panel.active == Some(Surface::Terminal));
        assert!(restarted.ui.shell.panel_open);
        let panel = &restarted.ui.panel;
        assert!(panel.stash[&ids[1]].surfaces == vec![Surface::Diff] && !panel.stash[&ids[1]].open);
        assert!(!panel.stash.contains_key("bad"));
        assert!(panel.stash["dupes"].surfaces == vec![Surface::Diff, Surface::Files] && panel.stash["dupes"].active == Some(Surface::Files));

        open_file(&mut app, "src/first.rs".into());
        let first = app.ui.panel.active.unwrap();
        open_file(&mut app, "src/second.rs".into());
        let second = app.ui.panel.active.unwrap();
        open_file(&mut app, "src/first.rs/".into());
        assert_eq!(app.ui.panel.active, Some(first));
        assert_eq!(&app.ui.panel.surfaces[2..], &[first, second]);
        app.ui.panel.surfaces.swap(2, 3);
        let saved = app.ui.panel.to_json(true);
        let rows = saved["byThreadKey"][&ids[0]]["surfaces"].as_array().unwrap();
        assert_eq!(rows[1]["path"], "src/second.rs");
        assert_eq!(rows[2]["path"], "src/first.rs");

        let mut saved_tabs = App::new();
        saved_tabs.ui.panel.restore(&saved);
        saved_tabs.navigate(Route::Thread(ids[0].clone()));
        assert!(switch_thread(&mut saved_tabs));
        let targets: Vec<_> = saved_tabs.ui.panel.surfaces.iter().filter_map(|&surface| saved_tabs.ui.panel.tabs.target(surface)).cloned().collect();
        assert_eq!(targets, vec![tabs::Target::File("src/second.rs".into()), tabs::Target::File("src/first.rs".into())]);
        assert!(matches!(active_target(&saved_tabs), Some(tabs::Target::File(path)) if path == "src/first.rs"));

        app.navigate(Route::Home { project_id: None });
        assert!(switch_thread(&mut app));
        app.ui.panel.surfaces = vec![Surface::Browser(2)];
        adopt(&mut app, "thread:launched");
        app.navigate(Route::Thread("thread:launched".into()));
        assert!(!switch_thread(&mut app));
        assert!(app.ui.panel.surfaces == vec![Surface::Browser(2)]);
    }

    #[test]
    fn a_new_thread_draft_roots_the_files_surface_at_its_project() {
        let mut app = App::new();
        let project = app.state().projects[0].clone();
        app.navigate(Route::Home { project_id: Some(data::s(&project, "id").to_string()) });
        assert_eq!(context(&app), None);
        assert_eq!(workspace(&app).as_deref(), Some(data::s(&project, "workspaceRoot")));
        let key = crate::chat::composer::home_key(data::s(&project, "id"));
        assert_eq!(diff_context(&app), Some((key, data::s(&project, "workspaceRoot").to_string())));
        app.navigate(Route::PullRequests);
        assert_eq!(workspace(&app), None);
        assert_eq!(diff_context(&app), None);
    }

    #[test]
    fn configured_mod_key_uses_the_platform_primary_modifier() {
        let mut app = App::new();
        let mut state = data::empty_state();
        state.config = Some(
            serde_json::json!({"keybindings":[{"command":"terminal.toggle","shortcut":{"key":"j","modKey":true}}]}),
        );
        app.src = data::Source::Fixture(state);
        let mut event = KeyEvent::from(Key::Command('j'));
        event.mods.cmd = true;
        event.mods.ctrl = !cfg!(target_os = "macos");
        assert!(shortcut(&app, &event, "terminal.toggle"));
        event.mods.shift = true;
        assert!(!shortcut(&app, &event, "terminal.toggle"));
    }

    #[test]
    fn when_clauses_pick_between_bindings_on_one_key() {
        let mut app = App::new();
        let mut state = data::empty_state();
        let id = |name: &str| serde_json::json!({"type": "identifier", "name": name});
        let not = |node| serde_json::json!({"type": "not", "node": node});
        let d = serde_json::json!({"key": "d", "modKey": true});
        state.config = Some(serde_json::json!({"keybindings": [
            {"command": "terminal.split", "shortcut": d, "whenAst": id("terminalFocus")},
            {"command": "diff.toggle", "shortcut": d, "whenAst": not(id("terminalFocus"))},
            {"command": "rightPanel.new", "shortcut": {"key": "t", "modKey": true}, "whenAst": {"type": "and", "left": id("rightPanelOpen"), "right": not(id("terminalFocus"))}},
            {"command": "rightPanel.toggleMaximized", "shortcut": {"key": "m", "modKey": true, "shiftKey": true}},
        ]}));
        app.src = data::Source::Fixture(state);
        let press = |key: char| {
            let mut event = KeyEvent::from(Key::Command(key));
            event.mods.cmd = true;
            event.mods.ctrl = !cfg!(target_os = "macos");
            event
        };
        assert!(shortcut(&app, &press('d'), "diff.toggle"));
        assert!(!shortcut(&app, &press('d'), "terminal.split"), "terminalFocus is false without a focused terminal");
        assert!(!shortcut(&app, &press('t'), "rightPanel.new"), "the panel is closed");
        app.ui.shell.panel_open = true;
        assert!(shortcut(&app, &press('t'), "rightPanel.new"));
        // rightPanel.toggleMaximized, which has no default: per thread, and only for an open inline panel.
        let mut max = press('m');
        max.mods.shift = true;
        assert!(shortcut(&app, &max, "rightPanel.toggleMaximized"));
        app.route = crate::Route::Thread("t1".into());
        toggle_maximized(&mut app);
        assert!(maximized(&app, Some(1440.)) && !maximized(&app, Some(700.)), "a narrow window's sheet never maximizes");
        app.route = crate::Route::Thread("t2".into());
        assert!(!maximized(&app, Some(1440.)), "each thread keeps its own");
        app.route = crate::Route::Thread("t1".into());
        app.ui.shell.panel_open = false;
        toggle_maximized(&mut app);
        assert!(!maximized(&app, Some(1440.)), "a closed panel is never maximized");
        app.ui.shell.panel_open = true;
        assert!(maximized(&app, Some(1440.)), "reopening restores it, as web's flag outlives the close");
        toggle_maximized(&mut app);
        assert!(!maximized(&app, Some(1440.)));
        let context = |name: &str| name == "terminalFocus";
        let bindings = app.state().config.as_ref().unwrap()["keybindings"].as_array().unwrap().clone();
        assert_eq!(resolve(&bindings, &press('d'), &context), Some("terminal.split"));
        assert!(when(&id("true"), &context) && !when(&id("unknown"), &context));
        // Keys the recorder saves by name, and shifted punctuation by its unshifted key.
        let named = serde_json::json!([
            {"command": "a.space", "shortcut": {"key": "space", "modKey": true}},
            {"command": "a.home", "shortcut": {"key": "home", "modKey": true}},
            {"command": "a.help", "shortcut": {"key": "/", "modKey": true, "shiftKey": true}},
        ]);
        let named = named.as_array().unwrap();
        let with = |key: Key, shift: bool| {
            let mut event = KeyEvent::from(key);
            event.mods.cmd = true;
            event.mods.ctrl = !cfg!(target_os = "macos");
            event.mods.shift = shift;
            event
        };
        assert_eq!(resolve(named, &with(Key::Command(' '), false), &context), Some("a.space"));
        assert_eq!(resolve(named, &with(Key::Home, false), &context), Some("a.home"));
        assert_eq!(resolve(named, &with(Key::Command('?'), true), &context), Some("a.help"));
    }

    #[test]
    fn other_layouts_match_by_the_keys_physical_position() {
        assert_eq!(event_keys("/", Some("Digit7")), ["/", "7"], "German ⌘⇧7");
        assert_eq!(event_keys("к", Some("KeyR")), ["к", "r"], "Russian letters fall back to the key's place");
        assert_eq!(event_keys("q", Some("KeyA")), ["q"], "AZERTY letters keep their own letter");
        assert_eq!(event_keys("ö", Some("Semicolon")), ["ö", ";"]);
        assert_eq!(event_keys("space", Some("Space")), ["space"]);
        let bindings = serde_json::json!([{"command": "seven", "shortcut": {"key": "7", "modKey": true, "shiftKey": true}}]);
        let mut e = KeyEvent::from(Key::Command('/'));
        e.mods.cmd = true;
        e.mods.ctrl = !cfg!(target_os = "macos");
        e.mods.shift = true;
        let context = |_: &str| false;
        assert_eq!(resolve(bindings.as_array().unwrap(), &e, &context), None, "without a physical code only the typed key counts");
        KEY_CONTEXT.with(|c| *c.borrow_mut() = Some((e.key.clone(), Some("Digit7".into()), true)));
        assert_eq!(resolve(bindings.as_array().unwrap(), &e, &context), Some("seven"));
        KEY_CONTEXT.with(|c| *c.borrow_mut() = None);
    }

    #[test]
    fn editable_focus_is_a_focused_text_field_unless_the_undo_notice_claims_the_key() {
        let app = App::new();
        let e = KeyEvent::from(Key::Command('z'));
        for (editable, expected) in [(false, false), (true, true)] {
            KEY_CONTEXT.with(|c| *c.borrow_mut() = Some((e.key.clone(), None, editable)));
            assert_eq!(when_context(&app, "editableFocus"), expected, "a focused button is not a text field");
        }
        let bindings = serde_json::json!([{"command": "thread.undo", "shortcut": {"key": "z", "modKey": true}, "whenAst": {"type": "not", "node": {"type": "identifier", "name": "editableFocus"}}}]);
        let mut cmd_z = e.clone();
        cmd_z.mods.cmd = true;
        cmd_z.mods.ctrl = !cfg!(target_os = "macos");
        let context = |name: &str| when_context(&app, name);
        assert_eq!(resolve(bindings.as_array().unwrap(), &cmd_z, &context), None, "the field keeps its own undo");
        KEY_CONTEXT.with(|c| *c.borrow_mut() = Some((e.key.clone(), None, false)));
        assert_eq!(resolve(bindings.as_array().unwrap(), &cmd_z, &context), Some("thread.undo"));
        KEY_CONTEXT.with(|c| *c.borrow_mut() = None);
    }

    #[test]
    fn preview_focus_takes_the_page_zoom_keys() {
        let mut app = App::new();
        let mut state = data::empty_state();
        let id = |name: &str| serde_json::json!({"type": "identifier", "name": name});
        let not = |node| serde_json::json!({"type": "not", "node": node});
        state.config = Some(serde_json::json!({"keybindings": [
            {"command": "preview.zoomOut", "shortcut": {"key": "-", "modKey": true}, "whenAst": id("previewFocus")},
            {"command": "chat.find", "shortcut": {"key": "f", "modKey": true}, "whenAst": not(id("previewFocus"))},
        ]}));
        app.src = data::Source::Fixture(state);
        let press = |key: char| {
            let mut event = KeyEvent::from(Key::Command(key));
            event.mods.cmd = true;
            event.mods.ctrl = !cfg!(target_os = "macos");
            event
        };
        assert!(!shortcut(&app, &press('-'), "preview.zoomOut"), "the app zooms while the page lacks focus");
        assert!(shortcut(&app, &press('f'), "chat.find"));
        app.ui.panel.browser.focused = true;
        assert!(shortcut(&app, &press('-'), "preview.zoomOut"));
        assert!(!shortcut(&app, &press('f'), "chat.find"), "web leaves mod+f to the page");
    }
}
