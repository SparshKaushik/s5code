//! The settings model picker (`ProviderModelPicker` without the composer's effort column): a
//! bordered trigger (`SETTINGS_PICKER_TRIGGER_CLASSNAME`) and a popover with the provider rail and
//! the selected provider's models. Rows: default, fallback and text generation model, and the
//! source control writer and review guide models, which a switch turns on (web
//! `SourceControlWritingSettings`).

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
    matches!(key, "defaultModelSelection" | "fallbackModelSelection" | "textGenerationModelSelection") || dedicated(key)
}

const WRITER: &str = "sourceControlWriterModelSelection";
const REVIEW_GUIDE: &str = "reviewGuideModelSelection";

/// A model that is off (`null`) until its switch turns it on, and otherwise falls back.
fn dedicated(key: &str) -> bool {
    matches!(key, WRITER | REVIEW_GUIDE)
}

/// What a dedicated key uses while off or unusable: the writer falls back to the text generation
/// model, the review guide to the writer (`resolveReviewGuideModelSelection`).
fn fallback_key(key: &str) -> &'static str {
    if key == REVIEW_GUIDE { WRITER } else { "textGenerationModelSelection" }
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

/// The scheduled task editor's model: its environment's providers and its draft, not a setting.
pub const TASK: &str = "scheduledTaskModel";

fn source<'a>(s: &'a App, key: &str) -> &'a [Value] {
    if key == TASK { super::tasks::model_providers(s) } else { providers(s) }
}

/// The settings holding a key's hidden models and favorites.
fn settings_of<'a>(s: &'a App, key: &str) -> &'a Value {
    if key == TASK { super::tasks::model_settings(s) } else { server_settings(s) }
}

