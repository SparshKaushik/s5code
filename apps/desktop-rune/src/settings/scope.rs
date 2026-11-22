use super::{request, value};
use crate::{
    App,
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
        return super::update(s, key, next);
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
pub fn sentence(s: &App, t: &T) -> El {
    let project =
        s.ui.settings
            .project
            .as_ref()
            .and_then(|id| s.state().projects.iter().find(|p| p["id"] == *id));
    let single = s.ui.settings.environment_only
        || matches!(&s.route,crate::Route::Settings(p) if matches!(p.as_str(),"providers"|"secrets"));
    let environment = s
        .state()
        .config
        .as_ref()
        .and_then(|c| c["environment"]["label"].as_str())
        .unwrap_or("Environment");
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
            project.map(|p| crate::data::s(p, "title")).unwrap_or("All projects"),
            |s: &mut App| s.ui.settings.scope_open = !s.ui.settings.scope_open,
        ))
        .child(
            text(if single { "on" } else { "across" })
                .font(theme::TEXT_SM)
                .color(t.c.muted_foreground),
        )
        .child(trigger(
            t,
            "scope-environment",
            "Environment scope",
            single.then_some(&crate::icons::SERVER),
            if single { environment } else { "All environments" },
            |s: &mut App| {
                s.ui.settings.environment_scope_open = !s.ui.settings.environment_scope_open
            },
        ))
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
/// The open scope picker, anchored under its trigger like web's select popups.
pub fn menu(s: &App, t: &T, layers: &mut Vec<El>) {
    use crate::kit::{Menu, MenuItem};
    let k = crate::kit::theme(t);
    if s.ui.settings.environment_scope_open {
        let environment = s
            .state()
            .config
            .as_ref()
            .and_then(|c| c["environment"]["label"].as_str())
            .unwrap_or("Environment")
            .to_owned();
        let single = matches!(&s.route,crate::Route::Settings(p) if matches!(p.as_str(),"providers"|"secrets"));
        let only = s.ui.settings.environment_only || single;
        let mut menu = Menu::new("environment-scope")
            .role("listbox")
            .anchor("scope-environment")
            .placement(crate::kit::Placement::BottomStart)
            .gutter(4.0)
            .open(true)
            .on_close(|s: &mut App| s.ui.settings.environment_scope_open = false);
        if !single {
            menu = menu.item(MenuItem::new("All environments").option(!only).on_select(|s: &mut App| {
                super::commit_edits(s, true);
                s.ui.settings.environment_only = false;
                s.ui.settings.environment_scope_open = false;
            }));
        }
        menu.item(MenuItem::new(environment).option(only).on_select(|s: &mut App| {
            super::commit_edits(s, true);
            s.ui.settings.environment_only = true;
            s.ui.settings.environment_scope_open = false;
        }))
        .overlay(&k, layers);
        return;
    }
    if !s.ui.settings.scope_open {
        return;
    }
    let mut menu = Menu::new("project-scope")
        .role("listbox")
        .anchor("scope-project")
        .placement(crate::kit::Placement::BottomStart)
        .gutter(4.0)
        .open(true)
        .on_close(|s: &mut App| s.ui.settings.scope_open = false);
    for (id, label) in std::iter::once((None, "All projects".to_owned())).chain(
        s.state().projects.iter().map(|p| {
            (
                p["id"].as_str().map(str::to_owned),
                crate::data::s(p, "title").to_owned(),
            )
        }),
    ) {
        let selected = s.ui.settings.project == id;
        menu = menu.item(MenuItem::new(label).option(selected).on_select(move |s: &mut App| {
            super::commit_edits(s, true);
            s.ui.settings.project = id.clone();
            s.ui.settings.scope_open = false;
        }));
    }
    menu.overlay(&k, layers);
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
fn environment_values(s: &App, key: &str) -> Vec<Value> {
    if s.ui.settings.environment_only {
        return vec![];
    }
    crate::env::keys(s)
        .iter()
        .filter(|k| crate::env::connected(s, k))
        .filter_map(|k| crate::env::source(s, k)?.state().config.clone())
        .map(|config| config["settings"].get(key).cloned().unwrap_or_else(|| defaults()[key].clone()))
        .collect()
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

/// `settingInheritanceLayers`: (label, value, effective, set), project first, default last.
fn layers(s: &App, key: &str) -> Vec<(String, String, bool, bool)> {
    let settings = super::server_settings(s);
    let environment_value = settings.get(key).cloned().unwrap_or_else(|| defaults()[key].clone());
    let environment_set = environment_value != defaults()[key];
    let mut out = Vec::new();
    let mut project_set = false;
    if let Some(project) = s.ui.settings.project.as_ref().filter(|_| project_key(key)) {
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

/// The layers marker beside a row's title. Values set right where they're shown explain nothing
/// new, so web shows their marker on hover only; here they have none.
pub fn marker(s: &App, t: &T, key: &str) -> Option<El> {
    let (state, summary) = inheritance(s, key)?;
    let overrides = server_key(key).map(|k| overriding(s, k).len()).unwrap_or(0);
    if matches!(state, Inheritance::Default | Inheritance::Environment) && overrides == 0 {
        return None;
    }
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
                .child(rune::icon(&crate::icons::LAYERS, 12.0).color(color))
                .on_press(move |s: &mut App| {
                    let open = s.ui.settings.inheritance.as_deref() == Some(owned.as_str());
                    s.ui.settings.inheritance = (!open).then(|| owned.clone());
                }),
        ),
    )
}

/// The open marker's chain of layers, and the projects that override the value.
pub fn inheritance_popover(s: &App, t: &T, layer: &mut Vec<El>) {
    let Some(row_key) = s.ui.settings.inheritance.clone() else { return };
    let Some(key) = server_key(&row_key).map(str::to_owned) else { return };
    let mut body = col().w_full().px(12.0).py(10.0).gap(2.0).child(
        row().gap(6.0).center_y().pb(6.0)
            .child(rune::icon(&crate::icons::SERVER, 14.0).color(t.c.muted_foreground))
            .child(text(environment_label(s)).font(theme::XS_MEDIUM).color(t.c.muted_foreground).truncate()),
    );
    for (label, value, effective, set) in layers(s, &key) {
        let tone = if effective { t.c.foreground } else if set { t.c.muted_foreground } else { t.c.muted_foreground.alpha_mul(0.6) };
        body = body.child(
            row().w_full().px(8.0).py(4.0).gap(12.0).center_y().radius(6.0)
                .fill(if effective { t.c.foreground.alpha_mul(0.06) } else { t.c.foreground.alpha_mul(0.0) })
                .child(text(label).font(if effective { theme::SM_MEDIUM } else { theme::TEXT_SM }).color(if effective { t.c.foreground } else { t.c.muted_foreground }).grow(1.0).truncate())
                .child(text(value).font(theme::TEXT_SM).color(tone).max_w(128.0).truncate())
                .child(if effective { rune::icon(&crate::icons::CHECK, 14.0).color(t.c.primary) } else { el().size(14.0, 14.0) }),
        );
    }
    let projects = overriding(s, &key);
    if !projects.is_empty() {
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
        body = body.child(el().h(1.0).w_full().mt(6.0).fill(t.c.border.alpha_mul(0.6))).child(header);
        for (id, title, value) in projects {
            body = body.child(
                row().w_full().px(8.0).py(4.0).gap(12.0).center_y()
                    .child(
                        text(title.clone()).font(theme::TEXT_SM).color(t.c.foreground).grow(1.0).truncate()
                            .role("button").label(format!("Open {title}")).focusable(true)
                            .on_press(move |s: &mut App| {
                                super::commit_edits(s, true);
                                s.ui.settings.project = Some(id.clone());
                                s.ui.settings.inheritance = None;
                            }),
                    )
                    .child(text(format_value(&key, &value)).font(theme::TEXT_SM).color(t.c.muted_foreground).max_w(128.0).truncate()),
            );
        }
    }
    let _ = crate::kit::Popover::new(format!("inherit-{row_key}"))
        .open(true)
        .padding(0.0)
        .gutter(4.0)
        .width("md")
        .placement(crate::kit::Placement::BottomStart)
        .label("Where this value comes from")
        .child(body)
        .on_close(|s: &mut App| s.ui.settings.inheritance = None)
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
