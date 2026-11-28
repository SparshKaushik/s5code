//! Sidebar v2 (apps/web `Sidebar.tsx`): brand header, search, pinned / active thread cards, the
//! Working / Snoozed / Settled shelves, and the footer. Rows drag between sections and open a
//! context menu; the rules live in `logic`, the drawing in `chrome`, `row` and `menu`.

mod brand;
mod chrome;
mod drafts;
pub mod everything;
pub mod release_notes;
pub mod favicon;
pub(crate) mod folders;
pub mod logic;
mod menu;
pub(crate) mod row;
pub mod share;
mod snooze;
mod stage;
mod updates;

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
    /// Dragged out of the list sideways (or from Working): the rows go to the composer as
    /// thread context instead of moving.
    pub context: bool,
    /// The pointer, in layout points.
    pub at: (f32, f32),
    /// The list's top when the drag began, so the row keeps under the pointer as it scrolls.
    pub scroll: Option<f32>,
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

/// A sent command: its request, the thread whose client owns it, web's failure title, and the
/// threads whose optimistic state a failure undoes.
#[derive(Clone, Debug)]
pub struct Inflight {
    pub request: u64,
    pub thread: String,
    pub title: &'static str,
    pub rollback: Vec<String>,
    /// The bulk action it belongs to: only that action's first failure toasts.
    pub batch: Option<u64>,
    pub quiet: bool,
}

#[derive(Default)]
pub struct Ui {
    pub collapsed: bool,
    pub search: TextEdit,
    /// Highlighted search result.
    pub highlight: usize,
    pub expanded: HashSet<Shelf>,
    /// "Show more" pages opened under the Settled shelf (`SETTLED_TAIL_PAGE_SIZE` each).
    pub settled_pages: usize,
    pub hover: Option<String>,
    /// Rows picked with Cmd/Shift-click for bulk actions.
    pub selected: HashSet<String>,
    pub anchor: Option<String>,
    pub drag: Option<Dragging>,
    /// The pressed row's rect and the press point, in layout points.
    pub press: Option<([f32; 4], f32, f32)>,
    /// Last frame's layout, read by drag handlers: the hovered row, the list top, the edge the
    /// pointer sits near (-1 top, 1 bottom) and the drop target with its stickiness applied.
    pub hover_rect: std::cell::Cell<Option<[f32; 4]>>,
    pub list_y: std::cell::Cell<f32>,
    pub drag_edge: std::cell::Cell<i8>,
    /// The item nearest the dragged row's centre, from the last layout.
    pub drag_over: std::cell::RefCell<Option<String>>,
    pub drag_target: std::cell::RefCell<Option<logic::DropTarget>>,
    pub menu: Option<menu::Menu>,
    pub patches: HashMap<String, Patch>,
    /// Commands awaiting their reply, to toast and roll back a failure.
    pub inflight: Vec<Inflight>,
    /// The bulk action dispatching now, and the toast its failure shows.
    batch: Option<(u64, &'static str)>,
    batches: u64,
    /// The threads a drop is changing, while it dispatches: a failure rolls back all of them.
    drop_group: Option<Vec<String>>,
    pub back: Vec<Route>,
    pub forward: Vec<Route>,
    /// The row whose title is being edited in place, and the edit.
    pub rename: Option<(String, TextEdit)>,
    /// The open "Share with guest" dialog.
    pub guest: Option<share::Guest>,
    /// A pending `projects.createFromThread` and the thread's title for its toast.
    pub to_project: Option<(u64, String)>,
    /// The Custom… snooze dialog and the threads it snoozes.
    pub custom_snooze: Option<snooze::Custom>,
    /// The row whose hover card (`ThreadHoverCard`) is showing.
    pub card: Option<String>,
    /// A row key the arrow keys move focus to on the next frame (see `row::list`).
    pub focus_to: Option<String>,
    /// Project ids the list is filtered to (the web's `projectScopeKeys`); empty shows all.
    pub project_scope: Vec<String>,
    pub scope_open: bool,
    /// The scope picker's group order, frozen while it is open so a pin doesn't move the row.
    pub scope_order: Vec<String>,
    /// The picker's search and its highlighted row ("all" or a group key; first when `None`).
    pub scope_search: TextEdit,
    pub scope_active: Option<String>,
    /// The boring sidebar's folders: which are folded, hover and the folder menu.
    pub folders: folders::State,
    pub visits: logic::Visits,
    pub undos: logic::Undos,
    pub drafts: drafts::Ui,
    /// The server's message matches for a search: (query, thread ids).
    pub content: (String, Vec<String>),
    /// Since when the thread-jump modifiers have been held alone (`useThreadJumpHintsVisible`).
    jump_held: Option<i64>,
    /// The thread row a file drag is over.
    drop_over: Option<String>,
    /// The width the rail set, read from the window prefs on first use.
    width: std::cell::Cell<Option<f32>>,
    /// The Dev or Nightly header artwork, rasterized for the current width.
    stage: stage::Art,
    /// Each matched thread's first message match (`threadId`, `snippet`, `source`), by `content`.
    content_matches: Vec<serde_json::Value>,
    content_due: Option<(i64, String)>,
    /// One request per connected environment, and the query they answer.
    content_req: Option<(Vec<u64>, String)>,
    favicons: favicon::Favicons,
    provider_updates: updates::State,
    everything: everything::Ui,
}

impl Ui {
    pub fn content_hits(&self) -> &[String] {
        if self.content.0 == self.search.text.trim() { &self.content.1 } else { &[] }
    }

    /// The message match a result row shows and opens find on, for the current query.
    pub fn content_match(&self, thread: &str) -> Option<&serde_json::Value> {
        (self.content.0 == self.search.text.trim()).then_some(())?;
        self.content_matches.iter().find(|m| m["threadId"] == thread)
    }

    /// `threadSearch.isPending`: message search is debouncing or in flight for the query.
    pub fn content_pending(&self) -> bool {
        let query = self.search.text.trim();
        self.content_due.as_ref().is_some_and(|d| d.1 == query) || self.content_req.as_ref().is_some_and(|r| r.1 == query)
    }
}

pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("sidebar-home", scene_home),
    ("sidebar-stage-nightly", |s| {
        scene_home(s);
        s.client_settings.values.insert("environmentIdentificationMode".into(), json!("nightly-artwork"));
    }),
    ("sidebar-stage-dev", |s| {
        scene_home(s);
        s.client_settings.values.insert("environmentIdentificationMode".into(), json!("dev-artwork"));
    }),
    ("sidebar-stage-nightly-grove", |s| {
        scene_home(s);
        s.client_settings.values.insert("environmentIdentificationMode".into(), json!("nightly-artwork"));
        s.client_settings.values.insert("themeDark".into(), json!("grove"));
    }),
    ("sidebar-stage-dev-ocean", |s| {
        scene_home(s);
        s.client_settings.values.insert("environmentIdentificationMode".into(), json!("dev-artwork"));
        s.client_settings.values.insert("themeDark".into(), json!("ocean"));
    }),
    // A file drag held over the second row.
    ("sidebar-file-drop", |s| {
        scene_home(s);
        s.ui.shell.sidebar.drop_over = s.state().threads.get(1).map(|t| crate::data::s(t, "id").to_owned());
    }),
    // Search with message matches: a title hit with its excerpt, and a thread found only by one.
    ("sidebar-search-excerpts", |s| {
        scene_home(s);
        let hit = |s: &App, title: &str| thread_titled(s, title).unwrap_or_default();
        let (showcase, todo) = (hit(s, "Replay showcase"), hit(s, "New thread"));
        let ui = &mut s.ui.shell.sidebar;
        ui.search = TextEdit::new("replay turn");
        ui.content = ("replay turn".into(), vec![showcase.clone(), todo.clone()]);
        ui.content_matches = vec![
            json!({"threadId": showcase, "source": "assistant", "snippet": "…the replay finished its last turn without errors"}),
            json!({"threadId": todo, "source": "user", "snippet": "Can you replay the previous turn?"}),
        ];
    }),
    ("sidebar-selected", |s| {
        scene_home(s);
        if let Some(id) = thread_titled(s, "Replay showcase") {
            s.ui.shell.sidebar.selected.insert(id);
        }
        if let Some(id) = thread_titled(s, "Todo list replay") {
            s.ui.shell.sidebar.selected.insert(id);
        }
    }),
    // A row's hover card, as the reference caught it (card shown, row style already settled).
    ("sidebar-hover", |s| {
        scene_on(s, "Replay showcase");
        s.ui.shell.sidebar.card = thread_titled(s, "Idempotency key definition");
    }),
    ("sidebar-rename", |s| {
        scene_on(s, "Replay showcase");
        if let Some(id) = thread_titled(s, "Todo list replay") {
            start_rename(s, &id);
        }
    }),
    ("sidebar-delete", |s| {
        scene_on(s, "Replay showcase");
        if let Some(id) = thread_titled(s, "Todo list replay") {
            ask_delete(s, &id);
        }
    }),
    // Two new-thread drafts (newest first) and a thread holding unsent text.
    ("sidebar-drafts", |s| {
        scene_on(s, "Replay showcase");
        scene_drafts(s);
    }),
    ("sidebar-drafts-hover", |s| {
        scene_on(s, "Replay showcase");
        scene_drafts(s);
        s.ui.shell.sidebar.card = thread_titled(s, "Todo list replay");
    }),
    ("sidebar-scope", |s| {
        scene_on(s, "Replay showcase");
        toggle_scope(s);
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
            s.ui.shell.sidebar.drag = Some(Dragging { key: id, section: Section::Active, dy: -250.0, context: false, at: (0.0, 0.0), scroll: None });
        }
    }),
    ("sidebar-light", |s| {
        scene_home(s);
        s.appearance = crate::Appearance::Light;
    }),
    // One provider updating (the pill) and one behind its latest version (the footer button).
    ("sidebar-updates", |s| {
        scene_home(s);
        if let crate::data::Source::Fixture(state) = &mut s.src
            && let Some(providers) = state.config.as_mut().and_then(|c| c["providers"].as_array_mut())
        {
            if let Some(p) = providers.get_mut(0) {
                p["updateState"] = json!({"status": "running"});
            }
            if let Some(p) = providers.get_mut(1) {
                p["versionAdvisory"] = json!({"status": "behind_latest", "latestVersion": "9.9.9", "canUpdate": true});
            }
        }
    }),
    ("sidebar-folders", |s| {
        scene_home(s);
        s.client_settings.values.insert("boringSidebarEnabled".into(), json!(true));
    }),
    ("sidebar-grouped", |s| {
        scene_home(s);
        scene_shared_repository(s);
        s.client_settings.values.insert("boringSidebarEnabled".into(), json!(true));
    }),
    ("sidebar-grouped-scope", |s| {
        scene_home(s);
        scene_shared_repository(s);
        toggle_scope(s);
    }),
];

