//! The two "nothing here" states: ChatView's NoActiveThreadState for a thread no environment
//! lists, and the root route's "Page not found" for a settings page this client doesn't have.

use rune::tok::TextStyle;
use rune::{El, col, row, text};

use crate::theme::{self, AlphaMul, T, layout};
use crate::{App, Route, data};

/// `Empty`'s default title (17/24 semibold) and description (13/18).
const TITLE: TextStyle = theme::weight(theme::ts(17.0, 24.0, 400.0), 600.0);
const DESCRIPTION: TextStyle = theme::ts(13.0, 18.0, 400.0);

/// A thread route whose thread is gone: the shell has loaded, no environment lists it, and its
/// timeline has nothing to show (a thread that just launched has its own messages).
pub fn missing_thread(s: &App, id: &str) -> bool {
    let st = s.state();
    let listed = st.threads.iter().chain(&st.archived).any(|t| data::s(t, "id") == id);
    let has_rows = st.thread_views.get(id).is_some_and(|v| v.synchronized) && !crate::chat::timeline::is_empty(s, id);
    st.shell_ready && !listed && !has_rows
}

/// `NoActiveThreadState`: the header says so, and the body asks for a thread.
pub fn no_active_thread(t: &T) -> El {
    let header = row()
        .key("topbar")
        .w_full()
        .h(layout::TOPBAR_H)
        .pl(20.0)
        .pr(13.0)
        .center_y()
        .window_drag()
        .child(text("No active thread").font(theme::TEXT_XS).color(t.c.muted_foreground.alpha_mul(0.5)));
    col()
        .key("no-active-thread")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(header)
        .child(rune::el().h(1.0).w_full().fill(t.c.border))
        .child(
            col().grow(1.0).min_h(0.0).center().px(32.0).py(48.0).child(
                col()
                    .max_w(512.0)
                    .gap(4.0)
                    .items_center()
                    .child(text("Pick a thread to continue").font(TITLE).color(t.c.foreground))
                    .child(text("Select an existing thread or create a new one to get started.").font(DESCRIPTION).color(t.c.muted_foreground).wrap()),
            ),
        )
}

/// A settings route this client has no page for, as a link can name.
pub fn unknown_page(route: &Route) -> bool {
    matches!(route, Route::Settings(page) if !crate::settings::has_page(page))
}

/// `RootRouteNotFoundView`.
pub fn page_not_found(t: &T) -> El {
    col().key("main").grow(1.0).min_w(0.0).h_full().fill(t.c.background).center().padding(24.0).child(
        col()
            .max_w(384.0)
            .gap(16.0)
            .items_center()
            .child(text("Page not found").font(theme::weight(theme::TEXT_LG, 500.0)).color(t.c.foreground))
            .child(
                text("This link doesn't point to a page in T3 Code. Go home to choose a project or start a thread.")
                    .font(theme::TEXT_SM)
                    .color(t.c.muted_foreground)
                    .wrap(),
            )
            .child(crate::kit::Button::new("Go home").on_press(|s: &mut App| s.navigate(Route::Home { project_id: None })).build(t)),
    )
}

fn scene_ready(s: &mut App) {
    crate::scene_clock(s);
    if let crate::data::Source::Fixture(st) = &mut s.src {
        st.shell_ready = true;
    }
}

pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("no-active-thread", |s| {
        scene_ready(s);
        s.route = Route::Thread("deleted-thread".into());
    }),
    ("page-not-found", |s| {
        scene_ready(s);
        s.route = Route::Settings("nonsense".into());
    }),
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_thread_is_missing_only_once_the_shell_has_loaded_without_it() {
        let mut s = App::new();
        let crate::data::Source::Fixture(st) = &mut s.src else { panic!("scenes run on fixtures") };
        st.shell_ready = false;
        st.threads = vec![json!({"id": "t1"})];
        st.archived = vec![json!({"id": "t2"})];
        assert!(!missing_thread(&s, "gone"), "still loading");
        let crate::data::Source::Fixture(st) = &mut s.src else { unreachable!() };
        st.shell_ready = true;
        assert!(missing_thread(&s, "gone"));
        assert!(!missing_thread(&s, "t1"));
        assert!(!missing_thread(&s, "t2"), "archived threads still open");
        assert!(unknown_page(&Route::Settings("nonsense".into())));
        assert!(!unknown_page(&Route::Settings("general".into())));
        assert!(!unknown_page(&Route::Home { project_id: None }));
    }
}
