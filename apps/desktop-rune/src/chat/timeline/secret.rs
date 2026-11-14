//! An agent's request for a secret (SecretRequestCard, client-runtime secretRequest.ts): a masked
//! field the value goes into, saved straight to the server's secret store, never into the chat.

use std::collections::HashMap;

use rune::{El, TextEdit, col, icon, row, text};
use serde_json::{Value, json};

use crate::App;
use crate::data::s;
use crate::icons;
use crate::theme::{self, AlphaMul, T};

#[derive(Default)]
pub struct State {
    typed: HashMap<String, TextEdit>,
    /// Turn item id -> the `secrets.answerRequest` in flight.
    sending: HashMap<String, u64>,
    errors: HashMap<String, String>,
}

/// SECRET_REQUEST_DEFAULT_PLACEHOLDER.
const PLACEHOLDER: &str = "Paste the secret";

/// secretRequestPrivacyNote: a `file` delivery lets the agent's commands read the value.
fn privacy_note(item: &Value) -> &'static str {
    if s(item, "delivery") == "file" {
        "Kept out of chat, in a private file this agent's commands can read for 24 hours"
    } else {
        "Stored securely, never shown to the agent"
    }
}

/// secretRequestDisplay's settled labels, or None while the form should show. A request a fork
/// inherited can only be answered in the thread that asked.
fn settled(item: &Value, thread: &str) -> Option<(&'static rune::Icon, &'static str)> {
    match s(item, "secretStatus") {
        "pending" if s(item, "threadId").is_empty() || s(item, "threadId") == thread => None,
        "pending" => Some((&icons::LOCK, "Waiting for an answer in the original thread")),
        "saved" => Some((&icons::CHECK, "Saved securely and kept private")),
        "declined" => Some((&icons::MINUS, "Declined")),
        _ => Some((&icons::MINUS, "Request ended")),
    }
}

/// secretRequestAnswerInput: a blank secret sends nothing.
fn answer(item: &Value, thread: &str, secret: Option<&str>) -> Option<Value> {
    let answer = match secret.map(str::trim) {
        None => json!({"type": "decline"}),
        Some("") => return None,
        Some(secret) => json!({"type": "save", "secret": secret}),
    };
    let thread = if s(item, "threadId").is_empty() { thread } else { s(item, "threadId") };
    Some(json!({"threadId": thread, "turnItemId": s(item, "id"), "answer": answer}))
}

/// secretRequestFailureMessage: only known server errors pass through, so a decoding error can
/// never echo the typed value.
fn failure_message(cause: &Value) -> String {
    let error = super::link::failure(cause);
    let message = s(error, "message").trim();
    if matches!(s(error, "_tag"), "SecretRequestError" | "EnvironmentAuthorizationError") && !message.is_empty() {
        message.to_owned()
    } else {
        "Could not answer the request. Try again.".to_owned()
    }
}

fn send(app: &mut App, item: &Value, thread: &str, secret: Option<&str>) {
    let id = s(item, "id").to_owned();
    let Some(input) = answer(item, thread, secret) else { return };
    let Some(net) = app.src.net() else { return };
    if app.ui.chat.timeline.secrets.sending.contains_key(&id) {
        return;
    }
    let request = net.request("secrets.answerRequest", input);
    let ui = &mut app.ui.chat.timeline.secrets;
    ui.sending.insert(id.clone(), request);
    ui.errors.remove(&id);
}

pub fn poll(app: &mut App) -> bool {
    let Some(net) = app.src.net() else { return false };
    let ui = &mut app.ui.chat.timeline.secrets;
    let mut changed = false;
    ui.sending.retain(|id, request| {
        let Some(reply) = net.take_reply(*request) else { return true };
        changed = true;
        match reply {
            Ok(_) => {
                ui.typed.remove(id);
            }
            Err(error) => {
                ui.errors.insert(id.clone(), failure_message(&error));
            }
        }
        false
    });
    changed
}

