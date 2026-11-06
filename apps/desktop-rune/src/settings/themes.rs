use crate::{
    App, Appearance,
    theme::{self, AlphaMul, T},
};
use rune::{Color, El, col, el, row, text};
use serde_json::{Value, json};
use std::sync::OnceLock;

pub fn builtins() -> &'static Vec<Value> {
    static THEMES: OnceLock<Vec<Value>> = OnceLock::new();
    THEMES.get_or_init(|| serde_json::from_str(include_str!("themes.json")).unwrap())
}
pub fn library(s: &App) -> Vec<Value> {
    builtins()
        .iter()
        .cloned()
        .chain(
            s.client_settings
                .values
                .get("customThemes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .cloned(),
        )
        .collect()
}
pub fn color(css: &str) -> Option<Color> {
    if css.starts_with('#') {
        return rune::color::hex(css);
    }
    let raw = css.strip_prefix("oklch(")?.strip_suffix(')')?;
    let mut split = raw.split('/');
    let p: Vec<_> = split.next()?.split_whitespace().collect();
    if p.len() != 3 {
        return None;
    }
    let number = |s: &str| -> Option<f32> {
        s.trim_end_matches('%')
            .parse::<f32>()
            .ok()
            .map(|v| if s.ends_with('%') { v / 100.0 } else { v })
    };
    let mut c = rune::color::oklch(number(p[0])?, number(p[1])?, number(p[2])?);
    if let Some(a) = split.next() {
        c.a = number(a.trim())?;
    }
    Some(c)
}
fn palette(theme: &Value, dark: bool) -> &Value {
    let mode = if dark { "dark" } else { "light" };
    theme["variants"].get(mode).unwrap_or(&theme["colors"])
}
fn paint(p: &Value, key: &str) -> Color {
    p[key]
        .as_str()
        .and_then(color)
        .unwrap_or(Color::TRANSPARENT)
}
pub fn apply(s: &App, mut t: T) -> T {
    let key = if t.dark { "themeDark" } else { "themeLight" };
    let id = s
        .client_settings
        .values
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("t3-code");
    if id == "t3-code" {
        return t;
    }
    let themes = library(s);
    let Some(selected) = themes.iter().find(|v| v["id"] == id) else {
        return t;
    };
    let p = palette(selected, t.dark);
    macro_rules! set {($($field:ident => $role:literal),* $(,)?)=>{$(t.c.$field=paint(p,$role);)*};}
    set!(background=>"canvas",foreground=>"text",contrast_foreground=>"text",muted_foreground=>"textMuted",contrast_muted_foreground=>"textMuted",card=>"surface",contrast_card_foreground=>"text",popover=>"surfaceOverlay",popover_foreground=>"text",contrast_popover_foreground=>"text",border=>"border",contrast_border=>"border",input=>"input",contrast_input=>"input",primary=>"accent",primary_foreground=>"accentForeground",accent=>"accentSurface",accent_foreground=>"accentSurfaceForeground",contrast_accent_foreground=>"accentSurfaceForeground",secondary=>"secondary",secondary_foreground=>"secondaryForeground",contrast_secondary_foreground=>"secondaryForeground",muted=>"muted",ring=>"focus",sidebar=>"sidebar",sidebar_foreground=>"sidebarForeground",surface_raised=>"surfaceRaised");
    t.sb = t.c;
    t.sb.sidebar = paint(p, "sidebar");
    t.sb.foreground = paint(p, "sidebarForeground");
    t.sb.muted_foreground = paint(p, "sidebarMutedForeground");
    t.sb.accent = paint(p, "sidebarRowHover");
    t
}
fn orb(theme: &Value, dark: bool) -> El {
    static PREVIEW: OnceLock<Value> = OnceLock::new();
    let preview =
        PREVIEW.get_or_init(|| serde_json::from_str(include_str!("theme-preview.json")).unwrap());
    let mode = if dark { "dark" } else { "light" };
    let p = if theme["id"] == "t3-code" {
        &preview["standard"][mode]
    } else {
        palette(theme, dark)
    };
    let spec = &preview["specs"][mode];
    let point = |key: &str| {
        [
            spec[key]["center"][0].as_f64().unwrap() as f32,
            spec[key]["center"][1].as_f64().unwrap() as f32,
        ]
    };
    let n = |key: &str, field: &str| spec[key][field].as_f64().unwrap() as f32;
    el().size(56.0, 56.0)
        .radius(28.0)
        .clip_rounded(28.0)
        .border(2.0, Color::BLACK)
        .child(
            el().absolute(-2.8, -2.8)
                .size(61.6, 61.6)
                .fill(paint(p, "canvas"))
                .blur(3.0)
                .radial_gradient(
                    point("action"),
                    67.0,
                    [
                        (
                            0.0,
                            paint(p, "messageAction").alpha_mul(n("action", "startOpacity")),
                        ),
                        (n("action", "endOffset"), Color::TRANSPARENT),
                    ],
                )
                .child(el().w_full().h_full().radial_gradient(
                    point("accent"),
                    67.0,
                    [
                        (0.0, paint(p, "accent")),
                        (
                            n("accent", "middleOffset"),
                            paint(p, "accent").alpha_mul(n("accent", "middleOpacity")),
                        ),
                        (n("accent", "endOffset"), Color::TRANSPARENT),
                    ],
                )),
        )
}

fn wireframe(p: &Value) -> El {
    row()
        .w_full()
        .h(140.0)
        .radius(8.0)
        .clip_rounded(8.0)
        .border(1.0, paint(p, "border"))
        .child(
            col()
                .w(56.0)
                .h_full()
                .padding(8.0)
                .gap(5.0)
                .fill(paint(p, "sidebar"))
                .child(
                    el().w_full()
                        .h(10.0)
                        .radius(4.0)
                        .border(1.0, paint(p, "border")),
                )
                .children((0..3).map(|_| {
                    el().w_full()
                        .h(10.0)
                        .radius(4.0)
                        .fill(paint(p, "textMuted").alpha_mul(0.16))
                })),
        )
        .child(
            col()
                .grow(1.0)
                .h_full()
                .padding(12.0)
                .gap(8.0)
                .fill(paint(p, "canvas"))
                .child(
                    el().w(60.0)
                        .h(13.0)
                        .radius(8.0)
                        .fill(paint(p, "textMuted").alpha_mul(0.2)),
                )
                .child(
                    el().w(86.0)
                        .h(6.0)
                        .radius(3.0)
                        .fill(paint(p, "textMuted").alpha_mul(0.24)),
                )
                .child(
                    el().w(65.0)
                        .h(6.0)
                        .radius(3.0)
                        .fill(paint(p, "textMuted").alpha_mul(0.24)),
                )
                .child(el().grow(1.0))
                .child(
                    el().w_full()
                        .h(20.0)
                        .radius(6.0)
                        .border(1.0, paint(p, "border")),
                ),
        )
}
pub fn view(s: &App, t: &T) -> El {
    let mut modes = row().w_full().gap(12.0);
    for (mode, label) in [("system", "System"), ("light", "Light"), ("dark", "Dark")] {
        let selected = matches!(
            (s.appearance, mode),
            (Appearance::System, "system")
                | (Appearance::Light, "light")
                | (Appearance::Dark, "dark")
        );
        modes = modes.child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .padding(8.0)
                .gap(6.0)
                .radius(12.0)
                .border(
                    if selected { 2.0 } else { 1.0 },
                    if selected { t.c.ring } else { t.c.border },
                )
                .fill(t.c.card.alpha_mul(0.4))
                .focusable(true)
                .role("button")
                .label(label)
                .on_press(move |s: &mut App| super::update(s, "appearance", json!(mode)))
                .child(if mode == "system" {
                    row()
                        .w_full()
                        .h(140.0)
                        .radius(8.0)
                        .clip_rounded(8.0)
                        .child(
                            el().grow(1.0).min_w(0.0).h_full().clip_rounded(0.0).child(
                                wireframe(palette(&builtins()[0], false))
                                    .w(256.0)
                                    .min_w(256.0),
                            ),
                        )
                        .child(
                            el().grow(1.0).min_w(0.0).h_full().clip_rounded(0.0).child(
                                wireframe(palette(&builtins()[0], true))
                                    .w(256.0)
                                    .min_w(256.0)
                                    .offset(-128.0, 0.0),
                            ),
                        )
                } else {
                    wireframe(palette(&builtins()[0], mode == "dark"))
                })
                .child(
                    el().w_full()
                        .center()
                        .child(text(label).font(theme::TEXT_XS).color(t.c.muted_foreground)),
                ),
        );
    }
    let mut cards = col().w_full().gap(8.0);
    for group in library(s).chunks(3) {
        let mut r = row().w_full().gap(8.0);
        for item in group {
            let id = item["id"].as_str().unwrap_or("").to_owned();
            let label = item["label"].as_str().unwrap_or("").to_owned();
            let mut previews = row().w_full().h(70.0).gap(20.0).center();
            for dark in [false, true] {
                let selected = s
                    .client_settings
                    .values
                    .get(if dark { "themeDark" } else { "themeLight" })
                    .and_then(Value::as_str)
                    .unwrap_or("t3-code")
                    == id;
                let choice = id.clone();
                previews = previews.child(
                    el().padding(4.0)
                        .radius(999.0)
                        .border(
                            2.0,
                            if selected {
                                t.c.ring
                            } else {
                                Color::TRANSPARENT
                            },
                        )
                        .focusable(true)
                        .role("button")
                        .label(format!("{label} {}", if dark { "dark" } else { "light" }))
                        .on_press(move |s: &mut App| {
                            super::update(
                                s,
                                if dark { "themeDark" } else { "themeLight" },
                                json!(choice),
                            )
                        })
                        .child(orb(item, dark)),
                );
            }
            let template = item.clone();
            let editor_dark = t.dark;
            r = r.child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .h(126.0)
                    .padding(12.0)
                    .gap(8.0)
                    .radius(12.0)
                    .border(1.0, t.c.border)
                    .fill(t.c.card.alpha_mul(0.4))
                    .child(previews)
                    .child(
                        row()
                            .w_full()
                            .center_y()
                            .child(
                                text(label)
                                    .font(theme::SM_MEDIUM)
                                    .color(t.c.foreground)
                                    .grow(1.0),
                            )
                            .child(
                                crate::kit::Button::new("Edit theme")
                                    .variant("ghost-muted")
                                    .size("icon-micro")
                                    .icon(&crate::icons::PENCIL)
                                    .on_press(move |s: &mut App| {
                                        edit(s, template.clone(), editor_dark)
                                    })
                                    .build(t),
                            ),
                    ),
            );
        }
        cards = cards.child(r);
    }
    let editor_dark = t.dark;
    col()
        .w_full()
        .gap(32.0)
        .child(
            col()
                .w_full()
                .gap(10.0)
                .child(
                    el().h(28.0).px(16.0).center_y().child(
                        text("Color scheme")
                            .font(theme::weight(theme::TEXT_BASE, 600.0))
                            .color(t.c.foreground),
                    ),
                )
                .child(modes.mt(2.0)),
        )
        .child(
            col()
                .w_full()
                .gap(10.0)
                .child(
                    row()
                        .w_full()
                        .h(28.0)
                        .px(16.0)
                        .center_y()
                        .child(
                            text("Themes")
                                .font(theme::weight(theme::TEXT_BASE, 600.0))
                                .color(t.c.foreground)
                                .grow(1.0),
                        )
                        .child(
                            crate::kit::Button::new("Add theme")
                                .variant("outline")
                                .size("xs")
                                .on_press(|s: &mut App| s.ui.settings.theme_import_open = true)
                                .build(t),
                        )
                        .child(
                            crate::kit::Button::new("Create theme")
                                .variant("outline")
                                .size("xs")
                                .on_press(move |s: &mut App| {
                                    edit(s, builtins()[0].clone(), editor_dark)
                                })
                                .build(t),
                        ),
                )
                .child(cards),
        )
}
pub fn edit(s: &mut App, mut template: Value, dark: bool) {
    if builtins().iter().any(|t| t["id"] == template["id"]) {
        template["id"] = json!(format!("custom-{}", s5_net::uuid()));
        template["label"] = json!(format!(
            "{} copy",
            template["label"].as_str().unwrap_or("Theme")
        ));
    }
    s.ui.settings.theme_edit_dark = dark;
    s.ui.settings.theme_editor = Some(template.clone());
    s.ui.settings.theme_name =
        rune::TextEdit::new(template["label"].as_str().unwrap_or("Custom theme"));
    let mode = dark;
    s.ui.settings.theme_colors = palette(&template, mode)
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| Some((k.clone(), rune::TextEdit::new(v.as_str()?))))
        .collect();
}
pub fn editor(s: &App, t: &T, layers: &mut Vec<El>) {
    if s.ui.settings.theme_editor.is_none() {
        return;
    }
    let mut fields = col().w_full().gap(12.0).child(
        crate::kit::Input::new(&s.ui.settings.theme_name)
            .placeholder("Theme name")
            .on_edit(|s: &mut App| &mut s.ui.settings.theme_name)
            .build(t),
    );
    let mut keys: Vec<_> = s.ui.settings.theme_colors.keys().cloned().collect();
    keys.sort();
    for key in keys {
        let owned = key.clone();
        let edit = &s.ui.settings.theme_colors[&key];
        fields = fields.child(
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(
                    text(key)
                        .w(180.0)
                        .font(theme::TEXT_SM)
                        .color(t.c.foreground),
                )
                .child(
                    crate::kit::Input::new(edit)
                        .invalid(color(&edit.text).is_none())
                        .on_edit(move |s: &mut App| {
                            s.ui.settings.theme_colors.get_mut(&owned).unwrap()
                        })
                        .build(t),
                ),
        );
    }
    fields = fields.child(
        crate::kit::Button::new("Save theme")
            .disabled(
                s.ui.settings.theme_name.text.trim().is_empty()
                    || s.ui
                        .settings
                        .theme_colors
                        .values()
                        .any(|e| color(&e.text).is_none()),
            )
            .on_press(|s: &mut App| {
                let mut theme = s.ui.settings.theme_editor.take().unwrap();
                theme["label"] = json!(s.ui.settings.theme_name.text.trim());
                let mode = if s.ui.settings.theme_edit_dark {
                    "dark"
                } else {
                    "light"
                };
                let colors = Value::Object(
                    s.ui.settings
                        .theme_colors
                        .iter()
                        .map(|(k, v)| (k.clone(), json!(v.text)))
                        .collect(),
                );
                if theme["appearance"] == mode {
                    theme["colors"] = colors;
                } else {
                    if !theme["variants"].is_object() {
                        theme["variants"] = json!({});
                    }
                    theme["variants"][mode] = colors;
                }
                let mut library = s
                    .client_settings
                    .values
                    .get("customThemes")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if let Some(existing) = library.iter_mut().find(|v| v["id"] == theme["id"]) {
                    *existing = theme;
                } else {
                    library.push(theme);
                }
                super::update(s, "customThemes", json!(library));
            })
            .build(t),
    );
    if let Some(id) =
        s.ui.settings
            .theme_editor
            .as_ref()
            .and_then(|v| v["id"].as_str())
            .filter(|id| {
                s.client_settings
                    .values
                    .get("customThemes")
                    .and_then(Value::as_array)
                    .is_some_and(|themes| themes.iter().any(|v| v["id"] == *id))
            })
    {
        let id = id.to_owned();
        fields = fields.child(
            crate::kit::Button::new("Delete theme")
                .variant("destructive-outline")
                .on_press(move |s: &mut App| {
                    let mut library = s
                        .client_settings
                        .values
                        .get("customThemes")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    library.retain(|t| t["id"] != id);
                    for key in ["themeLight", "themeDark"] {
                        if s.client_settings.values.get(key).is_some_and(|v| *v == id) {
                            super::update(s, key, json!("t3-code"));
                        }
                    }
                    super::update(s, "customThemes", json!(library));
                    s.ui.settings.theme_editor = None;
                })
                .build(t),
        );
    }
    crate::kit::Dialog::new("theme-editor", "Create theme")
        .open(true)
        .width(680.0)
        .on_close(|s: &mut App| s.ui.settings.theme_editor = None)
        .child(
            col()
                .h(600.0)
                .w_full()
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                })
                .child(fields),
        )
        .build(t, layers);
}