/// The providers a key picks from: text generation skips providers that can't generate text.
fn candidates<'a>(s: &'a App, key: &str) -> Vec<&'a Value> {
    source(s, key)
        .iter()
        .filter(|p| !(key == "textGenerationModelSelection" || dedicated(key)) || p["supportsTextGeneration"] != false)
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
    if key == TASK {
        return super::tasks::model(s);
    }
    let entries = candidates(s, key);
    let stored = super::value(s, key);
    if key == "fallbackModelSelection" {
        return pair(&stored).or_else(|| selection(s, "defaultModelSelection"));
    }
    if dedicated(key) {
        let usable = stored["instanceId"].as_str().is_some_and(|id| entries.iter().any(|p| p["instanceId"] == id && selectable(p)));
        return if usable { pair(&stored) } else { selection(s, fallback_key(key)) };
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

fn model_label(s: &App, key: &str, instance: &str, model: &str) -> String {
    if key != TASK {
        return crate::chat::composer::model_name(s, instance, model);
    }
    let named = source(s, key).iter().find(|p| p["instanceId"] == instance).and_then(|p| p["models"].as_array()?.iter().find(|m| m["slug"] == model));
    named.map(|m| data::s(m, "name")).filter(|n| !n.is_empty()).unwrap_or(model).to_owned()
}

/// Web's dedicated-model control: a muted line without a server, else the picker while on, then
/// the switch. Turning it on starts from what it was falling back to.
fn dedicated_control(s: &App, t: &T, key: &str) -> El {
    let none = |copy: &str| text(copy.to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground);
    let (noun, aria) = if key == WRITER {
        ("source control writer model", "Use a separate source control writer model")
    } else {
        ("review guide model", "Use a separate review guide model")
    };
    if s.state().config.is_none() {
        return none(&format!("Connect an environment to choose its {noun}."));
    }
    let on = !super::value(s, key).is_null();
    let start = selection(s, fallback_key(key));
    let usable = can_enable(&candidates(s, key), start.as_ref().map(|(i, _)| i.as_str()));
    let mut line = row().gap(8.0).center_y().justify_end();
    if on && !usable && key == WRITER {
        line = line.child(none("No text generation providers available."));
    }
    if on && usable {
        line = line.child(trigger(s, t, key));
        if let Some(traits) = traits_trigger(s, t, key) {
            line = line.child(traits);
        }
    }
    let owned = key.to_owned();
    line.child(
        crate::kit::Switch::new(on)
            .label(aria)
            .hide_label()
            .disabled(!on && !usable)
            .on_toggle(move |s: &mut App| {
                let next = if on {
                    Value::Null
                } else {
                    match selection(s, fallback_key(&owned)) {
                        Some((instance, model)) => json!({"instanceId": instance, "model": model}),
                        None => return,
                    }
                };
                update(s, &owned, next);
            })
            .build(&crate::kit::theme(t))
            .key(format!("setting-{key}")),
    )
}

const TRAITS: &str = "traits:";

/// Option ids of a reasoning control (`REASONING_DESCRIPTOR_IDS`), as the composer's own list.
const REASONING_IDS: [&str; 7] = ["reasoningEffort", "effort", "reasoning", "variant", "reasoning_effort", "thinking_level", "thinking"];

/// `isReasoningSelect`.
fn is_reasoning_select(d: &Value) -> bool {
    d["type"] == "select" && (d["role"] == "reasoning" || REASONING_IDS.contains(&data::s(d, "id")))
}

fn default_id(options: &[Value]) -> Option<String> {
    options.iter().find(|o| o["isDefault"] == true).and_then(|o| o["id"].as_str()).map(str::to_owned)
}

fn options_of(d: &Value) -> &[Value] {
    d["options"].as_array().map_or(&[], Vec::as_slice)
}

fn injected(d: &Value, id: &str) -> bool {
    d["promptInjectedValues"].as_array().is_some_and(|v| v.iter().any(|v| v.as_str() == Some(id)))
}

/// `resolveDescriptorChoiceValue`: a saved pick the select still offers, else its own value.
fn choice(d: &Value, raw: Option<&str>) -> Option<String> {
    let options = options_of(d);
    let fallback = || d["currentValue"].as_str().map(str::to_owned).or_else(|| default_id(options));
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else { return fallback() };
    if options.is_empty() {
        return Some(raw.to_owned());
    }
    let known = options.iter().any(|o| o["id"].as_str() == Some(raw));
    if known && injected(d, raw) {
        return default_id(options);
    }
    if known { Some(raw.to_owned()) } else { fallback() }
}

/// `getProviderOptionDescriptors` over the model's capabilities, without the plan agent unless
/// plan mode is on (`getProviderModelCapabilities`).
fn trait_descriptors(caps: &[Value], saved: &[Value], plan: bool) -> Vec<Value> {
    caps.iter()
        .filter_map(|d| {
            let mut d = d.clone();
            if !plan && d["type"] == "select" && d["id"] == "agent" {
                let options: Vec<Value> = options_of(&d).iter().filter(|o| o["id"] != "plan").cloned().collect();
                if options.is_empty() {
                    return None;
                }
                let current = d["currentValue"]
                    .as_str()
                    .filter(|c| options.iter().any(|o| o["id"].as_str() == Some(*c)))
                    .map(str::to_owned)
                    .or_else(|| default_id(&options))
                    .or_else(|| options[0]["id"].as_str().map(str::to_owned));
                d["options"] = Value::Array(options);
                if let Some(c) = current {
                    d["currentValue"] = json!(c);
                }
            }
            let raw = saved.iter().find(|o| o["id"] == d["id"]).map(|o| o["value"].clone());
            if d["type"] == "boolean" {
                if let Some(v @ Value::Bool(_)) = raw {
                    d["currentValue"] = v;
                }
            } else {
                match choice(&d, raw.as_ref().and_then(Value::as_str)) {
                    Some(c) => d["currentValue"] = json!(c),
                    None => {
                        if let Some(o) = d.as_object_mut() {
                            o.remove("currentValue");
                        }
                    }
                }
            }
            Some(d)
        })
        .collect()
}

/// `getProviderOptionCurrentValue` without a selection.
fn current(d: &Value) -> Option<Value> {
    if d["type"] == "boolean" {
        return d["currentValue"].is_boolean().then(|| d["currentValue"].clone());
    }
    d["currentValue"].as_str().filter(|c| !c.is_empty()).map(str::to_owned).or_else(|| default_id(options_of(d))).map(Value::String)
}

/// `buildProviderOptionSelectionsFromDescriptors`.
fn selections(ds: &[Value]) -> Vec<Value> {
    ds.iter().filter_map(|d| Some(json!({"id": d["id"], "value": current(d)?}))).collect()
}

/// `getTraitsSectionVisibility(...).hasAnyControls`.
fn has_controls(ds: &[Value]) -> bool {
    ds.iter().any(|d| d["type"] == "select" || (d["type"] == "boolean" && matches!(data::s(d, "id"), "thinking" | "fastMode")))
}

/// `buildTraitsTriggerDisplay(...).label` with no prompt-controlled Ultrathink.
fn traits_label(driver: &str, ds: &[Value]) -> String {
    let (mut fallback, mut speed, mut reasoning) = (None::<String>, None::<&str>, None::<usize>);
    let mut labels: Vec<String> = Vec::new();
    for d in ds {
        let id = data::s(d, "id");
        let boolean = d["type"] == "boolean";
        if id == "fastMode" && boolean {
            speed = (d["currentValue"] == true).then_some("Fast");
            fallback = Some(speed.unwrap_or("Normal").to_owned());
            continue;
        }
        let options = options_of(d);
        let label_of = |id: &str| options.iter().find(|o| o["id"].as_str() == Some(id)).map(|o| data::s(o, "label").to_owned());
        let value = current(d).and_then(|v| v.as_str().map(str::to_owned));
        if driver == "codex" && id == "serviceTier" && !boolean {
            let value = value.as_deref().unwrap_or("");
            let tier = |name: &str| options.iter().find(|o| o["label"] == name).map(|o| data::s(o, "id"));
            let (fast, ultra) = (tier("Fast"), tier("Ultrafast"));
            if ((fast.is_some() || ultra.is_some()) && value == "default") || fast == Some(value) || ultra == Some(value) {
                speed = if ultra == Some(value) {
                    Some("Ultrafast")
                } else if fast == Some(value) {
                    Some("Fast")
                } else {
                    None
                };
                fallback = Some(label_of(value).unwrap_or_else(|| "Normal".into()));
                continue;
            }
        }
        let label = if boolean {
            format!("{} {}", data::s(d, "label"), if d["currentValue"] == true { "On" } else { "Off" })
        } else {
            value.as_deref().and_then(label_of).unwrap_or_else(|| if id == "variant" { "Unknown".into() } else { String::new() })
        };
        if !label.is_empty() {
            if reasoning.is_none() && is_reasoning_select(d) {
                reasoning = Some(labels.len());
            }
            labels.push(label);
        }
    }
    if labels.is_empty() {
        if let Some(fallback) = fallback {
            return fallback;
        }
    }
    if let Some(speed) = speed {
        match reasoning {
            Some(i) => labels[i] = format!("{} {speed}", labels[i]),
            None => labels.push(speed.to_owned()),
        }
    }
    labels.join(" · ")
}

/// The stored dedicated selection's driver and option descriptors, when web's `TraitsPicker`
/// has anything to show for it.
fn traits(s: &App, key: &str) -> Option<(String, Vec<Value>)> {
    let stored = super::value(s, key);
    let (instance, model) = pair(&stored)?;
    let p = providers(s).iter().find(|p| p["instanceId"] == instance.as_str())?;
    let caps = p["models"].as_array()?.iter().find(|m| m["slug"] == model.as_str())?["capabilities"]["optionDescriptors"]
        .as_array()?
        .clone();
    let saved = stored["options"].as_array().cloned().unwrap_or_default();
    let plan = super::value(s, "planModeEnabled") == true || saved.iter().any(|o| o["id"] == "agent" && o["value"] == "plan");
    let ds = trait_descriptors(&caps, &saved, plan);
    has_controls(&ds).then(|| (data::s(p, "driver").to_owned(), ds))
}

/// `createModelSelection`: options only when there are any.
fn with_options(instance: &str, model: &str, options: Vec<Value>) -> Value {
    let mut v = json!({"instanceId": instance, "model": model});
    if !options.is_empty() {
        v["options"] = Value::Array(options);
    }
    v
}

fn set_trait(s: &mut App, key: &str, id: &str, value: Value) {
    let Some((_, mut ds)) = traits(s, key) else { return };
    let Some((instance, model)) = pair(&super::value(s, key)) else { return };
    if let Some(d) = ds.iter_mut().find(|d| d["id"] == id) {
        d["currentValue"] = value;
    }
    update(s, key, with_options(&instance, &model, selections(&ds)));
}

/// Web's `TraitsPicker` beside a dedicated model: a gauge and the current traits.
fn traits_trigger(s: &App, t: &T, key: &str) -> Option<El> {
    let (driver, ds) = traits(s, key)?;
    let label = traits_label(&driver, &ds);
    let id = format!("{TRAITS}{key}");
    let open = s.ui.settings.models.open.as_deref() == Some(id.as_str());
    let mut trigger = row()
        .key(format!("traits-trigger-{key}"))
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
        .label(label.clone())
        .expanded(open)
        .on_press(move |s: &mut App| {
            let p = &mut s.ui.settings.models;
            p.open = if p.open.as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) };
        });
    if open {
        trigger = trigger.fill(t.c.accent);
    }
    Some(
        trigger
            .child(icon(&icons::GAUGE, 16.0).color(t.c.muted_foreground))
            .child(text(label).font(theme::TEXT_SM).color(t.c.foreground).truncate().shrink()),
    )
}