pub fn view(app: &App, item: &Value, thread: &str, t: &T) -> El {
    let label = s(item, "label").to_owned();
    if let Some((glyph, status)) = settled(item, thread) {
        return row()
            .gap(12.0)
            .px(4.0)
            .py(4.0)
            .center_y()
            .child(icon(glyph, 14.0).color(t.c.muted_foreground))
            .child(text(format!("{label} · {status}")).font(theme::TEXT_SM).color(t.c.muted_foreground).truncate().shrink());
    }
    let id = s(item, "id").to_owned();
    let ui = &app.ui.chat.timeline.secrets;
    let blank = TextEdit::default();
    let typed = ui.typed.get(&id).unwrap_or(&blank);
    let busy = ui.sending.contains_key(&id);
    let placeholder = item["placeholder"].as_str().filter(|p| !p.trim().is_empty()).unwrap_or(PLACEHOLDER);
    let (save_item, save_thread) = (item.clone(), thread.to_owned());
    let save = move |app: &mut App| {
        let secret = app.ui.chat.timeline.secrets.typed.get(s(&save_item, "id")).map(|e| e.text.clone()).unwrap_or_default();
        send(app, &save_item, &save_thread, Some(&secret));
    };
    let submit = save.clone();
    let edit_id = id.clone();
    let field = crate::kit::Input::new(typed)
        .placeholder(placeholder)
        .disabled(busy)
        .invalid(ui.errors.contains_key(&id))
        .on_edit(move |app: &mut App| app.ui.chat.timeline.secrets.typed.entry(edit_id.clone()).or_default())
        .on_submit(move |app: &mut App, _| submit(app))
        .build(t)
        .single_line()
        .secure()
        .label(label.clone())
        .grow(1.0)
        .min_w(0.0);
    let (decline_item, decline_thread) = (item.clone(), thread.to_owned());
    let mut card = col()
        .w_full()
        .gap(12.0)
        .padding(16.0)
        .radius(12.0)
        .border(1.0, t.c.border.alpha_mul(0.6))
        .fill(t.c.card)
        .child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground));
    if !s(item, "reason").trim().is_empty() {
        card = card.child(text(s(item, "reason").to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground).selectable(true));
    }
    card = card.child(
        row().w_full().gap(8.0).center_y().child(field).child(
            crate::kit::button::Button::new("Save securely")
                .disabled(busy || typed.text.trim().is_empty())
                .on_press(save)
                .build(t),
        ),
    );
    if let Some(error) = ui.errors.get(&id) {
        card = card.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive));
    }
    card.child(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(
                row()
                    .gap(6.0)
                    .center_y()
                    .grow(1.0)
                    .min_w(0.0)
                    .child(icon(&icons::SHIELD_CHECK, 14.0).color(t.c.muted_foreground))
                    .child(text(privacy_note(item)).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().shrink()),
            )
            .child(
                crate::kit::button::Button::new("Decline")
                    .variant("ghost-muted")
                    .size("xs")
                    .disabled(busy)
                    .on_press(move |app: &mut App| send(app, &decline_item, &decline_thread, None))
                    .build(t),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::{answer, failure_message, settled};
    use serde_json::json;

    #[test]
    fn answers_trim_and_skip_a_blank_secret() {
        let item = json!({"id": "i", "threadId": "t"});
        assert_eq!(answer(&item, "t", Some("  ")), None);
        assert_eq!(answer(&item, "t", Some(" abc ")).unwrap()["answer"], json!({"type": "save", "secret": "abc"}));
        assert_eq!(answer(&item, "t", None).unwrap(), json!({"threadId": "t", "turnItemId": "i", "answer": {"type": "decline"}}));
    }

    #[test]
    fn only_known_errors_reach_the_card() {
        let fail = |error: serde_json::Value| json!([{"_tag": "Fail", "error": error}]);
        assert_eq!(failure_message(&fail(json!({"_tag": "SecretRequestError", "message": "This secret request was already answered."}))), "This secret request was already answered.");
        assert_eq!(failure_message(&fail(json!({"_tag": "ParseError", "message": "Expected string, got \"whsec_1\""}))), "Could not answer the request. Try again.");
    }

    #[test]
    fn a_forked_request_is_answered_where_it_was_asked() {
        let pending = json!({"secretStatus": "pending", "threadId": "origin"});
        assert!(settled(&pending, "origin").is_none());
        assert_eq!(settled(&pending, "fork").map(|(_, l)| l), Some("Waiting for an answer in the original thread"));
        assert_eq!(settled(&json!({"secretStatus": "saved"}), "t").map(|(_, l)| l), Some("Saved securely and kept private"));
    }
}
