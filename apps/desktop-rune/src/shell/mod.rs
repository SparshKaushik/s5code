//! The window's frame: sidebar, the main column the route fills, and the right panel.

pub mod chatgpt_welcome;
pub mod confirm;
pub mod lifecycle;
pub mod sidebar;
pub mod palette;
pub mod quit;
pub mod slow_requests;
pub mod swipe;
pub mod toast;
pub(crate) mod workspace;

use rune::{Cx, El, KeyEvent, Tx, row};

use crate::theme::T;
use crate::{App, Route, chat, pages, panel, settings};

#[derive(Default)]
pub struct Ui {
    pub sidebar: sidebar::Ui,
    pub palette: palette::Ui,
    pub panel_open: bool,
    pub confirm: confirm::Ui,
    pub quit: quit::QuitHold,
    pub toasts: toast::Stack,
    pub swipe: swipe::Swipe,
    pub chatgpt_welcome: chatgpt_welcome::Ui,
    pub lifecycle: lifecycle::Ui,
    pub slow_requests: slow_requests::Ui,
    /// The appearance last drawn, for the theme editor chord.
    pub dark: std::cell::Cell<bool>,
}

/// The references open on an empty "New thread" (d01) and on "Replay showcase" (d02).
pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("home", |s| open_titled(s, "New thread")),
    ("thread-showcase", |s| open_titled(s, "Replay showcase")),
    ("history-swipe-back", |s| {
        open_titled(s, "Replay showcase");
        swipe::scene(s, true, 72.0);
    }),
    ("history-swipe-forward", |s| {
        open_titled(s, "Replay showcase");
        swipe::scene(s, false, 140.0);
    }),
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
    ("toasts-stack", |s| {
        open_titled(s, "Replay showcase");
        toast::scene(s, false);
    }),
    ("toasts-expanded", |s| {
        open_titled(s, "Replay showcase");
        toast::scene(s, true);
    }),
    ("chatgpt-welcome", |s| {
        open_titled(s, "Replay showcase");
        chatgpt_welcome::scene(s);
    }),
    ("lifecycle-toasts", |s| {
        open_titled(s, "Replay showcase");
        lifecycle::scene(s);
        slow_requests::scene(s);
    }),
    ("palette-publish-host", |s| {
        open_titled(s, "Replay showcase");
        palette::add_project::scene_host(s);
    }),
    ("update-everything-confirm", |s| sidebar::everything::scene(s, "confirm")),
    ("update-capsule", |s| sidebar::everything::scene(s, "capsule")),
    ("update-release-notes", |s| sidebar::everything::scene(s, "notes")),
    ("sidebar-regenerating", |s| {
        open_titled(s, "Replay showcase");
        if let crate::data::Source::Fixture(state) = &mut s.src {
            for t in state.threads.iter_mut().filter(|t| t["title"] != "Replay showcase").take(3) {
                t["titleRegeneration"] = serde_json::json!({"requestedAt": "2026-01-01T00:00:00.000Z"});
            }
        }
        palette::toast(s, "Thread search is unavailable on this server.", Some("Update the server to enable it."));
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
    chat::details::on_key(s, e) || palette::on_key(s, e, tx) || theme_editor_key(s, e) || workspace::on_key(s, e, tx) || panel::on_key(s, e, tx) || sidebar::on_key(s, e, tx)
}

/// `themeEditor.toggle` on any route. Escape closes the editor from its own key handler while it has focus.
fn theme_editor_key(s: &mut App, e: &KeyEvent) -> bool {
    if !panel::shortcut(s, e, "themeEditor.toggle") {
        return false;
    }
    let dark = s.ui.shell.dark.get();
    settings::themes::toggle_editor(s, dark);
    true
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &T) -> El {
    if s.ui.shell.palette.gallery.is_some() {
        return crate::kit::gallery::view(s, cx, t);
    }
    if let Some(recovery) = crate::onboarding::recovery(s) {
        return rune_kit::root(crate::onboarding::recovery_view(recovery, t), Vec::new());
    }
    let mut overlays = Vec::new();
    s.ui.shell.dark.set(t.dark);
    let sidebar = sidebar::view(s, cx, t, &mut overlays);
    let main = match s.frame_route() {
        _ if crate::not_found::unknown_page(&s.route) => crate::not_found::page_not_found(t),
        Route::PullRequests => pages::pull_requests::view(s, cx, t, &mut overlays),
        // Usage opened with nothing to return to sits over an empty main column.
        Route::Usage => rune::col().key("main").grow(1.0).h_full().fill(t.c.background),
        _ if crate::onboarding::shows_hero(s) => crate::onboarding::hero(s, t),
        _ => chat::view(s, cx, t),
    };
    overlays.extend(chat::details::overlays(s, t));
    if swipe::tracking(s) {
        overlays.extend(swipe::indicator(s, cx.layout_with("main", |r| r), t));
    }
    // Everything under the frame is content: switching or loading a thread lands at once.
    // A maximized right panel takes the chat column's room (`w-0` and inert on the web).
    let maximized = panel::maximized(s, cx.layout_with("shell", |r| r[2]));
    let mut body = crate::kit::motion::content(row().key("shell").w_full().h_full().fill(t.c.background).child(sidebar).children((!maximized).then_some(main)));
    if s.ui.shell.panel_open {
        body = body.child(panel::view(s, cx, t, &mut overlays));
    }
    if matches!(s.route, Route::Settings(_)) && !crate::not_found::unknown_page(&s.route) {
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
    if !matches!(s.route, Route::Settings(_)) {
        settings::themes::editor(s, t, &mut overlays);
    }
    overlays.extend(panel::overlays(s, t));
    overlays.extend(palette::overlay(s, cx, t));
    confirm::view(s, t, &mut overlays);
    chatgpt_welcome::view(s, t, &mut overlays);
    overlays.extend(crate::search::overlay(s, t));
    crate::links::overlays(s, t, &mut overlays);
    crate::add_to_machine::overlays(s, t, &mut overlays);
    crate::wizard::overlays(s, t, &mut overlays);
    crate::cloud::onboarding::overlay(s, t, &mut overlays);
    crate::cloud::relay_client::overlay(s, t, &mut overlays);
    crate::desktop::updates::confirm(s, t, &mut overlays);
    overlays.extend(quit::overlay(s, t));
    rune_kit::root(body, overlays)
        .on_key_up(quit::on_key_up)
        .on_window_focus(|s: &mut App, focused, _| crate::notify::window_focus(s, focused))
}

#[cfg(test)]
mod tests {
    use rune::{Key, KeyEvent};

    use crate::{App, Route, data};

    #[test]
    fn the_theme_editor_chord_opens_on_the_theme_in_use_and_closes() {
        let mut s = App::new();
        if let data::Source::Fixture(state) = &mut s.src {
            state.config.get_or_insert_with(|| serde_json::json!({}))["keybindings"] =
                serde_json::json!([{"command": "themeEditor.toggle", "shortcut": {"key": "t", "modKey": true, "altKey": true, "shiftKey": true}}]);
        }
        s.client_settings.values.insert("themeDark".into(), serde_json::json!("grove"));
        s.navigate(Route::PullRequests);
        s.ui.shell.dark.set(true);
        let mut chord = KeyEvent::from(Key::Command('t'));
        chord.mods.cmd = true;
        chord.mods.ctrl = !cfg!(target_os = "macos");
        chord.mods.alt = true;
        chord.mods.shift = true;
        assert!(super::theme_editor_key(&mut s, &chord), "FAILED: the chord works off Settings");
        let editing = s.ui.settings.theme_editor.clone().expect("FAILED: the editor opens");
        assert!(data::s(&editing, "label").starts_with("Grove"), "FAILED: seeded from the dark theme in use, got {editing}");
        assert!(s.ui.settings.theme_edit_dark);
        assert!(super::theme_editor_key(&mut s, &chord));
        assert!(s.ui.settings.theme_editor.is_none(), "FAILED: the chord closes it again");
    }
}
