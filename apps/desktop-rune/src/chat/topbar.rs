//! The chat header (ChatHeader): the project / thread breadcrumb with inline rename, and the
//! details, terminal and right panel toggles. The bar drags the window, as the Electron title
//! bar region does.

use rune::tok::TextStyle;
use rune::{Color, Cx, El, Key, TextEdit, el, row, spacer, text, text_input};
use serde_json::json;

use crate::chat::composer::{self, icon_button};
use crate::data;
use crate::icons;
use crate::theme::{self, AlphaMul, T, layout, tw};
use crate::{App, Route};

#[derive(Default)]
pub struct Ui {
    /// The thread being renamed and its title field.
    pub rename: Option<(String, TextEdit)>,
    /// The rename in flight, for its failure toast.
    rename_request: Option<u64>,
    pub terminal_open: bool,
    /// The thread footer's requests in flight (ThreadNow).
    pub now: crate::chat::timeline::now::State,
}

/// A project's monogram and colour (projectIdentity.ts).
pub fn project_identity(name: &str) -> (String, usize) {
    let name = name.trim();
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let monogram = match words.first() {
        None => "PR".to_string(),
        Some(first) => {
            let glyphs: Vec<char> = first.chars().collect();
            let second = glyphs[1..]
                .iter()
                .copied()
                .find(|c| c.is_numeric())
                .or_else(|| if words.len() > 1 { words.last().and_then(|w| w.chars().next()) } else { glyphs.last().copied() })
                .unwrap_or(glyphs[0]);
            format!("{}{}", glyphs[0], second).to_uppercase().chars().take(2).collect()
        }
    };
    let seed = if name.is_empty() { "project".to_string() } else { name.to_lowercase() };
    let mut index = 0usize;
    for c in seed.chars() {
        index = (index * 31 + c as usize) % PROJECT_COLORS.len();
    }
    (monogram, index)
}

/// projectIconColors.ts, as `(light, dark)` text colours.
const PROJECT_COLORS: [(Color, Color); 18] = [
    (tw::GRAY_600, tw::GRAY_400),
    (tw::RED_600, tw::RED_400),
    (tw::ORANGE_600, tw::ORANGE_400),
    (tw::AMBER_600, tw::AMBER_400),
    (tw::YELLOW_600, tw::YELLOW_400),
    (tw::LIME_600, tw::LIME_400),
    (tw::GREEN_600, tw::GREEN_400),
    (tw::EMERALD_600, tw::EMERALD_400),
    (tw::TEAL_600, tw::TEAL_400),
    (tw::CYAN_600, tw::CYAN_400),
    (tw::SKY_600, tw::SKY_400),
    (tw::BLUE_600, tw::BLUE_400),
    (tw::INDIGO_600, tw::INDIGO_400),
    (tw::VIOLET_600, tw::VIOLET_400),
    (tw::PURPLE_600, tw::PURPLE_400),
    (tw::FUCHSIA_600, tw::FUCHSIA_400),
    (tw::PINK_600, tw::PINK_400),
    (tw::ROSE_600, tw::ROSE_400),
];

/// The project tile (ProjectMonogram): two mono letters on a 14% wash of the project colour.
pub fn monogram(t: &T, name: &str, size: f32) -> El {
    let (letters, i) = project_identity(name);
    let color = if t.dark { PROJECT_COLORS[i].1 } else { PROJECT_COLORS[i].0 };
    el().size(size, size)
        .center()
        .radius(size * 0.25)
        .fill(color.alpha_mul(0.14))
        .child(text(letters).font(theme::mono(TextStyle::new(8.25 * size / 16.0, theme::BOLD).lh(size))).color(color))
}

fn title(s: &App) -> String {
    match composer::ctx(s).thread {
        Some(t) => data::s(t, "title").to_string(),
        None => "New thread".to_string(),
    }
}

fn open_menu(s: &mut App, at: Option<(f32, f32)>) {
    if let Route::Thread(id) = s.frame_route().clone() {
        crate::shell::sidebar::open_header_menu(s, id, at);
    }
}

