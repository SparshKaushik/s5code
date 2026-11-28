use super::{request, value};
use crate::{
    App, env,
    theme::{self, AlphaMul, T},
};
use rune::{El, col, el, row, text};
use serde_json::{Value, json};
use std::sync::OnceLock;

/// Rows that stand for another setting (the auto-settle switch writes the day count, a
/// `parent.field` row writes its parent object).
fn setting_of(key: &str) -> &str {
    if key == "autoSettleInactive" {
        return "sidebarAutoSettleAfterDays";
    }
    super::nested(key).map_or(key, |(parent, _)| parent)
}

/// The writing style's fields reset to their defaults; resetting the mode also clears the
/// custom instructions, like web's reset button.
fn writing_style_default(s: &App, field: &str) -> Value {
    let defaults = &defaults()["sourceControlWritingStyle"];
    let mut object = super::nested_object(s, "sourceControlWritingStyle");
    object[field] = defaults[field].clone();
    if field == "mode" {
        object["customInstructions"] = defaults["customInstructions"].clone();
    }
    object
}
pub fn project_key(key: &str) -> bool {
    let key = setting_of(key);
    static KEYS: OnceLock<Vec<String>> = OnceLock::new();
    KEYS.get_or_init(|| serde_json::from_str(include_str!("project-keys.json")).unwrap())
        .iter()
        .any(|k| k == key)
}
pub fn defaults() -> &'static Value {
    static DEFAULTS: OnceLock<Value> = OnceLock::new();
    DEFAULTS.get_or_init(|| serde_json::from_str(include_str!("server-defaults.json")).unwrap())
}
pub fn override_patch(settings: &Value, project: &str, key: &str, next: Option<Value>) -> Value {
    let mut entry = settings["projectSettingsOverrides"][project]
        .as_object()
        .cloned()
        .unwrap_or_default();
    if let Some(v) = next {
        entry.insert(key.into(), v);
    } else {
        entry.remove(key);
    }
    json!({"projectSettingsOverrides": {project: if entry.is_empty() { Value::Null } else { Value::Object(entry) }}})
}
pub fn reset(s: &mut App, key: &str) {
    s.ui.settings.edits.remove(key);
    let field = key.strip_prefix("sourceControlWritingStyle.");
    let key = setting_of(key);
    if key == "snoozePresets" {
        return super::snooze::reset(s);
    }
    if let Some(field) = field.filter(|_| s.ui.settings.project.is_none()) {
        let next = writing_style_default(s, field);
        // Other environments reset just these fields, as web's reset patch does.
        let fan = json!({field: next[field].clone(), "customInstructions": next["customInstructions"].clone()});
        let fan = if field == "mode" { fan } else { json!({field: next[field].clone()}) };
        return super::write_server(s, key, next, fan);
    }
    if let Some(project) = s.ui.settings.project.clone().filter(|_| project_key(key)) {
        let patch = override_patch(super::server_settings(s), &project, key, None);
        request(s, "server.updateSettings", json!({"patch":patch}));
    } else if let Some(default) = super::client::defaults().get(key) {
        super::update(s, key, default.clone());
    } else if let Some(default) = defaults().get(key) {
        super::update(s, key, default.clone());
    }
}
pub fn customized(s: &App, key: &str) -> bool {
    // Row order is only display order, so it doesn't make the list differ from the defaults.
    if key == "snoozePresets" {
        return super::snooze::customized(s);
    }
    // Profiles are a list the user builds, not a value with a default to go back to.
    if key == "browserProfiles" {
        return false;
    }
    let row = key;
    let key = setting_of(key);
    if let Some(project) = s.ui.settings.project.as_ref().filter(|_| project_key(key)) {
        return super::server_settings(s)["projectSettingsOverrides"][project]
            .get(key)
            .is_some();
    }
    if let Some((parent, field)) = super::nested(row) {
        return parent == "sourceControlWritingStyle" && writing_style_default(s, field) != super::nested_object(s, parent);
    }
    let clients = super::client::defaults();
    clients
        .get(key)
        .or_else(|| defaults().get(key))
        .is_some_and(|d| *d != value(s, key))
}
pub fn device_only(path: &str) -> bool {
    matches!(
        path,
        "general"
            | "appearance"
            | "chat"
            | "diffs"
            | "snap-shot"
            | "connections"
            | "access"
            | "about"
            | "open-source-licenses"
    )
}
/// Pages that edit one environment at a time (web's `singleEnvironment`).
pub fn single(s: &App) -> bool {
    matches!(&s.route, crate::Route::Settings(p) if matches!(p.as_str(), "providers" | "secrets"))
}

fn project_row<'a>(s: &'a App, id: &str) -> Option<&'a Value> {
    s.state().projects.iter().find(|p| p["id"] == id)
}

/// The environment Settings talks to: the scoped one, else the scoped project's, else this machine.
pub fn environment(s: &App) -> String {
    let keys = env::keys(s);
    if let Some(machine) = s.ui.settings.machine.as_ref().filter(|m| keys.contains(*m)) {
        return machine.clone();
    }
    if let Some(project) = s.ui.settings.project.as_deref().filter(|p| project_row(s, p).is_some()) {
        return env::owner(s, project);
    }
    env::LOCAL.to_owned()
}

/// Focuses [`environment`] after the scope changes, and reloads the page from it.
pub fn retarget(s: &mut App) {
    let key = environment(s);
    if s.ui.env.active == key {
        return;
    }
    env::switch(s, &key);
    if s.ui.env.active == key {
        s.ui.settings.optimistic_settings = None;
        s.ui.settings.loaded_page = None;
    }
}

