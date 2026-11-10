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
pub mod scripts;

#[derive(Default)]
pub struct Ui {
    /// Threads (by draft key) whose inline card the user closed (`inlineOpen: false`).
    closed: HashSet<String>,
    /// The thread whose card is open as a popover, when the lane is too narrow for it inline.
    popover: Option<String>,
    pub menu: Option<Menu>,
    /// Refs from `vcs.listRefs`, loaded when the branch menu opens; `None` while loading.
    refs: Option<Vec<Ref>>,
    /// More refs than the first page: `(listed, total)` ("Showing N of M refs").
    refs_more: Option<(usize, u64)>,
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
}

enum Pending {
    List,
    /// A switch or create; its reply moves the thread onto the branch, in that worktree.
    Branch(String, Option<String>),
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
    let c = composer::ctx(s);
    let worktree = c.thread.map(|t| data::s(t, "worktreePath")).filter(|p| !p.is_empty());
    worktree.or_else(|| c.project.map(|p| data::s(p, "workspaceRoot"))).filter(|p| !p.is_empty()).map(str::to_string)
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
        ui.refs_more = None;
        if let (Some(cwd), Some(net)) = (cwd(s), s.src.net()) {
            let id = net.request("vcs.listRefs", json!({"cwd": cwd, "limit": REF_LIMIT}));
            s.ui.chat.details.pending = Some((id, Pending::List));
        }
    }
}

const REF_LIMIT: u64 = 100;

/// A branch picker row: a ref, or creating one named by the search.
#[derive(Clone, Debug, PartialEq)]
enum Item {
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
    let run = draft_run_mut(s);
    if run.worktree != worktree {
        run.worktree = worktree;
        run.base = None;
    }
}

fn pick_item(s: &mut App, item: Item) {
    s.ui.chat.details.menu = None;
    match item {
        Item::Create(name) => create_ref(s, &name),
        Item::Ref(i) => {
            if let Some(r) = s.ui.chat.details.refs.as_ref().and_then(|v| v.get(i)).cloned() {
                select_ref(s, &r);
            }
        }
    }
}

/// `selectBranch`: a worktree base for a worktree draft; a ref checked out in a worktree moves
/// the thread there; anything else is switched to in the thread's checkout.
fn select_ref(s: &mut App, r: &Ref) {
    if selecting_base(s) {
        draft_run_mut(s).base = Some(r.name.clone());
        return;
    }
    let c = composer::ctx(s);
    let project_cwd = c.project.map(|p| data::s(p, "workspaceRoot").to_string()).unwrap_or_default();
    let active = c.thread.map(|t| data::s(t, "worktreePath")).filter(|p| !p.is_empty()).map(str::to_string);
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
        s.ui.chat.details.pending = Some((id, Pending::Branch(name, next)));
    }
}

fn create_ref(s: &mut App, name: &str) {
    let Some(cwd) = cwd(s) else { return };
    let worktree = composer::ctx(s).thread.map(|t| data::s(t, "worktreePath")).filter(|p| !p.is_empty()).map(str::to_string);
    if let Some(net) = s.src.net() {
        let id = net.request("vcs.createRef", json!({"cwd": cwd, "refName": name, "switchRef": true}));
        s.ui.chat.details.pending = Some((id, Pending::Branch(name.to_string(), worktree)));
    }
}

fn set_thread_branch(s: &mut App, branch: &str, worktree: Option<String>) {
    let thread = composer::ctx(s).thread.map(|t| data::s(t, "id").to_string());
    if let (Some(thread), Some(net)) = (thread, s.src.net()) {
        net.dispatch(json!({"type": "thread.metadata.update", "threadId": thread, "branch": branch, "worktreePath": worktree}));
    }
}

fn visible_items(s: &App) -> Vec<Item> {
    let ui = &s.ui.chat.details;
    branch_items(ui.refs.as_deref().unwrap_or(&[]), &ui.branch_query.text, !selecting_base(s))
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
    let changed = git::poll(s) | scripts::poll(s);
    poll_branch(s) || changed
}

