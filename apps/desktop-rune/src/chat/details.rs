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
use crate::App;
use std::collections::{HashMap, HashSet};
use crate::shell::palette;

mod git;
pub mod pr;
pub mod scripts;

#[derive(Default)]
pub struct Ui {
    /// Threads (by draft key) whose inline card the user closed (`inlineOpen: false`).
    closed: HashSet<String>,
    /// The thread whose card is open as a popover, when the lane is too narrow for it inline.
    popover: Option<String>,
    pub menu: Option<Menu>,
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
    /// "Switch to {branch}?" over uncommitted changes.
    restore_confirm: Option<String>,
    /// This connection's session (`GET /api/auth/session`): the reply, when it came, and the
    /// read in flight. Its grants gate git actions, branch changes and the diff panel.
    session: Option<Result<Value, ()>>,
    session_at: i64,
    session_pending: Option<u64>,
}

const SESSION_STALE_MS: i64 = 5 * 60_000;

/// The connection's `permissionAtom` for a scope. Unknown until the session answers, which
/// leaves it to the server; a failed read denies, as web's guards do.
pub fn grants(s: &App, scope: &str) -> bool {
    match &s.ui.chat.details.session {
        Some(Ok(session)) => crate::settings::session_grants(session, scope),
        Some(Err(())) => false,
        None => true,
    }
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
    Branch(String, Option<String>, &'static str),
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
    let x = w - WIDTH - GAP;
    let card = !hidden && x - CLEARANCE - LANE_PADDING >= MIN_CHAT && h - 2.0 * GAP >= 160.0;
    let centered = (w - LANE_PADDING * 2.0).min(theme::layout::CHAT_MAX_W).max(0.0);
    let right = if card { x - CLEARANCE } else { w - LANE_PADDING };
    let width = centered.min(right - LANE_PADDING).max(0.0);
    let left = ((w - width) / 2.0).min(right - width).max(LANE_PADDING);
    Lane { left, width, card }
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
    if let Some(cwd) = cwd(s)
        && let Some(net) = s.src.net()
    {
        net.watch_vcs(&cwd);
    }
    scripts::load_file(s, false);
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
    row()
        .w_full()
        .gap(1.0)
        .child(main.w(229.0).corners([8.0, 0.0, 0.0, 8.0]))
        .child(
            el().key(key.to_string())
                .size(32.0, 32.0)
                .center()
                .corners([0.0, 8.0, 8.0, 0.0])
                .hover(t.c.accent)
                .cursor(Cursor::Pointer)
                .label(label.to_string())
                .help(label.to_string())
                .child(icon(&icons::CHEVRON_DOWN, 16.0).color(t.c.muted_foreground))
                .on_press(move |s: &mut App| toggle_menu(s, menu)),
        )
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
fn next_page_after_scroll(previous: Option<f32>, offset: f32, max: f32) -> bool {
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
    DraftRun { worktree: crate::settings::server_settings(s)["defaultThreadEnvMode"] == "worktree", ..DraftRun::default() }
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

fn set_env_mode(s: &mut App, worktree: bool) {
    s.ui.chat.details.menu = None;
    // A new worktree replaces the existing one the draft was in; the checkout keeps it.
    if worktree && draft_worktree(s).is_some() {
        s.ui.chat.home.workspace = None;
    }
    let run = draft_run_mut(s);
    if run.worktree != worktree {
        run.worktree = worktree;
        run.base = None;
    }
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
/// `runBranchAction`'s guard: changing the branch writes source control, and a thread's metadata.
fn branch_denied(s: &mut App) -> bool {
    let thread = composer::ctx(s).thread.is_some();
    let denied = !grants(s, "source-control:write") || (thread && !grants(s, "orchestration:operate"));
    if denied {
        palette::toast_error_brief(s, "Action unavailable", Some("This connection cannot change the thread's branch."));
    }
    denied
}

fn select_ref(s: &mut App, r: &Ref) {
    if selecting_base(s) {
        draft_run_mut(s).base = Some(r.name.clone());
        return;
    }
    if branch_denied(s) {
        return;
    }
    if switching(s) {
        return;
    }
    let c = composer::ctx(s);
    let project_cwd = c.project.map(|p| data::s(p, "workspaceRoot").to_string()).unwrap_or_default();
    let active = worktree_here(s);
    if let Some(path) = &r.worktree {
        let own = path == &project_cwd || path.starts_with(&format!("{}/", project_cwd.trim_end_matches('/')));
        set_thread_branch(s, &r.name, (!own).then(|| path.clone()));
        return;
    }
    let next = if r.default { None } else { active };
    let checkout = next.clone().unwrap_or(project_cwd);
    let name = if r.remote { local_name(&r.name) } else { r.name.clone() };
    if let Some(net) = s.src.net() {
        let id = net.request("vcs.switchRef", json!({"cwd": checkout, "refName": r.name}));
        s.ui.chat.details.pending = Some((id, Pending::Branch(name, next, "Failed to switch ref.")));
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
        s.ui.chat.details.pending = Some((id, Pending::Branch(name.to_string(), worktree, "Failed to create and switch ref.")));
    }
}

fn set_thread_branch(s: &mut App, branch: &str, worktree: Option<String>) {
    let c = composer::ctx(s);
    let (thread, project) = (c.thread.map(|t| data::s(t, "id").to_string()), c.project.map(|p| data::s(p, "id").to_string()));
    match (thread, project) {
        (Some(thread), _) => {
            if let Some(net) = s.src.net() {
                net.dispatch(json!({"type": "thread.metadata.update", "threadId": thread, "branch": branch, "worktreePath": worktree}));
            }
        }
        // A draft moves into the ref's worktree, or follows the switch in the one it is in.
        (None, Some(project)) if worktree.is_some() => {
            let path = worktree.unwrap_or_default();
            let mut strategy = json!({"type": "existing_worktree", "worktreePath": path});
            if !branch.is_empty() {
                strategy["branch"] = json!(branch);
            }
            s.ui.chat.home.workspace = Some((project, strategy));
        }
        _ => {}
    }
}

/// `resolveLocalCheckoutBranchMismatch`: a local-checkout thread whose branch is no longer the
/// one checked out, as `(thread branch, current branch)`.
fn branch_mismatch(thread_branch: &str, worktree: &str, current: &str) -> Option<(String, String)> {
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
    s.ui.chat.composer.drafts.get(&composer::draft_key(s, s.frame_route())).is_some_and(|d| !d.edit.text.trim().is_empty() || !d.attachments.is_empty())
}

/// The mismatch notice appears once the composer has content and stays for that mismatch.
fn note_mismatch(s: &mut App) -> bool {
    match mismatch(s) {
        Some((key, ..)) if composer_has_content(s) => s.ui.chat.details.mismatch_shown.insert(key),
        _ => false,
    }
}

fn restore_branch(s: &mut App, confirmed: bool) {
    let Some((_, branch, _)) = mismatch(s) else { return };
    if switching(s) {
        return;
    }
    if !confirmed && vcs(s).is_some_and(|v| v["local"]["hasWorkingTreeChanges"] == true) {
        s.ui.chat.details.restore_confirm = Some(branch);
        return;
    }
    s.ui.chat.details.restore_confirm = None;
    if let (Some(cwd), Some(net)) = (cwd(s), s.src.net()) {
        let id = net.request("vcs.switchRef", json!({"cwd": cwd, "refName": branch}));
        s.ui.chat.details.pending = Some((id, Pending::Branch(branch, None, "Failed to switch checkout")));
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
                    .on_press(|s: &mut App| restore_branch(s, false))
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
    let Some(branch) = s.ui.chat.details.restore_confirm.clone() else { return };
    let close = |s: &mut App| s.ui.chat.details.restore_confirm = None;
    let footer = row()
        .w_full()
        .gap(8.0)
        .child(el().grow(1.0))
        .child(crate::kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
        .child(crate::kit::Button::new("Switch branch").on_press(|s: &mut App| restore_branch(s, true)).build(t));
    crate::kit::Dialog::new("branch-restore", format!("Switch to {branch}?"))
        .open(true)
        .alert()
        .description("You have uncommitted changes. They'll carry over to the other branch, or block the switch if they conflict.")
        .child(footer)
        .on_close(close)
        .build(t, out);
}

fn visible_items(s: &App) -> Vec<Item> {
    let ui = &s.ui.chat.details;
    let mut items = branch_items(ui.refs.as_deref().unwrap_or(&[]), &ui.branch_query.text, !selecting_base(s));
    if let Some(reference) = pr::parse_reference(&ui.branch_query.text) {
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
    let changed = git::poll(s) | scripts::poll(s) | pr::poll(s) | poll_refs(s) | note_mismatch(s) | poll_session(s);
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
        (Pending::Branch(name, worktree, _), Ok(v)) => {
            let branch = v["refName"].as_str().unwrap_or(&name).to_string();
            set_thread_branch(s, &branch, worktree);
            git::refresh(s);
            let route = s.frame_route().clone();
            composer::focus(s, &route);
        }
        (Pending::Branch(_, _, title), Err(e)) => palette::toast_error_brief(s, title, Some(&palette::error_message(&e, "An error occurred."))),
    }
    true
}

fn open_in_editor(s: &mut App, editor: Option<String>) {
    let (Some(cwd), editor) = (cwd(s), editor.or_else(|| editors(s).first().map(|e| e.0.clone()))) else { return };
    let Some(editor) = editor else { return };
    if crate::env::ssh::open_in_editor(s, &editor, &cwd) {
        return;
    }
    if let Some(net) = s.src.net() {
        net.request("shell.openInEditor", json!({"cwd": cwd, "editor": editor}));
    }
}

/// `(id, label)` for each editor the server found.
fn editors(s: &App) -> Vec<(String, String)> {
    if let Some(remote) = crate::env::ssh::editors(s) {
        return remote;
    }
    s.state()
        .config
        .as_ref()
        .and_then(|c| c["availableEditors"].as_array())
        .map(|v| {
            v.iter()
                .map(|e| match e.as_str() {
                    Some(id) => (id.to_string(), id.to_string()),
                    None => (data::s(e, "id").to_string(), data::s(e, "label").to_string()),
                })
                .collect()
        })
        .unwrap_or_default()
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

pub fn view(s: &App, _cx: &mut Cx<App>, t: &T, shown: Shown, w: f32) -> Option<El> {
    let card = card(s, t)?;
    match shown {
        Shown::None => None,
        Shown::Inline => Some(card.absolute(w - WIDTH - GAP, GAP + super::timeline::find::offset(s))),
        Shown::Popover => Some(card.anchored("topbar-details", Side::Bottom, Align::End, 4.0).modal(true, |s: &mut App| {
            s.ui.chat.details.popover = None;
            s.ui.chat.details.menu = None;
        })),
    }
}

fn card(s: &App, t: &T) -> Option<El> {
    let muted = t.c.muted_foreground;
    let fg = t.c.foreground.alpha_mul(0.8);
    let env = env_label(s);
    let thread = composer::ctx(s).thread;
    let run = draft_run(s);
    let new_worktree = run.as_ref().is_some_and(|r| r.worktree);
    let worktree = worktree_here(s).is_some();
    let mode = match (new_worktree, worktree, thread.is_some()) {
        (true, ..) => "New worktree",
        (_, true, true) => "Worktree",
        (_, true, false) => "Current worktree",
        (_, false, true) => "Local checkout",
        _ => "Current checkout",
    };
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
        .child(icon(&icons::SERVER, 18.0).color(muted))
        .child(text(env).font(ROW).color(if draft { fg } else { muted }).truncate().grow(1.0).shrink().min_w(0.0))
        .child(icon(if new_worktree { &icons::FOLDER_GIT_2 } else if worktree { &icons::GIT_FORK } else { &icons::FOLDER }, 16.0).color(muted).ml(1.0).help(workspace_path(s).unwrap_or_else(|| mode.to_string())));
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
    let editor = split(
        t,
        "details-editor-menu",
        {
            let open = action(t, "details-editor", icon(&icons::SQUARE_ARROW_OUT_UP_RIGHT, 16.0).color(muted), "Open in editor", fg);
            // No editor found: the button is disabled (opacity-64), the chooser still opens.
            if editors(s).is_empty() { open.opacity(0.64) } else { open.on_press(|s: &mut App| open_in_editor(s, None)) }
        },
        "Choose editor",
        Menu::Editor,
    );
    let script = scripts::view(s, t);
    let general = col().w_full().padding(8.0).pb(10.0).child(run_context).child(editor).child(script);

    let mut card = col()
        .key("thread-details")
        .w(WIDTH)
        .radius(radius::XL2)
        .fill(t.c.popover.alpha_mul(0.836))
        .border(1.0, if t.dark { Color::WHITE.alpha(0.1) } else { Color::BLACK.alpha(0.08) })
        .shadow(8.0, 24.0, Color::BLACK.alpha(if t.dark { 0.32 } else { 0.08 }))
        .padding(1.0)
        .child(general);

    // A failed status stream still shows the section, with Commit disabled and +0 -0.
    let failed = (vcs(s).is_none() && git::status_error(s).is_some()).then(|| json!({"isRepo": true}));
    if let Some(v) = vcs(s).map(|v| &v["local"]).or(failed.as_ref()).filter(|v| v["isRepo"] == true) {
        let current = data::s(v, "refName").to_string();
        // resolveBranchTriggerLabel: a new worktree names the branch it starts from.
        let branch = match run.as_ref().filter(|r| r.worktree) {
            Some(r) => match worktree_base(s, r) {
                Some(base) if r.from_origin => format!("From {base} (remote)"),
                Some(base) => format!("From {base}"),
                None => "Select ref".into(),
            },
            None if current.is_empty() => "Select ref".into(),
            None => current,
        };
        let mut section = col()
            .w_full()
            .px(8.0)
            .pt(9.0)
            .pb(10.0)
            .child(el().absolute(0.0, 0.0).w_full().h(1.0).fill(t.c.border.alpha_mul(0.65)))
            .child(
                branch_row(t, &branch, fg, muted)
                    .help(with_shortcut(s, &branch, "composer.branch"))
                    .expanded(s.ui.chat.details.menu == Some(Menu::Branch))
                    .on_press(|s: &mut App| toggle_menu(s, Menu::Branch)),
            );
        if let Some(prs) = pr::rows(s, t) {
            section = section.child(prs);
        }
        // The Changes row hides with the quiet git row; on a draft it opens the project's changes.
        let Some(git) = git::view(s, t, v, draft) else { return Some(card.child(section)) };
        section = section.child(git);
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
        section = section.child(changes.on_press(|s: &mut App| crate::panel::open_changes(s)));
        card = card.child(section);
    }
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
            if editors.is_empty() {
                list = list.child(row().h(32.0).px(8.0).center_y().child(text("No editors found").font(theme::TEXT_SM).color(t.c.muted_foreground)));
            }
            for (id, label) in editors {
                list = list.child(menu_row(t, None, &label, Some(move |s: &mut App| {
                    s.ui.chat.details.menu = None;
                    open_in_editor(s, Some(id.clone()));
                }), None));
            }
            ("details-editor-menu", list)
        }
    };
    Some(list.anchored(anchor, Side::Bottom, Align::End, 4.0).modal(true, close))
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
    let worktree = draft_run(s).is_some_and(|r| r.worktree);
    let mut rows = vec![
        group_label(t, "Run on"),
        radio_row(t, &icons::SERVER, &env_label(s), true, |s: &mut App| s.ui.chat.details.menu = None),
        el().h(1.0).mx(8.0).my(4.0).fill(t.c.border),
        group_label(t, "Workspace"),
        radio_row(t, &icons::FOLDER, if draft_worktree(s).is_some() { "Current worktree" } else { "Current checkout" }, !worktree, |s: &mut App| set_env_mode(s, false)),
        radio_row(t, &icons::FOLDER_GIT_2, "New worktree", worktree, |s: &mut App| set_env_mode(s, true)),
    ];
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
    let search = row()
        .w_full()
        .h(32.0)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .child(icon(&icons::SEARCH, 16.0).color(muted.alpha_mul(0.8)))
        .child(
            text_input(&ui.branch_query, theme::TEXT_SM)
                .key("details-branch-search")
                .grow(1.0)
                .min_w(0.0)
                .font(theme::TEXT_SM)
                .color(t.c.foreground)
                .placeholder("Search refs...", muted)
                .on_input_key(on_branch_key)
                .on_edit(|s: &mut App| &mut s.ui.chat.details.branch_query),
        );
    let mut list = list.child(search).child(el().w_full().h(1.0).my(4.0).fill(t.c.border));
    let status = |label: &str| row().h(32.0).px(8.0).center_y().child(text(label.to_string()).font(theme::TEXT_SM).color(muted));
    let Some(refs) = ui.refs.as_deref() else { return list.child(status("Loading refs...")) };
    let items = visible_items(s);
    if items.is_empty() {
        list = list.child(status("No refs found."));
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
            .radius(radius::MD)
            .cursor(Cursor::Pointer)
            .label(label.clone())
            .child(text(label).font(theme::TEXT_SM).color(t.c.foreground).single_line().truncate_middle().grow(1.0).shrink().min_w(0.0))
            .on_hover(move |s: &mut App| s.ui.chat.details.branch_active = i);
        if let Item::Ref(r) = &item {
            let name = refs[*r].name.clone();
            line = line.on_secondary_press(move |s: &mut App, at: rune::PointerPress, _: &mut Tx| {
                s.ui.chat.details.branch_copy = Some((name.clone(), (at.x, at.y)));
            });
        }
        let mut line = line.on_press(move |s: &mut App| pick_item(s, item.clone()));
        if let Some(subtitle) = subtitle {
            line = line.child(text(subtitle).font(theme::TEXT_XS).color(muted).single_line().truncate().shrink().min_w(0.0).max_w(160.0));
        }
        if let Some(badge) = badge {
            line = line.child(text(badge).font(theme::TEXT_3XS).color(muted.alpha_mul(0.45)));
        }
        if i == active {
            line = line.fill(t.c.accent);
        }
        rows = rows.child(line);
    }
    let (near_end, last) = (ui.near_end.clone(), ui.last_scroll.clone());
    let waker = s.ui.panel.waker.clone();
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
        list = list.child(row().px(8.0).py(6.0).child(text(footer).font(theme::TEXT_XS).color(muted)));
    }
    if selecting_base(s) {
        let on = draft_run(s).is_some_and(|r| r.from_origin);
        list = list.child(el().w_full().h(1.0).my(4.0).fill(t.c.border)).child(
            row()
                .w_full()
                .h(32.0)
                .px(8.0)
                .gap(8.0)
                .center_y()
                .child(icon(&icons::REFRESH_CW, 14.0).color(muted))
                .child(text("Start from remote").font(theme::TEXT_SM).color(t.c.foreground).grow(1.0))
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

/// `composer.branch` opens the branch picker and `composer.workspace` a draft's run context,
/// showing the card first when it is closed. Web binds both outside the terminal.
pub fn shortcut(s: &mut App, e: &KeyEvent) -> bool {
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
    let menu = if crate::panel::shortcut(s, e, "composer.branch") && vcs(s).is_some_and(|v| v["local"]["isRepo"] == true) {
        Menu::Branch
    } else if crate::panel::shortcut(s, e, "composer.workspace") && draft_run(s).is_some() {
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
    use super::{DraftRun, Item, Pending, Ref, Shown, branch_items, draft_run, git, lane, local_name, shown, switching, take_draft_workspace, toggle};
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
        s.ui.chat.details.pending = Some((2, Pending::Branch("feature".into(), None, "Failed to switch ref.")));
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
        *super::draft_run_mut(&mut s) = DraftRun { worktree: true, base: Some("feature/x".into()), from_origin: true };
        assert_eq!(take_draft_workspace(&mut s), Some(json!({"type": "worktree", "baseRef": "feature/x", "startFromOrigin": true})));
        assert_eq!(take_draft_workspace(&mut s), None, "taken once: the next draft starts over");
    }

    #[test]
    fn a_branch_made_by_commit_on_new_branch_becomes_the_drafts() {
        let mut s = App::new();
        let project = s.state().projects.iter().map(|p| data::s(p, "id").to_string()).next().unwrap();
        s.navigate(Route::Home { project_id: Some(project) });
        *super::draft_run_mut(&mut s) = DraftRun { worktree: true, base: Some("main".into()), from_origin: true };
        git::sync_branch(&mut s, &json!({"branch": {"status": "skipped_not_requested"}}));
        assert_eq!(draft_run(&s).unwrap().base.as_deref(), Some("main"));
        git::sync_branch(&mut s, &json!({"branch": {"status": "created", "name": "feature/fix"}}));
        let run = draft_run(&s).unwrap();
        assert_eq!((run.base.as_deref(), run.from_origin), (Some("feature/fix"), false));
        assert_eq!(take_draft_workspace(&mut s), Some(json!({"type": "worktree", "baseRef": "feature/fix"})));
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
    }
}
