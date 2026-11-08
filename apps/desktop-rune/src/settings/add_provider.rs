//! Web `AddProviderInstanceDialog` for built-in drivers: Provider, Identity and Config steps,
//! then a `create` mutation. ACP Registry search and ChatGPT account sign-in aren't ported.
use super::{providers, request, server_settings};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons, kit};
use rune::{El, TextEdit, col, el, row, text};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

/// The id the accent picker uses while it edits the wizard's draft.
pub const DRAFT_ID: &str = "+add-provider";
const STEPS: [&str; 3] = ["Provider", "Identity", "Config"];

/// `providerClients` without ACP Registry; `featured: false` drivers sit under Show more.
const DRIVERS: &[(&str, &str, bool, Option<&str>)] = &[
    ("codex", "Codex", true, None),
    ("claudeAgent", "Claude", true, None),
    ("cursor", "Cursor", true, None),
    ("grok", "Grok", true, None),
    ("fx", "fx", false, None),
    ("opencode", "OpenCode", true, None),
    ("antigravity", "Antigravity", true, None),
    ("kiro", "Kiro", false, Some("Early Access")),
    ("muse", "Muse Code", true, None),
    ("pi", "Pi", true, None),
];

#[derive(Default)]
pub struct Wizard {
    step: usize,
    driver: Option<&'static str>,
    more: bool,
    /// Per driver, like web's `identityByDriver` / `configByDriver`.
    label: HashMap<&'static str, TextEdit>,
    id: HashMap<&'static str, TextEdit>,
    pub accent: HashMap<&'static str, String>,
    config: HashMap<String, TextEdit>,
    attempted: bool,
}

pub fn open(s: &mut App) {
    s.ui.settings.add_provider = Wizard::default();
    s.ui.settings.provider_picker = true;
}

fn wizard(s: &mut App) -> &mut Wizard {
    &mut s.ui.settings.add_provider
}

pub fn driver(w: &Wizard) -> &'static str {
    w.driver.unwrap_or("codex")
}

fn driver_label(driver: &str) -> &'static str {
    DRIVERS.iter().find(|d| d.0 == driver).map_or("Codex", |d| d.1)
}

/// `slugifyLabel` + `deriveInstanceId`.
fn derive_id(driver: &str, label: &str) -> String {
    let mut slug = String::new();
    for c in label.trim().to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('_') {
            slug.push('_');
        }
    }
    let slug: String = slug.trim_matches('_').chars().take(48).collect();
    if slug.is_empty() { String::new() } else { format!("{driver}_{slug}") }
}

/// `deriveAvailableInstanceId`: the label's id, suffixed `_2`, `_3`… past taken ones. The
/// driver's default label (or none) keeps the driver's own slot id.
fn available_id(driver: &str, label: &str, existing: &[String]) -> String {
    let base = if label.trim().is_empty() || label == driver_label(driver) { driver.to_owned() } else { derive_id(driver, label) };
    if base.is_empty() || !existing.contains(&base) {
        return base;
    }
    (2..)
        .map(|n: usize| {
            let suffix = format!("_{n}");
            format!("{}{suffix}", &base[..base.len().min(64 - suffix.len())])
        })
        .find(|candidate| !existing.contains(candidate))
        .unwrap_or(base)
}

fn validate_id(id: &str, existing: &[String]) -> Option<String> {
    if id.is_empty() {
        return Some("Instance ID is required.".into());
    }
    if id.len() > 64 {
        return Some("Instance ID must be 64 characters or fewer.".into());
    }
    let mut chars = id.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) || !chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Some("Instance ID must start with a letter and use only letters, digits, '-', or '_'.".into());
    }
    existing.contains(&id.to_owned()).then(|| format!("An instance named '{id}' already exists."))
}

/// Codex and Claude's default ids are reserved because they run before configuration.
fn existing(s: &App) -> Vec<String> {
    let mut ids = vec!["codex".to_owned(), "claudeAgent".to_owned()];
    ids.extend(server_settings(s)["providerInstances"].as_object().into_iter().flatten().map(|(id, _)| id.clone()));
    ids
}

fn label_text(w: &Wizard) -> String {
    let driver = driver(w);
    w.label.get(driver).map_or_else(|| driver_label(driver).to_owned(), |e| e.text.clone())
}

