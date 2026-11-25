use crate::{
    App, Appearance,
    theme::{self, AlphaMul, T},
};
use rune::{Color, El, col, el, icon, row, text};
use serde_json::{Value, json};
use std::sync::OnceLock;

mod editor;
pub(crate) mod open_vsx;
mod vscode;

pub use editor::{edit, editor, escape as editor_escape, scene as editor_scene};
pub use open_vsx::{Search, poll};

pub fn builtins() -> &'static Vec<Value> {
    static THEMES: OnceLock<Vec<Value>> = OnceLock::new();
    THEMES.get_or_init(|| serde_json::from_str(include_str!("themes.json")).unwrap())
}
pub fn library(s: &App) -> Vec<Value> {
    let mut out: Vec<Value> = builtins()
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
        .collect();
    // `useThemeCards`: published themes come last, and an id already listed wins.
    for theme in environment_themes(s) {
        if !out.iter().any(|v| v["id"] == theme["id"]) {
            out.push(theme);
        }
    }
    out
}

/// `RESERVED_THEME_IDS`: appearance keywords, built-ins and legacy aliases.
const RESERVED: [&str; 13] =
    ["system", "light", "dark", "t3-chat", "grove", "ocean", "ember", "iris", "t3-chat-dark", "t3-grove", "t3-ocean", "t3-ember", "t3-iris"];

/// `publishedThemeDefinitions` over the primary environment's `environmentThemes`.
fn environment_themes(s: &App) -> Vec<Value> {
    let Some(src) = crate::env::source(s, crate::env::LOCAL) else { return vec![] };
    let state = src.state();
    let Some(themes) = state.config.as_ref().and_then(|c| c["environmentThemes"].as_array()) else { return vec![] };
    themes.iter().filter_map(environment_theme).collect()
}

/// `lenientThemeColorOverrides` onto `colors`: known roles with valid colors; the count applied.
fn overlay_colors(colors: &mut Value, overrides: &Value) -> usize {
    let mut applied = 0;
    for (role, css) in overrides.as_object().into_iter().flatten() {
        if colors.get(role).is_some() && css.as_str().and_then(color).is_some() {
            colors[role] = css.clone();
            applied += 1;
        }
    }
    applied
}

/// `environmentThemeDefinition`: a published theme over the stock roles of each appearance. A
/// canvas and accent seed generates its own appearance's roles (`createVividThemeColors`).
pub(crate) fn environment_theme(theme: &Value) -> Option<Value> {
    let id = theme["id"].as_str().filter(|id| !id.is_empty() && !RESERVED.contains(id))?;
    let appearance = theme["appearance"].as_str().filter(|a| matches!(*a, "light" | "dark"))?;
    let other = if appearance == "dark" { "light" } else { "dark" };
    let seed = |key: &str| theme[key].as_str().filter(|css| color(css).is_some());
    let seeded = seed("canvas").is_some() && seed("accent").is_some();
    let stock = palette(&builtins()[0], appearance == "dark");
    let mut colors = match (seed("canvas"), seed("accent")) {
        (Some(canvas), Some(accent)) => super::vivid::colors(stock, appearance == "dark", canvas, accent),
        _ => stock.clone(),
    };
    let own = overlay_colors(&mut colors, &theme["colors"]);
    let mut variant = palette(&builtins()[0], other == "dark").clone();
    let theirs = overlay_colors(&mut variant, &theme["variants"][other]);
    if !seeded && own == 0 && theirs == 0 {
        return None;
    }
    let mut out = json!({"id": id, "label": theme["name"].as_str().unwrap_or(id), "appearance": appearance, "colors": colors});
    if theme["variants"][other].is_object() {
        out["variants"] = Value::Object(serde_json::Map::from_iter([(other.to_owned(), variant)]));
    }
    Some(out)
}

