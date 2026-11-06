use super::{request, value};
use crate::{
    App,
    theme::{self, T},
};
use rune::{El, col, el, row, text};
use serde_json::{Value, json};
use std::sync::OnceLock;

pub fn project_key(key: &str) -> bool {
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
    if let Some(project) = s.ui.settings.project.clone().filter(|_| project_key(key)) {
        let patch = override_patch(
            &s.state().config.as_ref().unwrap_or(&Value::Null)["settings"],
            &project,
            key,
            None,
        );
        request(s, "server.updateSettings", json!({"patch":patch}));
    } else if let Some(default) = super::client::ClientSettings::default().values.get(key) {
        super::update(s, key, default.clone());
    } else if let Some(default) = defaults().get(key) {
        super::update(s, key, default.clone());
    }
}
pub fn customized(s: &App, key: &str) -> bool {
    if let Some(project) = s.ui.settings.project.as_ref().filter(|_| project_key(key)) {
        return s.state().config.as_ref().is_some_and(|c| {
            c["settings"]["projectSettingsOverrides"][project]
                .get(key)
                .is_some()
        });
    }
    let clients = super::client::ClientSettings::default();
    clients
        .values
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
        .child(
            crate::kit::Button::new(
                project
                    .map(|p| crate::data::s(p, "name"))
                    .unwrap_or("All projects"),
            )
            .size("sm")
            .variant("ghost")
            .icon(&crate::icons::CHEVRON_DOWN)
            .on_press(|s: &mut App| s.ui.settings.scope_open = !s.ui.settings.scope_open)
            .build(t),
        )
        .child(
            text("across")
                .font(theme::TEXT_SM)
                .color(t.c.muted_foreground),
        )
        .child(
            text("All environments")
                .font(theme::SM_MEDIUM)
                .color(t.c.foreground),
        )
}
pub fn menu(s: &App, t: &T) -> Option<El> {
    if !s.ui.settings.scope_open {
        return None;
    }
    let mut menu = col()
        .w(280.0)
        .padding(4.0)
        .gap(2.0)
        .radius(8.0)
        .border(1.0, t.c.border)
        .fill(t.c.popover);
    for (id, label) in std::iter::once((None, "All projects".to_owned())).chain(
        s.state().projects.iter().map(|p| {
            (
                p["id"].as_str().map(str::to_owned),
                crate::data::s(p, "name").to_owned(),
            )
        }),
    ) {
        menu = menu.child(
            super::controls::button(&label, t).on_press(move |s: &mut App| {
                super::commit_edits(s, true);
                s.ui.settings.project = id.clone();
                s.ui.settings.scope_open = false;
            }),
        );
    }
    Some(
        el().absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .center()
            .child(menu.modal(true, |s: &mut App| s.ui.settings.scope_open = false)),
    )
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
