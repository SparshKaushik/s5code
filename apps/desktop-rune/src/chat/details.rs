//! The thread details card (ThreadDetailsCard): a glass card floating at the top right of the
//! chat area with the run context, editor and script actions, and version control. It also owns
//! the chat lane geometry (chatCanvasLayout.ts, threadDetailsCardLayout.ts), since the card is
//! what narrows the lane.

use rune::tok::TextStyle;
use rune::{Align, Color, Cursor, Cx, El, Key, KeyEvent, Side, TextEdit, Tx, col, el, icon, row, text, text_input};
use serde_json::{Value, json};

use crate::chat::composer;
use crate::data;
use crate::icons;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route};
use std::collections::{HashMap, HashSet};
use crate::shell::palette;

pub mod automations;
pub mod child_work;
pub mod cleanup;
pub mod editor_icons;
pub(crate) mod git;
pub mod html_render;
pub mod items;
pub mod pr;
pub mod run_on;
pub mod scripts;
pub mod servers;
pub(crate) mod lineage;
pub(crate) mod strip;
pub(crate) mod usage;

#[derive(Default)]
pub struct Ui {
    /// Threads (by draft key) whose inline card the user closed (`inlineOpen: false`).
    closed: HashSet<String>,
    /// The thread whose card is open as a popover, when the lane is too narrow for it inline.
    popover: Option<String>,
    pub menu: Option<Menu>,
    /// The context strip control the open run context or branch menu opened from.
    strip: Option<&'static str>,
    lineage: lineage::State,
    /// Refs from `vcs.listRefs` for `refs_query`, page after page; `None` while the first loads.
    refs: Option<Vec<Ref>>,
    refs_query: String,
    /// The next page's cursor, and how many refs match in all ("Showing N of M refs").
    refs_next: Option<u64>,
    refs_total: u64,
    /// The `vcs.listRefs` in flight: its request, query, and whether it is a next page.
    listing: Option<(u64, String, bool)>,
    /// Set by the list's scroll when it nears the end going down (`shouldLoadNextBranchPageAfterScroll`).
    near_end: std::rc::Rc<std::cell::Cell<bool>>,
    last_scroll: std::rc::Rc<std::cell::Cell<Option<f32>>>,
    branch_query: TextEdit,
    /// The highlighted branch picker row.
    branch_active: usize,
    /// A branch row's right-click menu: the branch and where it opened.
    branch_copy: Option<(String, (f32, f32))>,
    pending: Option<(u64, Pending)>,
    /// Where each new-thread draft runs (BranchToolbar's env mode), by draft key.
    drafts: HashMap<String, DraftRun>,
    run_on: run_on::State,
    cleanup: cleanup::State,
    pub git: git::State,
    pub scripts: scripts::State,
    pub checkout: Option<pr::Checkout>,
    /// "Show N more" under the thread's pull request.
    pr_expanded: bool,
    /// Each PR row's detail, checks and action (ThreadDetailsPrRow's queries), by url.
    pub prs: HashMap<String, pr::Live>,
    /// The PR row whose "Merge {singular}?" confirm is open.
    pub merge_confirm: Option<String>,
    /// The row (by url) whose checks popover is open, and whether it lists every check.
    pub checks_open: Option<String>,
    pub checks_all: bool,
    /// A row's Resolve or Fix handoff preparing its checkout: request, kind, the task prompt.
    pub handoff: Option<(u64, &'static str, String)>,
    /// Branch mismatches (by `mismatch_key`) shown once the composer had content, and the ones
    /// dismissed for this session.
    mismatch_shown: HashSet<String>,
    mismatch_dismissed: HashSet<String>,
    /// "Switch to {branch}?" over uncommitted changes, and the side chat whose notice asked.
    restore_confirm: Option<(String, Option<String>)>,
    /// This connection's session (`GET /api/auth/session`): the reply, when it came, and the
    /// read in flight. Its grants gate git actions, branch changes and the diff panel.
    session: Option<Result<Value, ()>>,
    session_at: i64,
    session_pending: Option<u64>,
    /// Whether the card fits inline in the last laid-out chat area, for `threadPanel.toggle`.
    fits: std::cell::Cell<bool>,
    /// The "Show in thread details" menu, where it opened.
    pub items_menu: Option<items::At>,
    pub servers: servers::State,
    pub automations: automations::State,
    pub child_work: child_work::State,
    pub html_render: html_render::State,
}

const SESSION_STALE_MS: i64 = 5 * 60_000;

/// The connection's `permissionAtom` for a scope. Unknown until the session answers, which
/// leaves it to the server; a failed read denies, as web's guards do.
pub(crate) fn set_session(s: &mut App, session: Option<Result<Value, ()>>) {
    s.ui.chat.details.session = session;
}

pub fn grants(s: &App, scope: &str) -> bool {
    match &s.ui.chat.details.session {
        Some(Ok(session)) => crate::settings::session_grants(session, scope),
        Some(Err(())) => false,
        None => true,
    }
}

/// useEnvironmentPermission's three states: checking until the session answers (the recorded
/// scenes never ask), then allowed or not.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Access {
    Pending,
    Granted,
    Denied,
}

pub fn access(s: &App, scope: &str) -> Access {
    match &s.ui.chat.details.session {
        Some(Ok(session)) if crate::settings::session_grants(session, scope) => Access::Granted,
        Some(_) => Access::Denied,
        None if matches!(s.src, data::Source::Live(_)) => Access::Pending,
        None => Access::Granted,
    }
}

/// An item the card shows on a server thread (`!draftId && visible[id]`).
fn shows(s: &App, id: &str) -> bool {
    composer::ctx(s).thread.is_some() && items::visible(&s.client_settings.values, id)
}

/// The first object in a reply tagged `tag`, such as a typed RPC error inside its Effect cause.
fn tagged<'a>(v: &'a Value, tag: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) if map.get("_tag").and_then(Value::as_str) == Some(tag) => Some(v),
        Value::Object(map) => map.values().find_map(|v| tagged(v, tag)),
        Value::Array(rows) => rows.iter().find_map(|v| tagged(v, tag)),
        _ => None,
    }
}

/// Scenes: the Replay showcase thread, whose card shows inline; its id.
fn scene_thread(s: &mut App) -> Option<String> {
    crate::scene_clock(s);
    let id = s.state().threads.iter().find(|t| t["title"] == "Replay showcase").map(|t| data::s(t, "id").to_string())?;
    s.navigate(crate::Route::Thread(id.clone()));
    Some(id)
}

/// ThreadDetailsSection: a titled list under a hairline when a section with content is above it.
pub(crate) fn section(t: &T, title: &str, actions: Option<El>, divided: bool, body: El) -> El {
    let heading = row()
        .w_full()
        .min_h(32.0)
        .mb(4.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .child(text(title.to_string()).font(theme::weight(theme::TEXT_2XS, theme::MEDIUM)).color(t.c.muted_foreground).truncate().grow(1.0).shrink().min_w(0.0))
        .children(actions);
    let mut out = col().w_full().px(8.0).pt(8.0).pb(10.0).label(title.to_string());
    if divided {
        out = out.child(el().absolute(0.0, 0.0).w_full().h(1.0).fill(t.c.border.alpha_mul(0.65)));
    }
    out.child(heading).child(body)
}

fn poll_session(s: &mut App) -> bool {
    let Some(net) = s.src.net() else { return false };
    let ui = &mut s.ui.chat.details;
    if let Some(reply) = ui.session_pending.and_then(|id| net.take_reply(id)) {
        ui.session_pending = None;
        ui.session_at = data::now_ms();
        ui.session = Some(reply.map_err(|_| ()));
        return true;
    }
    if ui.session_pending.is_none() && (ui.session_at == 0 || data::now_ms() - ui.session_at >= SESSION_STALE_MS) {
        ui.session_pending = Some(net.http_request("GET", "/api/auth/session", json!({})));
    }
    false
}

enum Pending {
    /// A switch or create; its reply moves the thread onto the branch, in that worktree. The
    /// title is the toast for a failure.
    Branch(String, Option<String>, &'static str, Option<BranchTarget>),
}

pub(super) enum BranchTarget {
    Thread { id: String, worktree: Option<String> },
    Draft { project: String, run: DraftRun },
}

impl BranchTarget {
    fn current(s: &App) -> Option<Self> {
        let ctx = composer::ctx(s);
        if let Some(thread) = ctx.thread {
            return Some(Self::Thread {
                id: data::s(thread, "id").to_string(),
                worktree: thread["worktreePath"].as_str().map(str::to_string),
            });
        }
        let project = data::s(ctx.project?, "id").to_string();
        Some(Self::Draft { project, run: draft_default(s) })
    }

    fn is_current(&self, s: &App) -> bool {
        let ctx = composer::ctx(s);
        match self {
            Self::Thread { id, .. } => ctx.thread.is_some_and(|t| data::s(t, "id") == id.as_str()),
            Self::Draft { project, .. } => ctx.thread.is_none() && ctx.project.is_some_and(|p| data::s(p, "id") == project.as_str()),
        }
    }

    fn set(&self, s: &mut App, branch: &str, worktree: Option<String>) {
        match self {
            Self::Thread { id, .. } => {
                if let Some(net) = s.src.net() {
                    net.dispatch(json!({"type": "thread.metadata.update", "threadId": id, "branch": branch, "worktreePath": worktree}));
                }
            }
            Self::Draft { project, .. } if worktree.is_some() => {
                let mut strategy = json!({"type": "existing_worktree", "worktreePath": worktree});
                if !branch.is_empty() {
                    strategy["branch"] = json!(branch);
                }
                s.ui.chat.home.workspace = Some((project.clone(), strategy));
            }
            _ => {}
        }
    }
}

/// A row of `vcs.listRefs` (VcsRef).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Ref {
    pub name: String,
    pub remote: bool,
    pub current: bool,
    pub default: bool,
    pub worktree: Option<String>,
}

impl Ref {
    fn from(v: &Value) -> Self {
        Ref {
            name: data::s(v, "name").to_string(),
            remote: v["isRemote"] == true,
            current: v["current"] == true,
            default: v["isDefault"] == true,
            worktree: v["worktreePath"].as_str().filter(|p| !p.is_empty()).map(str::to_string),
        }
    }

    /// `BranchPickerRefItem`'s badge.
    fn badge(&self, project_cwd: &str) -> Option<&'static str> {
        if self.current {
            Some("current")
        } else if self.worktree.as_deref().is_some_and(|w| w != project_cwd) {
            Some("worktree")
        } else if self.remote {
            Some("remote")
        } else if self.default {
            Some("default")
        } else {
            None
        }
    }
}

/// A draft's run context: a new worktree from `base` (else the checkout's branch), or the checkout.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DraftRun {
    pub worktree: bool,
    pub base: Option<String>,
    pub from_origin: bool,
    pub selection: run_on::Selection,
    /// The machine "Auto balance" moved the draft to (`loadBalancedEnvironmentId`).
    pub balanced: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Menu {
    Editor,
    Git,
    Branch,
    Scripts,
    RunContext,
    /// The workspace row's context menu: "Copy full path".
    Path,
    /// The Lineage section's "More thread actions".
    Lineage,
}

pub const GAP: f32 = 12.0;
pub const WIDTH: f32 = theme::layout::THREAD_DETAILS_W;
const CLEARANCE: f32 = 32.0;
const LANE_PADDING: f32 = 48.0;
const MIN_CHAT: f32 = 640.0;

/// How the card shows (`detailsCardMode` and `selectThreadPanelOpen`): inline beside the lane
/// when it fits and the thread hasn't closed it, else as a popover under the topbar toggle
/// once opened there. With no project there is no thread to describe.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shown {
    None,
    Inline,
    Popover,
}

pub fn shown(s: &App, w: f32, h: f32) -> Shown {
    if composer::ctx(s).project.is_none() {
        return Shown::None;
    }
    let key = composer::draft_key(s, s.frame_route());
    match lane(w, h, false).card {
        true if !s.ui.chat.details.closed.contains(&key) => Shown::Inline,
        false if s.ui.chat.details.popover.as_ref() == Some(&key) => Shown::Popover,
        _ => Shown::None,
    }
}

/// The topbar toggle: flips the inline card for this thread, or its popover when it can't fit.
pub fn toggle(s: &mut App, fits: bool) {
    let key = composer::draft_key(s, s.frame_route());
    let d = &mut s.ui.chat.details;
    d.menu = None;
    if fits {
        if !d.closed.remove(&key) {
            d.closed.insert(key);
        }
    } else {
        d.popover = if d.popover.as_ref() == Some(&key) { None } else { Some(key) };
    }
}

/// Where the chat column sits in a chat area `w` by `h`, and whether the card shows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Lane {
    pub left: f32,
    pub width: f32,
    pub card: bool,
}

pub fn lane(w: f32, h: f32, hidden: bool) -> Lane {
    lane_max(w, h, hidden, theme::layout::CHAT_MAX_W)
}

