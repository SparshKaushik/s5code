//! Sidebar v2 (apps/web `Sidebar.tsx`): brand header, search, pinned / active thread cards, the
//! Working / Snoozed / Settled shelves, and the footer. Rows drag between sections and open a
//! context menu; the rules live in `logic`, the drawing in `chrome`, `row` and `menu`.

mod brand;
mod chrome;
pub mod logic;
mod menu;
mod row;

use std::collections::{HashMap, HashSet};

use rune::{Cx, El, Key, KeyEvent, TextEdit, Tx, col, el, row};
use serde_json::{Value, json};

use self::logic::{Item, Keys, Marker, Plan, Section, Sections, Thread};
use crate::theme::{T, layout};
use crate::{App, Route};

/// Pointer travel before a press on a row becomes a drag (`SIDEBAR_DRAG_DISTANCE`).
const DRAG_DISTANCE: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shelf {
    Working,
    Snoozed,
    Settled,
}

/// A row being dragged: where it started and how far the pointer has moved, in window points.
#[derive(Clone, Debug, PartialEq)]
pub struct Dragging {
    pub key: String,
    pub section: Section,
    pub dy: f32,
}

/// A client-side write the server has not echoed yet, dropped once the row's `updatedAt` moves.
#[derive(Clone, Debug, Default)]
pub struct Patch {
    pub seen_updated_at: i64,
    pub pinned: Option<bool>,
    pub pin_key: Option<Option<String>>,
    pub active_key: Option<Option<String>>,
    pub settled: Option<bool>,
    pub snoozed_until: Option<Option<i64>>,
    pub archived: bool,
    pub title: Option<String>,
}

#[derive(Default)]
pub struct Ui {
    pub collapsed: bool,
    pub search: TextEdit,
    /// Highlighted search result.
    pub highlight: usize,
    pub expanded: HashSet<Shelf>,
    pub hover: Option<String>,
    /// Rows picked with Cmd/Shift-click for bulk actions.
    pub selected: HashSet<String>,
    pub anchor: Option<String>,
    pub drag: Option<Dragging>,
    pub menu: Option<menu::Menu>,
    pub patches: HashMap<String, Patch>,
    pub back: Vec<Route>,
    pub forward: Vec<Route>,
    /// The row whose title is being edited in place, and the edit.
    pub rename: Option<(String, TextEdit)>,
    /// The thread a delete confirmation is open for (`confirmThreadDelete`).
    pub delete: Option<String>,
    /// Project ids the list is filtered to (the web's `projectScopeKeys`); empty shows all.
    pub project_scope: Vec<String>,
    pub scope_open: bool,
}

pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("sidebar-home", scene_home),
    ("sidebar-selected", |s| {
        scene_home(s);
        if let Some(id) = thread_titled(s, "Replay showcase") {
            s.ui.shell.sidebar.selected.insert(id);
        }
        if let Some(id) = thread_titled(s, "Todo list replay") {
            s.ui.shell.sidebar.selected.insert(id);
        }
    }),
    // Pair with `--script` moving the pointer onto a row: `[{"t":0.1,"type":"move","x":120,"y":330}]`.
    ("sidebar-hover", |s| scene_on(s, "Replay showcase")),
    ("sidebar-rename", |s| {
        scene_on(s, "Replay showcase");
        if let Some(id) = thread_titled(s, "Todo list replay") {
            start_rename(s, &id);
        }
    }),
    ("sidebar-delete", |s| {
        scene_on(s, "Replay showcase");
        s.ui.shell.sidebar.delete = thread_titled(s, "Todo list replay");
    }),
    ("sidebar-scope", |s| {
        scene_on(s, "Replay showcase");
        s.ui.shell.sidebar.scope_open = true;
    }),
    ("sidebar-shelves", |s| {
        scene_home(s);
        let ids: Vec<String> = ["Docs site navigation ideas", "Approval flow replay"].iter().filter_map(|t| thread_titled(s, t)).collect();
        if let [settled, snoozed] = ids.as_slice() {
            let now = s.now_ms;
            let ui = &mut s.ui.shell.sidebar;
            ui.patches.insert(settled.clone(), Patch { seen_updated_at: i64::MIN, settled: Some(true), ..Default::default() });
            ui.patches.insert(snoozed.clone(), Patch { seen_updated_at: i64::MIN, snoozed_until: Some(Some(now + 3 * 3_600_000)), ..Default::default() });
        }
        if let Some(id) = thread_titled(s, "Sidebar v2 pinned and active sections") {
            s.ui.shell.sidebar.patches.insert(id, Patch { seen_updated_at: i64::MIN, pinned: Some(true), pin_key: Some(Some("n".into())), ..Default::default() });
        }
        s.ui.shell.sidebar.expanded.extend([Shelf::Snoozed, Shelf::Settled]);
    }),
    ("sidebar-search", |s| {
        scene_home(s);
        let ui = &mut s.ui.shell.sidebar;
        ui.search = TextEdit::new("replay");
        ui.search.anchor = 6;
        ui.search.focus = 6;
        ui.search.focused = true;
    }),
    ("sidebar-menu", |s| {
        scene_on(s, "Replay showcase");
        if let Some(id) = thread_titled(s, "Replay showcase") {
            s.ui.shell.sidebar.menu = Some(menu::Menu::new(id, 121.0, 340.0));
        }
    }),
    ("sidebar-drag", |s| {
        scene_home(s);
        if let Some(id) = thread_titled(s, "Todo list replay") {
            s.ui.shell.sidebar.drag = Some(Dragging { key: id, section: Section::Active, dy: -250.0 });
        }
    }),
    ("sidebar-light", |s| {
        scene_home(s);
        s.appearance = crate::Appearance::Light;
    }),
];

