//! Global command overlay. Navigation stays local; mutations go through Source::net.
use crate::{
    App, Appearance, Route, data, icons, kit,
    theme::{self, AlphaMul, T},
};
use rune::{Cx, El, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col, el, icon, row, text, text_input};

#[derive(Default)]
pub struct Ui {
    pub open: bool,
    pub query: TextEdit,
    pub selected: usize,
    pub page: String,
    pub gallery: Option<&'static str>,
    pub gallery_edit: TextEdit,
    pub gallery_on: bool,
    pub gallery_menu: bool,
}
#[derive(Clone)]
enum Action {
    Navigate(Route),
    Page(&'static str),
    Theme(Appearance),
    Restart(String),
}
struct Item {
    title: String,
    group: &'static str,
    glyph: &'static rune::Icon,
    shortcut: &'static str,
    action: Action,
    subtitle: Option<String>,
    timestamp: Option<String>,
}
fn item(
    title: impl Into<String>,
    group: &'static str,
    glyph: &'static rune::Icon,
    shortcut: &'static str,
    action: Action,
) -> Item {
    Item {
        title: title.into(),
        group,
        glyph,
        shortcut,
        action,
        subtitle: None,
        timestamp: None,
    }
}

fn items(s: &App) -> Vec<Item> {
    let ui = &s.ui.shell.palette;
    let mut out = vec![];
    if ui.page == "themes" {
        return [
            ("System", Appearance::System),
            ("Light", Appearance::Light),
            ("Dark", Appearance::Dark),
        ]
        .into_iter()
        .map(|(label, mode)| {
            item(
                label,
                "Change theme",
                &icons::PALETTE,
                "",
                Action::Theme(mode),
            )
        })
        .collect();
    }
    if ui.page.is_empty() {
        let project = match &s.route {
            Route::Thread(id) => s
                .state()
                .threads
                .iter()
                .find(|v| data::s(v, "id") == id)
                .map(|v| data::s(v, "projectId")),
            Route::Home { project_id } => project_id.as_deref(),
            _ => None,
        };
        if let Some(p) =
            project.and_then(|id| s.state().projects.iter().find(|v| data::s(v, "id") == id))
        {
            out.push(item(
                format!("New thread in {}", data::s(p, "title")),
                "Actions",
                &icons::SQUARE_PEN,
                "⇧⌘N",
                Action::Navigate(Route::Home {
                    project_id: Some(data::s(p, "id").into()),
                }),
            ));
        }
        out.push(item(
            "New thread in...",
            "Actions",
            &icons::SQUARE_PEN,
            "",
            Action::Page("projects"),
        ));
        out.push(item(
            "New thread without a project",
            "Actions",
            &icons::MESSAGE_SQUARE_DASHED,
            "",
            Action::Navigate(Route::Home { project_id: None }),
        ));
        out.push(item(
            "Open settings",
            "Actions",
            &icons::SETTINGS,
            "⌘,",
            Action::Navigate(Route::Settings("general".into())),
        ));
        out.push(item(
            "Change theme",
            "Actions",
            &icons::PALETTE,
            "",
            Action::Page("themes"),
        ));
        if let Route::Thread(id) = &s.route {
            out.push(item(
                "Restart agent session",
                "Actions",
                &icons::ROTATE_CCW,
                "",
                Action::Restart(id.clone()),
            ));
        }
        let mut threads: Vec<_> = s
            .state()
            .threads
            .iter()
            .filter(|v| v["archivedAt"].is_null())
            .collect();
        threads.sort_by_key(|v| std::cmp::Reverse(data::ts(v, "updatedAt").unwrap_or(0)));
        let limit = if ui.query.text.trim().is_empty() {
            12
        } else {
            threads.len()
        };
        for v in threads.into_iter().take(limit) {
            out.push(item(
                data::s(v, "title"),
                if ui.query.text.trim().is_empty() {
                    "Recent Threads"
                } else {
                    "Threads"
                },
                &icons::MESSAGE_SQUARE,
                "",
                Action::Navigate(Route::Thread(data::s(v, "id").into())),
            ));
            if let Some(row) = out.last_mut() {
                let project = s
                    .state()
                    .projects
                    .iter()
                    .find(|p| data::s(p, "id") == data::s(v, "projectId"))
                    .map(|p| data::s(p, "title"))
                    .unwrap_or("");
                row.subtitle = Some(format!("{project} · Local · ∞"));
                let minutes =
                    ((s.now_ms - data::ts(v, "updatedAt").unwrap_or(s.now_ms)) / 60_000).max(0);
                row.timestamp = Some(if minutes == 0 {
                    "now".into()
                } else if minutes < 60 {
                    format!("{minutes}m ago")
                } else if minutes < 1440 {
                    format!("{}h ago", minutes / 60)
                } else {
                    format!("{}d ago", minutes / 1440)
                });
            }
        }
    }
    if ui.page == "projects" || !ui.query.text.trim().is_empty() {
        for p in &s.state().projects {
            out.push(item(
                data::s(p, "title"),
                "Projects",
                &icons::FOLDER_GIT_2,
                "",
                Action::Navigate(Route::Home {
                    project_id: Some(data::s(p, "id").into()),
                }),
            ));
        }
    }
    if !ui.query.text.trim().is_empty() && ui.page.is_empty() {
        for (label, page) in [
            ("General", "general"),
            ("Appearance", "appearance"),
            ("Keybindings", "keybindings"),
            ("Providers", "providers"),
            ("Connections", "connections"),
        ] {
            out.push(item(
                label,
                "Settings",
                &icons::SETTINGS,
                "",
                Action::Navigate(Route::Settings(page.into())),
            ));
        }
    }
    let query = ui.query.text.trim().to_lowercase();
    if !query.is_empty() {
        let mut scored: Vec<_> = out
            .into_iter()
            .filter_map(|v| score(&v.title, &query).map(|rank| (v, rank)))
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        out = scored.into_iter().map(|(v, _)| v).collect();
    }
    out
}
/// Word starts, camel-case boundaries, prefixes, joined words and one-edit typo matching.
/// Mirrors the primary-name pass in packages/shared/src/paletteSearch.ts.
fn score(source: &str, query: &str) -> Option<f32> {
    let chars: Vec<_> = source.chars().collect();
    let folded: Vec<_> = source.to_lowercase().chars().collect();
    let starts: Vec<bool> = chars
        .iter()
        .enumerate()
        .map(|(i, c)| {
            i == 0
                || !chars[i - 1].is_alphanumeric()
                || (chars[i - 1].is_lowercase() && c.is_uppercase())
                || (i + 1 < chars.len()
                    && chars[i - 1].is_uppercase()
                    && c.is_uppercase()
                    && chars[i + 1].is_lowercase())
                || (chars[i - 1].is_numeric() != c.is_numeric())
        })
        .collect();
    let mut total = 0.;
    let mut count = 0.;
    for token in query.split_whitespace() {
        let needle: Vec<_> = token.chars().collect();
        let mut best: f32 = 0.;
        for i in 0..folded.len() {
            if folded[i..].starts_with(&needle) {
                if starts[i] {
                    let end = i + needle.len();
                    let whole =
                        end == folded.len() || starts[end] || !folded[end].is_alphanumeric();
                    let run = folded[i..]
                        .iter()
                        .take_while(|c| c.is_alphanumeric())
                        .count();
                    best = best.max(if whole {
                        1.
                    } else {
                        0.7 + 0.2 * needle.len() as f32 / run as f32
                    });
                } else if needle.len() >= 4 {
                    best = best.max(0.3);
                }
            }
            if starts[i]
                && needle.len() >= 3
                && (folded[i] == needle[0]
                    || (needle.len() > 3 && needle.len() > 1 && folded[i] == needle[1]))
            {
                let run: Vec<_> = folded[i..]
                    .iter()
                    .copied()
                    .take_while(|c| c.is_alphanumeric())
                    .collect();
                for len in [
                    needle.len(),
                    needle.len() + 1,
                    needle.len().saturating_sub(1),
                ] {
                    if len <= run.len()
                        && (needle.len() > 4 || len >= needle.len())
                        && one_edit(
                            &needle,
                            &run[..len],
                            needle.len() <= 4 && len == needle.len(),
                        )
                    {
                        best = best.max(0.45);
                    }
                }
            }
        }
        let joined: String = source
            .split(|c: char| !c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase();
        if joined.starts_with(token) {
            best = best.max(0.6);
        }
        if best == 0. {
            if [
                "a", "an", "and", "for", "in", "of", "on", "the", "to", "with",
            ]
            .contains(&token)
            {
                continue;
            }
            return None;
        }
        total += best;
        count += 1.;
    }
    if count == 0. {
        return None;
    }
    Some(
        total / count
            + if source.to_lowercase() == query {
                0.5
            } else if query.contains(' ') && source.to_lowercase().contains(query) {
                0.25
            } else {
                0.
            },
    )
}
fn one_edit(a: &[char], b: &[char], swap_only: bool) -> bool {
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let i = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[i..], &b[i..]);
    let swap = a.len() > 1 && b.len() > 1 && a[0] == b[1] && a[1] == b[0] && a[2..] == b[2..];
    swap || (!swap_only
        && (a == b
            || (a.len() > 0 && a[1..] == *b)
            || (b.len() > 0 && *a == b[1..])
            || (a.len() > 0 && b.len() > 0 && a[1..] == b[1..])))
}
fn execute(s: &mut App, action: Action) {
    match action {
        Action::Page(page) => {
            let ui = &mut s.ui.shell.palette;
            ui.page = page.into();
            ui.query = TextEdit::default();
            ui.query.focused = true;
            ui.selected = 0;
            return;
        }
        Action::Navigate(route) => s.navigate(route),
        Action::Theme(mode) => s.appearance = mode,
        Action::Restart(id) => {
            let sessions = s
                .state()
                .thread_views
                .get(&id)
                .and_then(|v| v.projection["providerSessions"].as_array())
                .cloned()
                .unwrap_or_default();
            if let Some(net) = s.src.net() {
                for session in sessions {
                    net.dispatch(serde_json::json!({"type":"provider-session.detach","commandId":s5_net::uuid(),"threadId":id,"providerSessionId":session["id"],"reason":"client-requested"}));
                }
            }
        }
    }
    s.ui.shell.palette.open = false;
}
fn toggle_key(s: &App, e: &KeyEvent) -> bool {
    if let Some(bindings) = s
        .state()
        .config
        .as_ref()
        .and_then(|v| v["keybindings"].as_array())
    {
        return bindings
            .iter()
            .filter(|v| v["command"] == "commandPalette.toggle")
            .any(|v| {
                let k = &v["shortcut"];
                let yes = |name: &str| k[name].as_bool().unwrap_or(false);
                let key = k["key"].as_str().unwrap_or("");
                let matches = match &e.key {
                    Key::Command(c) => key == c.to_string(),
                    Key::Text(c) => key == c,
                    _ => false,
                };
                matches
                    && e.mods.cmd == (yes("modKey") || yes("metaKey"))
                    && e.mods.shift == yes("shiftKey")
                    && e.mods.alt == yes("altKey")
                    && (!yes("ctrlKey") || e.mods.ctrl)
            });
    }
    e.is_command('k') && !e.mods.shift && !e.mods.alt
}
pub fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if toggle_key(s, e) {
        let ui = &mut s.ui.shell.palette;
        ui.open = !ui.open;
        ui.query = TextEdit::default();
        ui.page.clear();
        ui.selected = 0;
        if ui.open {
            ui.query.focused = true;
        }
        return true;
    }
    if !s.ui.shell.palette.open {
        return false;
    }
    let rows = items(s);
    let ui = &mut s.ui.shell.palette;
    match e.key {
        Key::Escape => {
            if !ui.page.is_empty() {
                ui.page.clear();
                ui.query = TextEdit::default();
            } else {
                ui.open = false;
            }
        }
        Key::Backspace if ui.query.text.is_empty() && !ui.page.is_empty() => {
            ui.page.clear();
            ui.selected = 0;
        }
        Key::Down => {
            ui.selected = if rows.is_empty() {
                0
            } else {
                (ui.selected + 1) % rows.len()
            }
        }
        Key::Up => {
            ui.selected = if rows.is_empty() {
                0
            } else {
                (ui.selected + rows.len() - 1) % rows.len()
            }
        }
        Key::Enter => {
            if let Some(item) = rows.get(ui.selected) {
                execute(s, item.action.clone());
            }
            return true;
        }
        _ => return false,
    }
    tx.scroll_into_view(format!("palette-item-{}", s.ui.shell.palette.selected));
    true
}
pub fn overlay(s: &App, _cx: &mut Cx<App>, t: &T) -> Vec<El> {
    if !s.ui.shell.palette.open {
        return vec![];
    }
    let ui = &s.ui.shell.palette;
    let rows = items(s);
    let input = text_input(&ui.query, theme::TEXT_SM)
        .key("palette-query")
        .w_full()
        .h(34.)
        .py(7.)
        .font(theme::TEXT_SM)
        .color(t.c.contrast_foreground)
        .placeholder(
            "Search commands, projects, and threads...",
            t.c.contrast_placeholder,
        )
        .on_input_key(on_key)
        .on_edit(|s: &mut App| &mut s.ui.shell.palette.query)
        .on_input_change(|s: &mut App, _: &mut Tx| s.ui.shell.palette.selected = 0);
    let head = row()
        .w_full()
        .h(48.)
        .px(17.)
        .gap(12.)
        .center_y()
        .child(icon(&icons::SEARCH, 16.).color(t.c.contrast_icon_muted))
        .child(input);
    let mut list = col().w_full().padding(8.);
    let mut group = "";
    for (i, v) in rows.iter().enumerate() {
        if group != v.group {
            group = v.group;
            list = list.child(
                row().w_full().h(28.).px(8.).center_y().child(
                    text(group)
                        .font(theme::XS_MEDIUM)
                        .color(t.c.contrast_muted_foreground),
                ),
            );
        }
        let action = v.action.clone();
        let mut label = col().grow(1.).child(
            text(v.title.clone())
                .font(theme::TEXT_SM)
                .color(t.c.contrast_foreground),
        );
        if let Some(subtitle) = &v.subtitle {
            label = label.child(
                text(subtitle)
                    .font(theme::TEXT_XS)
                    .color(t.c.contrast_muted_foreground),
            );
        }
        let mut entry = row()
            .key(format!("palette-item-{i}"))
            .role("option")
            .label(v.title.clone())
            .w_full()
            .h(if v.subtitle.is_some() { 50. } else { 32. })
            .px(8.)
            .gap(8.)
            .center_y()
            .radius(6.)
            .fill(if ui.selected == i {
                t.c.accent
            } else {
                rune::Color::TRANSPARENT
            })
            .hover(t.c.accent)
            .on_press(move |s: &mut App| execute(s, action.clone()))
            .child(icon(v.glyph, 16.).color(t.c.contrast_icon_muted))
            .child(label);
        if let Some(timestamp) = &v.timestamp {
            entry = entry.child(
                text(timestamp)
                    .font(theme::TEXT_XS)
                    .color(t.c.contrast_muted_foreground),
            );
        }
        if !v.shortcut.is_empty() {
            entry = entry.child(
                text(v.shortcut)
                    .font(theme::XS_MEDIUM)
                    .color(t.c.contrast_secondary_label),
            );
        }
        if matches!(v.action, Action::Page(_)) {
            entry = entry.child(icon(&icons::CHEVRON_RIGHT, 14.).color(t.c.contrast_icon_muted));
        }
        list = list.child(entry);
    }
    if rows.is_empty() {
        list = list.child(kit::empty("No results found.", "", t));
    }
    let list = kit::scroll_area(
        "palette-results",
        list.max_h(330.).scroll(ScrollSpec {
            stick_bottom: false,
        }),
        t,
    );
    let footer = row()
        .w_full()
        .h(41.)
        .px(16.)
        .gap(6.)
        .center_y()
        .fill(t.c.contrast_foreground.alpha_mul(0.025))
        .border(1., t.c.contrast_foreground.alpha_mul(0.07))
        .child(kit::kbd("↑", t).min_w(20.))
        .child(kit::kbd("↓", t).min_w(20.))
        .child(
            text("Navigate")
                .font(theme::TEXT_SM)
                .color(t.c.contrast_muted_foreground),
        )
        .child(kit::kbd("Enter", t))
        .child(
            text("Select")
                .font(theme::TEXT_SM)
                .color(t.c.contrast_muted_foreground),
        )
        .child(kit::kbd("Esc", t))
        .child(
            text("Close")
                .font(theme::TEXT_SM)
                .color(t.c.contrast_muted_foreground),
        );
    let panel = col()
        .key("command-palette")
        .role("dialog")
        .label("Command palette")
        .transition(rune::Transition::scale(0.98).combined(rune::Transition::opacity()))
        .w(576.)
        .max_h(420.)
        .radius(16.)
        .clip_rounded(16.)
        .fill(t.c.background.alpha_mul(0.8))
        .backdrop_blur(16.)
        .border(1., t.c.contrast_foreground.alpha_mul(0.1))
        .child(head)
        .child(list)
        .child(footer)
        .modal(true, |s: &mut App| s.ui.shell.palette.open = false);
    vec![
        el().key("palette-backdrop")
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .fill(t.c.background.alpha_mul(if t.dark { 0.64 } else { 0.6 }))
            .backdrop_blur(4.),
        col()
            .key("palette-layer")
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .pt(90.)
            .items_center()
            .child(panel),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_match_word_starts_joined_words_and_restricted_typos() {
        for (name, query, expected) in [
            ("GitHub", "githbu", true),
            ("Open settings", "settings", true),
            ("VS Code", "vscode", true),
            ("JSONSchema", "schema", true),
            ("Search", "arch", true),
            ("Model", "del", false),
            ("PR 123", "124", false),
            ("Thread", "xyz", false),
        ] {
            assert_eq!(score(name, query).is_some(), expected, "{name} / {query}");
        }
        assert!(
            score("Settings", "settings").unwrap() > score("Open settings", "settings").unwrap()
        );
    }
    #[test]
    fn configured_shortcut_replaces_default_and_requires_exact_modifiers() {
        let mut s = App::new();
        s.src = crate::data::Source::Fixture({
            let mut state = crate::data::fixture_state();
            state.config = Some(
                serde_json::json!({"keybindings":[{"command":"commandPalette.toggle","shortcut":{"key":"p","modKey":true,"shiftKey":true}}]}),
            );
            state
        });
        let mut e = KeyEvent {
            key: Key::Command('p'),
            mods: rune::Mods {
                cmd: true,
                shift: true,
                ..Default::default()
            },
        };
        assert!(toggle_key(&s, &e));
        e.mods.shift = false;
        assert!(!toggle_key(&s, &e));
        e.key = Key::Command('k');
        assert!(!toggle_key(&s, &e));
    }
}
