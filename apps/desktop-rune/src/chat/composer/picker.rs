//! The model picker (ModelPickerPopover): a provider rail, the selected provider's models with
//! jump-shortcut hints, the effort column, and the permissions footer. It opens above the model chip.

use rune::{Align, Cursor, Cx, El, Key, KeyEvent, Side, TextEdit, Tx, col, el, icon, row, spacer, text, text_input};
use serde_json::{Value, json};

use super::provider_icons::STAR_FILLED;
use super::search::{self, Searchable};
use super::{ModeOption, draft_mut, provider, provider_icon, providers, selection, set_runtime_mode};
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
    /// The row under the pointer, which shows its favorite star.
    pub hovered: Option<usize>,
    pub runtime_open: bool,
}

impl Default for Ui {
    fn default() -> Ui {
        Ui { open: false, rail: None, query: TextEdit::new(""), active: None, hovered: None, runtime_open: false }
    }
}

/// One model row (ModelListRow).
pub(super) struct Row {
    pub instance: String,
    pub slug: String,
    pub name: String,
    pub sub_provider: String,
    pub new: bool,
    /// The selected model the provider no longer lists.
    pub unavailable: bool,
}

fn row_of(instance: &str, m: &Value) -> Row {
    Row {
        instance: instance.to_owned(),
        slug: data::s(m, "slug").to_owned(),
        name: data::s(m, "name").to_owned(),
        sub_provider: data::s(m, "subProvider").to_owned(),
        new: data::s(m, "badge") == "new",
        unavailable: false,
    }
}

pub fn toggle(s: &mut App) {
    let p = &mut s.ui.chat.composer.picker;
    p.open = !p.open;
    p.rail = None;
    p.active = None;
    p.hovered = None;
    p.runtime_open = false;
    p.query = TextEdit::new("");
    p.query.focused = p.open;
}

/// `composer.mode` without the permissions chip: the picker opens on its permissions menu.
pub fn open_permissions(s: &mut App) {
    if !s.ui.chat.composer.picker.open {
        toggle(s);
    }
    s.ui.chat.composer.picker.runtime_open = true;
}

fn close(s: &mut App) {
    s.ui.chat.composer.picker.open = false;
    s.ui.chat.composer.picker.runtime_open = false;
    let key = super::draft_key(s, s.frame_route());
    if let Some(d) = s.ui.chat.composer.drafts.get_mut(&key) {
        d.edit.focused = true;
    }
}

/// Providers the rail lists: enabled ones, in config order.
fn rail_providers(s: &App) -> Vec<&Value> {
    providers(s).iter().filter(|p| p["enabled"] == true && data::s(p, "status") != "disabled").collect()
}

/// The saved favorites (`{provider, model}`): the environment's, else the client's older copy.
pub(super) fn favorites(s: &App) -> Vec<(String, String)> {
    let server = &crate::settings::server_settings(s)["favorites"];
    let list = server.as_array().or_else(|| s.client_settings.values.get("favorites").and_then(Value::as_array));
    list.into_iter().flatten().filter_map(|f| Some((f["provider"].as_str()?.to_owned(), f["model"].as_str()?.to_owned()))).collect()
}

fn is_favorite(favorites: &[(String, String)], instance: &str, slug: &str) -> bool {
    favorites.iter().any(|(i, m)| i == instance && m == slug)
}

/// useToggleEnvironmentModelFavorite: sends the wanted state, so two clients cannot cancel out.
fn toggle_favorite(s: &mut App, instance: &str, slug: &str) {
    let favorite = !is_favorite(&favorites(s), instance, slug);
    let patch = json!({"setModelFavorites": [{"provider": instance, "model": slug, "favorite": favorite}]});
    crate::settings::request(s, "server.updateSettings", json!({ "patch": patch }));
}

/// Whether `instance` may be picked while the thread is bound to `locked`'s driver.
fn allowed(s: &App, locked: Option<&str>, instance: &str) -> bool {
    locked.is_none_or(|d| provider(s, instance).is_some_and(|p| data::s(p, "driver") == d))
}