fn thread_titled(s: &App, title: &str) -> Option<String> {
    s.state().threads.iter().find(|t| t["title"].as_str().is_some_and(|x| x.starts_with(title))).map(|t| crate::data::s(t, "id").to_string())
}

/// The web's `/` redirects to the newest thread; the reference shots open on "New thread".
fn scene_home(s: &mut App) {
    scene_on(s, "New thread");
}

fn scene_on(s: &mut App, title: &str) {
    crate::scene_clock(s);
    if let Some(id) = thread_titled(s, title) {
        s.route = Route::Thread(id);
    }
}

/// Everything a frame needs, derived once from server state plus pending client writes.
pub struct Model {
    pub threads: Vec<Thread>,
    pub sections: Sections,
    pub projects: HashMap<String, String>,
    pub drivers: HashMap<String, String>,
    pub route: Option<String>,
    pub now: i64,
    pub working_shelf: bool,
    pub updates: usize,
}

impl Model {
    pub fn build(s: &App) -> Model {
        let state = s.state();
        let ui = &s.ui.shell.sidebar;
        let scope = &ui.project_scope;
        let threads = state
            .threads
            .iter()
            .map(|v| patched(Thread::from_shell(v), &ui.patches))
            .filter(|t| scope.is_empty() || scope.contains(&t.project_id))
            .collect::<Vec<_>>();
        let config = state.config.as_ref();
        let working_shelf = config.and_then(|c| c["settings"]["sidebarWorkingShelfEnabled"].as_bool()).unwrap_or(true);
        let providers = config.and_then(|c| c["providers"].as_array()).map(Vec::as_slice).unwrap_or(&[]);
        Model {
            sections: logic::partition(&threads, s.now_ms, working_shelf),
            threads,
            projects: state.projects.iter().map(|p| (crate::data::s(p, "id").to_string(), crate::data::s(p, "title").to_string())).collect(),
            drivers: providers.iter().map(|p| (crate::data::s(p, "instanceId").to_string(), crate::data::s(p, "driver").to_string())).collect(),
            route: match s.frame_route() {
                Route::Thread(id) => Some(id.clone()),
                _ => None,
            },
            now: s.now_ms,
            working_shelf,
            updates: providers.iter().filter(|p| p["versionAdvisory"]["status"] == "behind_latest").count(),
        }
    }

    pub fn thread(&self, id: &str) -> Option<&Thread> {
        self.threads.iter().find(|t| t.id == id)
    }

    pub fn project(&self, t: &Thread) -> &str {
        self.projects.get(&t.project_id).map(String::as_str).unwrap_or("")
    }

