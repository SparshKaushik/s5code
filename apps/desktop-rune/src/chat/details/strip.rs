//! ComposerSurface.ContextStrip (BranchToolbar's composer layout): the machine, workspace and
//! branch the next send uses, in a plain row under the composer. A new thread always shows it;
//! a started one only with "Keep composer context visible" (`persistComposerContextStrip`).
//! Its pickers are the card's menus, opened upward from the strip.

use rune::{Cursor, El, el, icon, row, spacer, text};
use serde_json::Value;

use super::{Menu, env_label, hosts, run_on, toggle_menu, vcs, with_shortcut, workspace_mode};
use crate::chat::composer;
use crate::theme::{self, AlphaMul, T};
use crate::{App, data, icons};

pub(super) const ENV: &str = "strip-env";
pub(super) const WORKSPACE: &str = "strip-workspace";
pub(super) const BRANCH: &str = "strip-branch";

/// The checkout's local status while it is a repository (`isGitRepo`).
fn repo(s: &App) -> Option<&Value> {
    vcs(s).map(|v| &v["local"]).filter(|v| v["isRepo"] == true)
}

/// shouldShowEnvironmentIndicator: a choice of machines, or a project away from this one.
fn indicator(s: &App) -> bool {
    let Some(project) = composer::ctx(s).project else { return false };
    !hosts(s).is_empty() || !crate::add_to_machine::targets(s, data::s(project, "id")).is_empty() || crate::env::of(s, project) != crate::env::LOCAL
}

/// shouldShowComposerContextStrip: a new thread names what its first send uses.
pub(crate) fn shown(s: &App) -> bool {
    let c = composer::ctx(s);
    c.project.is_some() && (c.thread.is_none() || (crate::settings::value(s, "persistComposerContextStrip") == true && (repo(s).is_some() || indicator(s))))
}

/// ComposerControl `size="xs"`: a 12px glyph and a text-xs label capped at 240px.
fn control(t: &T, key: &str, glyph: &'static rune::Icon, label: &str) -> El {
    let color = if t.dark { t.c.muted_foreground } else { t.c.foreground.alpha_mul(0.7) };
    row()
        .key(key.to_owned())
        .h(24.0)
        .px(7.0)
        .gap(6.0)
        .center_y()
        .shrink()
        .min_w(0.0)
        .radius(theme::layout::CONTROL_RADIUS)
        .child(icon(glyph, 12.0).color(color))
        .child(text(label.to_owned()).font(theme::TEXT_XS).color(color).single_line().truncate().max_w(240.0).shrink().min_w(0.0))
}

/// A control that opens one of the card's menus above itself.
fn opens(s: &App, t: &T, control: El, menu: Menu, anchor: &'static str) -> El {
    let ui = &s.ui.chat.details;
    control.hover(t.c.accent).cursor(Cursor::Pointer).role("button").expanded(ui.menu == Some(menu) && ui.strip == Some(anchor)).on_press(move |s: &mut App| {
        toggle_menu(s, menu);
        let ui = &mut s.ui.chat.details;
        ui.strip = ui.menu.map(|_| anchor);
    })
}

pub(crate) fn view(s: &App, t: &T) -> Option<El> {
    if !shown(s) {
        return None;
    }
    let draft = composer::ctx(s).thread.is_none();
    let fanout = composer::fanout::forces_worktree(s);
    let git = repo(s);
    let mut lead = row().gap(4.0).center_y().shrink().min_w(0.0);
    if indicator(s) {
        let auto = run_on::auto_label(s);
        let env = auto.map_or_else(|| env_label(s), str::to_owned);
        let control = control(t, ENV, if auto.is_some() { &icons::SCALE } else { &icons::SERVER }, &env).label(format!("Environment: {env}"));
        lead = lead.child(if draft { opens(s, t, control, Menu::RunContext, ENV) } else { control.help(env.clone()) });
        if git.is_some() {
            lead = lead.child(el().w(1.0).h(14.0).mx(2.0).fill(t.c.border));
        }
    }
    if git.is_some() {
        let (mode, new_worktree, worktree) = workspace_mode(s);
        let glyph = if new_worktree { &icons::FOLDER_GIT_2 } else if worktree { &icons::FOLDER_GIT } else { &icons::FOLDER };
        let control = control(t, WORKSPACE, glyph, mode).label(format!("Workspace: {mode}"));
        lead = lead.child(if draft && !fanout {
            opens(s, t, control, Menu::RunContext, WORKSPACE).help(with_shortcut(s, mode, "composer.workspace"))
        } else {
            control.opacity(0.7).help(if fanout { "Each model starts in its own worktree." } else { mode }.to_string())
        });
    }
    let branch = git.map(|v| {
        let label = super::branch_label(s, v);
        opens(s, t, control(t, BRANCH, &icons::GIT_BRANCH, &label).label(format!("Branch: {label}")), Menu::Branch, BRANCH).help(with_shortcut(s, &label, "composer.branch"))
    });
    let permissions = composer::strip_permissions(s, t);
    let gap = (branch.is_some() && permissions.is_some()).then(spacer);
    Some(row().key("composer-context-strip").w_full().px(8.0).pt(8.0).gap(4.0).center_y().child(lead).child(spacer()).children(branch).children(gap).children(permissions))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Route;
    use serde_json::json;

    #[test]
    fn a_started_thread_keeps_the_strip_only_with_the_setting() {
        let mut s = App::new();
        let id = s.state().thread_views.iter().find(|(_, v)| data::s(&v.projection["thread"], "title") == "Replay showcase").map(|(id, _)| id.clone()).unwrap();
        let project = data::s(&s.state().thread_views[&id].projection["thread"], "projectId").to_owned();
        s.navigate(Route::Home { project_id: Some(project) });
        assert!(shown(&s), "a new thread names where its first send runs");
        s.navigate(Route::Thread(id));
        assert!(repo(&s).is_some());
        assert!(!shown(&s));
        s.client_settings.values.insert("persistComposerContextStrip".into(), json!(true));
        assert!(shown(&s));
    }
}
