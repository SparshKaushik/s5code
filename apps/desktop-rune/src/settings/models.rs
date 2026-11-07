//! The settings model picker (`ProviderModelPicker` without the composer's effort column): a
//! bordered trigger (`SETTINGS_PICKER_TRIGGER_CLASSNAME`) and a popover with the provider rail and
//! the selected provider's models. Rows: default, fallback and text generation model.

use rune::{Align, Cursor, El, Side, TextEdit, col, el, icon, row, text, text_input};
use serde_json::{Value, json};

use super::{server_settings, update};
use crate::chat::composer::{provider_icon, providers};
use crate::chat::composer::provider_icons::STAR_FILLED;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route, data, icons};

const FAVORITES: &str = "favorites";

#[derive(Default)]
pub struct Picker {
    /// The setting key whose picker is open.
    pub open: Option<String>,
    /// The rail entry shown: an instance id or `favorites`.
    pub rail: Option<String>,
    pub query: TextEdit,
    /// The keyboard-highlighted row; Enter picks it, or the first row when unset.
    pub active: Option<usize>,
}

pub fn is_model_key(key: &str) -> bool {
    matches!(key, "defaultModelSelection" | "fallbackModelSelection" | "textGenerationModelSelection")
}

fn selectable(p: &Value) -> bool {
    p["enabled"] == true && p["availability"] != "unavailable"
}

/// `isProviderInstancePickerReady`.
fn ready(p: &Value) -> bool {
    selectable(p) && p["status"] == "ready"
}

/// `getDefaultProviderInstanceModel`.
fn default_model(p: &Value) -> Option<String> {
    let models = p["models"].as_array()?;
    models
        .iter()
        .find(|m| m["isDefault"] == true && m["isCustom"] != true)
        .or_else(|| models.iter().find(|m| m["isCustom"] != true))
        .or_else(|| models.first())
        .map(|m| data::s(m, "slug").to_owned())
}

/// The providers a key picks from: text generation skips providers that can't generate text.
fn candidates<'a>(s: &'a App, key: &str) -> Vec<&'a Value> {
    providers(s)
        .iter()
        .filter(|p| key != "textGenerationModelSelection" || p["supportsTextGeneration"] != false)
        .collect()
}

/// `resolveSelectableProviderInstanceEntry`: the requested instance when selectable, else the
/// first picker-ready one, else the first selectable one that isn't erroring.
fn selectable_entry<'a>(entries: &[&'a Value], requested: Option<&str>) -> Option<&'a Value> {
    if let Some(id) = requested {
        if let Some(p) = entries.iter().find(|p| p["instanceId"] == id).filter(|p| selectable(p)) {
            return Some(p);
        }
    }
    entries
        .iter()
        .find(|p| ready(p))
        .or_else(|| entries.iter().find(|p| selectable(p) && p["status"] != "error"))
        .copied()
}

fn pair(v: &Value) -> Option<(String, String)> {
    Some((v["instanceId"].as_str()?.to_owned(), v["model"].as_str()?.to_owned()))
}

/// The selection a key shows: `resolveDefaultProviderModelSelection` over the stored value, or
/// the automatic project default when nothing is stored.
pub fn selection(s: &App, key: &str) -> Option<(String, String)> {
    let entries = candidates(s, key);
    let stored = super::value(s, key);
    if key == "fallbackModelSelection" {
        return pair(&stored).or_else(|| selection(s, "defaultModelSelection"));
    }
    if stored.is_null() {
        // `resolveProjectDefaultProviderModelSelection`: the first driver group wins first.
        let first = entries.first().map(|p| p["driver"].clone());
        let group: Vec<_> = entries.iter().copied().filter(|p| Some(&p["driver"]) == first.as_ref()).collect();
        let entry = selectable_entry(&group, None).or_else(|| selectable_entry(&entries, None))?;
        return Some((data::s(entry, "instanceId").to_owned(), default_model(entry)?));
    }
    let requested = stored["instanceId"].as_str();
    let entry = selectable_entry(&entries, requested)?;
    if Some(data::s(entry, "instanceId")) == requested {
        return pair(&stored);
    }
    Some((data::s(entry, "instanceId").to_owned(), default_model(entry)?))
}

