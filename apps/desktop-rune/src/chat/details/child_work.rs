//! NativeChildWorkDialog: a read-only view of what a provider-native subagent's children did.
//! Loads only while open.

use rune::{El, col, row, text};
use serde_json::{Value, json};

use crate::theme::{self, T};
use crate::{App, data, icons, kit};

#[derive(Default)]
pub struct State {
    /// The subagent thread whose work the dialog shows; `None` while closed.
    thread: Option<String>,
    pending: Option<u64>,
    /// The last answer: the children, or the error to show.
    pub result: Option<Result<Value, String>>,
}

/// Opens the dialog on `thread`, a provider-native subagent (ProviderSubagentBar's "Subagent work").
pub fn open(s: &mut App, thread: &str) {
    s.ui.chat.details.child_work = State { thread: Some(thread.to_string()), ..State::default() };
    refresh(s);
}

fn refresh(s: &mut App) {
    let ui = &s.ui.chat.details.child_work;
    let Some(thread) = ui.thread.clone().filter(|_| ui.pending.is_none()) else { return };
    let Some(net) = s.src.net() else { return };
    let id = net.request("orchestration.getNativeChildWork", json!({"threadId": thread}));
    s.ui.chat.details.child_work.pending = Some(id);
}

fn close(s: &mut App) {
    s.ui.chat.details.child_work = State::default();
}

/// OrchestrationV2GetNativeChildWorkError's message, else the reply's own.
fn failure(error: &Value) -> String {
    match super::tagged(error, "OrchestrationV2GetNativeChildWorkError") {
        Some(_) => "Failed to read the subagent's work.".into(),
        None => crate::shell::palette::error_message(error, "Failed to read the subagent's work."),
    }
}

pub fn poll(s: &mut App) -> bool {
    let Some(id) = s.ui.chat.details.child_work.pending else { return false };
    let Some(reply) = s.src.net().and_then(|net| net.take_reply(id)) else { return false };
    let ui = &mut s.ui.chat.details.child_work;
    ui.pending = None;
    ui.result = Some(reply.map_err(|e| failure(&e)));
    true
}

/// Whether `thread` is a provider-native subagent whose children can be read: Antigravity's
/// (ChatView passes the dialog to ProviderSubagentBar only for that driver).
pub fn available(s: &App, thread: &Value) -> bool {
    let native = data::s(&thread["lineage"], "relationshipToParent") == "subagent" && data::s(thread, "creationSource") == "provider";
    let instance = data::s(&thread["modelSelection"], "instanceId");
    native && crate::chat::composer::provider(s, instance).is_some_and(|p| data::s(p, "driver") == "antigravity")
}

/// The bar's "Subagent work" button, for whoever draws ProviderSubagentBar.
pub fn button(s: &App, t: &T, thread: &Value) -> Option<El> {
    if !available(s, thread) {
        return None;
    }
    let id = data::s(thread, "id").to_string();
    Some(kit::Button::new("Subagent work").size("sm").variant("ghost").icon(&icons::LIST_TREE).on_press(move |s: &mut App| open(s, &id)).build(t))
}

