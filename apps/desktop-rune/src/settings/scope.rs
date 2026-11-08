use super::{request, value};
use crate::{
    App,
    theme::{self, AlphaMul, T},
};
use rune::{El, row, text};
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

#[cfg(test)]
mod tests {
    use super::*;
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