fn poll_branch(s: &mut App) -> bool {
    let Some(id) = s.ui.chat.details.pending.as_ref().map(|p| p.0) else { return false };
    let Some(reply) = s.src.net().and_then(|net| net.take_reply(id)) else { return false };
    let Some((_, pending)) = s.ui.chat.details.pending.take() else { return false };
    match (pending, reply) {
        (Pending::List, Ok(v)) => {
            let refs: Vec<Ref> = v["refs"].as_array().into_iter().flatten().map(Ref::from).collect();
            let ui = &mut s.ui.chat.details;
            ui.refs_more = (!v["nextCursor"].is_null()).then(|| (refs.len(), v["totalCount"].as_u64().unwrap_or(0)));
            ui.refs = Some(refs);
        }
        (Pending::Branch(name, worktree), Ok(v)) => {
            let branch = v["refName"].as_str().unwrap_or(&name).to_string();
            set_thread_branch(s, &branch, worktree);
            git::refresh(s);
        }
        (Pending::List, Err(e)) => {
            s.ui.chat.details.refs = Some(vec![]);
            palette::toast(s, "Failed to load branches.", Some(&palette::error_message(&e, "An error occurred.")));
        }
        (Pending::Branch(..), Err(e)) => palette::toast(s, "Failed to switch ref.", Some(&palette::error_message(&e, "An error occurred."))),
    }
    true
}

fn open_in_editor(s: &mut App, editor: Option<String>) {
    let (Some(cwd), editor) = (cwd(s), editor.or_else(|| editors(s).first().map(|e| e.0.clone()))) else { return };
    let Some(editor) = editor else { return };
    if let Some(net) = s.src.net() {
        net.request("shell.openInEditor", json!({"cwd": cwd, "editor": editor}));
    }
}

/// `(id, label)` for each editor the server found.
fn editors(s: &App) -> Vec<(String, String)> {
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
    let worktree = thread.is_some_and(|t| !data::s(t, "worktreePath").is_empty());
    let mode = if new_worktree { "New worktree" } else if worktree { "Worktree" } else if thread.is_some() { "Local checkout" } else { "Current checkout" };
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
        .child(icon(if new_worktree { &icons::FOLDER_GIT_2 } else if worktree { &icons::GIT_FORK } else { &icons::FOLDER }, 16.0).color(muted).ml(1.0).help(mode));
    if draft {
        run_context = run_context
            .radius(theme::layout::CONTROL_RADIUS)
            .hover(t.c.accent)
            .cursor(Cursor::Pointer)
            .role("button")
            .expanded(s.ui.chat.details.menu == Some(Menu::RunContext))
            .child(icon(&icons::CHEVRON_DOWN, 16.0).color(muted).ml(9.0))
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
                    .expanded(s.ui.chat.details.menu == Some(Menu::Branch))
                    .on_press(|s: &mut App| toggle_menu(s, Menu::Branch)),
            );
        // The Changes row hides with the quiet git row; a draft shows it disabled.
        let Some(git) = git::view(s, t, v, draft) else { return Some(card.child(section)) };
        section = section.child(git);
        let (add, del) = git::changes_totals(v);
        let stat = theme::mono(theme::XS_MEDIUM);
        let changes = action(t, "details-changes", icon(&icons::FILE_DIFF, 16.0).color(muted), "Changes", fg)
            .w_full()
            .child(
                row()
                    .gap(6.0)
                    .child(text(format!("+{add}")).font(stat).color(if add > 0 { t.c.diff_addition_foreground } else { muted }))
                    .child(text(format!("-{del}")).font(stat).color(if del > 0 { t.c.diff_deletion_foreground } else { muted })),
            );
        section = section.child(if draft { changes.opacity(0.64) } else { changes.on_press(|s: &mut App| crate::panel::open_changes(s)) });
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
    vec![
        group_label(t, "Run on"),
        radio_row(t, &icons::SERVER, &env_label(s), true, |s: &mut App| s.ui.chat.details.menu = None),
        el().h(1.0).mx(8.0).my(4.0).fill(t.c.border),
        group_label(t, "Workspace"),
        radio_row(t, &icons::FOLDER, "Current checkout", !worktree, |s: &mut App| set_env_mode(s, false)),
        radio_row(t, &icons::FOLDER_GIT_2, "New worktree", worktree, |s: &mut App| set_env_mode(s, true)),
    ]
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
            Item::Ref(r) => (refs[*r].name.clone(), refs[*r].badge(&project_cwd)),
            Item::Create(name) => (format!("Create new ref \"{name}\""), None),
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
        if let Some(badge) = badge {
            line = line.child(text(badge).font(theme::TEXT_3XS).color(muted.alpha_mul(0.45)));
        }
        if i == active {
            line = line.fill(t.c.accent);
        }
        rows = rows.child(line);
    }
    list = list.child(rows.max_h(224.0).scroll(rune::ScrollSpec { stick_bottom: false }));
    if let Some((shown, total)) = ui.refs_more {
        list = list.child(row().px(8.0).py(6.0).child(text(format!("Showing {shown} of {total} refs")).font(theme::TEXT_XS).color(muted)));
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
    use super::{DraftRun, Item, Ref, Shown, branch_items, draft_run, git, lane, local_name, shown, take_draft_workspace, toggle};
    use serde_json::json;
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