/// `TraitsMenuContent`: each select's choices, then each switch's On/Off.
fn traits_menu(s: &App, t: &T, key: &str, layers: &mut Vec<El>) {
    let Some((_, ds)) = traits(s, key) else { return };
    let close = |s: &mut App| s.ui.settings.models.open = None;
    let mut menu = crate::kit::Menu::new(format!("traits-{key}"))
        .anchor(format!("traits-trigger-{key}"))
        .placement(rune_kit::Placement::BottomStart)
        .open(true)
        .on_close(close);
    let groups = ds.iter().filter(|d| d["type"] != "boolean").chain(ds.iter().filter(|d| d["type"] == "boolean"));
    for (n, d) in groups.enumerate() {
        if n > 0 {
            menu = menu.separator();
        }
        menu = menu.group_label(data::s(d, "label"));
        let picked = current(d);
        let choices: Vec<(Value, String, bool, String)> = if d["type"] == "boolean" {
            [true, false].into_iter().map(|on| (Value::Bool(on), (if on { "On" } else { "Off" }).to_owned(), false, String::new())).collect()
        } else {
            options_of(d)
                .iter()
                .filter(|o| !injected(d, data::s(o, "id")))
                .map(|o| (o["id"].clone(), data::s(o, "label").to_owned(), o["isDefault"] == true, data::s(o, "description").to_owned()))
                .collect()
        };
        for (value, label, default, description) in choices {
            let mut item = crate::kit::MenuItem::new(label).radio(picked.as_ref() == Some(&value));
            if default {
                item = item.trailing(crate::kit::badge("Default", "outline", "sm", t));
            }
            if !description.is_empty() {
                item = item.description(description);
            }
            let (k, id) = (key.to_owned(), data::s(d, "id").to_owned());
            menu = menu.item(item.on_select(move |s: &mut App| {
                close(s);
                set_trait(s, &k, &id, value.clone());
            }));
        }
    }
    menu.overlay(&crate::kit::theme(t), layers);
}

