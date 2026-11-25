//! Side chats (useSideChat, SideChatPane): `/side` or `/btw` forks the open thread's latest
//! stable point into a hidden child thread, sends the prompt there, and shows that thread's
//! timeline and the composer, writing into it, in a pane beside the chat. A closed side chat
//! comes back through `view.reopenClosed` or by opening it from its parent (openSideChatTab);
//! Bring back merges its latest reply into the parent's next message.

use std::cmp::Ordering;
use std::collections::HashSet;

use rune::{Color, Cursor, Cx, El, KeyEvent, Tx, col, el, icon, row, text};
use serde_json::{Value, json};

use crate::chat::details::Lane;
use crate::data;
use crate::icons;
use crate::kit::button::{Button, icon_button};
use crate::theme::{self, AlphaMul, T};
use crate::{App, Route};

/// The right panel's default width.
const WIDTH: f32 = 540.0;
/// waitForThreadShell's timeout.
const SHELL_WAIT_MS: i64 = 5_000;
/// ThreadLineageGroup's first rows, and how many each "Show N more" adds.
const LIST_FIRST: usize = 6;
const LIST_PAGE: usize = 12;

#[derive(Default)]
pub struct State {
    pending: Option<Pending>,
    requests: Vec<(u64, Request)>,
    /// The thread whose details card lists more side chats than the first page, and how many.
    listed: Option<(String, usize)>,
}

/// A side chat being started from `parent`: the draft it came from and the text sent, so a
/// started side chat clears only a draft nobody changed meanwhile.
struct Pending {
    parent: String,
    target: String,
    prompt: String,
    draft: (String, String),
    stage: Stage,
}

#[derive(Clone, Copy)]
enum Stage {
    Fork(u64),
    /// Waiting for the forked thread's shell to reach this client, until the deadline.
    Shell(i64),
    Turn(u64),
}

#[derive(Clone)]
enum Request {
    Delete(String),
    BringBack(String),
}

/// parseSideChatCommand: `/side` or `/btw`, alone or before the prompt it carries.
pub fn parse(text: &str) -> Option<&str> {
    let text = text.trim();
    let (head, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    (head.eq_ignore_ascii_case("/side") || head.eq_ignore_ascii_case("/btw")).then(|| rest.trim())
}


fn thread<'a>(s: &'a App, id: &str) -> Option<&'a Value> {
    s.state().threads.iter().find(|t| data::s(t, "id") == id)
}

/// The thread the composer in use writes into: the route's, or a side chat's own.
fn route_thread(s: &App) -> Option<&Value> {
    match crate::chat::composer::route(s) {
        Route::Thread(id) => thread(s, &id),
        _ => None,
    }
}

/// The side chat open beside the routed thread.
pub fn open_beside(s: &App) -> Option<&str> {
    crate::panel::active_side_chat(s)
}

/// Why this route offers no side chats (ChatView's `sideChatsOffered` and `isServerThread`).
fn unavailable(s: &App) -> Option<&'static str> {
    let thread = route_thread(s);
    let lineage = thread.map(|t| &t["lineage"]);
    if lineage.is_some_and(|l| l["sideChat"] == true) {
        return Some("Side chats cannot start side chats.");
    }
    if thread.is_some_and(|t| t["lineage"]["relationshipToParent"] == "subagent" || !t["delegatedFrom"].is_null()) {
        return Some("Subagent threads cannot start side chats.");
    }
    let Some(config) = s.state().config.as_ref() else {
        return Some("Side chats are not ready yet. Try again in a moment.");
    };
    if config["environment"]["capabilities"]["sideChats"] != true {
        return Some("Update this environment's T3 Code server to use side chats.");
    }
    if thread.is_none() {
        return Some("Side chats start after the first message.");
    }
    None
}

/// Whether `/side` and `/btw` show in the command menu (`sideChat.available`).
pub fn available(s: &App) -> bool {
    unavailable(s).is_none()
}

/// ChatView's `sideChatDisabledReason`.
pub fn disabled_reason(s: &App) -> Option<&'static str> {
    unavailable(s).or_else(|| worktree_pending(s).then_some("Available once the thread's worktree is set up."))
}