fn rail(s: &App) -> String {
    s.ui.chat.composer.picker.rail.clone().or_else(|| selection(s).map(|(i, _)| i)).unwrap_or_else(|| FAVORITES.into())
}

fn is_searching(s: &App) -> bool {
    !s.ui.chat.composer.picker.query.text.trim().is_empty()
}

/// The rail entry's models, or while searching every ready provider's matches, best first; a
/// thread bound to one driver lists only that driver's models.
fn rows(s: &App) -> Vec<Row> {
    let mut list = if is_searching(s) { search_rows(s, &s.ui.chat.composer.picker.query.text) } else { rail_rows(s, &rail(s)) };
    let locked = super::locked_driver(s);
    list.retain(|r| allowed(s, locked.as_deref(), &r.instance));
    list
}

fn rail_rows(s: &App, r: &str) -> Vec<Row> {
    if r == FAVORITES {
        let favorites = favorites(s);
        let mut out = Vec::new();
        for p in rail_providers(s) {
            let instance = data::s(p, "instanceId");
            for m in p["models"].as_array().into_iter().flatten() {
                if is_favorite(&favorites, instance, data::s(m, "slug")) {
                    out.push(row_of(instance, m));
                }
            }
        }
        return out;
    }
    let Some(p) = provider(s, r) else { return Vec::new() };
    let mut out: Vec<Row> = p["models"].as_array().into_iter().flatten().map(|m| row_of(r, m)).collect();
    // appendUnavailableDynamicModelSelection: a dynamic catalog keeps the thread's model listed.
    if let Some((instance, slug)) = selection(s)
        && instance == r
        && matches!(data::s(p, "driver"), "opencode" | "antigravity")
        && !out.iter().any(|x| x.slug == slug)
    {
        out.push(Row { instance, name: slug.clone(), slug, sub_provider: String::new(), new: false, unavailable: true });
    }
    out
}

fn search_rows(s: &App, query: &str) -> Vec<Row> {
    let favorites = favorites(s);
    let models: Vec<(&Value, &Value)> = rail_providers(s)
        .into_iter()
        .filter(|p| data::s(p, "status") == "ready")
        .flat_map(|p| p["models"].as_array().into_iter().flatten().map(move |m| (p, m)))
        .collect();
    let searchable: Vec<Searchable> = models
        .iter()
        .map(|(p, m)| Searchable {
            name: data::s(m, "name"),
            short_name: data::s(m, "shortName"),
            sub_provider: data::s(m, "subProvider"),
            driver: data::s(p, "driver"),
            provider_name: data::s(p, "displayName"),
            favorite: is_favorite(&favorites, data::s(p, "instanceId"), data::s(m, "slug")),
        })
        .collect();
    search::rank(&searchable, query)
        .into_iter()
        .map(|i| {
            let (p, m) = models[i];
            row_of(data::s(p, "instanceId"), m)
        })
        .collect()
}

fn pick(s: &mut App, instance: String, slug: String) {
    if !allowed(s, super::locked_driver(s).as_deref(), &instance) {
        return;
    }
    let d = draft_mut(s);
    if d.model.as_ref() != Some(&(instance.clone(), slug.clone())) {
        d.effort = None;
        d.options.clear();
    }
    d.model = Some((instance, slug));
    close(s);
}

fn on_key(s: &mut App, e: &KeyEvent, _tx: &mut Tx) -> bool {
    let list = rows(s);
    let p = &mut s.ui.chat.composer.picker;
    match &e.key {
        Key::Down if !list.is_empty() => p.active = Some(p.active.map_or(0, |a| (a + 1) % list.len())),
        Key::Up if !list.is_empty() => p.active = Some(p.active.map_or(list.len() - 1, |a| (a + list.len() - 1) % list.len())),
        Key::Enter => {
            if let Some(r) = list.into_iter().nth(p.active.unwrap_or(0)) {
                pick(s, r.instance, r.slug);
            }
        }
        Key::Command(c) if c.is_ascii_digit() && *c != '0' => {
            let n = c.to_digit(10).unwrap_or(1) as usize - 1;
            if let Some(r) = list.into_iter().nth(n) {
                pick(s, r.instance, r.slug);
            }
        }
        _ => return false,
    }
    true
}