    pub fn section_of(&self, id: &str) -> Option<Section> {
        let i = self.threads.iter().position(|t| t.id == id)?;
        [Section::Pinned, Section::Active, Section::Working, Section::Snoozed, Section::Settled].into_iter().find(|s| self.sections.of(*s).contains(&i))
    }

    /// The drag list: markers and rows in display order (`sidebarListItems`).
    pub fn items(&self, ui: &Ui) -> Vec<Item> {
        let s = &self.sections;
        let row = |i: &usize, section| Item::Thread { key: self.threads[*i].id.clone(), section };
        let mut items = vec![Item::Marker(Marker::PinnedHeader)];
        items.extend(s.pinned.iter().map(|i| row(i, Section::Pinned)));
        items.push(Item::Marker(Marker::PinnedDivider));
        items.push(Item::Marker(Marker::ActivePlaceholder));
        items.extend(s.active.iter().map(|i| row(i, Section::Active)));
        let shelf = |items: &mut Vec<Item>, marker, rows: &[usize], section, shelf| {
            items.push(Item::Marker(marker));
            for i in rows {
                if ui.expanded.contains(&shelf) || self.route.as_deref() == Some(self.threads[*i].id.as_str()) {
                    items.push(row(i, section));
                }
            }
        };
        if !s.working.is_empty() {
            shelf(&mut items, Marker::WorkingHeader, &s.working, Section::Working, Shelf::Working);
        }
        if !s.snoozed.is_empty() {
            shelf(&mut items, Marker::SnoozedHeader, &s.snoozed, Section::Snoozed, Shelf::Snoozed);
        }
        items.push(Item::Marker(Marker::SettledHeader));
        items.push(Item::Marker(Marker::SettledPlaceholder));
        for i in &s.settled {
            if ui.expanded.contains(&Shelf::Settled) || self.route.as_deref() == Some(self.threads[*i].id.as_str()) {
                items.push(row(i, Section::Settled));
            }
        }
        items
    }

    /// Thread ids in the order the list shows them, for keyboard stepping.
    pub fn visible_ids(&self, ui: &Ui) -> Vec<String> {
        self.items(ui)
            .into_iter()
            .filter_map(|i| match i {
                Item::Thread { key, .. } => Some(key),
                Item::Marker(_) => None,
            })
            .collect()
    }

    fn keys(&self, rows: &[usize], key: fn(&Thread) -> &Option<String>) -> Keys {
        rows.iter().map(|i| (self.threads[*i].id.clone(), key(&self.threads[*i]).clone())).collect()
    }

    /// `planSidebarThreadDrop` for this model.
    pub fn plan(&self, active: &str, section: Section, target: &logic::DropTarget) -> Plan {
        let ids = |rows: &[usize]| rows.iter().map(|i| self.threads[*i].id.clone()).collect::<Vec<_>>();
        let (pinned_order, active_order) = (ids(&self.sections.pinned), ids(&self.sections.active));
        let (mut pinned_keys, mut active_keys) = (self.keys(&self.sections.pinned, |t| &t.pin_key), self.keys(&self.sections.active, |t| &t.active_key));
        // A row joining a section brings its retained key, if any (the web's keysById spans all rows).
        if let Some(t) = self.thread(active) {
            pinned_keys.entry(t.id.clone()).or_insert_with(|| t.pin_key.clone());
            active_keys.entry(t.id.clone()).or_insert_with(|| t.active_key.clone());
        }
        logic::plan_drop(&logic::DropInput {
            active,
            section,
            target,
            pinned_order: &pinned_order,
            pinned_keys: &pinned_keys,
            pinned_writable: None,
            active_order: &active_order,
            active_keys: &active_keys,
            active_writable: None,
            time_ordered: self.working_shelf,
        })
    }
}

fn patched(mut t: Thread, patches: &HashMap<String, Patch>) -> Thread {
    let Some(p) = patches.get(&t.id) else { return t };
    if p.seen_updated_at != i64::MIN && p.seen_updated_at != t.updated_at {
        return t;
    }
    if let Some(v) = p.pinned {
        t.pinned = v;
    }
    if let Some(v) = &p.pin_key {
        t.pin_key = v.clone();
    }
    if let Some(v) = &p.active_key {
        t.active_key = v.clone();
    }
    if let Some(v) = p.settled {
        t.settled = v;
    }
    if let Some(v) = p.snoozed_until {
        t.snoozed_until = v;
        t.snoozed_at = v.map(|_| t.updated_at);
    }
    t.archived |= p.archived;
    if let Some(v) = &p.title {
        t.title = v.clone();
    }
    t
}