/// The fixture's `server` and `s5code` projects as two checkouts of one repository, the first
/// pinned, with project organization on so the scope menu offers pins.
fn scene_shared_repository(s: &mut App) {
    let crate::data::Source::Fixture(state) = &mut s.src else { return };
    for p in &mut state.projects {
        let root = match p["title"].as_str() {
            Some("server") => "/home/ubuntu/s5code-ref",
            Some("s5code") => "/home/ubuntu/projects/s5code",
            _ => continue,
        };
        p["repositoryIdentity"] = json!({"canonicalKey": "github.com/s5labs/s5code", "displayName": "s5labs/s5code", "name": "s5code", "provider": "github", "rootPath": root});
        if p["title"] == "server" {
            p["pinnedAt"] = json!("2026-01-01T00:00:00.000Z");
        }
    }
    if let Some(config) = state.config.as_mut() {
        config["environment"]["capabilities"]["projectOrganization"] = json!(true);
    }
}

fn thread_titled(s: &App, title: &str) -> Option<String> {
    s.state().threads.iter().find(|t| t["title"].as_str().is_some_and(|x| x.starts_with(title))).map(|t| crate::data::s(t, "id").to_string())
}

fn scene_drafts(s: &mut App) {
    let now = s.now_ms;
    let projects: Vec<String> = s.state().projects.iter().take(2).map(|p| crate::data::s(p, "id").to_string()).collect();
    for (n, project) in projects.iter().enumerate() {
        let draft = s.ui.chat.composer.drafts.entry(crate::chat::composer::home_key(project)).or_default();
        draft.edit = TextEdit::new(if n == 0 { "Sketch the retry policy for webhooks" } else { "Ask about the flaky login test" });
        draft.created_at = now - (n as i64 + 1) * 600_000;
    }
    if let Some(id) = thread_titled(s, "Todo list replay") {
        s.ui.chat.composer.drafts.entry(id).or_default().edit = TextEdit::new("Half a follow-up question");
    }
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
    /// The boring sidebar's project folders, which hold the Active rows; empty otherwise.
    pub folders: Vec<folders::Folder>,
}

/// The expanded Settled shelf shows 10 rows, then 25 more per "Show more".
pub fn settled_limit(ui: &Ui) -> usize {
    10 + 25 * ui.settled_pages
}

/// Opens or closes the project scope picker, freezing its order: open groups, pinned first.
pub fn toggle_scope(s: &mut App) {
    let open = !s.ui.shell.sidebar.scope_open;
    if open {
        let (groups, _) = folders::group_of(s);
        let threads = Model::build(s).threads;
        let sort = crate::settings::value(s, "sidebarProjectSortOrder");
        let order = folders::ordered(&folders::Plan {
            groups: &groups,
            projects: &s.state().projects,
            threads: &threads,
            active: &[],
            scope: &HashSet::new(),
            sort: sort.as_str().unwrap_or("updated_at"),
        });
        s.ui.shell.sidebar.scope_order = order.into_iter().map(|n| groups[n].key.clone()).collect();
    }
    let ui = &mut s.ui.shell.sidebar;
    ui.scope_open = open;
    ui.scope_search = TextEdit::default();
    ui.scope_search.focused = open;
    ui.scope_active = None;
}

/// `boringSidebarEnabled`, or the legacy sidebar it replaced.
pub fn boring(s: &App) -> bool {
    crate::settings::value(s, "boringSidebarEnabled") == true || crate::settings::value(s, "legacySidebarEnabled") == true
}

impl Model {
    pub fn build(s: &App) -> Model {
        let state = s.state();
        let ui = &s.ui.shell.sidebar;
        let scope = &ui.project_scope;
        let (hidden, _) = tag_hidden(s);
        let threads = state
            .threads
            .iter()
            .filter(|v| hidden.is_empty() || !hidden.contains(&logic::thread_key(&crate::env::of(s, v), crate::data::s(v, "id"))))
            .map(|v| patched(Thread::from_shell(v), &ui.patches))
            .filter(|t| scope.is_empty() || scope.contains(&t.project_id))
            .collect::<Vec<_>>();
        let config = state.config.as_ref();
        let boring = boring(s);
        // The boring sidebar has no Working section.
        let working_shelf = !boring && config.and_then(|c| c["settings"]["sidebarWorkingShelfEnabled"].as_bool()).unwrap_or(true);
        let providers = config.and_then(|c| c["providers"].as_array()).map(Vec::as_slice).unwrap_or(&[]);
        let sections = logic::partition(&threads, s.now_ms, working_shelf);
        let folders = if boring {
            let (groups, _) = folders::group_of(s);
            let scope: HashSet<String> = groups.iter().filter(|g| g.members.iter().any(|m| ui.project_scope.contains(m))).map(|g| g.key.clone()).collect();
            let sort = crate::settings::value(s, "sidebarProjectSortOrder");
            folders::plan(&folders::Plan {
                groups: &groups,
                projects: &state.projects,
                threads: &threads,
                active: &sections.active,
                scope: &scope,
                sort: sort.as_str().unwrap_or("updated_at"),
            })
        } else {
            vec![]
        };
        Model {
            folders,
            sections,
            threads,
            projects: state.projects.iter().map(|p| (crate::data::s(p, "id").to_string(), crate::data::s(p, "title").to_string())).collect(),
            drivers: providers.iter().map(|p| (crate::data::s(p, "instanceId").to_string(), crate::data::s(p, "driver").to_string())).collect(),
            route: match s.frame_route() {
                Route::Thread(id) => Some(id.clone()),
                _ => None,
            },
            now: s.now_ms,
            working_shelf,
            updates: update_count(providers),
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
        if self.folders.is_empty() {
            items.extend(s.active.iter().map(|i| row(i, Section::Active)));
        }
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
        let limit = settled_limit(ui);
        for (n, i) in s.settled.iter().enumerate() {
            if (ui.expanded.contains(&Shelf::Settled) && n < limit) || self.route.as_deref() == Some(self.threads[*i].id.as_str()) {
                items.push(row(i, Section::Settled));
            }
        }
        items
    }

    /// Every row's id in section order, collapsed shelves included (`orderedThreadKeys`).
    pub fn ordered_ids(&self) -> Vec<String> {
        let s = &self.sections;
        [&s.pinned, &s.active, &s.working, &s.snoozed, &s.settled].into_iter().flatten().map(|i| self.threads[*i].id.clone()).collect()
    }

    /// Thread ids in the order the list shows them, for keyboard stepping.
    pub fn visible_ids(&self, ui: &Ui) -> Vec<String> {
        let mut out = vec![];
        for item in self.items(ui) {
            match item {
                Item::Thread { key, .. } => out.push(key),
                Item::Marker(Marker::ActivePlaceholder) => {
                    for folder in &self.folders {
                        out.extend(folders::shown(folder, self, &ui.folders).into_iter().map(|i| self.threads[i].id.clone()));
                    }
                }
                Item::Marker(_) => {}
            }
        }
        out
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

/// The environment key a row acts on.
/// The tag filter over every loaded environment (`useHiddenThreadKeys`): the hidden thread keys,
/// and whether any environment hides tags (`useAnyEnvironmentHiding`).
pub fn tag_hidden(s: &App) -> (HashSet<String>, bool) {
    let mut tags = HashMap::new();
    for key in crate::env::keys(s) {
        let found = if key == s.ui.env.active {
            logic::hidden_tags(crate::settings::server_settings(s))
        } else {
            crate::env::source(s, &key).and_then(|src| src.state().config.as_ref().and_then(|c| logic::hidden_tags(&c["settings"])))
        };
        if let Some(found) = found {
            tags.insert(key, found);
        }
    }
    if tags.is_empty() {
        return (HashSet::new(), false);
    }
    let keys: Vec<String> = s.state().threads.iter().map(|v| crate::env::of(s, v)).collect();
    let rows: Vec<(&str, &Value)> = keys.iter().map(String::as_str).zip(s.state().threads.iter()).collect();
    (logic::hidden_threads(&rows, &tags), true)
}

pub fn env_key<'a>(s: &'a App, th: &'a Thread) -> &'a str {
    th.env.as_deref().unwrap_or(&s.ui.env.active)
}

/// Web `readEnvironmentScope(environmentId, scope)` for the row's environment: the connection on
/// screen asks its own session; another environment its last session answer, unknown until it
/// comes (the server still authorizes).
pub fn grants_on(s: &App, key: &str, scope: &str) -> bool {
    if key == s.ui.env.active {
        return crate::chat::details::grants(s, scope);
    }
    crate::settings::permissions::reply_on(s, key).is_none_or(|r| r.as_ref().is_ok_and(|v| crate::settings::session_grants(v, scope)))
}

/// `canOperateThreads`: every thread's environment grants `orchestration:operate`.
pub fn can_operate(s: &App, threads: &[&Thread]) -> bool {
    threads.iter().all(|th| grants_on(s, env_key(s, th), "orchestration:operate"))
}

/// The row's server config, once it has loaded (`serverConfigs.get(environmentId)`).
pub fn config_of<'a>(s: &'a App, th: &Thread) -> Option<&'a Value> {
    match &th.env {
        None => s.state().config.as_ref(),
        Some(key) => crate::env::source(s, key).and_then(|src| src.state().config.as_ref()),
    }
}

/// The web's failure toast title for a sidebar command.
fn failure_title(kind: &str, extra: &Value) -> Option<&'static str> {
    Some(match kind {
        "thread.pin" => "Failed to pin thread",
        "thread.unpin" => "Failed to unpin thread",
        "thread.settle" => "Failed to settle thread",
        "thread.unsettle" => "Failed to un-settle thread",
        "thread.snooze" => "Failed to snooze thread",
        "thread.unsnooze" => "Failed to wake thread",
        "thread.archive" => "Failed to archive thread",
        "thread.delete" => "Failed to delete thread",
        "thread.pin.reorder" => "Failed to reorder pinned threads",
        "thread.active.reorder" => "Failed to reorder active threads",
        "thread.auto-settle.set" => "Failed to update auto-settle",
        "thread.metadata.update" if extra["regenerateTitle"] == true => "Failed to regenerate thread title",
        "thread.metadata.update" if extra.get("title").is_some() => "Failed to rename thread",
        _ => return None,
    })
}

