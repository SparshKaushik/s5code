use rune::{El, row};
use serde_json::Value;

use super::worktree_setup;
use crate::kit::{self, Button, Placement, Popover};
use crate::theme::T;
use crate::{App, Route, icons};

fn key(thread: &str) -> String {
    format!("background-worktree-setup:{thread}")
}

pub(super) fn chip(app: &App, thread: &str, snapshot: &Value, t: &T) -> El {
    let script = snapshot["setupScript"]["name"].as_str().unwrap_or("Setup script");
    let open = app.ui.chat.timeline.worktree.background_open.as_deref() == Some(thread);
    let thread_id = thread.to_owned();
    row().key(format!("{}-trigger", key(thread))).child(
        Button::new("Background setup")
            .variant("ghost-muted")
            .size("micro")
            .icon(&icons::LOADER_CIRCLE)
            .on_press(move |app: &mut App| {
                let ui = &mut app.ui.chat.timeline.worktree;
                ui.background_open =
                    (ui.background_open.as_deref() != Some(&thread_id)).then(|| thread_id.clone());
                ui.background_details = false;
            })
            .build(t)
            .label(format!("Background setup is still running: {script}. Show setup progress."))
            .expanded(open),
    )
}

fn open(app: &App) -> Option<(&str, &Value)> {
    let thread = app.ui.chat.timeline.worktree.background_open.as_deref()?;
    if !matches!(app.frame_route(), Route::Thread(id) if id == thread)
        && crate::chat::side::open_beside(app) != Some(thread)
    {
        return None;
    }
    Some((thread, worktree_setup::background(app, thread)?))
}

pub(super) fn close(app: &mut App) {
    let ui = &mut app.ui.chat.timeline.worktree;
    ui.background_open = None;
    ui.background_details = false;
}

pub(super) fn poll(app: &mut App) -> bool {
    if app.ui.chat.timeline.worktree.background_open.is_some() && open(app).is_none() {
        close(app);
        return true;
    }
    false
}

pub(in crate::chat) fn overlays(app: &App, t: &T) -> Vec<El> {
    let mut layers = Vec::new();
    if let Some((thread, snapshot)) = open(app) {
        Popover::new(key(thread))
            .open(true)
            .placement(Placement::BottomEnd)
            .width("lg")
            .padding(8.0)
            .label("Worktree setup")
            .on_close(close)
            .child(worktree_setup::card(app, thread, snapshot, t, true, true).px(4.0))
            .build(&kit::theme(t), &mut layers);
    }
    layers
}