// --- Commands ---------------------------------------------------------------------------------

/// Sends one orchestration command (the web's `thread.*` dispatches); fixtures only patch.
fn dispatch(s: &mut App, kind: &str, id: &str, extra: Value) {
    if let Some(net) = s.src.net() {
        let mut cmd = json!({ "type": kind, "commandId": s5_net::uuid(), "threadId": id });
        if let (Some(obj), Value::Object(more)) = (cmd.as_object_mut(), extra) {
            obj.extend(more);
        }
        net.dispatch(cmd);
    }
}

fn patch(s: &mut App, id: &str, f: impl FnOnce(&mut Patch)) {
    let seen = s.state().threads.iter().find(|t| t["id"] == id).and_then(|t| crate::data::ts(t, "updatedAt")).unwrap_or(0);
    let p = s.ui.shell.sidebar.patches.entry(id.to_string()).or_default();
    if p.seen_updated_at != seen && p.seen_updated_at != i64::MIN {
        *p = Patch::default();
    }
    if p.seen_updated_at != i64::MIN {
        p.seen_updated_at = seen;
    }
    f(p);
}

fn iso(ms: i64) -> String {
    let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    // Civil date from days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000)
}

pub fn pin(s: &mut App, id: &str, key: Option<String>) {
    dispatch(s, "thread.pin", id, key.as_ref().map_or(json!({}), |k| json!({ "orderKey": k })));
    patch(s, id, |p| {
        p.pinned = Some(true);
        p.pin_key = Some(key);
        p.settled = Some(false);
    });
}

pub fn unpin(s: &mut App, id: &str) {
    dispatch(s, "thread.unpin", id, json!({}));
    patch(s, id, |p| {
        p.pinned = Some(false);
        p.pin_key = Some(None);
    });
}

pub fn settle(s: &mut App, id: &str) {
    dispatch(s, "thread.settle", id, json!({ "settledAt": iso(crate::data::now_ms()) }));
    patch(s, id, |p| {
        p.settled = Some(true);
        p.pinned = Some(false);
        p.snoozed_until = Some(None);
    });
}

pub fn unsettle(s: &mut App, id: &str) {
    dispatch(s, "thread.unsettle", id, json!({ "reason": "user" }));
    patch(s, id, |p| p.settled = Some(false));
}

pub fn snooze(s: &mut App, id: &str, until: i64) {
    dispatch(s, "thread.snooze", id, json!({ "snoozedUntil": iso(until) }));
    patch(s, id, |p| p.snoozed_until = Some(Some(until)));
}

pub fn unsnooze(s: &mut App, id: &str) {
    dispatch(s, "thread.unsnooze", id, json!({ "reason": "user" }));
    patch(s, id, |p| p.snoozed_until = Some(None));
}

pub fn archive(s: &mut App, id: &str) {
    dispatch(s, "thread.archive", id, json!({}));
    patch(s, id, |p| p.archived = true);
}

fn client_flag(s: &App, key: &str) -> bool {
    s.client_settings.values.get(key).or_else(|| crate::settings::client::defaults().get(key)).and_then(Value::as_bool).unwrap_or(false)
}

/// "Delete": asks first when `confirmThreadDelete` is on, as `useThreadActionMenu` does.
pub fn request_delete(s: &mut App, id: &str) {
    if client_flag(s, "confirmThreadDelete") {
        s.ui.shell.sidebar.delete = Some(id.to_string());
    } else {
        delete(s, id);
    }
}

pub fn delete(s: &mut App, id: &str) {
    s.ui.shell.sidebar.delete = None;
    dispatch(s, "thread.delete", id, json!({}));
    patch(s, id, |p| p.archived = true);
    if s.route == Route::Thread(id.to_string()) {
        go(s, Route::Home { project_id: None });
    }
}

/// Opens the row's inline title editor (menu "Rename thread", double-click, F2).
pub fn start_rename(s: &mut App, id: &str) {
    let Some(title) = Model::build(s).thread(id).map(|t| t.title.clone()) else { return };
    let mut edit = TextEdit::new(title.clone());
    edit.set_selection(0, title.len());
    edit.focused = true;
    s.ui.shell.sidebar.rename = Some((id.to_string(), edit));
}