/// [`lane`] with the column at most `max` wide (`--chat-content-max-width`).
pub fn lane_max(w: f32, h: f32, hidden: bool, max: f32) -> Lane {
    let x = w - WIDTH - GAP;
    let card = !hidden && x - CLEARANCE - LANE_PADDING >= MIN_CHAT && h - 2.0 * GAP >= 160.0;
    let centered = (w - LANE_PADDING * 2.0).min(max).max(0.0);
    let right = if card { x - CLEARANCE } else { w - LANE_PADDING };
    let width = centered.min(right - LANE_PADDING).max(0.0);
    let left = ((w - width) / 2.0).min(right - width).max(LANE_PADDING);
    Lane { left, width, card }
}

/// The Chat width setting's `--chat-content-max-width`: 46rem, 72rem, or the whole chat area.
pub fn max_width(setting: Option<&str>) -> f32 {
    match setting {
        Some("wide") => 1152.0,
        Some("full") => f32::INFINITY,
        _ => theme::layout::CHAT_MAX_W,
    }
}

/// The card's row type: 13px medium.
const ROW: TextStyle = TextStyle::new(13.0, theme::MEDIUM).lh(19.5);

fn cwd(s: &App) -> Option<String> {
    worktree_here(s).or_else(|| composer::ctx(s).project.map(|p| data::s(p, "workspaceRoot").to_string())).filter(|p| !p.is_empty())
}

/// BranchToolbar's `workspacePath`: none while a new worktree is still to be made.
fn workspace_path(s: &App) -> Option<String> {
    if draft_run(s).is_some_and(|r| r.worktree) {
        return None;
    }
    cwd(s)
}

/// The worktree the thread runs in, or the existing one the draft will launch in.
fn worktree_here(s: &App) -> Option<String> {
    let c = composer::ctx(s);
    match c.thread {
        Some(t) => Some(data::s(t, "worktreePath").to_string()).filter(|p| !p.is_empty()),
        None => draft_worktree(s).map(|(path, _)| path),
    }
}

/// The existing worktree the open draft launches in (a "New thread on {branch}" or "Previous
/// worktree" pick), as `(path, branch)`.
pub fn draft_worktree(s: &App) -> Option<(String, Option<String>)> {
    let c = composer::ctx(s);
    if c.thread.is_some() {
        return None;
    }
    let project = data::s(c.project?, "id");
    let (id, strategy) = s.ui.chat.home.workspace.as_ref()?;
    let branch = strategy["branch"].as_str().filter(|b| !b.is_empty()).map(str::to_string);
    (id == project && strategy["type"] == "existing_worktree").then(|| (data::s(strategy, "worktreePath").to_string(), branch))
}

/// `resolvePreviousWorktreeSeed`: the project's most recently updated live thread in a worktree
/// other than the current one, as `(branch, path)`.
fn previous_worktree<'a>(threads: impl IntoIterator<Item = &'a Value>, project: &str, current: Option<&str>) -> Option<(Option<String>, String)> {
    let mut latest: Option<(i64, &Value)> = None;
    for t in threads {
        let path = data::s(t, "worktreePath");
        if data::s(t, "projectId") != project || path.is_empty() || Some(path) == current || !t["archivedAt"].is_null() {
            continue;
        }
        let Some(at) = data::ts(t, "updatedAt") else { continue };
        if latest.is_none_or(|(best, _)| at > best) {
            latest = Some((at, t));
        }
    }
    latest.map(|(_, t)| (Some(data::s(t, "branch").to_string()).filter(|b| !b.is_empty()), data::s(t, "worktreePath").to_string()))
}

/// The draft's "Previous worktree" choice; none on a thread, whose run context is fixed.
fn previous_seed(s: &App) -> Option<(Option<String>, String)> {
    let c = composer::ctx(s);
    if c.thread.is_some() {
        return None;
    }
    let project = data::s(c.project?, "id");
    let current = draft_worktree(s).map(|(p, _)| p);
    previous_worktree(s.state().threads.iter(), project, current.as_deref())
}

/// `usePreviousWorktree`: the draft launches in that worktree, on its branch.
fn use_previous_worktree(s: &mut App) -> bool {
    let (Some((branch, path)), Some(project)) = (previous_seed(s), composer::ctx(s).project.map(|p| data::s(p, "id").to_string())) else { return false };
    let mut strategy = json!({"type": "existing_worktree", "worktreePath": path});
    if let Some(branch) = branch {
        strategy["branch"] = json!(branch);
    }
    s.ui.chat.home.workspace = Some((project, strategy));
    let run = draft_run_mut(s);
    run.worktree = false;
    run.base = None;
    s.ui.chat.details.menu = None;
    true
}

/// The checkout's `{local, remote}` status (`subscribeVcsStatus`); `local` is null until it loads.
fn vcs<'a>(s: &'a App) -> Option<&'a Value> {
    s.state().vcs.get(&cwd(s)?).filter(|v| v["local"].is_object())
}

/// Subscribes to the framed checkout's status, as the card's `useVcsStatus` does.
pub fn watch(s: &mut App) {
    // The project checkout too (BranchToolbar's `projectStatusQuery`): its root tells a ref's own
    // worktree from another one while the thread works in a worktree.
    let project = composer::ctx(s).project.map(|p| data::s(p, "workspaceRoot").to_string()).filter(|p| !p.is_empty());
    let cwd = cwd(s);
    if let Some(net) = s.src.net() {
        for path in cwd.iter().chain(project.iter()) {
            net.watch_vcs(path);
        }
    }
    scripts::load_file(s, false);
    servers::watch(s);
    automations::watch(s);
}

/// A card row: icon, label, then trailing content. `split` leaves room for a 32-point chevron.
fn action(t: &T, key: &str, glyph: El, label: &str, color: Color) -> El {
    row()
        .key(key.to_string())
        .h(32.0)
        .pl(11.0)
        .pr(10.0)
        .gap(10.0)
        .center_y()
        .radius(theme::layout::CONTROL_RADIUS)
        .hover(t.c.accent)
        .cursor(Cursor::Pointer)
        .label(label.to_string())
        .child(glyph)
        .child(text(label.to_string()).font(ROW).color(color).truncate().grow(1.0).shrink().min_w(0.0))
}

fn split(t: &T, key: &str, main: El, label: &str, menu: Menu) -> El {
    split_with(t, key, main, label, menu, true)
}

/// [`split`] whose chooser is disabled unless `enabled`.
fn split_with(t: &T, key: &str, main: El, label: &str, menu: Menu, enabled: bool) -> El {
    let chooser = el().key(key.to_string()).size(32.0, 32.0).center().corners([0.0, 8.0, 8.0, 0.0]).label(label.to_string()).help(label.to_string());
    let chooser = if enabled {
        chooser.hover(t.c.accent).cursor(Cursor::Pointer).on_press(move |s: &mut App| toggle_menu(s, menu))
    } else {
        chooser.opacity(0.64)
    };
    row()
        .w_full()
        .gap(1.0)
        .child(main.w(229.0).corners([8.0, 0.0, 0.0, 8.0]))
        .child(chooser.child(icon(&icons::CHEVRON_DOWN, 16.0).color(t.c.muted_foreground)))
}

/// A card menu row: icon, label, and the reason it is disabled as its tooltip.
fn menu_row(t: &T, glyph: Option<El>, label: &str, on_press: Option<impl Fn(&mut App) + 'static>, help: Option<String>) -> El {
    let mut item = row().w_full().h(32.0).px(8.0).gap(8.0).center_y().radius(radius::MD).label(label.to_string());
    if let Some(glyph) = glyph {
        item = item.child(glyph);
    }
    item = item.child(text(label.to_string()).font(theme::TEXT_SM).color(t.c.foreground).truncate().grow(1.0).shrink().min_w(0.0));
    if let Some(help) = help {
        item = item.help(help);
    }
    match on_press {
        Some(f) => item.hover(t.c.accent).cursor(Cursor::Pointer).on_press(f),
        None => item.opacity(0.5),
    }
}

fn toggle_menu(s: &mut App, menu: Menu) {
    let ui = &mut s.ui.chat.details;
    ui.strip = None;
    ui.menu = if ui.menu == Some(menu) { None } else { Some(menu) };
    if ui.menu == Some(Menu::Git) {
        git::refresh(s);
        return;
    }
    let open = ui.menu;
    // The branch picker opens on fresh local status, as web's `handleOpenChange` asks.
    if open == Some(Menu::Branch)
        && let (Some(cwd), Some(net)) = (cwd(s), s.src.net())
    {
        net.request("vcs.refreshStatus", json!({"cwd": cwd, "localOnly": true}));
    }
    if open == Some(Menu::Scripts) {
        scripts::load_file(s, true);
        return;
    }
    if open == Some(Menu::Branch) {
        let ui = &mut s.ui.chat.details;
        ui.branch_query = TextEdit::new("");
        ui.branch_query.focused = true;
        ui.branch_active = 0;
        ui.refs = None;
        ui.refs_query.clear();
        ui.refs_next = None;
        ui.refs_total = 0;
        ui.last_scroll.set(None);
        ui.near_end.set(false);
        list_refs(s, false);
    }
}

const REF_LIMIT: u64 = 100;

/// `usePaginatedBranches`: the server searches refs by the sanitized query, 100 at a time; a
/// next page continues from the last one's cursor.
fn list_refs(s: &mut App, next: bool) {
    let Some(cwd) = cwd(s) else { return };
    let query: String = sanitize_ref_name(s.ui.chat.details.branch_query.text.trim()).chars().take(256).collect();
    let mut params = json!({"cwd": cwd, "limit": REF_LIMIT});
    if !query.is_empty() {
        params["query"] = json!(query);
    }
    if next {
        let Some(cursor) = s.ui.chat.details.refs_next else { return };
        params["cursor"] = json!(cursor);
    }
    let Some(net) = s.src.net() else { return };
    let id = net.request("vcs.listRefs", params);
    s.ui.chat.details.listing = Some((id, query, next));
}

/// `1234567` as `1,234,567`.
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Scrolling down to within 96 points of the list's end asks for the next page.
pub(crate) fn next_page_after_scroll(previous: Option<f32>, offset: f32, max: f32) -> bool {
    previous.is_some_and(|p| offset > p) && max - offset <= 96.0
}

/// Folds a `vcs.listRefs` page into the picker: a first page replaces the list, a next one
/// appends the refs it doesn't already have.
fn take_refs(ui: &mut Ui, query: String, next: bool, page: &Value) {
    let refs = page["refs"].as_array().into_iter().flatten().map(Ref::from);
    match ui.refs.as_mut().filter(|_| next) {
        Some(list) => {
            for r in refs {
                if !list.iter().any(|l| l.name == r.name) {
                    list.push(r);
                }
            }
        }
        None => ui.refs = Some(refs.collect()),
    }
    ui.refs_next = page["nextCursor"].as_u64();
    let total = page["totalCount"].as_u64().unwrap_or(0);
    ui.refs_total = if next { ui.refs_total.max(total) } else { total };
    ui.refs_query = query;
}

/// A branch picker row: a ref, or creating one named by the search.
#[derive(Clone, Debug, PartialEq)]
enum Item {
    /// "Checkout pull request" for a query that names one (`parsePullRequestReference`).
    CheckoutPr(String),
    Ref(usize),
    Create(String),
}

/// The branch picker's rows for `query` (`shouldIncludeBranchPickerItem`): refs containing the
/// query or its sanitized form, then "Create new ref" when `can_create` and no ref has that name.
fn branch_items(refs: &[Ref], query: &str, can_create: bool) -> Vec<Item> {
    let query = query.trim();
    let needle = query.to_lowercase();
    let sanitized = sanitize_ref_name(&needle);
    let mut items: Vec<Item> = refs
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            let name = r.name.to_lowercase();
            needle.is_empty() || name.contains(&needle) || (!sanitized.is_empty() && sanitized != needle && name.contains(&sanitized))
        })
        .map(|(i, _)| Item::Ref(i))
        .collect();
    let name = sanitize_ref_name(query);
    if can_create && !name.is_empty() && !refs.iter().any(|r| r.name == name) {
        items.push(Item::Create(name));
    }
    items
}

/// `sanitizeNewRefName`: whitespace runs become `-`.
fn sanitize_ref_name(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join("-")
}

/// `deriveLocalBranchNameFromRemoteRef`: `origin/feature/x` checks out as `feature/x`.
fn local_name(remote: &str) -> String {
    match remote.find('/') {
        Some(i) if i > 0 && i + 1 < remote.len() => remote[i + 1..].to_string(),
        _ => remote.to_string(),
    }
}

fn draft_default(s: &App) -> DraftRun {
    let worktree = crate::settings::server_settings(s)["defaultThreadEnvMode"] == "worktree";
    DraftRun { worktree, from_origin: starts_from_origin(s, worktree), ..DraftRun::default() }
}

