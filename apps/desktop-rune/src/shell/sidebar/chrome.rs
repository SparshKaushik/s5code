//! The sidebar's frame around the list: brand header, search, footer, shelf headers, drag
//! markers and search results (`SidebarChromeHeader`, `SidebarSearchControl`, `SidebarFooter`,
//! `SidebarListMarkers`).

use rune::{Color, Cursor, El, ScrollSpec, Tx, col, el, icon, row, spacer, text, text_input};

use super::brand;
use super::logic::{self, Section};
use super::{Model, Shelf};
use crate::icons as i;
use crate::theme::{self, AlphaMul, T, layout, radius};
use crate::{App, Route};

/// A ghost icon button: `rounded-[8]`, transparent until hovered (`row_hover`) or pressed.
pub fn icon_button(t: &T, key: &str, glyph: &'static rune::Icon, size: f32, icon_size: f32, color: Color, active: bool) -> El {
    el().key(key)
        .size(size, size)
        .radius(layout::CONTROL_RADIUS)
        .center()
        .fill(if active { t.sb.sidebar_row_active } else { Color::TRANSPARENT })
        .hover(t.sb.sidebar_row_hover)
        .press(t.sb.sidebar_row_active)
        .hover_group()
        .cursor(Cursor::Pointer)
        .child(icon(glyph, icon_size).color(if active { t.sb.sidebar_foreground } else { color }).group_color(t.sb.sidebar_foreground))
}

/// The brand row under the traffic lights: collapse toggle, wordmark, back and forward.
pub fn header(s: &App, t: &T, inset: f32) -> El {
    let left = brand_left(inset);
    let ui = &s.ui.shell.sidebar;
    let nav = |key: &str, glyph, enabled: bool| {
        let b = el().key(key).size(28.0, 28.0).radius(layout::CONTROL_RADIUS).center().child(icon(glyph, 14.0).color(t.c.muted_foreground));
        if enabled { b.hover(t.sb.sidebar_row_hover).press(t.sb.sidebar_row_active).cursor(Cursor::Pointer) } else { b.opacity(0.5) }
    };
    el().key("sb-header")
        .w_full()
        .h(layout::TOPBAR_H)
        .shrink()
        .window_drag()
        .child(
            icon_button(t, "sb-toggle", &brand::PANEL_LEFT_CLOSE, layout::TITLEBAR_CONTROL, 16.0, t.c.muted_foreground, false)
                .absolute(left, 12.0)
                .on_press(|s: &mut App| s.ui.shell.sidebar.collapsed = true),
        )
        .child(icon(&brand::T3_WORDMARK, 16.84).color(t.sb.sidebar_foreground).absolute(left + 39.0, 20.92))
        .child(text("Code").font(theme::SM_MEDIUM).color(t.sb.muted_foreground).absolute(left + 59.84, 16.0))
        .child(nav("sb-back", &i::ARROW_LEFT, !ui.back.is_empty()).absolute(188.45, 12.0).on_press(super::go_back))
        .child(nav("sb-forward", &i::ARROW_RIGHT, !ui.forward.is_empty()).absolute(216.45, 12.0).on_press(super::go_forward))
}

/// Shown in place of the header while the sidebar is collapsed.
pub fn floating_toggle(t: &T, inset: f32) -> El {
    let left = brand_left(inset);
    icon_button(t, "sb-expand", &brand::PANEL_LEFT, layout::TITLEBAR_CONTROL, 16.0, t.c.muted_foreground, false)
        .absolute(left, 12.0)
        .transition(rune::Transition::none())
        .on_press(|s: &mut App| s.ui.shell.sidebar.collapsed = false)
}

fn search_edit(s: &mut App) -> &mut rune::TextEdit {
    &mut s.ui.shell.sidebar.search
}

