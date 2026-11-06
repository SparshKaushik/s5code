//! The model picker (ModelPickerPopover): a provider rail, the selected provider's models with
//! ⌘-digit hints, the effort column, and the permissions footer. It opens above the model chip.

use rune::{Align, Cursor, Cx, El, Key, KeyEvent, Side, TextEdit, Tx, col, el, icon, row, spacer, text, text_input};
use serde_json::Value;

use super::provider_icons::STAR_FILLED;
use super::{RUNTIME_MODES, draft_mut, provider, provider_icon, providers, runtime_mode, selection, set_runtime_mode};
use crate::data;
use crate::icons;
use crate::theme::{self, AlphaMul, T, radius};
use crate::App;

const FAVORITES: &str = "favorites";

pub struct Ui {
    pub open: bool,
    /// The rail entry shown: a provider instance id or `favorites`.
    pub rail: Option<String>,
    pub query: TextEdit,
    /// The keyboard-highlighted row; none until the user moves through the list.
    pub active: Option<usize>,
    pub runtime_open: bool,
}

impl Default for Ui {
    fn default() -> Ui {
        Ui { open: false, rail: None, query: TextEdit::new(""), active: None, runtime_open: false }
    }
}

pub fn toggle(s: &mut App) {
    let p = &mut s.ui.chat.composer.picker;
    p.open = !p.open;
    p.rail = None;
    p.active = None;
    p.runtime_open = false;
    p.query = TextEdit::new("");
    p.query.focused = p.open;
}

fn close(s: &mut App) {
    s.ui.chat.composer.picker.open = false;
    s.ui.chat.composer.picker.runtime_open = false;
    if let Some(d) = s.ui.chat.composer.drafts.get_mut(&super::draft_key(&s.route)) {
        d.edit.focused = true;
    }
}

/// Providers the rail lists: enabled ones, in config order.
fn rail_providers(s: &App) -> Vec<&Value> {
    providers(s).iter().filter(|p| p["enabled"] == true && data::s(p, "status") != "disabled").collect()
}

fn favorites(s: &App) -> Vec<(String, String)> {
    let Some(list) = s.state().config.as_ref().and_then(|c| c["settings"]["favorites"].as_array()) else { return Vec::new() };
    list.iter().filter_map(|f| Some((f["instanceId"].as_str()?.to_string(), f["model"].as_str()?.to_string()))).collect()
}

fn rail(s: &App) -> String {
    s.ui.chat.composer.picker.rail.clone().or_else(|| selection(s).map(|(i, _)| i)).unwrap_or_else(|| FAVORITES.into())
}

/// `(instance, slug, name)` rows for the rail entry, filtered by the search.
fn rows(s: &App) -> Vec<(String, String, String)> {
    let query = s.ui.chat.composer.picker.query.text.trim().to_lowercase();
    let r = rail(s);
    let all: Vec<(String, String, String)> = if r == FAVORITES {
        favorites(s).into_iter().map(|(i, m)| { let n = super::model_name(s, &i, &m); (i, m, n) }).collect()
    } else {
        provider(s, &r)
            .and_then(|p| p["models"].as_array())
            .map(|ms| ms.iter().map(|m| (r.clone(), data::s(m, "slug").to_string(), data::s(m, "name").to_string())).collect())
            .unwrap_or_default()
    };
    all.into_iter().filter(|(_, slug, name)| query.is_empty() || name.to_lowercase().contains(&query) || slug.to_lowercase().contains(&query)).collect()
}

fn pick(s: &mut App, instance: String, slug: String) {
    draft_mut(s).model = Some((instance, slug));
    close(s);
}

fn on_key(s: &mut App, e: &KeyEvent, _tx: &mut Tx) -> bool {
    let list = rows(s);
    let p = &mut s.ui.chat.composer.picker;
    match &e.key {
        Key::Down if !list.is_empty() => p.active = Some(p.active.map_or(0, |a| (a + 1) % list.len())),
        Key::Up if !list.is_empty() => p.active = Some(p.active.map_or(list.len() - 1, |a| (a + list.len() - 1) % list.len())),
        Key::Enter => {
            if let Some((i, m, _)) = list.into_iter().nth(p.active.unwrap_or(0)) {
                pick(s, i, m);
            }
        }
        Key::Command(c) if c.is_ascii_digit() && *c != '0' => {
            let n = c.to_digit(10).unwrap_or(1) as usize - 1;
            if let Some((i, m, _)) = list.into_iter().nth(n) {
                pick(s, i, m);
            }
        }
        _ => return false,
    }
    true
}