/// "Thinking" for `THINKING`, "Tool call" for `tool_call`.
pub fn step_label(kind: &str) -> String {
    let words = kind.to_lowercase().replace('_', " ");
    let mut chars = words.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn step(t: &T, step: &Value) -> El {
    let muted = t.c.muted_foreground;
    let mut label = step_label(data::s(step, "type"));
    if let Some(status) = step["status"].as_str().filter(|s| *s != "DONE") {
        label = format!("{label} · {}", step_label(status));
    }
    let mut item = col().w_full().min_w(0.0).gap(4.0).child(text(label).font(theme::XS_MEDIUM).color(muted));
    let calls: Vec<String> = step["toolCalls"].as_array().into_iter().flatten().map(|c| format!("{} {}", data::s(c, "name"), data::s(c, "input"))).collect();
    if !calls.is_empty() {
        item = item.child(text(calls.join("\n")).font(theme::mono(theme::TEXT_XS)).color(t.c.foreground).wrap());
    }
    if let Some(body) = step["text"].as_str() {
        let pre = col().w_full().max_h(192.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(text(body.to_string()).font(theme::mono(theme::TEXT_XS)).color(t.c.foreground).wrap());
        item = item.child(pre);
    }
    item
}

fn child(t: &T, child: &Value) -> El {
    let muted = t.c.muted_foreground;
    let role = Some(data::s(child, "role")).filter(|r| !r.is_empty()).unwrap_or("Subagent").to_string();
    let mut section = col()
        .w_full()
        .min_w(0.0)
        .gap(8.0)
        .label(role.clone())
        .child(text(role).font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(text(data::s(child, "prompt").to_string()).font(theme::TEXT_SM).color(muted).wrap());
    if child["truncated"] == true {
        section = section.child(text("Earlier steps are not shown.").font(theme::TEXT_XS).color(muted));
    }
    match child["steps"].as_array() {
        None => section.child(text("No transcript yet.").font(theme::TEXT_XS).color(muted)),
        Some(steps) => {
            let list = col().w_full().gap(8.0).pl(12.0).children(steps.iter().map(|s| step(t, s)));
            section.child(row().w_full().items_stretch().child(rune::el().w(1.0).self_stretch().fill(t.c.border)).child(list.grow(1.0).min_w(0.0)))
        }
    }
}

pub fn overlay(s: &App, t: &T, out: &mut Vec<El>) {
    let ui = &s.ui.chat.details.child_work;
    if ui.thread.is_none() {
        return;
    }
    let muted = t.c.muted_foreground;
    let note = |words: String, color: rune::Color| text(words).font(theme::TEXT_SM).color(color).wrap();
    let body = match &ui.result {
        Some(Err(error)) => note(error.clone(), t.c.destructive_foreground),
        None => note("Loading…".into(), muted),
        Some(Ok(v)) => match v["children"].as_array().filter(|c| !c.is_empty()) {
            None => note("The provider has no record of which subagents this call started.".into(), muted),
            Some(children) => col().w_full().gap(24.0).children(children.iter().map(|c| child(t, c))),
        },
    };
    let refresh_button = kit::Button::new("Refresh").size("sm").variant("outline").disabled(ui.pending.is_some()).on_press(refresh).build(t);
    kit::Dialog::new("native-child-work", "Subagent work")
        .description("Read only. These subagents cannot take messages.")
        .width(672.0)
        .scroll_all()
        .open(true)
        .child(body)
        .child(row().w_full().justify_end().child(refresh_button))
        .on_close(close)
        .build(t, out);
}


/// Scene: two children, one with a transcript and one without.
pub fn scene(s: &mut App) {
    let Some(thread) = super::scene_thread(s) else { return };
    let children = json!({"children": [
        {"conversationId": "c1", "role": "Researcher", "prompt": "Find every caller of the billing schema and note which ones write to it.", "truncated": true, "steps": [
            {"index": 4, "type": "THINKING", "status": null, "text": "Three services import the schema; only the invoicer writes.", "toolCalls": []},
            {"index": 5, "type": "TOOL_CALL", "status": "RUNNING", "text": null, "toolCalls": [{"name": "grep", "input": "\"billing_schema\" src/"}]},
        ]},
        {"conversationId": "c2", "role": "", "prompt": "Draft the migration.", "truncated": false, "steps": null},
    ]});
    s.ui.chat.details.child_work = State { thread: Some(thread), pending: None, result: Some(Ok(children)) };
}

#[cfg(test)]
mod tests {
    use super::{failure, step_label};
    use serde_json::json;

    #[test]
    fn step_types_read_as_sentence_case_words() {
        assert_eq!(step_label("TOOL_CALL"), "Tool call");
        assert_eq!(step_label("thinking"), "Thinking");
        assert_eq!(step_label(""), "");
    }

    #[test]
    fn a_failed_read_names_the_subagent_work() {
        assert_eq!(failure(&json!({"_tag": "OrchestrationV2GetNativeChildWorkError", "threadId": "t"})), "Failed to read the subagent's work.");
        assert_eq!(failure(&json!({"message": "Not allowed"})), "Not allowed");
    }
}
