//! Approvals waiting in a thread's subagents and side chats, answered from the parent above the
//! composer (ChildThreadApprovals; client-runtime threadShell.ts childThreadInputsAtom).

use std::collections::HashSet;

use rune::{El, col, text};
use serde_json::Value;

use super::requests;
use super::rows::array;
use crate::data::s;
use crate::theme::{self, T};
use crate::{App, Route, env};

/// isChildRunLive: the shell's activity run when it reports one, else its status.
fn run_live(thread: &Value) -> bool {
    match thread.get("activityRunStatus") {
        Some(status) => !status.is_null(),
        None => matches!(s(thread, "status"), "preparing" | "starting" | "running" | "waiting"),
    }
}

/// hasPendingApprovals plus subagentRequest/sideChatRequest: a provider-native subagent asks on
/// its parent's own run, and a subagent whose run ended can no longer be answered.
fn asks_parent(thread: &Value) -> bool {
    let kind = s(&thread["pendingRuntimeRequest"], "kind");
    if kind.is_empty() || matches!(kind, "user_input" | "auth_refresh") {
        return false;
    }
    let lineage = &thread["lineage"];
    if lineage["sideChat"] == true {
        return true;
    }
    let subagent = s(lineage, "relationshipToParent") == "subagent";
    (subagent || !thread["delegatedFrom"].is_null()) && !(subagent && s(thread, "creationSource") == "provider") && run_live(thread)
}

fn parent_of(thread: &Value) -> &str {
    let lineage = &thread["lineage"];
    let parent = s(lineage, "parentThreadId");
    if !parent.is_empty() && (s(lineage, "relationshipToParent") == "subagent" || lineage["sideChat"] == true) {
        parent
    } else {
        s(&thread["delegatedFrom"], "threadId")
    }
}

/// The parent's descendants asking for approval, nearest first.
fn children<'a>(threads: &'a [Value], parent: &str) -> Vec<&'a Value> {
    if parent.is_empty() || !threads.iter().any(asks_parent) {
        return Vec::new();
    }
    let mut seen: HashSet<&str> = HashSet::from([parent]);
    let mut pending = vec![parent];
    let mut out = Vec::new();
    let mut next = 0;
    while next < pending.len() {
        let at = pending[next];
        next += 1;
        for thread in threads.iter().filter(|t| parent_of(t) == at) {
            let id = s(thread, "id");
            if !seen.insert(id) {
                continue;
            }
            pending.push(id);
            if asks_parent(thread) {
                out.push(thread);
            }
        }
    }
    out
}

/// fullAccessOverrideExplanation.
fn override_explanation(kind: &str) -> &'static str {
    if kind == "ask_rule" {
        "Full access skips approvals, but an ask rule in the agent's own settings matches this action, so it still asks. Remove or narrow that rule to stop these prompts."
    } else {
        "Full access skips approvals, but the agent still stops for a few safety checks of its own that no setting can skip, such as a delete whose path it can't verify. T3 Code can't turn these off, so check the request and approve it if it's what you expect."
    }
}

/// The route thread's children on this environment that ask; answers go through `App::src`.
fn asking(app: &App) -> Vec<&Value> {
    let Route::Thread(parent) = app.frame_route() else { return Vec::new() };
    children(&app.state().threads, parent).into_iter().filter(|c| env::of(app, c) == app.ui.env.active).collect()
}

/// Subscribes to each asking child, so its pending requests are known.
pub fn poll(app: &mut App) -> bool {
    let ids: Vec<String> = asking(app).into_iter().map(|c| s(c, "id").to_owned()).collect();
    if let Some(net) = app.src.net() {
        for id in ids {
            net.watch_thread(&id);
        }
    }
    false
}

pub fn view(app: &App, t: &T) -> Option<El> {
    let mut cards = Vec::new();
    for child in asking(app) {
        let id = s(child, "id");
        let Some(view) = app.state().thread_views.get(id) else { continue };
        let title = Some(s(child, "title")).filter(|t| !t.trim().is_empty()).unwrap_or("a subagent");
        for request in requests::pending_approvals(&view.projection) {
            let item = array(&view.projection, "turnItems").iter().find(|i| s(i, "requestId") == s(request, "id"));
            let override_note = item.and_then(|i| i["fullAccessOverride"].as_str()).map(override_explanation);
            cards.push(
                col()
                    .w_full()
                    .gap(4.0)
                    .padding(12.0)
                    .radius(8.0)
                    .border(1.0, t.c.border)
                    .fill(t.c.background)
                    .label(format!("Approval from {title}"))
                    .child(text(format!("Approval needed from {title}")).font(theme::XS_MEDIUM).color(t.c.foreground).truncate())
                    .child(super::request_view(app, id, request, item, t))
                    .children(override_note.map(|note| text(note).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap())),
            );
        }
    }
    (!cards.is_empty()).then(|| col().key("child-approvals").w_full().gap(8.0).max_h(320.0).scroll(rune::ScrollSpec { stick_bottom: false }).children(cards))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn thread(id: &str, parent: &str, extra: Value) -> Value {
        let mut t = json!({"id": id, "status": "running", "lineage": {"parentThreadId": parent, "relationshipToParent": "subagent"}});
        for (k, v) in extra.as_object().unwrap() {
            t[k] = v.clone();
        }
        t
    }

    #[test]
    fn descendants_that_can_answer_roll_up_to_the_parent() {
        let approval = json!({"pendingRuntimeRequest": {"kind": "command"}});
        let threads = vec![
            thread("a", "p", json!({})),
            thread("b", "a", approval.clone()),
            thread("c", "p", json!({"pendingRuntimeRequest": {"kind": "user_input"}})),
            thread("d", "p", json!({"pendingRuntimeRequest": {"kind": "command"}, "creationSource": "provider"})),
            thread("e", "p", json!({"pendingRuntimeRequest": {"kind": "command"}, "activityRunStatus": null})),
            thread("f", "q", approval.clone()),
            json!({"id": "g", "status": "idle", "lineage": {"parentThreadId": "p", "sideChat": true}, "pendingRuntimeRequest": {"kind": "file-change"}}),
            json!({"id": "h", "status": "running", "lineage": {}, "delegatedFrom": {"threadId": "p"}, "pendingRuntimeRequest": {"kind": "command"}}),
        ];
        let ids: Vec<_> = children(&threads, "p").into_iter().map(|t| s(t, "id")).collect();
        // A grandchild counts; a question, a provider-native subagent and an ended run do not;
        // a side chat asks even when idle.
        assert_eq!(ids, ["g", "h", "b"]);
        assert!(children(&threads, "").is_empty());
    }
}