/// Web's `selectScope`: commits pending edits to the old scope, then moves to the new one.
pub fn select(s: &mut App, project: Option<String>, machine: Option<String>) {
    super::commit_edits(s, true);
    let project = match (&project, &machine, &s.route) {
        (Some(id), Some(environment), crate::Route::Settings(page)) if page == "projects" => {
            project_row(s, id).and_then(|selected| {
                let grouping = crate::shell::sidebar::folders::Grouping::read(s);
                let logical = grouping.key(selected);
                s.state().projects.iter()
                    .find(|p| env::of(s, p) == *environment && grouping.key(p) == logical)
                    .map(|p| crate::data::s(p, "id").to_owned())
            }).or(project)
        }
        _ => project,
    };
    s.ui.settings.project = project;
    s.ui.settings.machine = machine;
    s.ui.settings.scope_open = false;
    s.ui.settings.environment_scope_open = false;
    s.ui.settings.all_instructions = None;
    retarget(s);
}

/// Web's `resolveSettingsScope` "unavailable" messages. A project here is one checkout, so one on
/// another environment than the scoped one reads as web's missing checkout.
pub fn unavailable(s: &App) -> Option<&'static str> {
    if s.ui.settings.machine.as_ref().is_some_and(|m| !env::keys(s).contains(m)) {
        return Some("This environment is no longer available.");
    }
    let project = s.ui.settings.project.as_deref()?;
    if project_row(s, project).is_none() {
        return Some("This project is no longer available.");
    }
    s.ui.settings
        .machine
        .as_ref()
        .is_some_and(|m| env::owner(s, project) != *m)
        .then_some("This checkout is no longer available in the selected project and environment.")
}

/// Web's `planScopedSettingsPatch` for "All environments": each other connected environment
/// gets the patch `plan` builds from its own settings.
pub fn fan_out(s: &mut App, plan: impl Fn(&Value) -> Value) {
    if s.ui.settings.machine.is_some() || s.ui.settings.project.is_some() || single(s) || !matches!(s.route, crate::Route::Settings(_)) {
        return;
    }
    let active = s.ui.env.active.clone();
    let patches: Vec<(String, Value)> = env::keys(s)
        .into_iter()
        .filter(|key| *key != active && env::connected(s, key))
        .filter_map(|key| {
            let settings = env::source(s, &key)?.state().config.as_ref()?.get("settings")?.clone();
            Some((key, plan(&settings)))
        })
        .collect();
    for (key, patch) in patches {
        let Some(net) = env::net_of(s, &key) else { continue };
        let id = net.request("server.updateSettings", json!({"patch": patch}));
        s.ui.settings.pending.insert(id, "server.updateSettings".into());
    }
}

pub fn sentence(s: &App, t: &T) -> El {
    let single = single(s);
    let project = match s.ui.settings.project.as_deref() {
        Some(id) => project_row(s, id).map_or("Unavailable project", |p| crate::data::s(p, "title")),
        None => "All projects",
    };
    let machine = s.ui.settings.machine.clone().or_else(|| single.then(|| s.ui.env.active.clone()));
    let environment = match &machine {
        Some(key) if env::keys(s).contains(key) => Some(env::label(s, key)),
        Some(_) => None,
        None => None,
    };
    let environment_label = match (&machine, &environment) {
        (_, Some(label)) => label.as_str(),
        (Some(_), None) => "Unavailable environment",
        (None, _) => "All environments",
    };
    row()
        .w_full()
        .px(16.0)
        .gap(6.0)
        .center_y()
        .child(
            text("Applying settings for")
                .font(theme::TEXT_SM)
                .color(t.c.muted_foreground),
        )
        .child(trigger(
            t,
            "scope-project",
            "Project scope",
            None,
            project,
            |s: &mut App| open_menu(s, false),
        ))
        .child(
            text(if machine.is_some() { "on" } else { "across" })
                .font(theme::TEXT_SM)
                .color(t.c.muted_foreground),
        )
        .child(trigger(
            t,
            "scope-environment",
            "Environment scope",
            environment.is_some().then(|| super::env_machine_icon(s, machine.as_deref().unwrap_or_default())),
            environment_label,
            |s: &mut App| open_menu(s, true),
        ))
}

fn open_menu(s: &mut App, environments: bool) {
    let open = if environments { &mut s.ui.settings.environment_scope_open } else { &mut s.ui.settings.scope_open };
    *open = !*open;
    let open = *open;
    if environments {
        s.ui.settings.scope_open = false;
    } else {
        s.ui.settings.environment_scope_open = false;
    }
    s.ui.settings.scope_search = rune::TextEdit::new("");
    s.ui.settings.scope_search.focused = open;
}

/// The scope picker's select-style trigger: optional icon, label, then the chevron.
fn trigger(
    t: &T,
    key: &str,
    name: &str,
    glyph: Option<&'static rune::Icon>,
    label: &str,
    press: impl Fn(&mut App) + 'static,
) -> El {
    row()
        .key(key.to_owned())
        .h(28.0)
        .px(9.0)
        .gap(4.0)
        .center_y()
        .shrink()
        .max_w(288.0)
        .radius(8.0)
        .border(1.0, t.c.input)
        .fill(if t.dark { t.c.input.alpha_mul(0.32) } else { t.c.background })
        .hover(t.c.accent)
        .focusable(true)
        .focus_ring(2.0, 1.0, t.c.ring)
        .role("button")
        .label(format!("{name}: {label}"))
        .children(glyph.map(|g| rune::icon(g, 14.0).color(t.c.foreground)))
        .child(text(label.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate())
        .child(rune::icon(&crate::icons::CHEVRON_DOWN, 14.0).color(t.c.muted_foreground))
        .on_press(press)
}

/// One scope choice: its value (None is "all"), label, icon, and whether it's offline.
struct Choice {
    value: Option<String>,
    label: String,
    icon: Option<&'static rune::Icon>,
    offline: bool,
}

/// Choices a scope menu's search keeps (web's `ScopeMenu` filter: label or value).
fn matching<'a>(choices: &'a [Choice], query: &str) -> Vec<&'a Choice> {
    let needle = query.trim().to_lowercase();
    choices
        .iter()
        .filter(|c| format!("{} {}", c.label, c.value.as_deref().unwrap_or("all")).to_lowercase().contains(&needle))
        .collect()
}

