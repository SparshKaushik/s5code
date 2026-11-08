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
        .children(home::banner(s, t, &lane))
        .child(composer::view(s, cx, t, &lane))
        .children(details::view(s, cx, t, shown, w))
        .children({
            let bw = details::WIDTH.min(w - 2.0 * details::GAP);
            timeline::find::bar(s, t, w - bw - details::GAP, details::GAP, bw)
        })
        .children(details::menu(s, t))
        .children(composer::overlays(s, cx, t))
        .children(timeline::actions::overlays(s, t))
        .children(timeline::attachments::lightbox(s));
    main.child(topbar::view(s, cx, t))
        .child(stack)
        .children(crate::panel::drawer::view(s, cx, t))
}

/// Chat keys no focused control took: find in thread, then a question's 1-9 shortcuts.
pub fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    timeline::attachments::on_key(s, e) || timeline::find::on_key(s, e, tx) || timeline::question::on_key(s, e, tx)
}
