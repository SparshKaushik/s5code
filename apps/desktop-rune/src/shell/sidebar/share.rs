//! "Share with guest" (`GuestShareDialog`) and "Turn into project" (`turnThreadIntoProject`) from
//! the thread menu.

use rune::{El, TextEdit, Tx, col, row, text};
use serde_json::{Value, json};

use super::logic::Thread;
use crate::kit::Button;
use crate::theme::{self, T};
use crate::App;

pub struct Guest {
    pub thread: String,
    pub state: GuestState,
    pub copied: bool,
}

pub enum GuestState {
    Idle,
    Creating(u64),
    Created(TextEdit),
    Failed(String),
}

/// `canShareWithGuestAtom`: the Guest links beta is on and no managed policy turns Connect off.
/// The desktop's own server session holds `access:write`.
pub fn can_share_with_guest(s: &App) -> bool {
    let config = s.state().config.as_ref();
    crate::settings::server_settings(s)["guestSharingEnabled"] == true && config.is_none_or(|c| c["managedPolicy"]["limits"]["connect"] != false)
}

/// `canTurnThreadIntoProject`: a worktree thread in the scratch project, on a server that can
/// promote it.
pub fn can_turn_into_project(s: &App, th: &Thread) -> bool {
    let Some(config) = s.state().config.as_ref() else { return false };
    if th.worktree.as_deref().is_none_or(|w| w.trim().is_empty()) || config["environment"]["capabilities"]["threadProjectFromScratch"] != true {
        return false;
    }
    s.state().projects.iter().find(|p| p["id"] == th.project_id.as_str()).is_some_and(|p| crate::shell::palette::actions::is_scratch_project(s, p))
}

pub fn open_guest(s: &mut App, thread: &str) {
    s.ui.shell.sidebar.guest = Some(Guest { thread: thread.to_string(), state: GuestState::Idle, copied: false });
}

pub fn turn_into_project(s: &mut App, thread: &str, title: &str) {
    if s.ui.shell.sidebar.to_project.is_some() {
        return;
    }
    if let Some(id) = crate::env::net_for(s, thread).map(|n| n.request("projects.createFromThread", json!({ "threadId": thread }))) {
        s.ui.shell.sidebar.to_project = Some((id, title.to_string()));
    }
}

fn create(s: &mut App) {
    let Some(thread) = s.ui.shell.sidebar.guest.as_ref().map(|g| g.thread.clone()) else { return };
    if let Some(id) = crate::env::net_for(s, &thread).map(|n| n.request("guestShare.create", json!({ "threadId": thread }))) {
        if let Some(g) = s.ui.shell.sidebar.guest.as_mut() {
            g.state = GuestState::Creating(id);
        }
    }
}

fn close(s: &mut App) {
    if !matches!(s.ui.shell.sidebar.guest.as_ref().map(|g| &g.state), Some(GuestState::Creating(_))) {
        s.ui.shell.sidebar.guest = None;
    }
}

fn reply(s: &mut App, id: u64) -> Option<Result<Value, Value>> {
    s.src.net().and_then(|n| n.take_reply(id))
}

pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    if let Some(GuestState::Creating(id)) = s.ui.shell.sidebar.guest.as_ref().map(|g| &g.state)
        && let Some(result) = reply(s, *id)
    {
        let state = match result {
            Ok(v) => GuestState::Created(TextEdit::new(crate::data::s(&v, "url").to_string())),
            Err(e) => GuestState::Failed(crate::shell::palette::error_message(&e, "Could not create a guest link.")),
        };
        if let Some(g) = s.ui.shell.sidebar.guest.as_mut() {
            g.state = state;
        }
        changed = true;
    }
    if let Some((id, title)) = s.ui.shell.sidebar.to_project.clone()
        && let Some(result) = reply(s, id)
    {
        s.ui.shell.sidebar.to_project = None;
        match result {
            Ok(_) => crate::shell::palette::toast(s, "Project created", Some(&title)),
            Err(e) => crate::shell::palette::toast_error(s, "Could not turn thread into a project", Some(&crate::shell::palette::error_message(&e, "An error occurred."))),
        }
        changed = true;
    }
    changed
}