/// `canEnableDedicatedModel`: what the model falls back to comes from a usable provider.
fn can_enable(entries: &[&Value], instance: Option<&str>) -> bool {
    instance.is_some_and(|id| entries.iter().any(|p| p["instanceId"] == id && selectable(p)))
}

/// The row's control: the trigger, or the web's muted fallback copy.
pub fn control(s: &App, t: &T, key: &str) -> El {
    if dedicated(key) {
        return dedicated_control(s, t, key);
    }
    trigger(s, t, key)
}

fn trigger(s: &App, t: &T, key: &str) -> El {
    let entries = candidates(s, key);
    let none = |copy: &str| text(copy.to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground);
    if key == "textGenerationModelSelection" && s.state().config.is_none() {
        return none("Connect an environment to choose its text generation model.");
    }
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
        model_label(s, key, &instance, &model)
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
            WRITER => "Source control writer model".to_owned(),
            REVIEW_GUIDE => "Review guide model".to_owned(),
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

fn favorites(s: &App, key: &str) -> Vec<(String, String)> {
    let list = settings_of(s, key)["favorites"].as_array().cloned().unwrap_or_default();
    list.iter()
        .filter_map(|f| Some((f["provider"].as_str()?.to_owned(), f["model"].as_str()?.to_owned())))
        .collect()
}

/// `(instance, slug, name)` for a rail entry, hidden models left out.
fn models_for(s: &App, key: &str, rail: &str) -> Vec<(String, String, String)> {
    if rail == FAVORITES {
        let ids: Vec<_> = rail_entries(s, key).iter().map(|p| data::s(p, "instanceId").to_owned()).collect();
        return favorites(s, key)
            .into_iter()
            .filter(|(i, _)| ids.contains(i))
            .map(|(i, m)| {
                let n = model_label(s, key, &i, &m);
                (i, m, n)
            })
            .collect();
    }
    let hidden = settings_of(s, key)["providerModelPreferences"][rail]["hiddenModels"].clone();
    let Some(p) = source(s, key).iter().find(|p| p["instanceId"] == rail) else { return Vec::new() };
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
    if !source(s, key).iter().any(|p| p["instanceId"] == instance.as_str() && ready(p)) {
        return;
    }
    if key == TASK {
        return super::tasks::set_model(s, &instance, &model);
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
    if let Some(key) = key.strip_prefix(TRAITS) {
        return traits_menu(s, t, key, layers);
    }
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
        let driver = source(s, &key)
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
    let setup = source(s, &key).iter().find(|p| p["instanceId"] == rail.as_str() && !ready(p) && !searching);
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
    #[test]
    fn dedicated_models_fall_back_like_resolve_review_guide_model_selection() {
        assert_eq!(fallback_key(REVIEW_GUIDE), WRITER);
        assert_eq!(fallback_key(WRITER), "textGenerationModelSelection");
        assert!(is_model_key(WRITER) && is_model_key(REVIEW_GUIDE));
        let on = json!({"instanceId": "codex", "enabled": true, "availability": "available"});
        let off = json!({"instanceId": "claude", "enabled": false});
        let entries = [&on, &off];
        assert!(can_enable(&entries, Some("codex")));
        assert!(!can_enable(&entries, Some("claude")), "a disabled provider can't back a dedicated model");
        assert!(!can_enable(&entries, None));
    }
    #[test]
    fn traits_apply_saved_options_like_get_provider_option_descriptors() {
        let caps = vec![
            json!({"id":"reasoningEffort","label":"Reasoning","type":"select","promptInjectedValues":["ultrathink"],
                "options":[{"id":"low","label":"Low"},{"id":"high","label":"High","isDefault":true},{"id":"ultrathink","label":"Ultrathink"}]}),
            json!({"id":"agent","label":"Agent","type":"select","currentValue":"plan","options":[{"id":"build","label":"Build"},{"id":"plan","label":"Plan"}]}),
            json!({"id":"fastMode","label":"Fast mode","type":"boolean"}),
        ];
        let saved = vec![json!({"id":"reasoningEffort","value":"low"}), json!({"id":"fastMode","value":true})];
        let ds = trait_descriptors(&caps, &saved, false);
        assert_eq!(ds[0]["currentValue"], "low");
        assert_eq!(ds[1]["options"].as_array().unwrap().len(), 1, "plan mode off drops the plan agent");
        assert_eq!(ds[1]["currentValue"], "build");
        assert_eq!(ds[2]["currentValue"], true);
        assert_eq!(
            selections(&ds),
            vec![json!({"id":"reasoningEffort","value":"low"}), json!({"id":"agent","value":"build"}), json!({"id":"fastMode","value":true})]
        );
        // A saved prompt-injected or unknown level falls back to the default choice.
        assert_eq!(trait_descriptors(&caps, &[json!({"id":"reasoningEffort","value":"ultrathink"})], true)[0]["currentValue"], "high");
        assert_eq!(trait_descriptors(&caps, &[json!({"id":"reasoningEffort","value":"max"})], true)[0]["currentValue"], "high");
        assert_eq!(trait_descriptors(&caps, &[], true)[1]["currentValue"], "plan");
        assert!(has_controls(&ds));
        assert!(!has_controls(&[json!({"id":"other","type":"boolean"})]));
        assert_eq!(with_options("codex", "gpt", vec![]), json!({"instanceId":"codex","model":"gpt"}));
    }
    #[test]
    fn traits_label_follows_build_traits_trigger_display() {
        let effort = json!({"id":"reasoningEffort","label":"Reasoning","type":"select","currentValue":"high",
            "options":[{"id":"high","label":"High"}]});
        let fast = |on: bool| json!({"id":"fastMode","label":"Fast mode","type":"boolean","currentValue":on});
        assert_eq!(traits_label("claude", &[effort.clone(), fast(true)]), "High Fast");
        assert_eq!(traits_label("claude", &[fast(false)]), "Normal");
        let thinking = json!({"id":"thinking","label":"Thinking","type":"boolean","currentValue":false});
        assert_eq!(traits_label("claude", &[effort.clone(), thinking]), "High · Thinking Off");
        let tier = |v: &str| json!({"id":"serviceTier","label":"Speed","type":"select","currentValue":v,
            "options":[{"id":"default","label":"Standard"},{"id":"priority","label":"Fast"}]});
        assert_eq!(traits_label("codex", &[effort.clone(), tier("priority")]), "High Fast");
        assert_eq!(traits_label("codex", &[tier("default")]), "Standard");
        assert_eq!(traits_label("claude", &[tier("priority")]), "Fast");
    }
}
