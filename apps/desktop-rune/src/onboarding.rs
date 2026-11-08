//! First run: `NoProjectsHero`, which `/` shows instead of a draft once the shell has loaded and
//! the workspace has no projects (routes/_chat.index.tsx), and `FirstRunRecovery`, which replaces
//! the app while the shell never arrives. The web's setup wizard (`/welcome`) is not ported; this
//! hero is what it leaves behind when dismissed.

use std::time::{Duration, Instant};

use rune::{El, Tx, col, el, row, text};
use s5_net::Conn;

use crate::data::Source;
use crate::theme::{self, T, layout};
use crate::{App, Route, icons, kit};

/// FirstRunGate's FIRST_RUN_DECISION_TIMEOUT_MS.
const STALL: Duration = Duration::from_secs(4);

#[derive(Default)]
pub struct Ui {
    started: Option<Instant>,
    stalled: bool,
}

#[derive(Debug, PartialEq)]
pub enum Recovery {
    /// The bundled server gave up (`host::Event::Failed`); its supervisor has returned.
    Failed(String),
    /// Still no shell after the timeout. The host restarts the server and Net reconnects on
    /// their own, so there is nothing to reload, only to wait for.
    Stalled,
}

pub fn poll(s: &mut App, tx: &mut Tx) -> bool {
    let ready = s.src.state().shell_ready;
    let ui = &mut s.ui.onboarding;
    let started = *ui.started.get_or_insert_with(Instant::now);
    if ready || ui.stalled {
        return false;
    }
    let elapsed = started.elapsed();
    if elapsed < STALL {
        tx.wake_after(STALL - elapsed);
        return false;
    }
    ui.stalled = true;
    true
}

pub fn recovery(s: &App) -> Option<Recovery> {
    let st = s.state();
    if st.shell_ready {
        return None;
    }
    match (&s.src, &st.conn) {
        (Source::Fixture(_), Conn::Closed(reason)) => Some(Recovery::Failed(reason.clone())),
        _ => s.ui.onboarding.stalled.then_some(Recovery::Stalled),
    }
}

fn retry(s: &mut App) {
    if let Source::Fixture(state) = &mut s.src {
        state.conn = Conn::Connecting;
    }
    s.ui.onboarding = Ui::default();
    if let Some(waker) = s.ui.panel.waker.clone() {
        crate::setup(s, waker);
    }
}

/// `FirstRunRecovery`, over the whole window as the web's gate renders it instead of the app.
pub fn recovery_view(recovery: Recovery, t: &T) -> El {
    let (title, description, action) = match &recovery {
        Recovery::Failed(reason) => ("Could not start the server", reason.as_str(), true),
        Recovery::Stalled => ("Still connecting", "T3 Code could not confirm this workspace.", false),
    };
    let mut body = col()
        .max_w(384.0)
        .items_center()
        .child(text(title).font(theme::weight(theme::TEXT_LG, theme::SEMIBOLD)).color(t.c.foreground))
        .child(text(description).font(theme::TEXT_SM).color(t.c.muted_foreground).mt(8.0));
    body = if action {
        body.child(
            row().pt(20.0).child(
                kit::Button::new("Retry").size("sm").variant("outline").icon(&icons::REFRESH_CW).on_press(retry).build(t),
            ),
        )
    } else {
        body.child(row().pt(20.0).child(kit::Spinner::new().size("sm").tone("muted").build(&kit::theme(t))))
    };
    col()
        .key("first-run-recovery")
        .role("main")
        .w_full()
        .h_full()
        .fill(t.c.background)
        .child(el().w_full().h(layout::TOPBAR_H).window_drag())
        .child(col().w_full().grow(1.0).min_h(0.0).px(24.0).center().child(body))
}

/// The draft route with nothing to draft in: the shell is in and every project is archived or gone.
pub fn shows_hero(s: &App) -> bool {
    let st = s.state();
    matches!(s.frame_route(), Route::Home { .. }) && st.shell_ready && !st.projects.iter().any(|p| p["archivedAt"].is_null())
}

pub fn hero(s: &App, t: &T) -> El {
    let scratch = crate::shell::palette::actions::scratch_available(s);
    let mut buttons = row().pt(24.0).gap(8.0).justify_center().child(
        kit::Button::new("Add project")
            .size("sm")
            .icon(&icons::PLUS)
            .on_press(|s: &mut App| crate::shell::palette::open_add_project(s))
            .build(t),
    );
    if scratch {
        buttons = buttons.child(
            kit::Button::new("Start without a project")
                .size("sm")
                .variant("outline")
                .icon(&icons::MESSAGE_SQUARE_DASHED)
                .on_press(crate::shell::palette::actions::new_without_project)
                .build(t),
        );
    }
    let description = if scratch { "Add a project, or start without one." } else { "Add a project to start your first thread." };
    // Empty size="hero": the title is text-3xl at desktop widths, the description text-sm, mt-1.
    let header = col()
        .w_full()
        .max_w(512.0)
        .px(32.0)
        .py(48.0)
        .items_center()
        .child(text("What should we work on?").font(theme::weight(theme::TEXT_3XL, theme::SEMIBOLD)).color(t.c.foreground))
        .child(text(description).font(theme::TEXT_SM).color(t.c.muted_foreground).mt(4.0))
        .child(buttons);
    col()
        .key("main")
        .role("main")
        .label("Home")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .h_full()
        .fill(t.c.background)
        // The desktop window only moves where it opts in, so keep a titlebar strip.
        .child(el().key("hero-titlebar").w_full().h(layout::TOPBAR_H).window_drag())
        .child(col().key("no-projects-hero").w_full().grow(1.0).min_h(0.0).padding(48.0).center().child(header))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_hero_waits_for_the_shell_and_only_counts_live_projects() {
        let mut s = App::new();
        let mut state = crate::data::empty_state();
        state.projects = vec![json!({"id": "p1", "archivedAt": "2026-01-01T00:00:00.000Z"})];
        s.src = crate::data::Source::Fixture(state);
        assert!(!shows_hero(&s), "a pending shell cannot prove the workspace is empty");
        if let crate::data::Source::Fixture(state) = &mut s.src {
            state.shell_ready = true;
        }
        assert!(shows_hero(&s));
        if let crate::data::Source::Fixture(state) = &mut s.src {
            state.projects.push(json!({"id": "p2", "archivedAt": null}));
        }
        assert!(!shows_hero(&s));
    }

    #[test]
    fn recovery_reports_a_failed_host_at_once_and_a_missing_shell_only_after_the_timeout() {
        let mut s = App::new();
        s.src = crate::data::Source::Fixture(crate::data::empty_state());
        assert_eq!(recovery(&s), None, "a starting server is not a failure yet");
        s.ui.onboarding.stalled = true;
        assert_eq!(recovery(&s), Some(Recovery::Stalled));
        if let crate::data::Source::Fixture(state) = &mut s.src {
            state.conn = Conn::Closed("No free port for the S5 Code server.".into());
        }
        assert_eq!(recovery(&s), Some(Recovery::Failed("No free port for the S5 Code server.".into())));
        if let crate::data::Source::Fixture(state) = &mut s.src {
            state.shell_ready = true;
        }
        assert_eq!(recovery(&s), None);
    }
}