fn model_label(s: &App, instance: &str, model: &str) -> String {
    crate::chat::composer::model_name(s, instance, model)
}

/// The row's control: the trigger, or the web's muted fallback copy.
pub fn control(s: &App, t: &T, key: &str) -> El {
    let entries = candidates(s, key);
    let none = |copy: &str| text(copy.to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground);
    if key == "textGenerationModelSelection" && !entries.iter().any(|p| selectable(p)) {
        return none("No text generation providers available.");
    }
    let Some((instance, model)) = selection(s, key) else {
        return none("No providers available");
    };
    let stored_null = super::value(s, key).is_null();
    let label = if key == "fallbackModelSelection" && stored_null {
        "None".to_owned()
    } else {
        model_label(s, &instance, &model)
    };
    let driver = entries
        .iter()
        .find(|p| p["instanceId"] == instance.as_str())
        .map(|p| data::s(p, "driver").to_owned())
        .unwrap_or_default();
    let open = s.ui.settings.models.open.as_deref() == Some(key);
    let owned = key.to_owned();
    let mut trigger = row()
        .key(format!("model-trigger-{key}"))
        .h(28.0)
        .px(8.0)
        .gap(6.0)
        .center_y()
        .min_w(0.0)
        .radius(theme::layout::CONTROL_RADIUS)
        .border(1.0, t.c.input)
        .fill(if t.dark { t.c.input.alpha_mul(0.32) } else { t.c.background })
        .hover(t.c.accent.alpha_mul(0.5))
        .cursor(Cursor::Pointer)
        .focusable(true)
        .role("button")
        .label(match key {
            "fallbackModelSelection" => "Fallback model".to_owned(),
            _ => label.clone(),
        })
        .expanded(open)
        .on_press(move |s: &mut App| toggle(s, &owned));
    if open {
        trigger = trigger.fill(t.c.accent);
    }
    // A label override (None, Mixed) hides the provider icon, as `triggerLabel` does.
    if !(key == "fallbackModelSelection" && stored_null) {
        trigger = trigger.child(provider_icon(t, &driver, 16.0));
    }
    trigger.child(text(label).font(theme::TEXT_SM).color(t.c.foreground).truncate().shrink())
}

pub fn toggle(s: &mut App, key: &str) {
    let p = &mut s.ui.settings.models;
    if p.open.as_deref() == Some(key) {
        p.open = None;
        return;
    }
    p.open = Some(key.to_owned());
    p.rail = None;
    p.active = None;
    p.query = TextEdit::new("");
    p.query.focused = true;
}

pub fn close(s: &mut App) {
    s.ui.settings.models.open = None;
}

fn rail_entries<'a>(s: &'a App, key: &str) -> Vec<&'a Value> {
    candidates(s, key).into_iter().filter(|p| p["enabled"] == true).collect()
}

fn favorites(s: &App) -> Vec<(String, String)> {
    let list = server_settings(s)["favorites"].as_array().cloned().unwrap_or_default();
    list.iter()
        .filter_map(|f| Some((f["provider"].as_str()?.to_owned(), f["model"].as_str()?.to_owned())))
        .collect()
}