fn instance_id(s: &App) -> String {
    let w = &s.ui.settings.add_provider;
    let driver = driver(w);
    match w.id.get(driver) {
        Some(edit) => edit.text.clone(),
        None => available_id(driver, &label_text(w), &existing(s)),
    }
}

fn config_key(driver: &str, field: &str) -> String {
    format!("{driver}::{field}")
}

/// `resolveWizardNavigation`: moving past Identity needs a valid instance id.
fn navigate(s: &mut App, step: usize) {
    let error = validate_id(&instance_id(s), &existing(s));
    let w = wizard(s);
    let step = step.min(STEPS.len() - 1);
    if w.step <= 1 && step > 1 && error.is_some() {
        w.attempted = true;
        w.step = 1;
        return;
    }
    w.step = step;
}

fn save(s: &mut App) {
    let id = instance_id(s);
    let existing = existing(s);
    let w = wizard(s);
    w.attempted = true;
    if validate_id(&id, &existing).is_some() {
        w.step = 1;
        return;
    }
    let driver = driver(w);
    let label = label_text(w).trim().to_owned();
    let mut config = Map::new();
    for (field, ..) in providers::runtime_fields(driver, &Value::Null) {
        if let Some(value) = w.config.get(&config_key(driver, field)).map(|e| e.text.trim().to_owned()).filter(|v| !v.is_empty()) {
            config.insert(field.into(), json!(value));
        }
    }
    if driver == "codex" {
        config.insert("setupMode".into(), json!("existing"));
    }
    let mut instance = json!({"driver": driver, "enabled": true});
    if !label.is_empty() {
        instance["displayName"] = json!(label);
    }
    if let Some(accent) = w.accent.get(driver).and_then(|a| super::accent::normalize(a)) {
        instance["accentColor"] = json!(accent);
    }
    if !config.is_empty() {
        instance["config"] = Value::Object(config);
    }
    s.ui.settings.provider_picker = false;
    s.ui.settings.provider = Some(id.clone());
    request(s, "server.updateSettings", json!({"patch": {}, "providerInstanceMutation": {"operation": "create", "instanceId": id, "instance": instance}}));
    let description = format!("{} instance '{id}' was added.", driver_label(driver));
    crate::shell::palette::toast(s, "Provider instance added", Some(&description));
}

fn steps(s: &App, t: &T) -> El {
    let w = &s.ui.settings.add_provider;
    let mut list = row().w_full().gap(4.0).padding(4.0).radius(12.0).fill(t.c.background).border(1.0, t.c.foreground.alpha_mul(0.05)).role("list").label("Setup progress");
    for (index, step) in STEPS.iter().enumerate() {
        let current = index == w.step;
        let (badge_fill, badge_text, ring) = if index < w.step {
            (t.c.primary, t.c.primary_foreground, t.c.primary)
        } else if current {
            (t.c.primary.alpha_mul(0.1), t.c.primary, t.c.primary.alpha_mul(0.3))
        } else {
            (t.c.card, t.c.muted_foreground, t.c.foreground.alpha_mul(0.1))
        };
        let badge = el().size(20.0, 20.0).radius(10.0).fill(badge_fill).border(1.0, ring).center().child(if index < w.step {
            rune::icon(&icons::CHECK, 16.0).color(badge_text)
        } else {
            text((index + 1).to_string()).font(theme::SM_MEDIUM).color(badge_text)
        });
        let mut item = row()
            .key(format!("add-provider-step-{index}"))
            .grow(1.0)
            .min_w(0.0)
            .gap(8.0)
            .px(10.0)
            .py(8.0)
            .radius(8.0)
            .center_y()
            .role("button")
            .label(format!("{step}, step {}", index + 1))
            .cursor(rune::Cursor::Pointer)
            .on_press(move |s: &mut App| navigate(s, index))
            .child(badge)
            .child(text(*step).font(theme::SM_MEDIUM).color(if current { t.c.foreground } else { t.c.muted_foreground }).truncate());
        item = if current { item.fill(t.c.card).border(1.0, t.c.foreground.alpha_mul(0.05)) } else { item.hover(t.c.card) };
        list = list.child(item);
    }
    list
}

