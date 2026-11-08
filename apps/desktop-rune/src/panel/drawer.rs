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

pub const DEFAULT_HEIGHT: f32 = 280.;
const MIN_HEIGHT: f32 = 180.;
pub const MAX_PER_GROUP: usize = 4;
const DEFAULT_ID: &str = "term-1";

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub id: String,
    pub ids: Vec<String>,
    pub vertical: bool,
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
        self.active = id.into();
        true
    }
    pub fn activate(&mut self, id: &str) -> bool {
        let known = self.ids().any(|v| v == id);
        if known {
            self.active = id.into();
        }
        known
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
        }
        self.groups.retain(|g| !g.ids.is_empty());
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
    /// The terminal waiting on the close confirm.
    confirm: Option<(Owner, String, String)>,
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
    fn owner_of(&self, thread: &str, id: &str) -> Option<Owner> {
        [Owner::Drawer, Owner::Panel]
            .into_iter()
            .find(|&o| self.groups(o, thread).is_some_and(|g| g.ids().any(|v| v == id)))
    }
    pub fn is_open(&self, thread: &str) -> bool {
        self.threads.get(thread).is_some_and(|t| t.open)
    }
}

/// The current thread, its terminal cwd and its worktree.
fn launch_context(app: &App) -> Option<(String, String, Option<String>)> {
    let (thread, cwd) = super::context(app)?;
    let worktree = app
        .state()
        .threads
        .iter()
        .find(|v| v["id"] == thread.as_str())
        .and_then(|v| v["worktreePath"].as_str())
        .filter(|v| !v.is_empty())
        .map(String::from);
    Some((thread, cwd, worktree))
}

fn current_thread(app: &App) -> Option<String> {
    match &app.route {
        Route::Thread(id) => Some(id.clone()),
        _ => None,
    }
}

/// Every id the thread's drawer and panel hold, so a new one never shares a PTY.
fn allocate(app: &App, thread: &str) -> String {
    let ui = &app.ui.panel.drawer;
    let ids: Vec<String> = [Owner::Drawer, Owner::Panel]
        .into_iter()
        .filter_map(|o| ui.groups(o, thread))
        .flat_map(|g| g.ids().cloned())
        .collect();
    terminal::next_id(&ids)
}

fn start(app: &mut App, thread: &str, id: &str, focus: bool) {
    if let Some((_, cwd, worktree)) = launch_context(app).filter(|(t, ..)| t == thread) {
        terminal::ensure(app, thread, id, &cwd, worktree.as_deref());
    }
    if focus {
        terminal::focus(app, thread, id);
    }
}

/// `terminal.toggle`: opening a drawer with no terminals starts `term-1`.
pub fn toggle(app: &mut App) {
    let Some(thread) = current_thread(app) else {
        return;
    };
    let state = app.ui.panel.drawer.threads.entry(thread.clone()).or_default();
    if state.open {
        state.open = false;
    } else {
        state.open = true;
        if state.groups.is_empty() {
            state.groups.add(DEFAULT_ID);
        }
        let active = state.groups.active.clone();
        start(app, &thread, &active, true);
    }
    sync_topbar(app);
}

pub fn sync_topbar(app: &mut App) {
    app.ui.chat.topbar.terminal_open =
        current_thread(app).is_some_and(|t| app.ui.panel.drawer.is_open(&t));
}

pub fn new_terminal(app: &mut App, owner: Owner) {
    let Some(thread) = current_thread(app) else {
        return;
    };
    let id = allocate(app, &thread);
    app.ui.panel.drawer.groups_mut(owner, &thread).add(&id);
    if owner == Owner::Drawer {
        app.ui.panel.drawer.threads.get_mut(&thread).unwrap().open = true;
    }
    start(app, &thread, &id, true);
    sync_topbar(app);
}

/// Types `input` into an idle drawer terminal, the active one first, else a new one
/// (`findReusableProjectScriptTerminal`); panel-owned terminals are never reused.
pub fn run(app: &mut App, cwd: &str, worktree: Option<&str>, input: String) {
    let Some(thread) = current_thread(app) else {
        return;
    };
    let ui = &app.ui.panel;
    let panel: Vec<&String> = ui.drawer.panel.get(&thread).map(|g| g.ids().collect()).unwrap_or_default();
    let idle = |id: &&String| !panel.contains(id) && !ui.terminal.get(&thread, id).is_some_and(|s| s.running);
    let reusable = ui.drawer.threads.get(&thread).and_then(|t| {
        let ids: Vec<&String> = t.groups.ids().filter(idle).collect();
        ids.iter().find(|id| ***id == t.groups.active).or(ids.first()).map(|id| id.to_string())
    });
    let id = reusable.unwrap_or_else(|| allocate(app, &thread));
    let state = app.ui.panel.drawer.threads.entry(thread.clone()).or_default();
    state.open = true;
    if !state.groups.ids().any(|v| *v == id) {
        state.groups.add(&id);
    }
    state.groups.activate(&id);
    terminal::run(app, &thread, &id, cwd, worktree, input);
    terminal::focus(app, &thread, &id);
    sync_topbar(app);
}