/// The open scope picker, anchored under its trigger like web's select popups. Past six
/// choices it gains web's combobox search.
pub fn menu(s: &App, t: &T, layers: &mut Vec<El>) {
    use crate::kit::{Menu, MenuItem};
    let environments = s.ui.settings.environment_scope_open;
    if !environments && !s.ui.settings.scope_open {
        return;
    }
    let k = crate::kit::theme(t);
    let single = single(s);
    let (key, anchor, search_label, all, options, current) = if environments {
        let options: Vec<Choice> = env::keys(s)
            .into_iter()
            .map(|key| Choice {
                label: env::label(s, &key),
                icon: Some(super::env_machine_icon(s, &key)),
                offline: !env::connected(s, &key),
                value: Some(key),
            })
            .collect();
        let all = (!single).then(|| Choice { value: None, label: "All environments".into(), icon: Some(&crate::icons::LAYERS), offline: false });
        let current = s.ui.settings.machine.clone().or_else(|| single.then(|| s.ui.env.active.clone()));
        ("environment-scope", "scope-environment", "Search environments", all, options, current)
    } else {
        // Web's `projectScopeMenuGroups`: a scoped environment lists only its own projects.
        let options: Vec<Choice> = s
            .state()
            .projects
            .iter()
            .filter(|p| s.ui.settings.machine.as_ref().is_none_or(|m| env::of(s, p) == *m))
            .map(|p| Choice {
                value: p["id"].as_str().map(str::to_owned),
                label: crate::data::s(p, "title").to_owned(),
                icon: None,
                offline: false,
            })
            .collect();
        let all = Some(Choice { value: None, label: "All projects".into(), icon: None, offline: false });
        ("project-scope", "scope-project", "Search projects", all, options, s.ui.settings.project.clone())
    };
    let searching = options.len() > 6;
    let mut menu = Menu::new(key)
        .role("listbox")
        .anchor(anchor)
        .placement(crate::kit::Placement::BottomStart)
        .gutter(4.0)
        .width(if searching { 320.0 } else { 288.0 })
        .open(true)
        .on_close(|s: &mut App| {
            s.ui.settings.scope_open = false;
            s.ui.settings.environment_scope_open = false;
        });
    if searching {
        menu = menu.row(
            rune::text_input(&s.ui.settings.scope_search, theme::TEXT_SM)
                .key(format!("{key}-search"))
                .label(search_label)
                .placeholder(search_label, t.c.muted_foreground)
                .w_full()
                .h(32.0)
                .px(8.0)
                .color(t.c.foreground)
                .on_edit(|s: &mut App| &mut s.ui.settings.scope_search),
        );
    }
    let choices: Vec<Choice> = all.into_iter().chain(options).collect();
    let shown = if searching { matching(&choices, &s.ui.settings.scope_search.text) } else { choices.iter().collect() };
    if shown.is_empty() {
        menu = menu.row(text("No matches found.").font(theme::TEXT_SM).color(t.c.muted_foreground).w_full().px(8.0).py(6.0));
    }
    for (i, choice) in shown.into_iter().enumerate() {
        let mut item = MenuItem::new(choice.label.clone()).option(choice.value == current);
        if let Some(glyph) = choice.icon {
            item = item.icon(glyph);
        }
        if choice.offline {
            item = item.trailing(text("Offline").font(theme::TEXT_XS).color(t.c.muted_foreground));
        }
        let value = choice.value.clone();
        item = item.on_select(move |s: &mut App| {
            let (project, machine) = if environments {
                (s.ui.settings.project.clone(), value.clone())
            } else {
                (value.clone(), s.ui.settings.machine.clone())
            };
            select(s, project, machine);
        });
        menu = menu.item(item);
        if i == 0 && choice.value.is_none() && !searching {
            menu = menu.separator();
        }
    }
    menu.overlay(&k, layers);
}

/// Web's `SettingsScopeNotice`: why the page needs another scope, and the scopes to pick from.
pub enum Target {
    /// One environment, from those listed (all of them when None).
    Environment(Option<Vec<String>>),
    /// Every environment with no project.
    All,
}

pub fn notice(s: &App, t: &T, message: &str, target: Target) -> El {
    let choices: Vec<(String, &'static rune::Icon, Option<String>)> = match target {
        Target::Environment(eligible) => env::keys(s)
            .into_iter()
            .filter(|key| eligible.as_ref().is_none_or(|keys| keys.contains(key)))
            .map(|key| (env::label(s, &key), super::env_machine_icon(s, &key), Some(key)))
            .collect(),
        Target::All => vec![("Open all environments".to_owned(), &crate::icons::LAYERS, None)],
    };
    let rows = choices
        .into_iter()
        .map(|(label, glyph, machine)| {
            let all = machine.is_none();
            row()
                .w_full()
                .px(16.0)
                .py(12.0)
                .gap(12.0)
                .center_y()
                .hover(t.c.accent.alpha_mul(0.5))
                .focusable(true)
                .focus_ring(2.0, -2.0, t.c.ring)
                .role("button")
                .label(label.clone())
                .child(rune::icon(glyph, 16.0).color(t.c.muted_foreground))
                .child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground).truncate().grow(1.0).min_w(0.0))
                .child(rune::icon(&crate::icons::CHEVRON_RIGHT, 16.0).color(t.c.muted_foreground))
                .on_press(move |s: &mut App| {
                    let project = if all { None } else { s.ui.settings.project.clone() };
                    select(s, project, machine.clone());
                })
        })
        .collect();
    col()
        .w_full()
        .gap(12.0)
        .child(text(message.to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap().px(16.0).role("status"))
        .child(super::controls::card(rows, t))
}

