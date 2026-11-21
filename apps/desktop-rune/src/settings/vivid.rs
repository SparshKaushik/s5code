//! Port of web's `createVividThemeColors` (apps/web/src/themePalette.ts): the full role set a
//! published theme's canvas and accent seed generates for that seed's own appearance.

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rgb {
    r: f64,
    g: f64,
    b: f64,
}

#[derive(Clone, Copy, Debug)]
struct Lch {
    l: f64,
    c: f64,
    h: f64,
}

const LIGHT_FOREGROUND: Rgb = Rgb { r: 255.0, g: 250.0, b: 255.0 };
const DARK_FOREGROUND: Rgb = Rgb { r: 36.0, g: 21.0, b: 35.0 };
const WHITE: Rgb = Rgb { r: 255.0, g: 255.0, b: 255.0 };
const BLACK: Rgb = Rgb { r: 0.0, g: 0.0, b: 0.0 };

fn parse(css: &str) -> Option<Lch> {
    let css = css.trim();
    if let Some(hex) = css.strip_prefix('#') {
        let digits: Vec<u8> = match hex.len() {
            3 | 4 => hex.chars().map(|c| c.to_digit(16).map(|d| (d * 17) as u8)).collect::<Option<_>>()?,
            6 | 8 => (0..hex.len())
                .step_by(2)
                .map(|i| hex.get(i..i + 2).and_then(|p| u8::from_str_radix(p, 16).ok()))
                .collect::<Option<_>>()?,
            _ => return None,
        };
        return Some(to_lch(Rgb { r: digits[0] as f64, g: digits[1] as f64, b: digits[2] as f64 }));
    }
    let raw = css.strip_prefix("oklch(")?.strip_suffix(')')?;
    let parts: Vec<&str> = raw.split('/').next()?.split_whitespace().collect();
    let [l, c, h] = parts[..] else { return None };
    let number = |s: &str, percent: f64| -> Option<f64> {
        match s.strip_suffix('%') {
            Some(p) => p.parse::<f64>().ok().map(|v| v * percent / 100.0),
            None => s.trim_end_matches("deg").parse::<f64>().ok(),
        }
    };
    let (l, c, h) = (number(l, 1.0)?, number(c, 0.4)?, number(h, 1.0)?);
    [l, c, h].iter().all(|v| v.is_finite()).then(|| Lch { l: l.clamp(0.0, 1.0), c: c.max(0.0), h })
}

fn rgb_of(css: &str, fallback: Rgb) -> Rgb {
    parse(css).map_or(fallback, |c| to_rgb(c, true))
}

/// `formatThemeColorNumber`: fixed precision with trailing zeros dropped.
fn number(value: f64, precision: usize) -> String {
    let value = if value.abs() < 10f64.powi(-(precision as i32)) / 2.0 { 0.0 } else { value };
    let text = format!("{value:.precision$}");
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_owned() } else { text }
}

fn format(c: Lch) -> String {
    let hue = if c.c < 0.0000005 { 0.0 } else { c.h.rem_euclid(360.0) };
    format!("oklch({} {} {})", number(c.l, 6), number(c.c, 6), number(hue, 3))
}

fn rgb_css(rgb: Rgb) -> String {
    format(to_lch(rgb))
}

fn lch_css(c: Lch) -> String {
    format(gamut(c))
}

fn mix(base: Rgb, overlay: Rgb, amount: f64) -> Rgb {
    Rgb {
        r: base.r + (overlay.r - base.r) * amount,
        g: base.g + (overlay.g - base.g) * amount,
        b: base.b + (overlay.b - base.b) * amount,
    }
}