/// `resolveNewDraftStartFromOrigin`: a new worktree starts from the remote base when the
/// project's "New worktrees start from remote" setting is on.
fn starts_from_origin(s: &App, worktree: bool) -> bool {
    let project = composer::ctx(s).project.map(|p| data::s(p, "id").to_string()).unwrap_or_default();
    worktree && crate::settings::project_value(s, &project, "newWorktreesStartFromOrigin") == true
}

/// Where the open draft runs; a thread's run context is fixed.
fn draft_run(s: &App) -> Option<DraftRun> {
    if composer::ctx(s).thread.is_some() {
        return None;
    }
    let key = composer::draft_key(s, s.frame_route());
    Some(s.ui.chat.details.drafts.get(&key).cloned().unwrap_or_else(|| draft_default(s)))
}

/// A prepared pull request checkout makes the draft run in it, not in a new worktree.
pub(crate) fn reset_draft_run(s: &mut App) {
    let key = composer::draft_key(s, s.frame_route());
    s.ui.chat.details.drafts.remove(&key);
}

fn draft_run_mut(s: &mut App) -> &mut DraftRun {
    let fallback = draft_default(s);
    let key = composer::draft_key(s, s.frame_route());
    s.ui.chat.details.drafts.entry(key).or_insert(fallback)
}

/// A new-thread draft's effective branch, as its branch toolbar resolves it: a picked existing
/// worktree's branch, a new worktree's base, else the project checkout's current branch.
pub(crate) fn draft_branch(s: &App, key: &str) -> Option<String> {
    let project = key.strip_prefix("home:")?;
    let preset = s.ui.chat.home.workspace.as_ref().filter(|(p, w)| p == project && w["type"] == "existing_worktree");
    if let Some((_, w)) = preset {
        return w["branch"].as_str().filter(|b| !b.is_empty()).map(str::to_owned);
    }
    let root = s.state().projects.iter().find(|p| data::s(p, "id") == project).map(|p| data::s(p, "workspaceRoot"))?;
    let current = s.state().vcs.get(root).and_then(|v| v["local"]["refName"].as_str()).filter(|b| !b.is_empty()).map(str::to_owned);
    match s.ui.chat.details.drafts.get(key).filter(|r| r.worktree) {
        Some(run) => run.base.clone().or(current),
        None => current,
    }
}

/// The branch picker chooses the new worktree's base instead of switching the checkout.
fn selecting_base(s: &App) -> bool {
    draft_run(s).is_some_and(|r| r.worktree)
}

/// The base a draft's new worktree starts from: its pick, else the checkout's branch.
fn worktree_base(s: &App, run: &DraftRun) -> Option<String> {
    run.base.clone().or_else(|| vcs(s).and_then(|v| v["local"]["refName"].as_str()).map(str::to_string))
}

/// Takes the sending draft's run context: a new worktree (`prepareWorktree`) as the launch's
/// `workspaceStrategy`, or `None` to run in the checkout.
pub fn take_draft_workspace(s: &mut App) -> Option<Value> {
    let run = draft_run(s)?;
    let key = composer::draft_key(s, s.frame_route());
    s.ui.chat.details.drafts.remove(&key);
    if !run.worktree {
        return None;
    }
    let mut strategy = json!({"type": "worktree", "baseRef": worktree_base(s, &run)?});
    if run.from_origin {
        strategy["startFromOrigin"] = json!(true);
    }
    Some(strategy)
}

/// The base each picked model's worktree starts from, and whether from origin; none outside a
/// Git checkout (`multipleModelSendBlock`'s branch).
pub fn draft_base(s: &App) -> Option<(String, bool)> {
    let run = draft_run(s)?;
    if !vcs(s).is_some_and(|v| v["local"]["isRepo"] == true) {
        return None;
    }
    Some((worktree_base(s, &run)?, run.from_origin))
}

fn set_env_mode(s: &mut App, worktree: bool) {
    s.ui.chat.details.menu = None;
    // A new worktree replaces the existing one the draft was in; the checkout keeps it.
    if worktree && draft_worktree(s).is_some() {
        s.ui.chat.home.workspace = None;
    }
    let from_origin = starts_from_origin(s, worktree);
    let run = draft_run_mut(s);
    if run.worktree != worktree {
        run.worktree = worktree;
        run.base = None;
        run.from_origin = from_origin;
    }
}

/// The draft's project and one copy per other environment in its repository group; empty when
/// it lives on one environment only.
fn hosts(s: &App) -> Vec<String> {
    let Some(project) = composer::ctx(s).project.map(|p| data::s(p, "id").to_string()) else { return Vec::new() };
    let (groups, by_project) = crate::shell::sidebar::folders::group_of(s);
    let Some(&g) = by_project.get(&project) else { return Vec::new() };
    let rows = &s.state().projects;
    let env_of = |id: &str| rows.iter().find(|p| data::s(p, "id") == id).filter(|p| p["archivedAt"].is_null()).map(|p| crate::env::of(s, p));
    let mut seen = vec![env_of(&project).unwrap_or_default()];
    let mut hosts = vec![project.clone()];
    for member in &groups[g].members {
        if let Some(env) = env_of(member).filter(|e| !seen.contains(e)) {
            seen.push(env);
            hosts.push(member.clone());
        }
    }
    if hosts.len() > 1 { hosts } else { Vec::new() }
}

/// Moves the new-thread draft (prompt and run context) to `project`'s copy on another
/// environment, as RunContextSelector's "Run on" does.
fn move_draft(s: &mut App, project: &str) {
    s.ui.chat.details.menu = None;
    let (from, to) = (composer::draft_key(s, s.frame_route()), composer::home_key(project));
    if from == to {
        return;
    }
    if let Some(d) = s.ui.chat.composer.drafts.remove(&from) {
        s.ui.chat.composer.drafts.insert(to.clone(), d);
    }
    if let Some(run) = s.ui.chat.details.drafts.remove(&from) {
        s.ui.chat.details.drafts.insert(to, run);
    }
    s.navigate(Route::Home { project_id: Some(project.to_string()) });
}

/// `composer.cycleHost` (ChatView): the draft moves to the next machine holding its project, in
/// a fixed machine order, as picking that machine under "Run on" does.
fn cycle_host(s: &mut App) -> bool {
    let Some(current) = composer::ctx(s).project.map(|p| data::s(p, "id").to_string()).filter(|_| draft_run(s).is_some()) else { return false };
    let rows = &s.state().projects;
    let mut hosts: Vec<(String, String)> =
        hosts(s).into_iter().filter_map(|id| rows.iter().find(|p| data::s(p, "id") == id).map(|p| (crate::env::of(s, p), id))).collect();
    hosts.sort();
    let Some(i) = hosts.iter().position(|(_, id)| *id == current) else { return false };
    let next = hosts[(i + 1) % hosts.len()].1.clone();
    run_on::keep_here(s);
    move_draft(s, &next);
    true
}

fn pick_item(s: &mut App, item: Item) {
    s.ui.chat.details.menu = None;
    match item {
        Item::CheckoutPr(reference) => return pr::open_checkout(s, Some(reference)),
        Item::Create(name) => create_ref(s, &name),
        Item::Ref(i) => {
            if let Some(r) = s.ui.chat.details.refs.as_ref().and_then(|v| v.get(i)).cloned() {
                select_ref(s, &r);
            }
        }
    }
    // onComposerFocusRequest: the caret goes back to the prompt after a pick.
    let route = s.frame_route().clone();
    composer::focus(s, &route);
}

/// `selectBranch`: a worktree base for a worktree draft; a ref checked out in a worktree moves
/// the thread there; anything else is switched to in the thread's checkout.
/// `canUpdateThreadBranch`: a started thread's branch is orchestration metadata.
fn can_update_branch(s: &App) -> bool {
    composer::ctx(s).thread.is_none() || grants(s, "orchestration:operate")
}

/// `canChangeThreadBranch`: switching or creating a ref also writes source control.
fn can_change_branch(s: &App) -> bool {
    grants(s, "source-control:write") && can_update_branch(s)
}

/// `BranchPickerRefItem`'s `disabled`: moving to a ref's existing worktree needs no source control.
fn ref_disabled(s: &App, r: &Ref) -> bool {
    !can_update_branch(s) || (!grants(s, "source-control:write") && !selecting_base(s) && r.worktree.is_none())
}

/// `resolveBranchSelectionTarget`: a ref's worktree is the project's own checkout when it is the
/// checkout root, or, until that root loads, when it holds the project.
fn own_checkout(worktree: &str, project_cwd: &str, root: Option<&str>) -> bool {
    let trim = |p: &str| p.trim_end_matches('/').to_string();
    let worktree = trim(worktree);
    match root {
        Some(root) => worktree == trim(root),
        None => {
            let project = trim(project_cwd);
            worktree == project || worktree.starts_with(&format!("{project}/"))
        }
    }
}

/// `runBranchAction`'s guard: changing the branch writes source control, and a thread's metadata.
fn branch_denied(s: &mut App) -> bool {
    let denied = !can_change_branch(s);
    if denied {
        palette::toast_error_brief(s, "Action unavailable", Some("This connection cannot change the thread's branch."));
    }
    denied
}

fn select_ref(s: &mut App, r: &Ref) {
    if ref_disabled(s, r) || switching(s) {
        return;
    }
    if selecting_base(s) {
        draft_run_mut(s).base = Some(r.name.clone());
        return;
    }
    let c = composer::ctx(s);
    let project_cwd = c.project.map(|p| data::s(p, "workspaceRoot").to_string()).unwrap_or_default();
    let active = worktree_here(s);
    if let Some(path) = &r.worktree {
        let root = s.state().vcs.get(&project_cwd).and_then(|v| v["local"]["workingTree"]["rootPath"].as_str());
        let own = own_checkout(path, &project_cwd, root);
        set_thread_branch(s, &r.name, (!own).then(|| path.clone()));
        return;
    }
    if branch_denied(s) {
        return;
    }
    let next = if r.default { None } else { active };
    let checkout = next.clone().unwrap_or(project_cwd);
    let name = if r.remote { local_name(&r.name) } else { r.name.clone() };
    if let Some(net) = s.src.net() {
        let id = net.request("vcs.switchRef", json!({"cwd": checkout, "refName": r.name}));
        s.ui.chat.details.pending = Some((id, Pending::Branch(name, next, "Failed to switch ref.", BranchTarget::current(s))));
    }
}

/// `isBranchActionPending`: a pick waits for the switch already under way.
fn switching(s: &App) -> bool {
    matches!(s.ui.chat.details.pending, Some((_, Pending::Branch(..))))
}

fn create_ref(s: &mut App, name: &str) {
    if switching(s) || branch_denied(s) {
        return;
    }
    let Some(cwd) = cwd(s) else { return };
    let worktree = worktree_here(s);
    if let Some(net) = s.src.net() {
        let id = net.request("vcs.createRef", json!({"cwd": cwd, "refName": name, "switchRef": true}));
        s.ui.chat.details.pending = Some((id, Pending::Branch(name.to_string(), worktree, "Failed to create and switch ref.", BranchTarget::current(s))));
    }
}

fn set_thread_branch(s: &mut App, branch: &str, worktree: Option<String>) {
    if let Some(target) = BranchTarget::current(s) {
        target.set(s, branch, worktree);
    }
}

/// `resolveLocalCheckoutBranchMismatch`: a local-checkout thread whose branch is no longer the
/// one checked out, as `(thread branch, current branch)`.
pub(crate) fn branch_mismatch(thread_branch: &str, worktree: &str, current: &str) -> Option<(String, String)> {
    (worktree.is_empty() && !thread_branch.is_empty() && !current.is_empty() && thread_branch != current).then(|| (thread_branch.to_string(), current.to_string()))
}

/// The open thread's mismatch and its key (`threadId:threadBranch:currentBranch`).
fn mismatch(s: &App) -> Option<(String, String, String)> {
    let thread = composer::ctx(s).thread?;
    let current = vcs(s).map(|v| data::s(&v["local"], "refName")).unwrap_or_default();
    let (from, to) = branch_mismatch(data::s(thread, "branch"), data::s(thread, "worktreePath"), current)?;
    Some((format!("{}:{from}:{to}", data::s(thread, "id")), from, to))
}

fn composer_has_content(s: &App) -> bool {
    s.ui.chat.composer.drafts.get(&composer::draft_key(s, &composer::route(s))).is_some_and(|d| !d.edit.text.trim().is_empty() || !d.attachments.is_empty())
}

/// The mismatch notice appears once the composer has content and stays for that mismatch; the
/// open side chat's composer notes its own.
fn note_mismatch(s: &mut App) -> bool {
    let app: &App = s;
    let side = super::side::open_beside(app).map(str::to_owned);
    let noted: Vec<String> = [None, side]
        .into_iter()
        .filter_map(|target| composer::within(app, target, || mismatch(app).filter(|_| composer_has_content(app)).map(|(key, ..)| key)))
        .collect();
    noted.into_iter().fold(false, |changed, key| s.ui.chat.details.mismatch_shown.insert(key) | changed)
}

