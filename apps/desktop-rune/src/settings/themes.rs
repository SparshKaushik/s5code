use crate::{
    App, Appearance,
    theme::{self, AlphaMul, T},
};
use rune::{Color, El, col, el, icon, row, text};
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
pub fn apply(s: &App, t: T) -> T {
    let percent = |key: &str, default: f64| s.client_settings.values.get(key).and_then(Value::as_f64).unwrap_or(default) as f32;
    crate::kit::menu::set_glass_opacity(percent("glassOpacity", 80.0));
    let mut t = apply_theme(s, t);
    t.code = super::fonts::code_size(&s.client_settings.values);
    t.px = 1.0 / super::fonts::interface_scale(&s.client_settings.values);
    super::diff_scheme::apply(super::value(s, "diffColorScheme").as_str().unwrap_or(""), &mut t);
    let contrast = percent("appearanceContrast", 100.0);
    if contrast != 100.0 {
        let target = if t.dark { rune::Color::WHITE } else { rune::Color::BLACK };
        t.c = contrasted(t.c, contrast, target);
        t.sb = contrasted(t.sb, contrast, target);
    }
    t
}

/// `color-mix(in oklab, a p, b)`.
fn mix_oklab(a: rune::Color, b: rune::Color, p: f32) -> rune::Color {
    let (x, y) = (a.to_oklab(), b.to_oklab());
    rune::Color::from_oklab(std::array::from_fn(|i| x[i] * p + y[i] * (1.0 - p)))
}

/// `color-mix(in srgb, color-mix(in srgb, a p, transparent), b q)`: premultiplied, gamma-encoded.
fn mix_border(a: rune::Color, b: rune::Color, p: f32, q: f32) -> rune::Color {
    use rune::color::{srgb_decode, srgb_encode};
    let (wa, wb) = (a.a * p * (1.0 - q), b.a * q);
    let alpha = wa + wb;
    if alpha <= 0.0 {
        return rune::Color::TRANSPARENT;
    }
    let ch = |x: f32, y: f32| srgb_decode((srgb_encode(x) * wa + srgb_encode(y) * wb) / alpha);
    rune::Color { r: ch(a.r, b.r), g: ch(a.g, b.g), b: ch(a.b, b.b), a: alpha }
}

/// Web's `--contrast-*` tokens (index.css) for an Appearance contrast other than 100%. Rune's
/// views read the plain tokens for `text-foreground` and friends, so both are re-pointed.
fn contrasted(c: crate::tokens::Tokens, contrast: f32, target: rune::Color) -> crate::tokens::Tokens {
    let base = contrast.min(100.0) / 100.0;
    let boost = (contrast - 100.0).max(0.0) / 100.0;
    let border_boost = boost / 4.0;
    let text = |fg: rune::Color, bg: rune::Color| mix_oklab(mix_oklab(fg, bg, base), target, 1.0 - boost);
    let mut out = c;
    macro_rules! contrast_text {($($field:ident / $contrast:ident on $bg:ident),* $(,)?)=>{$(
        out.$contrast = text(c.$field, c.$bg);
        out.$field = out.$contrast;
    )*};}
    contrast_text!(
        foreground / contrast_foreground on background,
        muted_foreground / contrast_muted_foreground on background,
        placeholder / contrast_placeholder on background,
        secondary_label / contrast_secondary_label on background,
        icon_muted / contrast_icon_muted on background,
        card_foreground / contrast_card_foreground on card,
        popover_foreground / contrast_popover_foreground on popover,
        accent_foreground / contrast_accent_foreground on accent,
        secondary_foreground / contrast_secondary_foreground on secondary,
        message_foreground / contrast_message_foreground on message_surface,
        sidebar_foreground / contrast_sidebar_foreground on sidebar,
        sidebar_muted_foreground / contrast_sidebar_muted_foreground on sidebar,
    );
    out.contrast_border = mix_border(c.border, c.foreground, base, border_boost);
    out.contrast_input = mix_border(c.input, c.foreground, base, border_boost);
    out.contrast_sidebar_border = mix_border(c.sidebar_border, c.sidebar_foreground, base, border_boost);
    out.border = out.contrast_border;
    out.input = out.contrast_input;
    out.sidebar_border = out.contrast_sidebar_border;
    out
}

