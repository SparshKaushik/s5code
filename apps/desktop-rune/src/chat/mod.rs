//! The main column: topbar, then either the home/draft screen or a thread's timeline, with the
//! composer docked at the bottom. `timeline` is one worker's; `topbar`, `composer`, `home` and
//! `details` (the floating thread details card) are another's.

pub mod composer;
pub mod details;
pub mod home;
pub mod timeline;
pub mod topbar;

use rune::{Cx, El, col};

use crate::theme::T;
use crate::{App, Route};

#[derive(Default)]
pub struct Ui {
    pub timeline: timeline::Ui,
    pub composer: composer::Ui,
    pub home: home::Ui,
    pub details: details::Ui,
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &T) -> El {
    let route = if matches!(s.route, Route::Settings(_)) {
        s.ui.settings.return_route.as_ref().unwrap_or(&s.route)
    } else { &s.route };
    let body = match route {
        Route::Thread(id) => timeline::view(s, cx, t, id),
        _ => home::view(s, cx, t),
    };
    col()
        .key("main")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .h_full()
        .fill(t.c.background)
        .child(topbar::view(s, cx, t))
        .child(body)
        .child(composer::view(s, cx, t))
}
