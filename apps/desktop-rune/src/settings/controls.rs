use super::{update, value};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, col, el, row, text};
use serde_json::{Value, json};

pub fn button(label: &str, t: &T) -> El {
    crate::kit::Button::new(label)
        .variant("outline")
        .size("compact")
        .build(t)
}

pub fn switch(s: &App, t: &T, key: &str, label: &str) -> El {
    let current = value(s, key);
    let on = if key == "sidebarProjectGroupingMode" {
        current.as_str() != Some("separate")
    } else {
        current.as_bool().unwrap_or(false)
    };
    let owned = key.to_owned();
    crate::kit::Switch::new(on)
        .label(label)
        .hide_label()
        .on_toggle(move |s: &mut App| {
            let key = owned.as_str();
            let v = if key == "sidebarProjectGroupingMode" {
                json!(if on { "separate" } else { "repository" })
            } else {
                json!(!on)
            };
            update(s, key, v);
            if key == "boringSidebarEnabled" && !on {
                update(s, "legacySidebarEnabled", json!(false));
            }
        })
        .build(&crate::kit::theme(t))
        .key(format!("setting-{key}"))
}

pub fn select_width(key: &str) -> f32 {
    match key {
        "notificationMode" => 224.0,
        "sidebarProjectSortOrder" => 176.0,
        "sendShortcut" => 70.0,
        "followUpBehavior" => 71.0,
        _ => 160.0,
    }
}

fn current_label(s: &App, key: &str, options: &[(Value, String)]) -> String {
    let current = match value(s, key) {
        // An unset workspace resolves to the current checkout (`resolveProjectSettings`).
        Value::Null if key == "defaultThreadEnvMode" => json!("local"),
        current => current,
    };
    options
        .iter()
        .find(|(v, _)| *v == current)
        .map(|(_, l)| l.clone())
        .unwrap_or_else(|| current.as_str().unwrap_or("Default").to_owned())
}

fn labels(options: &[(Value, String)]) -> Vec<String> {
    options.iter().map(|(_, l)| l.clone()).collect()
}

/// The setting's `kit::Select` trigger; its list mounts in the settings overlay (`select_menu`).
pub fn select(
    s: &App,
    t: &T,
    key: &str,
    title: &str,
    options: &[(Value, String)],
    width: f32,
) -> El {
    let open = s.ui.settings.open_select.as_deref() == Some(key);
    let owned = key.to_owned();
    let choices = options.to_vec();
    // The kit wraps its trigger in a `self_start` row; this row lets the setting row centre it.
    row().child(crate::kit::Select::new(format!("setting-{key}"), &labels(options))
        .current(Some(&current_label(s, key, options)))
        .label(title)
        .width(width)
        .size("sm")
        .on_toggle(move |s: &mut App| {
            s.ui.settings.open_select = if open { None } else { Some(owned.clone()) };
            s.ui.settings.select_options = choices.clone();
        })
        .trigger(&crate::kit::theme(t)))
}

/// The open select's list, anchored to the trigger `select` mounted.
pub fn select_menu(s: &App, t: &T, key: &str, options: &[(Value, String)], width: f32, layer: &mut Vec<El>) {
    let (owned, choices) = (key.to_owned(), options.to_vec());
    let _trigger = crate::kit::Select::new(format!("setting-{key}"), &labels(options))
        .current(Some(&current_label(s, key, options)))
        .width(width)
        .size("sm")
        .open(true)
        .on_choose(move |s: &mut App, label: &str, _: &mut rune::Tx| {
            if let Some((v, _)) = choices.iter().find(|(_, l)| l == label) {
                update(s, &owned, v.clone());
            }
            s.ui.settings.open_select = None;
        })
        .on_close(|s: &mut App| s.ui.settings.open_select = None)
        .build(&crate::kit::theme(t), layer);
}