fn apply_theme(s: &App, mut t: T) -> T {
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
    // A custom theme saved before the terminal roles keeps the base terminal colours.
    for (slot, role) in [
        (&mut t.c.terminal_background, "terminalBackground"),
        (&mut t.c.terminal_foreground, "terminalForeground"),
        (&mut t.c.terminal_cursor, "terminalCursor"),
        (&mut t.c.terminal_selection_background, "terminalSelection"),
    ] {
        if p[role].is_string() {
            *slot = paint(p, role);
        }
    }
    t.sb = t.c;
    t.sb.sidebar = paint(p, "sidebar");
    t.sb.foreground = paint(p, "sidebarForeground");
    t.sb.muted_foreground = paint(p, "sidebarMutedForeground");
    t.sb.accent = paint(p, "sidebarRowHover");
    t
}
fn preview() -> &'static Value {
    static PREVIEW: OnceLock<Value> = OnceLock::new();
    PREVIEW.get_or_init(|| serde_json::from_str(include_str!("theme-preview.json")).unwrap())
}
fn preview_palette(theme: &Value, dark: bool) -> &Value {
    if theme["id"] == "t3-code" {
        &preview()["standard"][if dark { "dark" } else { "light" }]
    } else {
        palette(theme, dark)
    }
}
fn scheme_colors(s: &App, dark: bool) -> Value {
    let id = s
        .client_settings
        .values
        .get(if dark { "themeDark" } else { "themeLight" })
        .and_then(Value::as_str)
        .unwrap_or("t3-code");
    let themes = library(s);
    let theme = themes.iter().find(|v| v["id"] == id).unwrap_or(&builtins()[0]);
    preview_palette(theme, dark).clone()
}
fn orb(theme: &Value, dark: bool, background: Color) -> El {
    let p = preview_palette(theme, dark);
    let spec = &preview()["specs"][if dark { "dark" } else { "light" }];
    let point = |key: &str| {
        [
            spec[key]["center"][0].as_f64().unwrap() as f32,
            spec[key]["center"][1].as_f64().unwrap() as f32,
        ]
    };
    let n = |key: &str, field: &str| spec[key][field].as_f64().unwrap() as f32;
    let top = |key: &str, default: f64| spec[key].as_f64().unwrap_or(default) as f32;
    let size = 52.0 * top("scale", 1.1);
    let inset = (size - 52.0) / 2.0;
    let reach = |c: [f32; 2]| size * c[0].max(1.0 - c[0]).hypot(c[1].max(1.0 - c[1]));
    let base = mix_oklab(
        paint(p, "canvas"),
        spec["baseTarget"].as_str().and_then(color).unwrap_or_default(),
        top("baseWeight", 0.8),
    );
    let (accent, action) = (point("accent"), point("action"));
    let edge = if dark {
        Color::WHITE.alpha_mul(0.14)
    } else {
        Color::BLACK.alpha_mul(0.1)
    };
    el().size(56.0, 56.0)
        .padding(2.0)
        .radius(28.0)
        .fill(background)
        .shadow(1.0, 2.0, Color::BLACK.alpha_mul(if dark { 0.18 } else { 0.08 }))
        .child(
            el().size(52.0, 52.0)
                .radius(26.0)
                .clip_rounded(26.0)
                .box_shadow(rune::Shadow {
                    spread: 1.0,
                    color: edge,
                    inset: true,
                    ..Default::default()
                })
                .child(
                    el().absolute(-inset, -inset)
                        .size(size, size)
                        .fill(base)
                        .blur(top("blurAt56Px", 3.0))
                        .radial_gradient(
                            action,
                            reach(action),
                            [
                                (
                                    0.0,
                                    paint(p, "messageAction").alpha_mul(n("action", "startOpacity")),
                                ),
                                (n("action", "endOffset"), Color::TRANSPARENT),
                            ],
                        )
                        .child(el().w_full().h_full().radial_gradient(
                            accent,
                            reach(accent),
                            [
                                (0.0, paint(p, "accent")),
                                (
                                    n("accent", "middleOffset"),
                                    paint(p, "accent").alpha_mul(n("accent", "middleOpacity")),
                                ),
                                (n("accent", "endOffset"), Color::TRANSPARENT),
                            ],
                        )),
                ),
        )
}

const WIREFRAME_H: f32 = 138.0;

