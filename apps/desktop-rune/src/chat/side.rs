//! Side chats (useSideChat, SideChatPane): `/side` or `/btw` forks the open thread's latest
//! stable point into a hidden child thread, sends the prompt there, and shows that thread's
//! timeline and its own composer in a pane beside the chat.

use std::collections::HashMap;

use rune::{Cx, El, Key, KeyEvent, TextEdit, Tx, auto_grow, col, el, row, text, text_input};
use serde_json::{Value, json};

use crate::chat::details::Lane;
use crate::data;
use crate::icons;
use crate::kit::button::icon_button;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route};

/// The right panel's default width.
const WIDTH: f32 = 540.0;
const MIN_WIDTH: f32 = 320.0;
/// waitForThreadShell's timeout.
const SHELL_WAIT_MS: i64 = 5_000;

#[derive(Default)]
pub struct State {
    /// Each parent thread's open side chat (rightPanelStore's `side-chat:` surface).
    pub open: HashMap<String, String>,
    pending: Option<Pending>,
    /// Each side chat's composer text.
    drafts: HashMap<String, TextEdit>,
    requests: Vec<(u64, Request)>,
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
    Send,
    Delete(String),
}

/// parseSideChatCommand: `/side` or `/btw`, alone or before the prompt it carries.
pub fn parse(text: &str) -> Option<&str> {
    let text = text.trim();
    let (head, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    (head.eq_ignore_ascii_case("/side") || head.eq_ignore_ascii_case("/btw")).then(|| rest.trim())
}

/// deriveThreadTitleSeed for plain text: whitespace collapsed, cut at 50 characters.
fn title_seed(text: &str) -> String {
    let words = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match words.char_indices().nth(50) {
        Some((at, _)) => format!("{}...", &words[..at]),
        None => words,
    }
}

fn thread<'a>(s: &'a App, id: &str) -> Option<&'a Value> {
    s.state().threads.iter().find(|t| data::s(t, "id") == id)
}

fn route_thread(s: &App) -> Option<&Value> {
    match s.frame_route() {
        Route::Thread(id) => thread(s, id),
        _ => None,
    }
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
    let Route::Thread(parent) = s.frame_route().clone() else { return };
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
                "titleSeed": title_seed(&p.prompt), "deliveryIntent": "auto", "dispatchMode": {"type": "start_immediately"},
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
    if let Some(net) = s.src.net() {
        net.watch_thread(&p.target);
    }
    if let Some(d) = s.ui.chat.composer.drafts.get_mut(&p.draft.0)
        && d.edit.text == p.draft.1
    {
        d.edit.clear();
    }
    s.ui.chat.side.open.insert(p.parent, p.target);
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
        match (request, reply) {
            (Request::Send, Err(cause)) => {
                crate::shell::palette::toast_error(s, "Could not send the side chat message.", Some(&message(cause, "")).filter(|m| !m.is_empty()).map(String::as_str))
            }
            (Request::Delete(_), Err(cause)) => {
                crate::shell::palette::toast_error(s, "Could not delete the side chat", Some(&message(cause, "")).filter(|m| !m.is_empty()).map(String::as_str))
            }
            (Request::Delete(side), Ok(_)) => s.ui.chat.side.open.retain(|_, open| open != side),
            (Request::Send, Ok(_)) => {}
        }
    }
    !done.is_empty()
}

fn close(s: &mut App, side: &str) {
    s.ui.chat.side.open.retain(|_, open| open != side);
}

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

/// The side chat composer's send: a message to the side chat, queued while it runs.
fn send(s: &mut App, side: &str) {
    let text = s.ui.chat.side.drafts.get(side).map(|e| e.text.trim().to_owned()).unwrap_or_default();
    if text.is_empty() {
        return;
    }
    let running = thread(s, side).is_some_and(|t| !t["activeRunId"].is_null());
    let Some(net) = s.src.net() else { return };
    let mut cmd = json!({
        "type": "message.dispatch", "commandId": s5_net::uuid(), "createdBy": "user", "creationSource": "web",
        "threadId": side, "messageId": s5_net::uuid(), "text": text, "attachments": [],
        "dispatchMode": {"type": if running { "queue_after_active" } else { "start_immediately" }},
    });
    if !running {
        cmd["deliveryIntent"] = json!("auto");
    }
    let rpc = net.dispatch(cmd);
    s.ui.chat.side.requests.push((rpc, Request::Send));
    if let Some(edit) = s.ui.chat.side.drafts.get_mut(side) {
        edit.clear();
    }
}

/// `chat` with the open thread's side chat pane beside it, when one is open.
pub fn beside(s: &App, cx: &mut Cx<App>, t: &T, chat: El) -> El {
    let Route::Thread(parent) = s.frame_route() else { return chat };
    let Some(side) = s.ui.chat.side.open.get(parent) else { return chat };
    row().key("chat-split").grow(1.0).min_h(0.0).w_full().child(chat.min_w(0.0)).child(pane(s, cx, t, side))
}

/// SideChatPane: the side chat's title, Delete and Close over its timeline and composer.
fn pane(s: &App, cx: &mut Cx<App>, t: &T, side: &str) -> El {
    let shell = thread(s, side);
    let title = shell.map(|r| data::s(r, "title")).filter(|r| !r.is_empty()).unwrap_or("Side chat");
    let (close_id, delete_id) = (side.to_owned(), side.to_owned());
    let mut delete_button = icon_button(&icons::TRASH_2, "Delete side chat").size("icon-xs").disabled(shell.is_none());
    if shell.is_some() {
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
                    .child(composer(s, t, side)),
            )
        }
    };
    row()
        .key("side-chat-pane")
        .w_frac(0.45)
        .max_w(WIDTH)
        .min_w(MIN_WIDTH)
        .h_full()
        .fill(t.c.background)
        .child(el().w(1.0).h_full().fill(t.c.border))
        .child(content)
}

/// The side chat's own composer, floating over the bottom of its timeline.
fn composer(s: &App, t: &T, side: &str) -> El {
    let empty = TextEdit::default();
    let edit = s.ui.chat.side.drafts.get(side).unwrap_or(&empty);
    let (edit_id, key_id) = (side.to_owned(), side.to_owned());
    let field = text_input(edit, theme::TEXT_SM)
        .key("side-chat-input")
        .role("textbox")
        .label("Side chat message")
        .w_full()
        .color(t.c.foreground)
        .placeholder("Ask anything...", t.c.placeholder)
        .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
        .on_edit(move |s: &mut App| s.ui.chat.side.drafts.entry(edit_id.clone()).or_default())
        .on_input_key(move |s: &mut App, e: &KeyEvent, _: &mut Tx| {
            if !matches!(e.key, Key::Enter) || e.mods.shift {
                return false;
            }
            send(s, &key_id);
            true
        });
    col()
        .key("side-chat-composer")
        .pin(Some(12.0), None, Some(12.0), Some(12.0))
        .padding(12.0)
        .radius(radius::XL2)
        .fill(t.c.card)
        .border(1.0, t.c.border)
        .child(auto_grow(field, 1, 8))
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
        assert_eq!(title_seed("  a  b\nc "), "a b c");
        assert_eq!(title_seed(&"x".repeat(51)), format!("{}...", "x".repeat(50)));
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
}