/// orchestrationV2WorktreePending: no worktree yet, while a run is (or failed) preparing one.
fn worktree_pending(s: &App) -> bool {
    let Some(t) = route_thread(s) else { return false };
    let runs = s.state().thread_views.get(data::s(t, "id")).and_then(|v| v.projection["runs"].as_array());
    data::s(t, "worktreePath").is_empty()
        && runs.into_iter().flatten().any(|r| {
            matches!(data::s(r, "status"), "preparing" | "failed" | "interrupted") && r["workspacePreparation"]["type"] == "worktree"
        })
}

/// useComposerSend's side chat branch for a composer send of `text` (already `parse`d as a side
/// chat command); `attached` says the draft carries files or context, which side chats refuse.
pub fn send_command(s: &mut App, text: &str, attached: bool) {
    let Some(prompt) = parse(text) else { return };
    if let Some(reason) = disabled_reason(s) {
        crate::shell::palette::toast(s, "Side chat not started", Some(reason));
        return;
    }
    if attached {
        crate::shell::palette::toast(s, "Side chats take text only", Some("Remove the attachments and context, then send it again."));
        return;
    }
    let Route::Thread(parent) = crate::chat::composer::route(s) else { return };
    // Another side chat still starting takes this send as not started.
    if s.ui.chat.side.pending.is_some() {
        return;
    }
    let key = crate::chat::composer::draft_key(s, &Route::Thread(parent.clone()));
    let sent = s.ui.chat.composer.drafts.get(&key).map(|d| d.edit.text.clone()).unwrap_or_default();
    let target = s5_net::uuid();
    let Some(net) = s.src.net() else { return };
    let rpc = net.dispatch(json!({
        "type": "thread.fork", "commandId": s5_net::uuid(), "createdBy": "user", "creationSource": "web",
        "sourceThreadId": parent, "targetThreadId": target, "sourcePoint": {"type": "latest_stable"},
        "sideChat": true, "title": "Side chat",
    }));
    s.ui.chat.side.pending = Some(Pending { parent, target, prompt: prompt.to_owned(), draft: (key, sent), stage: Stage::Fork(rpc) });
}

/// Moves a starting side chat along (fork, its shell arriving, the first turn) and takes the
/// replies to side chat sends and deletes.
pub fn poll(s: &mut App) -> bool {
    let mut changed = poll_requests(s);
    let Some(mut p) = s.ui.chat.side.pending.take() else { return changed };
    let reply = match p.stage {
        Stage::Fork(rpc) | Stage::Turn(rpc) => s.src.net().and_then(|net| net.take_reply(rpc)),
        Stage::Shell(_) => None,
    };
    match (p.stage, reply) {
        (Stage::Fork(_), Some(Err(cause))) => return failed(s, &p, &cause, "Could not start a side chat.", false),
        (Stage::Fork(_), Some(Ok(_))) => {
            p.stage = Stage::Shell(data::now_ms() + SHELL_WAIT_MS);
            changed = true;
        }
        (Stage::Turn(_), Some(Err(cause))) => return failed(s, &p, &cause, "Could not send the side chat message.", true),
        (Stage::Turn(_), Some(Ok(_))) => {
            opened(s, p);
            return true;
        }
        _ => {}
    }
    if let Stage::Shell(deadline) = p.stage {
        if thread(s, &p.target).is_some() {
            if p.prompt.is_empty() {
                opened(s, p);
                return true;
            }
            let Some(net) = s.src.net() else { return changed };
            // The server asks before edits on a side chat's first turn whatever mode is sent.
            let rpc = net.dispatch(json!({
                "type": "message.dispatch", "commandId": s5_net::uuid(), "createdBy": "user", "creationSource": "web",
                "threadId": p.target, "messageId": s5_net::uuid(), "text": p.prompt, "attachments": [],
                "titleSeed": crate::chat::thread_title::seed(&p.prompt, &[], &[]), "deliveryIntent": "auto", "dispatchMode": {"type": "start_immediately"},
            }));
            p.stage = Stage::Turn(rpc);
            changed = true;
        } else if data::now_ms() > deadline {
            report(s, &p, "The side chat was created, but it did not reach this client.", true);
            return true;
        }
    }
    s.ui.chat.side.pending = Some(p);
    changed
}

fn failed(s: &mut App, p: &Pending, cause: &Value, fallback: &str, discard: bool) -> bool {
    report(s, p, &message(cause, fallback), discard);
    true
}

fn message(cause: &Value, fallback: &str) -> String {
    let error = crate::chat::timeline::rpc_failure(cause);
    error.as_str().or(error["message"].as_str()).filter(|m| !m.trim().is_empty()).unwrap_or(fallback).to_owned()
}