pub fn search_bar(s: &App, t: &T) -> El {
    let ui = &s.ui.shell.sidebar;
    let focused = ui.search.focused;
    let searching = !ui.search.text.is_empty();
    let fg = t.sb.sidebar_foreground;
    let field = text_input(&ui.search, theme::SM_MEDIUM)
        .key("sb-search-input")
        .single_line()
        .mt(1.0)
        .grow(1.0)
        .color(fg)
        .placeholder("Search", t.sb.sidebar_muted_foreground)
        .caret(fg, t.c.primary.alpha_mul(0.3))
        .on_edit(search_edit)
        .on_input_key(|s: &mut App, e, tx| super::search_key(s, e, tx));
    let mut bar = row()
        .key("sb-search")
        .h(32.0)
        .grow(1.0)
        .radius(radius::MD)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .cursor(Cursor::Text)
        .child(icon(&i::SEARCH, 16.0).color(t.sb.sidebar_icon_color))
        .child(field);
    bar = if focused { bar.fill(t.sb.sidebar_row_hover).border(1.0, t.sb.ring.alpha_mul(0.7)) } else { bar.hover(t.sb.sidebar_row_hover) };
    if searching {
        bar = bar.child(
            el().key("sb-search-clear")
                .size(20.0, 20.0)
                .radius(radius::SM)
                .center()
                .hover(t.sb.sidebar_row_hover)
                .cursor(Cursor::Pointer)
                .child(icon(&i::X, 12.0).color(t.sb.sidebar_muted_foreground))
                .on_press(super::clear_search),
        );
    }
    row().key("sb-search-row")
        .w_full()
        .shrink()
        .pt(8.0)
        .px(8.0)
        .gap(4.0)
        .center_y()
        .child(bar)
        .child(
            row()
                .child(filter_button(s, t))
                .child(icon_button(t, "sb-add-project", &i::FOLDER_PLUS, 28.0, 16.0, t.sb.sidebar_icon_color, false).on_press(crate::shell::palette::open_add_project))
                .child(icon_button(t, "sb-new-thread", &i::SQUARE_PEN, 28.0, 16.0, t.sb.sidebar_icon_color, false).on_press(super::new_thread)),
        )
}

/// The scope trigger: the filter glyph, or the scoped groups' favicons (two at most, overlapped).
fn filter_button(s: &App, t: &T) -> El {
    let scope = &s.ui.shell.sidebar.project_scope;
    let mut b = icon_button(t, "sb-filter", &i::LIST_FILTER, 28.0, 16.0, t.sb.sidebar_icon_color, s.ui.shell.sidebar.scope_open).role("button");
    if scope.is_empty() {
        return b.label("Filter threads by project").on_press(super::toggle_scope);
    }
    let (groups, _) = super::folders::group_of(s);
    let scoped: Vec<_> = groups.iter().filter(|g| g.members.iter().any(|m| scope.contains(m))).collect();
    let names: Vec<&str> = scoped.iter().map(|g| g.label.as_str()).collect();
    let face = |g: &super::folders::Group, size: f32| match s.state().projects.iter().find(|p| crate::data::s(p, "id") == g.representative) {
        Some(p) => super::favicon::view(s, t, p, size),
        None => super::favicon::mark(s, t, &g.representative, &g.label, false, size),
    };
    let faces = match scoped.as_slice() {
        [] => None,
        [one] => Some(row().child(face(one, 16.0))),
        [a, b, ..] => Some(row().child(face(a, 14.0)).child(face(b, 14.0).ml(-6.0))),
    };
    if let Some(faces) = faces {
        b = el().key("sb-filter").size(28.0, 28.0).radius(layout::CONTROL_RADIUS).center().hover(t.sb.sidebar_row_hover).press(t.sb.sidebar_row_active).cursor(Cursor::Pointer).role("button").child(faces);
    }
    b.label(format!("Filter threads by project: {}", names.join(", "))).on_press(super::toggle_scope)
}

