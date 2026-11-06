//! The window's frame: sidebar, the main column the route fills, and the right panel.

pub mod sidebar;

use rune::{Cx, El, KeyEvent, Tx, row};

use crate::theme::T;
use crate::{App, Route, chat, panel, settings};

#[derive(Default)]
pub struct Ui {
    pub sidebar: sidebar::Ui,
    pub panel_open: bool,
}

pub const SCENES: &[(&str, fn(&mut App))] = &[("home", |s| crate::scene_clock(s)), ("thread-showcase", |s| {
    crate::scene_clock(s);
    if let Some(id) = s.state().threads.iter().find(|t| t["title"] == "Replay showcase").map(|t| crate::data::s(t, "id").to_string()) {
        s.navigate(Route::Thread(id));
    }
})];

pub fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    sidebar::on_key(s, e, tx)
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &T) -> El {
    let main = match &s.route {
        Route::Settings(_) => settings::view(s, cx, t),
        _ => chat::view(s, cx, t),
    };
    let mut overlays = Vec::new();
    let mut body = row().key("shell").w_full().h_full().fill(t.c.background).child(sidebar::view(s, cx, t, &mut overlays)).child(main);
    if s.ui.shell.panel_open {
        body = body.child(panel::view(s, cx, t));
    }
    rune_kit::root(body, overlays)
}