fn restore_branch(s: &mut App, confirmed: bool) {
    if !grants(s, "source-control:write") {
        return;
    }
    let Some((_, branch, _)) = mismatch(s) else { return };
    if switching(s) {
        return;
    }
    if !confirmed && vcs(s).is_some_and(|v| v["local"]["hasWorkingTreeChanges"] == true) {
        s.ui.chat.details.restore_confirm = Some((branch, composer::drawing(s)));
        return;
    }
    s.ui.chat.details.restore_confirm = None;
    if let (Some(cwd), Some(net)) = (cwd(s), s.src.net()) {
        let id = net.request("vcs.switchRef", json!({"cwd": cwd, "refName": branch}));
        s.ui.chat.details.pending = Some((id, Pending::Branch(branch, None, "Failed to switch checkout", BranchTarget::current(s))));
    }
}

/// "Branch changed from X", docked on the composer (branchMismatchBannerItem).
pub fn mismatch_banner(s: &App, t: &T) -> Option<El> {
    let (key, from, to) = mismatch(s)?;
    let ui = &s.ui.chat.details;
    if ui.mismatch_dismissed.contains(&key) || !(ui.mismatch_shown.contains(&key) || composer_has_content(s)) {
        return None;
    }
    let muted = t.c.muted_foreground;
    let restoring = switching(s);
    let dismiss = key.clone();
    Some(
        row()
            .key("branch-mismatch")
            .w_full()
            .corners([radius::XL2, radius::XL2, 0.0, 0.0])
            .fill(if t.dark { t.c.surface_raised } else { t.c.card })
            .border(1.0, if t.dark { Color::WHITE.alpha(0.05) } else { Color::BLACK.alpha(0.08) })
            .pl(16.0)
            .pr(8.0)
            .pt(4.0)
            .pb(17.0 + 4.0)
            .gap(8.0)
            .center_y()
            .child(icon(&icons::GIT_BRANCH, 14.0).color(t.c.info_foreground))
            .child(text("Branch changed from").font(theme::TEXT_XS).color(muted))
            .child(
                text(from.clone())
                    .font(theme::mono(theme::XS_MEDIUM))
                    .color(t.c.foreground)
                    .single_line()
                    .truncate()
                    .shrink()
                    .min_w(0.0)
                    .help(format!("This thread last ran on {from}. Sending will continue on {to}.")),
            )
            .child(el().grow(1.0))
            .child(
                crate::kit::Button::new(if restoring { "Restoring..." } else { "Restore branch" })
                    .variant("ghost")
                    .size("xs")
                    .disabled(restoring)
                    .on_press(composer::act(s, |s: &mut App| restore_branch(s, false)))
                    .build(t),
            )
            .child(
                crate::kit::icon_button(&icons::X, "Dismiss branch change notice")
                    .size("icon-xs")
                    .on_press(move |s: &mut App| {
                        s.ui.chat.details.mismatch_dismissed.insert(dismiss.clone());
                    })
                    .build(t),
            ),
    )
}

fn restore_dialog(s: &App, t: &T, out: &mut Vec<El>) {
    let Some((branch, side)) = s.ui.chat.details.restore_confirm.clone() else { return };
    let close = |s: &mut App| s.ui.chat.details.restore_confirm = None;
    let footer = row()
        .w_full()
        .gap(8.0)
        .child(el().grow(1.0))
        .child(crate::kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
        .child(crate::kit::Button::new("Switch branch").disabled(!grants(s, "source-control:write")).on_press(move |s: &mut App| composer::acting(s, side.clone(), |s| restore_branch(s, true))).build(t));
    // ChatOverlayHosts: `Switch to <code className="font-medium">{branch}</code>?`.
    let code = rune::Span::new("Switch to ".len(), "Switch to ".len() + branch.len()).mono().weight(500.0);
    crate::kit::Dialog::new("branch-restore", format!("Switch to {branch}?"))
        .title_spans(vec![code])
        .open(true)
        .alert()
        .description("You have uncommitted changes. They'll carry over to the other branch, or block the switch if they conflict.")
        .child(footer)
        .on_close(close)
        .build(t, out);
}

fn visible_items(s: &App) -> Vec<Item> {
    let ui = &s.ui.chat.details;
    let mut items = branch_items(ui.refs.as_deref().unwrap_or(&[]), &ui.branch_query.text, !selecting_base(s) && can_change_branch(s));
    if let Some(reference) = pr::parse_reference(&ui.branch_query.text).filter(|_| can_change_branch(s)) {
        items.insert(0, Item::CheckoutPr(reference));
    }
    items
}

fn on_branch_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    let items = visible_items(s);
    let ui = &mut s.ui.chat.details;
    match &e.key {
        Key::Escape => ui.menu = None,
        Key::Enter => {
            if let Some(item) = items.get(ui.branch_active.min(items.len().saturating_sub(1))).cloned() {
                pick_item(s, item);
            }
        }
        Key::Down | Key::Up if !items.is_empty() => {
            let n = items.len() as isize;
            let delta = if e.key == Key::Down { 1 } else { -1 };
            ui.branch_active = (ui.branch_active.min(items.len() - 1) as isize + delta).rem_euclid(n) as usize;
            tx.scroll_into_view(format!("details-branch-item-{}", ui.branch_active));
        }
        _ => return false,
    }
    true
}

pub fn poll(s: &mut App) -> bool {
    let changed = git::poll(s) | scripts::poll(s) | pr::poll(s) | poll_refs(s) | note_mismatch(s) | poll_session(s) | super::topbar::poll(s) | run_on::poll(s) | cleanup::poll(s) | servers::poll(s) | automations::poll(s) | child_work::poll(s) | html_render::poll(s) | lineage::poll(s);
    poll_branch(s) || changed
}

/// The open picker's ref listing: a new search when the query changes, the next page when the
/// list scrolls near its end, and the pages coming back.
fn poll_refs(s: &mut App) -> bool {
    let mut changed = false;
    if let Some((id, query, next)) = s.ui.chat.details.listing.clone()
        && let Some(reply) = s.src.net().and_then(|net| net.take_reply(id))
    {
        let ui = &mut s.ui.chat.details;
        ui.listing = None;
        changed = true;
        match reply {
            Ok(page) => take_refs(ui, query, next, &page),
            Err(e) => {
                ui.refs.get_or_insert_with(Vec::new);
                palette::toast_error_brief(s, "Failed to load refs.", Some(&palette::error_message(&e, "An error occurred.")));
            }
        }
    }
    let ui = &s.ui.chat.details;
    if ui.menu != Some(Menu::Branch) {
        return changed;
    }
    let want: String = sanitize_ref_name(ui.branch_query.text.trim()).chars().take(256).collect();
    let asking = ui.listing.as_ref().filter(|l| !l.2).map(|l| l.1.clone());
    if asking.as_ref().map_or(want != ui.refs_query && ui.refs.is_some(), |q| *q != want) {
        list_refs(s, false);
    } else if ui.listing.is_none() && ui.refs_next.is_some() && ui.near_end.replace(false) {
        list_refs(s, true);
        changed = true;
    }
    changed
}

fn poll_branch(s: &mut App) -> bool {
    let Some(id) = s.ui.chat.details.pending.as_ref().map(|p| p.0) else { return false };
    let Some(reply) = s.src.net().and_then(|net| net.take_reply(id)) else { return false };
    let Some((_, pending)) = s.ui.chat.details.pending.take() else { return false };
    match (pending, reply) {
        (Pending::Branch(name, worktree, _, target), Ok(v)) => {
            if let Some(target) = target {
                target.set(s, v["refName"].as_str().unwrap_or(&name), worktree);
                if target.is_current(s) {
                    git::refresh(s);
                    let route = s.frame_route().clone();
                    composer::focus(s, &route);
                }
            }
        }
        (Pending::Branch(_, _, title, _), Err(e)) => palette::toast_error_brief(s, title, Some(&palette::error_message(&e, "An error occurred."))),
    }
    true
}

fn open_in_editor(s: &mut App, editor: Option<String>) {
    if !can_open_editor(s) {
        return;
    }
    let (Some(cwd), editor) = (cwd(s), editor.or_else(|| preferred_editor(s).map(|e| e.0))) else { return };
    let Some(editor) = editor else { return };
    if crate::env::ssh::open_in_editor(s, &editor, &cwd, crate::env::ssh::toast_open_error) {
        return;
    }
    if let Some(net) = s.src.net() {
        net.request("shell.openInEditor", json!({"cwd": cwd, "editor": editor}));
    }
}

fn editor_mode(s: &App) -> crate::env::ssh::OpenMode {
    crate::env::ssh::open_mode(s, &s.ui.env.active)
}

/// OpenInPicker's `isHostEditorDenied`: the server's own editors need orchestration access.
fn editor_denied(s: &App) -> bool {
    editor_mode(s) == crate::env::ssh::OpenMode::Local && !grants(s, "orchestration:operate")
}

fn can_open_editor(s: &App) -> bool {
    editor_mode(s) != crate::env::ssh::OpenMode::Unavailable && !editor_denied(s)
}

/// OpenInPicker's option icon: the editor's brand mark; the file manager is Finder, File
/// Explorer, or a muted folder elsewhere. `None` for an editor web draws no icon for.
fn editor_mark(t: &T, id: &str, size: f32) -> Option<El> {
    let id = match id {
        "file-manager" if cfg!(target_os = "macos") => "finder",
        "file-manager" if cfg!(target_os = "windows") => "file-explorer",
        "file-manager" => return Some(icon(&icons::FOLDER_CLOSED, size).color(t.c.muted_foreground)),
        "antigravity" => return composer::provider_mark(t, "antigravity", size),
        id => id,
    };
    let layers = editor_icons::layers(id)?;
    Some(layers.iter().fold(el().size(size, size), |mark, &(glyph, light, dark)| {
        let hex = if t.dark { dark } else { light };
        let color = if hex == "currentColor" { t.c.foreground } else { rune::color::hex(hex).unwrap_or(t.c.foreground) };
        mark.child(icon(glyph, size).color(color).absolute(0.0, 0.0))
    }))
}

/// `editorLabelForPlatform`: the file manager takes this machine's name for it.
fn editor_label(id: &str) -> String {
    match id {
        "file-manager" if cfg!(target_os = "macos") => "Finder".into(),
        "file-manager" if cfg!(target_os = "windows") => "File Explorer".into(),
        "file-manager" => "Files".into(),
        id => crate::panel::diff::file_menu::label(id).to_string(),
    }
}

/// `(id, label)` for each editor the server found; none without a route to the host.
fn editors(s: &App) -> Vec<(String, String)> {
    if let Some(remote) = crate::env::ssh::editors(s) {
        return remote;
    }
    if editor_mode(s) == crate::env::ssh::OpenMode::Unavailable {
        return Vec::new();
    }
    s.state()
        .config
        .as_ref()
        .and_then(|c| c["availableEditors"].as_array())
        .map(|v| {
            v.iter()
                .map(|e| match e.as_str() {
                    Some(id) => (id.to_string(), editor_label(id)),
                    None => (data::s(e, "id").to_string(), data::s(e, "label").to_string()),
                })
                .collect()
        })
        .unwrap_or_default()
}

const PREFERRED_EDITOR: &str = "preferredEditor";

/// `usePreferredEditor`: the editor last picked, while it is still installed, else the first.
pub(crate) fn preferred_editor(s: &App) -> Option<(String, String)> {
    let stored = s.client_settings.values.get(PREFERRED_EDITOR).and_then(|v| v.as_str());
    pick_editor(editors(s), stored)
}

fn pick_editor(editors: Vec<(String, String)>, stored: Option<&str>) -> Option<(String, String)> {
    let at = editors.iter().position(|e| Some(e.0.as_str()) == stored).unwrap_or(0);
    editors.into_iter().nth(at)
}

fn set_preferred_editor(s: &mut App, editor: &str) {
    s.client_settings.values.insert(PREFERRED_EDITOR.into(), json!(editor));
    s.client_settings.save();
}

fn env_label(s: &App) -> String {
    s.state().config.as_ref().map(|c| data::s(&c["environment"], "label").to_string()).unwrap_or_default()
}