fn wireframe_pane(p: &Value) -> El {
    let line = color("#7f7f7f40").unwrap_or_default();
    let band = |top: f32, height: f32| {
        row()
            .pin(Some(0.0), Some(top * WIREFRAME_H), Some(0.0), None)
            .h(height * WIREFRAME_H)
    };
    let edge = || rune::Shadow {
        spread: 1.0,
        color: line,
        inset: true,
        ..Default::default()
    };
    let pill = |top: f32, height: f32, fill: Color, opacity: f32| {
        band(top, height).child(el().w_frac(0.03)).child(
            el().w_frac(0.16)
                .h_full()
                .radius(6.0)
                .fill(fill)
                .opacity(opacity),
        )
    };
    let bar = |top: f32, width: f32| {
        band(top, 0.05)
            .child(el().w_frac(0.27))
            .child(el().w_frac(width).h_full().radius(4.0).fill(line))
    };
    let composer_h = 0.15 * WIREFRAME_H;
    let send = 0.58 * composer_h;
    let panel_h = 0.46 * WIREFRAME_H;
    let dots = [
        color("#34d399").unwrap_or_default(),
        paint(p, "messageAction"),
        color("#fbbf24").unwrap_or_default(),
    ];
    el().fill(paint(p, "canvas"))
        .child(
            el().pin(Some(0.0), Some(0.0), None, Some(0.0))
                .w_frac(0.22)
                .fill(paint(p, "sidebar"))
                .child(el().pin(None, Some(0.0), Some(0.0), Some(0.0)).w(1.0).fill(line)),
        )
        .child(
            band(0.08, 0.08).child(el().w_frac(0.03)).child(
                el().w_frac(0.16)
                    .h_full()
                    .radius(6.0)
                    .fill(paint(p, "surface"))
                    .box_shadow(edge()),
            ),
        )
        .child(pill(0.22, 0.07, paint(p, "accentSurface"), 1.0))
        .child(pill(0.32, 0.07, paint(p, "messageSurface"), 0.7))
        .child(pill(0.42, 0.07, paint(p, "messageSurface"), 0.5))
        .child(
            band(0.11, 0.09)
                .justify_end()
                .child(
                    el().w_frac(0.24)
                        .h_full()
                        .radius(8.0)
                        .fill(paint(p, "messageSurface")),
                )
                .child(el().w_frac(0.28)),
        )
        .child(bar(0.28, 0.34))
        .child(bar(0.38, 0.26))
        .child(
            band(0.77, 0.15).child(el().w_frac(0.26)).child(
                row()
                    .w_frac(0.68)
                    .h_full()
                    .px(4.0)
                    .center_y()
                    .justify_between()
                    .radius(6.0)
                    .fill(paint(p, "surface"))
                    .box_shadow(edge())
                    .child(
                        el().w_frac(0.34)
                            .h(0.26 * composer_h)
                            .radius(999.0)
                            .fill(line)
                            .opacity(0.7),
                    )
                    .child(
                        el().size(send, send)
                            .radius(send / 2.0)
                            .fill(paint(p, "messageAction")),
                    ),
            ),
        )
        .child(
            band(0.08, 0.46)
                .justify_end()
                .child(
                    el().w_frac(0.2)
                        .h_full()
                        .radius(8.0)
                        .fill(paint(p, "surface"))
                        .box_shadow(edge())
                        .shadow(2.0, 5.0, Color::BLACK.alpha_mul(0.14))
                        .children(dots.into_iter().enumerate().map(|(i, dot)| {
                            let dot_size = 0.26 * 0.2 * panel_h;
                            row()
                                .pin(
                                    Some(0.0),
                                    Some((0.1 + 0.3 * i as f32) * panel_h),
                                    Some(0.0),
                                    None,
                                )
                                .h(0.2 * panel_h)
                                .child(el().w_frac(0.11))
                                .child(
                                    row()
                                        .w_frac(0.78)
                                        .h_full()
                                        .gap(4.0)
                                        .center_y()
                                        .child(
                                            el().size(dot_size, dot_size)
                                                .radius(dot_size / 2.0)
                                                .fill(dot)
                                                .opacity(0.55),
                                        )
                                        .child(
                                            el().w_frac(0.52)
                                                .h(0.3 * 0.2 * panel_h)
                                                .radius(4.0)
                                                .fill(line),
                                        ),
                                )
                        })),
                )
                .child(el().w_frac(0.05)),
        )
}