/// Sends one orchestration command (the web's `thread.*` dispatches); fixtures only patch.
fn dispatch(s: &mut App, kind: &str, id: &str, extra: Value) {
    let title = failure_title(kind, &extra);
    let Some(net) = crate::env::net_for(s, id) else { return };
    let mut cmd = json!({ "type": kind, "commandId": s5_net::uuid(), "threadId": id });
    if let (Some(obj), Value::Object(more)) = (cmd.as_object_mut(), extra) {
        obj.extend(more);
    }
    let request = net.dispatch(cmd);
    if let Some(title) = title {
        let ui = &mut s.ui.shell.sidebar;
        let rollback = ui.drop_group.clone().unwrap_or_else(|| vec![id.to_string()]);
        let (title, batch) = ui.batch.map_or((title, None), |(batch, title)| (title, Some(batch)));
        ui.inflight.push(Inflight { request, thread: id.to_string(), title, rollback, batch, quiet: false });
    }
}

/// Runs a bulk action whose failures share one toast, as web's "Failed to delete threads".
pub(crate) fn batched(s: &mut App, title: &'static str, f: impl FnOnce(&mut App)) {
    let ui = &mut s.ui.shell.sidebar;
    ui.batches += 1;
    ui.batch = Some((ui.batches, title));
    f(s);
    s.ui.shell.sidebar.batch = None;
}

/// Takes command replies: a failure toasts its title and undoes the optimistic state it set.
fn poll_inflight(s: &mut App) -> bool {
    let mut changed = false;
    let mut i = 0;
    while i < s.ui.shell.sidebar.inflight.len() {
        let (request, thread) = (s.ui.shell.sidebar.inflight[i].request, s.ui.shell.sidebar.inflight[i].thread.clone());
        let reply = match crate::env::net_for(s, &thread) {
            Some(net) => net.take_reply(request),
            // The thread's client is gone, and its reply with it.
            None => Some(Ok(Value::Null)),
        };
        let Some(reply) = reply else {
            i += 1;
            continue;
        };
        let done = s.ui.shell.sidebar.inflight.remove(i);
        if let Err(error) = reply {
            failed(s, &done, &error);
            changed = true;
        }
    }
    changed
}

fn failed(s: &mut App, done: &Inflight, error: &Value) {
    for id in &done.rollback {
        s.ui.shell.sidebar.patches.remove(id);
    }
    if done.quiet {
        return;
    }
    for other in s.ui.shell.sidebar.inflight.iter_mut().filter(|f| done.batch.is_some() && f.batch == done.batch) {
        other.quiet = true;
    }
    let message = crate::shell::palette::error_message(error, "An error occurred.");
    crate::shell::palette::toast_error_brief(s, done.title, Some(&message));
}

/// Runs a row action when `checkThreadOperations` allows it.
/// `useAcknowledgeThreadWoke`: a visit at the wake time clears "Woke" without opening the thread.
pub fn acknowledge_woke(s: &mut App, id: &str, woke_at: i64) {
    dispatch(s, "thread.visit", id, json!({ "visitedAt": iso(woke_at) }));
}

pub fn checked(s: &mut App, id: &str, f: fn(&mut App, &str)) {
    if check_operations(s, &[id.to_string()]) {
        f(s, id);
    }
}

pub fn can_operate_id(s: &App, id: &str) -> bool {
    Model::build(s).thread(id).is_none_or(|th| can_operate(s, &[th]))
}