/// `getThemeModes`: the appearances a theme has colors for.
pub(crate) fn modes(theme: &Value) -> Vec<&'static str> {
    let has = |colors: &Value| colors.as_object().is_some_and(|c| !c.is_empty());
    ["light", "dark"]
        .into_iter()
        .filter(|mode| (theme["appearance"] == *mode && has(&theme["colors"])) || has(&theme["variants"][*mode]))
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
    // An open editor previews its palette, switching the app to the appearance being edited.
    let mut t = match editor::preview(s) {
        Some((dark, colors)) => paint_palette(if dark == t.dark { t } else { crate::theme::theme(dark) }, colors),
        None => apply_theme(s, t),
    };
    t.code = super::fonts::code_size(&s.client_settings.values);
    t.px = 1.0 / super::fonts::interface_scale(&s.client_settings.values);
    t.prompt = super::fonts::prompt_style(&s.client_settings.values);
    t.chat_w = crate::chat::details::max_width(super::value(s, "chatWidth").as_str());
    t.math = super::value(s, "mathRenderingEnabled") == true;
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

fn apply_theme(s: &App, t: T) -> T {
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
    paint_palette(t, palette(selected, t.dark))
}

fn paint_palette(mut t: T, p: &Value) -> T {
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
    el().size(56.0, 56.0)
        .padding(2.0)
        .radius(28.0)
        .fill(background)
        .shadow(1.0, 2.0, Color::BLACK.alpha_mul(if dark { 0.18 } else { 0.08 }))
        .child(preview_circle(theme, dark, 52.0))
}

/// `ThemePreviewCircle` without its card border, `d` across; the palette's theme rows use 12.
pub(crate) fn preview_circle(theme: &Value, dark: bool, d: f32) -> El {
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
    let size = d * top("scale", 1.1);
    let inset = (size - d) / 2.0;
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
    el().size(d, d)
        .radius(d / 2.0)
        .clip_rounded(d / 2.0)
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
                .blur(top("blurAt56Px", 3.0) * d / 52.0)
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
    let mut modes = row().w_full().gap(12.0).role("group").label("Appearance mode");
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
            let mut previews = row().w_full().h(68.0).gap(10.0).center().role("group").label("Light and dark theme variants");
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
            let editor_dark = t.dark;
            // The standard theme has no actions; other built-ins and published themes duplicate;
            // saved themes duplicate, edit, export and remove.
            let custom = is_custom(s, &id);
            let mut actions: Vec<El> = Vec::new();
            if item["id"] != builtins()[0]["id"] {
                let template = item.clone();
                actions.push(super::permissions::tip(
                    &format!("theme-duplicate-{id}"),
                    crate::kit::Button::new(format!("Duplicate {label}"))
                        .variant("ghost")
                        .size("icon-xs")
                        .icon(&crate::icons::COPY)
                        .on_press(move |s: &mut App| duplicate(s, template.clone(), editor_dark))
                        .build(t),
                    "Duplicate theme".into(),
                ));
            }
            if custom {
                let template = item.clone();
                actions.push(super::permissions::tip(
                    &format!("theme-edit-{id}"),
                    crate::kit::Button::new(format!("Edit {label}"))
                        .variant("ghost")
                        .size("icon-xs")
                        .icon(&crate::icons::PEN_LINE)
                        .on_press(move |s: &mut App| edit(s, template.clone(), editor_dark))
                        .build(t),
                    "Edit theme".into(),
                ));
                let template = item.clone();
                actions.push(super::permissions::tip(
                    &format!("theme-export-{id}"),
                    crate::kit::Button::new(format!("Export {label}"))
                        .variant("ghost")
                        .size("icon-xs")
                        .icon(&crate::icons::DOWNLOAD)
                        .build(t)
                        .on_press_tx(move |_: &mut App, tx: &mut rune::Tx| export(&template, tx)),
                    "Export theme file".into(),
                ));
                let target = id.clone();
                actions.push(super::permissions::tip(
                    &format!("theme-remove-{id}"),
                    crate::kit::Button::new(format!("Remove {label}"))
                        .variant("ghost-destructive")
                        .size("icon-xs")
                        .icon(&crate::icons::TRASH_2)
                        .on_press(move |s: &mut App| s.ui.settings.theme_remove = Some(target.clone()))
                        .build(t),
                    "Remove theme".into(),
                ));
            }
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
                            .child(row().gap(4.0).center_y().children(actions)),
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
                            .key(super::title_key("Color scheme"))
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
                                .key(super::title_key("Themes"))
                                .font(theme::weight(theme::TEXT_BASE, 600.0))
                                .color(t.c.foreground)
                                .grow(1.0),
                        )
                        .child(
                            crate::kit::Button::new("Create theme")
                                .variant("outline")
                                .size("xs")
                                .icon(&crate::icons::PAINTBRUSH)
                                .on_press(move |s: &mut App| editor::create(s, editor_dark))
                                .build(t),
                        )
                        .child(
                            crate::kit::Button::new("Add theme")
                                .variant("outline")
                                .size("xs")
                                .icon(&crate::icons::PLUS)
                                .on_press(|s: &mut App| {
                                    s.ui.settings.theme_import_open = true;
                                    s.ui.settings.theme_search = Search::default();
                                })
                                .build(t),
                        ),
                )
                .child(cards),
        )
}
fn custom_themes(s: &App) -> Vec<Value> {
    s.client_settings.values.get("customThemes").and_then(Value::as_array).cloned().unwrap_or_default()
}

