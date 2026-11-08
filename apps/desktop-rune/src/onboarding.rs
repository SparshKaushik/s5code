//! First run: `NoProjectsHero`, which `/` shows instead of a draft once the shell has loaded and
//! the workspace has no projects (routes/_chat.index.tsx). The web's setup wizard (`/welcome`)
//! is not ported; this hero is what it leaves behind when dismissed.

use rune::{El, col, el, row, text};

use crate::theme::{self, T, layout};
use crate::{App, Route, icons, kit};

/// The draft route with nothing to draft in: the shell is in and every project is archived or gone.
pub fn shows_hero(s: &App) -> bool {
    let st = s.state();
    matches!(s.frame_route(), Route::Home { .. }) && st.shell_ready && !st.projects.iter().any(|p| p["archivedAt"].is_null())
}

pub fn hero(s: &App, t: &T) -> El {
    let scratch = crate::shell::palette::actions::scratch_available(s);
    let mut buttons = row().pt(24.0).gap(8.0).justify_center().child(
        kit::Button::new("Add project")
            .size("sm")
            .icon(&icons::PLUS)
            .on_press(|s: &mut App| crate::shell::palette::open_add_project(s))
            .build(t),
    );
    if scratch {
        buttons = buttons.child(
            kit::Button::new("Start without a project")
                .size("sm")
                .variant("outline")
                .icon(&icons::MESSAGE_SQUARE_DASHED)
                .on_press(crate::shell::palette::actions::new_without_project)
                .build(t),
        );
    }
    let description = if scratch { "Add a project, or start without one." } else { "Add a project to start your first thread." };
    // Empty size="hero": the title is text-3xl at desktop widths, the description text-sm, mt-1.
    let header = col()
        .w_full()
        .max_w(512.0)
        .px(32.0)
        .py(48.0)
        .items_center()
        .child(text("What should we work on?").font(theme::weight(theme::TEXT_3XL, theme::SEMIBOLD)).color(t.c.foreground))
        .child(text(description).font(theme::TEXT_SM).color(t.c.muted_foreground).mt(4.0))
        .child(buttons);
    col()
        .key("main")
        .role("main")
        .label("Home")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .h_full()
        .fill(t.c.background)
        // The desktop window only moves where it opts in, so keep a titlebar strip.
        .child(el().key("hero-titlebar").w_full().h(layout::TOPBAR_H).window_drag())
        .child(col().key("no-projects-hero").w_full().grow(1.0).min_h(0.0).padding(48.0).center().child(header))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_hero_waits_for_the_shell_and_only_counts_live_projects() {
        let mut s = App::new();
        let mut state = crate::data::empty_state();
        state.projects = vec![json!({"id": "p1", "archivedAt": "2026-01-01T00:00:00.000Z"})];
        s.src = crate::data::Source::Fixture(state);
        assert!(!shows_hero(&s), "a pending shell cannot prove the workspace is empty");
        if let crate::data::Source::Fixture(state) = &mut s.src {
            state.shell_ready = true;
        }
        assert!(shows_hero(&s));
        if let crate::data::Source::Fixture(state) = &mut s.src {
            state.projects.push(json!({"id": "p2", "archivedAt": null}));
        }
        assert!(!shows_hero(&s));
    }
}