fn option_grid(s: &App, t: &T, featured: bool) -> El {
    let selected = driver(&s.ui.settings.add_provider);
    let mut grid = row().w_full().flex_wrap().gap(0.0).role("radiogroup");
    for &(kind, label, _, badge) in DRIVERS.iter().filter(|d| d.2 == featured) {
        let checked = kind == selected;
        let mut option = row()
            .key(format!("add-provider-{kind}"))
            .w_frac(0.5)
            .gap(12.0)
            .px(12.0)
            .py(12.0)
            .radius(8.0)
            .center_y()
            .role("radio")
            .label(label)
            .checked(checked)
            .cursor(rune::Cursor::Pointer)
            .on_press(move |s: &mut App| {
                let w = wizard(s);
                w.driver = Some(kind);
                w.attempted = false;
            })
            .child(crate::chat::composer::provider_icon(t, kind, 16.0))
            .child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground).grow(1.0).min_w(0.0).truncate());
        if checked {
            option = option
                .fill(t.c.primary.alpha_mul(if t.dark { 0.15 } else { 0.08 }))
                .border(2.0, t.c.primary)
                .child(el().size(20.0, 20.0).radius(10.0).fill(t.c.primary).center().child(rune::icon(&icons::CHECK, 14.0).color(t.c.primary_foreground)));
        } else {
            option = option.fill(if t.dark { t.c.foreground.alpha_mul(0.03) } else { t.c.card }).hover(t.c.muted);
        }
        if let Some(badge) = badge {
            option = option.child(kit::badge(badge, "warning", "sm", t));
        }
        grid = grid.child(option);
    }
    grid
}