/// Web's writing style when the selected environments differ: one set of instructions for all.
pub fn all_instructions(s: &App, t: &T) -> El {
    let body = col().w_full().max_w(672.0).pt(12.0).pb(14.0).gap(8.0);
    let Some(edit) = &s.ui.settings.all_instructions else {
        return body.child(row().child(
            crate::kit::Button::new("Write custom instructions for all")
                .variant("outline")
                .size("sm")
                .build(t)
                .on_press(|s: &mut App| s.ui.settings.all_instructions = Some(rune::TextEdit::new(""))),
        ));
    };
    let field = rune::text_input(edit, theme::TEXT_SM)
        .key("setting-all-instructions")
        .label("Custom source control instructions for all selected environments")
        .placeholder("Write the instructions each selected environment should use.", t.c.muted_foreground.alpha_mul(0.72))
        .w_full()
        .min_h(88.0)
        .max_h(256.0)
        .px(11.0)
        .py(5.0)
        .radius(8.0)
        .border(1.0, t.c.input)
        .fill(t.c.input.alpha_mul(if t.dark { 0.32 } else { 0.0 }))
        .color(t.c.foreground)
        .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
        .focus_ring(3.0, -3.0, t.c.ring.alpha_mul(0.24))
        .on_edit(|s: &mut App| s.ui.settings.all_instructions.get_or_insert_with(|| rune::TextEdit::new("")));
    body.child(field).child(row().child(
        crate::kit::Button::new("Apply instructions to all")
            .variant("outline")
            .size("sm")
            .disabled(edit.text.is_empty())
            .build(t)
            .on_press(apply_all_instructions),
    ))
}

fn apply_all_instructions(s: &mut App) {
    let Some(edit) = s.ui.settings.all_instructions.take() else { return };
    let style = json!({"mode": "custom", "customInstructions": edit.text.trim()});
    let mut object = super::nested_object(s, "sourceControlWritingStyle");
    object["mode"] = style["mode"].clone();
    object["customInstructions"] = style["customInstructions"].clone();
    super::write_server(s, "sourceControlWritingStyle", object, style);
}

/// Whether the environments settings span hold different values for `key`, or for one field
/// of it (`sourceControlWritingStyle.mode`).
pub fn mixed(s: &App, key: &str) -> bool {
    let (setting, field) = super::nested(key).map_or((key, None), |(parent, field)| (parent, Some(field)));
    let values: Vec<Value> = environment_values(s, setting).into_iter().map(|v| field.map_or_else(|| v.clone(), |f| v[f].clone())).collect();
    values.len() > 1 && values.iter().any(|v| *v != values[0])
}

/// Scenes: two more machines beside this one, each a fixture with `patches` over its settings.
pub(super) fn scene_environments(s: &mut App, patches: [Value; 2]) {
    for ((id, label), patch) in [("env-box", "Hetzner box"), ("env-mac", "Studio Mac")].into_iter().zip(patches) {
        s.ui.env.saved.push(env::Saved { id: id.into(), label: label.into(), base: format!("https://{id}.tail1234.ts.net"), bearer: String::new() });
        let mut state = crate::data::fixture_state();
        if let Some(config) = state.config.as_mut() {
            config["environment"]["label"] = json!(label);
            if !config["settings"].is_object() {
                config["settings"] = json!({});
            }
            for (key, value) in patch.as_object().into_iter().flatten() {
                config["settings"][key.as_str()] = value.clone();
            }
        }
        state.projects.clear();
        state.threads.clear();
        state.archived.clear();
        env::park(s, id, crate::data::Source::Fixture(state));
    }
}

/// Web's `SettingInheritanceState`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Inheritance {
    Default,
    Environment,
    Inherited,
    Overridden,
    Mixed,
}

/// The server setting a row explains, when it has layers to explain (device settings don't).
fn server_key(key: &str) -> Option<&str> {
    let key = setting_of(key);
    (defaults().get(key).is_some() && super::client::defaults().get(key).is_none()).then_some(key)
}

fn environment_label(s: &App) -> String {
    s.state().config.as_ref().and_then(|c| c["environment"]["label"].as_str()).unwrap_or("Environment").to_owned()
}

/// The values the connected environments hold for `key`, while settings apply across them.
/// A project here is one checkout on one environment, so only the unscoped page spans several.
fn environment_values(s: &App, key: &str) -> Vec<Value> {
    if s.ui.settings.machine.is_some() || s.ui.settings.project.is_some() || single(s) {
        return vec![];
    }
    environment_settings(s)
        .into_iter()
        .map(|(_, settings)| {
            let value = settings.get(key).cloned().unwrap_or_else(|| defaults()[key].clone());
            if super::nested(&format!("{key}.")).is_some() { super::complete_object(key, value) } else { value }
        })
        .collect()
}