/// ThreadRenameInput commits on blur: focus moved off the editor.
pub fn poll(s: &mut App) -> bool {
    if s.ui.shell.sidebar.rename.as_ref().is_some_and(|(_, e)| !e.focused) {
        commit_rename(s);
        return true;
    }
    false
}

pub fn commit_rename(s: &mut App) {
    let Some((id, edit)) = s.ui.shell.sidebar.rename.take() else { return };
    let next = edit.text.trim().to_string();
    let current = Model::build(s).thread(&id).map(|t| t.title.clone()).unwrap_or_default();
    if next.is_empty() || next == current {
        return;
    }
    dispatch(s, "thread.metadata.update", &id, json!({ "title": next }));
    patch(s, &id, |p| p.title = Some(next));
}

/// The delete confirmation, as the web's destructive `dialogs.confirm`.
fn delete_dialog(s: &App, model: &Model, t: &T, overlays: &mut Vec<El>) {
    let Some(id) = s.ui.shell.sidebar.delete.clone() else { return };
    let title = model.thread(&id).map(|t| t.title.clone()).unwrap_or_default();
    let cancel = crate::kit::Button::new("Cancel").variant("outline").on_press(|s: &mut App| s.ui.shell.sidebar.delete = None).build(t);
    let confirm = crate::kit::Button::new("Delete").variant("destructive").on_press(move |s: &mut App| delete(s, &id)).build(t);
    crate::kit::Dialog::new("sb-delete", format!("Delete thread \"{title}\"?"))
        .alert()
        .width(448.0)
        .open(true)
        .description("This permanently clears conversation history for this thread.")
        .child(row().w_full().gap(8.0).justify_end().child(cancel).child(confirm))
        .on_close(|s: &mut App| s.ui.shell.sidebar.delete = None)
        .build(t, overlays);
}

fn reorder(s: &mut App, kind: &str, assignments: &[(String, String)]) {
    for (id, key) in assignments {
        dispatch(s, kind, id, json!({ "orderKey": key }));
        let key = Some(key.clone());
        patch(s, id, |p| if kind == "thread.pin.reorder" { p.pin_key = Some(key) } else { p.active_key = Some(key) });
    }
}

/// Runs a drop plan as the web's `handleDragEnd` does: lifecycle first, then order writes.
pub fn apply_plan(s: &mut App, id: &str, plan: Plan) {
    match plan {
        Plan::None => {}
        Plan::Settle => settle(s, id),
        Plan::Pin { key, extra, .. } => {
            pin(s, id, key);
            reorder(s, "thread.pin.reorder", &extra);
        }
        Plan::ReorderPinned { assignments, .. } => reorder(s, "thread.pin.reorder", &assignments),
        Plan::MoveActive { assignments, unpin: u, unsettle: us, unsnooze: un, .. } => {
            if u {
                unpin(s, id);
            }
            if us {
                unsettle(s, id);
            }
            if un {
                unsnooze(s, id);
            }
            reorder(s, "thread.active.reorder", &assignments);
        }
    }
}

// --- Navigation -------------------------------------------------------------------------------

pub fn go(s: &mut App, route: Route) {
    if s.route == route {
        return;
    }
    let ui = &mut s.ui.shell.sidebar;
    ui.back.push(s.route.clone());
    ui.forward.clear();
    s.navigate(route);
}

pub fn go_back(s: &mut App) {
    if let Some(r) = s.ui.shell.sidebar.back.pop() {
        s.ui.shell.sidebar.forward.push(s.route.clone());
        s.navigate(r);
    }
}

pub fn go_forward(s: &mut App) {
    if let Some(r) = s.ui.shell.sidebar.forward.pop() {
        s.ui.shell.sidebar.back.push(s.route.clone());
        s.navigate(r);
    }
}

pub fn open_thread(s: &mut App, id: &str) {
    let ui = &mut s.ui.shell.sidebar;
    ui.selected.clear();
    ui.anchor = Some(id.to_string());
    go(s, Route::Thread(id.to_string()));
}