/// `(instance, slug, name)` for a rail entry, hidden models left out.
fn models_for(s: &App, key: &str, rail: &str) -> Vec<(String, String, String)> {
    if rail == FAVORITES {
        let ids: Vec<_> = rail_entries(s, key).iter().map(|p| data::s(p, "instanceId").to_owned()).collect();
        return favorites(s)
            .into_iter()
            .filter(|(i, _)| ids.contains(i))
            .map(|(i, m)| {
                let n = model_label(s, &i, &m);
                (i, m, n)
            })
            .collect();
    }
    let hidden = server_settings(s)["providerModelPreferences"][rail]["hiddenModels"].clone();
    let Some(p) = providers(s).iter().find(|p| p["instanceId"] == rail) else { return Vec::new() };
    p["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| !hidden.as_array().is_some_and(|h| h.contains(&m["slug"])))
        .map(|m| (rail.to_owned(), data::s(m, "slug").to_owned(), data::s(m, "name").to_owned()))
        .collect()
}

fn rows(s: &App, key: &str, rail: &str) -> Vec<(String, String, String)> {
    let query = s.ui.settings.models.query.text.trim().to_lowercase();
    if query.is_empty() {
        return models_for(s, key, rail);
    }
    // Searching covers every rail entry, as the web's flat search does.
    rail_entries(s, key)
        .iter()
        .flat_map(|p| models_for(s, key, data::s(p, "instanceId")))
        .filter(|(_, slug, name)| name.to_lowercase().contains(&query) || slug.to_lowercase().contains(&query))
        .collect()
}

fn pick(s: &mut App, key: &str, instance: String, model: String) {
    close(s);
    if !providers(s).iter().any(|p| p["instanceId"] == instance.as_str() && ready(p)) {
        return;
    }
    update(s, key, json!({"instanceId": instance, "model": model}));
}

/// The rail entry shown: the one picked, else the current selection's provider.
fn shown_rail(s: &App, key: &str) -> String {
    s.ui.settings
        .models
        .rail
        .clone()
        .or_else(|| selection(s, key).map(|(i, _)| i))
        .unwrap_or_else(|| FAVORITES.into())
}

/// Arrow keys wrap through the rows, starting from the first (Down) or last (Up).
fn step(active: Option<usize>, len: usize, down: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match (active, down) {
        (None, true) => 0,
        (None, false) => len - 1,
        (Some(a), true) => (a + 1) % len,
        (Some(a), false) => (a + len - 1) % len,
    })
}

fn on_key(s: &mut App, e: &rune::KeyEvent, _tx: &mut rune::Tx) -> bool {
    let Some(key) = s.ui.settings.models.open.clone() else { return false };
    let list = rows(s, &key, &shown_rail(s, &key));
    let p = &mut s.ui.settings.models;
    match e.key {
        rune::Key::Down => p.active = step(p.active, list.len(), true),
        rune::Key::Up => p.active = step(p.active, list.len(), false),
        rune::Key::Enter => {
            if let Some((i, m, _)) = list.into_iter().nth(p.active.unwrap_or(0)) {
                pick(s, &key, i, m);
            }
        }
        _ => return false,
    }
    true
}

