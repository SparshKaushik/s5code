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
                .child(icon_button(t, "sb-filter", &i::LIST_FILTER, 28.0, 16.0, t.sb.sidebar_icon_color, false).on_press(|s: &mut App| s.ui.shell.sidebar.scope_open ^= true))
                .child(icon_button(t, "sb-add-project", &i::FOLDER_PLUS, 28.0, 16.0, t.sb.sidebar_icon_color, false).on_press(crate::shell::palette::open_add_project))
                .child(icon_button(t, "sb-new-thread", &i::SQUARE_PEN, 28.0, 16.0, t.sb.sidebar_icon_color, false).on_press(super::new_thread)),
        )
}

/// The project scope picker under the filter button: all projects, or any set of them.
pub fn scope_menu(s: &App, _model: &Model, t: &T, overlays: &mut Vec<El>) {
    let ui = &s.ui.shell.sidebar;
    if !ui.scope_open {
        return;
    }
    let mut menu = crate::kit::Menu::new("sb-scope")
        .anchor("sb-filter")
        .placement(crate::kit::Placement::BottomStart)
        .min_width(224.0)
        .open(true)
        .on_close(|s: &mut App| s.ui.shell.sidebar.scope_open = false)
        .item(crate::kit::MenuItem::new("All projects").icon(&i::FOLDER).checked(ui.project_scope.is_empty()).on_select(|s: &mut App| s.ui.shell.sidebar.project_scope.clear()))
        .separator();
    for p in s.state().projects.iter().filter(|p| p["archivedAt"].is_null()) {
        let id = crate::data::s(p, "id").to_string();
        let on = ui.project_scope.contains(&id);
        menu = menu.item(crate::kit::MenuItem::new(crate::data::s(p, "title")).checked(on).on_select(move |s: &mut App| {
            let scope = &mut s.ui.shell.sidebar.project_scope;
            if on {
                scope.retain(|x| *x != id);
            } else {
                scope.push(id.clone());
            }
        }));
    }
    menu.overlay(&crate::kit::theme(t), overlays);
}

pub fn footer(model: &Model, t: &T) -> El {
    let btn = |key: &str, glyph| icon_button(t, key, glyph, 32.0, 16.0, t.sb.sidebar_icon_color, false);
    // SidebarChrome swaps these for "Back" only on utility pages (/projects/*, /pull-requests);
    // settings is a modal over the frame, so the footer stays.
    let menu = row()
        .gap(4.0)
        .child(btn("sb-settings", &i::SETTINGS).on_press(|s: &mut App| super::go(s, Route::Settings("general".into()))))
        .child(btn("sb-pull-requests", &i::GIT_PULL_REQUEST_ARROW))
        .child(btn("sb-usage", &i::CHART_NO_AXES_COLUMN));
    let mut bar = row().key("sb-footer").w_full().shrink().px(8.0).pt(4.0).pb(8.0).center_y().child(menu).child(spacer());
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
    bar
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
            let e = &mut s.ui.shell.sidebar.expanded;
            if !e.remove(&shelf) {
                e.insert(shelf);
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
    let hits = logic::search(&model.threads, &ui.search.text);
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
            .child(super::row::monogram(t, model.project(th), settled && !on))
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