/// The project scope picker under the filter button (the web's project scope `Combobox`): a
/// search field over "All projects" and the project groups. A plain pick scopes to one group,
/// Shift adds or removes one, and each group row carries its pin and settings buttons.
pub fn scope_menu(s: &App, _model: &Model, t: &T, overlays: &mut Vec<El>) {
    let ui = &s.ui.shell.sidebar;
    if !ui.scope_open {
        return;
    }
    let (groups, _) = super::folders::group_of(s);
    let can_pin = crate::shell::palette::actions::can_organize_projects(s);
    let ordered: Vec<&super::folders::Group> = ui.scope_order.iter().filter_map(|k| groups.iter().find(|g| g.key == *k)).collect();
    let shown = scope_items(&ordered.iter().map(|g| g.label.as_str()).collect::<Vec<_>>(), &ui.scope_search.text);
    let keys: Vec<String> = shown.iter().map(|n| n.map_or_else(|| ALL.to_string(), |n| ordered[n].key.clone())).collect();
    let active = ui.scope_active.as_ref().and_then(|k| keys.iter().position(|v| v == k)).or((!keys.is_empty()).then_some(0));
    let pin_label = crate::shell::palette::shortcut_label(s, "thread.pin");
    let k = crate::kit::theme(t);

    let mut rows = col().w_full().padding(4.0);
    for (n, item) in shown.iter().enumerate() {
        let key = keys[n].clone();
        let highlighted = active == Some(n);
        let (label, selected, glyph) = match item {
            None => ("All projects".to_string(), ui.project_scope.is_empty(), icon(&i::FOLDER, 16.0).color(t.c.muted_foreground)),
            Some(g) => {
                let g = ordered[*g];
                let on = !g.members.is_empty() && g.members.iter().all(|m| ui.project_scope.contains(m));
                let mark = match s.state().projects.iter().find(|p| crate::data::s(p, "id") == g.representative) {
                    Some(p) => super::favicon::view(s, t, p, 16.0),
                    None => super::favicon::mark(s, t, &g.representative, &g.label, false, 16.0),
                };
                (g.label.clone(), on, mark)
            }
        };
        let mut entry = row()
            .key(format!("sb-scope-{key}"))
            .role("option")
            .label(label.clone())
            .checked(selected)
            .focus_selected(highlighted)
            .w_full()
            .h(28.0)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(4.0)
            .fill(if highlighted { t.c.foreground.alpha_mul(0.09) } else { Color::TRANSPARENT })
            .cursor(Cursor::Pointer)
            .child(scope_icon(t, glyph, selected))
            .child(text(label).font(theme::TEXT_SM).color(t.c.foreground).grow(1.0).truncate());
        if let Some(g) = item.map(|n| ordered[n]) {
            let remote = s.state().projects.iter().find(|p| crate::data::s(p, "id") == g.representative).and_then(|p| crate::env::remote_label(s, p));
            if let Some(remote) = remote {
                entry = entry.child(icon(&i::SERVER, 14.0).color(t.c.muted_foreground).help(format!("On {remote}")));
            }
            if can_pin {
                entry = entry.child(scope_pin(t, g, highlighted, &pin_label));
            }
            entry = entry.child(scope_settings(t, g));
            let project = g.representative.clone();
            entry = entry.on_secondary_press(move |s: &mut App, _: rune::PointerPress, _: &mut Tx| open_project_settings(s, &project));
        }
        let (hover_key, members) = (key.clone(), item.map(|n| ordered[n].members.clone()));
        entry = entry
            .on_hover(move |s: &mut App| s.ui.shell.sidebar.scope_active = Some(hover_key.clone()))
            .on_press_tx(move |s: &mut App, tx: &mut Tx| pick_scope(s, members.as_deref(), tx.mods().shift));
        rows = rows.child(entry);
    }
    let list = col()
        .key("sb-scope-results")
        .w_full()
        .min_h(0.0)
        .shrink()
        .role("listbox")
        .label("Projects")
        .preserve_focus_on_press(true)
        .scroll(ScrollSpec { stick_bottom: false })
        .child(if shown.is_empty() {
            row().w_full().padding(8.0).justify_center().child(text("No matching projects.").font(theme::TEXT_SM).color(t.c.muted_foreground))
        } else {
            rows
        });

    let (field_keys, field_groups) = (keys.clone(), ordered.iter().map(|g| (g.key.clone(), g.members.clone(), g.representative.clone(), g.pinned)).collect::<Vec<_>>());
    let mut field = text_input(&ui.scope_search, theme::TEXT_SM)
        .key("sb-scope-search")
        .w_full()
        .h(26.0)
        .pl(20.0)
        .py(3.0)
        .color(t.c.foreground)
        .placeholder("Search projects...", t.c.muted_foreground)
        .label("Search projects");
    if let Some(n) = active {
        field = field.active_descendant(format!("sb-scope-{}", keys[n]));
    }
    let field = field
        .on_edit(|s: &mut App| &mut s.ui.shell.sidebar.scope_search)
        .on_input_change(|s: &mut App, _: &mut Tx| s.ui.shell.sidebar.scope_active = None)
        .on_input_key(move |s: &mut App, e: &rune::KeyEvent, tx: &mut Tx| {
            if s.ui.shell.sidebar.scope_search.preedit.is_some() {
                return false;
            }
            let at = active.map(|n| field_keys[n].clone());
            let group = at.as_ref().and_then(|k| field_groups.iter().find(|g| g.0 == *k));
            if let Some((_, members, _, pinned)) = group
                && can_pin
                && crate::panel::shortcut(s, e, "thread.pin")
            {
                for m in members {
                    crate::shell::palette::actions::pin_project(s, m, !pinned);
                }
                return true;
            }
            if e.mods.ctrl || e.mods.alt || e.mods.logo || e.mods.cmd {
                return false;
            }
            match &e.key {
                rune::Key::Escape => {
                    if !tx.key_repeat() {
                        super::toggle_scope(s);
                    }
                }
                rune::Key::Enter if !tx.key_repeat() => {
                    if at.is_some() {
                        pick_scope(s, group.map(|g| g.1.as_slice()), e.mods.shift);
                    }
                }
                rune::Key::Down | rune::Key::Up if !field_keys.is_empty() => {
                    let step = if e.key == rune::Key::Down { 1 } else { -1 };
                    let next = (active.unwrap_or(0) as isize + step).rem_euclid(field_keys.len() as isize) as usize;
                    tx.scroll_into_view(format!("sb-scope-{}", field_keys[next]));
                    s.ui.shell.sidebar.scope_active = Some(field_keys[next].clone());
                }
                rune::Key::Function(10) if e.mods.shift => {
                    if let Some((_, _, project, _)) = group {
                        open_project_settings(s, project);
                    }
                }
                _ => return false,
            }
            true
        });
    let search = col()
        .w_full()
        .px(12.0)
        .pt(10.0)
        .child(
            row()
                .w_full()
                .pb(6.0)
                .child(icon(&i::SEARCH, 16.0).absolute(0.0, 6.0).color(t.c.muted_foreground.alpha_mul(0.55)))
                .child(field),
        )
        .child(el().w_full().h(1.0).fill(t.c.ring));
    let body = col().key("sb-scope").role("dialog").label("Filter threads by project").w(288.0).min_h(0.0).max_h(360.0).child(search).child(list);
    let close = |s: &mut App| {
        if s.ui.shell.sidebar.scope_open {
            super::toggle_scope(s);
        }
    };
    overlays.push(rune_kit::scrim("sb-scope-scrim", close));
    let surface = crate::kit::menu::glass(body, &k, false).focus_return("sb-filter").modal(true, close);
    overlays.push(crate::kit::motion::popup(rune_kit::popover::anchored(&k, surface, "sb-filter", rune_kit::Placement::BottomStart, 4.0)));
}

