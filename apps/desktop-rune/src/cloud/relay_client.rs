//! Web `ensureRelayClientAvailable` and `RelayClientInstallDialog`: before an environment links
//! with a managed tunnel, it needs the relay client, which it installs once the user agrees.

use rune::{El, col, icon, row, text};
use serde_json::{Value, json};

use crate::theme::{self, T};
use crate::{App, icons, kit};

const STEPS: [(&str, &str); 7] = [
    ("checking", "Checking current installation"),
    ("waiting_for_lock", "Waiting for installer"),
    ("downloading", "Downloading relay client"),
    ("verifying", "Verifying download"),
    ("installing", "Installing relay client"),
    ("validating", "Validating executable"),
    ("activating", "Activating installation"),
];

enum Phase {
    Checking(u64),
    Confirming(String),
    Installing { request: u64, stage: Option<String>, last: Option<Value> },
}

pub struct Install {
    environment: String,
    phase: Phase,
    then: Box<dyn FnOnce(&mut App, Result<(), String>)>,
}

/// What `cloud.getRelayClientStatus` says to do next.
#[derive(Debug, PartialEq)]
enum Next {
    Ready,
    Confirm(String),
    Fail(String),
}

fn unsupported(status: &Value) -> String {
    format!("T3 Code cannot install the relay client automatically on {}-{}.", crate::data::s(status, "platform"), crate::data::s(status, "arch"))
}

fn next(status: &Value) -> Next {
    match crate::data::s(status, "status") {
        "available" => Next::Ready,
        "unsupported" => Next::Fail(unsupported(status)),
        _ => Next::Confirm(crate::data::s(status, "version").to_owned()),
    }
}

/// The outcome of an install stream whose last event was `last`.
fn installed(last: Option<&Value>) -> Result<(), String> {
    let Some(complete) = last.filter(|e| e["type"] == "complete") else {
        return Err("The relay client install completed without a final status.".into());
    };
    match crate::data::s(&complete["status"], "status") {
        "available" => Ok(()),
        "unsupported" => Err(unsupported(&complete["status"])),
        _ => Err("The relay client is still unavailable after installation.".into()),
    }
}

/// Makes sure `environment` has the relay client, asking before installing it, then runs `then`
/// with the outcome; an `Err` is the message to show. Call it before the link challenge when the
/// link turns on a managed tunnel.
pub(crate) fn ensure(s: &mut App, environment: &str, then: impl FnOnce(&mut App, Result<(), String>) + 'static) {
    let Some(request) = crate::env::net_of(s, environment).map(|net| net.request("cloud.getRelayClientStatus", json!({}))) else {
        return then(s, Err("Could not check relay client availability.".into()));
    };
    if let Some(earlier) = s.ui.cloud.install.take() {
        (earlier.then)(s, Err("Relay client installation was cancelled.".into()));
    }
    s.ui.cloud.install = Some(Install { environment: environment.to_owned(), phase: Phase::Checking(request), then: Box::new(then) });
}

fn finish(s: &mut App, result: Result<(), String>) {
    if let Some(install) = s.ui.cloud.install.take() {
        (install.then)(s, result);
    }
}

fn respond(s: &mut App, confirmed: bool) {
    if !matches!(s.ui.cloud.install.as_ref().map(|i| &i.phase), Some(Phase::Confirming(_))) {
        return;
    }
    if !confirmed {
        return finish(s, Err("Relay client installation was cancelled.".into()));
    }
    let environment = s.ui.cloud.install.as_ref().map(|i| i.environment.clone()).unwrap_or_default();
    match crate::env::net_of(s, &environment).map(|net| net.request("cloud.installRelayClient", json!({}))) {
        Some(request) => {
            if let Some(install) = s.ui.cloud.install.as_mut() {
                install.phase = Phase::Installing { request, stage: None, last: None };
            }
        }
        None => finish(s, Err("Could not install the relay client.".into())),
    }
}

pub(super) fn poll(s: &mut App) -> bool {
    // Scenes park the dialog on a fixture environment without a socket.
    if !super::live() {
        return false;
    }
    let (environment, request) = match s.ui.cloud.install.as_ref() {
        Some(Install { environment, phase: Phase::Checking(id) | Phase::Installing { request: id, .. }, .. }) => (environment.clone(), *id),
        _ => return false,
    };
    let Some(net) = crate::env::net_of(s, &environment) else {
        finish(s, Err("Could not install the relay client.".into()));
        return true;
    };
    let events = net.take_chunks(request);
    let reply = net.take_reply(request);
    let changed = !events.is_empty() || reply.is_some();
    let Some(install) = s.ui.cloud.install.as_mut() else { return changed };
    let outcome = match install.phase {
        Phase::Confirming(_) => None,
        Phase::Checking(_) => match reply {
            None => None,
            Some(Err(_)) => Some(Err("Could not check relay client availability.".into())),
            Some(Ok(status)) => match next(&status) {
                Next::Ready => Some(Ok(())),
                Next::Fail(message) => Some(Err(message)),
                Next::Confirm(version) => {
                    install.phase = Phase::Confirming(version);
                    None
                }
            },
        },
        Phase::Installing { ref mut stage, ref mut last, .. } => {
            for event in events {
                if event["type"] == "progress" {
                    *stage = event["stage"].as_str().map(str::to_owned);
                }
                *last = Some(event);
            }
            match reply {
                None => None,
                Some(Err(_)) => Some(Err("Could not install the relay client.".into())),
                Some(Ok(_)) => Some(installed(last.as_ref())),
            }
        }
    };
    if let Some(result) = outcome {
        finish(s, result);
    }
    changed
}

