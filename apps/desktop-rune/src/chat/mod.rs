//! The main column: topbar, then either the home/draft screen or a thread's timeline, with the
//! composer docked at the bottom. `timeline` is one worker's; `topbar`, `composer`, `home` and
//! `details` (the floating thread details card) are another's.

pub mod composer;
pub mod details;
pub mod home;
pub mod timeline;
pub mod topbar;

use rune::{Cx, El, KeyEvent, Tx, col};

use crate::theme::T;
use crate::{App, Route};

#[derive(Default)]
pub struct Ui {
    pub timeline: timeline::Ui,
    pub composer: composer::Ui,
    pub home: home::Ui,
    pub details: details::Ui,
    pub topbar: topbar::Ui,
    /// The remembered route was restored (or found stale) once the shell loaded.
    route_restored: bool,
}

const LAST_ROUTE: &str = "lastRoute";

/// Web reopens the last thread on restart. The main route is kept in client settings; once the
/// shell has loaded, a launch still on the bare home route returns to it if it still exists.
pub fn remember_route(s: &mut App) -> bool {
    if !matches!(s.src, crate::data::Source::Live(_)) || !s.state().shell_ready {
        return false;
    }
    if !s.ui.chat.route_restored {
        s.ui.chat.route_restored = true;
        let stored = s.client_settings.values.get(LAST_ROUTE).and_then(decode_route);
        if s.route == (Route::Home { project_id: None })
            && let Some(route) = stored.filter(|r| route_exists(s, r))
        {
            s.navigate(route);
            return true;
        }
    }
    let current = encode_route(s.frame_route());
    if current.is_some() && s.client_settings.values.get(LAST_ROUTE) != current.as_ref() {
        s.client_settings.values.insert(LAST_ROUTE.into(), current.unwrap_or_default());
        s.client_settings.save();
    }
    false
}

fn encode_route(route: &Route) -> Option<serde_json::Value> {
    match route {
        Route::Thread(id) => Some(serde_json::json!({"thread": id})),
        Route::Home { project_id } => Some(serde_json::json!({"home": project_id})),
        Route::PullRequests => Some(serde_json::json!({"pullRequests": true})),
        Route::Settings(_) | Route::Usage => None,
    }
}

fn decode_route(v: &serde_json::Value) -> Option<Route> {
    if let Some(id) = v["thread"].as_str() {
        return Some(Route::Thread(id.to_owned()));
    }
    if v["pullRequests"] == true {
        return Some(Route::PullRequests);
    }
    v.get("home").map(|p| Route::Home { project_id: p.as_str().map(str::to_owned) })
}

fn route_exists(s: &App, route: &Route) -> bool {
    let state = s.state();
    let has = |list: &[serde_json::Value], id: &str| list.iter().any(|v| crate::data::s(v, "id") == id);
    match route {
        Route::Thread(id) => has(&state.threads, id) || has(&state.archived, id),
        Route::Home { project_id: Some(id) } => has(&state.projects, id),
        _ => true,
    }
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &T) -> El {
    let main = col().key("main").grow(1.0).shrink().min_w(0.0).h_full().fill(t.c.background);
    // Settings opened directly (no route to return to) sits over an empty main column.
    if matches!(s.frame_route(), Route::Settings(_)) {
        return main;
    }
    let (w, h) = cx.layout_with("chat-body", |r| (r[2].round(), r[3].round())).unwrap_or((1182.0, 848.0));
    let shown = details::shown(s, w, h);
    let lane = details::lane(w, h, shown != details::Shown::Inline);
    // Under the settings modal the frame keeps showing the route settings was opened from.
    let body = match s.frame_route() {
        Route::Thread(id) if !timeline::is_empty(s, id) => timeline::view(s, cx, t, id, &lane),
        _ => home::view(cx, t, &lane),
    };
    // The composer, details card and their menus float over the body.
    let stack = col()
        .key("chat-body")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .child(body)
        .children(timeline::scroll_to_end(s, cx, t, &lane))
        .children(timeline::banners::lane(s, t, &lane))
        .children(timeline::banners::stack(s, cx, t, &lane))
        .child(composer::view(s, cx, t, &lane))
        .children(details::view(s, cx, t, shown, w))
        .children({
            let bw = details::WIDTH.min(w - 2.0 * details::GAP);
            timeline::find::bar(s, t, w - bw - details::GAP, details::GAP, bw)
        })
        .children(details::menu(s, t))
        .children(composer::overlays(s, cx, t))
        .children(timeline::actions::overlays(s, t))
        .children(timeline::plan::overlays(s, t))
        .children(timeline::chip::overlay(s, t))
        .children(timeline::attachments::lightbox(s));
    main.child(topbar::view(s, cx, t))
        .child(stack)
        .children(crate::panel::drawer::view(s, cx, t))
}

/// Chat keys no focused control took: find in thread, then a question's 1-9 shortcuts.
pub fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    timeline::attachments::on_key(s, e) || timeline::find::on_key(s, e, tx) || timeline::question::on_key(s, e, tx) || composer::shortcut(s, e) || details::shortcut(s, e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_remembered_route_round_trips() {
        for route in [Route::Thread("t1".into()), Route::Home { project_id: Some("p".into()) }, Route::Home { project_id: None }, Route::PullRequests] {
            assert_eq!(encode_route(&route).as_ref().and_then(decode_route), Some(route));
        }
        assert_eq!(encode_route(&Route::Usage), None);
        assert_eq!(decode_route(&serde_json::json!("garbage")), None);
    }
}