pub fn setting_row(s: &App, cx: &rune::Cx<App>, t: &T, spec: &Value) -> El {
    let key = spec["key"].as_str().unwrap_or("");
    let title = spec["title"].as_str().unwrap_or(key);
    let description = spec["description"].as_str().unwrap_or("");
    let mut heading = row().gap(6.0).center_y().child(
        text(title.to_owned())
            .font(theme::SM_MEDIUM)
            .color(t.c.foreground),
    );
    if super::scope::customized(s, key) {
        let owned = key.to_owned();
        heading = heading.child(
            crate::kit::Button::new(format!("Reset {title}"))
                .variant("ghost-muted")
                .size("icon-micro")
                .icon(&icons::UNDO_2)
                .on_press(move |s: &mut App| super::scope::reset(s, &owned))
                .build(t),
        );
    }
    let mut copy = col()
        .key(format!("copy-{key}"))
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(heading);
    if !description.is_empty() {
        copy = copy.child(
            text(super::balance::description(
                description,
                cx.layout(&format!("copy-{key}"))
                    .map(|r| r[2])
                    .unwrap_or(558.0)
                    .min(672.0),
            ))
            .font(theme::TEXT_XS.lh(18.0))
            .color(t.c.muted_foreground)
            .wrap(),
        );
    }
    let ctrl = match spec["kind"].as_str() {
        Some("switch") => row()
            .w(160.0)
            .child(el().grow(1.0))
            .child(switch(s, t, key, title)),
        Some("select") => {
            let options: Vec<_> = spec["options"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|o| (o[0].clone(), o[1].as_str().unwrap_or("").to_owned()))
                .collect();
            select(s, t, key, title, &options, select_width(key))
        }
        Some("slider") => slider(s, t, key, title),
        _ => {
            let current = value(s, key);
            let edit = s.ui.settings.edits.get(key).cloned().unwrap_or_else(|| {
                rune::TextEdit::new(current.as_str().map(str::to_owned).unwrap_or_else(|| {
                    if current.is_null() {
                        String::new()
                    } else {
                        current.to_string()
                    }
                }))
            });
            let (k1, k2) = (key.to_owned(), key.to_owned());
            let (height, pad) = if spec["size"] == "default" { (32.0, 11.0) } else { (28.0, 9.0) };
            rune::text_input(&edit, theme::TEXT_SM)
                .key(format!("setting-{key}"))
                .placeholder(spec["placeholder"].as_str().unwrap_or(""), t.c.muted_foreground.alpha_mul(0.72))
                .w(spec["width"].as_f64().map_or(208.0, |w| w as f32))
                .h(height)
                .px(pad)
                .center_y()
                .radius(8.0)
                .border(1.0, t.c.input)
                .fill(t.c.input.alpha_mul(if t.dark { 0.32 } else { 0.0 }))
                .color(t.c.foreground)
                .single_line()
                .on_edit(move |s: &mut App| {
                    s.ui.settings
                        .edits
                        .entry(k1.clone())
                        .or_insert_with(|| edit.clone())
                })
                .on_submit(move |s: &mut App, _| {
                    if let Some(edit) = s.ui.settings.edits.get(&k2) {
                        let raw = edit.text.clone();
                        let v = if value(s, &k2).is_number() {
                            serde_json::from_str(&raw).ok().filter(Value::is_number)
                        } else {
                            Some(json!(raw))
                        };
                        if let Some(v) = v {
                            update(s, &k2, v);
                        }
                    }
                })
        }
    };
    let environment_wide = s.ui.settings.project.is_some()
        && !s.client_settings.values.contains_key(key)
        && !super::scope::project_key(key);
    let ctrl = if environment_wide {
        ctrl.disabled(true).opacity(0.5)
    } else {
        ctrl
    };
    if s.ui.settings.project.is_some() && super::scope::project_key(key) {
        copy = copy.child(
            text(if super::scope::customized(s, key) {
                "Overridden for this project"
            } else {
                "Inherited from environment"
            })
            .font(theme::TEXT_XS)
            .color(t.c.muted_foreground),
        );
    }
    row()
        .key(format!("row-{key}"))
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(32.0)
        .center_y()
        .child(copy)
        .child(ctrl)
}

pub fn section(title: &str, children: Vec<El>, t: &T) -> El {
    let mut group = col()
        .w_full()
        .padding(1.0)
        .radius(12.0)
        .border(1.0, t.c.border.alpha_mul(0.6))
        .fill(t.c.card.alpha_mul(0.4));
    for (i, child) in children.into_iter().enumerate() {
        if i > 0 {
            group = group.child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.5)));
        }
        group = group.child(child);
    }
    col()
        .w_full()
        .gap(10.0)
        .child(
            el().h(28.0).px(16.0).center_y().child(
                text(title.to_owned())
                    .font(theme::weight(theme::TEXT_BASE, theme::SEMIBOLD))
                    .color(t.c.foreground),
            ),
        )
        .child(group)
}

fn slider(s: &App, t: &T, key: &str, title: &str) -> El {
    let n = value(s, key).as_f64().unwrap_or(100.0) as f32;
    let (min, max, step) = match key {
        "appearanceContrast" => (50.0, 200.0, 5.0),
        "glassOpacity" => (40.0, 100.0, 5.0),
        _ => (0.0, 400.0, 25.0),
    };
    let key = key.to_owned();
    row()
        .w(208.0)
        .center_y()
        .gap(12.0)
        .child(
            el().min_w(48.0).h(24.0).px(8.0).center().radius(6.0).fill(t.c.muted).child(text(format!(
                "{n}{}",
                if key == "panelAnimationDurationMs" {
                    " ms"
                } else {
                    "%"
                }
            ))
            .font(theme::weight(theme::MONO_XS, theme::MEDIUM))
            .color(t.c.foreground)),
        )
        .child(
            crate::kit::Slider::new(n)
                .range(min, max)
                .step(step)
                .width(148.0)
                .label(title)
                .on_change(move |s: &mut App, n, _| update(s, &key, json!(n)))
                .build(t),
        )
}