const ALL: &str = "all";

/// `filterProjectScopeItems`: every row for an empty query, else the groups whose name contains
/// it, without "All projects". `None` is "All projects", `Some(n)` the nth group.
pub fn scope_items(labels: &[&str], query: &str) -> Vec<Option<usize>> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return std::iter::once(None).chain((0..labels.len()).map(Some)).collect();
    }
    (0..labels.len()).filter(|n| labels[*n].to_lowercase().contains(&query)).map(Some).collect()
}

/// A plain pick scopes to the group (or all projects) and closes; Shift adds or removes the
/// group and keeps the picker open.
fn pick_scope(s: &mut App, members: Option<&[String]>, shift: bool) {
    let ui = &mut s.ui.shell.sidebar;
    match (members, shift) {
        (Some(members), true) => {
            let on = members.iter().all(|m| ui.project_scope.contains(m));
            ui.project_scope.retain(|x| !members.contains(x));
            if !on {
                ui.project_scope.extend(members.iter().cloned());
            }
        }
        (Some(members), false) => ui.project_scope = members.to_vec(),
        (None, _) => ui.project_scope.clear(),
    }
    ui.selected.clear();
    ui.settled_pages = 0;
    if !(shift && members.is_some()) {
        super::toggle_scope(s);
    }
}

fn open_project_settings(s: &mut App, project: &str) {
    if s.ui.shell.sidebar.scope_open {
        super::toggle_scope(s);
    }
    s.ui.settings.project = Some(project.to_string());
    super::go(s, Route::Settings("projects".into()));
}

/// `ProjectScopeRowIcon`: the row's icon, badged with a check while that scope is on.
fn scope_icon(t: &T, glyph: El, selected: bool) -> El {
    let mut e = el().shrink().child(glyph);
    if selected {
        let badge = el()
            .size(12.0, 12.0)
            .radius(6.0)
            .center()
            .fill(t.c.primary)
            .border(2.0, t.c.popover)
            .child(icon(&i::CHECK, 8.0).color(t.c.primary_foreground));
        e = e.child(badge.pin(None, None, Some(-4.0), Some(-4.0)));
    }
    e
}