pub fn split(app: &mut App, owner: Owner, vertical: bool) {
    let Some(thread) = current_thread(app) else {
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
    let Some(thread) = current_thread(app) else {
        return;
    };
    let empty = app
        .ui
        .panel
        .drawer
        .groups(Owner::Panel, &thread)
        .is_none_or(Groups::is_empty);
    if empty {
        let id = allocate(app, &thread);
        app.ui.panel.drawer.groups_mut(Owner::Panel, &thread).add(&id);
    }
    let ids: Vec<String> = app.ui.panel.drawer.panel[&thread].ids().cloned().collect();
    for id in &ids {
        start(app, &thread, id, false);
    }
}

/// Closing the panel's Terminal surface stops its terminals.
pub fn close_panel(app: &mut App) {
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
    if let Some(thread) = current_thread(app) {
        app.ui.panel.drawer.confirm = Some((owner, thread, id.into()));
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
    match command {
        Some("terminal.toggle") => toggle(app),
        Some("terminal.split") => split(app, owner, false),
        Some("terminal.splitVertical") => split(app, owner, true),
        Some("terminal.new") => new_terminal(app, owner),
        Some("terminal.close") => request_close(app, owner, id),
        Some(_) => app.ui.shell.panel_open = !app.ui.shell.panel_open,
        None => return false,
    }
    true
}

/// Keeps the topbar toggle in step with the drawer of the thread on screen.
pub fn poll(app: &mut App) -> bool {
    let before = app.ui.chat.topbar.terminal_open;
    sync_topbar(app);
    before != app.ui.chat.topbar.terminal_open
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

fn actions(t: &T, owner: Owner, full: bool, sidebar: bool) -> El {
    let suffix = |base: &str| {
        if full {
            format!("{base} (max {MAX_PER_GROUP} per group)")
        } else {
            base.to_string()
        }
    };
    let buttons = [
        action(
            t,
            &suffix("Split Terminal Horizontally"),
            &icons::SQUARE_SPLIT_HORIZONTAL,
            full,
            sidebar,
            move |s| split(s, owner, false),
        ),
        action(
            t,
            &suffix("Split Terminal Vertically"),
            &icons::SQUARE_SPLIT_VERTICAL,
            full,
            sidebar,
            move |s| split(s, owner, true),
        ),
        action(t, "New Terminal", &icons::PLUS, false, sidebar, move |s| {
            new_terminal(s, owner)
        }),
        action(t, "Close Terminal", &icons::TRASH_2, false, sidebar, move |s| {
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
            let label = app
                .ui
                .panel
                .terminal
                .get(thread, id)
                .map_or_else(|| terminal::label_for(id), |s| s.title());
            let fg = if active {
                t.c.foreground
            } else {
                t.c.muted_foreground
            };
            let (close_id, select_id) = (id.clone(), id.clone());
            let mut entry = row()
                .w_full()
                .h(24.)
                .gap(2.)
                .pl(6.)
                .pr(8.)
                .center_y()
                .radius(6.)
                .child(
                    el().size(12., 12.)
                        .role("button")
                        .label(format!("Close {label}"))
                        .focusable(true)
                        .child(icon(&icons::SQUARE_TERMINAL, 12.).size(12., 12.).color(fg))
                        .on_press(move |s: &mut App| request_close(s, owner, &close_id)),
                )
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
                .child(actions(t, owner, groups.split_full(), true)),
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

/// The drawer or panel body for one owner's terminals.
pub fn surface(app: &App, t: &T, owner: Owner, width: f32) -> El {
    let thread = current_thread(app).unwrap_or_default();
    let empty = Groups::default();
    let groups = app.ui.panel.drawer.groups(owner, &thread).unwrap_or(&empty);
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
                kit::Button::new("New Terminal")
                    .variant("outline")
                    .size("xs")
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
            actions(t, owner, groups.split_full(), false)
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
                    }),
            ),
    )
}

pub fn overlays(app: &App, t: &T, out: &mut Vec<El>) {
    let Some((_, thread, id)) = app.ui.panel.drawer.confirm.clone() else {
        return;
    };
    let label = app
        .ui
        .panel
        .terminal
        .get(&thread, &id)
        .map_or_else(|| terminal::label_for(&id), |s| s.title());
    let cancel = kit::Button::new("Cancel")
        .variant("outline")
        .on_press(|s: &mut App| s.ui.panel.drawer.confirm = None)
        .build(t);
    let confirm = kit::Button::new("Confirm")
        .variant("destructive")
        .on_press(|s: &mut App| {
            if let Some((owner, thread, id)) = s.ui.panel.drawer.confirm.take() {
                close_now(s, owner, &thread, &id);
            }
        })
        .build(t);
    kit::Dialog::new("terminal-close", format!("Close terminal \"{label}\"?"))
        .alert()
        .width(448.)
        .open(true)
        .description("This stops the running process and clears its history.")
        .child(row().w_full().gap(8.).justify_end().child(cancel).child(confirm))
        .on_close(|s: &mut App| s.ui.panel.drawer.confirm = None)
        .build(t, out);
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
}
