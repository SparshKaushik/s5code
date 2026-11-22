//! Guest sharing in the chat (GuestShareControls): the header's share status with Stop sharing,
//! and the guest's messages waiting at the end of the timeline for Send or Dismiss.

use std::collections::HashMap;

use rune::{El, col, icon, row, text};
use serde_json::{Value, json};

use crate::data::{iso_ms, s};
use crate::kit::button::Button;
use crate::theme::{self, AlphaMul, T};
use crate::{App, Route, icons};

#[derive(Default)]
pub struct State {
    /// Guest message id -> its send or dismiss in flight.
    resolving: HashMap<String, u64>,
    /// Thread id -> its `thread.guest-share.close` in flight.
    closing: HashMap<String, u64>,
}

/// The thread's guest share, which stays after it expires until the owner clears it.
fn share<'a>(app: &'a App, thread: &str) -> Option<&'a Value> {
    let share = &app.state().thread_views.get(thread)?.projection["thread"]["guestShare"];
    share.is_object().then_some(share)
}

/// GuestShareStatus.
fn status(share: &Value, now: i64) -> String {
    if iso_ms(s(share, "expiresAt")).is_some_and(|at| at / 60_000 <= now / 60_000) {
        return "Guest link expired".to_owned();
    }
    match share["guestName"].as_str().filter(|n| !n.is_empty()) {
        Some(name) => format!("{name} is a guest"),
        None => "Guest link active".to_owned(),
    }
}

/// `pendingGuestMessageCount`'s messages: the guest's, still waiting on the owner.
fn waiting(share: &Value) -> Vec<&Value> {
    share["messages"].as_array().into_iter().flatten().filter(|m| s(m, "status") == "pending").collect()
}

/// `thread.guest-share.close`.
fn close_command(thread: &str, command_id: &str) -> Value {
    json!({"type": "thread.guest-share.close", "commandId": command_id, "threadId": thread})
}

/// resolveGuestMessage: Send runs the guest's text as a queued turn (the server supplies the
/// text), Dismiss drops it.
fn resolve_command(thread: &str, guest_message: &str, send: bool, command_id: &str) -> Value {
    if !send {
        return json!({"type": "thread.guest-message.dismiss", "commandId": command_id, "threadId": thread, "guestMessageId": guest_message});
    }
    json!({
        "type": "message.dispatch", "commandId": command_id, "threadId": thread, "messageId": command_id,
        "guestMessageId": guest_message, "text": "", "attachments": [],
        "dispatchMode": {"type": "queue_after_active"}, "createdBy": "user", "creationSource": "web",
    })
}

fn close(app: &mut App, thread: &str) {
    let command = close_command(thread, &s5_net::uuid());
    let Some(net) = crate::env::net_for(app, thread) else { return };
    let request = net.dispatch(command);
    app.ui.chat.guest.closing.insert(thread.to_owned(), request);
}

fn resolve(app: &mut App, thread: &str, guest_message: &str, send: bool) {
    let command = resolve_command(thread, guest_message, send, &s5_net::uuid());
    let Some(net) = app.src.net() else { return };
    let request = net.dispatch(command);
    app.ui.chat.guest.resolving.insert(guest_message.to_owned(), request);
}

/// Settles the sends, dismissals and closes in flight. Failures are left to the projection,
/// which keeps the message waiting.
pub fn poll(app: &mut App) -> bool {
    let ui = &app.ui.chat.guest;
    if ui.resolving.is_empty() && ui.closing.is_empty() {
        return false;
    }
    let before = ui.resolving.len() + ui.closing.len();
    // A close went to the thread's own environment.
    let closing: Vec<(String, u64)> = ui.closing.iter().map(|(thread, request)| (thread.clone(), *request)).collect();
    for (thread, request) in closing {
        if crate::env::net_for(app, &thread).is_none_or(|net| net.take_reply(request).is_some()) {
            app.ui.chat.guest.closing.remove(&thread);
        }
    }
    if let Some(net) = app.src.net() {
        app.ui.chat.guest.resolving.retain(|_, request| net.take_reply(*request).is_none());
    }
    let ui = &app.ui.chat.guest;
    ui.resolving.len() + ui.closing.len() != before
}

