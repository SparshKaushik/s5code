//! Workspace surfaces; wire payloads stay identical to the web client's RPCs.
pub mod browser;
mod diff;
pub mod drawer;
mod files;
mod preview;
mod terminal;
mod vt;

use crate::{
    App, Route, data, icons, kit,
    theme::{self, AlphaMul, T},
};
use rune::{Cx, El, Key, KeyEvent, Phase, Tx, Waker, col, el, icon, row, text};
use serde_json::{Map, Value, json};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Browser(browser::TabId),
    Terminal,
    Files,
    Diff,
}
impl Surface {
    fn title(self) -> &'static str {
        match self {
            Self::Browser(_) => "Browser",
            Self::Terminal => "Terminal",
            Self::Files => "Files",
            Self::Diff => "Diff",
        }
    }
    fn glyph(self) -> &'static rune::Icon {
        match self {
            Self::Browser(_) => &icons::GLOBE,
            Self::Terminal => &icons::SQUARE_TERMINAL,
            Self::Files => &icons::FILES,
            Self::Diff => &icons::FILE_DIFF,
        }
    }
}
pub struct Ui {
    width: f32,
    surfaces: Vec<Surface>,
    active: Option<Surface>,
    pub diff: diff::Ui,
    files: files::Ui,
    terminal: terminal::Ui,
    drawer: drawer::Ui,
    browser: preview::Ui,
    add_menu: bool,
    thread: String,
    /// Each thread's surfaces, as web's right panel state is per thread; browser tabs stay
    /// alive while their thread is in the background.
    stash: std::collections::HashMap<String, Stash>,
    stash_key: String,
    /// What `right-panel.json` holds; `None` until the first poll reads it.
    saved: Option<Value>,
    pub waker: Option<Waker>,
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
            surfaces: vec![],
            active: None,
            diff: Default::default(),
            files: Default::default(),
            terminal: Default::default(),
            drawer: Default::default(),
            browser: Default::default(),
            add_menu: false,
            thread: String::new(),
            stash: Default::default(),
            stash_key: String::new(),
            saved: None,
            waker: None,
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
/// The launcher's entries; Browser always opens a new tab, as web's.
const LAUNCH: [(Surface, &str); 4] = [
    (Surface::Browser(0), "B"),
    (Surface::Terminal, "T"),
    (Surface::Files, "F"),
    (Surface::Diff, "D"),
];
fn add(app: &mut App, surface: Surface) {
    match surface {
        Surface::Browser(_) => {
            let tab = preview::open(app);
            activate(app, Surface::Browser(tab));
        }
        _ => activate(app, surface),
    }
}
fn activate(app: &mut App, surface: Surface) {
    app.ui.shell.panel_open = true;
    if !app.ui.panel.surfaces.contains(&surface) {
        app.ui.panel.surfaces.push(surface);
    }
    app.ui.panel.active = Some(surface);
    match surface {
        Surface::Diff => diff::load(app),
        Surface::Files => files::load(app, "".into()),
        Surface::Terminal => drawer::open_panel(app),
        _ => {}
    }
}
/// The thread details card's "Changes" row (`openChangesFromThreadPanel`).
pub fn open_changes(app: &mut App) {
    diff::open_changes(app);
    activate(app, Surface::Diff);
}
/// Shows `url` in the browser surface (a project script's `autoOpenPreview`).
pub fn open_preview(app: &mut App, url: &str) {
    let tab = preview::open(app);
    activate(app, Surface::Browser(tab));
    preview::navigate(app, tab, url);
}
/// The terminal holds the keyboard, so `!terminalFocus` shortcuts stand down.
pub fn terminal_active(app: &App) -> bool {
    app.ui.shell.panel_open && app.ui.panel.active == Some(Surface::Terminal)
}
fn close(app: &mut App, surface: Surface) {
    match surface {
        Surface::Terminal => drawer::close_panel(app),
        Surface::Files => files::flush(app),
        Surface::Browser(tab) => preview::close(app, tab),
        _ => {}
    }
    remove_surface(app, surface);
}
fn remove_surface(app: &mut App, surface: Surface) {
    app.ui.panel.surfaces.retain(|v| *v != surface);
    if app.ui.panel.active == Some(surface) {
        app.ui.panel.active = app.ui.panel.surfaces.last().copied();
    }
}
/// Opens a workspace file in the Files surface (the palette's "Go to file").
pub fn open_file(app: &mut App, path: String) {
    app.ui.shell.panel_open = true;
    if !app.ui.panel.surfaces.contains(&Surface::Files) {
        app.ui.panel.surfaces.push(Surface::Files);
    }
    app.ui.panel.active = Some(Surface::Files);
    files::open(app, path);
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
            Self::Browser(_) => None,
        }
    }
    fn from_saved_id(id: &str) -> Option<Self> {
        [Self::Terminal, Self::Files, Self::Diff].into_iter().find(|s| s.saved_id() == Some(id))
    }
}