pub fn new_thread(s: &mut App) {
    go(s, Route::Home { project_id: None });
}

fn clear_search(s: &mut App) {
    let ui = &mut s.ui.shell.sidebar;
    ui.search = TextEdit::default();
    ui.highlight = 0;
}

/// Window-level keys the sidebar owns (`sidebar.toggle`, `chat.new`, `thread.previous/next`,
/// `thread.jump.N`), and Escape for its menu, selection and search.
pub fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    let m = e.mods;
    if s.ui.shell.sidebar.menu.is_some() {
        return menu::on_key(s, e, tx);
    }
    match &e.key {
        Key::Command('b') if !m.shift => s.ui.shell.sidebar.collapsed ^= true,
        Key::Command('n') if !m.shift => new_thread(s),
        Key::Command('o') if m.shift => new_thread(s),
        Key::Command(c @ ('[' | ']' | '{' | '}')) if m.shift => step(s, if matches!(c, '[' | '{') { -1 } else { 1 }),
        Key::Command(c @ '1'..='9') if !m.shift => {
            let ids = Model::build(s).visible_ids(&s.ui.shell.sidebar);
            if let Some(id) = ids.get(*c as usize - '1' as usize) {
                open_thread(s, &id.clone());
            }
        }
        Key::Escape if !s.ui.shell.sidebar.selected.is_empty() => s.ui.shell.sidebar.selected.clear(),
        Key::Escape if s.ui.shell.sidebar.drag.is_some() => s.ui.shell.sidebar.drag = None,
        _ => return false,
    }
    true
}

fn step(s: &mut App, by: i32) {
    let ids = Model::build(s).visible_ids(&s.ui.shell.sidebar);
    if ids.is_empty() {
        return;
    }
    let at = match &s.route {
        Route::Thread(id) => ids.iter().position(|x| x == id),
        _ => None,
    };
    let next = match at {
        Some(i) => (i as i32 + by).rem_euclid(ids.len() as i32) as usize,
        None if by > 0 => 0,
        None => ids.len() - 1,
    };
    open_thread(s, &ids[next].clone());
}

fn search_key(s: &mut App, e: &KeyEvent, _tx: &mut Tx) -> bool {
    let hits: Vec<String> = {
        let model = Model::build(s);
        logic::search(&model.threads, &s.ui.shell.sidebar.search.text).into_iter().map(|i| model.threads[i].id.clone()).collect()
    };
    let ui = &mut s.ui.shell.sidebar;
    match e.key {
        Key::Escape => clear_search(s),
        Key::Down => ui.highlight = (ui.highlight + 1).min(hits.len().saturating_sub(1)),
        Key::Up => ui.highlight = ui.highlight.saturating_sub(1),
        Key::Enter => {
            if let Some(id) = hits.get(ui.highlight).cloned() {
                clear_search(s);
                open_thread(s, &id);
            }
        }
        _ => return false,
    }
    true
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &T, overlays: &mut Vec<El>) -> El {
    let ui = &s.ui.shell.sidebar;
    let inset = cx.titlebar_inset();
    if ui.collapsed {
        overlays.push(chrome::floating_toggle(t, inset));
        return el().key("sidebar").w(0.0).h_full();
    }
    let model = Model::build(s);
    let searching = !ui.search.text.trim().is_empty();
    let list = if searching { chrome::search_results(s, &model, t) } else { row::list(s, cx, &model, t) };
    if let Some(m) = &ui.menu
        && let Some(th) = model.thread(&m.thread)
    {
        overlays.extend(menu::view(s, cx, &model, th, m, t));
    }
    delete_dialog(s, &model, t, overlays);
    chrome::scope_menu(s, &model, t, overlays);
    // 257pt of content and a 1pt border: the web's `w-64` plus its `border-r` and rail.
    let content = col()
        .w(layout::SIDEBAR_W + 1.0)
        .h_full()
        .child(chrome::header(s, t, inset))
        .child(chrome::search_bar(s, t))
        .child(list)
        .child(chrome::footer(&model, t));
    row()
        .key("sidebar")
        .w(layout::SIDEBAR_W + 2.0)
        .h_full()
        .fill(t.sb.sidebar)
        .child(content)
        .child(el().w(1.0).h_full().fill(t.sb.contrast_sidebar_border))
}