/// Web `RelayClientInstallDialog`.
pub fn overlay(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(install) = &s.ui.cloud.install else { return };
    let (title, description, body) = match &install.phase {
        Phase::Checking(_) => return,
        Phase::Confirming(version) => (
            "Install relay client?",
            "T3 Code needs the relay client to make this environment available through T3 Connect.",
            col()
                .w_full()
                .gap(4.0)
                .padding(12.0)
                .radius(12.0)
                .border(1.0, t.c.border.alpha_mul(0.7))
                .fill(t.c.muted.alpha_mul(0.35))
                .child(text("Managed relay client").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(text(format!("T3 Code will download and install version {version} locally.")).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap()),
        ),
        Phase::Installing { stage, .. } => {
            let at = STEPS.iter().position(|(id, _)| Some(*id) == stage.as_deref());
            let label = at.map(|i| STEPS[i].1).unwrap_or_default();
            let done = at.map_or(0, |i| i + 1) as f32 / STEPS.len() as f32;
            (
                "Installing relay client",
                "T3 Code is preparing this environment for secure access through T3 Connect.",
                col()
                    .w_full()
                    .gap(10.0)
                    .child(
                        row()
                            .w_full()
                            .gap(12.0)
                            .center_y()
                            .child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground).grow(1.0))
                            .child(text(format!("{} of {}", at.map_or(0, |i| i + 1), STEPS.len())).font(theme::TEXT_SM).color(t.c.muted_foreground)),
                    )
                    .child(
                        row()
                            .w_full()
                            .h(8.0)
                            .radius(4.0)
                            .fill(t.c.muted)
                            .role("progressbar")
                            .label("Relay client installation progress")
                            .child(rune::el().h_full().w_frac(done).radius(4.0).fill(t.c.primary)),
                    )
                    .child(text("Keep T3 Code open while the relay client is installed.").font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()),
            )
        }
    };
    let confirming = matches!(install.phase, Phase::Confirming(_));
    let glyph = row().w(36.0).h(36.0).center().radius(8.0).border(1.0, t.c.border.alpha_mul(0.7)).fill(t.c.muted.alpha_mul(0.6)).child(icon(&icons::DOWNLOAD, 18.0).color(t.c.muted_foreground));
    let mut dialog = kit::Dialog::new("relay-client-install", title).open(true).width(448.0).title_icon(glyph).description(description);
    if !confirming {
        // Web only closes it from the confirmation; an install runs to its end.
        dialog = dialog.alert();
    }
    dialog = dialog.on_close(|s: &mut App| respond(s, false)).child(body);
    if confirming {
        dialog = dialog.child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(kit::Button::new("Cancel").variant("outline").on_press(|s: &mut App| respond(s, false)).build(t))
                .child(kit::Button::new("Download and install").on_press(|s: &mut App| respond(s, true)).build(t)),
        );
    }
    dialog.build(t, layers);
}

/// Scenes: the confirmation, or an install three steps in.
pub(crate) fn scene(s: &mut App, installing: bool) {
    let phase = if installing { Phase::Installing { request: 0, stage: Some("verifying".into()), last: None } } else { Phase::Confirming("0.4.2".into()) };
    s.ui.cloud.install = Some(Install { environment: crate::env::LOCAL.into(), phase, then: Box::new(|_, _| {}) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn the_status_says_whether_to_ask_before_installing() {
        assert_eq!(next(&json!({"status": "available", "executablePath": "/x", "source": "managed", "version": "0.4.2"})), Next::Ready);
        assert_eq!(next(&json!({"status": "missing", "version": "0.4.2"})), Next::Confirm("0.4.2".into()));
        assert_eq!(next(&json!({"status": "unsupported", "platform": "freebsd", "arch": "x64", "version": "0.4.2"})), Next::Fail("T3 Code cannot install the relay client automatically on freebsd-x64.".into()));
    }

    #[test]
    fn an_install_succeeds_only_on_a_final_available_status() {
        let complete = |status: Value| json!({"type": "complete", "status": status});
        assert_eq!(installed(Some(&complete(json!({"status": "available", "version": "0.4.2"})))), Ok(()));
        assert_eq!(installed(Some(&complete(json!({"status": "missing", "version": "0.4.2"})))), Err("The relay client is still unavailable after installation.".into()));
        assert_eq!(installed(Some(&complete(json!({"status": "unsupported", "platform": "linux", "arch": "riscv64"})))), Err("T3 Code cannot install the relay client automatically on linux-riscv64.".into()));
        assert_eq!(installed(Some(&json!({"type": "progress", "stage": "activating"}))), Err("The relay client install completed without a final status.".into()));
        assert_eq!(installed(None), Err("The relay client install completed without a final status.".into()));
    }

    #[test]
    fn declining_the_confirmation_cancels_the_link() {
        let mut s = App::new();
        let outcome = Rc::new(RefCell::new(None));
        let seen = outcome.clone();
        s.ui.cloud.install = Some(Install { environment: "env-1".into(), phase: Phase::Confirming("0.4.2".into()), then: Box::new(move |_, result| *seen.borrow_mut() = Some(result)) });
        respond(&mut s, false);
        assert_eq!(*outcome.borrow(), Some(Err("Relay client installation was cancelled.".into())));
        assert!(s.ui.cloud.install.is_none());
    }
}
