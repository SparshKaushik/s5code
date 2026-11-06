use super::{update, value};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, col, el, icon, row, text};
use serde_json::{Value, json};

pub fn button(label: &str, t: &T) -> El {
    el().h(28.0)
        .px(10.0)
        .center()
        .radius(6.0)
        .border(1.0, t.c.border)
        .fill(t.c.background)
        .hover(t.c.accent)
        .role("button")
        .label(label)
        .focusable(true)
        .focus_ring(2.0, 1.0, t.c.ring)
        .child(
            text(label.to_owned())
                .font(theme::TEXT_XS)
                .color(t.c.foreground),
        )
}

pub fn switch(s: &App, t: &T, key: &str, label: &str) -> El {
    let current = value(s, key);
    let on = if key == "sidebarProjectGroupingMode" {
        current.as_str() != Some("separate")
    } else {
        current.as_bool().unwrap_or(false)
    };
    let key = key.to_owned();
    el().key(format!("setting-{key}"))
        .role("switch")
        .label(label)
        .checked(on)
        .focusable(true)
        .focus_ring(2.0, 2.0, t.c.ring)
        .size(32.0, 18.0)
        .radius(999.0)
        .fill(if on { t.c.primary } else { t.c.input })
        .hover(t.c.accent.alpha_mul(0.1))
        .child(
            el().absolute(if on { 16.0 } else { 2.0 }, 2.0)
                .size(14.0, 14.0)
                .radius(999.0)
                .fill(if on {
                    t.c.primary_foreground
                } else {
                    t.c.muted_foreground
                }),
        )
        .on_press(move |s: &mut App| {
            let v = if key == "sidebarProjectGroupingMode" {
                json!(if on { "separate" } else { "repository" })
            } else {
                json!(!on)
            };
            update(s, &key, v);
            if key == "boringSidebarEnabled" && !on {
                update(s, "legacySidebarEnabled", json!(false));
            }
        })
}

pub fn select(
    s: &App,
    t: &T,
    key: &str,
    title: &str,
    options: &[(Value, String)],
    width: f32,
) -> El {
    let current = value(s, key);
    let display = options
        .iter()
        .find(|(v, _)| *v == current)
        .map(|(_, l)| l.clone())
        .unwrap_or_else(|| current.as_str().unwrap_or("Default").to_owned());
    let open = s.ui.settings.open_select.as_deref() == Some(key);
    let owned = key.to_owned();
    let trigger = row()
        .key(format!("setting-{key}"))
        .w(width)
        .h(28.0)
        .px(9.0)
        .gap(8.0)
        .center_y()
        .radius(6.0)
        .border(1.0, t.c.border)
        .fill(t.c.input.alpha_mul(0.3))
        .hover(t.c.accent)
        .focusable(true)
        .role("combobox")
        .label(title)
        .expanded(open)
        .focus_ring(2.0, 1.0, t.c.ring)
        .on_press(move |s: &mut App| {
            s.ui.settings.open_select = if open { None } else { Some(owned.clone()) }
        })
        .child(
            text(display)
                .font(theme::TEXT_XS)
                .color(t.c.foreground)
                .grow(1.0),
        )
        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(t.c.muted_foreground));
    trigger
}