/// Each connected environment's settings, the one on screen first with its unsaved changes.
fn environment_settings(s: &App) -> Vec<(String, Value)> {
    let active = s.ui.env.active.clone();
    let mut out = vec![];
    for key in env::keys(s) {
        if !env::connected(s, &key) {
            continue;
        }
        let settings = if key == active {
            super::server_settings(s).clone()
        } else {
            match env::source(s, &key).and_then(|src| src.state().config.as_ref()) {
                Some(config) => config["settings"].clone(),
                None => continue,
            }
        };
        if key == active { out.insert(0, (key, settings)) } else { out.push((key, settings)) }
    }
    out
}

/// `inheritance` in web's `SettingsRow`: where the row's value comes from, in one line.
pub fn inheritance(s: &App, key: &str) -> Option<(Inheritance, String)> {
    let key = server_key(key)?;
    let values = environment_values(s, key);
    if values.len() > 1 && values.iter().any(|v| *v != values[0]) {
        return Some((Inheritance::Mixed, "Mixed across selected environments".into()));
    }
    let settings = super::server_settings(s);
    if let Some(project) = s.ui.settings.project.as_ref().filter(|_| project_key(key)) {
        return Some(if settings["projectSettingsOverrides"][project].get(key).is_some() {
            (Inheritance::Overridden, "Overridden for this project".into())
        } else {
            (Inheritance::Inherited, format!("Inherited from {}", environment_label(s)))
        });
    }
    Some(if settings.get(key).is_some_and(|v| *v != defaults()[key]) {
        (Inheritance::Environment, "Set on the environment".into())
    } else {
        (Inheritance::Default, "Built-in default".into())
    })
}

/// Projects that override `key`, as (id, title, value), while no project is selected.
fn overriding(s: &App, key: &str) -> Vec<(String, String, Value)> {
    if s.ui.settings.project.is_some() || !project_key(key) {
        return vec![];
    }
    let overrides = &super::server_settings(s)["projectSettingsOverrides"];
    s.state()
        .projects
        .iter()
        .filter_map(|p| {
            let id = p["id"].as_str()?;
            let value = overrides[id].get(key)?.clone();
            Some((id.to_owned(), crate::data::s(p, "title").to_owned(), value))
        })
        .collect()
}

/// Web's `formatValue` for the inheritance chain.
pub fn format_value(key: &str, value: &Value) -> String {
    match value {
        Value::Null => match key {
            "pullRequestMergeMethod" => "Last selected",
            "sidebarAutoSettleAfterDays" => "Never",
            "defaultModelSelection" => "Automatic",
            "sourceControlWriterModelSelection" => "Text generation model",
            "reviewGuideModelSelection" => "Source control writer model",
            "defaultThreadEnvMode" | "worktreeSubmodules" => "Inherit",
            _ => "Not set",
        }
        .into(),
        Value::Bool(on) => (if *on { "On" } else { "Off" }).into(),
        Value::Number(n) if key == "sidebarAutoSettleAfterDays" => {
            format!("{n} {}", if n.as_u64() == Some(1) { "day" } else { "days" })
        }
        Value::Number(n) => n.to_string(),
        Value::String(text) => match (key, text.as_str()) {
            (_, "") => "Empty".into(),
            ("defaultThreadEnvMode", "local") => "Local".into(),
            ("defaultThreadEnvMode", "worktree") => "New worktree".into(),
            ("pullRequestMergeMethod", "merge") => "Merge commit".into(),
            ("pullRequestMergeMethod", "squash") => "Squash and merge".into(),
            ("pullRequestMergeMethod", "rebase") => "Rebase and merge".into(),
            _ => text.clone(),
        },
        Value::Array(items) => format!("{} {}", items.len(), if items.len() == 1 { "item" } else { "items" }),
        Value::Object(object) => {
            if key == "backgroundActivity" {
                if let Some(profile) = object.get("profile").and_then(Value::as_str) {
                    return match profile {
                        "balanced" => "Balanced",
                        "performance" => "Performance",
                        "battery-saver" => "Battery saver",
                        "custom" => "Advanced",
                        other => other,
                    }
                    .into();
                }
            }
            if let Some(model) = object.get("model").and_then(Value::as_str) {
                return model.into();
            }
            match object.get("mode").and_then(Value::as_str) {
                Some(mode) if key == "worktreeCleanup" => (if mode == "off" { "Off" } else { "Custom" }).into(),
                Some("repo_conventions") if key == "sourceControlWritingStyle" => "Repository conventions".into(),
                Some("conventional_commits") if key == "sourceControlWritingStyle" => "Conventional Commits".into(),
                Some("custom") if key == "sourceControlWritingStyle" => "Custom instructions".into(),
                Some(mode) => mode.into(),
                None => "Custom".into(),
            }
        }
    }
}

/// One row of an inheritance chain: (label, value, effective, set).
type Layer = (String, String, bool, bool);

/// `settingInheritanceLayers` over `settings`: project first, default last.
fn layers(settings: &Value, project: Option<&str>, key: &str) -> Vec<Layer> {
    let environment_value = settings.get(key).cloned().unwrap_or_else(|| defaults()[key].clone());
    let environment_set = environment_value != defaults()[key];
    let mut out = Vec::new();
    let mut project_set = false;
    if let Some(project) = project.filter(|_| project_key(key)) {
        let value = settings["projectSettingsOverrides"][project].get(key);
        project_set = value.is_some();
        out.push(("Project".into(), value.map_or("Inherits".into(), |v| format_value(key, v)), project_set, project_set));
    }
    out.push((
        "Environment".into(),
        if environment_set { format_value(key, &environment_value) } else { "Inherits".into() },
        !project_set && environment_set,
        environment_set,
    ));
    out.push(("Default".into(), format_value(key, &defaults()[key]), !project_set && !environment_set, true));
    out
}