fn is_custom(s: &App, id: &str) -> bool {
    custom_themes(s).iter().any(|v| v["id"] == id)
}

/// Web's `handleConfirmRemoveTheme`: a half that used the theme falls back to the default.
fn remove_theme(s: &mut App, id: &str) {
    let mut library = custom_themes(s);
    library.retain(|t| t["id"] != id);
    for key in ["themeLight", "themeDark"] {
        if s.client_settings.values.get(key).is_some_and(|v| *v == id) {
            super::update(s, key, json!("t3-code"));
        }
    }
    super::update(s, "customThemes", json!(library));
    if s.client_settings.error.is_some() {
        crate::shell::palette::toast_error(s, "Couldn\u{2019}t remove theme", Some("Try again."));
    }
}

/// Web's theme removal `AlertDialog` for a single theme.
pub fn remove_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(id) = &s.ui.settings.theme_remove else { return };
    let Some(theme) = custom_themes(s).into_iter().find(|v| v["id"] == *id.as_str()) else { return };
    let id = id.clone();
    let cancel = |s: &mut App| s.ui.settings.theme_remove = None;
    crate::kit::Dialog::new("theme-remove", format!("Remove “{}”?", theme["label"].as_str().unwrap_or("")))
        .alert()
        .open(true)
        .description("You can bring it back anytime by importing its JSON file.")
        .child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(crate::kit::Button::new("Cancel").variant("outline").on_press(cancel).build(t))
                .child(
                    crate::kit::Button::new("Remove theme")
                        .variant("destructive")
                        .on_press(move |s: &mut App| {
                            remove_theme(s, &id);
                            s.ui.settings.theme_remove = None;
                        })
                        .build(t),
                ),
        )
        .on_close(cancel)
        .build(t, layers);
}

pub fn scene_remove(s: &mut App) {
    let mut theme = builtins().get(1).cloned().unwrap_or_else(|| builtins()[0].clone());
    theme["id"] = json!("custom-scene");
    theme["label"] = json!("Midnight copy");
    s.client_settings.values.insert("customThemes".into(), json!([theme]));
    s.ui.settings.theme_remove = Some("custom-scene".into());
}

/// Web's `onDuplicate`: a new theme seeded from this one, named “… copy”.
fn duplicate(s: &mut App, theme: Value, dark: bool) {
    let mut copy = json!({
        "id": format!("custom-{}", s5_net::uuid()),
        "label": format!("{} copy", theme["label"].as_str().unwrap_or("Theme")),
        "appearance": theme["appearance"],
        "colors": theme["colors"],
    });
    if theme["variants"].is_object() {
        copy["variants"] = theme["variants"].clone();
    }
    // A managed original opens in the two-color mode, as web seeds from the original.
    if theme["managed"] == true {
        copy["managed"] = json!(true);
    }
    edit(s, copy, dark);
}

