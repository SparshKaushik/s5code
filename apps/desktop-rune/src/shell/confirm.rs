//! `ConfirmDialogHost`: the one place `dialogs.confirm` questions show. Requests queue and show
//! one at a time; the message splits into a title and a description.

use std::collections::VecDeque;
use std::rc::Rc;

use rune::{El, row};

use crate::App;
use crate::theme::T;

struct Pending {
    message: String,
    destructive: bool,
    on_confirm: Rc<dyn Fn(&mut App)>,
}

#[derive(Default)]
pub struct Ui {
    queue: VecDeque<Pending>,
}

impl Ui {
    pub fn is_open(&self) -> bool {
        !self.queue.is_empty()
    }
}

/// `requestConfirmDialog`: asks `message`, running `on_confirm` only if it is confirmed.
pub fn request(s: &mut App, message: impl Into<String>, destructive: bool, on_confirm: impl Fn(&mut App) + 'static) {
    s.ui.shell.confirm.queue.push_back(Pending { message: message.into(), destructive, on_confirm: Rc::new(on_confirm) });
}

/// The question on screen, for tests and callers that must not stack a second one.
pub fn current(s: &App) -> Option<&str> {
    s.ui.shell.confirm.queue.front().map(|p| p.message.as_str())
}

/// `respondToConfirmDialog`: closes the question; the next queued one follows.
pub fn respond(s: &mut App, confirmed: bool) {
    let Some(pending) = s.ui.shell.confirm.queue.pop_front() else { return };
    if confirmed {
        (pending.on_confirm)(s);
    }
}

/// `resolveConfirmDialogCopy`: the line ending in "?" is the title, else the text through the
/// first "?", else a generic title over the whole message.
pub fn copy(message: &str) -> (String, Option<String>) {
    let message = message.trim();
    let lines: Vec<&str> = message.split('\n').collect();
    if let Some(at) = lines.iter().position(|l| l.trim().ends_with('?')) {
        let rest = lines.iter().enumerate().filter(|(n, _)| *n != at).map(|(_, l)| *l).collect::<Vec<_>>().join("\n");
        let rest = rest.trim();
        return (lines[at].trim().to_owned(), (!rest.is_empty()).then(|| rest.to_owned()));
    }
    if let Some(at) = message.find('?') {
        let rest = message[at + 1..].trim();
        return (message[..=at].trim().to_owned(), (!rest.is_empty()).then(|| rest.to_owned()));
    }
    let description = if message.is_empty() { "This action requires your confirmation." } else { message };
    ("Confirm action".to_owned(), Some(description.to_owned()))
}

pub fn view(s: &App, t: &T, overlays: &mut Vec<El>) {
    let Some(pending) = s.ui.shell.confirm.queue.front() else { return };
    let (title, description) = copy(&pending.message);
    let cancel = crate::kit::Button::new("Cancel").variant("outline").on_press(|s: &mut App| respond(s, false)).build(t);
    let confirm = crate::kit::Button::new("Confirm")
        .variant(if pending.destructive { "destructive" } else { "default" })
        .on_press(|s: &mut App| respond(s, true))
        .build(t);
    let mut dialog = crate::kit::Dialog::new("confirm-dialog", title).alert().width(448.0).open(true);
    if let Some(description) = description {
        dialog = dialog.description(description);
    }
    dialog
        .child(row().w_full().gap(8.0).justify_end().child(cancel).child(confirm))
        .on_close(|s: &mut App| respond(s, false))
        .build(t, overlays);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_splits_like_the_web_host() {
        let own = |t: &str, d: Option<&str>| (t.to_owned(), d.map(str::to_owned));
        assert_eq!(copy("Delete 2 threads?\nThis permanently clears conversation history for these threads."), own("Delete 2 threads?", Some("This permanently clears conversation history for these threads.")));
        assert_eq!(copy("Some context first.\nRemove it?\nMore after."), own("Remove it?", Some("Some context first.\nMore after.")), "FAILED: the question line is the title wherever it sits");
        assert_eq!(copy("Archive thread \"a?b\"?"), own("Archive thread \"a?b\"?", None));
        assert_eq!(copy("Stop now? It is running"), own("Stop now?", Some("It is running")), "FAILED: no line ends in ?, so it splits at the first ?");
        assert_eq!(copy("Proceed. Really?? Yes"), own("Proceed. Really?", Some("? Yes")));
        assert_eq!(copy("Just do it"), own("Confirm action", Some("Just do it")));
        assert_eq!(copy("  "), own("Confirm action", Some("This action requires your confirmation.")));
    }

    #[test]
    fn questions_queue_and_only_a_confirm_runs_the_action() {
        let mut s = App::new();
        request(&mut s, "First?", false, |s: &mut App| s.ui.shell.panel_open = true);
        request(&mut s, "Second?", true, |s: &mut App| s.ui.shell.panel_open = false);
        assert_eq!(current(&s), Some("First?"));
        s.ui.shell.panel_open = false;
        respond(&mut s, false);
        assert!(!s.ui.shell.panel_open, "FAILED: cancel runs nothing");
        assert_eq!(current(&s), Some("Second?"), "FAILED: the next question follows");
        s.ui.shell.panel_open = true;
        respond(&mut s, true);
        assert!(!s.ui.shell.panel_open && current(&s).is_none());
    }
}