pub fn dialog(s: &App, t: &T, overlays: &mut Vec<El>) {
    let Some(g) = s.ui.shell.sidebar.guest.as_ref() else { return };
    let active = s.state().threads.iter().find(|x| x["id"] == g.thread.as_str()).is_some_and(|x| !x["guestShare"].is_null());
    let creating = matches!(g.state, GuestState::Creating(_));
    let mut body = col().w_full().gap(8.0);
    body = match &g.state {
        GuestState::Created(url) => {
            let link = url.text.clone();
            let copy = Button::new(if g.copied { "Copied" } else { "Copy" })
                .size("sm")
                .variant("outline")
                .icon(if g.copied { &crate::icons::CHECK } else { &crate::icons::COPY })
                .build(t)
                .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                    tx.copy(&link);
                    if let Some(g) = s.ui.shell.sidebar.guest.as_mut() {
                        g.copied = true;
                    }
                });
            body.child(row().w_full().gap(8.0).center_y().child(crate::kit::input(url, "", "sm", t).key("sb-guest-link").label("Guest link").grow(1.0)).child(copy))
        }
        _ => body.child(
            text(if active { "A new link ends the current one." } else { "The link works once and expires in 24 hours." }).font(theme::TEXT_SM).color(t.c.muted_foreground),
        ),
    };
    if let GuestState::Failed(message) = &g.state {
        body = body.child(text(message.clone()).font(theme::TEXT_XS).color(t.c.destructive).role("alert"));
    }
    let footer = match &g.state {
        GuestState::Created(_) => row().w_full().justify_end().child(Button::new("Done").size("sm").on_press(|s: &mut App| s.ui.shell.sidebar.guest = None).build(t)),
        _ => row()
            .w_full()
            .gap(8.0)
            .justify_end()
            .child(Button::new("Cancel").size("sm").variant("outline").disabled(creating).on_press(close).build(t))
            .child(Button::new(if creating { "Creating..." } else { "Create link" }).size("sm").disabled(creating).on_press(create).build(t)),
    };
    crate::kit::Dialog::new("sb-guest", "Share with guest")
        .width(512.0)
        .open(true)
        .description("The guest sees the thread as you do, including command output and the agent's reasoning. Each message they send waits for you.")
        .child(body)
        .child(footer)
        .on_close(close)
        .build(t, overlays);
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::can_turn_into_project;
    use crate::shell::sidebar::logic::Thread;
    use crate::{App, data};

    #[test]
    fn only_a_worktree_thread_in_the_scratch_project_turns_into_a_project() {
        let mut s = App::new();
        let data::Source::Fixture(state) = &mut s.src else { unreachable!("App::new runs on fixtures") };
        state.config = Some(json!({"scratchWorkspaceRoot": "/root/.s5code/scratch/", "environment": {"capabilities": {"threadProjectFromScratch": true}}}));
        state.projects.push(json!({"id": "scratch", "title": "Scratch", "workspaceRoot": "/root/.s5code/scratch"}));
        state.projects.push(json!({"id": "app", "title": "app", "workspaceRoot": "/src/app"}));
        let thread = |project: &str, worktree: Option<&str>| Thread::from_shell(&json!({"id": "t", "projectId": project, "title": "Spike", "worktreePath": worktree}));
        assert!(can_turn_into_project(&s, &thread("scratch", Some("/root/.s5code/worktrees/scratch/spike"))));
        assert!(!can_turn_into_project(&s, &thread("scratch", None)), "a scratch thread without a worktree has nothing to promote");
        assert!(!can_turn_into_project(&s, &thread("scratch", Some("  "))));
        assert!(!can_turn_into_project(&s, &thread("app", Some("/src/app-wt"))), "a real project's thread already has one");
        if let data::Source::Fixture(state) = &mut s.src {
            state.config.as_mut().unwrap()["environment"]["capabilities"]["threadProjectFromScratch"] = json!(false);
        }
        assert!(!can_turn_into_project(&s, &thread("scratch", Some("/root/.s5code/worktrees/scratch/spike"))), "an older server can't");
    }
}
