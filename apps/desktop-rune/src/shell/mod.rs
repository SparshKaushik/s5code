//! The window's frame: sidebar, the main column the route fills, and the right panel.

pub mod sidebar;
pub mod palette;
pub mod quit;
mod workspace;

use rune::{Cx, El, KeyEvent, Tx, row};

use crate::theme::T;
use crate::{App, Route, chat, pages, panel, settings};

#[derive(Default)]
pub struct Ui {
    pub sidebar: sidebar::Ui,
    pub palette: palette::Ui,
    pub panel_open: bool,
    pub quit: quit::QuitHold,
}

/// The references open on an empty "New thread" (d01) and on "Replay showcase" (d02).
pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("home", |s| open_titled(s, "New thread")),
    ("thread-showcase", |s| open_titled(s, "Replay showcase")),
    ("palette", |s| {
        open_titled(s, "Replay showcase");
        s.ui.shell.palette.open = true;
        s.ui.shell.palette.query.focused = true;
    }),
];

fn open_titled(s: &mut App, title: &str) {
    crate::scene_clock(s);
    if let Some(id) = s.state().threads.iter().find(|t| t["title"] == title).map(|t| crate::data::s(t, "id").to_string()) {
        s.navigate(Route::Thread(id));
    }
}

pub fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    sidebar::release_undo_key(s, e);
    chat::details::on_key(s, e) || palette::on_key(s, e, tx) || workspace::on_key(s, e, tx) || panel::on_key(s, e, tx) || sidebar::on_key(s, e, tx)
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &T) -> El {
    if s.ui.shell.palette.gallery.is_some() {
        return crate::kit::gallery::view(s, cx, t);
    }
    if let Some(recovery) = crate::onboarding::recovery(s) {
        return rune_kit::root(crate::onboarding::recovery_view(recovery, t), Vec::new());
    }
    let mut overlays = Vec::new();
    let sidebar = sidebar::view(s, cx, t, &mut overlays);
    let main = match s.frame_route() {
        Route::PullRequests => pages::pull_requests::view(s, cx, t, &mut overlays),
        // Usage opened with nothing to return to sits over an empty main column.
        Route::Usage => rune::col().key("main").grow(1.0).h_full().fill(t.c.background),
        _ if crate::onboarding::shows_hero(s) => crate::onboarding::hero(s, t),
        _ => chat::view(s, cx, t),
    };
    overlays.extend(chat::details::overlays(s, t));
    // Everything under the frame is content: switching or loading a thread lands at once.
    let mut body = crate::kit::motion::content(row().key("shell").w_full().h_full().fill(t.c.background).child(sidebar).child(main));
    if s.ui.shell.panel_open {
        body = body.child(panel::view(s, cx, t));
    }
    if matches!(s.route, Route::Settings(_)) {
        // The web mounts settings as a route modal over the inert previous frame.
        body = body.inert();
        overlays.clear();
        overlays.push(settings::view(s, cx, t));
    }
    if matches!(s.route, Route::Usage) {
        body = body.inert();
        overlays.clear();
        overlays.push(pages::usage::view(s, cx, t));
    }
    overlays.extend(panel::overlays(s, t));
    overlays.extend(palette::overlay(s, cx, t));
    overlays.extend(crate::search::overlay(s, t));
    overlays.extend(crate::notify::overlays(s, t));
    overlays.extend(quit::overlay(s, t));
    rune_kit::root(body, overlays)
        .on_key_up(quit::on_key_up)
        .on_window_focus(|s: &mut App, focused, _| crate::notify::window_focus(s, focused))
}