/// The open picker's popover, anchored under its trigger.
pub fn popover(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(key) = s.ui.settings.models.open.clone() else { return };
    let current = selection(s, &key);
    let rail = shown_rail(s, &key);
    let line = t.c.border.alpha_mul(0.7);

    // Rail: w-11 p-1 gap-1, square buttons, a primary bar on the shown entry.
    let rail_item = |id: String, mark: El, active: bool, enabled: bool, name: String| {
        let target = id.clone();
        let mut item = el()
            .key(format!("settings-model-rail-{id}"))
            .size(36.0, 36.0)
            .center()
            .radius(radius::MD)
            .label(name)
            .child(mark);
        item = if enabled {
            item.cursor(Cursor::Pointer).hover(t.c.foreground.alpha_mul(0.1)).on_press(move |s: &mut App| {
                s.ui.settings.models.rail = Some(target.clone());
                s.ui.settings.models.active = None;
            })
        } else {
            item.opacity(0.5)
        };
        let mut wrap = el().w(36.0).h(36.0).child(item);
        if active {
            wrap = wrap.child(el().absolute(36.0, 8.0).size(3.0, 20.0).radius(radius::FULL).fill(t.c.primary));
        }
        wrap
    };
    let mut rail_col = col().w(44.0).h_full().padding(4.0).gap(4.0).fill(t.c.muted.alpha_mul(0.3));
    rail_col = rail_col.child(rail_item(
        FAVORITES.into(),
        icon(&STAR_FILLED, 20.0).color(t.c.foreground),
        rail == FAVORITES,
        true,
        "Favorites".into(),
    ));
    rail_col = rail_col.child(el().w_full().h(1.0).fill(line));
    for p in rail_entries(s, &key) {
        let id = data::s(p, "instanceId").to_owned();
        rail_col = rail_col.child(rail_item(
            id.clone(),
            provider_icon(t, data::s(p, "driver"), 20.0),
            rail == id,
            true,
            data::s(p, "displayName").to_owned(),
        ));
    }

    // List: the search field over the models.
    let q = &s.ui.settings.models.query;
    let search = row()
        .w_full()
        .h(26.0)
        .center_y()
        .child(icon(&icons::SEARCH, 16.0).color(t.c.muted_foreground.alpha_mul(0.55)).absolute(0.0, 5.0))
        .child(
            text_input(q, theme::TEXT_SM.lh(26.0))
                .key("settings-model-search")
                .single_line()
                .w_full()
                .pl(20.0)
                .color(t.c.foreground)
                .placeholder("Search models...", t.c.placeholder)
                .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
                .on_edit(|s: &mut App| &mut s.ui.settings.models.query)
                .on_input_change(|s: &mut App, _| s.ui.settings.models.active = None)
                .on_input_key(on_key),
        );
    let header = col()
        .w_full()
        .px(12.0)
        .pt(10.0)
        .child(col().w_full().pb(6.0).child(search))
        .child(el().w_full().h(1.0).fill(if q.focused { t.c.ring } else { line }));
    let list = rows(s, &key, &rail);
    let searching = !q.text.trim().is_empty();
    let mut body = col().w_full().pl(8.0).pr(1.0).py(6.0).gap(2.0);
    for (n, (instance, slug, name)) in list.iter().enumerate() {
        let driver = providers(s)
            .iter()
            .find(|p| p["instanceId"] == instance.as_str())
            .map(|p| data::s(p, "driver").to_owned())
            .unwrap_or_default();
        let selected = current.as_ref().is_some_and(|(i, m)| i == instance && m == slug)
            && !(key == "fallbackModelSelection" && super::value(s, &key).is_null());
        let mut label = row().grow(1.0).min_w(0.0).gap(8.0).center_y().child(
            text(name.clone()).font(theme::TEXT_SM.lh(19.25)).color(t.c.foreground).truncate().shrink(),
        );
        if selected {
            label = label.child(icon(&icons::CHECK, 14.0).color(t.c.foreground));
        }
        let (k, i, m) = (key.clone(), instance.clone(), slug.clone());
        let active = s.ui.settings.models.active == Some(n);
        body = body.child(
            row()
                .key(format!("settings-model-{instance}-{slug}"))
                .fill(if active { t.c.accent } else { rune::Color::TRANSPARENT })
                .w_full()
                .h(32.0)
                .px(8.0)
                .gap(8.0)
                .center_y()
                .radius(radius::SM)
                .cursor(Cursor::Pointer)
                .hover(t.c.foreground.alpha_mul(0.09))
                .role("option")
                .label(name.clone())
                .child(provider_icon(t, &driver, 16.0))
                .child(label)
                .on_press(move |s: &mut App| pick(s, &k, i.clone(), m.clone())),
        );
    }
    let mut column = col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .fill(t.c.muted.alpha_mul(0.4))
        .child(el().absolute(0.0, 0.0).w(1.0).h_full().fill(line))
        .child(header)
        .child(col().w_full().grow(1.0).min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(body));
    // `ModelPickerProviderNotices`: a provider that isn't ready offers its setup.
    let setup = providers(s).iter().find(|p| p["instanceId"] == rail.as_str() && !ready(p) && !searching);
    if list.is_empty() {
        if let Some(p) = setup {
            let id = data::s(p, "instanceId").to_owned();
            column = column.child(
                col()
                    .w_full()
                    .padding(8.0)
                    .child(el().absolute(0.0, 0.0).w_full().h(1.0).fill(line))
                    .child(
                        col()
                            .px(4.0)
                            .py(6.0)
                            .gap(4.0)
                            .child(
                                text(data::s(p, "message").to_owned())
                                    .font(theme::TEXT_XS.lh(16.5))
                                    .color(t.c.muted_foreground)
                                    .wrap(),
                            )
                            .child(
                                text("Open provider setup")
                                    .font(theme::TEXT_XS)
                                    .color(t.c.foreground)
                                    .cursor(Cursor::Pointer)
                                    .on_press(move |s: &mut App| {
                                        close(s);
                                        s.ui.settings.provider = Some(id.clone());
                                        s.navigate(Route::Settings("providers".into()));
                                    }),
                            ),
                    ),
            );
        } else {
            column = column.child(
                text(if rail == FAVORITES && !searching { "No favorites yet" } else { "No models found" })
                    .font(theme::TEXT_SM)
                    .color(t.c.muted_foreground)
                    .px(16.0)
                    .pb(16.0),
            );
        }
    }

    // `min(calc(var(--spacing) * (rows + chrome)) + 1px, 86.5 units)` over the tallest tab.
    let tallest = std::iter::once(models_for(s, &key, FAVORITES).len())
        .chain(rail_entries(s, &key).iter().map(|p| models_for(s, &key, data::s(p, "instanceId")).len()))
        .max()
        .unwrap_or(0) as f32;
    let height = (4.0 * (15.5 + 8.5 * tallest - 0.5) + 1.0).clamp(0.0, 346.0);
    let card = row()
        .key("settings-model-picker")
        .w(360.0)
        .h(height)
        .radius(radius::LG)
        .fill(t.c.popover)
        .border(1.0, t.c.border)
        .shadow(8.0, 24.0, rune::Color::BLACK.alpha(if t.dark { 0.4 } else { 0.12 }))
        .clip_rounded(radius::LG)
        .role("dialog")
        .label("Model picker")
        .child(rail_col)
        .child(column)
        .anchored(&format!("model-trigger-{key}"), Side::Bottom, Align::End, 4.0)
        .modal(true, close);
    layers.push(card);
}