/// A column row (ComposerModelEffort's ColumnRow): a label, Default, and a check when picked.
fn column_row(t: &T, key: String, label: &str, selected: bool, is_default: bool, description: &str) -> El {
    let mut r = row()
        .key(key)
        .h(32.0)
        .px(8.0)
        .gap(6.0)
        .center_y()
        .radius(theme::layout::CONTROL_RADIUS)
        .hover(t.c.accent)
        .cursor(Cursor::Pointer)
        .role("menuitemradio")
        .child(text(label.to_owned()).font(theme::TEXT_SM).color(t.c.foreground).truncate().shrink());
    if is_default && !selected {
        r = r.child(text("Default").font(theme::TEXT_XS).color(t.c.muted_foreground.alpha_mul(0.7)));
    }
    r = r.child(spacer());
    if selected {
        r = r.fill(t.c.accent).child(icon(&icons::CHECK, 14.0).color(t.c.foreground));
    }
    if !description.is_empty() {
        r = r.help(description.to_owned());
    }
    r
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
    let locked = super::locked_driver(s);

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
    let mut entries = rail_providers(s);
    // A thread bound to one driver lists that driver first and the rest disabled.
    if let Some(d) = &locked {
        entries.sort_by_key(|p| data::s(p, "driver") != d.as_str());
    }
    for prov in entries {
        let id = data::s(prov, "instanceId").to_string();
        let mark = provider_icon(t, data::s(prov, "driver"), 20.0);
        let name = data::s(prov, "displayName").to_string();
        let lockout = !allowed(s, locked.as_deref(), &id);
        let ready = data::s(prov, "status") == "ready" && !lockout;
        let help = if lockout { format!("{name} is unavailable in this thread. Start a new thread to switch providers.") } else { name };
        rail_col = rail_col.child(rail_item(id.clone(), mark, r == id, ready).help(help));
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
    let searching = is_searching(s);
    let favorite_list = favorites(s);
    let in_favorites = r == FAVORITES && !searching;
    let mut models = col().w_full().px(8.0).pt(8.0);
    if list.is_empty() {
        let message = if searching {
            "No models found".to_string()
        } else if r == FAVORITES {
            "No favorites yet".to_string()
        } else {
            provider(s, &r).map(|p| data::s(p, "message").to_string()).filter(|m| !m.is_empty()).unwrap_or_else(|| "No models found".into())
        };
        models = models.child(text(message).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap().px(5.0).py(6.0));
    }
    for (n, m) in list.into_iter().enumerate() {
        let driver = provider(s, &m.instance).map(|p| data::s(p, "driver").to_string()).unwrap_or_default();
        let provider_name = provider(s, &m.instance).map(|p| data::s(p, "displayName").to_string()).unwrap_or_default();
        let selected = current.as_ref().is_some_and(|(i, slug)| *i == m.instance && *slug == m.slug);
        let favorite = is_favorite(&favorite_list, &m.instance, &m.slug);
        let lit = p.active == Some(n) || p.hovered == Some(n);
        let mut label = row()
            .gap(8.0)
            .center_y()
            .grow(1.0)
            .shrink()
            .min_w(0.0)
            .label(format!("{}, {provider_name}", m.name))
            .child(text(m.name.clone()).font(theme::TEXT_SM.lh(19.25)).color(t.c.foreground).truncate().shrink());
        if selected {
            label = label.child(icon(&icons::CHECK, 14.0).color(t.c.muted_foreground));
        }
        if !m.sub_provider.is_empty() {
            label = label.child(text(m.sub_provider.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground.alpha_mul(0.7)).truncate().shrink());
        }
        if m.new {
            label = label.child(
                el().px(2.0)
                    .py(1.0)
                    .radius(radius::SM)
                    .fill(t.c.update.alpha_mul(0.15))
                    .border(1.0, t.c.update.alpha_mul(0.35))
                    .label("New model")
                    .child(text("NEW").font(theme::weight(theme::TEXT_3XS, theme::BOLD)).color(t.c.update_foreground)),
            );
        }
        if m.unavailable {
            label = label.child(
                el().h(18.0).px(6.0).center().radius(radius::SM).border(1.0, t.c.border).child(text("Unavailable").font(theme::TEXT_XS).color(t.c.foreground)),
            );
        }
        let mut r = row()
            .key(format!("picker-model-{}-{}", m.instance, m.slug))
            .h(34.0)
            .pl(13.0)
            .pr(12.0)
            .gap(8.0)
            .center_y()
            .radius(theme::layout::CONTROL_RADIUS)
            .cursor(Cursor::Pointer)
            .hover(t.c.accent)
            .on_hover_change(move |s: &mut App, inside, _| {
                let hovered = &mut s.ui.chat.composer.picker.hovered;
                if inside {
                    *hovered = Some(n);
                } else if *hovered == Some(n) {
                    *hovered = None;
                }
            })
            .child(provider_icon(t, &driver, 16.0))
            .child(label);
        // In Favorites every row is starred, so the star shows only on a lit row there.
        let star_shown = if favorite { lit || !in_favorites } else { lit };
        let star_label = if favorite { "Remove from favorites" } else { "Add to favorites" };
        let (fav_instance, fav_slug) = (m.instance.clone(), m.slug.clone());
        let star = if favorite { icon(&STAR_FILLED, 12.0).color(t.c.warning) } else { icon(&icons::STAR, 12.0).color(t.c.muted_foreground) };
        let mut star_button = el()
            .key(format!("picker-favorite-{}-{}", m.instance, m.slug))
            .size(24.0, 24.0)
            .center()
            .radius(radius::SM)
            .hover(t.c.accent)
            .cursor(Cursor::Pointer)
            .role("button")
            .label(star_label)
            .help(star_label)
            .on_press(move |s: &mut App| toggle_favorite(s, &fav_instance, &fav_slug));
        if star_shown {
            star_button = star_button.child(star);
        }
        r = r.child(star_button);
        // The configured jump shortcut (modelJumpLabelsByKey): Ctrl+1 off macOS, none when unbound.
        let jump = if n < 9 { crate::shell::palette::shortcut_label(s, &format!("modelPicker.jump.{}", n + 1)) } else { String::new() };
        if !jump.is_empty() {
            r = r.child(
                el().h(20.0)
                    .px(4.0)
                    .center()
                    .radius(radius::SM)
                    .fill(t.c.foreground.alpha_mul(0.07))
                    .child(text(jump).font(theme::XS_MEDIUM).color(t.c.muted_foreground)),
            );
        }
        if p.active == Some(n) {
            r = r.fill(t.c.accent);
        }
        let (i2, m2) = (m.instance.clone(), m.slug.clone());
        models = models.child(r.on_press(move |s: &mut App| pick(s, i2.clone(), m2.clone())));
    }
    // A search spans every provider, so the rail steps aside (showSidebar).
    let middle = col()
        .w(if searching { 360.0 } else { 316.0 })
        .h_full()
        .fill(panel)
        .child(el().absolute(0.0, 0.0).w(1.0).h_full().fill(line))
        .child(header)
        .child(el().w_full().h(1.0).fill(line))
        .child(col().w_full().grow(1.0).min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(models));

    // Effort: the selected model's reasoning levels under the header that names them, then its
    // other options (ComposerEffortColumn).
    let effort_label = current.as_ref().map(|(i, m)| super::model_name(s, i, m)).unwrap_or_default();
    let reasoning = super::effort_descriptor(s);
    let efforts = super::effort_options(s).map(|(_, o)| o).unwrap_or_default();
    let others = super::column_options(s);
    let chosen = super::effort(s);
    let column_title = reasoning
        .as_ref()
        .map(|d| data::s(d, "label").to_owned())
        .filter(|l| !l.is_empty() && !efforts.is_empty())
        .unwrap_or_else(|| if efforts.is_empty() && !others.is_empty() { "Options".to_owned() } else { "Effort".to_owned() });
    let mut effort_body = col().w_full().px(9.0).mt(21.0);
    if efforts.is_empty() && others.is_empty() {
        effort_body = effort_body.child(row().h(32.0).px(8.0).center_y().child(text("No effort levels").font(theme::TEXT_SM).color(t.c.muted_foreground)));
    }
    let effort_count = efforts.len();
    for o in efforts {
        let id = data::s(&o, "id").to_owned();
        let selected = chosen.as_deref() == Some(id.as_str());
        let r = column_row(t, format!("picker-effort-{id}"), data::s(&o, "label"), selected, o["isDefault"] == true, data::s(&o, "description"));
        effort_body = effort_body.child(r.on_press(move |s: &mut App| draft_mut(s).effort = Some(id.clone())));
    }
    if !others.is_empty() {
        let mut group = col().w_full().gap(2.0);
        if effort_count > 0 {
            group = group.mt(4.0).pt(4.0).child(el().absolute(0.0, 0.0).w_full().h(1.0).fill(line));
        }
        for d in &others {
            let id = data::s(d, "id").to_owned();
            let value = super::option_value(s, d);
            if data::s(d, "type") == "boolean" {
                let on = value == true;
                let next = !on;
                let option = id.clone();
                group = group.child(
                    row()
                        .key(format!("picker-option-{id}"))
                        .h(32.0)
                        .px(8.0)
                        .gap(6.0)
                        .center_y()
                        .radius(theme::layout::CONTROL_RADIUS)
                        .hover(t.c.accent)
                        .cursor(Cursor::Pointer)
                        .role("switch")
                        .child(text(data::s(d, "label").to_owned()).font(theme::TEXT_SM).color(t.c.foreground).truncate().shrink())
                        .child(spacer())
                        .child(text(if on { "On" } else { "Off" }).font(theme::TEXT_XS).color(t.c.muted_foreground))
                        .on_press(move |s: &mut App| super::set_option(s, &option, json!(next))),
                );
                continue;
            }
            group = group.child(text(data::s(d, "label").to_owned()).font(theme::XS_MEDIUM).color(t.c.muted_foreground).px(8.0).pt(6.0).pb(4.0));
            for choice in d["options"].as_array().into_iter().flatten() {
                let choice_id = data::s(choice, "id").to_owned();
                let selected = value.as_str() == Some(choice_id.as_str());
                let option = id.clone();
                let r = column_row(t, format!("picker-option-{id}-{choice_id}"), data::s(choice, "label"), selected, choice["isDefault"] == true, data::s(choice, "description"));
                group = group.child(r.on_press(move |s: &mut App| super::set_option(s, &option, json!(choice_id))));
            }
        }
        effort_body = effort_body.child(group);
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
                .child(text(column_title).font(theme::TEXT_SM).color(t.c.muted_foreground))
                .child(spacer())
                .child(text(effort_label).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().shrink()),
        )
        .child(col().w_full().grow(1.0).min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(effort_body));

    // Footer: the speed toggle, then the permissions menu (ComposerPickerFooter).
    let options = super::runtime_mode_options(s);
    let mode = super::effective_runtime_mode(s);
    let mut footer = row().w_full().h(45.0).px(12.0).gap(8.0).center_y().fill(panel).child(el().absolute(0.0, 0.0).w_full().h(1.0).fill(line));
    if let Some(speed) = speed_control(s, t) {
        footer = footer.child(speed);
    }
    footer = footer.child(spacer());
    if let Some(current_mode) = options.iter().find(|o| o.mode == mode) {
        footer = footer.child(
            row()
                .key("picker-permissions")
                .h(28.0)
                .px(8.0)
                .gap(6.0)
                .center_y()
                .radius(radius::MD)
                .hover(t.c.accent)
                .cursor(Cursor::Pointer)
                .label(format!("Permissions: {}", current_mode.label))
                .child(icon(current_mode.icon, 14.0).color(t.c.muted_foreground))
                .child(text(current_mode.label).font(theme::TEXT_XS).color(t.c.muted_foreground))
                .child(icon(&icons::CHEVRON_UP, 12.0).color(t.c.muted_foreground))
                .on_press(|s: &mut App| {
                    let p = &mut s.ui.chat.composer.picker;
                    p.runtime_open = !p.runtime_open;
                }),
        );
    }

    let mut body = row().w_full().h(346.0);
    if !searching {
        body = body.child(rail_col);
    }
    let body = body.child(middle).child(effort);
    let card = col()
        .key("model-picker")
        .w(554.0)
        .radius(radius::LG)
        .fill(t.c.popover.alpha_mul(0.836))
        .backdrop_blur(16.0)
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
        layers.push(runtime_menu(t, &options, &mode, "picker-runtime-menu", "picker-permissions", Align::End));
    }
    layers
}