/// `serializeThemeFile`: the versioned theme file web exports and imports.
fn theme_file(theme: &Value) -> String {
    let mut file = json!({
        "version": 1,
        "id": theme["id"],
        "name": theme["label"],
        "appearance": theme["appearance"],
        "colors": theme["colors"],
    });
    for key in ["variants", "collection"] {
        if theme[key].is_object() {
            file[key] = theme[key].clone();
        }
    }
    if theme["managed"] == true {
        file["managed"] = json!(true);
    }
    format!("{}\n", serde_json::to_string_pretty(&file).unwrap_or_default())
}

/// Web downloads `<id>.json`; the desktop asks where to save it.
fn export(theme: &Value, tx: &mut rune::Tx) {
    let contents = theme_file(theme);
    let name = format!("{}.json", theme["id"].as_str().unwrap_or("theme"));
    let options = rune::FileDialogOptions { title: "Export theme".into(), file_name: Some(name), ..Default::default() }.filter("JSON", &["json"]);
    tx.save_file(options, move |s: &mut App, picked: rune::FileDialogResult, _| {
        if let Ok(Some(path)) = picked
            && let Err(e) = std::fs::write(&path, &contents)
        {
            crate::shell::palette::toast(s, "Could not export theme", Some(&e.to_string()));
        }
    });
}

/// `toggleThemeEditorForTheme`: closes an open editor, else opens one seeded from the theme
/// the current appearance uses.
pub fn toggle_editor(s: &mut App, dark: bool) {
    if s.ui.settings.theme_editor.take().is_some() {
        return;
    }
    let id = s.client_settings.values.get(if dark { "themeDark" } else { "themeLight" }).and_then(Value::as_str).unwrap_or("t3-code").to_string();
    let theme = library(s).into_iter().find(|v| v["id"] == id.as_str()).unwrap_or_else(|| builtins()[0].clone());
    edit(s, theme, dark);
}

/// `getThemeModes`: the theme's own appearance, then any other mode its variants cover.
fn imported_modes(theme: &Value) -> Vec<&'static str> {
    let mut modes: Vec<&'static str> = ["light", "dark"].into_iter().filter(|m| theme["appearance"] == *m).collect();
    for mode in ["light", "dark"] {
        if theme["variants"].get(mode).is_some() && !modes.contains(&mode) {
            modes.push(mode);
        }
    }
    modes
}

/// Web's `parseThemeImport`: a VS Code color theme converts, anything else is a theme file.
pub fn import_theme(raw: &str) -> Result<Value, String> {
    if let Ok(parsed) = vscode::jsonc(raw)
        && vscode::is_theme(&parsed)
    {
        return vscode::convert(&parsed, |dark| palette(&builtins()[0], dark).clone());
    }
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
    // `parseThemeFile`: the file's id, else one from its name, so a theme imported again matches.
    let id = theme["id"].as_str().map_or_else(|| vscode::theme_id(&label), str::to_owned);
    let valid = id.len() <= 48 && id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit()) && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid {
        return Err("Theme ids may only contain lowercase letters, numbers, and hyphens.".into());
    }
    if RESERVED.contains(&id.as_str()) {
        return Err(format!("The theme id \"{id}\" is reserved."));
    }
    theme["id"] = json!(id);
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
const MAX_THEME_FILE_BYTES: u64 = 256 * 1024;

fn close_import(s: &mut App) {
    s.ui.settings.theme_import_open = false;
    s.ui.settings.theme_import_error = None;
    s.ui.settings.theme_conflict = None;
    s.ui.settings.theme_import.clear();
    s.ui.settings.theme_search = Search::default();
}