/// Description overrides: the default model says when it is chosen automatically.
pub fn description(s: &App, key: &str) -> Option<String> {
    (key == "defaultModelSelection").then(|| {
        if s.ui.settings.project.is_some() {
            "Model for new threads in this project.".to_owned()
        } else if super::value(s, key).is_null() && !candidates(s, key).is_empty() {
            "Default model for new threads, selected automatically. Projects can override it.".to_owned()
        } else {
            "Default model for new threads. Projects can override it.".to_owned()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selectable_entry_prefers_ready_then_non_erroring() {
        let a = json!({"instanceId":"codex","enabled":true,"status":"error"});
        let b = json!({"instanceId":"opencode","enabled":true,"status":"ready"});
        let c = json!({"instanceId":"off","enabled":false,"status":"ready"});
        let entries = vec![&a, &b, &c];
        assert_eq!(selectable_entry(&entries, Some("codex")).unwrap()["instanceId"], "codex");
        assert_eq!(selectable_entry(&entries, Some("off")).unwrap()["instanceId"], "opencode");
        assert_eq!(selectable_entry(&entries, None).unwrap()["instanceId"], "opencode");
    }
    #[test]
    fn arrow_keys_wrap_through_the_rows() {
        assert_eq!(step(None, 3, true), Some(0));
        assert_eq!(step(None, 3, false), Some(2));
        assert_eq!(step(Some(2), 3, true), Some(0));
        assert_eq!(step(Some(0), 3, false), Some(2));
        assert_eq!(step(Some(1), 0, true), None);
    }
    #[test]
    fn default_model_skips_custom_models() {
        let p = json!({"models":[{"slug":"c","isCustom":true,"isDefault":true},{"slug":"a"},{"slug":"b","isDefault":true}]});
        assert_eq!(default_model(&p).as_deref(), Some("b"));
    }
}