/// SpeedControl: Codex service tiers, or Standard and Fast for a fast-mode switch.
fn speed_control(s: &App, t: &T) -> Option<El> {
    let d = super::speed_descriptor(s)?;
    let id = data::s(&d, "id").to_owned();
    let value = super::option_value(s, &d);
    let segments: Vec<(String, Value, bool)> = if data::s(&d, "type") == "boolean" {
        [(false, "Standard"), (true, "Fast")].into_iter().map(|(v, label)| (label.to_owned(), json!(v), (value == true) == v)).collect()
    } else {
        d["options"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|o| (data::s(o, "label").to_owned(), o["id"].clone(), value == o["id"]))
            .collect()
    };
    let mut group = row().key("picker-speed").padding(2.0).radius(radius::LG).fill(t.c.input.alpha_mul(0.4)).role("radiogroup").label(data::s(&d, "label").to_owned());
    for (label, v, checked) in segments {
        let option = id.clone();
        let fast = label == "Fast" || label == "Ultrafast";
        let mut segment = row()
            .key(format!("picker-speed-{label}"))
            .h(24.0)
            .px(8.0)
            .gap(4.0)
            .center_y()
            .radius(radius::MD)
            .cursor(Cursor::Pointer)
            .role("radio")
            .on_press(move |s: &mut App| super::set_option(s, &option, v.clone()));
        let ink = if checked { t.c.foreground } else { t.c.muted_foreground };
        if checked {
            segment = segment.fill(t.c.input.alpha_mul(0.72));
        }
        if fast {
            segment = segment.child(icon(&icons::ZAP, 12.0).color(ink));
        }
        group = group.child(segment.child(text(label).font(theme::TEXT_XS).color(ink)));
    }
    Some(group)
}