/// The branch selector row: the label middle-truncates, as `MiddleTruncate` does.
fn branch_row(t: &T, label: &str, fg: Color, muted: Color) -> El {
    row()
        .key("details-branch")
        .w_full()
        .h(32.0)
        .pl(11.0)
        .pr(8.0)
        .gap(10.0)
        .center_y()
        .radius(theme::layout::CONTROL_RADIUS)
        .hover(t.c.accent)
        .cursor(Cursor::Pointer)
        .role("combobox")
        .label(label.to_string())
        .child(icon(&icons::GIT_BRANCH, 16.0).color(muted))
        .child(text(label.to_string()).font(ROW).color(fg).single_line().truncate_middle().grow(1.0).shrink().min_w(0.0))
        .child(icon(&icons::CHEVRON_DOWN, 16.0).color(muted))
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &T, shown: Shown, w: f32) -> Option<El> {
    let h = cx.layout_with("chat-body", |r| r[3].round()).unwrap_or(848.0);
    s.ui.chat.details.fits.set(lane(w, h, false).card);
    // ThreadDetailsCard: the card never runs past the chat area; its content scrolls instead.
    let offset = if shown == Shown::Inline { super::timeline::find::offset(s) } else { 0.0 };
    let card = card(s, t, (h - 2.0 * GAP - offset).max(0.0))?;
    match shown {
        Shown::None => None,
        Shown::Inline => Some(card.absolute(w - WIDTH - GAP, GAP + super::timeline::find::offset(s))),
        Shown::Popover => Some(card.anchored("topbar-details", Side::Bottom, Align::End, 4.0).modal(true, |s: &mut App| {
            s.ui.chat.details.popover = None;
            s.ui.chat.details.menu = None;
        })),
    }
}

/// The card's glass frame around its scrolling content, at most `cap` tall.
fn frame(t: &T, body: El, cap: f32) -> El {
    col()
        .key("thread-details")
        .w(WIDTH)
        .radius(radius::XL2)
        .fill(t.c.popover.alpha_mul(0.836))
        .border(1.0, if t.dark { Color::WHITE.alpha(0.1) } else { Color::BLACK.alpha(0.08) })
        .shadow(8.0, 24.0, Color::BLACK.alpha(if t.dark { 0.32 } else { 0.08 }))
        .padding(1.0)
        .child(body.key("thread-details-scroll").w_full().max_h((cap - 2.0).max(0.0)).scroll(rune::ScrollSpec { stick_bottom: false }))
}

/// The workspace label (`resolveEnvModeLabel` and the locked labels), whether the run makes a
/// new worktree, and whether it is in an existing one.
fn workspace_mode(s: &App) -> (&'static str, bool, bool) {
    let new_worktree = composer::fanout::forces_worktree(s) || draft_run(s).is_some_and(|r| r.worktree);
    let worktree = worktree_here(s).is_some();
    let mode = match (new_worktree, worktree, composer::ctx(s).thread.is_some()) {
        (true, ..) => "New worktree",
        (_, true, true) => "Worktree",
        (_, true, false) => "Current worktree",
        (_, false, true) => "Local checkout",
        _ => "Current checkout",
    };
    (mode, new_worktree, worktree)
}

/// resolveBranchTriggerLabel over the checkout's local status `v`: a new worktree names the
/// branch it starts from.
fn branch_label(s: &App, v: &Value) -> String {
    let current = data::s(v, "refName").to_string();
    match draft_run(s).filter(|r| r.worktree) {
        Some(r) => match worktree_base(s, &r) {
            Some(base) if r.from_origin => format!("From {base} (remote)"),
            Some(base) => format!("From {base}"),
            None => "Select ref".into(),
        },
        None if current.is_empty() => "Select ref".into(),
        None => current,
    }
}

fn card(s: &App, t: &T, cap: f32) -> Option<El> {
    let muted = t.c.muted_foreground;
    let fg = t.c.foreground.alpha_mul(0.8);
    let env = run_on::auto_label(s).map_or_else(|| env_label(s), str::to_string);
    let thread = composer::ctx(s).thread;
    let fanout = composer::fanout::forces_worktree(s);
    let (mode, new_worktree, worktree) = workspace_mode(s);
    // A draft can still choose where it runs, so its row is the env mode picker.
    let draft = thread.is_none();
    let mut run_context = row()
        .key("details-env")
        .h(32.0)
        .pl(10.0)
        .pr(if draft { 8.0 } else { 11.0 })
        .gap(9.0)
        .center_y()
        .label(format!("Run context: {env}, {mode}"))
        .child(icon(if run_on::auto_label(s).is_some() { &icons::SCALE } else { &icons::SERVER }, 18.0).color(muted))
        .child(text(env).font(ROW).color(if draft { fg } else { muted }).truncate().grow(1.0).shrink().min_w(0.0))
        .child(icon(if new_worktree { &icons::FOLDER_GIT_2 } else if worktree { &icons::GIT_FORK } else { &icons::FOLDER }, 16.0).color(muted).ml(1.0).help(if fanout { "Each model starts in its own worktree.".to_string() } else { workspace_path(s).unwrap_or_else(|| mode.to_string()) }));
    if workspace_path(s).is_some() {
        run_context = run_context.on_secondary_press(|s: &mut App, _: rune::PointerPress, _: &mut Tx| s.ui.chat.details.menu = Some(Menu::Path));
    }
    if draft {
        run_context = run_context
            .radius(theme::layout::CONTROL_RADIUS)
            .hover(t.c.accent)
            .cursor(Cursor::Pointer)
            .role("button")
            .expanded(s.ui.chat.details.menu == Some(Menu::RunContext))
            .child(icon(&icons::CHEVRON_DOWN, 16.0).color(muted).ml(9.0))
            .help(with_shortcut(s, mode, "composer.workspace"))
            .on_press(|s: &mut App| toggle_menu(s, Menu::RunContext));
    }
    let preferred = preferred_editor(s);
    let editor = split_with(
        t,
        "details-editor-menu",
        {
            let label = preferred.as_ref().map_or_else(|| "Open in editor".to_string(), |(_, name)| format!("Open in {name}"));
            let glyph = preferred.as_ref().and_then(|(id, _)| editor_mark(t, id, 16.0)).unwrap_or_else(|| icon(&icons::SQUARE_ARROW_OUT_UP_RIGHT, 16.0).color(muted));
            let open = action(t, "details-editor", glyph, &label, fg);
            // No editor found: the button is disabled (opacity-64), the chooser still opens.
            if preferred.is_none() || !can_open_editor(s) { open.opacity(0.64).cursor(Cursor::Default) } else { open.help(with_shortcut(s, &label, "editor.openFavorite")).on_press(|s: &mut App| open_in_editor(s, None)) }
        },
        "Choose editor",
        Menu::Editor,
        !editor_denied(s),
    );
    let on = |id: &str| items::visible(&s.client_settings.values, id);
    let general: Vec<El> = [(on("environment"), Some(run_context)), (on("openIn"), Some(editor)), (on("scripts"), None)]
        .into_iter()
        .filter(|(shown, _)| *shown)
        .map(|(_, item)| item.unwrap_or_else(|| scripts::view(s, t)))
        .collect();
    let mut divided = !general.is_empty();
    let mut body = col().w_full();
    if divided {
        body = body.child(col().w_full().padding(8.0).pb(10.0).children(general));
    }

    // A failed status stream still shows the section, with Commit disabled and +0 -0.
    let failed = (vcs(s).is_none() && git::status_error(s).is_some()).then(|| json!({"isRepo": true}));
    if let Some(v) = vcs(s).map(|v| &v["local"]).or(failed.as_ref()).filter(|v| v["isRepo"] == true) {
        let mut rows: Vec<El> = vec![];
        if on("branch") {
            let branch = branch_label(s, v);
            rows.push(
                branch_row(t, &branch, fg, muted)
                    .help(with_shortcut(s, &branch, "composer.branch"))
                    .expanded(s.ui.chat.details.menu == Some(Menu::Branch))
                    .on_press(|s: &mut App| toggle_menu(s, Menu::Branch)),
            );
            rows.extend(pr::rows(s, t));
        }
        // The Changes row hides with the quiet git row; on a draft it opens the project's changes.
        if let Some(git) = git::view(s, t, v, draft).filter(|_| on("gitActions") || on("changes")) {
            if on("gitActions") {
                rows.push(git);
            }
            if on("changes") {
                let (add, del) = git::changes_totals(v);
                let stat = theme::mono(theme::XS_MEDIUM);
                let changes = action(t, "details-changes", icon(&icons::FILE_DIFF, 16.0).color(muted), "Changes", fg)
                    .w_full()
                    .child(
                        row()
                            .gap(6.0)
                            .child(text(format!("+{}", thousands(add))).font(stat).color(if add > 0 { t.c.diff_addition_foreground } else { muted }))
                            .child(text(format!("-{}", thousands(del))).font(stat).color(if del > 0 { t.c.diff_deletion_foreground } else { muted })),
                    );
                rows.push(changes.on_press(|s: &mut App| crate::panel::open_changes(s)));
            }
        }
        if !rows.is_empty() {
            let mut section = col().w_full().px(8.0).pt(9.0).pb(10.0);
            if divided {
                section = section.child(el().absolute(0.0, 0.0).w_full().h(1.0).fill(t.c.border.alpha_mul(0.65)));
            }
            body = body.child(section.children(rows));
            divided = true;
        }
    }
    if let Some(id) = thread.map(|row| data::s(row, "id")) {
        if on("servers")
            && let Some(panel) = servers::view(s, t, id, divided)
        {
            body = body.child(panel);
            divided = true;
        }
        if on("automations")
            && let Some(panel) = automations::view(s, t, id, divided)
        {
            body = body.child(panel);
            divided = true;
        }
        if on("lineage")
            && let Some(panel) = lineage::view(s, t, id, divided)
        {
            body = body.child(panel);
            divided = true;
        }
        if on("sideChats")
            && let Some(panel) = super::side::details_list(s, t, id, divided)
        {
            body = body.child(panel);
            divided = true;
        }
    }
    // ThreadDetailsUsage and ThreadDetailsContext, off unless chosen.
    if on("usage")
        && let Some(panel) = usage::limits(s, t, divided)
    {
        body = body.child(panel);
        divided = true;
    }
    if on("context")
        && let Some(panel) = usage::context(s, t, divided)
    {
        body = body.child(panel);
        divided = true;
    }
    // Shown only when no section has content, so an emptied card can be refilled.
    if !divided {
        let choose = action(t, items::CHOOSE_KEY, icon(&icons::SLIDERS_HORIZONTAL, 16.0).color(muted), "Choose what to show", fg)
            .h(24.0)
            .w_full()
            .on_press(|s: &mut App| items::open(s, items::At::Choose));
        body = body.child(col().w_full().padding(8.0).child(choose));
    }
    let card = frame(t, body, cap).on_secondary_press(|s: &mut App, at: rune::PointerPress, _: &mut Tx| items::open(s, items::At::Point(at.x, at.y)));
    Some(card)
}

/// The open card menu, anchored under its trigger. Attach after the card so it draws above.
pub fn menu(s: &App, t: &T) -> Option<El> {
    let which = s.ui.chat.details.menu?;
    let close = |s: &mut App| s.ui.chat.details.menu = None;
    let surface = || col().key("details-menu").w(WIDTH - 18.0).padding(4.0).radius(radius::LG).fill(t.c.popover).border(1.0, t.c.border).shadow(8.0, 24.0, Color::BLACK.alpha(0.3));
    let (anchor, list) = match which {
        Menu::Git => ("details-git-menu", surface().children(git::menu_rows(s, t))),
        Menu::Scripts => ("details-scripts-menu", surface().children(scripts::menu_rows(s, t))),
        Menu::Lineage => ("details-lineage-more", surface().children(lineage::menu_rows(s, t))),
        Menu::Branch => ("details-branch", branch_picker(s, t, surface())),
        Menu::RunContext => ("details-env", surface().children(run_context_rows(s, t))),
        Menu::Path => {
            let copy = row()
                .key("details-copy-path")
                .w_full()
                .h(32.0)
                .px(8.0)
                .gap(8.0)
                .center_y()
                .radius(radius::MD)
                .role("menuitem")
                .label("Copy full path")
                .hover(t.c.accent)
                .cursor(Cursor::Pointer)
                .child(icon(&icons::COPY, 16.0).color(t.c.muted_foreground))
                .child(text("Copy full path").font(theme::TEXT_SM).color(t.c.foreground))
                .on_press_tx(|s: &mut App, tx: &mut Tx| {
                    s.ui.chat.details.menu = None;
                    if let Some(path) = workspace_path(s) {
                        tx.copy(path.clone());
                        palette::toast(s, "Path copied", Some(&path));
                    }
                });
            ("details-env", surface().child(copy))
        }
        Menu::Editor => {
            let editors = editors(s);
            let mut list = col().key("details-menu").min_w(200.0).padding(4.0).radius(radius::LG).fill(t.c.popover).border(1.0, t.c.border).shadow(8.0, 24.0, Color::BLACK.alpha(0.3));
            let note = |list: El, line: String| list.child(row().h(32.0).px(8.0).center_y().child(text(line).font(theme::TEXT_SM).color(t.c.muted_foreground)));
            let mode = editor_mode(s);
            if mode == crate::env::ssh::OpenMode::Unavailable {
                list = note(list, format!("No SSH route to {}", crate::env::label(s, &s.ui.env.active)));
            } else if editors.is_empty() {
                list = note(list, "No installed editors found".into());
            }
            let hint = matches!(mode, crate::env::ssh::OpenMode::Links(..)) && s.client_settings.values.get(crate::env::ssh::REMOTE_OPEN_HINT_SEEN) != Some(&json!(true));
            let preferred = preferred_editor(s).map(|e| e.0);
            let shortcut = palette::shortcut_label(s, "editor.openFavorite");
            // OpenInPicker's panel menu picks the button's editor; the button opens it.
            for (id, label) in editors {
                let mark = (preferred.as_deref() == Some(id.as_str()) && !shortcut.is_empty()).then(|| text(shortcut.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground));
                list = list.child(
                    menu_row(t, editor_mark(t, &id, 16.0), &label, Some(move |s: &mut App| {
                        s.ui.chat.details.menu = None;
                        set_preferred_editor(s, &id);
                    }), None)
                    .children(mark),
                );
            }
            if hint {
                list = note(list, format!("Opens over SSH. Needs your key on {}", crate::env::label(s, &s.ui.env.active)));
            }
            ("details-editor-menu", list)
        }
    };
    // From the context strip the run context and branch menus open upward, as BranchToolbar's do.
    let list = match s.ui.chat.details.strip.filter(|_| matches!(which, Menu::Branch | Menu::RunContext)) {
        Some(at) => list.anchored(at, Side::Top, if at == strip::BRANCH { Align::End } else { Align::Start }, 4.0),
        None => list.anchored(anchor, Side::Bottom, Align::End, 4.0),
    };
    Some(list.modal(true, close))
}