/// Shows a failed start on the parent; `discard` drops the empty side chat it left behind.
fn report(s: &mut App, p: &Pending, message: &str, discard: bool) {
    if discard && let Some(net) = s.src.net() {
        net.dispatch(json!({"type": "thread.delete", "commandId": s5_net::uuid(), "threadId": p.target, "onlyIfUnused": true}));
    }
    crate::chat::timeline::error_banner::set_local(s, &p.draft.0, Some(message));
}

fn opened(s: &mut App, p: Pending) {
    if let Some(d) = s.ui.chat.composer.drafts.get_mut(&p.draft.0)
        && d.edit.text == p.draft.1
    {
        d.edit.clear();
    }
    show(s, &p.parent, &p.target);
}

/// Opens `side` beside `parent` (rightPanelStore's openSideChat) with the caret in its composer.
fn show(s: &mut App, parent: &str, side: &str) {
    crate::panel::open_side_chat(s, parent, side);
}

/// `view.reopenClosed` bringing back side chat `side` beside `parent`, which is open by now.
pub fn reopen(s: &mut App, parent: &str, side: &str) {
    show(s, parent, side);
}

/// openSideChatTab: a side chat of the routed thread opens beside it; false for any other thread.
pub fn open_tab(s: &mut App, id: &str) -> bool {
    let Route::Thread(parent) = s.frame_route().clone() else { return false };
    let Some(t) = thread(s, id) else { return false };
    if t["lineage"]["sideChat"] != true || data::s(&t["lineage"], "parentThreadId") != parent {
        return false;
    }
    show(s, &parent, id);
    true
}

/// Moves the caret into the side chat's composer, or back to the chat's (`to_side` false).
pub(crate) fn focus(s: &mut App, side: &str, to_side: bool) {
    let main = crate::chat::composer::draft_key(s, &s.frame_route().clone());
    for (key, on) in [(main, !to_side), (side.to_owned(), to_side)] {
        s.ui.chat.composer.drafts.entry(key).or_default().edit.focused = on;
    }
}

fn poll_requests(s: &mut App) -> bool {
    if s.ui.chat.side.requests.is_empty() {
        return false;
    }
    let Some(net) = s.src.net() else { return false };
    let mut done = Vec::new();
    s.ui.chat.side.requests.retain(|(id, request)| match net.take_reply(*id) {
        Some(reply) => {
            done.push((request.clone(), reply));
            false
        }
        None => true,
    });
    for (request, reply) in &done {
        let detail = reply.as_ref().err().map(|cause| message(cause, "")).filter(|m| !m.is_empty());
        match (request, reply) {
            (Request::Delete(_), Err(_)) => crate::shell::palette::toast_error(s, "Could not delete the side chat", detail.as_deref()),
            (Request::Delete(side), Ok(_)) => crate::panel::close_side_chat(s, side, false),
            (Request::BringBack(_), Err(_)) => crate::shell::palette::toast_error(s, "Could not bring back the side chat", detail.as_deref()),
            (Request::BringBack(_), Ok(_)) => {
                crate::shell::palette::toast(s, "Brought back", Some("The main thread gets this side chat with your next message there."))
            }
        }
    }
    !done.is_empty()
}

/// Closes the pane (useRightPanelSurfaceClose): reopenable through `view.reopenClosed`, and a side
/// chat that never ran is deleted (the server keeps one with a run or message).
fn close(s: &mut App, side: &str) {
    crate::panel::close_side_chat(s, side, true);
}

pub(crate) fn close_tab(s: &mut App, side: &str) {
    // A loaded shell without the side chat means it is already deleted.
    let gone = s.state().shell_ready && thread(s, side).is_none();
    if !gone
        && can_operate(s)
        && let Some(net) = s.src.net()
    {
        net.dispatch(json!({"type": "thread.delete", "commandId": s5_net::uuid(), "threadId": side, "onlyIfUnused": true}));
    }
    if open_beside(s) == Some(side) { focus(s, side, false); }
}

use super::can_operate;

fn confirm_delete(s: &App) -> bool {
    let key = "confirmThreadDelete";
    s.client_settings.values.get(key).or_else(|| crate::settings::client::defaults().get(key)).and_then(Value::as_bool).unwrap_or(false)
}