fn add_imported(s: &mut App) {
    match import_theme(&s.ui.settings.theme_import.text) {
        Ok(theme) if custom_themes(s).iter().any(|t| t["id"] == theme["id"]) => {
            s.ui.settings.theme_import_error = None;
            s.ui.settings.theme_conflict = Some(theme);
        }
        Ok(theme) => {
            let mut library = custom_themes(s);
            let (id, label) = (theme["id"].clone(), theme["label"].as_str().unwrap_or("Theme").to_owned());
            let modes = imported_modes(&theme);
            library.push(theme);
            super::update(s, "customThemes", json!(library));
            // `onImported`: a one-mode theme becomes that half; one with both takes both.
            for mode in &modes {
                super::update(s, if *mode == "dark" { "themeDark" } else { "themeLight" }, id.clone());
            }
            let description = match modes.as_slice() {
                [mode] => format!("It\u{2019}s now your {mode} theme."),
                _ => "It\u{2019}s now active.".to_owned(),
            };
            crate::shell::palette::toast(s, &format!("{label} added"), Some(&description));
            close_import(s);
        }
        Err(error) => s.ui.settings.theme_import_error = Some(error),
    }
}

/// Web `versionedCopy`: the theme as "Name (n)", the first n whose id is free.
fn versioned_copy(library: &[Value], theme: &Value) -> Option<Value> {
    let label = theme["label"].as_str().unwrap_or("Theme");
    (1..100).find_map(|n| {
        let suffix = format!(" ({n})");
        let name = format!("{}{suffix}", label.chars().take(48 - suffix.len()).collect::<String>());
        let id = vscode::theme_id(&name);
        (!library.iter().any(|t| t["id"] == id.as_str())).then(|| {
            let mut copy = theme.clone();
            (copy["id"], copy["label"]) = (json!(id), json!(name));
            copy
        })
    })
}

/// Web `resolveConflicts`: replace the installed theme (keeping its collection) or keep both.
fn resolve_conflict(s: &mut App, update: bool) {
    let Some(theme) = s.ui.settings.theme_conflict.take() else { return };
    let mut library = custom_themes(s);
    let resolved = if update {
        let at = library.iter().position(|t| t["id"] == theme["id"]);
        at.map(|at| {
            let mut next = theme.clone();
            if library[at]["collection"].is_object() {
                next["collection"] = library[at]["collection"].clone();
            }
            library[at] = next.clone();
            next
        })
    } else {
        versioned_copy(&library, &theme).inspect(|copy| library.push(copy.clone()))
    };
    let Some(resolved) = resolved else {
        let label = theme["label"].as_str().unwrap_or("Theme");
        s.ui.settings.theme_import_error = Some(format!("{label}: Too many copies of \"{label}\"."));
        return;
    };
    super::update(s, "customThemes", json!(library));
    let label = resolved["label"].as_str().unwrap_or("Theme");
    crate::shell::palette::toast(s, &format!("{label} {}", if update { "updated" } else { "added" }), Some(label));
    close_import(s);
}

/// Scenes: importing a theme a custom theme already has the id of.
pub fn scene_conflict(s: &mut App) {
    let theme = json!({"id": "aurora", "label": "Aurora", "appearance": "dark", "colors": palette(&builtins()[0], true).clone()});
    s.client_settings.values.insert("customThemes".into(), json!([theme.clone()]));
    s.ui.settings.theme_conflict = Some(theme);
}

/// ThemeImportDialog's `formatByteSize`.
fn byte_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1024 * 1024 => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
        b if b >= 1024 => format!("{} KB", (b as f64 / 1024.0).round()),
        b => format!("{b} bytes"),
    }
}

/// A picked theme file's JSON, refusing what no theme is as large as (`describeOversizedThemeFile`).
fn read_theme_file(path: &std::path::Path) -> Result<String, String> {
    let unreadable = || "Could not read that file. Paste the JSON below instead.".to_owned();
    let size = std::fs::metadata(path).map_err(|_| unreadable())?.len();
    if size > MAX_THEME_FILE_BYTES {
        return Err(format!(
            "That file is {}. Theme files are only a few KB, so this one was not read (limit {}).",
            byte_size(size),
            byte_size(MAX_THEME_FILE_BYTES)
        ));
    }
    std::fs::read_to_string(path).map_err(|_| unreadable())
}