/// The permissions menu above `anchor`: the picker's footer toggle or the composer's chip. Modes
/// above an administrator's ceiling stay listed, disabled with the reason.
pub(super) fn runtime_menu(t: &T, options: &[ModeOption], mode: &str, key: &str, anchor: &str, align: Align) -> El {
    let mut menu = col()
        .key(key)
        .w(300.0)
        .padding(4.0)
        .radius(radius::LG)
        .fill(t.c.popover)
        .border(1.0, t.c.border)
        .shadow(8.0, 24.0, rune::Color::BLACK.alpha(0.3))
        .anchored(anchor, Side::Top, align, 4.0);
    for option in options {
        let id = option.mode;
        let description = option.disabled.clone().unwrap_or_else(|| option.description.to_owned());
        let mut item = row()
            .key(format!("runtime-{id}"))
            .w_full()
            .px(8.0)
            .py(6.0)
            .gap(8.0)
            .radius(radius::MD)
            .child(icon(option.icon, 16.0).color(t.c.muted_foreground))
            .child(
                col().grow(1.0).shrink().min_w(0.0)
                    .child(text(option.label).font(theme::SM_MEDIUM).color(t.c.foreground))
                    .child(text(description).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()),
            );
        item = if option.disabled.is_some() {
            item.opacity(0.4).cursor(Cursor::NotAllowed)
        } else {
            item.hover(t.c.accent).cursor(Cursor::Pointer).on_press(move |s: &mut App| {
                set_runtime_mode(s, id);
                s.ui.chat.composer.picker.runtime_open = false;
                s.ui.chat.composer.permissions_open = false;
            })
        };
        if id == mode {
            item = item.child(icon(&icons::CHECK, 14.0).color(t.c.foreground));
        }
        menu = menu.child(item);
    }
    menu
}