pub fn select_menu(s: &App, t: &T, key: &str, options: &[(Value, String)], width: f32) -> El {
    let current = value(s, key);
    let mut menu = col()
        .key(format!("setting-menu-{key}"))
        .w(width)
        .padding(4.0)
        .gap(2.0)
        .radius(6.0)
        .border(1.0, t.c.border)
        .fill(t.c.popover)
        .role("listbox")
        .focus_group()
        .focus_follows();
    for (index, (v, label)) in options.iter().enumerate() {
        let (key, v) = (key.to_owned(), v.clone());
        menu = menu.child(
            el().w_full()
                .min_h(28.0)
                .key(format!("setting-option-{key}-{index}"))
                .px(8.0)
                .center_y()
                .radius(4.0)
                .hover(t.c.accent)
                .focus_highlight(t.c.accent)
                .focusable(true)
                .role("option")
                .label(label.clone())
                .focus_selected(v == current)
                .on_press(move |s: &mut App| {
                    update(s, &key, v.clone());
                    s.ui.settings.open_select = None;
                })
                .child(
                    text(label.clone())
                        .font(theme::TEXT_XS)
                        .color(t.c.popover_foreground),
                ),
        );
    }
    menu.modal(true, |s: &mut App| s.ui.settings.open_select = None)
        .focus_return(format!("setting-{key}"))
        .anchored_fit(
            format!("setting-{key}"),
            rune::Side::Bottom,
            rune::Align::End,
            4.0,
        )
}

pub fn setting_row(s: &App, t: &T, spec: &Value) -> El {
    let key = spec["key"].as_str().unwrap_or("");
    let title = spec["title"].as_str().unwrap_or(key);
    let description = spec["description"].as_str().unwrap_or("");
    let mut copy = col().grow(1.0).min_w(0.0).gap(4.0).child(
        text(title.to_owned())
            .font(theme::SM_MEDIUM)
            .color(t.c.foreground),
    );
    if !description.is_empty() {
        copy = copy.child(
            text(description.to_owned())
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
            select(
                s,
                t,
                key,
                title,
                &options,
                if key == "notificationMode" {
                    224.0
                } else {
                    160.0
                },
            )
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
            rune::text_input(&edit, theme::TEXT_XS)
                .key(format!("setting-{key}"))
                .w(208.0)
                .min_h(28.0)
                .px(9.0)
                .py(4.0)
                .radius(6.0)
                .border(1.0, t.c.border)
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
            text(title.to_owned())
                .h(28.0)
                .px(16.0)
                .font(theme::weight(theme::TEXT_BASE, theme::SEMIBOLD))
                .color(t.c.foreground),
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
    let ratio = ((n - min) / (max - min)).clamp(0.0, 1.0);
    let (k1, k2) = (key.to_owned(), key.to_owned());
    row()
        .w(208.0)
        .center_y()
        .gap(12.0)
        .child(
            el().w(48.0)
                .h(24.0)
                .center()
                .radius(6.0)
                .fill(t.c.muted)
                .child(
                    text(format!(
                        "{n}{}",
                        if key == "panelAnimationDurationMs" {
                            " ms"
                        } else {
                            "%"
                        }
                    ))
                    .font(theme::MONO_XS)
                    .color(t.c.foreground),
                ),
        )
        .child(
            el().w(148.0)
                .h(20.0)
                .role("slider")
                .label(title)
                .focusable(true)
                .on_drag(move |s: &mut App, d, _| {
                    if !d.cancelled() {
                        let next = (min + (d.x / d.w).clamp(0.0, 1.0) * (max - min)) / step;
                        update(s, &k1, json!(next.round() * step));
                    }
                })
                .on_key(move |s: &mut App, e, _| {
                    let delta = match e.key {
                        rune::Key::Right | rune::Key::Up => step,
                        rune::Key::Left | rune::Key::Down => -step,
                        _ => return false,
                    };
                    update(s, &k2, json!((n + delta).clamp(min, max)));
                    true
                })
                .child(
                    el().absolute(0.0, 8.0)
                        .w(148.0)
                        .h(4.0)
                        .radius(2.0)
                        .fill(t.c.muted),
                )
                .child(
                    el().absolute(0.0, 8.0)
                        .w(148.0 * ratio)
                        .h(4.0)
                        .radius(2.0)
                        .fill(t.c.primary),
                )
                .child(
                    el().absolute(132.0 * ratio, 2.0)
                        .size(16.0, 16.0)
                        .radius(8.0)
                        .fill(t.c.background)
                        .border(1.0, t.c.primary),
                ),
        )
}