pub fn view(s: &App, _cx: &mut Cx<App>, t: &T) -> Vec<El> {
    let p = &s.ui.chat.composer.picker;
    if !p.open {
        return Vec::new();
    }
    let current = selection(s);
    let r = rail(s);
    let line = t.c.border.alpha_mul(0.7);
    let panel = t.c.muted.alpha_mul(0.4);

    // Rail: favourites, a divider, then each provider; a primary bar marks the shown entry.
    let rail_item = |key: String, mark: El, active: bool, ready: bool| {
        let id = key.clone();
        let mut item = el().key(format!("picker-rail-{key}")).w(44.0).h(40.0).center().child(mark);
        // An instance that isn't ready can't be browsed (opacity-50, disabled).
        item = if ready {
            item.cursor(Cursor::Pointer).hover(t.c.accent).on_press(move |s: &mut App| {
                let p = &mut s.ui.chat.composer.picker;
                p.rail = Some(id.clone());
                p.active = None;
            })
        } else {
            item.opacity(0.5)
        };
        if active {
            item = item.child(el().absolute(37.0, 10.0).size(3.0, 20.0).radius(radius::FULL).fill(t.c.primary));
        }
        item
    };
    let mut rail_col = col().w(44.0).h_full().pt(2.0).fill(t.c.muted.alpha_mul(0.3));
    rail_col = rail_col.child(rail_item(FAVORITES.into(), icon(&STAR_FILLED, 20.0).color(t.c.foreground), r == FAVORITES, true).help("Favorites"));
    rail_col = rail_col.child(el().h(5.0).w_full().center().child(el().h(1.0).w(20.0).fill(line)));
    for prov in rail_providers(s) {
        let id = data::s(prov, "instanceId").to_string();
        let mark = provider_icon(t, data::s(prov, "driver"), 20.0);
        let ready = data::s(prov, "status") == "ready";
        rail_col = rail_col.child(rail_item(id.clone(), mark, r == id, ready).help(data::s(prov, "displayName").to_string()));
    }

    // Models: search, then one row per model.
    let search = row()
        .h(26.0)
        .w(239.0)
        .center_y()
        .child(icon(&icons::SEARCH, 16.0).color(t.c.muted_foreground.alpha_mul(0.55)).absolute(0.0, 5.0))
        .child(
            text_input(&p.query, theme::TEXT_SM.lh(26.0))
                .key("picker-search")
                .single_line()
                .w_full()
                .pl(20.0)
                .pr(9.0)
                .color(t.c.foreground)
                .placeholder("Search models...", t.c.placeholder)
                .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
                .on_edit(|s: &mut App| &mut s.ui.chat.composer.picker.query)
                .on_input_change(|s: &mut App, _| s.ui.chat.composer.picker.active = None)
                .on_input_key(on_key),
        );
    let mut header = col().w_full().px(13.0).pt(9.0).pb(8.0).child(search);
    if p.query.focused {
        header = header.child(el().absolute(13.0, 41.0).w(290.0).h(1.0).fill(t.c.primary));
    }
    let list = rows(s);
    let mut models = col().w_full().px(8.0).pt(8.0);
    if list.is_empty() {
        let message = if r == FAVORITES {
            "No favorite models yet.".to_string()
        } else {
            provider(s, &r).map(|p| data::s(p, "message").to_string()).filter(|m| !m.is_empty()).unwrap_or_else(|| "No models found.".into())
        };
        models = models.child(text(message).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap().px(5.0).py(6.0));
    }
    for (n, (instance, slug, name)) in list.into_iter().enumerate() {
        let driver = provider(s, &instance).map(|p| data::s(p, "driver").to_string()).unwrap_or_default();
        let selected = current.as_ref().is_some_and(|(i, m)| *i == instance && *m == slug);
        let mut label = row().gap(8.0).center_y().shrink().min_w(0.0).child(text(name).font(theme::TEXT_SM.lh(19.25)).color(t.c.foreground).truncate().shrink());
        if selected {
            label = label.child(icon(&icons::CHECK, 14.0).color(t.c.muted_foreground));
        }
        let mut r = row()
            .key(format!("picker-model-{instance}-{slug}"))
            .h(34.0)
            .pl(13.0)
            .pr(12.0)
            .gap(8.0)
            .center_y()
            .radius(theme::layout::CONTROL_RADIUS)
            .cursor(Cursor::Pointer)
            .hover(t.c.accent)
            .child(provider_icon(t, &driver, 16.0))
            .child(label)
            .child(spacer());
        if n < 9 {
            r = r.child(
                el().h(20.0)
                    .px(4.0)
                    .center()
                    .radius(radius::SM)
                    .fill(t.c.foreground.alpha_mul(0.07))
                    .child(text(format!("⌘{}", n + 1)).font(theme::XS_MEDIUM).color(t.c.muted_foreground)),
            );
        }
        if p.active == Some(n) {
            r = r.fill(t.c.accent);
        }
        let (i2, m2) = (instance.clone(), slug.clone());
        models = models.child(r.on_press(move |s: &mut App| pick(s, i2.clone(), m2.clone())));
    }
    let middle = col()
        .w(316.0)
        .h_full()
        .fill(panel)
        .child(el().absolute(0.0, 0.0).w(1.0).h_full().fill(line))
        .child(header)
        .child(el().w_full().h(1.0).fill(line))
        .child(col().w_full().grow(1.0).min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(models));

    // Effort: the selected model's effort options, when it has any.
    let effort_label = current.as_ref().map(|(i, m)| super::model_name(s, i, m)).unwrap_or_default();
    let efforts: Vec<String> = current
        .as_ref()
        .and_then(|(i, m)| provider(s, i)?["models"].as_array()?.iter().find(|x| data::s(x, "slug") == m).cloned())
        .and_then(|m| m["capabilities"]["optionDescriptors"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter(|d| data::s(d, "id") == "effort" || data::s(d, "id") == "reasoningEffort")
        .flat_map(|d| d["options"].as_array().cloned().unwrap_or_default())
        .map(|o| data::s(&o, "label").to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let mut effort_body = col().w_full().px(9.0).mt(21.0);
    if efforts.is_empty() {
        effort_body = effort_body.child(row().h(32.0).px(8.0).center_y().child(text("No effort levels").font(theme::TEXT_SM).color(t.c.muted_foreground)));
    }
    for e in efforts {
        effort_body = effort_body.child(row().h(32.0).px(8.0).center_y().radius(theme::layout::CONTROL_RADIUS).hover(t.c.accent).child(text(e).font(theme::TEXT_SM).color(t.c.foreground)));
    }
    let effort = col()
        .w(192.0)
        .h_full()
        .fill(panel)
        .child(el().absolute(0.0, 0.0).w(1.0).h_full().fill(line))
        .child(
            row()
                .w_full()
                .pl(13.0)
                .pr(12.0)
                .pt(12.0)
                .gap(8.0)
                .center_y()
                .child(text("Effort").font(theme::TEXT_SM).color(t.c.muted_foreground))
                .child(spacer())
                .child(text(effort_label).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().shrink()),
        )
        .child(effort_body);

    // Footer: the permissions toggle.
    let mode = runtime_mode(s);
    let (_, mode_label, _, mode_icon) = RUNTIME_MODES.iter().find(|m| m.0 == mode).copied().unwrap_or(RUNTIME_MODES[3]);
    let footer = row()
        .w_full()
        .h(45.0)
        .px(12.0)
        .center_y()
        .fill(panel)
        .child(el().absolute(0.0, 0.0).w_full().h(1.0).fill(line))
        .child(spacer())
        .child(
            row()
                .key("picker-permissions")
                .h(28.0)
                .px(8.0)
                .gap(6.0)
                .center_y()
                .radius(radius::MD)
                .hover(t.c.accent)
                .cursor(Cursor::Pointer)
                .label(format!("Permissions: {mode_label}"))
                .child(icon(mode_icon, 14.0).color(t.c.muted_foreground))
                .child(text(mode_label).font(theme::TEXT_XS).color(t.c.muted_foreground))
                .child(icon(&icons::CHEVRON_UP, 12.0).color(t.c.muted_foreground))
                .on_press(|s: &mut App| {
                    let p = &mut s.ui.chat.composer.picker;
                    p.runtime_open = !p.runtime_open;
                }),
        );

    let body = row().w_full().h(346.0).child(rail_col).child(middle).child(effort);
    let card = col()
        .key("model-picker")
        .w(554.0)
        .radius(radius::LG)
        .fill(t.c.popover.alpha_mul(0.836))
        .border(1.0, t.c.border)
        .shadow(8.0, 24.0, rune::Color::BLACK.alpha(if t.dark { 0.4 } else { 0.12 }))
        .clip_rounded(radius::LG)
        .padding(1.0)
        .child(body)
        .child(footer)
        .anchored("composer-model-chip", Side::Top, Align::End, 4.0)
        .modal(true, close);
    let mut layers = vec![card];
    if p.runtime_open {
        layers.push(runtime_menu(s, t, &mode));
    }
    layers
}

/// The permissions menu above the footer toggle.
fn runtime_menu(_s: &App, t: &T, mode: &str) -> El {
    let mut menu = col()
        .key("picker-runtime-menu")
        .w(300.0)
        .padding(4.0)
        .radius(radius::LG)
        .fill(t.c.popover)
        .border(1.0, t.c.border)
        .shadow(8.0, 24.0, rune::Color::BLACK.alpha(0.3))
        .anchored("picker-permissions", Side::Top, Align::End, 4.0);
    for (id, label, description, i) in RUNTIME_MODES {
        let mut item = row()
            .key(format!("runtime-{id}"))
            .w_full()
            .px(8.0)
            .py(6.0)
            .gap(8.0)
            .radius(radius::MD)
            .hover(t.c.accent)
            .cursor(Cursor::Pointer)
            .child(icon(i, 16.0).color(t.c.muted_foreground))
            .child(
                col().grow(1.0).shrink().min_w(0.0)
                    .child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground))
                    .child(text(description).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()),
            )
            .on_press(move |s: &mut App| {
                set_runtime_mode(s, id);
                s.ui.chat.composer.picker.runtime_open = false;
            });
        if id == mode {
            item = item.child(icon(&icons::CHECK, 14.0).color(t.c.foreground));
        }
        menu = menu.child(item);
    }
    menu
}