pub fn import_theme(raw: &str) -> Result<Value, String> {
    let mut theme: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let appearance = theme["appearance"]
        .as_str()
        .filter(|a| matches!(*a, "light" | "dark"))
        .ok_or("Choose a light or dark appearance.")?
        .to_owned();
    let label = theme["name"]
        .as_str()
        .or(theme["label"].as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or("Theme name is required.")?
        .to_owned();
    let overrides = theme["colors"]
        .as_object()
        .ok_or("Theme colors must be an object.")?;
    let mut colors = palette(&builtins()[0], appearance == "dark").clone();
    for (role, css) in overrides {
        if colors.get(role).is_none() {
            continue;
        }
        let css = css
            .as_str()
            .filter(|s| color(s).is_some())
            .ok_or_else(|| format!("Invalid color for {role}. Use hex or oklch."))?;
        colors[role] = json!(css);
    }
    theme["id"] = json!(format!("custom-{}", s5_net::uuid()));
    theme["label"] = json!(label);
    theme["colors"] = colors;
    if let Some(variants) = theme["variants"].as_object() {
        for variant in variants.values() {
            for css in variant
                .as_object()
                .ok_or("Invalid variant colors.")?
                .values()
            {
                if css.as_str().and_then(color).is_none() {
                    return Err("Invalid color in theme variant.".into());
                }
            }
        }
    }
    Ok(theme)
}
pub fn import_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    if !s.ui.settings.theme_import_open {
        return;
    }
    crate::kit::Dialog::new("import-theme", "Add theme")
        .open(true)
        .width(640.0)
        .description("Paste a theme JSON file.")
        .on_close(|s: &mut App| s.ui.settings.theme_import_open = false)
        .child(
            rune::text_input(&s.ui.settings.theme_import, theme::MONO_XS)
                .w_full()
                .h(260.0)
                .padding(12.0)
                .wrap()
                .border(1.0, t.c.border)
                .radius(8.0)
                .color(t.c.foreground)
                .on_edit(|s: &mut App| &mut s.ui.settings.theme_import),
        )
        .child(
            crate::kit::Button::new("Import theme")
                .on_press(
                    |s: &mut App| match import_theme(&s.ui.settings.theme_import.text) {
                        Ok(theme) => {
                            let mut library = s
                                .client_settings
                                .values
                                .get("customThemes")
                                .and_then(Value::as_array)
                                .cloned()
                                .unwrap_or_default();
                            library.push(theme);
                            super::update(s, "customThemes", json!(library));
                            s.ui.settings.theme_import_open = false;
                            s.ui.settings.theme_import.clear();
                        }
                        Err(error) => s.ui.settings.error = Some(error),
                    },
                )
                .build(t),
        )
        .build(t, layers);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn theme_import_validates_colors_before_filling_missing_roles() {
        let theme = import_theme(
            r##"{"name":"Example","appearance":"dark","colors":{"accent":"#123456"}}"##,
        )
        .unwrap();
        assert_eq!(theme["colors"]["accent"], "#123456");
        assert!(theme["colors"].get("canvas").is_some());
        assert!(
            import_theme(
                r#"{"name":"Bad","appearance":"dark","colors":{"accent":"url(example)"}}"#
            )
            .is_err()
        );
    }
}