/// Web's popover groups: environments whose chains match share one, the one on screen first.
fn chain_groups(chains: Vec<(String, Vec<Layer>)>) -> Vec<(Vec<String>, Vec<Layer>)> {
    let mut groups: Vec<(Vec<String>, Vec<Layer>)> = vec![];
    for (label, chain) in chains {
        match groups.iter_mut().find(|(_, first)| *first == chain) {
            Some((members, _)) => members.push(label),
            None => groups.push((vec![label], chain)),
        }
    }
    groups
}

/// Whether the marker shows without hover: values set right where they're shown explain
/// nothing new, so web reveals theirs on hover or while their popover is open.
fn marker_shown(state: Inheritance, overrides: usize, hovered: bool, open: bool) -> bool {
    !matches!(state, Inheritance::Default | Inheritance::Environment) || overrides > 0 || hovered || open
}

/// The layers marker beside a row's title.
pub fn marker(s: &App, t: &T, key: &str) -> Option<El> {
    let (state, summary) = inheritance(s, key)?;
    let overrides = server_key(key).map(|k| overriding(s, k).len()).unwrap_or(0);
    let hovered = s.ui.settings.hover_row.as_deref() == Some(key);
    let open = s.ui.settings.inheritance.as_deref() == Some(key);
    let shown = marker_shown(state, overrides, hovered, open);
    let summary = match overrides {
        0 => summary,
        1 => format!("{summary} · 1 project override"),
        n => format!("{summary} · {n} project overrides"),
    };
    let color = match state {
        Inheritance::Overridden => t.c.primary,
        Inheritance::Mixed => t.c.warning_foreground,
        _ => t.c.muted_foreground,
    };
    let owned = key.to_owned();
    Some(
        row().key(format!("inherit-{key}-trigger")).child(
            el().size(20.0, 20.0)
                .center()
                .radius(6.0)
                .hover(t.c.accent)
                .focusable(true)
                .role("button")
                .label(format!("{summary}. Show where this value comes from"))
                .help(summary)
                .opacity(if shown { 1.0 } else { 0.0 })
                .child(rune::icon(&crate::icons::LAYERS, 12.0).color(color))
                .on_press(move |s: &mut App| {
                    let open = s.ui.settings.inheritance.as_deref() == Some(owned.as_str());
                    s.ui.settings.inheritance = (!open).then(|| owned.clone());
                    s.ui.settings.inheritance_open.clear();
                }),
        ),
    )
}

fn chain_rows(t: &T, chain: Vec<Layer>) -> El {
    let mut rows = col().w_full().gap(2.0);
    for (label, value, effective, set) in chain {
        let tone = if effective { t.c.foreground } else if set { t.c.muted_foreground } else { t.c.muted_foreground.alpha_mul(0.6) };
        rows = rows.child(
            row().w_full().px(8.0).py(4.0).gap(12.0).center_y().radius(6.0)
                .fill(if effective { t.c.foreground.alpha_mul(0.06) } else { t.c.foreground.alpha_mul(0.0) })
                .child(text(label).font(if effective { theme::SM_MEDIUM } else { theme::TEXT_SM }).color(if effective { t.c.foreground } else { t.c.muted_foreground }).grow(1.0).truncate())
                .child(text(value).font(theme::TEXT_SM).color(tone).max_w(128.0).truncate())
                .child(if effective { rune::icon(&crate::icons::CHECK, 14.0).color(t.c.primary) } else { el().size(14.0, 14.0) }),
        );
    }
    rows
}