/// `deleteSideChat`: web's destructive `dialogs.confirm` while `confirmThreadDelete` is on.
fn request_delete(s: &mut App, side: &str) {
    if !confirm_delete(s) {
        return delete(s, side);
    }
    let title = thread(s, side).map(|r| data::s(r, "title")).filter(|r| !r.is_empty()).unwrap_or("Side chat");
    let question = format!("Delete side chat \"{title}\"?\nThis permanently clears its conversation history.");
    let side = side.to_owned();
    crate::shell::confirm::request(s, question, true, move |s: &mut App| delete(s, &side));
}

fn delete(s: &mut App, side: &str) {
    let Some(net) = s.src.net() else { return };
    let rpc = net.dispatch(json!({"type": "thread.delete", "commandId": s5_net::uuid(), "threadId": side}));
    s.ui.chat.side.requests.push((rpc, Request::Delete(side.to_owned())));
}

/// resolveLatestMergeBackRun: the newest run that finished its reply (still `waiting` while its
/// checkpoint is captured), unless a newer one is under way.
pub(crate) fn merge_back_run(projection: &Value) -> Option<&str> {
    let runs = projection["runs"].as_array()?;
    let ordinal = |r: &Value| r["ordinal"].as_i64().unwrap_or(0);
    let latest = runs.iter().filter(|r| matches!(data::s(r, "status"), "waiting" | "completed")).max_by_key(|r| ordinal(r))?;
    let newer = runs.iter().any(|r| ordinal(r) > ordinal(latest) && matches!(data::s(r, "status"), "preparing" | "starting" | "running"));
    (!newer).then(|| data::s(latest, "id"))
}

/// Why Bring back waits (SideChatPane's bringBackBlockedReason): a pending merge-back would fail
/// the main thread's queued message, so the server refuses it while that runs.
fn bring_back_blocked(s: &App, parent: &str, side: &str) -> Option<&'static str> {
    if !can_operate(s) {
        return Some("This connection cannot change threads.");
    }
    if thread(s, parent).is_some_and(|p| crate::shell::sidebar::logic::Thread::from_shell(p).running()) {
        return Some("Available when the main thread is idle.");
    }
    let run = s.state().thread_views.get(side).and_then(|v| merge_back_run(&v.projection));
    run.is_none().then_some("Available after a finished reply.")
}

fn bringing_back(s: &App, side: &str) -> bool {
    s.ui.chat.side.requests.iter().any(|(_, r)| matches!(r, Request::BringBack(id) if id == side))
}

/// Bring back: the side chat's latest reply rides along with the main thread's next message.
fn bring_back(s: &mut App, parent: &str, side: &str) {
    if bring_back_blocked(s, parent, side).is_some() || bringing_back(s, side) {
        return;
    }
    let Some(run) = s.state().thread_views.get(side).and_then(|v| merge_back_run(&v.projection)).map(str::to_owned) else { return };
    let Some(net) = s.src.net() else { return };
    let rpc = net.dispatch(json!({
        "type": "thread.merge_back", "commandId": s5_net::uuid(), "createdBy": "user", "creationSource": "web",
        "sourceThreadId": side, "targetThreadId": parent, "sourcePoint": {"type": "run", "runId": run},
    }));
    s.ui.chat.side.requests.push((rpc, Request::BringBack(side.to_owned())));
}

pub(crate) fn panel(s: &App, cx: &mut Cx<App>, t: &T, side: &str) -> El {
    let Route::Thread(parent) = s.frame_route() else { return col() };
    pane(s, cx, t, parent, side)
}