pub fn start_rename(s: &mut App) {
    let Route::Thread(id) = s.frame_route().clone() else { return };
    if composer::ctx(s).thread.is_none() {
        return;
    }
    let current = title(s);
    let mut edit = TextEdit::new(current.clone());
    edit.set_selection(0, current.len());
    edit.focused = true;
    s.ui.chat.topbar.rename = Some((id, edit));
}

/// The title field, while it renames the thread on screen; a rename started on another thread
/// is not this one's.
fn renaming<'a>(rename: &'a Option<(String, TextEdit)>, route: &Route) -> Option<&'a TextEdit> {
    match (rename, route) {
        (Some((id, edit)), Route::Thread(shown)) if id == shown => Some(edit),
        _ => None,
    }
}

fn commit_rename(s: &mut App) {
    let Some((id, edit)) = s.ui.chat.topbar.rename.take() else { return };
    if s.frame_route() != &Route::Thread(id.clone()) {
        return;
    }
    let next = edit.text.trim().to_string();
    if next.is_empty() {
        crate::shell::palette::toast(s, "Thread title cannot be empty", None);
        return;
    }
    if next == title(s) {
        return;
    }
    match &mut s.src {
        crate::data::Source::Live(net) => {
            let request = net.dispatch(json!({"type": "thread.metadata.update", "commandId": s5_net::uuid(), "threadId": id, "title": next}));
            s.ui.chat.topbar.rename_request = Some(request);
        }
        crate::data::Source::Fixture(state) => {
            if let Some(t) = state.threads.iter_mut().find(|t| data::s(t, "id") == id) {
                t["title"] = json!(next);
            }
        }
    }
}

/// Drops a rename once its thread leaves the screen, commits one whose field lost focus (the
/// title input's blur), and takes the rename's reply. Also polls the thread footer.
pub fn poll(s: &mut App) -> bool {
    let mut changed = crate::chat::timeline::now::poll(s);
    let route = s.frame_route().clone();
    let rename = &s.ui.chat.topbar.rename;
    let stale = rename.is_some() && renaming(rename, &route).is_none();
    let blurred = rename.as_ref().is_some_and(|(_, edit)| !edit.focused);
    if stale {
        s.ui.chat.topbar.rename = None;
        changed = true;
    } else if blurred {
        commit_rename(s);
        changed = true;
    }
    if let Some(id) = s.ui.chat.topbar.rename_request
        && let Some(reply) = s.src.net().and_then(|net| net.take_reply(id))
    {
        s.ui.chat.topbar.rename_request = None;
        if let Err(e) = reply {
            let message = crate::shell::palette::error_message(&e, "An error occurred.");
            crate::shell::palette::toast_error_brief(s, "Failed to rename thread", Some(&message));
        }
        changed = true;
    }
    changed
}

/// The open subagent's parent (ChatHeader's parent breadcrumb): its id and title.
fn parent(s: &App) -> Option<(String, String)> {
    let thread = composer::ctx(s).thread?;
    let lineage = &thread["lineage"];
    let id = data::s(lineage, "parentThreadId");
    if id.is_empty() || data::s(lineage, "relationshipToParent") != "subagent" {
        return None;
    }
    let title = s.state().threads.iter().find(|t| data::s(t, "id") == id).map(|t| data::s(t, "title")).filter(|t| !t.is_empty()).unwrap_or("Parent thread");
    Some((id.to_string(), title.to_string()))
}

/// A header toggle's tooltip: `{label} ({shortcut})`.
fn with_shortcut(s: &App, label: &str, command: &str) -> String {
    match crate::shell::palette::shortcut_label(s, command) {
        k if k.is_empty() => label.to_string(),
        k => format!("{label} ({k})"),
    }
}

const CRUMB: TextStyle = theme::SM_MEDIUM;