/// `checkThreadOperations`: whether the threads' environments let this client change them,
/// else web's "Thread action unavailable" toast.
pub fn check_operations(s: &mut App, ids: &[String]) -> bool {
    let model = Model::build(s);
    let threads: Vec<&Thread> = ids.iter().filter_map(|id| model.thread(id)).collect();
    if can_operate(s, &threads) {
        return true;
    }
    crate::shell::palette::toast_error_brief(s, "Thread action unavailable", Some("This connection cannot change one or more selected threads."));
    false
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

/// Offers the footer's undo for an action about to change the thread.
fn record(s: &mut App, action: logic::UndoAction, id: &str) {
    let Some(plan) = Model::build(s).thread(id).map(|t| logic::undo_plan(action, t)) else { return };
    s.ui.shell.sidebar.undos.push(action, id, plan, crate::data::now_ms());
}

/// `thread.undo`: reverses the notice's newest group of actions.
pub fn undo(s: &mut App) -> bool {
    let group = s.ui.shell.sidebar.undos.take(crate::data::now_ms());
    for (id, plan) in &group {
        for step in plan {
            match step {
                logic::UndoStep::Unsettle => unsettle(s, id),
                logic::UndoStep::Pin(key) => pin(s, id, key.clone()),
                // A snooze can lapse before Undo runs, and the server rejects an expired one.
                logic::UndoStep::Snooze(until) if *until > crate::data::now_ms() => set_snooze(s, id, *until),
                logic::UndoStep::Snooze(_) => {}
                logic::UndoStep::Unsnooze => unsnooze(s, id),
                logic::UndoStep::RestoreDraft(stored) => {
                    if !crate::chat::composer::put_draft(s, id, stored) {
                        crate::shell::palette::toast_error_brief(s, "Failed to restore draft", Some("The draft has new content."));
                    }
                }
                logic::UndoStep::Unarchive => {
                    dispatch(s, "thread.unarchive", id, json!({}));
                    if let Some(p) = s.ui.shell.sidebar.patches.get_mut(id) {
                        p.archived = false;
                    }
                }
            }
        }
    }
    !group.is_empty()
}

pub fn unpin(s: &mut App, id: &str) {
    record(s, logic::UndoAction::Unpinned, id);
    dispatch(s, "thread.unpin", id, json!({}));
    patch(s, id, |p| {
        p.pinned = Some(false);
        p.pin_key = Some(None);
    });
}

pub fn settle(s: &mut App, id: &str) {
    settle_parking(s, id, &HashSet::new());
}

/// `planForwardNavigation`, taken before `id` parks: the next card when it is the open thread.
fn forward_after_park(s: &App, id: &str, parking: &HashSet<String>) -> Option<Route> {
    if s.route != Route::Thread(id.to_string()) {
        return None;
    }
    let model = Model::build(s);
    let order = model.ordered_ids();
    let parked: HashSet<String> = [&model.sections.snoozed, &model.sections.settled].into_iter().flatten().map(|i| model.threads[*i].id.clone()).collect();
    Some(match logic::next_card(&order, &parked, parking, id) {
        Some(next) => Route::Thread(next.to_string()),
        None => Route::Home { project_id: model.thread(id).map(|t| t.project_id.clone()).filter(|p| !p.is_empty()) },
    })
}

fn park_forward(s: &mut App, route: Option<Route>) {
    match route {
        Some(Route::Thread(next)) => open_thread(s, &next),
        Some(route) => {
            s.ui.chat.home.workspace = None;
            go(s, route);
        }
        None => {}
    }
}

fn settle_parking(s: &mut App, id: &str, parking: &HashSet<String>) {
    let forward = forward_after_park(s, id, parking);
    settle_in_place(s, id);
    park_forward(s, forward);
}

/// Settles without moving on, as a drop onto the Settled shelf does.
fn settle_in_place(s: &mut App, id: &str) {
    record(s, logic::UndoAction::Settled, id);
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
    snooze_many(s, &[id.to_string()], until);
}

/// A snooze menu choice: `pickSnoozePreset` refuses a time that passed while the menu was open.
pub fn snooze_picked(s: &mut App, ids: &[String], until: i64) {
    if until <= crate::data::now_ms() {
        let toast = crate::shell::toast::Toast::new("Could not snooze thread", Some("That snooze time has passed. Choose another time."), crate::shell::toast::Kind::Warning);
        crate::shell::toast::add(s, toast, None);
        return;
    }
    snooze_many(s, ids, until);
}

/// Snoozes every target; the open one moves on past its co-snoozing rows.
pub fn snooze_many(s: &mut App, ids: &[String], until: i64) {
    // Several threads share one failure toast, as web's bulk snooze.
    if ids.len() > 1 {
        return batched(s, "Failed to snooze threads", |s| snooze_each(s, ids, until));
    }
    snooze_each(s, ids, until);
}

fn snooze_each(s: &mut App, ids: &[String], until: i64) {
    let parking: HashSet<String> = ids.iter().cloned().collect();
    let forward = ids.iter().find_map(|id| forward_after_park(s, id, &parking));
    for id in ids {
        record(s, logic::UndoAction::Snoozed, id);
        set_snooze(s, id, until);
    }
    s.ui.shell.sidebar.selected.clear();
    park_forward(s, forward);
}

/// Settles every target that is not settled yet, as the bulk "Settle (n)".
pub fn settle_many(s: &mut App, ids: &[String]) {
    let parking: HashSet<String> = ids.iter().cloned().collect();
    let model = Model::build(s);
    let open: Vec<String> = ids.iter().filter(|id| model.thread(id).is_some_and(|t| !t.settled)).cloned().collect();
    // The open thread parks last so its forward step sees the others already parking.
    let (current, rest): (Vec<String>, Vec<String>) = open.into_iter().partition(|id| s.route == Route::Thread(id.clone()));
    for id in rest.iter().chain(&current) {
        settle_parking(s, id, &parking);
    }
    s.ui.shell.sidebar.selected.clear();
}

/// Opens the Custom… snooze dialog for `ids`.
pub fn custom_snooze(s: &mut App, ids: Vec<String>) {
    s.ui.shell.sidebar.custom_snooze = Some(snooze::Custom::new(ids, crate::data::now_ms(), crate::data::Zone::Local));
}

fn set_snooze(s: &mut App, id: &str, until: i64) {
    dispatch(s, "thread.snooze", id, json!({ "snoozedUntil": iso(until) }));
    patch(s, id, |p| p.snoozed_until = Some(Some(until)));
}

pub fn unsnooze(s: &mut App, id: &str) {
    dispatch(s, "thread.unsnooze", id, json!({ "reason": "user" }));
    patch(s, id, |p| p.snoozed_until = Some(None));
}

/// "Archive thread": asks first when `confirmThreadArchive` is on.
pub fn request_archive(s: &mut App, id: &str) {
    if client_flag(s, "confirmThreadArchive") {
        let id = id.to_string();
        crate::shell::confirm::request(s, format!("Archive thread \"{}\"?", title_of(s, &id)), false, move |s: &mut App| archive(s, &id));
    } else {
        archive(s, id);
    }
}

/// "Unpin thread": asks first when `confirmThreadUnpin` is on.
pub fn request_unpin(s: &mut App, id: &str) {
    if client_flag(s, "confirmThreadUnpin") {
        ask_unpin(s, id);
    } else {
        unpin(s, id);
    }
}

/// Providers a click can update, one per driver (`collectProviderUpdateCandidates` filtered by
/// `canOneClickUpdateProviderCandidate`).
fn update_count(providers: &[Value]) -> usize {
    let mut drivers: Vec<&str> = providers
        .iter()
        .filter(|p| {
            let advisory = &p["versionAdvisory"];
            let latest = &p["compatibilityAdvisory"]["latestVersionStatus"];
            p["enabled"] != false
                && latest != "broken"
                && latest != "unsupported"
                && advisory["status"] == "behind_latest"
                && advisory["canUpdate"] == true
                && !advisory["updateCommand"].is_null()
        })
        .map(|p| crate::data::s(p, "driver"))
        .collect();
    drivers.sort_unstable();
    drivers.dedup();
    drivers.len()
}

/// "Unpin (n)": web unpins each through `attemptUnpin`, so each asks in turn.
pub fn request_unpin_many(s: &mut App, ids: Vec<String>) {
    if !client_flag(s, "confirmThreadUnpin") {
        for id in &ids {
            unpin(s, id);
        }
        return;
    }
    for id in &ids {
        ask_unpin(s, id);
    }
}

/// A thread's title for a question about it.
fn title_of(s: &App, id: &str) -> String {
    Model::build(s).thread(id).map(|t| t.title.clone()).unwrap_or_default()
}

fn ask_unpin(s: &mut App, id: &str) {
    let message = format!("Unpin thread \"{}\"?\nThis will move the thread out of your pinned section.", title_of(s, id));
    let id = id.to_string();
    crate::shell::confirm::request(s, message, false, move |s: &mut App| unpin(s, &id));
}

fn ask_delete(s: &mut App, id: &str) {
    let message = format!("Delete thread \"{}\"?\nThis permanently clears conversation history for this thread.", title_of(s, id));
    let id = id.to_string();
    crate::shell::confirm::request(s, message, true, move |s: &mut App| delete(s, &id));
}

/// "Delete (n)": asks first when `confirmThreadDelete` is on.
pub fn request_delete_many(s: &mut App, ids: Vec<String>) {
    if client_flag(s, "confirmThreadDelete") {
        let n = ids.len();
        let message = format!("Delete {n} thread{}?\nThis permanently clears conversation history for these threads.", if n == 1 { "" } else { "s" });
        crate::shell::confirm::request(s, message, true, move |s: &mut App| delete_many(s, &ids));
    } else {
        delete_many(s, &ids);
    }
}

fn delete_many(s: &mut App, ids: &[String]) {
    batched(s, "Failed to delete threads", |s| {
        for (n, id) in ids.iter().enumerate() {
            delete_after(s, id, &ids[..n]);
        }
    });
    s.ui.shell.sidebar.selected.retain(|id| !ids.contains(id));
}

pub fn archive(s: &mut App, id: &str) {
    record(s, logic::UndoAction::Archived, id);
    dispatch(s, "thread.archive", id, json!({}));
    patch(s, id, |p| p.archived = true);
}

fn client_flag(s: &App, key: &str) -> bool {
    s.client_settings.values.get(key).or_else(|| crate::settings::client::defaults().get(key)).and_then(Value::as_bool).unwrap_or(false)
}

/// "Delete": asks first when `confirmThreadDelete` is on, as `useThreadActionMenu` does.
pub fn request_delete(s: &mut App, id: &str) {
    if client_flag(s, "confirmThreadDelete") {
        ask_delete(s, id);
    } else {
        delete(s, id);
    }
}

pub fn delete(s: &mut App, id: &str) {
    delete_after(s, id, &[]);
}

/// Deletes `id` after `earlier` in a bulk delete, which no longer hold a worktree it shares.
fn delete_after(s: &mut App, id: &str, earlier: &[String]) {
    crate::chat::details::cleanup::before_delete(s, id, earlier);
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

/// ThreadRenameInput commits on blur: focus moved off the editor. Also records visits to the
/// thread on screen.
pub fn poll(s: &mut App) -> bool {
    folders::ensure_loaded(s);
    folders::sync_scope(s);
    drafts::poll(s);
    let searched = poll_content(s) | favicon::poll(s) | updates::poll(s) | share::poll(s) | poll_inflight(s);
    let shown = match &s.route {
        Route::Thread(id) => Some(id.clone()),
        _ => None,
    };
    let shell = shown.as_ref().and_then(|id| s.state().threads.iter().find(|t| t["id"] == *id).cloned());
    if let Some((visited_at, opened)) = s.ui.shell.sidebar.visits.tick(shown.as_deref(), shell.as_ref(), crate::data::now_ms())
        && let Some(id) = shown
    {
        let extra = if opened { json!({ "visitedAt": visited_at, "opened": true }) } else { json!({ "visitedAt": visited_at }) };
        dispatch(s, "thread.visit", &id, extra);
    }
    if s.ui.shell.sidebar.rename.as_ref().is_some_and(|(_, e)| !e.focused) {
        commit_rename(s);
        return true;
    }
    searched
}

/// Asks every environment which threads' messages match the search, 200ms after typing stops, as
/// the web's sidebar does through `orchestration.searchThreads`; matches show as each answers.
fn poll_content(s: &mut App) -> bool {
    let now = crate::data::now_ms();
    let ui = &mut s.ui.shell.sidebar;
    let query = ui.search.text.trim().to_string();
    let wanted = ui.content_due.as_ref().map(|d| &d.1).or(ui.content_req.as_ref().map(|r| &r.1)).unwrap_or(&ui.content.0);
    if *wanted != query {
        ui.content_due = (!query.is_empty()).then(|| (now + 200, query));
    }
    if let Some((_, q)) = ui.content_due.take_if(|(due, _)| now >= *due) {
        let ids = crate::env::request_all(s, "orchestration.searchThreads", &json!({ "query": q.chars().take(200).collect::<String>(), "limit": 20 }));
        s.ui.shell.sidebar.content_req = Some((ids, q));
    }
    let Some((pending, q)) = s.ui.shell.sidebar.content_req.take() else { return false };
    let Some(net) = s.src.net() else {
        s.ui.shell.sidebar.content_req = Some((pending, q));
        return false;
    };
    let (mut answered, mut waiting) = (vec![], vec![]);
    for id in pending {
        match net.take_reply(id) {
            Some(reply) => answered.push(reply),
            None => waiting.push(id),
        }
    }
    let ui = &mut s.ui.shell.sidebar;
    let changed = !answered.is_empty();
    if changed && ui.content.0 != q {
        ui.content = (q.clone(), vec![]);
        ui.content_matches.clear();
    }
    for reply in answered {
        let matches = reply.ok().and_then(|v| v["matches"].as_array().cloned()).unwrap_or_default();
        ui.content.1.extend(matches.iter().filter_map(|m| m["threadId"].as_str().map(str::to_string)));
        ui.content_matches.extend(matches);
    }
    if !waiting.is_empty() {
        ui.content_req = Some((waiting, q));
    }
    changed
}

pub fn commit_rename(s: &mut App) {
    let Some((id, edit)) = s.ui.shell.sidebar.rename.take() else { return };
    let next = edit.text.trim().to_string();
    if next.is_empty() {
        crate::shell::palette::toast_warning(s, "Thread title cannot be empty", None, vec![], false);
        return;
    }
    let current = Model::build(s).thread(&id).map(|t| t.title.clone()).unwrap_or_default();
    if next == current {
        return;
    }
    dispatch(s, "thread.metadata.update", &id, json!({ "title": next }));
    patch(s, &id, |p| p.title = Some(next));
}


/// The rows a drag carries: the selection when the dragged row is in it, else that row.
pub fn dragged_ids(s: &App, model: &Model, key: &str) -> Vec<String> {
    let ui = &s.ui.shell.sidebar;
    if !ui.selected.contains(key) {
        return vec![key.to_string()];
    }
    let items = model.items(ui);
    items.iter().filter_map(|it| match it {
        Item::Thread { key, .. } if ui.selected.contains(key) => Some(key.clone()),
        _ => None,
    }).collect()
}

/// A sidebar drag dropped on the composer: each thread joins the draft as a context chip.
pub fn add_thread_contexts(s: &mut App, ids: &[String]) {
    let environment = s.state().config.as_ref().map(|c| crate::data::s(&c["environment"], "environmentId").to_string()).unwrap_or_default();
    let model = Model::build(s);
    let records: Vec<Value> = ids
        .iter()
        .filter_map(|id| model.thread(id))
        .map(|th| {
            json!({
                "version": 1, "kind": "thread", "contextId": crate::chat::composer::chips::context_id("thread", &th.id),
                "label": crate::chat::composer::chips::sanitize_label(&th.title, "thread"),
                "title": th.title, "environmentId": environment, "threadId": th.id,
            })
        })
        .collect();
    for record in records {
        crate::chat::composer::add_context(s, record);
    }
}

/// The context drag's feedback: the composer lights up as the drop target and a ghost follows
/// the pointer (`SidebarThreadDragOverlay`).
fn drag_ghost(s: &App, model: &Model, cx: &mut Cx<App>, t: &T, d: &Dragging, overlays: &mut Vec<El>) {
    use crate::theme::{self, AlphaMul, radius};
    if let Some(r) = cx.layout("composer") {
        overlays.push(el().absolute(r[0], r[1]).size(r[2], r[3]).radius(radius::XL).fill(t.c.accent.alpha_mul(0.2)).border(2.0, t.c.primary.alpha_mul(0.7)).a11y_hidden());
    }
    let n = dragged_ids(s, model, &d.key).len();
    let title = model.thread(&d.key).map(|t| t.title.clone()).unwrap_or_default();
    let mut ghost = row()
        .key("sb-drag-ghost")
        .absolute(d.at.0 + 12.0, d.at.1 + 12.0)
        .max_w(260.0)
        .gap(8.0)
        .px(10.0)
        .py(6.0)
        .center_y()
        .radius(radius::LG)
        .fill(t.c.popover)
        .border(1.0, t.c.border)
        .child(rune::icon(&crate::icons::MESSAGES_SQUARE, 14.0).color(t.c.muted_foreground))
        .child(rune::text(title).font(theme::TEXT_SM).color(t.c.foreground).truncate().shrink());
    if n > 1 {
        ghost = ghost.child(rune::text(format!("+{}", n - 1)).font(theme::XS_MEDIUM).color(t.c.muted_foreground));
    }
    overlays.push(ghost.a11y_hidden());
}

fn reorder(s: &mut App, kind: &str, assignments: &[(String, String)]) {
    for (id, key) in assignments {
        dispatch(s, kind, id, json!({ "orderKey": key }));
        let key = Some(key.clone());
        patch(s, id, |p| if kind == "thread.pin.reorder" { p.pin_key = Some(key) } else { p.active_key = Some(key) });
    }
}

/// Runs a drop plan as the web's `handleDragEnd` does: lifecycle first, then order writes. A
/// failed write rolls back the whole drop.
pub fn apply_plan(s: &mut App, id: &str, plan: Plan) {
    let mut touched = vec![id.to_string()];
    match &plan {
        Plan::Pin { extra: a, .. } | Plan::ReorderPinned { assignments: a, .. } | Plan::MoveActive { assignments: a, .. } => {
            touched.extend(a.iter().map(|(id, _)| id.clone()));
        }
        Plan::None | Plan::Settle => {}
    }
    if !matches!(plan, Plan::None) && !check_operations(s, &touched) {
        return;
    }
    s.ui.shell.sidebar.drop_group = Some(touched);
    run_plan(s, id, plan);
    s.ui.shell.sidebar.drop_group = None;
}

fn run_plan(s: &mut App, id: &str, plan: Plan) {
    match plan {
        Plan::None => {}
        Plan::Settle => settle_in_place(s, id),
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
    // A side chat of the open thread opens beside it (onOpenRelatedThread's openSideChatTab).
    if crate::chat::side::open_tab(s, id) {
        return;
    }
    let ui = &mut s.ui.shell.sidebar;
    ui.selected.clear();
    ui.anchor = Some(id.to_string());
    go(s, Route::Thread(id.to_string()));
}

/// `chat.new`: with several open projects, pick one in the palette; otherwise `chat.newLocal`.
pub fn new_thread(s: &mut App) {
    new_thread_click(s, false);
}

/// `shouldCreateNewThreadInCurrentProject`: Shift, a single open project, or none at all.
pub(crate) fn new_thread_in_current_project(s: &App, shift: bool) -> bool {
    let projects = &s.state().projects;
    shift || projects.is_empty() || projects.iter().filter(|p| p["archivedAt"].is_null()).count() == 1
}

/// The header's New thread: Shift+click starts one in the current project.
pub(crate) fn new_thread_click(s: &mut App, shift: bool) {
    if new_thread_in_current_project(s, shift) {
        new_local(s);
    } else {
        crate::shell::palette::open_new_thread_in(s);
    }
}

/// `chat.newLocal`: a new thread in the current project, else the first open one.
pub fn new_local(s: &mut App) {
    s.ui.chat.home.workspace = None;
    let current = match &s.route {
        Route::Thread(id) => s.state().threads.iter().find(|t| t["id"] == *id).map(|t| crate::data::s(t, "projectId").to_string()),
        Route::Home { project_id } => project_id.clone(),
        _ => None,
    };
    let project = current
        .filter(|p| !p.is_empty())
        .or_else(|| s.state().projects.iter().find(|p| p["archivedAt"].is_null()).map(|p| crate::data::s(p, "id").to_string()));
    match project {
        Some(id) => go(s, Route::Home { project_id: Some(id) }),
        None => crate::shell::palette::open_new_thread_in(s),
    }
}

/// The open thread's sidebar row, for the shortcuts that act on it.
fn shown_thread(s: &App) -> Option<Thread> {
    let Route::Thread(id) = &s.route else { return None };
    Model::build(s).thread(id).cloned()
}

/// `thread.settle` (toggles) and `thread.pin` (toggles) on the open thread.
fn thread_shortcut(s: &mut App, e: &KeyEvent) -> bool {
    let settle_key = crate::panel::shortcut(s, e, "thread.settle");
    if !settle_key && !crate::panel::shortcut(s, e, "thread.pin") {
        return false;
    }
    let Some(t) = shown_thread(s) else { return true };
    match (settle_key, t.settled, t.pinned) {
        (true, true, _) => unsettle(s, &t.id),
        (true, false, _) => settle(s, &t.id),
        (false, _, true) => request_unpin(s, &t.id),
        (false, _, false) => pin(s, &t.id, None),
    }
    true
}

/// Any key but `thread.undo` hands mod+z back to text fields (`releaseThreadUndoShortcut`).
pub fn release_undo_key(s: &mut App, e: &KeyEvent) {
    if !crate::panel::shortcut(s, e, "thread.undo") {
        s.ui.shell.sidebar.undos.release();
    }
}

/// Shortcuts from the server's keybindings that the web binds outside the terminal.
fn bound_key(s: &mut App, e: &KeyEvent, editable: bool) -> bool {
    if crate::panel::terminal_active(s) {
        return false;
    }
    let commands = ["navigation.back", "navigation.forward", "usage.open", "chat.newWithoutProject", "chat.newLocal", "chat.new", "thread.undo"];
    match commands.into_iter().find(|c| crate::panel::shortcut(s, e, c)) {
        Some("navigation.back") => go_back(s),
        Some("navigation.forward") => go_forward(s),
        Some("usage.open") => go(s, Route::Usage),
        // The web leaves the key alone when no server offers a scratch workspace.
        Some("chat.newWithoutProject") if !crate::shell::palette::actions::scratch_available(s) => return false,
        Some("chat.newWithoutProject") => crate::shell::palette::actions::new_without_project(s),
        Some("chat.newLocal") => new_local(s),
        // A collapsed sidebar hides the notice, so text fields keep their own undo.
        Some("thread.undo") => {
            let notice_shown = !s.ui.shell.sidebar.collapsed;
            return s.ui.shell.sidebar.undos.takes_key(crate::data::now_ms(), editable, notice_shown) && undo(s);
        }
        Some(_) => new_thread(s),
        None => return thread_shortcut(s, e),
    }
    true
}

/// Window prefs key for the dragged width (web's `chat_thread_sidebar_width`).
const WIDTH_KEY: &str = "threadSidebarWidth";
const MIN_W: f32 = 208.0;
/// The main area keeps 40rem beside the sidebar.
const MAIN_MIN_W: f32 = 640.0;

/// `clampThreadSidebarWidth`: at least 13rem and the header's brand and history buttons
/// (`resolveThreadSidebarMinimumWidth`), and no wider than leaves the main area 40rem.
fn clamp_width(width: f32, viewport: Option<f32>, brand: f32) -> f32 {
    let min = MIN_W.max(brand.ceil());
    let max = viewport.map_or(f32::MAX, |v| (v.floor() - MAIN_MIN_W).max(min));
    width.max(min).min(max).round()
}

/// The window's width and the header's natural width (`SidebarBrandWidthProbe`).
fn bounds(cx: &mut Cx<App>) -> (Option<f32>, f32) {
    (cx.layout_with("shell", |r| r[2]), chrome::brand_width(cx.titlebar_inset()))
}

/// The sidebar's content width.
pub fn width(s: &App, cx: &mut Cx<App>) -> f32 {
    let (viewport, brand) = bounds(cx);
    let ui = &s.ui.shell.sidebar;
    let stored = ui.width.get().unwrap_or_else(|| {
        let stored = crate::desktop::prefs().get::<Value>(WIDTH_KEY);
        let saved = stored.as_ref().and_then(Value::as_f64).map(|w| w as f32).filter(|w| w.is_finite());
        if stored.is_some_and(|v| !v.is_null()) && saved.is_none() {
            eprintln!("s5code-rune: Could not read persisted thread sidebar width.");
        }
        let saved = saved.unwrap_or(layout::SIDEBAR_W);
        ui.width.set(Some(saved));
        saved
    });
    clamp_width(stored, viewport, brand)
}

fn save_width(width: Option<f32>) {
    let mut prefs = crate::desktop::prefs();
    if let Some(w) = width {
        let _ = prefs.set(WIDTH_KEY, &w);
    } else if prefs.remove(WIDTH_KEY).is_err() {
        eprintln!("s5code-rune: Could not clear persisted thread sidebar width.");
    }
}

/// `SidebarRail`: drag to resize, double-click for the default width, both kept across launches.
fn rail(width: f32, (viewport, brand): (Option<f32>, f32)) -> El {
    el().key("sb-rail")
        .role("separator")
        .label("Resize Sidebar")
        .help("Drag to resize sidebar")
        .pin(None, Some(0.0), Some(0.0), Some(0.0))
        .w(6.0)
        .cursor(rune::Cursor::ResizeColumn)
        .on_drag(move |s: &mut App, d, _| {
            if d.phase == rune::Phase::Down {
                d.data = width;
                if d.clicks == 2 {
                    s.ui.shell.sidebar.width.set(Some(layout::SIDEBAR_W));
                    save_width(None);
                }
                return;
            }
            if d.cancelled() || d.clicks == 2 {
                return;
            }
            let next = clamp_width(d.data + d.dx, viewport, brand);
            s.ui.shell.sidebar.width.set(Some(next));
            if d.phase == rune::Phase::Up {
                save_width(Some(next));
            }
        })
}

/// Jump hints wait this long after the modifiers go down, so a quick shortcut never flashes them.
const JUMP_HINT_DELAY_MS: i64 = 200;

/// `shouldShowThreadJumpHintsForModifiers`: the held modifiers are exactly those of a
/// `thread.jump.N` shortcut, and the terminal does not have focus.
fn jump_modifiers(s: &App, mods: rune::Mods) -> bool {
    if crate::panel::terminal_focused(s) {
        return false;
    }
    let bindings = s.state().config.as_ref().and_then(|c| c["keybindings"].as_array().cloned());
    let Some(bindings) = bindings else { return crate::panel::modifiers_match(&json!({"modKey": true}), mods) };
    bindings
        .iter()
        .filter(|b| b["command"].as_str().is_some_and(|c| c.starts_with("thread.jump.")))
        .any(|b| crate::panel::modifiers_match(&b["shortcut"], mods))
}

/// Tracks the jump modifiers held alone; true when the hints' state changes.
pub fn on_modifier(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    let changed = hold_jump(s, e.mods);
    if changed && s.ui.shell.sidebar.jump_held.is_some() {
        tx.wake_after(std::time::Duration::from_millis(JUMP_HINT_DELAY_MS as u64));
    }
    changed
}

pub fn release_jump(s: &mut App) {
    s.ui.shell.sidebar.jump_held = None;
}

fn hold_jump(s: &mut App, mods: rune::Mods) -> bool {
    let held = jump_modifiers(s, mods);
    let ui = &mut s.ui.shell.sidebar;
    if held == ui.jump_held.is_some() {
        return false;
    }
    ui.jump_held = held.then(crate::data::now_ms);
    true
}

/// The shortcut each of the first nine visible threads jumps by, while the hints show.
pub(crate) fn jump_labels(s: &App, model: &Model) -> HashMap<String, String> {
    let ui = &s.ui.shell.sidebar;
    let shown = ui.jump_held.is_some_and(|at| crate::data::now_ms() >= at + JUMP_HINT_DELAY_MS) && !s.ui.notify.background;
    if !shown {
        return HashMap::new();
    }
    let mac = cfg!(target_os = "macos");
    let configured = s.state().config.as_ref().is_some_and(|c| c["keybindings"].is_array());
    model
        .visible_ids(ui)
        .into_iter()
        .take(9)
        .enumerate()
        .filter_map(|(i, id)| {
            let label = if configured {
                crate::shell::palette::shortcut_label(s, &format!("thread.jump.{}", i + 1))
            } else if mac {
                format!("⌘{}", i + 1)
            } else {
                format!("Ctrl+{}", i + 1)
            };
            (!label.is_empty()).then_some((id, label))
        })
        .collect()
}

/// `handleThreadFileDrop`: files dropped on a row open its thread with them attached; folders
/// are left out, as web's row drop ignores them.
pub fn drop_files(s: &mut App, id: &str, files: &[std::path::PathBuf]) {
    s.ui.shell.sidebar.drop_over = None;
    let files: Vec<String> = files.iter().filter(|p| !p.is_dir()).map(|p| p.display().to_string()).collect();
    if files.is_empty() {
        return;
    }
    open_thread(s, id);
    if matches!(&s.route, Route::Thread(open) if open == id) {
        crate::chat::composer::add_files(s, files);
    }
}

/// A thread row that takes dropped files (`makeWorkspaceFileDropHandlers`), and follows the drag.
pub(super) fn drop_target(e: El, id: &str) -> El {
    let (dropped, hovered) = (id.to_owned(), id.to_owned());
    e.on_drop_files(move |s: &mut App, files: &[std::path::PathBuf], _: &mut Tx| drop_files(s, &dropped, files))
        .on_drop_hover(move |s: &mut App, over, _: &mut Tx| drop_hover(&mut s.ui.shell.sidebar, &hovered, over))
}

fn drop_hover(ui: &mut Ui, id: &str, over: bool) {
    if over {
        ui.drop_over = Some(id.to_owned());
    } else if ui.drop_over.as_deref() == Some(id) {
        ui.drop_over = None;
    }
}

/// `ring-1 ring-inset ring-primary/70` on the row a file drag is over.
pub(super) fn drop_ring(e: El, t: &T, over: bool) -> El {
    use crate::theme::AlphaMul;
    if over { e.border(1.0, t.sb.primary.alpha_mul(0.7)) } else { e }
}

/// `selectThreadSearchResult`: opens the thread, and find on its message match as the palette does.
pub(crate) fn open_result(s: &mut App, id: &str) {
    let ui = &s.ui.shell.sidebar;
    let find = ui.content_match(id).map(|m| crate::shell::palette::find_query(crate::data::s(m, "snippet"), ui.search.text.trim()));
    clear_search(s);
    open_thread(s, id);
    if let Some(query) = find {
        crate::shell::palette::open_find(s, query);
    }
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
    if bound_key(s, e, tx.focused_editable()) {
        return true;
    }
    // The server's keybindings decide; the defaults stand in until its config arrives.
    let defaults = s.state().config.as_ref().is_none_or(|c| !c["keybindings"].is_array());
    let bound = |cmd: &str| crate::panel::shortcut(s, e, cmd);
    let jump = (1..=9).find(|n| bound(&format!("thread.jump.{n}"))).or(match e.key {
        Key::Command(c @ '1'..='9') if defaults && !m.shift => Some(c as usize - '0' as usize),
        _ => None,
    });
    let default_step = match e.key {
        Key::Command('[' | '{') if defaults && m.shift => Some(-1),
        Key::Command(']' | '}') if defaults && m.shift => Some(1),
        _ => None,
    };
    let toggle = bound("sidebar.toggle") || (defaults && e.key == Key::Command('b') && !m.shift);
    let by = if bound("thread.previous") {
        Some(-1)
    } else if bound("thread.next") {
        Some(1)
    } else {
        default_step
    };
    match &e.key {
        _ if toggle => s.ui.shell.sidebar.collapsed ^= true,
        _ if by.is_some() => step(s, by.unwrap_or(1)),
        _ if jump.is_some() => {
            let ids = Model::build(s).visible_ids(&s.ui.shell.sidebar);
            if let Some(id) = jump.and_then(|n| ids.get(n - 1)) {
                open_thread(s, &id.clone());
            }
        }
        Key::Escape if s.ui.shell.sidebar.folders.menu.is_some() => s.ui.shell.sidebar.folders.menu = None,
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
        logic::search(&model.threads, &s.ui.shell.sidebar.search.text, s.ui.shell.sidebar.content_hits()).into_iter().map(|i| model.threads[i].id.clone()).collect()
    };
    let ui = &mut s.ui.shell.sidebar;
    match e.key {
        Key::Escape => clear_search(s),
        Key::Down => ui.highlight = (ui.highlight + 1).min(hits.len().saturating_sub(1)),
        Key::Up => ui.highlight = ui.highlight.saturating_sub(1),
        Key::Enter => {
            if let Some(id) = hits.get(ui.highlight).cloned() {
                open_result(s, &id);
            }
        }
        _ => return false,
    }
    true
}

/// The thread menu from the chat header: under the title, or at a right-click point.
pub fn open_header_menu(s: &mut App, thread: String, at: Option<(f32, f32)>) {
    let mut m = match at {
        Some((x, y)) => menu::Menu::new(thread, x, y),
        None => menu::Menu { anchor: Some("topbar-title".into()), ..menu::Menu::new(thread, 0.0, 0.0) },
    };
    m.header = true;
    s.ui.shell.sidebar.menu = Some(m);
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &T, overlays: &mut Vec<El>) -> El {
    let ui = &s.ui.shell.sidebar;
    // AppSidebarLayout drops the traffic-light inset in full screen, where the lights hide.
    let inset = if crate::desktop::is_fullscreen() { 0.0 } else { cx.titlebar_inset() };
    if ui.collapsed {
        overlays.push(chrome::floating_toggle(t, inset));
        // The header's thread menu and its delete confirmation work with the sidebar hidden.
        if ui.menu.is_some() {
            let model = Model::build(s);
            if let Some(m) = &ui.menu
                && let Some(th) = model.thread(&m.thread)
            {
                overlays.extend(menu::view(s, cx, &model, th, m, t));
            }
        }
        share::dialog(s, t, overlays);
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
    row::hover_card(s, &model, t, overlays);
    everything::overlays(s, t, overlays);
    if let Some(d) = ui.drag.as_ref().filter(|d| d.context) {
        drag_ghost(s, &model, cx, t, d, overlays);
    }
    share::dialog(s, t, overlays);
    snooze::dialog(s, t, overlays);
    chrome::scope_menu(s, &model, t, overlays);
    folders::menu(s, t, overlays);
    drafts::menu(s, t, overlays);
    // The width plus 1pt of content and a 1pt border: the web's `w-64` plus its `border-r` and rail.
    let w = width(s, cx);
    let art = stage::variant(s).and_then(|v| stage::backdrop(s, t, v, w + 1.0, cx.env().scale));
    let content = col()
        .w(w + 1.0)
        .h_full()
        .child(chrome::header(s, t, inset, art, stage::pill(s)))
        .child(chrome::search_bar(s, t))
        .child(list)
        .child(chrome::footer(s, &model, t));
    row()
        .key("sidebar")
        .w(w + 2.0)
        .h_full()
        .fill(t.sb.sidebar)
        .child(content)
        .child(el().w(1.0).h_full().fill(t.sb.contrast_sidebar_border))
        .child(rail(w, bounds(cx)))
}

impl Shelf {
    pub fn section(self) -> Section {
        match self {
            Shelf::Working => Section::Working,
            Shelf::Snoozed => Section::Snoozed,
            Shelf::Settled => Section::Settled,
        }
    }

    /// The header's element key.
    pub fn key(self) -> String {
        let name = match self {
            Shelf::Working => "working",
            Shelf::Snoozed => "snoozed",
            Shelf::Settled => "settled",
        };
        format!("sb-shelf-{name}")
    }
}

/// `sidebarTree` Right on an open folder: its first row.
pub fn folder_child(model: &Model, ui: &Ui, group: &str) -> Option<String> {
    let folder = model.folders.iter().find(|f| f.group.key == group)?;
    folders::shown(folder, model, &ui.folders).first().map(|i| format!("sb-row-{}", model.threads[*i].id))
}

/// `sidebarTree` Right on an open shelf: its first row.
pub fn shelf_child(model: &Model, ui: &Ui, shelf: Shelf) -> Option<String> {
    model.items(ui).into_iter().find_map(|it| match it {
        Item::Thread { key, section } if section == shelf.section() => Some(format!("sb-row-{key}")),
        _ => None,
    })
}

/// `sidebarTree` Left on a row: its shelf or folder header.
pub fn tree_parent(model: &Model, id: &str) -> Option<String> {
    match model.section_of(id)? {
        Section::Working => Some(Shelf::Working.key()),
        Section::Snoozed => Some(Shelf::Snoozed.key()),
        Section::Settled => Some(Shelf::Settled.key()),
        Section::Active => model.folders.iter().find(|f| f.rows.iter().any(|i| model.threads[*i].id == id)).map(|f| format!("sb-folder-{}", f.group.key)),
        Section::Pinned => None,
    }
}

/// Moves keyboard focus to a sidebar row by its element key.
pub fn focus_row(s: &mut App, key: String, tx: &mut Tx) {
    tx.scroll_into_view(key.clone());
    s.ui.shell.sidebar.focus_to = Some(key);
}

/// Opens or folds a shelf; the Settled shelf remembers it.
pub fn set_shelf(s: &mut App, shelf: Shelf, open: bool) {
    let ui = &mut s.ui.shell.sidebar;
    if open {
        ui.expanded.insert(shelf);
    } else {
        ui.expanded.remove(&shelf);
    }
    if shelf == Shelf::Settled {
        ui.folders.set_settled(open);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(id: &str) -> Thread {
        Thread::from_shell(&json!({"id": id, "projectId": "p", "title": id}))
    }

    #[test]
    fn a_snooze_time_that_passed_while_the_menu_was_open_warns() {
        let mut s = App::new();
        snooze_picked(&mut s, &["t".to_owned()], crate::data::now_ms() - 1);
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Could not snooze thread"));
        assert_eq!(crate::shell::palette::toast_description(&s), Some("That snooze time has passed. Choose another time."));
        assert!(s.ui.shell.sidebar.inflight.is_empty(), "FAILED: nothing is sent");
    }

    #[test]
    fn files_dropped_on_a_row_open_its_thread_and_folders_alone_do_nothing() {
        let mut s = App::new();
        s.route = Route::Home { project_id: None };
        let id = crate::data::s(&s.state().threads[0], "id").to_string();
        drop_hover(&mut s.ui.shell.sidebar, &id, true);
        drop_hover(&mut s.ui.shell.sidebar, "other", false);
        assert_eq!(s.ui.shell.sidebar.drop_over.as_deref(), Some(id.as_str()), "leaving another row keeps this one lit");
        drop_files(&mut s, &id, &[std::env::temp_dir()]);
        assert!(matches!(s.route, Route::Home { .. }), "a folder alone is ignored");
        assert_eq!(s.ui.shell.sidebar.drop_over, None, "the drop clears the highlight");
        let file = std::env::temp_dir().join("s5code-rune-drop-test.txt");
        std::fs::write(&file, "x").unwrap();
        drop_files(&mut s, &id, &[file.clone()]);
        assert!(matches!(&s.route, Route::Thread(open) if *open == id));
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn the_width_stays_between_13rem_and_what_leaves_the_main_area_40rem() {
        assert_eq!(clamp_width(100.0, None, 0.0), MIN_W);
        assert_eq!(clamp_width(300.4, Some(1400.0), 0.0), 300.0);
        assert_eq!(clamp_width(900.0, Some(1200.0), 0.0), 560.0);
        assert_eq!(clamp_width(300.0, Some(700.0), 0.0), MIN_W, "a narrow window still keeps the minimum");
        assert_eq!(clamp_width(100.0, None, chrome::brand_width(28.0)), 245.0, "the macOS header's brand and history buttons stay whole");
    }

    #[test]
    fn jump_hints_follow_the_jump_shortcuts_modifiers_after_a_moment() {
        let mut s = App::new();
        let mods = |cmd: bool, ctrl: bool, shift: bool| rune::Mods { cmd, ctrl, shift, ..Default::default() };
        let (cmd, ctrl) = if cfg!(target_os = "macos") { (true, false) } else { (true, true) };
        assert!(!hold_jump(&mut s, mods(false, false, true)), "Shift alone is not the jump modifier");
        assert!(hold_jump(&mut s, mods(cmd, ctrl, false)));
        let model = Model::build(&s);
        assert!(jump_labels(&s, &model).is_empty(), "not before the delay");
        s.ui.shell.sidebar.jump_held = Some(crate::data::now_ms() - JUMP_HINT_DELAY_MS);
        let labels = jump_labels(&s, &model);
        let first = model.visible_ids(&s.ui.shell.sidebar)[0].clone();
        assert_eq!(labels.get(&first).map(String::as_str), Some(if cfg!(target_os = "macos") { "⌘1" } else { "Ctrl+1" }));
        assert!(hold_jump(&mut s, mods(cmd, ctrl, true)), "adding Shift hides them");
        assert!(jump_labels(&s, &model).is_empty());
    }

    #[test]
    fn enter_on_a_message_match_opens_find_on_the_snippets_spelling() {
        let mut s = App::new();
        let id = crate::data::s(&s.state().threads[0], "id").to_string();
        s.ui.shell.sidebar.search = TextEdit::new("needle");
        s.ui.shell.sidebar.content = ("needle".into(), vec![id.clone()]);
        s.ui.shell.sidebar.content_matches = vec![json!({"threadId": id, "snippet": "a Needle here", "source": "user"})];
        assert!(!s.ui.shell.sidebar.content_pending());
        open_result(&mut s, &id);
        assert!(matches!(&s.route, Route::Thread(open) if *open == id));
        assert!(s.ui.shell.sidebar.search.text.is_empty());
        assert_eq!(s.ui.chat.timeline.find.edit.text, "Needle");
    }

    #[test]
    fn tree_arrows_find_the_first_child_and_the_parent() {
        let group = folders::Group { key: "g".into(), label: "app".into(), members: vec!["p".into()], representative: "p".into(), root: "/p".into(), pinned: false, archived: false };
        let model = Model {
            threads: ["pin", "a1", "a2", "s1", "s2"].map(thread).to_vec(),
            sections: Sections { pinned: vec![0], active: vec![1, 2], settled: vec![3, 4], ..Default::default() },
            projects: HashMap::new(),
            drivers: HashMap::new(),
            route: None,
            now: 0,
            working_shelf: false,
            updates: 0,
            folders: vec![folders::Folder { group, rows: vec![1, 2] }],
        };
        let mut ui = Ui::default();
        assert_eq!(folder_child(&model, &ui, "g").as_deref(), Some("sb-row-a1"));
        assert_eq!(folder_child(&model, &ui, "missing"), None);
        assert_eq!(shelf_child(&model, &ui, Shelf::Settled), None, "a folded shelf has no rows to enter");
        ui.expanded.insert(Shelf::Settled);
        assert_eq!(shelf_child(&model, &ui, Shelf::Settled).as_deref(), Some("sb-row-s1"));
        assert_eq!(tree_parent(&model, "a2").as_deref(), Some("sb-folder-g"));
        assert_eq!(tree_parent(&model, "s2").as_deref(), Some("sb-shelf-settled"));
        assert_eq!(tree_parent(&model, "pin"), None, "pinned rows sit at the top level");
    }

    #[test]
    fn a_row_dragged_to_the_lists_bottom_edge_scrolls_the_list() {
        fn view(s: &App, cx: &mut Cx<App>) -> El {
            el().size(300., 500.).child(super::view(s, cx, &crate::theme::theme(true), &mut Vec::new()))
        }
        let mut app = App::new();
        app.client_settings.values.insert("boringSidebarEnabled".into(), json!(false));
        app.client_settings.values.insert("legacySidebarEnabled".into(), json!(false));
        let active = |app: &App| {
            let model = Model::build(app);
            model.items(&app.ui.shell.sidebar).into_iter().filter_map(|it| match it { Item::Thread { key, section: Section::Active } => Some(key), _ => None }).collect::<Vec<_>>()
        };
        let seed = active(&app).first().cloned().expect("the fixture has an active row");
        let crate::data::Source::Fixture(state) = &mut app.src else { unreachable!("App::new runs on fixtures") };
        let row = state.threads.iter().find(|t| t["id"] == seed.as_str()).cloned().unwrap();
        for i in 0..40 {
            let mut t = row.clone();
            t["id"] = json!(format!("overflow-{i}"));
            t["title"] = json!(format!("Overflow {i}"));
            state.threads.push(t);
        }
        let keys = active(&app);
        assert!(keys.len() > 40, "the clones are active rows");
        let mut rt = rune::Runtime::new(app, view, (300., 500.), rune::Env::new(true, 1.));
        for _ in 0..3 {
            rt.frame(1. / 60.);
        }
        let viewport = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("sb-scroll")).expect("the list").rect;
        let list = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("sb-list")).expect("the rows").rect;
        assert!(list[3] > viewport[3] + 100., "FAILED: the list keeps its content height, taller than the viewport: {list:?} in {viewport:?}");
        let rows = |rt: &rune::Runtime<App>| rt.elements().into_iter().filter(|e| !e.exiting && e.key.as_deref().is_some_and(|k| k.starts_with("sb-row-"))).collect::<Vec<_>>();
        let visible: Vec<_> = rows(&rt)
            .into_iter()
            .filter(|e| keys.iter().any(|k| e.key.as_deref() == Some(format!("sb-row-{k}").as_str())) && e.rect[1] >= viewport[1] && e.rect[1] + e.rect[3] <= viewport[1] + viewport[3] * 0.5)
            .collect();
        let (anchor, grab) = (visible[0].clone(), visible.get(1).cloned().expect("two active rows in the list's top half"));
        let anchor_y = |rt: &rune::Runtime<App>| rt.elements().into_iter().find(|e| e.key == anchor.key).map_or(f32::NAN, |e| e.rect[1]);
        let (x, y) = (grab.rect[0] + grab.rect[2] / 2., grab.rect[1] + grab.rect[3] / 2.);
        rt.pointer_move(x, y);
        rt.frame(1. / 60.);
        assert!(rt.state.ui.shell.sidebar.hover.is_some(), "hovering the row");
        rt.frame(1. / 60.);
        rt.pointer_down(x, y);
        rt.frame(1. / 60.);
        assert!(rt.state.ui.shell.sidebar.press.is_some(), "the press keeps the row's rect: hover_rect {:?}", rt.state.ui.shell.sidebar.hover_rect.get());
        let edge = viewport[1] + viewport[3] * 0.95;
        let state = |rt: &rune::Runtime<App>| {
            let ui = &rt.state.ui.shell.sidebar;
            let list = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("sb-list")).map(|e| (e.rect, e.scrolled));
            let low: Vec<_> = rt
                .elements()
                .into_iter()
                .filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("sb-li-")) && e.rect[1] + e.rect[3] > viewport[1] + viewport[3] - 90.)
                .take(5)
                .map(|e| (e.key.unwrap_or_default(), e.rect[1], e.rect[3], e.target[1], e.scrolled[1]))
                .collect();
            format!(
                "drag {:?} edge {} over {:?} target {:?} anchor y {} viewport {viewport:?} list {list:?} low {low:?}",
                ui.drag.as_ref().map(|d| (d.key.clone(), d.at, d.dy, d.context, d.scroll)),
                ui.drag_edge.get(),
                ui.drag_over.borrow(),
                ui.drag_target.borrow().as_ref().map(|t| t.section),
                anchor_y(rt),
            )
        };
        for step in 1..=60 {
            let to = (y + step as f32 * 12.).min(edge) - (step % 2) as f32;
            rt.pointer_move(x, to);
            rt.frame(1. / 60.);
            if step == 3 {
                assert!(rt.state.ui.shell.sidebar.drag.as_ref().is_some_and(|d| !d.context), "a reorder drag: {}", state(&rt));
            }
        }
        let after = state(&rt);
        let scrolled = rows(&rt).iter().map(|e| e.scrolled[1]).fold(0., f32::max);
        assert_eq!(rt.state.ui.shell.sidebar.drag_edge.get(), 1, "FAILED: the pointer sits at the bottom edge: {after}");
        rt.pointer_up(x, edge);
        rt.frame(1. / 60.);
        assert!(scrolled > 0., "FAILED: holding a dragged row at the bottom edge scrolls the list: {after}");
    }

    #[test]
    fn right_enters_a_folder_and_left_returns_to_it() {
        fn view(s: &App, cx: &mut Cx<App>) -> El {
            el().size(300., 900.).child(super::view(s, cx, &crate::theme::theme(true), &mut Vec::new()))
        }
        let mut app = App::new();
        app.client_settings.values.insert("boringSidebarEnabled".into(), json!(true));
        let mut rt = rune::Runtime::new(app, view, (300., 900.), rune::Env::new(true, 1.));
        rt.frame(0.);
        let press = |rt: &mut rune::Runtime<App>, key: Key| {
            rt.key(KeyEvent { key, mods: Default::default() });
            for _ in 0..3 {
                rt.frame(0.);
            }
        };
        let focused = |rt: &rune::Runtime<App>| rt.elements().into_iter().find(|e| e.focused).and_then(|e| e.key).unwrap_or_default();
        for _ in 0..60 {
            let at = focused(&rt);
            if at.starts_with("sb-folder-") {
                break;
            }
            press(&mut rt, if at.starts_with("sb-row-") || at.starts_with("sb-shelf-") { Key::Down } else { Key::Tab });
        }
        let header = focused(&rt);
        let group = header.strip_prefix("sb-folder-").expect("the keyboard reaches a folder").to_string();
        let first = folder_child(&Model::build(&rt.state), &rt.state.ui.shell.sidebar, &group).expect("the fixture's folder has rows");
        press(&mut rt, Key::Right);
        assert_eq!(focused(&rt), first, "Right on an open folder enters it");
        assert_eq!(rt.state.ui.shell.sidebar.focus_to, None, "the request clears after one layout");
        press(&mut rt, Key::Left);
        assert_eq!(focused(&rt), header, "Left on a row returns to its folder");
        press(&mut rt, Key::Left);
        assert!(!rt.state.ui.shell.sidebar.folders.expanded(&group), "Left on an open folder folds it");
        press(&mut rt, Key::Right);
        assert!(rt.state.ui.shell.sidebar.folders.expanded(&group), "Right on a folded folder opens it");
        assert_eq!(focused(&rt), header, "opening keeps focus on the folder");
    }

    #[test]
    fn a_failed_command_toasts_web_s_title_and_rolls_back_its_drop() {
        assert_eq!(failure_title("thread.metadata.update", &json!({"regenerateTitle": true})), Some("Failed to regenerate thread title"));
        assert_eq!(failure_title("thread.metadata.update", &json!({"title": "x"})), Some("Failed to rename thread"));
        assert_eq!(failure_title("thread.pin.reorder", &json!({})), Some("Failed to reorder pinned threads"));
        assert_eq!(failure_title("thread.mark-unread", &json!({})), None, "FAILED: web toasts nothing for mark unread");

        let mut s = App::new();
        for id in ["a", "b", "c"] {
            patch(&mut s, id, |p| p.pinned = Some(true));
        }
        let done = Inflight { request: 1, thread: "a".into(), title: "Failed to pin thread", rollback: vec!["a".into(), "b".into()], batch: None, quiet: false };
        failed(&mut s, &done, &json!({"message": "denied"}));
        let ui = &s.ui.shell.sidebar;
        assert!(!ui.patches.contains_key("a") && !ui.patches.contains_key("b"), "FAILED: the dropped group returns to the server's order");
        assert!(ui.patches.contains_key("c"), "FAILED: an unrelated patch stays");
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Failed to pin thread"));
    }

    #[test]
    fn a_bulk_action_s_failures_share_one_toast() {
        let mut s = App::new();
        s.src = crate::data::Source::Live(s5_net::Net::in_memory("http://local.test"));
        s.src.net().expect("live").state_mut().threads = vec![json!({"id": "a"}), json!({"id": "b"})];
        dispatch(&mut s, "thread.delete", "a", json!({}));
        batched(&mut s, "Failed to delete threads", |s| {
            dispatch(s, "thread.delete", "a", json!({}));
            dispatch(s, "thread.delete", "b", json!({}));
        });
        let titles: Vec<_> = s.ui.shell.sidebar.inflight.iter().map(|f| (f.title, f.batch.is_some())).collect();
        assert_eq!(titles, [("Failed to delete thread", false), ("Failed to delete threads", true), ("Failed to delete threads", true)]);
        let first = s.ui.shell.sidebar.inflight.remove(1);
        failed(&mut s, &first, &json!({"message": "denied"}));
        assert_eq!(crate::shell::palette::toast_description(&s), Some("denied"));
        let flags: Vec<bool> = s.ui.shell.sidebar.inflight.iter().map(|f| f.quiet).collect();
        assert_eq!(flags, [false, true], "FAILED: the rest of the bulk action stays quiet, a lone delete does not");
    }

    #[test]
    fn a_session_without_operate_blocks_thread_actions_with_web_s_toast() {
        let mut s = App::new();
        let id = Model::build(&s).threads[0].id.clone();
        assert!(check_operations(&mut s, &[id.clone()]), "FAILED: no session answer yet lets the server decide");
        crate::chat::details::set_session(&mut s, Some(Ok(json!({"authenticated": true, "permissions": ["orchestration:read"]}))));
        assert!(!can_operate_id(&s, &id));
        assert!(!check_operations(&mut s, &[id]), "FAILED: a read-only session cannot change threads");
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Thread action unavailable"));
    }

    #[test]
    fn bulk_unpin_asks_for_each_thread_in_turn() {
        let mut s = App::new();
        s.client_settings.values.insert("confirmThreadUnpin".into(), json!(true));
        request_unpin_many(&mut s, vec!["a".into(), "b".into()]);
        let asked = |s: &App| crate::shell::confirm::current(s).map(str::to_owned);
        assert_eq!(asked(&s), Some("Unpin thread \"\"?\nThis will move the thread out of your pinned section.".into()), "FAILED: the first unpin asks");
        crate::shell::confirm::respond(&mut s, false);
        assert!(asked(&s).is_some(), "FAILED: answering asks the next");
        crate::shell::confirm::respond(&mut s, false);
        assert_eq!(asked(&s), None);
    }

    #[test]
    fn the_update_button_counts_providers_a_click_can_update_once_per_driver() {
        let provider = |driver: &str, extra: Value| {
            let mut p = json!({"driver": driver, "enabled": true, "versionAdvisory": {"status": "behind_latest", "canUpdate": true, "updateCommand": "npm i -g x"}});
            for (k, v) in extra.as_object().unwrap() {
                p[k] = v.clone();
            }
            p
        };
        let providers = vec![
            provider("codex", json!({})),
            provider("codex", json!({})),
            provider("claude", json!({"versionAdvisory": {"status": "behind_latest", "canUpdate": false, "updateCommand": null}})),
            provider("cursor", json!({"enabled": false})),
            provider("grok", json!({"compatibilityAdvisory": {"latestVersionStatus": "broken"}})),
            provider("kiro", json!({"versionAdvisory": {"status": "current"}})),
        ];
        assert_eq!(update_count(&providers), 1, "FAILED: only codex can be updated, once");
    }
}