fn group_label(t: &T, label: &str) -> El {
    row().px(8.0).py(6.0).child(text(label.to_string()).font(theme::XS_MEDIUM).color(t.c.muted_foreground))
}

fn radio_row(t: &T, glyph: &'static rune::Icon, label: &str, checked: bool, on_press: impl Fn(&mut App) + 'static) -> El {
    let mark = if checked { icon(&icons::CHECK, 16.0).color(t.c.foreground) } else { el().size(16.0, 16.0) };
    menu_row(t, Some(icon(glyph, 16.0).color(t.c.muted_foreground)), label, Some(on_press), None).child(mark)
}

/// RunContextSelector: where the draft runs (this environment) and in which workspace.
fn run_context_rows(s: &App, t: &T) -> Vec<El> {
    let fanout = composer::fanout::forces_worktree(s);
    let worktree = fanout || draft_run(s).is_some_and(|r| r.worktree);
    let checkout = if draft_worktree(s).is_some() { "Current worktree" } else { "Current checkout" };
    let auto = run_on::auto_label(s).is_some();
    let mut rows = vec![group_label(t, "Run on")];
    // One row per environment holding the project's repository; picking one moves the draft.
    let hosts = hosts(s);
    if hosts.is_empty() {
        rows.push(radio_row(t, &icons::SERVER, &env_label(s), !auto, run_on::keep_here));
    }
    let current = composer::ctx(s).project.map(|p| data::s(p, "id").to_string()).unwrap_or_default();
    for project in hosts {
        let label = s.state().projects.iter().find(|p| data::s(p, "id") == project).map(|p| crate::env::location(s, p)).unwrap_or_default();
        let checked = !auto && project == current;
        rows.push(radio_row(t, &icons::SERVER, &label, checked, move |s: &mut App| {
            run_on::keep_here(s);
            move_draft(s, &project);
        }));
    }
    // BranchToolbar's "Add to another machine": clone this project onto a connected environment.
    if let Some(id) = composer::ctx(s).project.map(|p| data::s(p, "id").to_owned())
        && !crate::add_to_machine::targets(s, &id).is_empty()
    {
        rows.push(menu_row(t, Some(icon(&icons::PLUS, 16.0).color(t.c.muted_foreground)), "Add to another machine", Some(move |s: &mut App| {
            s.ui.chat.details.menu = None;
            crate::add_to_machine::open(s, &id, None);
        }), None));
    }
    rows.extend([
        el().h(1.0).mx(8.0).my(4.0).fill(t.c.border),
        group_label(t, "Workspace"),
        if fanout {
            menu_row(t, Some(icon(&icons::FOLDER, 16.0).color(t.c.muted_foreground)), checkout, None::<fn(&mut App)>, None).child(el().size(16.0, 16.0))
        } else {
            radio_row(t, &icons::FOLDER, checkout, !worktree, |s: &mut App| set_env_mode(s, false))
        },
        radio_row(t, &icons::FOLDER_GIT_2, "New worktree", worktree, |s: &mut App| set_env_mode(s, true)),
    ]);
    if let Some(auto) = run_on::auto_row(s, t) {
        rows.insert(1, auto);
    }
    if let Some((branch, _)) = previous_seed(s) {
        let label = branch.map_or_else(|| "Previous worktree".to_string(), |b| format!("Previous worktree ({b})"));
        rows.push(radio_row(t, &icons::HISTORY, &label, false, |s: &mut App| {
            if use_previous_worktree(s) {
                let route = s.frame_route().clone();
                composer::focus(s, &route);
            }
        }));
    }
    rows
}

/// BranchPicker: a ref search, a capped list with badges, and "Create new ref" for a new name.
fn branch_picker(s: &App, t: &T, list: El) -> El {
    let ui = &s.ui.chat.details;
    let muted = t.c.muted_foreground;
    let project_cwd = composer::ctx(s).project.map(|p| data::s(p, "workspaceRoot").to_string()).unwrap_or_default();
    // ComboboxSearchInput: a borderless field over a divider that takes the ring colour on focus.
    let field = text_input(&ui.branch_query, theme::TEXT_SM)
        .key("details-branch-search")
        .w_full()
        .h(26.0)
        .pl(20.0)
        .py(3.0)
        .font(theme::TEXT_SM)
        .color(t.c.contrast_foreground)
        .placeholder("Search refs...", t.c.contrast_placeholder)
        .on_input_key(on_branch_key)
        .on_edit(|s: &mut App| &mut s.ui.chat.details.branch_query);
    let underline = if ui.branch_query.focused { t.c.ring } else { t.c.contrast_border.alpha_mul(0.7) };
    let search = col()
        .w_full()
        .px(12.0)
        .pt(9.0)
        .child(row().w_full().pb(6.0).child(icon(&icons::SEARCH, 16.0).absolute(0.0, 6.0).color(t.c.contrast_muted_foreground.alpha_mul(0.55))).child(field))
        .child(el().w_full().h(1.0).fill(underline));
    let list = list.padding(0.0).child(search);
    // ComboboxEmpty (centred, p-2) and ComboboxStatus (px-3 py-2, medium xs).
    let empty = |label: &str| row().w_full().padding(8.0).justify_center().child(text(label.to_string()).font(theme::TEXT_SM).color(muted));
    let status = |label: &str| row().w_full().px(12.0).py(8.0).child(text(label.to_string()).font(theme::XS_MEDIUM).color(muted));
    let Some(refs) = ui.refs.as_deref() else { return list.child(status("Loading refs...")) };
    let items = visible_items(s);
    let mut list = list;
    if items.is_empty() {
        list = list.child(empty("No refs found."));
    }
    let active = ui.branch_active.min(items.len().saturating_sub(1));
    let mut rows = col().w_full();
    for (i, item) in items.into_iter().enumerate() {
        let (label, badge) = match &item {
            Item::CheckoutPr(_) => (format!("Checkout {}", pr::singular(s)), None),
            Item::Ref(r) => (refs[*r].name.clone(), refs[*r].badge(&project_cwd)),
            Item::Create(name) => (format!("Create new ref \"{name}\""), None),
        };
        let subtitle = match &item {
            Item::CheckoutPr(reference) => Some(reference.clone()),
            _ => None,
        };
        let mut line = row()
            .key(format!("details-branch-item-{i}"))
            .w_full()
            .h(28.0)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(4.0)
            .cursor(Cursor::Pointer)
            .label(label.clone())
            .child(text(label).font(theme::TEXT_SM).color(t.c.contrast_foreground).single_line().truncate_middle().grow(1.0).shrink().min_w(0.0))
            .on_hover(move |s: &mut App| s.ui.chat.details.branch_active = i);
        if let Item::Ref(r) = &item {
            let name = refs[*r].name.clone();
            line = line.on_secondary_press(move |s: &mut App, at: rune::PointerPress, _: &mut Tx| {
                s.ui.chat.details.branch_copy = Some((name.clone(), (at.x, at.y)));
            });
        }
        let disabled = matches!(&item, Item::Ref(r) if ref_disabled(s, &refs[*r]));
        let mut line = if disabled { line.opacity(0.64).cursor(Cursor::Default) } else { line.on_press(move |s: &mut App| pick_item(s, item.clone())) };
        if let Some(subtitle) = subtitle {
            line = line.child(text(subtitle).font(theme::TEXT_XS).color(muted).single_line().truncate().shrink().min_w(0.0).max_w(160.0));
        }
        if let Some(badge) = badge {
            line = line.child(text(badge).font(theme::TEXT_3XS).color(muted.alpha_mul(0.45)));
        }
        if i == active {
            line = line.fill(t.c.contrast_foreground.alpha_mul(0.09));
        }
        rows = rows.child(line);
    }
    let (near_end, last) = (ui.near_end.clone(), ui.last_scroll.clone());
    let waker = s.ui.panel.waker.clone();
    // The list: ps-1 py-2 inside ComboboxListVirtualized's px-1 py-1, at most 224 tall.
    let rows = col().w_full().pl(8.0).pr(4.0).py(12.0).child(rows);
    let rows = rows.max_h(224.0).scroll(rune::ScrollSpec { stick_bottom: false }).on_scroll_offset(move |at: rune::ScrollPos| {
        if next_page_after_scroll(last.replace(Some(at.offset[1])), at.offset[1], at.max[1]) && !near_end.replace(true) {
            if let Some(w) = &waker {
                w();
            }
        }
    });
    list = list.child(rows);
    let footer = match &ui.listing {
        Some((_, _, true)) => Some("Loading more refs...".to_string()),
        _ if ui.refs_next.is_some() => Some(format!("Showing {} of {} refs", thousands(refs.len() as u64), thousands(ui.refs_total))),
        _ => None,
    };
    if let Some(footer) = footer {
        list = list.child(status(&footer));
    }
    if selecting_base(s) {
        let on = draft_run(s).is_some_and(|r| r.from_origin);
        // A label row under a border-border/60 rule: px-3 py-2, medium xs muted, a 12px icon.
        list = list.child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.6))).child(
            row()
                .w_full()
                .px(12.0)
                .py(8.0)
                .gap(12.0)
                .center_y()
                .help("Uses the latest commit of the base branch’s tracked remote branch, even if its name differs. Otherwise uses the same-named branch on origin. Uses your local branch if that remote branch is missing.")
                .child(row().gap(6.0).center_y().grow(1.0).min_w(0.0).child(icon(&icons::REFRESH_CW, 12.0).color(muted)).child(text("Start from remote").font(theme::XS_MEDIUM).color(muted).truncate()))
                .child(
                    crate::kit::Switch::new(on)
                        .size("sm")
                        .label("Start from remote")
                        .hide_label()
                        .on_toggle(|s: &mut App| {
                            let run = draft_run_mut(s);
                            run.from_origin = !run.from_origin;
                        })
                        .build(&crate::kit::theme(t)),
                ),
        );
    }
    list
}

/// The git and script dialogs, drawn over the whole window.
pub fn overlays(s: &App, t: &T) -> Vec<El> {
    let mut out = vec![];
    git::overlays(s, t, &mut out);
    scripts::overlays(s, t, &mut out);
    pr::overlays(s, t, &mut out);
    restore_dialog(s, t, &mut out);
    composer::fanout::retry_dialog(s, t, &mut out);
    cleanup::dialog(s, t, &mut out);
    items::overlay(s, t, &mut out);
    child_work::overlay(s, t, &mut out);
    if let Some((name, (x, y))) = s.ui.chat.details.branch_copy.clone() {
        let copy = crate::kit::MenuItem::new("Copy branch name").icon(&icons::COPY).on_select_tx(move |s: &mut App, tx: &mut Tx| {
            s.ui.chat.details.branch_copy = None;
            tx.copy(name.clone());
            palette::toast(s, "Branch name copied", Some(&name));
        });
        crate::kit::Menu::new("details-branch-copy")
            .native(true)
            .width(198.0)
            .entries(vec![crate::kit::MenuEntry::item(copy)])
            .open(true)
            .at(x, y)
            .on_close(|s: &mut App| s.ui.chat.details.branch_copy = None)
            .overlay(&crate::kit::theme(t), &mut out);
    }
    out
}

/// Scene: the Open in editor chooser, with web's editor names.
pub fn scene_editor_menu(s: &mut App) {
    (crate::shell::SCENES[1].1)(s);
    if let crate::data::Source::Fixture(state) = &mut s.src
        && let Some(config) = state.config.as_mut()
    {
        config["availableEditors"] = json!(["cursor", "vscode", "zed", "file-manager"]);
    }
    s.ui.chat.details.menu = Some(Menu::Editor);
}