pub fn view(s: &App, cx: &mut Cx<App>, t: &T) -> El {
    let project = composer::ctx(s).project;
    let scratch = project.is_some_and(|p| crate::shell::palette::actions::is_scratch_project(s, p));
    let project = if scratch { "No project".to_string() } else { project.map(|p| data::s(p, "title").to_string()).unwrap_or_default() };
    let mut crumbs = row().gap(12.0).center_y().shrink().min_w(0.0);
    if !project.is_empty() {
        let mark = if scratch { rune::icon(&icons::MESSAGE_SQUARE_DASHED, 14.0).color(t.c.muted_foreground) } else { composer::ctx(s).project.map_or_else(|| monogram(t, &project, 14.0), |p| crate::shell::sidebar::favicon::view(s, t, p, 14.0)) };
        crumbs = crumbs
            .child(
                row()
                    .key("topbar-project")
                    .gap(6.0)
                    .center_y()
                    .shrink()
                    .min_w(0.0)
                    .max_w(160.0)
                    .cursor(rune::Cursor::Pointer)
                    .focusable(true)
                    .role("button")
                    .label(format!("New thread in {project}"))
                    .help(format!("New thread in {project}"))
                    .on_press(crate::shell::sidebar::new_local)
                    .child(mark)
                    .child(text(project).font(CRUMB).color(t.c.muted_foreground).truncate().shrink()),
            )
            .child(text("/").font(theme::TEXT_SM).color(t.c.muted_foreground));
    }
    if let Some((id, parent)) = parent(s) {
        crumbs = crumbs
            .child(
                row()
                    .key("topbar-parent")
                    .center_y()
                    .shrink()
                    .min_w(0.0)
                    .max_w(160.0)
                    .hover_group()
                    .cursor(rune::Cursor::Pointer)
                    .focusable(true)
                    .role("button")
                    .label(format!("Open parent thread: {parent}"))
                    .help(parent.clone())
                    .on_press(move |s: &mut App| crate::shell::sidebar::open_thread(s, &id))
                    .child(text(parent).font(CRUMB).color(t.c.muted_foreground).group_color(t.c.foreground).truncate().shrink()),
            )
            .child(text("/").font(theme::TEXT_SM).color(t.c.muted_foreground));
    }
    let thread = composer::ctx(s).thread;
    let field = renaming(&s.ui.chat.topbar.rename, s.frame_route()).filter(|_| thread.is_some());
    let title_el = match field {
        Some(edit) => text_input(edit, CRUMB)
            .key("topbar-rename")
            .single_line()
            .grow(1.0)
            .shrink()
            .min_w(40.0)
            .px(6.0)
            .radius(6.0)
            .border(1.0, t.c.ring)
            .color(t.c.foreground)
            .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
            .label("Thread title")
            .on_edit(|s: &mut App| &mut s.ui.chat.topbar.rename.get_or_insert_with(|| (String::new(), TextEdit::new(""))).1)
            .on_submit(|s: &mut App, _| commit_rename(s))
            .on_input_key(|s: &mut App, e, _| {
                if e.key == Key::Escape {
                    s.ui.chat.topbar.rename = None;
                    return true;
                }
                false
            }),
        // A draft's "New thread" is plain text: there is no thread to act on yet.
        None if thread.is_none() => text(title(s)).key("topbar-title").font(CRUMB).color(t.c.foreground).truncate().shrink().min_w(0.0).help(title(s)),
        // ChatHeader: the title opens the thread's action menu, a double-click renames, and a
        // right-click opens the menu where it lands.
        None => row()
            .key("topbar-title")
            .gap(4.0)
            .center_y()
            .shrink()
            .min_w(0.0)
            .hover_group()
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(format!("Thread actions for {}", title(s)))
            .help(title(s))
            .child(text(title(s)).font(CRUMB).color(t.c.foreground).truncate().shrink())
            .child(rune::icon(&icons::CHEVRON_DOWN, 14.0).color(t.c.muted_foreground).group_opacity(0.0, 1.0))
            .on_press(|s: &mut App| open_menu(s, None))
            .on_double_press(|s: &mut App| {
                s.ui.shell.sidebar.menu = None;
                start_rename(s);
            })
            .on_secondary_press(|s: &mut App, at: rune::PointerPress, _: &mut rune::Tx| open_menu(s, Some((at.x, at.y)))),
    };
    let editing = field.is_some();
    crumbs = crumbs.child(title_el);
    // A thread a linked environment's agent started here.
    let origin = thread.map(|t| data::s(&t["linkOrigin"], "label")).filter(|l| !l.is_empty());
    if let Some(origin) = origin.filter(|_| !editing) {
        crumbs = crumbs.child(crate::kit::badge(&format!("From {origin}"), "info", "sm", t).help(format!("Started by an agent on {origin} through a linked environment.")));
    }
    if editing {
        crumbs = crumbs.grow(1.0);
    }

    let (w, h) = cx.layout_with("chat-body", |r| (r[2].round(), r[3].round())).unwrap_or((1182.0, 848.0));
    let fits = crate::chat::details::lane(w, h, false).card;
    let details_on = crate::chat::details::shown(s, w, h) != crate::chat::details::Shown::None;
    let mut toggles = row().gap(4.0).center_y();
    // No project, no card (the web shows its no-projects page there instead of a thread).
    if composer::ctx(s).project.is_some() {
        toggles = toggles.child(
            toggle(t, &icons::SQUARE_MENU, "Toggle thread details panel", details_on)
                .key("topbar-details")
                .help(with_shortcut(s, "Toggle thread details", "threadPanel.toggle"))
                .on_press(move |s: &mut App| crate::chat::details::toggle(s, fits)),
        );
    }
    let toggles = toggles
        .child(
            toggle(t, &icons::PANEL_BOTTOM, "Toggle terminal drawer", s.ui.chat.topbar.terminal_open)
                .help(with_shortcut(s, "Toggle terminal drawer", "terminal.toggle"))
                .on_press(|s: &mut App| {
                    crate::panel::toggle_terminal(s);
                }),
        )
        .child(
            toggle(t, &icons::PANEL_RIGHT, "Toggle right panel", s.ui.shell.panel_open)
                .help(with_shortcut(s, "Toggle right panel", "rightPanel.toggle"))
                .on_press(|s: &mut App| {
                    s.ui.shell.panel_open = !s.ui.shell.panel_open;
                }),
        );

    row()
        .key("topbar")
        .w_full()
        .h(layout::TOPBAR_H)
        .pl(20.0)
        .pr(13.0)
        .gap(12.0)
        .center_y()
        .window_drag()
        .child(crumbs)
        .children((!editing).then(spacer))
        .child(toggles)
}