/// SideChatPane: the side chat's title, Bring back, Delete and Close over its timeline and the
/// composer writing into it.
fn pane(s: &App, cx: &mut Cx<App>, t: &T, parent: &str, side: &str) -> El {
    let shell = thread(s, side);
    let title = shell.map(|r| data::s(r, "title")).filter(|r| !r.is_empty()).unwrap_or("Side chat");
    let (close_id, delete_id, back_id, back_parent, key_id) = (side.to_owned(), side.to_owned(), side.to_owned(), parent.to_owned(), side.to_owned());
    let blocked = bring_back_blocked(s, parent, side);
    let mut back = Button::new("Bring back").variant("ghost").size("xs").build(t).help(blocked.unwrap_or("Adds this side chat to the main thread's next message."));
    back = if blocked.is_some() || bringing_back(s, side) { back.opacity(0.64) } else { back.on_press(move |s: &mut App| bring_back(s, &back_parent, &back_id)) };
    let deletable = shell.is_some() && can_operate(s);
    let mut delete_button = icon_button(&icons::TRASH_2, "Delete side chat").size("icon-xs").disabled(!deletable);
    if deletable {
        delete_button = delete_button.on_press(move |s: &mut App| request_delete(s, &delete_id));
    }
    let header = row()
        .h(36.0)
        .w_full()
        .pl(12.0)
        .pr(4.0)
        .gap(4.0)
        .center_y()
        .child(text(title.to_owned()).font(theme::XS_MEDIUM).color(t.c.foreground).truncate().grow(1.0).min_w(0.0))
        .child(back)
        .child(delete_button.build(t))
        .child(icon_button(&icons::X, "Close side chat").size("icon-xs").on_press(move |s: &mut App| close(s, &close_id)).build(t));
    let mut content = col().grow(1.0).min_w(0.0).h_full().child(header).child(el().h(1.0).w_full().fill(t.c.border));
    content = match shell {
        None if s.state().shell_ready => {
            content.child(text("This side chat was deleted.").font(theme::TEXT_XS).color(t.c.muted_foreground).padding(12.0))
        }
        None => content,
        Some(_) => {
            let w = cx.layout_with("side-chat-body", |r| r[2].round()).unwrap_or(WIDTH);
            let lane = Lane { left: 12.0, width: (w - 24.0).max(0.0), card: false };
            content.child(
                col()
                    .key("side-chat-body")
                    .grow(1.0)
                    .min_h(0.0)
                    .w_full()
                    .child(crate::chat::timeline::view(s, cx, t, side, &lane))
                    .children(crate::chat::timeline::side_scroll_to_end(s, cx, t, &lane, side))
                    .child(crate::chat::composer::view_for(s, cx, t, &lane, side))
                    .children(crate::chat::composer::overlays_for(s, cx, t, side)),
            )
        }
    };
    row()
        .key("side-chat-pane")
        .w_full()
        .min_w(0.0)
        .h_full()
        .fill(t.c.background)
        // Composer chords pressed in the pane are its composer's (useChatShortcuts' embedded mode),
        // and digits pick its own pending question's options.
        .on_key(move |s: &mut App, e: &KeyEvent, tx: &mut Tx| {
            crate::chat::composer::side_shortcut(s, &key_id, e)
                || crate::chat::composer::acting(s, Some(key_id.clone()), |s| crate::chat::timeline::question::on_key(s, e, tx))
        })
        .child(el().w(1.0).h_full().fill(t.c.border))
        .child(content)
}

/// The thread a side chat forked from: a run fork's thread, else its lineage parent.
pub(crate) fn parent_of(t: &Value) -> &str {
    if t["forkedFrom"]["type"] == "run" { data::s(&t["forkedFrom"], "threadId") } else { data::s(&t["lineage"], "parentThreadId") }
}

/// ThreadRelationshipsPanel's `sideChats` group: `parent`'s side chats, live shells ahead of
/// archived copies, newest first (orderWebThreadLineageRows).
fn side_chats<'a>(s: &'a App, parent: &str) -> Vec<&'a Value> {
    let state = s.state();
    let mut seen = HashSet::new();
    let mut rows: Vec<&Value> = state
        .threads
        .iter()
        .chain(&state.archived)
        .filter(|&t| t["lineage"]["sideChat"] == true && parent_of(t) == parent && seen.insert(data::s(t, "id")))
        .collect();
    rows.sort_by(|a, b| match (data::ts(a, "createdAt"), data::ts(b, "createdAt")) {
        (Some(x), Some(y)) if x != y => y.cmp(&x),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        _ => data::s(a, "id").cmp(data::s(b, "id")),
    });
    rows
}

/// threadRelationshipRowStatus for a side chat: its run's activity, else its own status.
fn row_status(t: &Value) -> &str {
    t["activityRunStatus"].as_str().or_else(|| t["status"].as_str()).unwrap_or("")
}