/// Scene: the branch picker over a page of refs, its second row highlighted.
pub fn scene_branch_menu(s: &mut App) {
    (crate::shell::SCENES[1].1)(s);
    let project = cwd(s).unwrap_or_default();
    let r = |name: &str, remote: bool, current: bool, default: bool, worktree: Option<String>| Ref { name: name.into(), remote, current, default, worktree };
    let refs = vec![
        r("feat/greeting", false, true, false, None),
        r("main", false, false, true, None),
        r("fix/terminal-first-key", false, false, false, Some(format!("{project}-worktrees/terminal"))),
        r("chore/bump-deps", false, false, false, None),
        r("origin/main", true, false, false, None),
        r("origin/release/1.4", true, false, false, None),
    ];
    let ui = &mut s.ui.chat.details;
    ui.refs_total = refs.len() as u64;
    ui.refs = Some(refs);
    ui.branch_query = TextEdit::new("");
    ui.branch_query.focused = true;
    ui.branch_active = 1;
    ui.menu = Some(Menu::Branch);
}

/// Scene: the confirm before restoring the thread's branch over uncommitted changes.
pub fn scene_branch_restore(s: &mut App) {
    (crate::shell::SCENES[1].1)(s);
    s.ui.chat.details.restore_confirm = Some(("feat/greeting".into(), None));
}

/// `composer.branch` opens the branch picker and `composer.workspace` a draft's run context,
/// showing the card first when it is closed. Web binds both outside the terminal.
pub fn shortcut(s: &mut App, e: &KeyEvent) -> bool {
    // `editor.openFavorite` opens the card's preferred editor, as its button does.
    if crate::panel::shortcut(s, e, "editor.openFavorite") {
        let Some((editor, _)) = preferred_editor(s).filter(|_| cwd(s).is_some() && can_open_editor(s)) else { return false };
        open_in_editor(s, Some(editor));
        return true;
    }
    if crate::panel::terminal_active(s) || composer::ctx(s).project.is_none() {
        return false;
    }
    if crate::panel::shortcut(s, e, "composer.previousWorktree") {
        if !use_previous_worktree(s) {
            return false;
        }
        let route = s.frame_route().clone();
        composer::focus(s, &route);
        return true;
    }
    if crate::panel::shortcut(s, e, "composer.cycleHost") {
        return cycle_host(s);
    }
    if crate::panel::shortcut(s, e, "threadPanel.toggle") {
        let fits = s.ui.chat.details.fits.get();
        toggle(s, fits);
        return true;
    }
    // The menus anchor to their rows, so a hidden row's shortcut falls through.
    let shown = |id: &str| items::visible(&s.client_settings.values, id);
    let menu = if crate::panel::shortcut(s, e, "composer.branch") && shown("branch") && vcs(s).is_some_and(|v| v["local"]["isRepo"] == true) {
        Menu::Branch
    } else if crate::panel::shortcut(s, e, "composer.workspace") && shown("environment") && draft_run(s).is_some() {
        Menu::RunContext
    } else if crate::panel::shortcut(s, e, "composer.host") && draft_run(s).is_some() && !hosts(s).is_empty() {
        Menu::RunContext
    } else {
        return false;
    };
    let key = composer::draft_key(s, s.frame_route());
    let ui = &mut s.ui.chat.details;
    ui.closed.remove(&key);
    ui.popover = Some(key);
    if ui.menu != Some(menu) {
        ui.menu = None;
        toggle_menu(s, menu);
    }
    true
}

/// A control's tooltip with its shortcut, as web's `{label} · {shortcut}`.
fn with_shortcut(s: &App, label: &str, command: &str) -> String {
    match palette::shortcut_label(s, command) {
        k if k.is_empty() => label.to_string(),
        k => format!("{label} · {k}"),
    }
}

/// Escape closes an open menu even when the terminal has focus; then script editor shortcut
/// capture and `script.{id}.run` shortcuts.
pub fn on_key(s: &mut App, e: &KeyEvent) -> bool {
    if e.key == Key::Escape && s.ui.chat.details.menu.take().is_some() {
        return true;
    }
    scripts::on_key(s, e)
}

#[cfg(test)]
mod tests {
    use super::{Access, DraftRun, Item, Pending, Ref, Shown, access, branch_denied, branch_items, draft_run, grants, git, lane, lane_max, local_name, max_width, shown, switching, take_draft_workspace, toggle};
    use serde_json::{Value, json};
    use crate::{App, Route, data};

    #[test]
    fn visibility_is_per_thread_and_a_narrow_lane_opens_a_popover() {
        let mut s = App::new();
        let ids: Vec<String> = s.state().threads.iter().filter(|t| !data::s(t, "projectId").is_empty()).map(|t| data::s(t, "id").to_string()).take(2).collect();
        s.navigate(Route::Thread(ids[0].clone()));
        let (wide, narrow) = ((1182.0, 848.0), (900.0, 848.0));
        assert_eq!(shown(&s, wide.0, wide.1), Shown::Inline);
        assert_eq!(shown(&s, narrow.0, narrow.1), Shown::None, "narrow: closed until toggled");
        toggle(&mut s, true);
        assert_eq!(shown(&s, wide.0, wide.1), Shown::None);
        s.navigate(Route::Thread(ids[1].clone()));
        assert_eq!(shown(&s, wide.0, wide.1), Shown::Inline, "another thread keeps its card");
        toggle(&mut s, false);
        assert_eq!(shown(&s, narrow.0, narrow.1), Shown::Popover);
        s.navigate(Route::Thread(ids[0].clone()));
        assert_eq!(shown(&s, narrow.0, narrow.1), Shown::None);
        toggle(&mut s, true);
        assert_eq!(shown(&s, wide.0, wide.1), Shown::Inline);
    }

    #[test]
    fn branch_picker_filters_refs_and_offers_create_for_a_new_name() {
        let r = |name: &str| Ref { name: name.into(), ..Ref::default() };
        let refs = vec![r("main"), r("feature/my-branch"), r("origin/main")];
        assert_eq!(branch_items(&refs, "", true), vec![Item::Ref(0), Item::Ref(1), Item::Ref(2)]);
        assert_eq!(branch_items(&refs, "MAIN", true), vec![Item::Ref(0), Item::Ref(2), Item::Create("MAIN".into())]);
        assert_eq!(branch_items(&refs, "main", true), vec![Item::Ref(0), Item::Ref(2)], "an existing name is not offered");
        assert_eq!(branch_items(&refs, " my branch ", true), vec![Item::Ref(1), Item::Create("my-branch".into())], "matches the sanitized query");
        assert_eq!(branch_items(&refs, "new", false), vec![], "no create while picking a worktree base");
        assert_eq!(local_name("origin/feature/x"), "feature/x");
        assert_eq!(local_name("main"), "main");
    }