/// The open marker's chain of layers, and the projects that override the value. Across several
/// environments, matching chains share a section and differing ones fold under their value.
pub fn inheritance_popover(s: &App, t: &T, layer: &mut Vec<El>) {
    let Some(row_key) = s.ui.settings.inheritance.clone() else { return };
    let Some(key) = server_key(&row_key).map(str::to_owned) else { return };
    let targets = if environment_values(s, &key).len() > 1 {
        environment_settings(s).into_iter().map(|(env_key, settings)| (env::label(s, &env_key), settings)).collect()
    } else {
        vec![(environment_label(s), super::server_settings(s).clone())]
    };
    let several = targets.len() > 1;
    let project = s.ui.settings.project.as_deref();
    let groups = chain_groups(targets.iter().map(|(label, settings)| (label.clone(), layers(settings, project, &key))).collect());
    let folded = groups.len() > 1;
    let mut body = col().w_full();
    for (i, (members, chain)) in groups.into_iter().enumerate() {
        let mut section = col().w_full().px(12.0).py(10.0).gap(2.0);
        if i > 0 {
            body = body.child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.6)));
        }
        if !several {
            section = section.child(
                row().gap(6.0).center_y().pb(6.0)
                    .child(rune::icon(&crate::icons::SERVER, 14.0).color(t.c.muted_foreground))
                    .child(text(members[0].clone()).font(theme::XS_MEDIUM).color(t.c.muted_foreground).truncate()),
            );
        }
        if folded {
            let open = s.ui.settings.inheritance_open.contains(&i);
            let (winner, value) = chain.iter().find(|layer| layer.2).map(|(label, value, ..)| (label.clone(), value.clone())).unwrap_or_default();
            let mut machines = col().w_full().gap(2.0);
            for member in &members {
                machines = machines.child(
                    row().gap(4.0).center_y()
                        .child(rune::icon(&crate::icons::SERVER, 12.0).color(t.c.muted_foreground))
                        .child(text(member.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate()),
                );
            }
            section = section.child(
                col().w_full().px(8.0).py(6.0).gap(4.0).radius(6.0).hover(t.c.accent)
                    .focusable(true).role("button").expanded(open).label(format!("{value} · {winner}"))
                    .child(
                        row().w_full().gap(8.0).center_y()
                            .child(text(value).font(theme::TEXT_SM).color(t.c.foreground).grow(1.0).truncate())
                            .child(text(winner).font(theme::TEXT_XS).color(t.c.muted_foreground))
                            .child(rune::icon(if open { &crate::icons::CHEVRON_DOWN } else { &crate::icons::CHEVRON_RIGHT }, 12.0).color(t.c.muted_foreground)),
                    )
                    .child(machines)
                    .on_press(move |s: &mut App| {
                        if !s.ui.settings.inheritance_open.remove(&i) {
                            s.ui.settings.inheritance_open.insert(i);
                        }
                    }),
            );
            if open {
                section = section.child(el().h(6.0)).child(chain_rows(t, chain));
            }
        } else {
            section = section.child(chain_rows(t, chain));
            if several {
                section = section.child(text("Same on all machines").font(theme::TEXT_XS).color(t.c.muted_foreground).px(8.0).pt(6.0));
            }
        }
        body = body.child(section);
    }
    let projects = overriding(s, &key);
    if !projects.is_empty() {
        let mut overrides = col().w_full().px(12.0).pb(10.0).gap(2.0);
        let mut header = row().w_full().px(8.0).pt(8.0).gap(12.0).center_y()
            .child(text("Overridden by").font(theme::TEXT_XS).color(t.c.muted_foreground).grow(1.0));
        if super::permissions::can_write_settings(s) {
            let ids: Vec<String> = projects.iter().map(|(id, ..)| id.clone()).collect();
            let k = key.clone();
            header = header.child(
                crate::kit::Button::new(if ids.len() == 1 { "Reset it" } else { "Reset all" })
                    .variant("link")
                    .size("xs")
                    .build(t)
                    .on_press(move |s: &mut App| clear_overrides(s, &ids, &k)),
            );
        }
        overrides = overrides.child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.6))).child(header);
        for (id, title, value) in projects {
            overrides = overrides.child(
                row().w_full().px(8.0).py(4.0).gap(12.0).center_y()
                    .child(
                        text(title.clone()).font(theme::TEXT_SM).color(t.c.foreground).grow(1.0).truncate()
                            .role("button").label(format!("Open {title}")).focusable(true)
                            .on_press(move |s: &mut App| {
                                s.ui.settings.inheritance = None;
                                let machine = s.ui.settings.machine.clone();
                                select(s, Some(id.clone()), machine);
                            }),
                    )
                    .child(text(format_value(&key, &value)).font(theme::TEXT_SM).color(t.c.muted_foreground).max_w(128.0).truncate()),
            );
        }
        body = body.child(overrides);
    }
    let _ = crate::kit::Popover::new(format!("inherit-{row_key}"))
        .open(true)
        .padding(0.0)
        .gutter(4.0)
        .width("md")
        .placement(crate::kit::Placement::BottomStart)
        .label("Where this value comes from")
        .child(body)
        .on_close(|s: &mut App| {
            s.ui.settings.inheritance = None;
            s.ui.settings.inheritance_open.clear();
        })
        .build(&crate::kit::theme(t), layer);
}

/// Web's `clearProjectOverrides`: one patch removing `key` from each project's overrides.
fn clear_overrides(s: &mut App, projects: &[String], key: &str) {
    let settings = super::server_settings(s).clone();
    let mut patch = json!({"projectSettingsOverrides": {}});
    for project in projects {
        let one = override_patch(&settings, project, key, None);
        patch["projectSettingsOverrides"][project] = one["projectSettingsOverrides"][project].clone();
    }
    s.ui.settings.inheritance = None;
    request(s, "server.updateSettings", json!({"patch": patch}));
}

#[cfg(test)]
mod tests {
    use super::*;
    fn connected(label: &str, settings: Value) -> (s5_net::Net, s5_net::Sent) {
        let (mut net, sent) = s5_net::Net::in_memory_sent(&format!("http://{label}.test"));
        let state = net.state_mut();
        state.conn = s5_net::Conn::Open;
        state.shell_ready = true;
        state.config = Some(json!({"environment": {"label": label}, "settings": settings}));
        (net, sent)
    }

    fn patches(sent: &s5_net::Sent) -> Vec<Value> {
        sent.requests().into_iter().filter(|r| r.1 == "server.updateSettings").map(|r| r.2["patch"].clone()).collect()
    }