impl Ui {
    /// Web's `t3code:right-panel-state`: each thread's open flag, surfaces and active surface.
    fn to_json(&self, open: bool) -> Value {
        let entry = |surfaces: &[Surface], active: Option<Surface>, open: bool| {
            let ids: Vec<&str> = surfaces.iter().filter_map(|s| s.saved_id()).collect();
            (!ids.is_empty()).then(|| json!({"isOpen": open, "activeSurfaceId": active.and_then(Surface::saved_id), "surfaces": ids}))
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
        json!({"version": 1, "byThreadKey": threads})
    }

    /// Fills the stash, so the first switch to a thread brings its saved panel back.
    fn restore(&mut self, v: &Value) {
        for (key, entry) in v["byThreadKey"].as_object().into_iter().flatten() {
            if key.is_empty() || *key == self.stash_key || self.stash.contains_key(key) {
                continue;
            }
            let mut surfaces = vec![];
            for surface in entry["surfaces"].as_array().into_iter().flatten().filter_map(|v| v.as_str().and_then(Surface::from_saved_id)) {
                if !surfaces.contains(&surface) {
                    surfaces.push(surface);
                }
            }
            let Some(&last) = surfaces.last() else { continue };
            let active = entry["activeSurfaceId"].as_str().and_then(Surface::from_saved_id).filter(|s| surfaces.contains(s)).unwrap_or(last);
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
    app.ui.shell.panel_open = next.open;
    true
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
        let thread = context(app).map(|(id, _)| id).unwrap_or_default();
        if app.ui.panel.thread != thread {
            app.ui.panel.thread = thread;
            diff::reset(app);
            files::reset(app);
            if app.ui.shell.panel_open {
                match app.ui.panel.active {
                    Some(Surface::Diff) => diff::load(app),
                    Some(Surface::Files) => files::load(app, String::new()),
                    _ => {}
                }
            }
            context_changed = true;
        }
        if app.ui.panel.active == Some(Surface::Terminal) {
            drawer::open_panel(app);
        }
    }

    let changed = diff::poll(app)
        | diff::reveal(app, tx)
        | files::poll(app, tx)
        | terminal::poll(app, tx)
        | drawer::poll(app)
        | preview::poll(app, tx);
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
    let key = match &e.key {
        // Shift turns "[" into "{" on the key event; bindings name the unshifted key.
        Key::Command('{') => "[".to_string(),
        Key::Command('}') => "]".to_string(),
        Key::Command(c) => c.to_string(),
        Key::Text(v) => v.clone(),
        _ => String::new(),
    };
    bindings
        .iter()
        .rev()
        .find(|v| {
            let k = &v["shortcut"];
            let yes = |name: &str| k[name].as_bool().unwrap_or(false);
            (v["whenAst"].is_null() || when(&v["whenAst"], context))
                && k["key"].as_str().is_some_and(|v| v.eq_ignore_ascii_case(&key))
                && e.mods.cmd == (yes("modKey") || yes("metaKey") || (!cfg!(target_os = "macos") && yes("ctrlKey")))
                && e.mods.ctrl == (yes("ctrlKey") || (!cfg!(target_os = "macos") && yes("modKey")))
                && e.mods.shift == yes("shiftKey")
                && e.mods.alt == yes("altKey")
        })
        .and_then(|v| v["command"].as_str())
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
        "usagePageOpen" => matches!(app.route, crate::Route::Usage),
        _ => false,
    }
}
pub fn on_key(app: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if shortcut(app, e, "rightPanel.toggle") {
        app.ui.shell.panel_open = !app.ui.shell.panel_open;
        return true;
    }
    if app.ui.shell.panel_open && shortcut(app, e, "rightPanel.new") {
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
    if context(app).is_some() && shortcut(app, e, "diff.toggle") {
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
                add(app, surface);
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
fn picker(app: &App, t: &T) -> El {
    let mut list = col().w(320.).gap(2.).child(
        text("Open a surface")
            .font(theme::SM_MEDIUM)
            .align(rune::Align::Center)
            .color(t.c.contrast_foreground)
            .mb(10.),
    );
    for (surface, key) in LAUNCH {
        let available = matches!(surface, Surface::Browser(_)) || context(app).is_some();
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
                .label(surface.title())
                .disabled(!available)
                .child(
                    icon(surface.glyph(), 16.)
                        .size(16., 16.)
                        .color(t.c.contrast_foreground),
                )
                .child(
                    text(surface.title())
                        .font(theme::TEXT_SM)
                        .color(t.c.contrast_foreground)
                        .grow(1.),
                )
                .child(text(key).font(theme::MONO_XS).color(t.c.muted_foreground))
                .on_press(move |s: &mut App| add(s, surface)),
        );
    }
    for (title, glyph, key) in [
        ("Pull request", &icons::GIT_PULL_REQUEST_ARROW, "P"),
        ("Linked pull requests", &icons::LINK, "L"),
        ("Device", &icons::SMARTPHONE, "M"),
    ] {
        list = list.child(
            row()
                .h(32.)
                .px(10.)
                .gap(10.)
                .center_y()
                .opacity(0.5)
                .role("button")
                .label(title)
                .disabled(true)
                .child(
                    icon(glyph, 16.)
                        .size(16., 16.)
                        .color(t.c.contrast_foreground),
                )
                .child(
                    text(title)
                        .font(theme::TEXT_SM)
                        .color(t.c.contrast_foreground)
                        .grow(1.),
                )
                .child(text(key).font(theme::MONO_XS).color(t.c.muted_foreground)),
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
pub fn view(app: &App, cx: &mut Cx<App>, t: &T) -> El {
    let width = app
        .ui
        .panel
        .width
        .min(cx.layout_with("shell", |r| r[2] * 0.7).unwrap_or(1000.));
    // Web's RightPanelTabs: 24pt tabs whose surface icon turns into the close button on hover.
    let mut tabs = row().h(51.).gap(4.).pl(8.).pr(12.).center_y().role("tablist");
    for &surface in &app.ui.panel.surfaces {
        let active = app.ui.panel.active == Some(surface);
        let title = match surface {
            Surface::Browser(tab) => preview::title(app, tab),
            _ => surface.title().to_string(),
        };
        let fg = if active { t.c.contrast_foreground } else { t.c.muted_foreground };
        let mut glyph = el()
            .absolute(2., 2.)
            .size(12., 12.)
            .group_opacity(1., 0.)
            .child(icon(surface.glyph(), 12.).size(12., 12.).color(fg));
        if surface == Surface::Files && files::pending(app) {
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
        tabs = tabs.child(
            row()
                .h(24.)
                .max_w(144.)
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
                .on_press(move |s: &mut App| s.ui.panel.active = Some(surface))
                .child(close_button)
                .child(
                    text(title)
                        .font(theme::TEXT_XS)
                        .color(fg)
                        .group_color(t.c.contrast_foreground)
                        .truncate()
                        .shrink(),
                ),
        );
    }
    tabs = tabs
        .child(
            el().key("panel-add").child(
                action(t, "Add panel surface", &icons::PLUS)
                    .on_press(|s: &mut App| s.ui.panel.add_menu ^= true)
                    .build(t),
            ),
        )
        .child(el().grow(1.))
        .child(
            action(t, "Close right panel", &icons::PANEL_RIGHT)
                .on_press(|s: &mut App| s.ui.shell.panel_open = false)
                .build(t),
        );
    let content = match app.ui.panel.active {
        None => picker(app, t),
        Some(Surface::Diff) => diff::view(app, t, cx.layout_with("panel-diff-refs", |r| r[2])),
        Some(Surface::Files) => files::view(app, t),
        Some(Surface::Terminal) => col()
            .grow(1.)
            .min_h(0.)
            .child(drawer::surface(app, t, drawer::Owner::Panel, width - 1.)),
        Some(Surface::Browser(tab)) => preview::view(app, t, tab, cx.env().scale),
    };
    row()
        .key("right-panel")
        .w(width)
        .h_full()
        .fill(t.c.background)
        .child(
            el().w(1.)
                .h_full()
                .fill(t.c.border)
                .cursor(rune::Cursor::ResizeColumn)
                .on_drag(|s: &mut App, d, _| {
                    if d.phase == Phase::Down {
                        d.data = s.ui.panel.width;
                    }
                    if !d.cancelled() {
                        s.ui.panel.width = (d.data - d.dx).clamp(320., 1100.);
                    }
                }),
        )
        .child(
            col()
                .grow(1.)
                .min_w(0.)
                .h_full()
                .child(tabs)
                .child(el().h(1.).w_full().fill(t.c.border))
                .child(content),
        )
}
fn scene(app: &mut App, surface: Option<Surface>) {
    (crate::shell::SCENES[1].1)(app);
    app.ui.shell.panel_open = true;
    if let Some(surface) = surface {
        activate(app, surface);
    }
}
pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("panel-picker", |s| scene(s, None)),
    ("panel-browser", |s| {
        scene(s, None);
        add(s, Surface::Browser(0));
    }),
    ("panel-diff-empty", |s| scene(s, Some(Surface::Diff))),
    ("panel-diff", |s| {
        scene(s, Some(Surface::Diff));
        diff::fixture(s);
    }),
    ("panel-diff-split", |s| {
        scene(s, Some(Surface::Diff));
        diff::fixture(s);
        s.ui.panel.diff.state.set_mode(diff::DiffMode::Split);
    }),
    ("panel-files", |s| {
        scene(s, Some(Surface::Files));
        files::fixture(s);
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
    ("terminal-drawer-close", |s| {
        (crate::shell::SCENES[1].1)(s);
        drawer::fixture(s, drawer::Owner::Drawer, &[(&[("term-1", TERMINAL_DEV)], false)], "term-1");
        drawer::request_close(s, drawer::Owner::Drawer, "term-1");
    }),
];

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
    preview::pip(app, t, &mut out);
    if app.ui.shell.panel_open && app.ui.panel.active == Some(Surface::Diff) {
        out.extend(diff::overlays(app, t));
    }
    terminal::overlays(app, t, &mut out);
    drawer::overlays(app, t, &mut out);
    if app.ui.shell.panel_open {
        if app.ui.panel.active == Some(Surface::Files) {
            files::overlays(app, t, &mut out);
        }
        preview::overlays(app, t, &mut out);
        if app.ui.panel.add_menu {
            add_menu(app, t, &mut out);
        }
    }
    out
}

/// The tab strip's + menu (web's add-surface menu).
fn add_menu(app: &App, t: &T, out: &mut Vec<El>) {
    let mut menu = kit::Menu::new("panel-add-menu")
        .anchor("panel-add")
        .placement(kit::Placement::BottomStart)
        .min_width(224.)
        .open(true)
        .on_close(|s: &mut App| s.ui.panel.add_menu = false);
    for (surface, key) in LAUNCH {
        let available = matches!(surface, Surface::Browser(_)) || context(app).is_some();
        menu = menu.item(
            kit::MenuItem::new(surface.title())
                .icon(surface.glyph())
                .shortcut(key)
                .disabled(!available)
                .on_select(move |s: &mut App| {
                    s.ui.panel.add_menu = false;
                    add(s, surface);
                }),
        );
    }
    for (title, glyph, key) in [
        ("Pull request", &icons::GIT_PULL_REQUEST_ARROW, "P"),
        ("Linked pull requests", &icons::LINK, "L"),
        ("Device", &icons::SMARTPHONE, "M"),
    ] {
        menu = menu.item(kit::MenuItem::new(title).icon(glyph).shortcut(key).disabled(true));
    }
    menu.overlay(&kit::theme(t), out);
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let saved = app.ui.panel.to_json(app.ui.shell.panel_open);
        let mut restarted = App::new();
        restarted.ui.panel.restore(&saved);
        restarted.ui.panel.restore(&json!({"byThreadKey": {"bad": {"surfaces": ["browser:1", 7]}, "dupes": {"surfaces": ["diff", "files", "diff"], "activeSurfaceId": "terminal"}}}));
        restarted.navigate(Route::Thread(ids[0].clone()));
        assert!(switch_thread(&mut restarted));
        assert!(restarted.ui.panel.surfaces == vec![Surface::Terminal] && restarted.ui.panel.active == Some(Surface::Terminal));
        assert!(restarted.ui.shell.panel_open);
        let panel = &restarted.ui.panel;
        assert!(panel.stash[&ids[1]].surfaces == vec![Surface::Diff] && !panel.stash[&ids[1]].open);
        assert!(!panel.stash.contains_key("bad"));
        assert!(panel.stash["dupes"].surfaces == vec![Surface::Diff, Surface::Files] && panel.stash["dupes"].active == Some(Surface::Files));
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
        let context = |name: &str| name == "terminalFocus";
        let bindings = app.state().config.as_ref().unwrap()["keybindings"].as_array().unwrap().clone();
        assert_eq!(resolve(&bindings, &press('d'), &context), Some("terminal.split"));
        assert!(when(&id("true"), &context) && !when(&id("unknown"), &context));
    }
}