fn luminance(c: Rgb) -> f64 {
    let lin = |ch: f64| {
        let n = ch / 255.0;
        if n <= 0.03928 { n / 12.92 } else { ((n + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn to_linear(ch: f64) -> f64 {
    let c = ch / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(ch: f64, round: bool) -> f64 {
    let c = if ch <= 0.0031308 { ch * 12.92 } else { 1.055 * ch.powf(1.0 / 2.4) - 0.055 };
    let v = c.clamp(0.0, 1.0) * 255.0;
    if round { v.round() } else { v }
}

fn to_lch(c: Rgb) -> Lch {
    let (r, g, b) = (to_linear(c.r), to_linear(c.g), to_linear(c.b));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    let big_l = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
    let a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
    let bb = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
    Lch { l: big_l, c: a.hypot(bb), h: bb.atan2(a).to_degrees() }
}

fn unclamped(c: Lch) -> [f64; 3] {
    let hr = c.h.to_radians();
    let (a, bb) = (c.c * hr.cos(), c.c * hr.sin());
    let l = (c.l + 0.3963377774 * a + 0.2158037573 * bb).powi(3);
    let m = (c.l - 0.1055613458 * a - 0.0638541728 * bb).powi(3);
    let s = (c.l - 0.0894841775 * a - 1.291485548 * bb).powi(3);
    [
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
    ]
}

/// `mapThemeOklchToSrgbGamut`: the largest in-gamut chroma at the same lightness and hue.
fn gamut(color: Lch) -> Lch {
    let fits = |c: f64| unclamped(Lch { c, ..color }).iter().all(|ch| (-0.0001..=1.0001).contains(ch));
    if fits(color.c) {
        return color;
    }
    let (mut low, mut high) = (0.0, color.c);
    let resolution: f64 = 0.000001;
    let steps = ((color.c.max(resolution).log2() - resolution.log2()).ceil() as usize).max(1);
    for _ in 0..steps {
        let mid = (low + high) / 2.0;
        if fits(mid) { low = mid } else { high = mid }
    }
    Lch { c: low, ..color }
}

fn to_rgb(color: Lch, round: bool) -> Rgb {
    let [r, g, b] = unclamped(gamut(color));
    Rgb { r: to_srgb(r, round), g: to_srgb(g, round), b: to_srgb(b, round) }
}

/// `solveOklchLightness`: the nearest lightness (only) that reaches `min` contrast on every surface.
fn solve(base: Lch, against: &[Rgb], min: f64, lighter: bool) -> Lch {
    let contrast_of = |c: Lch| against.iter().map(|s| contrast(to_rgb(c, true), *s)).fold(f64::INFINITY, f64::min);
    if contrast_of(base) >= min {
        return base;
    }
    let (mut low, mut high) = if lighter { (base.l, 1.0) } else { (0.0, base.l) };
    for _ in 0..18 {
        let mid = (low + high) / 2.0;
        let passes = contrast_of(Lch { l: mid, ..base }) >= min;
        if passes == lighter { high = mid } else { low = mid }
    }
    Lch { l: if lighter { high } else { low }, ..base }
}

fn readable_foreground(bg: Rgb) -> Rgb {
    let (light, dark) = (contrast(bg, LIGHT_FOREGROUND), contrast(bg, DARK_FOREGROUND));
    if light.max(dark) >= 4.5 {
        return if light >= dark { LIGHT_FOREGROUND } else { DARK_FOREGROUND };
    }
    if contrast(bg, WHITE) >= contrast(bg, BLACK) { WHITE } else { BLACK }
}

fn readable_text(bg: Rgb, fg: Rgb, amount: f64, min: f64) -> Rgb {
    let softened = mix(fg, bg, amount);
    if contrast(softened, bg) >= min {
        return softened;
    }
    let (mut readable, mut lower, mut upper) = (fg, 0.0, amount);
    for _ in 0..12 {
        let at = (lower + upper) / 2.0;
        let candidate = mix(fg, bg, at);
        if contrast(candidate, bg) >= min {
            readable = candidate;
            lower = at;
        } else {
            upper = at;
        }
    }
    readable
}

fn muted_text(bg: Rgb, fg: Rgb) -> Rgb {
    readable_text(bg, fg, 1.0, if luminance(bg) < 0.179 { 5.082 } else { 4.705 })
}

fn status(canvas: Rgb) -> [(&'static str, String); 6] {
    let dark = luminance(canvas) < 0.179;
    let [error, error_fg, warning, warning_fg] = if dark {
        ["#fb414a", "#ff6467", "#fe9a00", "#ffb900"]
    } else {
        ["#fb2c36", "#c10007", "#fe9a00", "#bb4d00"]
    };
    let surface = |css: &str| mix(canvas, rgb_of(css, canvas), if dark { 0.16 } else { 0.08 });
    let readable = |css: &str, on: Rgb| lch_css(solve(to_lch(rgb_of(css, canvas)), &[on], 4.6, dark));
    let canonical = |css: &str| parse(css).map(format).unwrap_or_default();
    let (error_surface, warning_surface) = (surface(error), surface(warning));
    [
        ("error", canonical(error)),
        ("errorForeground", readable(error_fg, error_surface)),
        ("errorSurface", rgb_css(error_surface)),
        ("warning", canonical(warning)),
        ("warningForeground", readable(warning_fg, warning_surface)),
        ("warningSurface", rgb_css(warning_surface)),
    ]
}

/// `createVividThemeColors` over `defaults`, the stock roles of `appearance`.
pub(crate) fn colors(defaults: &Value, dark_appearance: bool, canvas_css: &str, accent_css: &str) -> Value {
    let fallback = if dark_appearance { Rgb { r: 24.0, g: 15.0, b: 27.0 } } else { Rgb { r: 250.0, g: 245.0, b: 250.0 } };
    let canvas_rgb = rgb_of(canvas_css, fallback);
    let accent_rgb = rgb_of(accent_css, Rgb { r: 168.0, g: 67.0, b: 112.0 });
    let canvas = to_lch(canvas_rgb);
    let accent = to_lch(accent_rgb);
    let dark = luminance(canvas_rgb) < 0.179;
    let pick = |d: f64, l: f64| if dark { d } else { l };
    let hue = if accent.c < 0.02 { canvas.h } else { accent.h };
    let tint = (accent.c * 0.22).clamp(0.008, 0.045);
    let step = pick(1.0, -1.0);
    let at = |delta: f64, c: f64| Lch { l: (canvas.l + step * delta).clamp(0.05, 0.98), c, h: hue };
    let ac = |k: f64, cap: f64| (accent.c * k).min(cap);

    let text_base = Lch { l: pick(0.95, 0.2), c: ac(0.25, 0.035), h: hue };
    let text_rgb = to_rgb(solve(text_base, &[canvas_rgb], 7.0, dark), true);
    let muted_rgb_text = muted_text(canvas_rgb, text_rgb);

    let action = Lch {
        l: (accent.l + pick(0.06, -0.02)).clamp(0.35, 0.85),
        c: (accent.c * 0.9).max(0.06),
        h: (hue + 50.0) % 360.0,
    };
    let action_rgb = to_rgb(action, true);

    let sidebar = at(0.045, tint * 1.4);
    let sidebar_rgb = to_rgb(sidebar, true);
    let surface_raised = at(0.05, tint);
    let secondary = at(pick(0.1, 0.06), ac(0.5, 0.09));
    let muted = at(pick(0.06, 0.04), ac(0.35, 0.06));
    let accent_surface = at(pick(0.13, 0.08), ac(0.55, 0.11));
    let message_surface = at(pick(0.16, 0.1), ac(0.6, 0.13));
    let update_surface = at(pick(0.14, 0.09), ac(0.55, 0.12));
    let search = to_rgb(at(0.22, ac(1.0, 0.12)), true);
    let search_active = to_rgb(at(0.4, ac(1.0, 0.18)), true);

    let on = |surface: Lch| lch_css(solve(text_base, &[to_rgb(surface, true)], 4.6, dark));
    let text = rgb_css(text_rgb);
    let muted_text_css = rgb_css(muted_rgb_text);
    let accent_css = rgb_css(accent_rgb);
    let canvas_out = rgb_css(canvas_rgb);

    let roles: Vec<(&str, String)> = vec![
        ("canvas", canvas_out.clone()),
        ("chrome", canvas_out.clone()),
        ("toolbar", canvas_out.clone()),
        ("toolbarForeground", text.clone()),
        ("toolbarBorder", lch_css(at(pick(0.14, 0.1), ac(0.4, 0.08)))),
        ("toolbarControl", lch_css(at(pick(0.09, 0.05), tint * 1.3))),
        ("toolbarControlForeground", text.clone()),
        ("toolbarControlHover", lch_css(at(pick(0.14, 0.09), tint * 1.6))),
        ("surface", lch_css(at(0.015, tint))),
        ("surfaceRaised", lch_css(surface_raised)),
        ("surfaceOverlay", lch_css(at(0.075, tint))),
        ("text", text.clone()),
        ("textMuted", muted_text_css.clone()),
        ("border", lch_css(at(pick(0.16, 0.12), ac(0.35, 0.07)))),
        ("input", lch_css(at(pick(0.21, 0.16), ac(0.4, 0.08)))),
        ("focus", accent_css.clone()),
        ("accent", accent_css.clone()),
        ("accentForeground", rgb_css(readable_foreground(accent_rgb))),
        ("secondary", lch_css(secondary)),
        ("secondaryForeground", on(secondary)),
        ("muted", lch_css(muted)),
        ("mutedForeground", rgb_css(readable_text(to_rgb(muted, true), text_rgb, 1.0, 4.6))),
        ("placeholder", rgb_css(readable_text(to_rgb(surface_raised, true), text_rgb, 1.0, 4.6))),
        ("secondaryLabel", muted_text_css.clone()),
        ("iconMuted", muted_text_css.clone()),
        ("update", accent_css.clone()),
        ("updateForeground", on(update_surface)),
        ("updateSurface", lch_css(update_surface)),
        ("accentSurface", lch_css(accent_surface)),
        ("accentSurfaceForeground", on(accent_surface)),
        ("messageSurface", lch_css(message_surface)),
        ("messageForeground", on(message_surface)),
        ("messageAction", rgb_css(action_rgb)),
        ("messageActionForeground", rgb_css(readable_foreground(action_rgb))),
        ("messageActionHover", lch_css(Lch { l: action.l + pick(0.06, -0.06), ..action })),
        ("codeBackground", lch_css(at(0.035, tint * 0.8))),
        ("codeForeground", text.clone()),
        ("searchMatchBackground", rgb_css(search)),
        ("searchMatchForeground", rgb_css(readable_foreground(search))),
        ("searchMatchActiveBackground", rgb_css(search_active)),
        ("searchMatchActiveForeground", rgb_css(readable_foreground(search_active))),
        ("sidebar", lch_css(sidebar)),
        ("sidebarForeground", on(sidebar)),
        ("sidebarMutedForeground", rgb_css(muted_text(sidebar_rgb, text_rgb))),
        ("sidebarControlSurface", lch_css(at(pick(0.1, 0.07), tint * 1.5))),
        ("sidebarRowHover", lch_css(at(pick(0.08, 0.06), ac(0.45, 0.08)))),
        ("sidebarRowActive", lch_css(at(pick(0.12, 0.09), ac(0.55, 0.1)))),
        ("sidebarRowSelected", lch_css(at(pick(0.14, 0.1), ac(0.6, 0.11)))),
        ("sidebarBorder", lch_css(at(pick(0.17, 0.12), ac(0.4, 0.08)))),
        ("terminalBackground", canvas_out.clone()),
        ("terminalForeground", text),
        ("terminalCursor", accent_css),
        ("terminalSelection", lch_css(at(pick(0.18, 0.12), ac(0.55, 0.12)))),
        ("terminalScrollbar", lch_css(at(pick(0.22, 0.16), tint))),
        ("terminalScrollbarHover", lch_css(at(pick(0.3, 0.22), tint))),
    ];
    let mut out = defaults.clone();
    for (role, css) in status(canvas_rgb).into_iter().chain(roles) {
        out[role] = Value::String(css);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(css: &Value) -> Rgb {
        to_rgb(parse(css.as_str().unwrap()).unwrap(), true)
    }

    #[test]
    fn hex_reads_as_culori_oklch_and_formats_like_web() {
        // culori: converter("oklch")(parse("#3b82f6")) → l 0.6231, c 0.1880, h 259.81
        let c = parse("#3b82f6").unwrap();
        assert!((c.l - 0.6231).abs() < 0.0005 && (c.c - 0.188).abs() < 0.0005 && (c.h - 259.81).abs() < 0.05);
        assert_eq!(format(Lch { l: 0.5, c: 0.0, h: 120.0 }), "oklch(0.5 0 0)");
        assert_eq!(format(Lch { l: 0.25, c: 0.1, h: -30.0 }), "oklch(0.25 0.1 330)");
        assert_eq!(rgb_of("oklch(0.62 0.2 280)", BLACK), to_rgb(Lch { l: 0.62, c: 0.2, h: 280.0 }, true));
    }

    #[test]
    fn a_seed_generates_readable_roles_for_its_own_appearance() {
        let defaults = serde_json::json!({"canvas": "#000", "fontKeep": "x"});
        for (dark, canvas, accent) in [(true, "#101418", "#3b82f6"), (false, "#fbf7ef", "#c2410c")] {
            let out = colors(&defaults, dark, canvas, accent);
            let surface = rgb(&out["canvas"]);
            assert_eq!(surface, rgb_of(canvas, BLACK));
            assert!(contrast(rgb(&out["text"]), surface) >= 7.0, "{out}");
            assert!(contrast(rgb(&out["textMuted"]), surface) >= 4.7);
            assert!(contrast(rgb(&out["messageForeground"]), rgb(&out["messageSurface"])) >= 4.5);
            assert!(contrast(rgb(&out["errorForeground"]), rgb(&out["errorSurface"])) >= 4.5);
            assert_ne!(out["sidebar"], out["canvas"]);
            assert_eq!(out["accent"], out["focus"]);
            assert_eq!(luminance(rgb(&out["text"])) > luminance(surface), dark);
            assert_eq!(out["fontKeep"], "x");
        }
    }
}