/// Electron's `pickThemeFiles`: a JSON picker that starts in VS Code's extensions folder, where
/// installed themes live. The file fills the JSON field.
fn choose_theme_file(tx: &mut rune::Tx) {
    let extensions = std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".vscode").join("extensions")).filter(|dir| dir.is_dir());
    let options = rune::FileDialogOptions { title: "Choose a theme file".into(), directory: extensions, ..Default::default() }.filter("JSON", &["json"]);
    tx.open_file(options, |s: &mut App, picked: rune::FileDialogResult, _| {
        let Ok(Some(path)) = picked else { return };
        match read_theme_file(&path) {
            Ok(contents) => {
                s.ui.settings.theme_import = rune::TextEdit::new(&contents);
                s.ui.settings.theme_import_error = None;
            }
            Err(error) => s.ui.settings.theme_import_error = Some(error),
        }
    });
}

/// Web's `ThemeImportDialog`: community search above, a theme file or pasted JSON below.
/// Web's conflict step: what is already installed, then update it or keep both.
fn conflict_body(theme: &Value, t: &T) -> El {
    let installed = col()
        .w_full()
        .gap(4.0)
        .padding(12.0)
        .radius(12.0)
        .border(1.0, t.c.border.alpha_mul(0.7))
        .fill(t.c.muted.alpha_mul(0.2))
        .child(text("Already installed").font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(text(theme["label"].as_str().unwrap_or("Theme").to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground));
    let actions = row()
        .gap(8.0)
        .flex_wrap()
        .child(crate::kit::Button::new("Update existing").size("sm").on_press(|s: &mut App| resolve_conflict(s, true)).build(t))
        .child(crate::kit::Button::new("Keep both").variant("outline").size("sm").on_press(|s: &mut App| resolve_conflict(s, false)).build(t))
        .child(crate::kit::Button::new("Back").variant("ghost").size("sm").on_press(|s: &mut App| s.ui.settings.theme_conflict = None).build(t))
        .child(crate::kit::Button::new("Cancel").variant("ghost").size("sm").on_press(close_import).build(t));
    col().w_full().gap(12.0).child(installed).child(actions)
}

pub fn import_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    if !s.ui.settings.theme_import_open {
        return;
    }
    let mut menus = Vec::new();
    let search = open_vsx::section(s, t, &mut menus);
    let divider = || el().h(1.0).grow(1.0).fill(t.c.border);
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(search)
        .child(
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(divider())
                .child(text("OR IMPORT A FILE").font(theme::TEXT_2XS).color(t.c.muted_foreground))
                .child(divider()),
        )
        .child(
            row()
                .w_full()
                .gap(12.0)
                .padding(12.0)
                .radius(12.0)
                .border(1.0, t.c.border.alpha_mul(0.8))
                .border_dash(4.0, 3.0)
                .fill(t.c.muted.alpha_mul(0.2))
                .center_y()
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .child(text("Theme file").font(theme::SM_MEDIUM).color(t.c.foreground))
                        .child(text("T3 Code or VS Code .json files").font(theme::TEXT_XS).color(t.c.muted_foreground).truncate()),
                )
                .child(
                    crate::kit::Button::new("Choose file")
                        .variant("outline")
                        .size("sm")
                        .icon(&crate::icons::UPLOAD)
                        .build(t)
                        .on_press_tx(|_: &mut App, tx: &mut rune::Tx| choose_theme_file(tx)),
                ),
        )
        .child(
            col()
                .w_full()
                .gap(8.0)
                .child(text("Theme JSON").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(
                    rune::text_input(&s.ui.settings.theme_import, theme::MONO_XS)
                        .w_full()
                        .h(220.0)
                        .padding(12.0)
                        .wrap()
                        .border(1.0, t.c.border)
                        .radius(8.0)
                        .color(t.c.foreground)
                        .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
                        .focus_ring(3.0, -3.0, t.c.ring.alpha_mul(0.24))
                        .on_edit(|s: &mut App| &mut s.ui.settings.theme_import),
                ),
        )
        .child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(crate::kit::Button::new("Cancel").variant("ghost").on_press(close_import).build(t))
                .child(
                    crate::kit::Button::new("Add theme")
                        .icon(&crate::icons::PLUS)
                        .disabled(s.ui.settings.theme_import.text.trim().is_empty())
                        .on_press(add_imported)
                        .build(t),
                ),
        );
    if let Some(conflict) = &s.ui.settings.theme_conflict {
        body = conflict_body(conflict, t);
    }
    if let Some(error) = &s.ui.settings.theme_import_error {
        body = body.child(text(error.clone()).role("alert").font(theme::TEXT_XS.lh(18.0)).color(t.c.destructive).wrap());
    }
    crate::kit::Dialog::new("import-theme", "Add a theme")
        .open(true)
        .width(768.0)
        .scroll_all()
        .on_close(|s: &mut App| {
            if !open_vsx::escape(s) {
                close_import(s);
            }
        })
        .child(body)
        .build(t, layers);
    layers.extend(menus);
    open_vsx::confirm_dialog(s, t, layers);
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
    fn published_themes_follow_the_web_filter_and_keep_their_appearances() {
        let mono = environment_theme(&json!({"id":"dusk","name":"Dusk","appearance":"dark","colors":{"accent":"#123456"}})).unwrap();
        assert_eq!((mono["label"].as_str(), modes(&mono)), (Some("Dusk"), vec!["dark"]));
        assert_eq!(mono["colors"]["accent"], "#123456");
        let both = environment_theme(&json!({"id":"dawn","name":"Dawn","appearance":"light","canvas":"#fafafa","accent":"#aa3355","colors":{"border":"#010203"},"variants":{"dark":{}}})).unwrap();
        assert_eq!(modes(&both), vec!["light", "dark"]);
        // The seed generates the light roles (web's vivid generator), overrides land on top, and the
        // dark variant stays the stock dark palette.
        let stock = palette(&builtins()[0], false);
        assert!(both["colors"]["canvas"].as_str().unwrap().starts_with("oklch(0.98"));
        assert_ne!(both["colors"]["sidebar"], stock["sidebar"]);
        assert_ne!(both["colors"]["text"], stock["text"]);
        assert_eq!(both["colors"]["border"], "#010203");
        assert_eq!(&both["variants"]["dark"], palette(&builtins()[0], true));
        for rejected in [
            json!({"id":"grove","name":"Taken","appearance":"dark","colors":{"accent":"#123456"}}),
            json!({"id":"empty","name":"Empty","appearance":"dark","colors":{"accent":"url(x)"}}),
            json!({"id":"half","name":"Half","appearance":"dark","canvas":"#000000"}),
        ] {
            assert!(environment_theme(&rejected).is_none(), "FAILED: {rejected}");
        }
        assert_eq!(modes(&builtins()[0]), vec!["light", "dark"]);
    }

    #[test]
    fn theme_import_validates_colors_before_filling_missing_roles() {
        let theme = import_theme(
            r##"{"name":"Example","appearance":"dark","colors":{"accent":"#123456"}}"##,
        )
        .unwrap();
        assert_eq!(theme["colors"]["accent"], "#123456");
        assert!(theme["colors"].get("canvas").is_some());
        assert_eq!(imported_modes(&theme), ["dark"], "a one-mode theme becomes that half");
        let both = json!({"appearance": "light", "variants": {"dark": {}}});
        assert_eq!(imported_modes(&both), ["light", "dark"]);
        assert!(
            import_theme(
                r#"{"name":"Bad","appearance":"dark","colors":{"accent":"url(example)"}}"#
            )
            .is_err()
        );
        let vscode = import_theme(r##"{"name": "night-owl", "type": "dark", "colors": {"editor.background": "#011627"}} // VS Code"##).unwrap();
        assert_eq!(vscode["id"], "night-owl", "a VS Code theme's id follows its name");
        assert_eq!(theme["id"], "example", "a file without an id takes one from its name");
        assert!(import_theme(r##"{"name":"Light","appearance":"light","colors":{}}"##).unwrap_err().contains("reserved"));
        assert!(import_theme(r##"{"id":"Bad Id","name":"X","appearance":"light","colors":{}}"##).is_err());
        assert_eq!((vscode["label"].as_str(), vscode["appearance"].as_str()), (Some("Night Owl"), Some("dark")));
    }

    #[test]
    fn an_exported_file_reads_back_and_a_duplicate_is_a_new_theme() {
        let theme = json!({"id": "custom-a", "label": "Aurora", "appearance": "dark", "colors": {"accent": "#123456"}, "variants": {"light": {"accent": "#654321"}}, "collection": {"id": "open-vsx:x", "label": "X"}, "managed": true});
        let file: Value = serde_json::from_str(&theme_file(&theme)).unwrap();
        assert_eq!(file, json!({"version": 1, "id": "custom-a", "name": "Aurora", "appearance": "dark", "colors": {"accent": "#123456"}, "variants": {"light": {"accent": "#654321"}}, "collection": {"id": "open-vsx:x", "label": "X"}, "managed": true}));
        let back = import_theme(&theme_file(&theme)).unwrap();
        assert_eq!((back["label"].as_str(), back["colors"]["accent"].as_str()), (Some("Aurora"), Some("#123456")));
        assert_eq!(imported_modes(&back), ["dark", "light"]);
        let mut app = App::new();
        duplicate(&mut app, theme, true);
        let copy = app.ui.settings.theme_editor.clone().unwrap();
        assert_eq!(copy["label"], "Aurora copy");
        assert_ne!(copy["id"], "custom-a");
        assert!(copy.get("collection").is_none(), "a copy leaves the collection");
        assert_eq!(app.ui.settings.form["form.themeEditor"]["advanced"], false, "a managed original opens with two colors");
        assert_eq!(copy["variants"]["light"]["accent"], "#654321");
    }

    #[test]
    fn a_theme_imported_again_asks_then_updates_or_keeps_both() {
        let mut s = App::new();
        // An unreadable-file error keeps `save` from writing the real settings file.
        s.client_settings.error = Some("tests do not write settings".into());
        s.client_settings.values.insert("customThemes".into(), json!([{"id": "aurora", "label": "Aurora", "appearance": "dark", "colors": {}, "collection": {"id": "c", "label": "C"}}]));
        s.ui.settings.theme_import = rune::TextEdit::new(r##"{"name":"Aurora","appearance":"dark","colors":{"accent":"#123456"}}"##);
        add_imported(&mut s);
        assert_eq!(s.ui.settings.theme_conflict.as_ref().map(|t| t["id"].clone()), Some(json!("aurora")), "nothing installs yet");
        assert_eq!(custom_themes(&s).len(), 1);
        resolve_conflict(&mut s, false);
        let ids: Vec<Value> = custom_themes(&s).iter().map(|t| t["id"].clone()).collect();
        assert_eq!(ids, [json!("aurora"), json!("aurora-1")]);
        assert_eq!(custom_themes(&s)[1]["label"], "Aurora (1)");
        s.ui.settings.theme_import = rune::TextEdit::new(r##"{"name":"Aurora","appearance":"dark","colors":{"accent":"#654321"}}"##);
        add_imported(&mut s);
        resolve_conflict(&mut s, true);
        let updated = &custom_themes(&s)[0];
        assert_eq!((updated["colors"]["accent"].as_str(), updated["collection"]["id"].as_str()), (Some("#654321"), Some("c")), "an update keeps the collection");
        assert_eq!(custom_themes(&s).len(), 2);
    }

    #[test]
    fn a_picked_theme_file_is_read_only_under_256_kb() {
        let dir = std::env::temp_dir().join(format!("s5-theme-file-{}", s5_net::uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        let small = dir.join("small.json");
        std::fs::write(&small, r#"{"name":"Small"}"#).unwrap();
        assert_eq!(read_theme_file(&small).as_deref(), Ok(r#"{"name":"Small"}"#));
        let large = dir.join("large.json");
        std::fs::write(&large, vec![b' '; 300 * 1024]).unwrap();
        assert_eq!(
            read_theme_file(&large),
            Err("That file is 300 KB. Theme files are only a few KB, so this one was not read (limit 256 KB).".into())
        );
        assert!(read_theme_file(&dir.join("missing.json")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
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
