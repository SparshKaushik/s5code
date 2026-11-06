use super::{controls as c, request};
use crate::{
    App, icons,
    theme::{self, T},
};
use rune::{El, col, icon, row, text};
use serde_json::{Value, json};

pub fn field_key(id: &str, field: &str) -> String {
    format!("provider::{id}::{field}")
}
pub fn field_value(s: &App, id: &str, field: &str) -> Value {
    let entry =
        &s.state().config.as_ref().unwrap_or(&Value::Null)["settings"]["providerInstances"][id];
    if field == "displayName" {
        entry[field].clone()
    } else {
        entry["config"][field].clone()
    }
}
pub fn config_patch(settings: &Value, id: &str, driver: &str, field: &str, value: Value) -> Value {
    let mut entry = settings["providerInstances"][id].clone();
    if !entry.is_object() {
        entry = json!({"driver":driver,"config":{}});
    }
    if field == "displayName" || field == "enabled" {
        entry[field] = value;
    } else {
        if !entry["config"].is_object() {
            entry["config"] = json!({});
        }
        entry["config"][field] = value;
    }
    json!({"providerInstances":{id:entry}})
}
pub fn update(s: &mut App, id: &str, field: &str, value: Value) {
    let config = s.state().config.as_ref().unwrap_or(&Value::Null);
    let driver = config["providers"]
        .as_array()
        .and_then(|ps| ps.iter().find(|p| p["instanceId"] == id))
        .and_then(|p| p["driver"].as_str())
        .unwrap_or("");
    let patch = config_patch(&config["settings"], id, driver, field, value);
    request(s, "server.updateSettings", json!({"patch":patch}));
}
fn input(s: &App, t: &T, id: &str, field: &str, label: &str, description: &str) -> El {
    let key = field_key(id, field);
    let k = key.clone();
    let current = field_value(s, id, field);
    let edit =
        s.ui.settings
            .edits
            .get(&key)
            .cloned()
            .unwrap_or_else(|| rune::TextEdit::new(current.as_str().unwrap_or("")));
    col()
        .gap(8.0)
        .w_full()
        .child(
            text(label.to_owned())
                .font(theme::SM_MEDIUM)
                .color(t.c.foreground),
        )
        .child(
            text(description.to_owned())
                .font(theme::TEXT_XS)
                .color(t.c.muted_foreground)
                .wrap(),
        )
        .child(
            crate::kit::Input::new(&edit)
                .placeholder(label)
                .on_edit(move |s: &mut App| {
                    s.ui.settings
                        .edits
                        .entry(k.clone())
                        .or_insert_with(|| edit.clone())
                })
                .build(t)
                .key(format!("setting-{key}"))
                .single_line(),
        )
}
pub fn view(s: &App, t: &T) -> El {
    let providers = s
        .state()
        .config
        .as_ref()
        .and_then(|c| c["providers"].as_array());
    let mut list = col().w(272.0).gap(4.0);
    let selected = providers.and_then(|ps| {
        s.ui.settings
            .provider
            .as_ref()
            .and_then(|id| ps.iter().find(|p| p["instanceId"] == *id))
            .or_else(|| ps.first())
    });
    for p in providers.into_iter().flatten() {
        let id = p["instanceId"].as_str().unwrap_or("").to_owned();
        let name = p["displayName"].as_str().unwrap_or(&id).to_owned();
        let status = p["message"]
            .as_str()
            .unwrap_or_else(|| p["status"].as_str().unwrap_or(""));
        list = list.child(
            row()
                .w_full()
                .padding(12.0)
                .gap(12.0)
                .radius(8.0)
                .hover(t.c.accent)
                .focusable(true)
                .role("button")
                .label(name.clone())
                .fill(if selected.is_some_and(|p| p["instanceId"] == id) {
                    t.c.accent
                } else {
                    rune::Color::TRANSPARENT
                })
                .on_press(move |s: &mut App| {
                    super::commit_edits(s, true);
                    s.ui.settings.provider = Some(id.clone());
                })
                .child(icon(&icons::BOT, 20.0).color(t.c.muted_foreground))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(4.0)
                        .child(
                            row()
                                .gap(8.0)
                                .center_y()
                                .child(text(name).font(theme::SM_MEDIUM).color(t.c.foreground))
                                .child(
                                    text(p["version"].as_str().unwrap_or("").to_owned())
                                        .font(theme::MONO_XS)
                                        .color(t.c.muted_foreground),
                                ),
                        )
                        .child(
                            text(status.to_owned())
                                .font(theme::TEXT_XS.lh(18.0))
                                .color(t.c.muted_foreground)
                                .wrap(),
                        ),
                ),
        );
    }
    let mut details = col().grow(1.0).min_w(0.0).gap(24.0);
    if let Some(p) = selected {
        let id = p["instanceId"].as_str().unwrap_or("");
        let driver = p["driver"].as_str().unwrap_or("");
        let update_id = id.to_owned();
        let update_driver = driver.to_owned();
        details = details.child(
            row()
                .w_full()
                .center_y()
                .child(
                    text(p["displayName"].as_str().unwrap_or(id).to_owned())
                        .font(theme::weight(theme::TEXT_LG, 600.0))
                        .color(t.c.foreground)
                        .grow(1.0),
                )
                .child(c::button("Update", t).on_press(move |s: &mut App| {
                    request(
                        s,
                        "server.updateProvider",
                        json!({"provider":update_driver,"instanceId":update_id}),
                    )
                })),
        );
        details = details.child(c::section(
            "Status",
            vec![
                col()
                    .padding(12.0)
                    .gap(4.0)
                    .child(
                        text(p["status"].as_str().unwrap_or("").to_owned())
                            .font(theme::SM_MEDIUM)
                            .color(t.c.foreground),
                    )
                    .child(
                        text(p["message"].as_str().unwrap_or("").to_owned())
                            .font(theme::TEXT_XS)
                            .color(t.c.muted_foreground)
                            .wrap(),
                    ),
            ],
            t,
        ));
        details = details.child(input(s, t, id, "displayName", "Display name", ""));
        let mut runtime = col().padding(12.0).gap(20.0).child(input(
            s,
            t,
            id,
            if driver == "acpRegistry" {
                "commandPath"
            } else {
                "binaryPath"
            },
            "Binary path",
            "Path to the binary used by this instance.",
        ));
        if driver == "codex" || driver == "claudeAgent" {
            runtime = runtime.child(input(
                s,
                t,
                id,
                "homePath",
                if driver == "codex" {
                    "CODEX_HOME path"
                } else {
                    "Config directory"
                },
                "Custom home and config directory.",
            ));
        }
        if driver == "codex" {
            runtime=runtime.child(input(s,t,id,"shadowHomePath","Shadow home path","Account-specific Codex home. Keeps auth.json separate while sharing state from CODEX_HOME."));
        }
        details = details.child(c::section("Runtime", vec![runtime], t));
    }
    col()
        .w_full()
        .gap(16.0)
        .child(
            row()
                .w_full()
                .px(16.0)
                .center_y()
                .child(
                    text("Providers")
                        .font(theme::weight(theme::TEXT_BASE, 600.0))
                        .color(t.c.foreground)
                        .grow(1.0),
                )
                .child(c::button("Refresh", t).on_press(|s: &mut App| {
                    request(
                        s,
                        "server.refreshProviders",
                        json!({"fresh":true,"reloadSecrets":true}),
                    )
                })),
        )
        .child(row().w_full().gap(16.0).child(list).child(details))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_edit_preserves_driver_environment_and_other_config_fields() {
        let settings = json!({"providerInstances":{"custom":{"driver":"codex","environment":[{"name":"FOO","value":"bar"}],"config":{"homePath":"/custom","future":true}}}});
        let patch = config_patch(
            &settings,
            "custom",
            "codex",
            "binaryPath",
            json!("/bin/codex"),
        );
        assert_eq!(
            patch["providerInstances"]["custom"]["environment"],
            settings["providerInstances"]["custom"]["environment"]
        );
        assert_eq!(
            patch["providerInstances"]["custom"]["config"],
            json!({"homePath":"/custom","future":true,"binaryPath":"/bin/codex"})
        );
    }
}