/// `ProjectPinButton`: pins every member of the group. Unpinned it shows on the highlighted
/// row; pinned it stays and turns into PinOff under the pointer.
fn scope_pin(t: &T, g: &super::folders::Group, highlighted: bool, shortcut: &str) -> El {
    let (members, pinned) = (g.members.clone(), g.pinned);
    let color = t.c.muted_foreground;
    let action = if pinned { "Unpin" } else { "Pin" };
    let tip = if shortcut.is_empty() { format!("{action} {}", g.label) } else { format!("{action} {} ({shortcut})", g.label) };
    let glyph = |glyph| icon(glyph, 14.0).color(color).group_color(t.c.foreground);
    let face = if pinned {
        el().child(glyph(&i::PIN).group_opacity(1.0, 0.0)).child(glyph(&i::PIN_OFF).group_opacity(0.0, 1.0).pin(Some(0.0), Some(0.0), None, None))
    } else {
        el().child(glyph(&i::PIN)).opacity(if highlighted { 1.0 } else { 0.0 })
    };
    el().key(format!("sb-scope-pin-{}", g.key))
        .size(24.0, 24.0)
        .ml(4.0)
        .center()
        .radius(radius::SM)
        .hover(t.c.foreground.alpha_mul(0.09))
        .hover_group()
        .cursor(Cursor::Pointer)
        .help(tip)
        .child(face)
        .on_press(move |s: &mut App| {
            for m in &members {
                crate::shell::palette::actions::pin_project(s, m, !pinned);
            }
        })
}

/// The row's settings button: opens the project's settings and closes the picker.
fn scope_settings(t: &T, g: &super::folders::Group) -> El {
    let project = g.representative.clone();
    el().key(format!("sb-scope-settings-{}", g.key))
        .size(24.0, 24.0)
        .center()
        .radius(radius::SM)
        .hover(t.c.foreground.alpha_mul(0.09))
        .hover_group()
        .cursor(Cursor::Pointer)
        .help(format!("Project settings for {}", g.label))
        .child(icon(&i::SETTINGS, 14.0).color(t.c.muted_foreground).group_color(t.c.foreground))
        .on_press(move |s: &mut App| open_project_settings(s, &project))
}

pub fn footer(s: &App, model: &Model, t: &T) -> El {
    let btn = |key: &str, glyph| icon_button(t, key, glyph, 32.0, 16.0, t.sb.sidebar_icon_color, false);
    // SidebarChrome swaps these for "Back" only on utility pages (/projects/*, /pull-requests);
    // settings and usage are modals over the frame, so the footer stays.
    let menu = if matches!(s.frame_route(), Route::PullRequests) {
        row()
            .key("sb-back")
            .grow(1.0)
            .h(32.0)
            .px(10.0)
            .gap(8.0)
            .radius(layout::CONTROL_RADIUS)
            .center_y()
            .hover(t.sb.sidebar_row_hover)
            .press(t.sb.sidebar_row_active)
            .cursor(Cursor::Pointer)
            .role("button")
            .label("Back")
            .child(icon(&i::ARROW_LEFT, 16.0).color(t.sb.sidebar_icon_color))
            .child(text("Back").font(theme::SM_MEDIUM).color(t.sb.sidebar_muted_foreground))
            .on_press(crate::pages::back)
    } else {
        let mut menu = row().gap(4.0).child(btn("sb-settings", &i::SETTINGS).on_press(|s: &mut App| super::go(s, Route::Settings("general".into()))));
        if crate::pages::pull_requests::supported(s) {
            menu = menu.child(btn("sb-pull-requests", &i::GIT_PULL_REQUEST_ARROW).label("Pull requests").on_press(|s: &mut App| super::go(s, Route::PullRequests)));
        }
        menu.child(btn("sb-usage", &i::CHART_NO_AXES_COLUMN).label("Usage").on_press(|s: &mut App| super::go(s, Route::Usage)))
    };
    let mut bar = row().key("sb-footer").w_full().shrink().px(8.0).pt(4.0).pb(8.0).center_y().child(menu).child(undo_notice(s, t));
    if model.updates > 0 {
        bar = bar.child(
            el().key("sb-update")
                .size(32.0, 32.0)
                .radius(radius::FULL)
                .center()
                .fill(t.sb.sidebar_control_surface)
                .hover(t.sb.sidebar_row_hover)
                .press(t.sb.sidebar_row_active)
                .cursor(Cursor::Pointer)
                .label(format!("Update {} provider{}", model.updates, if model.updates == 1 { "" } else { "s" }))
                .child(icon(&i::CIRCLE_ARROW_UP, 16.0).color(t.sb.sidebar_foreground))
                .on_press(|s: &mut App| super::go(s, Route::Settings("providers".into()))),
        );
    }
    match super::updates::pill(s, t) {
        Some(pill) => rune::col().w_full().shrink().child(rune::el().w_full().px(8.0).pt(8.0).child(pill)).child(bar),
        None => bar,
    }
}