/// GuestShareIndicator: the routed thread's share status, then Stop sharing.
pub fn indicator(app: &App, t: &T) -> Option<El> {
    let Route::Thread(thread) = app.frame_route() else { return None };
    let share = share(app, thread)?;
    let closing = app.ui.chat.guest.closing.contains_key(thread.as_str());
    let thread = thread.clone();
    Some(
        row()
            .key("topbar-guest-share")
            .gap(4.0)
            .center_y()
            .child(icon(&icons::USER, 14.0).color(t.c.muted_foreground))
            .child(text(status(share, app.now_ms)).font(theme::TEXT_XS).color(t.c.muted_foreground))
            .child(
                Button::new("Stop sharing")
                    .variant("ghost-muted")
                    .size("xs")
                    .disabled(closing)
                    .on_press(move |app: &mut App| close(app, &thread))
                    .build(t),
            ),
    )
}

/// GuestMessageName: the guest's name above their message.
pub fn name(name: &str, t: &T) -> El {
    text(name.to_owned()).font(theme::TEXT_2XS).color(t.c.guest_foreground).mr(4.0).label(format!("{name} (guest)"))
}

/// GuestPendingMessages: each waiting message under the guest's name in a dashed bubble, with
/// Dismiss and Send for a connection that may change threads.
pub fn pending(app: &App, thread: &str, t: &T, width: f32) -> Option<El> {
    let share = share(app, thread)?;
    let messages = waiting(share);
    if messages.is_empty() {
        return None;
    }
    let guest = share["guestName"].as_str().filter(|n| !n.is_empty()).unwrap_or("Guest").to_owned();
    let operate = crate::chat::can_operate(app);
    let count = messages.len();
    let mut list = col()
        .w_full()
        .gap(16.0)
        .pb(16.0)
        .role("list")
        .label(format!("{count} guest message{} waiting", if count == 1 { "" } else { "s" }));
    for message in messages {
        let id = s(message, "id").to_owned();
        let busy = app.ui.chat.guest.resolving.contains_key(&id);
        let mut bubble = col().max_w(width * 0.8).min_w(0.0).gap(8.0).padding(12.0).radius(16.0).border(1.0, t.c.guest.alpha_mul(0.5)).border_dash(4.0, 3.0);
        for attachment in message["attachments"].as_array().into_iter().flatten() {
            bubble = bubble.child(crate::chat::timeline::attachments::view(app, attachment, t));
        }
        if !s(message, "text").is_empty() {
            bubble = bubble.child(text(s(message, "text").to_owned()).font(theme::TEXT_SM.lh(22.75)).color(t.c.foreground).wrap_anywhere().selectable(true));
        }
        let mut item = col().w_full().items_end().gap(4.0).role("listitem").child(name(&guest, t)).child(bubble);
        if operate {
            let button = |label: &str, variant: &str, send: bool| {
                let (thread, id) = (thread.to_owned(), id.clone());
                Button::new(label)
                    .variant(variant)
                    .size("xs")
                    .disabled(busy)
                    .on_press(move |app: &mut App| resolve(app, &thread, &id, send))
                    .build(t)
                    .label(format!("{label} message from {guest}"))
            };
            item = item.child(row().gap(4.0).center_y().pr(4.0).child(button("Dismiss", "ghost-muted", false)).child(button("Send", "default", true)));
        }
        list = list.child(item);
    }
    Some(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_share_status_names_the_guest_until_it_expires() {
        let now = iso_ms("2026-03-14T09:00:30Z").unwrap();
        let mut share = json!({"expiresAt": "2026-03-15T09:00:00Z", "guestName": null, "messages": []});
        assert_eq!(status(&share, now), "Guest link active");
        share["guestName"] = json!("Ada");
        assert_eq!(status(&share, now), "Ada is a guest");
        share["expiresAt"] = json!("2026-03-14T09:00:59Z");
        assert_eq!(status(&share, now), "Guest link expired", "compared by the minute");
    }

    #[test]
    fn guest_messages_send_as_queued_turns_or_dismiss() {
        let sent = resolve_command("t1", "g1", true, "c1");
        assert_eq!(sent["type"], "message.dispatch");
        assert_eq!(sent["messageId"], "c1");
        assert_eq!(sent["guestMessageId"], "g1");
        assert_eq!(sent["text"], "", "the server supplies the guest's text");
        assert_eq!(sent["dispatchMode"]["type"], "queue_after_active");
        let dismissed = resolve_command("t1", "g1", false, "c2");
        assert_eq!(dismissed, json!({"type": "thread.guest-message.dismiss", "commandId": "c2", "threadId": "t1", "guestMessageId": "g1"}));
        assert_eq!(close_command("t1", "c3")["type"], "thread.guest-share.close");
        let share = json!({"messages": [{"id": "a", "status": "pending"}, {"id": "b", "status": "sent"}, {"id": "c", "status": "pending"}]});
        assert_eq!(waiting(&share).iter().map(|m| s(m, "id")).collect::<Vec<_>>(), ["a", "c"]);
    }
}