    #[test]
    fn ref_pages_append_without_duplicates_and_load_on_a_downward_scroll_near_the_end() {
        let mut ui = super::Ui::default();
        let page = |names: &[&str], next: Option<u64>| json!({"refs": names.iter().map(|n| json!({"name": n})).collect::<Vec<_>>(), "nextCursor": next, "totalCount": 1234});
        super::take_refs(&mut ui, "fe".into(), false, &page(&["feat/a", "feat/b"], Some(2)));
        super::take_refs(&mut ui, "fe".into(), true, &page(&["feat/b", "feat/c"], None));
        let names: Vec<_> = ui.refs.as_ref().unwrap().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["feat/a", "feat/b", "feat/c"]);
        assert_eq!((ui.refs_next, ui.refs_total, ui.refs_query.as_str()), (None, 1234, "fe"));
        super::take_refs(&mut ui, "x".into(), false, &page(&["x"], Some(1)));
        assert_eq!(ui.refs.as_ref().unwrap().len(), 1, "a new search replaces the list");
        assert!(!super::next_page_after_scroll(None, 400.0, 420.0), "the first offset is not a scroll");
        assert!(super::next_page_after_scroll(Some(300.0), 330.0, 420.0));
        assert!(!super::next_page_after_scroll(Some(340.0), 330.0, 420.0), "scrolling up");
        assert!(!super::next_page_after_scroll(Some(100.0), 200.0, 420.0), "far from the end");
        assert_eq!((super::thousands(0), super::thousands(999), super::thousands(1234), super::thousands(1234567)), ("0".into(), "999".into(), "1,234".into(), "1,234,567".into()));
    }

    #[test]
    fn a_local_thread_off_its_branch_is_a_mismatch() {
        assert_eq!(super::branch_mismatch("feature", "", "main"), Some(("feature".into(), "main".into())));
        assert_eq!(super::branch_mismatch("main", "", "main"), None);
        assert_eq!(super::branch_mismatch("feature", "/wt/feature", "main"), None, "a worktree thread has its own checkout");
        assert_eq!(super::branch_mismatch("", "", "main"), None);
        assert_eq!(super::branch_mismatch("feature", "", ""), None, "detached or unknown");
    }

    #[test]
    fn a_pick_waits_for_the_switch_under_way() {
        let mut s = App::new();
        assert!(!switching(&s));
        s.ui.chat.details.pending = Some((2, Pending::Branch("feature".into(), None, "Failed to switch ref.", None)));
        assert!(switching(&s));
        super::create_ref(&mut s, "other");
        assert!(matches!(&s.ui.chat.details.pending, Some((2, Pending::Branch(name, ..))) if name == "feature"));
    }

    #[test]
    fn the_connections_session_gates_branch_changes() {
        let mut s = App::new();
        assert!(grants(&s, "source-control:write"));
        s.ui.chat.details.session = Some(Ok(json!({"authenticated": true, "scopes": ["orchestration:read"]})));
        assert!(!grants(&s, "source-control:write"));
        assert!(branch_denied(&mut s));
        s.ui.chat.details.session = Some(Ok(json!({"authenticated": true, "scopes": ["orchestration:operate"]})));
        assert!(grants(&s, "source-control:write") && !branch_denied(&mut s));
        s.ui.chat.details.session = Some(Err(()));
        assert!(!grants(&s, "filesystem:read"));
        assert_eq!(access(&s, "filesystem:read"), Access::Denied);
        s.ui.chat.details.session = Some(Ok(json!({"authenticated": true, "scopes": ["filesystem:read"]})));
        assert_eq!(access(&s, "filesystem:read"), Access::Granted);
        s.ui.chat.details.session = None;
        assert_eq!(access(&s, "filesystem:read"), Access::Granted, "the recorded scenes never ask");
    }

    #[test]
    fn the_previous_worktree_scene_offers_it_on_a_draft() {
        let mut s = App::new();
        let scene = crate::panel::SCENES.iter().find(|(name, _)| *name == "details-previous-worktree").unwrap();
        (scene.1)(&mut s);
        assert!(crate::chat::composer::ctx(&s).thread.is_none(), "a draft");
        assert_eq!(super::previous_seed(&s), Some((Some("feat/login-redirect".into()), "/home/dev/.t3/worktrees/demo/feat-login".into())));
    }

    #[test]
    fn previous_worktree_is_the_projects_latest_live_thread_elsewhere() {
        let t = |project: &str, path: &str, branch: &str, at: &str, archived: bool| {
            json!({"projectId": project, "worktreePath": path, "branch": branch, "updatedAt": at, "archivedAt": if archived { json!(at) } else { Value::Null }})
        };
        let threads = [
            t("p", "/w/a", "feat/a", "2026-01-02T00:00:00Z", false),
            t("p", "/w/b", "feat/b", "2026-01-03T00:00:00Z", false),
            t("p", "/w/c", "feat/c", "2026-01-04T00:00:00Z", true),
            t("q", "/w/d", "feat/d", "2026-01-05T00:00:00Z", false),
            t("p", "", "main", "2026-01-06T00:00:00Z", false),
            t("p", "/w/e", "", "2026-01-01T00:00:00Z", false),
        ];
        assert_eq!(super::previous_worktree(&threads, "p", None), Some((Some("feat/b".into()), "/w/b".into())));
        assert_eq!(super::previous_worktree(&threads, "p", Some("/w/b")), Some((Some("feat/a".into()), "/w/a".into())), "not the one the draft is in");
        assert_eq!(super::previous_worktree(&threads[5..], "p", None), Some((None, "/w/e".into())));
        assert_eq!(super::previous_worktree(&threads, "z", None), None);
    }

    #[test]
    fn a_draft_in_an_existing_worktree_launches_and_runs_there() {
        let mut s = App::new();
        let project = s.state().projects.iter().map(|p| data::s(p, "id").to_string()).next().unwrap();
        s.navigate(Route::Home { project_id: Some(project.clone()) });
        s.ui.chat.home.workspace = Some((project.clone(), json!({"type": "existing_worktree", "worktreePath": "/w/b", "branch": "feat/b"})));
        assert_eq!(super::draft_worktree(&s), Some(("/w/b".into(), Some("feat/b".into()))));
        assert_eq!(super::cwd(&s).as_deref(), Some("/w/b"), "git status and branch picks act in the worktree");
        super::set_env_mode(&mut s, false);
        assert!(super::draft_worktree(&s).is_some(), "the checkout choice keeps the worktree");
        super::set_env_mode(&mut s, true);
        assert_eq!(super::draft_worktree(&s), None, "a new worktree replaces it");
        assert_eq!(crate::chat::home::take_workspace(&mut s, &project), json!({"type": "root"}));
    }

    #[test]
    fn a_draft_set_to_new_worktree_launches_one_from_its_base() {
        let mut s = App::new();
        let project = s.state().projects.iter().map(|p| data::s(p, "id").to_string()).next().unwrap();
        s.navigate(Route::Home { project_id: Some(project) });
        assert_eq!(take_draft_workspace(&mut s), None, "the checkout by default");
        *super::draft_run_mut(&mut s) = DraftRun { worktree: true, base: Some("feature/x".into()), from_origin: true, ..DraftRun::default() };
        assert_eq!(take_draft_workspace(&mut s), Some(json!({"type": "worktree", "baseRef": "feature/x", "startFromOrigin": true})));
        assert_eq!(take_draft_workspace(&mut s), None, "taken once: the next draft starts over");
    }

    #[test]
    fn new_worktrees_start_from_origin_per_the_project_setting() {
        let mut s = App::new();
        let project = s.state().projects.iter().map(|p| data::s(p, "id").to_string()).next().unwrap();
        s.navigate(Route::Home { project_id: Some(project.clone()) });
        s.ui.settings.optimistic_settings = Some(json!({"newWorktreesStartFromOrigin": true}));
        super::set_env_mode(&mut s, true);
        assert!(draft_run(&s).unwrap().from_origin);
        super::set_env_mode(&mut s, false);
        assert!(!draft_run(&s).unwrap().from_origin, "FAILED: the checkout never starts from origin");
        s.ui.settings.optimistic_settings = Some(json!({"newWorktreesStartFromOrigin": true, "projectSettingsOverrides": {project: {"newWorktreesStartFromOrigin": false}}}));
        super::set_env_mode(&mut s, true);
        assert!(!draft_run(&s).unwrap().from_origin, "FAILED: the project's override wins");
    }

    fn chord(c: char) -> rune::KeyEvent {
        let mut e = rune::KeyEvent::from(rune::Key::Command(c));
        e.mods.cmd = true;
        e.mods.ctrl = !cfg!(target_os = "macos");
        e.mods.shift = true;
        e
    }

    #[test]
    fn the_thread_panel_chord_toggles_the_details_card_as_the_lane_fits() {
        let mut s = App::new();
        let id = s.state().threads.iter().find(|t| !data::s(t, "projectId").is_empty()).map(|t| data::s(t, "id").to_owned()).unwrap();
        if let data::Source::Fixture(state) = &mut s.src {
            state.config.get_or_insert_with(|| serde_json::json!({}))["keybindings"] =
                serde_json::json!([{"command": "threadPanel.toggle", "shortcut": {"key": "b", "modKey": true, "shiftKey": true}}]);
        }
        s.navigate(Route::Thread(id));
        let mut chord = rune::KeyEvent::from(rune::Key::Command('b'));
        chord.mods.cmd = true;
        chord.mods.ctrl = !cfg!(target_os = "macos");
        chord.mods.shift = true;
        let (wide, narrow) = ((1182.0, 848.0), (900.0, 848.0));
        s.ui.chat.details.fits.set(true);
        assert_eq!(shown(&s, wide.0, wide.1), Shown::Inline);
        assert!(super::shortcut(&mut s, &chord));
        assert_eq!(shown(&s, wide.0, wide.1), Shown::None, "the chord closes the inline card");
        assert!(super::shortcut(&mut s, &chord));
        assert_eq!(shown(&s, wide.0, wide.1), Shown::Inline, "and opens it again");
        s.ui.chat.details.fits.set(false);
        assert!(super::shortcut(&mut s, &chord));
        assert_eq!(shown(&s, narrow.0, narrow.1), Shown::Popover, "a narrow lane opens the popover");
    }

    #[test]
    fn the_host_chord_lists_one_copy_per_environment_and_moves_the_draft() {
        let mut s = App::new();
        let identity = json!({"canonicalKey": "github.com/acme/app", "name": "app", "rootPath": "/src/app"});
        let project = |id: &str| json!({"id": id, "title": "app", "workspaceRoot": "/src/app", "repositoryIdentity": identity, "pinnedAt": null, "archivedAt": null});
        if let data::Source::Fixture(state) = &mut s.src {
            state.config.get_or_insert_with(|| json!({}))["keybindings"] =
                json!([{"command": "composer.host", "shortcut": {"key": "h", "modKey": true, "shiftKey": true}},
                    {"command": "composer.cycleHost", "shortcut": {"key": "j", "modKey": true, "shiftKey": true}}]);
            let mut remote = project("remote-app");
            remote[crate::env::TAG] = json!("box");
            state.projects.extend([project("local-app"), project("local-app-2"), remote]);
        }
        s.navigate(Route::Home { project_id: Some("local-app".into()) });
        assert_eq!(super::hosts(&s), vec!["local-app".to_string(), "remote-app".to_string()], "FAILED: one row per environment");
        s.ui.chat.composer.drafts.entry(crate::chat::composer::home_key("local-app")).or_default().edit.replace_text("carry me");
        assert!(super::shortcut(&mut s, &chord('h')), "FAILED: the chord opens Run on");
        assert_eq!(s.ui.chat.details.menu, Some(super::Menu::RunContext));
        super::move_draft(&mut s, "remote-app");
        assert_eq!(s.route, Route::Home { project_id: Some("remote-app".into()) });
        assert_eq!(s.ui.chat.composer.drafts[&crate::chat::composer::home_key("remote-app")].edit.text, "carry me", "FAILED: the prompt moves along");
        assert!(!s.ui.chat.composer.drafts.contains_key(&crate::chat::composer::home_key("local-app")));

        // composer.cycleHost steps to the next machine and wraps around.
        assert!(super::shortcut(&mut s, &chord('j')), "FAILED: the cycle chord moves the draft");
        assert_eq!(s.route, Route::Home { project_id: Some("local-app".into()) });
        assert_eq!(s.ui.chat.composer.drafts[&crate::chat::composer::home_key("local-app")].edit.text, "carry me");
        assert!(super::shortcut(&mut s, &chord('j')));
        assert_eq!(s.route, Route::Home { project_id: Some("remote-app".into()) }, "FAILED: the cycle wraps");
    }

    #[test]
    fn a_branch_made_by_commit_on_new_branch_becomes_the_drafts() {
        let mut s = App::new();
        let project = s.state().projects.iter().map(|p| data::s(p, "id").to_string()).next().unwrap();
        s.navigate(Route::Home { project_id: Some(project.clone()) });
        *super::draft_run_mut(&mut s) = DraftRun { worktree: true, base: Some("main".into()), from_origin: true, ..DraftRun::default() };
        let target = super::BranchTarget::current(&s);
        git::sync_branch(&mut s, target.as_ref(), &json!({"branch": {"status": "skipped_not_requested"}}));
        assert_eq!(draft_run(&s).unwrap().base.as_deref(), Some("main"));
        let other_thread = data::s(s.state().threads.iter().next().unwrap(), "id").to_string();
        s.navigate(Route::Thread(other_thread));
        git::sync_branch(&mut s, target.as_ref(), &json!({"branch": {"status": "created", "name": "feature/fix"}}));
        s.navigate(Route::Home { project_id: Some(project) });
        let run = draft_run(&s).unwrap();
        assert_eq!((run.base.as_deref(), run.from_origin), (Some("feature/fix"), false));
        assert_eq!(take_draft_workspace(&mut s), Some(json!({"type": "worktree", "baseRef": "feature/fix"})));
    }

    #[test]
    fn a_ref_in_a_worktree_is_the_projects_own_only_at_its_checkout_root() {
        use super::{can_change_branch, own_checkout, ref_disabled, set_session};
        assert!(own_checkout("/repo/", "/repo/apps/web", Some("/repo")), "a subfolder project's checkout is the repository root");
        assert!(!own_checkout("/repo/apps/web", "/repo/apps/web", Some("/repo")));
        assert!(!own_checkout("/wt/feature", "/repo", Some("/repo")));
        assert!(own_checkout("/repo/nested", "/repo", None), "until the root loads, one holding the project counts");
        assert!(!own_checkout("/repo-2", "/repo", None));
        let r = |worktree: Option<&str>| Ref { name: "feature".into(), remote: false, current: false, default: false, worktree: worktree.map(str::to_string) };
        let mut s = App::new();
        set_session(&mut s, Some(Ok(json!({"authenticated": true, "scopes": ["orchestration:operate"], "auth": {"serverUpdateScope": "environment:maintain"}}))));
        assert!(ref_disabled(&s, &r(None)), "FAILED: switching needs source-control:write");
        assert!(!ref_disabled(&s, &r(Some("/wt/feature"))), "FAILED: moving to an existing worktree does not");
        assert!(!can_change_branch(&s));
    }

    #[test]
    fn every_editor_web_lists_has_its_brand_mark() {
        // contracts EDITORS, in order.
        let ids = ["cursor", "trae", "kiro", "vscode", "vscode-insiders", "vscodium", "zed", "antigravity", "idea", "aqua", "clion", "datagrip", "dataspell", "goland", "phpstorm", "pycharm", "rider", "rubymine", "rustrover", "webstorm", "file-manager"];
        for dark in [false, true] {
            let t = crate::theme::theme(dark);
            for id in ids {
                assert!(super::editor_mark(&t, id, 16.0).is_some(), "FAILED: {id} has no mark");
            }
        }
        assert!(super::editor_mark(&crate::theme::theme(true), "nano", 16.0).is_none());
    }

    #[test]
    fn open_in_editor_needs_orchestration_access_and_a_route() {
        use super::{can_open_editor, editor_denied, editor_label, editors, set_session};
        let mut s = App::new();
        assert!(can_open_editor(&s));
        set_session(&mut s, Some(Ok(json!({"authenticated": true, "scopes": ["orchestration:read"]}))));
        assert!(editor_denied(&s) && !can_open_editor(&s), "FAILED: the server's editors need orchestration:operate");
        set_session(&mut s, None);
        s.ui.env.saved.push(crate::env::Saved { id: "box".into(), label: "Box".into(), base: "https://box.example.com".into(), bearer: String::new() });
        s.ui.env.active = "box".into();
        if let crate::data::Source::Fixture(state) = &mut s.src
            && let Some(config) = state.config.as_mut().and_then(|c| c.as_object_mut())
        {
            config.remove("remoteOpenTargets");
        }
        assert!(!editor_denied(&s) && !can_open_editor(&s), "FAILED: no SSH route leaves nothing to open with");
        assert!(editors(&s).is_empty());
        assert_eq!(editor_label("vscode"), "VS Code");
        assert_eq!(editor_label("file-manager"), if cfg!(target_os = "macos") { "Finder" } else { "Files" });
    }

    #[test]
    fn the_preferred_editor_is_the_last_picked_while_installed() {
        let editors = || vec![("cursor".to_string(), "Cursor".to_string()), ("vscode".to_string(), "VS Code".to_string())];
        assert_eq!(super::pick_editor(editors(), Some("vscode")).map(|e| e.0), Some("vscode".to_string()));
        assert_eq!(super::pick_editor(editors(), Some("zed")).map(|e| e.0), Some("cursor".to_string()), "an uninstalled pick falls back to the first");
        assert_eq!(super::pick_editor(editors(), None).map(|e| e.0), Some("cursor".to_string()));
        assert_eq!(super::pick_editor(Vec::new(), Some("zed")), None);
    }

    #[test]
    fn lane_matches_web_layout() {
        // 1440 window minus the 256 sidebar and its border: the web lane sits at x=122, 736 wide.
        let l = lane(1182.0, 848.0, false);
        assert!(l.card);
        assert_eq!((l.left, l.width), (122.0, 736.0));
        // Hidden card: centered.
        let l = lane(1182.0, 848.0, true);
        assert_eq!((l.left, l.width), (223.0, 736.0));
        // Too narrow for the card.
        assert!(!lane(1000.0, 848.0, false).card);
        // Chat width: wide caps at 72rem, full fills the area inside the padding or the card.
        let l = lane_max(1600.0, 848.0, true, max_width(Some("wide")));
        assert_eq!((l.left, l.width), (224.0, 1152.0));
        let l = lane_max(1600.0, 848.0, true, max_width(Some("full")));
        assert_eq!((l.left, l.width), (48.0, 1504.0));
        let l = lane_max(1600.0, 848.0, false, max_width(Some("full")));
        assert_eq!((l.left, l.width), (48.0, 1228.0));
        assert_eq!(max_width(Some("comfortable")), max_width(None));
    }
}