/// A `SettingsRow` in the wizard's plain group: copy on the left, the control on the right.
fn field_row(t: &T, title: &str, description: &str, error: Option<String>, control: El) -> El {
    let mut copy = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(text(title.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(text(description.to_owned()).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap());
    if let Some(error) = error {
        copy = copy.child(text(error).font(theme::TEXT_XS.lh(18.0)).color(t.c.destructive).wrap().role("alert"));
    }
    row().w_full().py(12.0).gap(32.0).center_y().child(copy).child(control)
}

fn input(t: &T, edit: &TextEdit, placeholder: &str, edit_of: impl for<'a> Fn(&'a mut App) -> &'a mut TextEdit + 'static) -> El {
    kit::Input::new(edit).size("sm").placeholder(placeholder.to_owned()).on_edit(edit_of).build(t).w(224.0).single_line()
}

fn identity(s: &App, t: &T) -> El {
    let w = &s.ui.settings.add_provider;
    let driver = driver(w);
    let label = TextEdit::new(label_text(w));
    let label_edit = w.label.get(driver).cloned().unwrap_or(label);
    let id = instance_id(s);
    let id_edit = w.id.get(driver).cloned().unwrap_or_else(|| TextEdit::new(id.clone()));
    let error = validate_id(&id, &existing(s)).filter(|_| w.attempted);
    col()
        .w_full()
        .child(field_row(
            t,
            "Label",
            "Shown in the provider list.",
            None,
            input(t, &label_edit, "e.g. Work", move |s: &mut App| {
                let initial = label_text(&s.ui.settings.add_provider);
                wizard(s).label.entry(driver).or_insert_with(|| TextEdit::new(initial))
            })
            .label("Label"),
        ))
        .child(field_row(
            t,
            "Instance ID",
            "Letters, digits, '-', or '_'.",
            error,
            input(t, &id_edit, &format!("{driver}_work"), move |s: &mut App| {
                let initial = instance_id(s);
                wizard(s).id.entry(driver).or_insert_with(|| TextEdit::new(initial))
            })
            .label("Instance ID"),
        ))
        .child(field_row(
            t,
            "Accent color",
            "Optional marker shown in the picker.",
            None,
            super::accent::trigger(s, t, DRAFT_ID, &label_text(w)),
        ))
}

fn config(s: &App, t: &T) -> El {
    let w = &s.ui.settings.add_provider;
    let driver = driver(w);
    let fields = providers::runtime_fields(driver, &Value::Null);
    if fields.is_empty() {
        return text("This driver has no required configuration. You can add the instance now.").font(theme::TEXT_SM).color(t.c.muted_foreground).wrap();
    }
    let mut body = col().w_full();
    for (field, title, description, placeholder) in fields {
        let key = config_key(driver, field);
        let edit = w.config.get(&key).cloned().unwrap_or_default();
        let owned = key.clone();
        let mut control = input(t, &edit, placeholder, move |s: &mut App| wizard(s).config.entry(owned.clone()).or_default()).label(title);
        if providers::is_secret_field(driver, field) {
            control = control.secure();
        }
        body = body.child(field_row(t, title, description, None, control));
    }
    body
}

pub fn dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    if !s.ui.settings.provider_picker {
        return;
    }
    let w = &s.ui.settings.add_provider;
    let environment = s.state().config.as_ref().and_then(|c| c["environment"]["label"].as_str()).unwrap_or("this environment");
    let mut panel = col().w_full().gap(16.0).px(24.0).py(20.0).radius(12.0).fill(t.c.background.alpha_mul(0.8)).border(1.0, t.c.foreground.alpha_mul(0.05));
    panel = match w.step {
        0 => {
            let mut step = col()
                .w_full()
                .gap(8.0)
                .child(text("Provider").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(option_grid(s, t, true))
                .child(
                    row().pt(8.0).child(
                        kit::Button::new(if w.more { "Show less" } else { "Show more" })
                            .variant("ghost-muted")
                            .size("xs")
                            .icon(if w.more { &icons::CHEVRON_UP } else { &icons::CHEVRON_DOWN })
                            .on_press(|s: &mut App| wizard(s).more ^= true)
                            .build(t)
                            .expanded(w.more),
                    ),
                );
            if w.more {
                step = step.child(col().w_full().pt(16.0).child(option_grid(s, t, false)));
            }
            panel.child(step)
        }
        1 => panel.child(identity(s, t)),
        _ => panel.child(config(s, t)),
    };
    let step = w.step;
    let mut footer = row().w_full().gap(8.0).child(el().grow(1.0)).child(
        kit::Button::new(if step == 0 { "Cancel" } else { "Back" })
            .variant(if step == 0 { "ghost-muted" } else { "outline" })
            .size("sm")
            .on_press(move |s: &mut App| {
                if step == 0 {
                    s.ui.settings.provider_picker = false;
                } else {
                    wizard(s).step = step - 1;
                }
            })
            .build(t),
    );
    footer = footer.child(match step {
        0 if driver(w) == "codex" => kit::Button::new("Configure manually").variant("outline").size("sm").on_press(|s: &mut App| navigate(s, 1)).build(t),
        0 | 1 => kit::Button::new("Next").size("sm").on_press(move |s: &mut App| navigate(s, step + 1)).build(t),
        _ => kit::Button::new("Add instance").size("sm").on_press(save).build(t),
    });
    kit::Dialog::new("add-provider", "Add provider")
        .open(true)
        .width(768.0)
        .description(format!("Add an account or configure a provider on {environment}."))
        .on_close(|s: &mut App| {
            super::accent::close(s);
            s.ui.settings.provider_picker = false;
        })
        .child(steps(s, t))
        .child(panel)
        .child(footer)
        .build(t, layers);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_ids_derive_from_the_label_and_skip_taken_ones() {
        let existing: Vec<String> = ["codex", "claudeAgent", "codex_work", "codex_work_2"].map(String::from).to_vec();
        assert_eq!(available_id("codex", "Codex", &existing), "codex_2");
        assert_eq!(available_id("grok", "", &existing), "grok");
        assert_eq!(available_id("codex", "  Work!! ", &existing), "codex_work_3");
        assert_eq!(available_id("opencode", "My Team / Prod", &existing), "opencode_my_team_prod");
        assert_eq!(derive_id("codex", "***"), "");
        assert_eq!(validate_id("", &existing).as_deref(), Some("Instance ID is required."));
        assert!(validate_id("1abc", &existing).unwrap().starts_with("Instance ID must start with a letter"));
        assert_eq!(validate_id("codex_work", &existing).as_deref(), Some("An instance named 'codex_work' already exists."));
        assert_eq!(validate_id("codex-work_9", &existing), None);
        assert!(validate_id(&"a".repeat(65), &existing).is_some());
    }
}