/// threadRelationshipStatusLabel.
pub(crate) fn status_label(status: &str) -> &'static str {
    match status {
        "preparing" | "starting" => "Starting",
        "running" | "in_progress" => "Running",
        "pending" | "queued" => "Queued",
        "waiting" | "blocked" => "Waiting",
        "completed" => "Done",
        "failed" | "error" => "Failed",
        "cancelled" | "interrupted" => "Stopped",
        "rolled_back" => "Reverted",
        "resolved_native" => "Resolved (native)",
        "resolved_portable" => "Resolved (portable)",
        "consumed" => "Consumed",
        "superseded" => "Superseded",
        "idle" => "Idle",
        _ => "Unknown",
    }
}

/// ThreadRelationshipIcon's status dot.
pub(crate) fn status_dot(t: &T, status: &str) -> Color {
    match status {
        "running" | "in_progress" | "pending" | "waiting" => t.c.info,
        "failed" | "error" => t.c.destructive,
        "completed" => t.c.success,
        _ => t.c.muted_foreground.alpha_mul(0.45),
    }
}

/// resolveThreadLineageWindow: how many rows show, and the "Show N more" under them.
fn window(total: usize, shown: usize) -> (usize, Option<String>) {
    let visible = total.min(shown);
    let hidden = total - visible;
    (visible, (hidden > 0).then(|| format!("Show {} more", hidden.min(LIST_PAGE))))
}