/// `SidebarThreadUndoNotice`: fills the footer's free space, right-aligned, while an action can
/// be undone.
fn undo_notice(s: &App, t: &T) -> El {
    let Some((action, count)) = s.ui.shell.sidebar.undos.notice(crate::data::now_ms()) else { return spacer() };
    let action = format!("{action:?}");
    let summary = if count == 1 { action } else { format!("{action} {count}") };
    let shortcut = crate::shell::palette::shortcut_label(s, "thread.undo");
    let label = if shortcut.is_empty() { "Undo".to_string() } else { format!("{shortcut} to undo") };
    row()
        .key("sb-undo")
        .grow(1.0)
        .shrink()
        .h(24.0)
        .gap(4.0)
        .center_y()
        .child(spacer())
        .child(text(format!("{summary},")).font(theme::TEXT_2XS).color(t.sb.sidebar_muted_foreground))
        .child(
            text(label.clone())
                .font(theme::TEXT_2XS)
                .color(t.sb.sidebar_muted_foreground)
                .cursor(Cursor::Pointer)
                .role("button")
                .label(label)
                .on_press(|s: &mut App| {
                    super::undo(s);
                }),
        )
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    Muted,
    Emphasized,
    Accent,
}

/// The web insets the brand by `--desktop-window-controls-inset` (90px) past the traffic
/// lights, wider than the native inset, so a window with traffic lights uses that offset.
fn brand_left(inset: f32) -> f32 {
    if inset > 0.0 { 91.0 } else { 8.0 }
}

/// `SidebarSectionHeader`: a shelf's label, count and chevron.
pub fn shelf_header(t: &T, shelf: Shelf, count: usize, expanded: bool, tone: Tone) -> El {
    let (label, key) = match shelf {
        Shelf::Working => ("Working", "working"),
        Shelf::Snoozed => ("Snoozed", "snoozed"),
        Shelf::Settled => ("Settled", "settled"),
    };
    let color = match tone {
        Tone::Muted => t.sb.sidebar_muted_foreground,
        Tone::Emphasized => t.sb.foreground.alpha_mul(0.8),
        Tone::Accent => t.sb.primary,
    };
    row()
        .key(format!("sb-shelf-{key}"))
        .mx(2.0)
        .h(32.0)
        .px(8.0)
        .gap(8.0)
        .radius(radius::MD)
        .center_y()
        .hover(t.sb.sidebar_row_hover)
        .hover_group()
        .cursor(Cursor::Pointer)
        .expanded(expanded)
        .child(text(label).font(theme::XS_MEDIUM).color(color).group_color(t.sb.sidebar_foreground))
        .child(spacer())
        .child(text(count.to_string()).font(theme::TEXT_XS.features("'tnum' 1")).color(color).group_color(t.sb.sidebar_foreground))
        .child(icon(&i::CHEVRON_DOWN, 12.0).color(color).group_color(t.sb.sidebar_foreground).rotate(if expanded { 0.0 } else { -std::f32::consts::FRAC_PI_2 }))
        .on_press(move |s: &mut App| {
            let ui = &mut s.ui.shell.sidebar;
            let open = !ui.expanded.remove(&shelf) && ui.expanded.insert(shelf);
            if shelf == Shelf::Settled {
                ui.folders.set_settled(open);
            }
        })
}

