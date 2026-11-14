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
    /// Whether the details card fits inline in the chat lane, from the last layout, for the
    /// `threadPanel.toggle` chord (a key handler can't read layout).
    pub details_fits: std::cell::Cell<bool>,
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
    ("home-local-off", |s| {
        crate::scene_clock(s);
        let mut state = crate::data::empty_state();
        state.conn = s5_net::Conn::Closed(crate::host::LOCAL_OFF.into());
        state.shell_ready = true;
        s.src = crate::data::Source::Fixture(state);
        s.ui.desktop.local_off = true;
        s.navigate(crate::Route::Home { project_id: None });
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
    chat::details::on_key(s, e) || palette::on_key(s, e, tx) || workspace::on_key(s, e, tx) || panel::on_key(s, e, tx) || chat_shortcut(s, e) || sidebar::on_key(s, e, tx)
}

/// The chords the web's ChatView answers anywhere on a thread: `threadPanel.toggle` flips the
/// details card, `editor.openFavorite` opens the checkout in the card's editor.
fn chat_shortcut(s: &mut App, e: &KeyEvent) -> bool {
    if !matches!(s.route, Route::Thread(_) | Route::Home { .. }) || crate::onboarding::shows_hero(s) {
        return false;
    }
    if chat::composer::ctx(s).project.is_some() && panel::shortcut(s, e, "threadPanel.toggle") {
        let fits = s.ui.shell.details_fits.get();
        chat::details::toggle(s, fits);
        return true;
    }
    panel::shortcut(s, e, "editor.openFavorite") && chat::details::open_favorite(s)
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &T) -> El {
    if s.ui.shell.palette.gallery.is_some() {
        return crate::kit::gallery::view(s, cx, t);
    }
    if let Some(recovery) = crate::onboarding::recovery(s) {
        return rune_kit::root(crate::onboarding::recovery_view(recovery, t), Vec::new());
    }
    let mut overlays = Vec::new();
    s.ui.shell.details_fits.set(cx.layout_with("chat-body", |r| chat::details::lane(r[2].round(), r[3].round(), false).card).unwrap_or(true));
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
    crate::links::overlays(s, t, &mut overlays);
    crate::add_to_machine::overlays(s, t, &mut overlays);
    crate::wizard::overlays(s, t, &mut overlays);
    overlays.extend(quit::overlay(s, t));
    rune_kit::root(body, overlays)
        .on_key_up(quit::on_key_up)
        .on_window_focus(|s: &mut App, focused, _| crate::notify::window_focus(s, focused))
}

#[cfg(test)]
mod tests {
    use rune::{Key, KeyEvent};

    use crate::chat::details::{Shown, shown};
    use crate::{App, Route, data};

    #[test]
    fn the_thread_panel_chord_toggles_the_details_card_as_the_lane_fits() {
        let mut s = App::new();
        let id = s.state().threads.iter().find(|t| !data::s(t, "projectId").is_empty()).map(|t| data::s(t, "id").to_owned()).unwrap();
        if let data::Source::Fixture(state) = &mut s.src {
            state.config.get_or_insert_with(|| serde_json::json!({}))["keybindings"] =
                serde_json::json!([{"command": "threadPanel.toggle", "shortcut": {"key": "b", "modKey": true, "shiftKey": true}}]);
        }
        s.navigate(Route::Thread(id));
        let mut chord = KeyEvent::from(Key::Command('b'));
        chord.mods.cmd = true;
        chord.mods.ctrl = !cfg!(target_os = "macos");
        chord.mods.shift = true;
        let (wide, narrow) = ((1182.0, 848.0), (900.0, 848.0));
        s.ui.shell.details_fits.set(true);
        assert_eq!(shown(&s, wide.0, wide.1), Shown::Inline);
        assert!(super::chat_shortcut(&mut s, &chord));
        assert_eq!(shown(&s, wide.0, wide.1), Shown::None, "the chord closes the inline card");
        assert!(super::chat_shortcut(&mut s, &chord));
        assert_eq!(shown(&s, wide.0, wide.1), Shown::Inline, "and opens it again");
        s.ui.shell.details_fits.set(false);
        assert!(super::chat_shortcut(&mut s, &chord));
        assert_eq!(shown(&s, narrow.0, narrow.1), Shown::Popover, "a narrow lane opens the popover");
        s.navigate(Route::PullRequests);
        assert!(!super::chat_shortcut(&mut s, &chord), "off a thread the chord falls through");
    }
}