/// The details card's "Side chats" section (ThreadRelationshipsPanel): `parent`'s side chats,
/// each opening beside it. Hidden while it has none.
pub fn details_list(s: &App, t: &T, parent: &str, divided: bool) -> Option<El> {
    let chats = side_chats(s, parent);
    if chats.is_empty() {
        return None;
    }
    let muted = t.c.muted_foreground;
    let running = chats.iter().filter(|c| row_status(c) == "running").count();
    let shown = s.ui.chat.side.listed.as_ref().filter(|(id, _)| id == parent).map_or(LIST_FIRST, |(_, n)| *n);
    let (visible, more) = window(chats.len(), shown);
    let mut rows = col().key("details-side-chats").w_full();
    for chat in &chats[..visible] {
        let id = data::s(chat, "id").to_owned();
        let title = Some(data::s(chat, "title")).filter(|t| !t.is_empty()).unwrap_or(&id).to_owned();
        let status = row_status(chat);
        let failed = matches!(status, "failed" | "error");
        let mark = el()
            .size(16.0, 16.0)
            .center()
            .child(icon(&icons::MESSAGES_SQUARE, 16.0).color(muted))
            .child(el().absolute(12.0, 12.0).size(8.0, 8.0).radius(4.0).fill(status_dot(t, status)).border(2.0, t.c.card));
        let line = row()
            .key(format!("details-side-chat-{id}"))
            .w_full()
            .h(32.0)
            .pl(11.0)
            .pr(10.0)
            .gap(10.0)
            .center_y()
            .radius(theme::layout::CONTROL_RADIUS)
            .hover(t.c.accent)
            .hover_group()
            .cursor(Cursor::Pointer)
            .role("button")
            .label(format!("{title} {}", status_label(status)))
            .help(format!("{title}\nOpen side chat in this chat"))
            .child(mark)
            .child(text(title).font(theme::SM_MEDIUM.lh(16.0)).color(t.c.foreground.alpha_mul(0.85)).truncate().grow(1.0).shrink().min_w(0.0))
            .child(icon(&icons::ARROW_RIGHT, 12.0).color(muted).group_opacity(0.0, 1.0))
            .child(text(status_label(status)).font(theme::TEXT_2XS).color(if failed { t.c.destructive } else { muted }))
            .on_press(move |s: &mut App| crate::shell::sidebar::open_thread(s, &id));
        rows = rows.child(line);
    }
    let mut body = col().w_full().child(rows.max_h(216.0).scroll(rune::ScrollSpec { stick_bottom: false }));
    if let Some(label) = more {
        let (parent, next) = (parent.to_owned(), shown + LIST_PAGE);
        let faint = muted.alpha_mul(0.7);
        body = body.child(
            row()
                .key("details-side-chats-more")
                .w_full()
                .h(32.0)
                .pl(11.0)
                .gap(10.0)
                .center_y()
                .radius(theme::layout::CONTROL_RADIUS)
                .hover(t.c.accent)
                .cursor(Cursor::Pointer)
                .role("button")
                .label(label.clone())
                .child(icon(&icons::PLUS, 16.0).color(faint))
                .child(text(label).font(theme::SM_MEDIUM).color(faint))
                .on_press(move |s: &mut App| s.ui.chat.side.listed = Some((parent.clone(), next))),
        );
    }
    let title = if running > 0 { format!("Side chats · {running} running") } else { "Side chats".to_owned() };
    Some(crate::chat::details::section(t, &title, None, divided, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_and_btw_parse_alone_or_before_a_prompt() {
        assert_eq!(parse("/side what does this do?"), Some("what does this do?"));
        assert_eq!(parse("  /BTW\n  why  "), Some("why"));
        assert_eq!(parse("/side"), Some(""));
        assert_eq!(parse("/sidebar"), None);
        assert_eq!(parse("ask /side later"), None);
    }

    #[test]
    fn side_chats_need_a_capable_server_and_a_top_level_thread() {
        let mut s = App::new();
        let capable = |s: &mut App, on: bool| {
            if let data::Source::Fixture(state) = &mut s.src {
                state.config.get_or_insert_with(|| json!({}))["environment"]["capabilities"]["sideChats"] = json!(on);
            }
        };
        capable(&mut s, false);
        assert_eq!(disabled_reason(&s), Some("Update this environment's T3 Code server to use side chats."));
        capable(&mut s, true);
        assert_eq!(disabled_reason(&s), Some("Side chats start after the first message."));
        let id = s.state().threads.iter().find(|t| t["lineage"]["relationshipToParent"] != "subagent" && t["lineage"]["sideChat"] != true && t["delegatedFrom"].is_null()).map(|t| data::s(t, "id").to_owned()).unwrap();
        s.navigate(Route::Thread(id.clone()));
        assert!(available(&s));
        if let data::Source::Fixture(state) = &mut s.src
            && let Some(t) = state.threads.iter_mut().find(|t| data::s(t, "id") == id)
        {
            t["lineage"]["sideChat"] = json!(true);
        }
        assert_eq!(disabled_reason(&s), Some("Side chats cannot start side chats."));
    }

    #[test]
    fn bring_back_takes_the_newest_finished_reply_unless_a_newer_run_is_under_way() {
        let runs = |r: Value| json!({ "runs": r });
        let done = runs(json!([{"id": "a", "ordinal": 1, "status": "completed"}, {"id": "b", "ordinal": 2, "status": "waiting"}, {"id": "c", "ordinal": 3, "status": "failed"}]));
        assert_eq!(merge_back_run(&done), Some("b"));
        let busy = runs(json!([{"id": "a", "ordinal": 1, "status": "completed"}, {"id": "b", "ordinal": 2, "status": "running"}]));
        assert_eq!(merge_back_run(&busy), None);
        assert_eq!(merge_back_run(&runs(json!([]))), None);
    }

    /// The first top-level thread, routed, with side chat `side-1` of it and `side-2` of another.
    fn with_side_chats() -> (App, String) {
        let mut s = App::new();
        let parent = s.state().threads.iter().find(|t| t["lineage"]["sideChat"] != true && t["delegatedFrom"].is_null()).map(|t| data::s(t, "id").to_owned()).unwrap();
        s.navigate(Route::Thread(parent.clone()));
        if let data::Source::Fixture(state) = &mut s.src {
            if let Some(t) = state.threads.iter_mut().find(|t| data::s(t, "id") == parent) {
                for idle in ["latestRunId", "activeProviderThreadId", "activityRunStatus", "pendingBackgroundTasks"] {
                    t[idle] = Value::Null;
                }
                t["status"] = json!("idle");
            }
            state.threads.push(json!({"id": "side-1", "title": "Side", "status": "idle", "lineage": {"parentThreadId": parent, "sideChat": true}}));
            state.threads.push(json!({"id": "side-2", "title": "Elsewhere", "status": "idle", "lineage": {"parentThreadId": "someone-else", "sideChat": true}}));
        }
        (s, parent)
    }

    #[test]
    fn a_closed_side_chat_opens_again_from_its_parent_with_the_caret() {
        let (mut s, parent) = with_side_chats();
        assert!(!open_tab(&mut s, &parent), "not a side chat");
        assert!(!open_tab(&mut s, "side-2"), "another thread's side chat navigates as usual");
        assert!(open_tab(&mut s, "side-1"));
        assert_eq!(open_beside(&s), Some("side-1"));
        assert!(s.ui.chat.composer.drafts["side-1"].edit.focused);
        assert!(!s.ui.chat.composer.drafts[&parent].edit.focused, "one composer holds the caret");
        close(&mut s, "side-1");
        assert_eq!(open_beside(&s), None);
        assert!(s.ui.chat.composer.drafts[&parent].edit.focused);
        assert!(!s.ui.chat.composer.drafts["side-1"].edit.focused);
        reopen(&mut s, &parent, "side-1");
        assert_eq!(open_beside(&s), Some("side-1"));
    }

    #[test]
    fn bring_back_waits_for_a_finished_reply_and_an_idle_parent() {
        let (mut s, parent) = with_side_chats();
        assert_eq!(bring_back_blocked(&s, &parent, "side-1"), Some("Available after a finished reply."));
        if let data::Source::Fixture(state) = &mut s.src {
            let projection = json!({"runs": [{"id": "r1", "ordinal": 1, "status": "completed"}]});
            state.thread_views.insert("side-1".into(), s5_net::ThreadView { projection, synchronized: true, sequence: 0, ..Default::default() });
        }
        assert_eq!(bring_back_blocked(&s, &parent, "side-1"), None);
        if let data::Source::Fixture(state) = &mut s.src
            && let Some(t) = state.threads.iter_mut().find(|t| data::s(t, "id") == parent)
        {
            t["status"] = json!("running");
            t["latestRunId"] = json!("parent-run");
        }
        assert_eq!(bring_back_blocked(&s, &parent, "side-1"), Some("Available when the main thread is idle."));
    }

    #[test]
    fn the_details_card_lists_a_threads_side_chats_newest_first() {
        let (mut s, parent) = with_side_chats();
        if let data::Source::Fixture(state) = &mut s.src {
            let side = |id: &str, title: &str, status: &str, created: Value| {
                json!({"id": id, "title": title, "status": status, "createdAt": created, "lineage": {"parentThreadId": parent, "sideChat": true}})
            };
            state.threads.push(side("side-3", "Newer", "running", json!("2026-11-02T00:00:00Z")));
            state.threads.push(json!({"id": "fork-1", "title": "Fork", "lineage": {"parentThreadId": parent}}));
            state.archived.push(side("side-4", "Archived", "idle", json!("2026-11-01T00:00:00Z")));
            state.archived.push(side("side-3", "Stale copy", "idle", Value::Null));
        }
        let chats = side_chats(&s, &parent);
        let ids: Vec<&str> = chats.iter().map(|t| data::s(t, "id")).collect();
        assert_eq!(ids, ["side-3", "side-4", "side-1"], "undated last; a live shell wins over its archived copy");
        assert_eq!(status_label(row_status(chats[0])), "Running");
        assert_eq!(window(8, LIST_FIRST), (6, Some("Show 2 more".into())));
        assert_eq!(window(30, LIST_FIRST), (6, Some("Show 12 more".into())));
        assert_eq!(window(8, LIST_FIRST + LIST_PAGE), (8, None));
    }

    #[test]
    fn a_side_chats_question_is_answered_from_its_own_composer() {
        use crate::chat::{composer, timeline::question};
        let (mut s, parent) = with_side_chats();
        reopen(&mut s, &parent, "side-1");
        let ask = |id: &str| json!({"id": id, "header": "Pick", "question": "Which?", "options": [{"label": "A"}, {"label": "B"}]});
        let projection = json!({
            "runtimeRequests": [{"id": "ask", "kind": "user_input", "status": "pending", "responseCapability": {"type": "live"}}],
            "turnItems": [{"id": "item", "type": "user_input_request", "requestId": "ask", "questions": [ask("q1"), ask("q2")]}],
        });
        if let data::Source::Fixture(state) = &mut s.src {
            state.thread_views.insert("side-1".into(), s5_net::ThreadView { projection, synchronized: true, sequence: 0, ..Default::default() });
        }
        assert_ne!(question::active(&s).map(|a| a.thread), Some("side-1".to_owned()), "the chat's composer leaves it alone");
        assert_eq!(composer::within(&s, Some("side-1".into()), || question::active(&s).map(|a| a.thread)), Some("side-1".to_owned()));
        // A picked single choice moves the side chat's question on, though the chat is routed.
        s.ui.chat.timeline.answers.entry("ask".into()).or_default().insert("q1".into(), vec!["A".into()]);
        s.ui.chat.timeline.auto_advance = Some(("ask".into(), 0, 0));
        assert!(question::poll(&mut s));
        assert_eq!(s.ui.chat.timeline.question_index.get("ask"), Some(&1));
    }
}