/// `SidebarDragBoundary`: the "Pinned" / "Active" label a drag reveals over a section's top.
pub fn boundary(t: &T, label: &str, target: bool) -> El {
    let (fg, line) = if target { (t.sb.primary, t.sb.primary.alpha_mul(0.5)) } else { (t.sb.sidebar_foreground.alpha_mul(0.8), t.sb.sidebar_foreground.alpha_mul(0.25)) };
    row()
        .absolute(8.0, 4.0)
        .w(layout::SIDEBAR_W - 1.0 - 16.0 - 16.0 - 4.0)
        .h(16.0)
        .gap(8.0)
        .center_y()
        .child(text(label).font(theme::XS_MEDIUM).color(fg))
        .child(el().h(1.0).grow(1.0).fill(line))
}

/// `SidebarSectionPlaceholder`: the dashed drop box for an empty section.
pub fn placeholder(t: &T, label: &str, target: bool) -> El {
    let (border, fill, fg) = if target {
        (t.sb.primary.alpha_mul(0.4), t.sb.primary.alpha_mul(0.05), t.sb.primary)
    } else {
        (t.sb.sidebar_foreground.alpha_mul(0.25), Color::TRANSPARENT, t.sb.sidebar_foreground.alpha_mul(0.8))
    };
    el().absolute(0.0, 0.0)
        .w(layout::SIDEBAR_W - 1.0 - 16.0 - 4.0)
        .h(36.0)
        .radius(radius::MD)
        .border(1.0, border)
        .border_dash(4.0, 3.0)
        .fill(fill)
        .center()
        .child(text(label).font(theme::TEXT_XS).color(fg))
}

/// Search results replace the list: one row per hit, the highlighted one filled.
pub fn search_results(s: &App, model: &Model, t: &T) -> El {
    let ui = &s.ui.shell.sidebar;
    let hits = logic::search(&model.threads, &ui.search.text, ui.content_hits());
    let mut list = col().key("sb-results").w_full().px(8.0).py(8.0).gap(1.0);
    if hits.is_empty() {
        list = list.child(el().w_full().px(8.0).py(24.0).center().child(text("No threads found").font(theme::TEXT_XS).color(t.sb.sidebar_muted_foreground)));
    }
    for (n, idx) in hits.iter().enumerate() {
        let th = &model.threads[*idx];
        let on = n == ui.highlight || model.route.as_deref() == Some(th.id.as_str());
        let settled = model.section_of(&th.id) == Some(Section::Settled);
        let title_color = if settled { t.sb.secondary_label.alpha_mul(0.7) } else { t.sb.sidebar_foreground };
        let id = th.id.clone();
        let mut r = row()
            .key(format!("sb-result-{}", th.id))
            .w_full()
            .min_h(36.0)
            .px(10.0)
            .py(4.0)
            .gap(10.0)
            .radius(radius::MD)
            .center_y()
            .cursor(Cursor::Pointer)
            .child(super::favicon::mark(s, t, &th.project_id, model.project(th), settled && !on, 16.0))
            .child(text(th.title.clone()).font(if settled { theme::TEXT_SM } else { theme::SM_MEDIUM }).color(title_color).truncate().grow(1.0).shrink())
            .child(text(logic::relative_label(th.card_time(), model.now)).font(theme::TEXT_XS).color(t.sb.muted_foreground.alpha_mul(0.55)).w(32.0).align(rune::Align::End))
            .on_hover_change(move |s: &mut App, on, _: &mut Tx| {
                if on {
                    s.ui.shell.sidebar.highlight = n;
                }
            })
            .on_press(move |s: &mut App| {
                super::clear_search(s);
                super::open_thread(s, &id);
            });
        r = if on { r.fill(t.sb.sidebar_row_active) } else { r.hover(t.sb.sidebar_row_hover) };
        list = list.child(r);
    }
    el().key("sb-scroll").w_full().grow(1.0).shrink().scroll(ScrollSpec { stick_bottom: false }).child(list)
}

#[cfg(test)]
mod tests {
    use super::scope_items;

    #[test]
    fn scope_search_drops_all_projects_and_matches_names_anywhere() {
        let labels = ["acme/demo", "Other", "s5labs/s5code"];
        assert_eq!(scope_items(&labels, "  "), vec![None, Some(0), Some(1), Some(2)]);
        assert_eq!(scope_items(&labels, "O"), vec![Some(0), Some(1), Some(2)]);
        assert_eq!(scope_items(&labels, "labs"), vec![Some(2)]);
        assert_eq!(scope_items(&labels, "all"), Vec::<Option<usize>>::new());
    }
}