fn wireframe(t: &T, light: &Value, dark: Option<&Value>) -> El {
    let panes = match dark {
        None => el()
            .w_full()
            .h_full()
            .child(wireframe_pane(light).pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))),
        Some(dark) => row()
            .w_full()
            .h_full()
            .gap(2.0)
            .child(
                el().grow(1.0).min_w(0.0).h_full().clip_rounded(0.0).child(
                    wireframe_pane(light)
                        .pin(Some(0.0), Some(0.0), None, Some(0.0))
                        .w_frac(2.0),
                ),
            )
            .child(
                el().grow(1.0).min_w(0.0).h_full().clip_rounded(0.0).child(
                    wireframe_pane(dark)
                        .pin(None, Some(0.0), Some(0.0), Some(0.0))
                        .w_frac(2.0),
                ),
            ),
    };
    el().w_full()
        .h(WIREFRAME_H + 2.0)
        .padding(1.0)
        .radius(8.0)
        .border(1.0, t.c.border.alpha_mul(0.6))
        .child(
            el().w_full()
                .h_full()
                .radius(7.0)
                .clip_rounded(7.0)
                .child(panes),
        )
}
pub fn view(s: &App, t: &T) -> El {
    let mut modes = row().w_full().gap(12.0);
    let (light_colors, dark_colors) = (scheme_colors(s, false), scheme_colors(s, true));
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
                .padding(9.0)
                .gap(6.0)
                .radius(12.0)
                .border(
                    if selected { 2.0 } else { 1.0 },
                    if selected {
                        t.c.ring
                    } else {
                        t.c.border.alpha_mul(0.7)
                    },
                )
                .fill(if selected {
                    t.c.accent.alpha_mul(0.3)
                } else {
                    t.c.card.alpha_mul(0.6)
                })
                .focusable(true)
                .role("button")
                .label(label)
                .on_press(move |s: &mut App| super::update(s, "appearance", json!(mode)))
                .child(match mode {
                    "system" => wireframe(t, &light_colors, Some(&dark_colors)),
                    "dark" => wireframe(t, &dark_colors, None),
                    _ => wireframe(t, &light_colors, None),
                })
                .child(
                    el().w_full().center().child(
                        text(label)
                            .font(theme::XS_MEDIUM)
                            .color(if selected {
                                t.c.foreground
                            } else {
                                t.c.muted_foreground
                            }),
                    ),
                ),
        );
    }
    let mut cards = col().w_full().gap(8.0);
    for group in library(s).chunks(3) {
        let mut r = row().w_full().gap(8.0);
        for item in group {
            let id = item["id"].as_str().unwrap_or("").to_owned();
            let label = item["label"].as_str().unwrap_or("").to_owned();
            let mut previews = row().w_full().h(68.0).gap(10.0).center();
            for dark in [false, true] {
                let selected = s
                    .client_settings
                    .values
                    .get(if dark { "themeDark" } else { "themeLight" })
                    .and_then(Value::as_str)
                    .unwrap_or("t3-code")
                    == id;
                let choice = id.clone();
                let mut swatch = el()
                    .size(68.0, 68.0)
                    .padding(6.0)
                    .radius(34.0)
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
                    .child(orb(item, dark, t.c.background));
                if selected {
                    swatch = swatch.border(2.0, t.c.ring).child(
                        el().absolute(46.0, 46.0)
                            .size(20.0, 20.0)
                            .radius(10.0)
                            .fill(t.c.background)
                            .shadow(1.0, 3.0, Color::BLACK.alpha_mul(0.1))
                            .center()
                            .child(
                                el().absolute(0.0, 0.0)
                                    .size(20.0, 20.0)
                                    .radius(10.0)
                                    .border(1.0, t.c.border.alpha_mul(0.7)),
                            )
                            .child(
                                icon(
                                    if dark {
                                        &crate::icons::MOON
                                    } else {
                                        &crate::icons::SUN
                                    },
                                    12.0,
                                )
                                .color(t.c.foreground),
                            ),
                    );
                }
                previews = previews.child(swatch);
            }
            let template = item.clone();
            let editor_dark = t.dark;
            // The standard theme has no actions; other built-ins duplicate, saved themes edit.
            let action = if item["id"] == builtins()[0]["id"] {
                None
            } else if builtins().iter().any(|b| b["id"] == item["id"]) {
                Some((format!("Duplicate {label}"), &crate::icons::COPY))
            } else {
                Some(("Edit theme".to_owned(), &crate::icons::PENCIL))
            };
            r = r.child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .h(126.0)
                    .pt(13.0)
                    .px(13.0)
                    .pb(13.0)
                    .gap(8.0)
                    .radius(12.0)
                    .border(1.0, t.c.border.alpha_mul(0.7))
                    .fill(t.c.card.alpha_mul(0.6))
                    .child(previews)
                    .child(
                        row()
                            .w_full()
                            .gap(8.0)
                            .center_y()
                            .child(
                                text(label)
                                    .font(theme::SM_MEDIUM)
                                    .color(t.c.foreground)
                                    .grow(1.0),
                            )
                            .children(action.map(|(name, glyph)| {
                                crate::kit::Button::new(name)
                                    .variant("ghost")
                                    .size("icon-xs")
                                    .icon(glyph)
                                    .on_press(move |s: &mut App| {
                                        edit(s, template.clone(), editor_dark)
                                    })
                                    .build(t)
                            })),
                    ),
            );
        }
        // Web's auto-fit grid keeps a short last row at column width.
        for _ in group.len()..3 {
            r = r.child(el().grow(1.0).min_w(0.0));
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
                        .gap(8.0)
                        .center_y()
                        .child(
                            text("Themes")
                                .font(theme::weight(theme::TEXT_BASE, 600.0))
                                .color(t.c.foreground)
                                .grow(1.0),
                        )
                        .child(
                            crate::kit::Button::new("Create theme")
                                .variant("outline")
                                .size("xs")
                                .icon(&crate::icons::PAINTBRUSH)
                                .on_press(move |s: &mut App| {
                                    edit(s, builtins()[0].clone(), editor_dark)
                                })
                                .build(t),
                        )
                        .child(
                            crate::kit::Button::new("Add theme")
                                .variant("outline")
                                .size("xs")
                                .icon(&crate::icons::PLUS)
                                .on_press(|s: &mut App| s.ui.settings.theme_import_open = true)
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
    // Web's `isEditing`: the theme is already in the library (a builtin opens as a new copy).
    let editing = s.ui.settings.theme_editor.as_ref().is_some_and(|theme| {
        s.client_settings
            .values
            .get("customThemes")
            .and_then(Value::as_array)
            .is_some_and(|themes| themes.iter().any(|v| v["id"] == theme["id"]))
    });
    fields = fields.child(
        crate::kit::Button::new(if editing { "Save changes" } else { "Create theme" })
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
    crate::kit::Dialog::new("theme-editor", if editing { "Edit theme" } else { "Create theme" })
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
                .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
                .focus_ring(3.0, -3.0, t.c.ring.alpha_mul(0.24))
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
mod contrast_tests {
    use super::*;
    #[test]
    fn contrast_follows_web_color_mix() {
        let t = crate::theme::theme(true);
        let same = contrasted(t.c, 100.0, rune::Color::WHITE);
        let close = |a: rune::Color, b: rune::Color| (a.r - b.r).abs() + (a.g - b.g).abs() + (a.b - b.b).abs() + (a.a - b.a).abs() < 0.01;
        assert!(close(same.foreground, t.c.foreground), "100% leaves text alone");
        assert!(close(same.border, t.c.border), "100% leaves borders alone");
        let low = contrasted(t.c, 50.0, rune::Color::WHITE);
        assert!(low.foreground.luminance() < t.c.foreground.luminance(), "50% pulls text toward the background");
        let high = contrasted(t.c, 200.0, rune::Color::WHITE);
        assert!(close(high.foreground, rune::Color::WHITE), "200% is the target colour");
        assert!(high.border.a > t.c.border.a || high.border.luminance() > t.c.border.luminance());
    }
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

    #[test]
    fn a_theme_recolours_the_terminal() {
        let mut app = App::new();
        app.client_settings.values.insert("themeDark".into(), json!("grove"));
        let grove = builtins().iter().find(|v| v["id"] == "grove").unwrap();
        let t = apply_theme(&app, crate::theme::theme(true));
        let p = palette(grove, true);
        assert_eq!(t.c.terminal_background, paint(p, "terminalBackground"));
        assert_eq!(t.c.terminal_foreground, paint(p, "terminalForeground"));
        assert_eq!(t.c.terminal_cursor, paint(p, "terminalCursor"));
        assert_eq!(t.c.terminal_selection_background, paint(p, "terminalSelection"));
    }
}