    #[test]
    fn all_environments_sends_each_setting_to_every_connected_environment() {
        let mut s = App::new();
        let style = json!({"mode": "repo_conventions", "customInstructions": "", "followChangeRequestTemplates": false});
        let (local, local_sent) = connected("Mac", json!({"sourceControlWritingStyle": style, "worktreeCleanup": {"mode": "off"}}));
        s.src = crate::data::Source::Live(local);
        let (remote, box_sent) = connected("Box", json!({"sourceControlWritingStyle": {"mode": "custom", "customInstructions": "Theirs", "followChangeRequestTemplates": true}}));
        s.ui.env.saved.push(env::Saved { id: "box".into(), label: "Box".into(), base: "http://box.test".into(), bearer: String::new() });
        env::park(&mut s, "box", crate::data::Source::Live(remote));
        s.route = crate::Route::Settings("source-control".into());

        super::super::update(&mut s, "sourceControlWritingStyle.customInstructions", json!("Be brief"));
        assert_eq!(patches(&local_sent)[0]["sourceControlWritingStyle"]["mode"], "repo_conventions", "the active environment gets its whole object");
        assert_eq!(
            patches(&box_sent),
            [json!({"sourceControlWritingStyle": {"customInstructions": "Be brief"}})],
            "FAILED: another environment gets the field alone, keeping its own mode"
        );

        assert!(mixed(&s, "sourceControlWritingStyle.mode"));
        s.ui.settings.all_instructions = Some(rune::TextEdit::new("  Use the imperative  "));
        apply_all_instructions(&mut s);
        let applied = json!({"sourceControlWritingStyle": {"mode": "custom", "customInstructions": "Use the imperative"}});
        assert_eq!(patches(&local_sent)[0]["sourceControlWritingStyle"]["customInstructions"], "Use the imperative");
        assert_eq!(patches(&box_sent), [applied], "FAILED: apply to all sets mode and instructions everywhere");

        // Web's `forDestination`: each environment's worktree rules come from its own policy.
        super::super::storage::set(&mut s, "worktreeOnMerge", json!(true));
        assert_eq!(patches(&local_sent)[0]["worktreeCleanup"]["rules"]["worktreeOnMerge"], true);
        let theirs = patches(&box_sent);
        assert_eq!(theirs[0]["storageCleanup"]["worktreeOnMerge"], true);
        assert!(theirs[0].get("worktreeCleanup").is_none(), "FAILED: Box has no policy, so none is written there");

        s.ui.settings.machine = Some(env::LOCAL.into());
        super::super::update(&mut s, "reviewGuidesEnabled", json!(true));
        assert_eq!(patches(&local_sent).len(), 1);
        assert!(patches(&box_sent).is_empty(), "FAILED: a scoped environment keeps the write to itself");
    }

    #[test]
    fn a_scope_that_no_longer_resolves_reads_as_unavailable() {
        let mut s = App::new();
        let project = crate::data::s(&s.state().projects[0], "id").to_owned();
        s.ui.env.saved.push(env::Saved { id: "box".into(), label: "Box".into(), base: "http://box.test".into(), bearer: String::new() });
        assert_eq!(unavailable(&s), None);
        s.ui.settings.machine = Some("gone".into());
        assert_eq!(unavailable(&s), Some("This environment is no longer available."));
        s.ui.settings.machine = None;
        s.ui.settings.project = Some("missing".into());
        assert_eq!(unavailable(&s), Some("This project is no longer available."));
        s.ui.settings.project = Some(project);
        s.ui.settings.machine = Some("box".into());
        assert_eq!(unavailable(&s), Some("This checkout is no longer available in the selected project and environment."));
        s.ui.settings.machine = Some(env::LOCAL.into());
        assert_eq!(unavailable(&s), None);
        assert_eq!(environment(&s), env::LOCAL);
    }

    #[test]
    fn environments_resolving_alike_share_one_chain() {
        let chain = |value: &str| vec![("Environment".to_owned(), value.to_owned(), true, true)];
        let groups = chain_groups(vec![("Mac".into(), chain("On")), ("Box".into(), chain("Off")), ("Pi".into(), chain("On"))]);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, ["Mac", "Pi"]);
        assert_eq!(groups[1].0, ["Box"]);
        assert_eq!(chain_groups(vec![("Mac".into(), chain("On")), ("Box".into(), chain("On"))]).len(), 1, "one group reads as the same on all machines");
    }

    #[test]
    fn plain_values_reveal_their_marker_on_hover_only() {
        assert!(!marker_shown(Inheritance::Default, 0, false, false));
        assert!(!marker_shown(Inheritance::Environment, 0, false, false));
        assert!(marker_shown(Inheritance::Environment, 0, true, false), "FAILED: hover reveals it");
        assert!(marker_shown(Inheritance::Default, 0, false, true), "FAILED: an open popover keeps it");
        assert!(marker_shown(Inheritance::Default, 2, false, false), "FAILED: project overrides always show");
        assert!(marker_shown(Inheritance::Overridden, 0, false, false));
        assert!(marker_shown(Inheritance::Mixed, 0, false, false));
    }

    #[test]
    fn inheritance_values_read_like_web() {
        assert_eq!(format_value("pullRequestMergeMethod", &Value::Null), "Last selected");
        assert_eq!(format_value("sidebarAutoSettleAfterDays", &json!(1)), "1 day");
        assert_eq!(format_value("sidebarAutoSettleAfterDays", &json!(7)), "7 days");
        assert_eq!(format_value("defaultAutoPull", &json!(true)), "On");
        assert_eq!(format_value("x", &json!("")), "Empty");
        assert_eq!(format_value("worktreeCleanup", &json!({"mode": "off"})), "Off");
        assert_eq!(format_value("sourceControlWritingStyle", &json!({"mode": "custom"})), "Custom instructions");
        assert_eq!(format_value("textGenerationModelSelection", &json!({"model": "gpt-5"})), "gpt-5");
        assert_eq!(format_value("snoozePresets", &json!([1, 2])), "2 items");
    }

    #[test]
    fn removing_override_preserves_other_and_unknown_keys() {
        let settings = json!({"projectSettingsOverrides":{"p":{"defaultAutoPull":true,"futureSetting":42,"responseStreamingMode":"paragraph"},"other":{"defaultAutoPull":false}}});
        assert_eq!(
            override_patch(&settings, "p", "defaultAutoPull", None),
            json!({"projectSettingsOverrides":{"p":{"futureSetting":42,"responseStreamingMode":"paragraph"}}})
        );
        assert_eq!(
            override_patch(&settings, "p", "responseStreamingMode", Some(Value::Null))["projectSettingsOverrides"]
                ["p"]["responseStreamingMode"],
            Value::Null
        );
    }
}
