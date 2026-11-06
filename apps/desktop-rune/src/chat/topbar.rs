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
    /// The title being edited, while renaming.
    pub rename: Option<TextEdit>,
    pub terminal_open: bool,
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

pub fn start_rename(s: &mut App) {
    if !matches!(s.route, Route::Thread(_)) {
        return;
    }
    let current = title(s);
    let mut edit = TextEdit::new(current.clone());
    edit.set_selection(0, current.len());
    edit.focused = true;
    s.ui.chat.topbar.rename = Some(edit);
}

fn commit_rename(s: &mut App) {
    let Some(edit) = s.ui.chat.topbar.rename.take() else { return };
    let next = edit.text.trim().to_string();
    let Route::Thread(id) = s.route.clone() else { return };
    if next.is_empty() || next == title(s) {
        return;
    }
    match &mut s.src {
        crate::data::Source::Live(net) => {
            net.dispatch(json!({"type": "thread.metadata.update", "commandId": s5_net::uuid(), "threadId": id, "title": next}));
        }
        crate::data::Source::Fixture(state) => {
            if let Some(t) = state.threads.iter_mut().find(|t| data::s(t, "id") == id) {
                t["title"] = json!(next);
            }
        }
    }
}

const CRUMB: TextStyle = theme::SM_MEDIUM;

pub fn view(s: &App, _cx: &mut Cx<App>, t: &T) -> El {
    let project = composer::ctx(s).project.map(|p| data::s(p, "title").to_string()).unwrap_or_default();
    let mut crumbs = row().gap(12.0).center_y().shrink().min_w(0.0);
    if !project.is_empty() {
        crumbs = crumbs
            .child(row().gap(6.0).center_y().child(monogram(t, &project, 14.0)).child(text(project).font(CRUMB).color(t.c.muted_foreground).truncate()))
            .child(text("/").font(theme::TEXT_SM).color(t.c.muted_foreground));
    }
    let title_el = match &s.ui.chat.topbar.rename {
        Some(edit) => text_input(edit, CRUMB)
            .key("topbar-rename")
            .single_line()
            .w(320.0)
            .px(6.0)
            .radius(6.0)
            .border(1.0, t.c.ring)
            .color(t.c.foreground)
            .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
            .on_edit(|s: &mut App| s.ui.chat.topbar.rename.get_or_insert_with(|| TextEdit::new("")))
            .on_submit(|s: &mut App, _| commit_rename(s))
            .on_input_key(|s: &mut App, e, _| {
                if e.key == Key::Escape {
                    s.ui.chat.topbar.rename = None;
                    return true;
                }
                false
            }),
        None => text(title(s))
            .key("topbar-title")
            .font(CRUMB)
            .color(t.c.foreground)
            .truncate()
            .shrink()
            .help_when_truncated(title(s))
            .on_double_press(start_rename)
            .on_secondary_press(|s: &mut App, _, _| start_rename(s)),
    };
    crumbs = crumbs.child(title_el);

    let details_on = !s.ui.chat.details.hidden;
    let toggles = row()
        .gap(4.0)
        .center_y()
        .child(toggle(t, &icons::SQUARE_MENU, "Toggle thread details panel", details_on).on_press(|s: &mut App| {
            let d = &mut s.ui.chat.details;
            d.hidden = !d.hidden;
            d.menu = None;
        }))
        .child(toggle(t, &icons::PANEL_BOTTOM, "Toggle terminal drawer", s.ui.chat.topbar.terminal_open).on_press(|s: &mut App| {
            s.ui.chat.topbar.terminal_open = !s.ui.chat.topbar.terminal_open;
        }))
        .child(toggle(t, &icons::PANEL_RIGHT, "Toggle right panel", s.ui.shell.panel_open).on_press(|s: &mut App| {
            s.ui.shell.panel_open = !s.ui.shell.panel_open;
        }));

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
        .child(spacer())
        .child(toggles)
}

/// A header toggle: the icon at 80% foreground, the accent wash while on.
fn toggle(t: &T, i: &'static rune::Icon, label: &str, on: bool) -> El {
    icon_button(t, i, label, on, t.c.foreground.alpha_mul(0.8))
}