/// A header toggle: the icon at 80% foreground, the accent wash while on.
fn toggle(t: &T, i: &'static rune::Icon, label: &str, on: bool) -> El {
    icon_button(t, i, label, on, t.c.foreground.alpha_mul(0.8))
}

#[cfg(test)]
mod tests {
    use super::{commit_rename, poll, start_rename};
    use crate::{App, Route, data};

    fn title_of(s: &App, id: &str) -> String {
        s.state().threads.iter().find(|t| data::s(t, "id") == id).map(|t| data::s(t, "title").to_string()).unwrap_or_default()
    }

    fn rename_to(s: &mut App, text: &str) {
        let edit = &mut s.ui.chat.topbar.rename.as_mut().expect("renaming").1;
        *edit = rune::TextEdit::new(text);
        edit.focused = true;
    }

    #[test]
    fn a_rename_only_ever_renames_the_thread_it_started_on() {
        let mut s = App::new();
        let ids: Vec<String> = s.state().threads.iter().map(|t| data::s(t, "id").to_string()).take(2).collect();
        let (a, b) = (ids[0].clone(), ids[1].clone());
        let (title_a, title_b) = (title_of(&s, &a), title_of(&s, &b));

        s.navigate(Route::Thread(a.clone()));
        start_rename(&mut s);
        rename_to(&mut s, "Renamed");
        s.navigate(Route::Thread(b.clone()));
        commit_rename(&mut s);
        assert_eq!((title_of(&s, &a), title_of(&s, &b)), (title_a.clone(), title_b.clone()), "Enter after switching threads renames neither");
        assert!(s.ui.chat.topbar.rename.is_none());

        s.navigate(Route::Thread(a.clone()));
        start_rename(&mut s);
        s.navigate(Route::Thread(b.clone()));
        poll(&mut s);
        assert!(s.ui.chat.topbar.rename.is_none(), "leaving the thread drops its rename");

        s.navigate(Route::Thread(a.clone()));
        start_rename(&mut s);
        rename_to(&mut s, "Renamed");
        s.ui.chat.topbar.rename.as_mut().expect("renaming").1.focused = false;
        poll(&mut s);
        assert_eq!(title_of(&s, &a), "Renamed", "losing focus commits");
        assert_eq!(title_of(&s, &b), title_b);
    }
}
