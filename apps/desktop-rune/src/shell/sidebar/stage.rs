//! Environment identification in the sidebar header (web `SidebarStageBackdrop`): Dev and
//! Nightly builds draw blueprint or night-sky artwork behind the header, or a version pill, as
//! Settings > Appearance's "Environment identification" chooses.

use std::cell::RefCell;

use rune::{El, ImageCache, el};

use crate::App;
use crate::theme::{AlphaMul, T};

pub const HEIGHT: f32 = 80.0;
const MODE: &str = "environmentIdentificationMode";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Variant {
    Nightly,
    Dev,
}

/// The art's rasterized image, keyed by variant, appearance, width and scale.
#[derive(Default)]
pub struct Art {
    cache: RefCell<Option<ImageCache>>,
    image: RefCell<Option<(String, rune::Image)>>,
}

/// `NIGHTLY_SERVER_VERSION_PATTERN`.
fn nightly_version(version: &str) -> bool {
    let Some((base, rest)) = version.split_once('-') else { return false };
    if base.is_empty() || base.contains('+') {
        return false;
    }
    let Some(tail) = rest.strip_prefix("nightly.").or_else(|| rest.strip_prefix("preview.")) else { return false };
    let Some((date, n)) = tail.split_once('.') else { return false };
    date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()) && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
}

/// `useEnvironmentStageLabel`: Nightly when the primary server runs a nightly, else this build's
/// stage (Dev for a debug build).
pub fn stage_label(s: &App) -> &'static str {
    let server = s.state().config.as_ref().and_then(|c| c["environment"]["serverVersion"].as_str().map(str::to_owned));
    if server.as_deref().is_some_and(nightly_version) {
        return "Nightly";
    }
    if cfg!(debug_assertions) {
        "Dev"
    } else if crate::settings::about::release_channel(env!("CARGO_PKG_VERSION")) != "stable" {
        "Nightly"
    } else {
        "Beta"
    }
}

fn mode(s: &App) -> String {
    s.client_settings.values.get(MODE).and_then(|v| v.as_str()).unwrap_or("artwork").to_owned()
}

/// `resolveSidebarStageBackdropVariant`.
pub fn variant_for(stage: &str, mode: &str) -> Option<Variant> {
    match mode {
        "nightly-artwork" => Some(Variant::Nightly),
        "dev-artwork" => Some(Variant::Dev),
        "artwork" => match stage.trim().to_lowercase().as_str() {
            "nightly" => Some(Variant::Nightly),
            "dev" => Some(Variant::Dev),
            _ => None,
        },
        _ => None,
    }
}

pub fn variant(s: &App) -> Option<Variant> {
    variant_for(stage_label(s), &mode(s))
}

/// `resolveEnvironmentIdentificationPillLabel`, while the mode is "pill".
pub fn pill(s: &App) -> Option<&'static str> {
    if mode(s) != "pill" {
        return None;
    }
    match stage_label(s).to_lowercase().as_str() {
        "dev" => Some("Dev"),
        "nightly" => Some("Nightly"),
        _ => None,
    }
}

/// `oklch()` as `#rrggbb`.
fn oklch(l: f32, c: f32, h: f32) -> String {
    let (a, b) = (c * h.to_radians().cos(), c * h.to_radians().sin());
    let l_ = (l + 0.396_337_8 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_35 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    let linear = [
        4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_,
        -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_4 * s_,
        -0.004_196_086 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_,
    ];
    let gamma = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let v = if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (v * 255.0).round() as u8
    };
    format!("#{:02x}{:02x}{:02x}", gamma(linear[0]), gamma(linear[1]), gamma(linear[2]))
}

type Lch = (f32, f32, f32);

/// Web's default `--stage-art-*` (light; dark changes only the first three) and
/// `--stage-night-base-*`, `top, mid, bottom, highlight, secondary, tertiary, line`.
const ART: [Lch; 7] = [
    (0.782_169, 0.123_386, 240.226),
    (0.616_111, 0.195_824, 259.735),
    (0.441_553, 0.232_394, 265.474),
    (0.951_597, 0.037_289, 215.482),
    (0.794_668, 0.121_36, 235.46),
    (0.678_991, 0.170_261, 275.365),
    (0.959_666, 0.029_238, 218.179),
];
const ART_DARK_TOP: [Lch; 3] = [(0.581_473, 0.149_124, 256.9), (0.456_509, 0.159_377, 261.945), (0.291_327, 0.136_578, 267.649)];
const NIGHT: [Lch; 7] = [
    (0.283_792, 0.117_327, 297.201),
    (0.227_147, 0.086_086, 277.99),
    (0.200_528, 0.055_699, 261.216),
    (0.707_246, 0.157_418, 252.091),
    (0.600_473, 0.182_225, 277.296),
    (0.625_83, 0.210_886, 305.994),
    (0.938_794, 0.029_114, 273.103),
];

/// (art light, art dark, night light, night dark) per built-in theme: `--stage-art-*` and
/// `--stage-night-*` in web's index.css, as oklch, in `top, mid, bottom, highlight, secondary,
/// tertiary, line` order.
const THEMES: [(&str, [Lch; 7], [Lch; 7], Option<([Lch; 7], [Lch; 7])>); 5] = [
    ("t3-chat", [(0.760675, 0.141361, 348.019), (0.588093, 0.188832, 353.138), (0.383959, 0.135096, 348.656), (0.949689, 0.034241, 339.171), (0.763402, 0.163836, 352.525), (0.70819, 0.180285, 311.949), (0.952158, 0.034194, 336.179)], [(0.540689, 0.143665, 347.587), (0.396586, 0.126592, 347.6), (0.249959, 0.079694, 340.523), (0.921297, 0.051708, 343.229), (0.667398, 0.165674, 352.549), (0.609315, 0.163722, 306.315), (0.945349, 0.036045, 341.433)], None),
    ("grove", [(0.751465, 0.112731, 157.313), (0.567105, 0.11099, 158.859), (0.367918, 0.067635, 160.558), (0.976387, 0.034353, 161.456), (0.80968, 0.117071, 158.817), (0.784102, 0.108047, 88.516), (0.981098, 0.027812, 158.718)], [(0.58719, 0.09869, 157.426), (0.454979, 0.079031, 159.756), (0.297856, 0.050355, 161.167), (0.952407, 0.053872, 158.44), (0.732591, 0.120606, 155.853), (0.716282, 0.116547, 80.563), (0.961577, 0.035285, 157.03)], Some(([(0.45805, 0.079616, 160.092), (0.368483, 0.065631, 162.719), (0.264797, 0.044397, 168.088), (0.912563, 0.055173, 158.974), (0.665652, 0.109731, 156.599), (0.698651, 0.103024, 89.828), (0.945336, 0.041923, 157.222)], [(0.398632, 0.065534, 158.601), (0.290561, 0.049694, 160.456), (0.210147, 0.03173, 169.818), (0.866303, 0.057526, 156.796), (0.586553, 0.093722, 157.365), (0.6364, 0.101769, 82.985), (0.913292, 0.035718, 156.976)]))),
    ("ocean", [(0.792993, 0.088072, 220.421), (0.590296, 0.116841, 243.184), (0.409123, 0.093718, 247.98), (0.971217, 0.023008, 213.615), (0.788391, 0.090856, 215.684), (0.76441, 0.099607, 187.893), (0.976025, 0.019647, 212.543)], [(0.59663, 0.089167, 233.427), (0.461094, 0.084904, 243.478), (0.294818, 0.05947, 250.526), (0.952907, 0.032224, 221.27), (0.732079, 0.09296, 224.414), (0.720885, 0.095495, 190.903), (0.961039, 0.027355, 219.756)], None),
    ("ember", [(0.782439, 0.118806, 52.569), (0.62863, 0.15922, 39.401), (0.404251, 0.117664, 30.424), (0.962469, 0.02774, 70.95), (0.815735, 0.11904, 53.659), (0.69361, 0.159346, 24.787), (0.970313, 0.02052, 67.583)], [(0.597533, 0.120694, 43.455), (0.437763, 0.101287, 34.86), (0.264269, 0.055858, 26.548), (0.929214, 0.042638, 55.801), (0.705592, 0.137369, 43.176), (0.629583, 0.158322, 24.088), (0.945058, 0.033906, 58.824)], Some(([(0.470236, 0.102322, 39.355), (0.345367, 0.080697, 33.217), (0.227386, 0.056077, 27.437), (0.903891, 0.059281, 56.181), (0.641705, 0.126508, 44.376), (0.538694, 0.129931, 25.865), (0.926348, 0.046029, 58.73)], [(0.392352, 0.081287, 36.444), (0.271305, 0.056352, 31.135), (0.182126, 0.028154, 27.774), (0.851007, 0.061294, 53.805), (0.560789, 0.10645, 42.953), (0.476228, 0.106656, 24.165), (0.884931, 0.046607, 56.556)]))),
    ("iris", [(0.738305, 0.113454, 298.926), (0.551826, 0.159189, 294.531), (0.37147, 0.133317, 289.999), (0.949472, 0.029204, 303.081), (0.745085, 0.125892, 298.647), (0.73066, 0.167815, 340.964), (0.960278, 0.024064, 306.969)], [(0.57297, 0.145973, 295.185), (0.419499, 0.13752, 292.131), (0.274235, 0.095798, 286.608), (0.916698, 0.047206, 300.224), (0.670994, 0.13095, 296.689), (0.679357, 0.165376, 340.439), (0.940582, 0.032921, 299.076)], None),
];

/// The stage palette's theme: `data-theme-id`, which web sets only for a built-in palette (legacy
/// `t3-*` ids name the same ones).
pub fn theme_id(s: &App, dark: bool) -> &str {
    let id = s.client_settings.values.get(if dark { "themeDark" } else { "themeLight" }).and_then(serde_json::Value::as_str).unwrap_or("t3-code");
    match id {
        "t3-chat-dark" => "t3-chat",
        "t3-grove" | "t3-ocean" | "t3-ember" | "t3-iris" => &id[3..],
        id => id,
    }
}

/// `color-mix(in oklch, a p, b)`: the shorter hue arc, and an achromatic side takes the other hue.
fn mix(a: Lch, b: Lch, p: f32) -> Lch {
    let (ha, hb) = match (a.1 == 0.0, b.1 == 0.0) {
        (true, _) => (b.2, b.2),
        (_, true) => (a.2, a.2),
        _ => (a.2, b.2),
    };
    let d = (hb - ha + 540.0).rem_euclid(360.0) - 180.0;
    let h = (hb - d * p).rem_euclid(360.0);
    (a.0 * p + b.0 * (1.0 - p), a.1 * p + b.1 * (1.0 - p), h)
}

/// The `--stage-*` colours for `variant` under `theme` (index.css's `html[data-theme-id]` rules).
fn colors(variant: Variant, dark: bool, theme: &str) -> Vec<(&'static str, String)> {
    let hex = |c: Lch| oklch(c.0, c.1, c.2);
    let builtin = THEMES.iter().find(|(id, ..)| *id == theme);
    let (art, night) = match builtin {
        None => {
            let mut art = ART;
            if dark {
                art[..3].copy_from_slice(&ART_DARK_TOP);
            }
            (art, NIGHT)
        }
        Some((_, light, dark_art, own)) => {
            let art = if dark { *dark_art } else { *light };
            let night = match own {
                Some((light, dark_night)) => if dark { *dark_night } else { *light },
                None => {
                    let (top, mid) = (mix(art[0], NIGHT[0], 0.38), mix(art[1], NIGHT[1], 0.38));
                    [top, mid, mix(art[2], NIGHT[2], 0.38), art[3], mix(art[4], mid, 0.55), mix(art[5], top, 0.6), mix(art[6], (1.0, 0.0, 0.0), 0.82)]
                }
            };
            (art, night)
        }
    };
    let names = ["top", "mid", "bottom", "highlight", "secondary", "tertiary", "line"];
    let (palette, extra): ([Lch; 7], Vec<(&'static str, Lch)>) = match (variant, builtin) {
        (Variant::Nightly, None) => (night, vec![
            ("glow-highlight", (0.553_749, 0.176_543, 271.958)),
            ("glow-secondary", (0.345_571, 0.117_466, 273.568)),
            ("sparkle", (0.880_867, 0.057_747, 269.011)),
        ]),
        (Variant::Nightly, Some(_)) => (night, vec![("glow-highlight", night[3]), ("glow-secondary", night[4]), ("sparkle", night[6])]),
        (Variant::Dev, None) => (art, vec![
            ("celeste-highlight", (0.968_763, 0.045_822, 196.42)),
            ("celeste-secondary", (0.827_395, 0.126_071, 211.26)),
            ("violet-highlight", (0.895_381, 0.053_248, 286.447)),
            ("grid-line", (0.966_822, 0.017_57, 239.99)),
        ]),
        (Variant::Dev, Some(_)) => (art, vec![("celeste-highlight", art[3]), ("celeste-secondary", art[4]), ("violet-highlight", art[3]), ("grid-line", art[6])]),
    };
    names.iter().zip(palette).map(|(n, c)| (*n, hex(c))).chain(extra.into_iter().map(|(n, c)| (n, hex(c)))).collect()
}

const NIGHTLY_STARS: [(f32, f32, f32, f32); 17] = [
    (14., 10., 0.6, 0.85),
    (38., 22., 0.4, 0.55),
    (58., 8., 0.5, 0.7),
    (84., 16., 0.4, 0.5),
    (104., 7., 0.6, 0.8),
    (126., 20., 0.4, 0.55),
    (148., 11., 0.5, 0.7),
    (170., 24., 0.4, 0.5),
    (192., 9., 0.6, 0.8),
    (214., 18., 0.4, 0.55),
    (236., 8., 0.5, 0.7),
    (258., 20., 0.45, 0.6),
    (278., 11., 0.55, 0.75),
    (26., 34., 0.4, 0.45),
    (118., 34., 0.4, 0.45),
    (202., 32., 0.4, 0.5),
    (268., 34., 0.4, 0.45),
];
const NIGHTLY_SPARKLES: [(f32, f32); 3] = [(70., 28.), (160., 36.), (246., 26.)];

const NIGHTLY: &str = r##"<defs>
<linearGradient id="sky" x1="24" y1="0" x2="264" y2="96" gradientUnits="userSpaceOnUse" spreadMethod="reflect"><stop stop-color="{bottom}"/><stop offset="0.5" stop-color="{mid}"/><stop offset="1" stop-color="{top}"/></linearGradient>
<radialGradient id="glow" cx="0" cy="0" r="1" gradientTransform="translate(216 18) rotate(137) scale(120 84)" gradientUnits="userSpaceOnUse"><stop stop-color="{glow-highlight}" stop-opacity="0.4"/><stop offset="0.5" stop-color="{glow-secondary}" stop-opacity="0.16"/><stop offset="1" stop-color="{bottom}" stop-opacity="0"/></radialGradient>
<linearGradient id="cloud" x1="0" y1="60" x2="288" y2="96" gradientUnits="userSpaceOnUse"><stop stop-color="{highlight}" stop-opacity="0.5"/><stop offset="0.52" stop-color="{secondary}" stop-opacity="0.62"/><stop offset="1" stop-color="{tertiary}" stop-opacity="0.5"/></linearGradient>
<filter id="soft" x="-24" y="-24" width="336" height="144" filterUnits="userSpaceOnUse"><feGaussianBlur stdDeviation="4"/></filter>
<pattern id="stars" width="288" height="96" patternUnits="userSpaceOnUse"><g fill="{line}">{stars}</g><g stroke="{sparkle}" stroke-linecap="round" stroke-opacity="0.7" stroke-width="0.6">{sparkles}</g></pattern>
<pattern id="glows" width="640" height="96" patternUnits="userSpaceOnUse"><rect width="640" height="96" fill="url(#glow)"/></pattern>
</defs>
<rect width="100%" height="96" fill="url(#sky)"/>
<rect width="100%" height="96" fill="url(#glows)"/>
<rect width="100%" height="96" fill="url(#stars)"/>
<g filter="url(#soft)"><path d="M-12 88C-12 74 0 63 14 63C18 50 30 41 44 41C58 41 70 49 74 62C79 57 86 54 94 54C110 54 123 66 124 82C132 83 138 88 141 96H-12V88Z" fill="url(#cloud)"/></g>
<g filter="url(#soft)"><path d="M150 96C151 84 161 75 173 75C176 64 186 57 198 57C210 57 220 64 223 75C231 75 238 80 241 87C250 87 257 91 260 96H150Z" fill="url(#cloud)" fill-opacity="0.8"/></g>"##;

const DEV: &str = r##"<defs>
<linearGradient id="paper" x1="60" y1="0" x2="220" y2="96" gradientUnits="userSpaceOnUse" spreadMethod="reflect"><stop stop-color="{bottom}"/><stop offset="0.5" stop-color="{mid}"/><stop offset="1" stop-color="{top}"/></linearGradient>
<radialGradient id="glow" cx="0" cy="0" r="1" gradientTransform="translate(216 14) rotate(137) scale(120 84)" gradientUnits="userSpaceOnUse"><stop stop-color="{highlight}" stop-opacity="0.4"/><stop offset="0.52" stop-color="{secondary}" stop-opacity="0.16"/><stop offset="1" stop-color="{bottom}" stop-opacity="0"/></radialGradient>
<radialGradient id="celeste" cx="0" cy="0" r="1" gradientTransform="translate(474 44) rotate(166) scale(156 92)" gradientUnits="userSpaceOnUse"><stop stop-color="{celeste-highlight}" stop-opacity="0.34"/><stop offset="0.5" stop-color="{celeste-secondary}" stop-opacity="0.18"/><stop offset="1" stop-color="{bottom}" stop-opacity="0"/></radialGradient>
<radialGradient id="violet" cx="0" cy="0" r="1" gradientTransform="translate(704 18) rotate(145) scale(132 88)" gradientUnits="userSpaceOnUse"><stop stop-color="{violet-highlight}" stop-opacity="0.3"/><stop offset="0.52" stop-color="{tertiary}" stop-opacity="0.14"/><stop offset="1" stop-color="{bottom}" stop-opacity="0"/></radialGradient>
<pattern id="minor" width="8" height="8" patternUnits="userSpaceOnUse"><path d="M8 0H0V8" stroke="{grid-line}" stroke-opacity="0.14" stroke-width="0.5"/></pattern>
<pattern id="major" width="32" height="32" patternUnits="userSpaceOnUse"><path d="M32 0H0V32" stroke="{grid-line}" stroke-opacity="0.26" stroke-width="0.6"/></pattern>
<pattern id="ruler" width="32" height="6" patternUnits="userSpaceOnUse"><path d="M4 0V2.5M12 0V2.5M20 0V4M28 0V2.5" stroke="{line}" stroke-opacity="0.5" stroke-width="0.5"/></pattern>
<pattern id="glows" width="768" height="96" patternUnits="userSpaceOnUse"><rect width="768" height="96" fill="url(#glow)"/><rect width="768" height="96" fill="url(#celeste)"/><rect width="768" height="96" fill="url(#violet)"/></pattern>
<pattern id="notes" width="768" height="96" patternUnits="userSpaceOnUse">
<g stroke="{line}" stroke-linecap="round" stroke-opacity="0.6" stroke-width="0.7"><path d="M180 64H264" stroke-dasharray="5 4"/><path d="M180 61V67M264 61V67"/><path d="M276 10V44" stroke-dasharray="4 4" stroke-opacity="0.5"/><path d="M273 10H279M273 44H279" stroke-opacity="0.5"/><path d="M348 30H428" stroke-dasharray="3.5 5" stroke-opacity="0.5"/><path d="M348 27V33M428 27V33" stroke-opacity="0.5"/><path d="M512 48V80" stroke-dasharray="5 3" stroke-opacity="0.45"/><path d="M509 48H515M509 80H515" stroke-opacity="0.45"/><path d="M590 70H724" stroke-dasharray="7 4" stroke-opacity="0.55"/><path d="M590 67V73M724 67V73" stroke-opacity="0.55"/></g>
<g stroke="{line}" stroke-linecap="round" stroke-opacity="0.55" stroke-width="0.6"><path d="M34 60L38 64M38 60L34 64"/><path d="M228 26H234M231 23V29"/><path d="M143 51H149M146 48V54"/><path d="M316 16L322 22M322 16L316 22"/><path d="M468 70H476M472 66V74"/><path d="M558 28L564 34M564 28L558 34"/><path d="M742 44H750M746 40V48"/></g>
<g stroke="{line}" stroke-opacity="0.35" stroke-width="0.6"><circle cx="196" cy="38" r="13" stroke-dasharray="3.5 4"/><path d="M196 33V43M191 38H201" stroke-opacity="0.6" stroke-width="0.4"/><circle cx="414" cy="64" r="10" stroke-dasharray="2.5 3.5"/><path d="M414 60V68M410 64H418" stroke-opacity="0.6" stroke-width="0.4"/><circle cx="648" cy="32" r="15" stroke-dasharray="4 5"/><path d="M648 26V38M642 32H654" stroke-opacity="0.6" stroke-width="0.4"/></g>
</pattern>
</defs>
<rect width="100%" height="96" fill="url(#paper)"/>
<rect width="100%" height="96" fill="url(#glows)"/>
<rect width="100%" height="96" fill="url(#minor)"/>
<rect width="100%" height="96" fill="url(#major)"/>
<rect width="100%" height="6" fill="url(#ruler)"/>
<rect width="100%" height="96" fill="url(#notes)"/>"##;

/// The artwork `width` points wide and `HEIGHT` tall: web's 96-unit-tall drawing sliced from
/// its left edge (`xMinYMin slice`), so its scale follows the height.
pub fn svg(variant: Variant, dark: bool, theme: &str, width: f32) -> String {
    let mut body = match variant {
        Variant::Nightly => {
            let stars: String = NIGHTLY_STARS.iter().map(|(x, y, r, o)| format!(r#"<circle cx="{x}" cy="{y}" r="{r}" fill-opacity="{o}"/>"#)).collect();
            let sparkles: String = NIGHTLY_SPARKLES
                .iter()
                .map(|(x, y)| format!(r#"<path d="M{} {y}H{}"/><path d="M{x} {}V{}"/>"#, x - 1.5, x + 1.5, y - 1.5, y + 1.5))
                .collect();
            NIGHTLY.replace("{stars}", &stars).replace("{sparkles}", &sparkles)
        }
        Variant::Dev => DEV.to_owned(),
    };
    for (name, hex) in colors(variant, dark, theme) {
        body = body.replace(&format!("{{{name}}}"), &hex);
    }
    let view = width * 96.0 / HEIGHT;
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{HEIGHT}" viewBox="0 0 {view} 96" fill="none">{body}</svg>"#)
}

/// `SidebarStageBackdrop`: the art across the header's top, fading into the sidebar.
pub fn backdrop(s: &App, t: &T, variant: Variant, width: f32, scale: f32) -> Option<El> {
    let art = &s.ui.shell.sidebar.stage;
    let theme = theme_id(s, t.dark);
    let key = format!("{variant:?}:{theme}:{}:{width}:{scale}", t.dark);
    let stale = art.image.borrow().as_ref().is_none_or(|(k, _)| *k != key);
    if stale {
        let waker = s.ui.panel.waker.clone()?;
        let mut cache = art.cache.borrow_mut();
        let cache = cache.get_or_insert_with(|| ImageCache::new(waker));
        let image = cache.svg(svg(variant, t.dark, theme, width).into_bytes(), width, scale);
        *art.image.borrow_mut() = Some((key, image));
    }
    let image = art.image.borrow();
    let (_, image) = image.as_ref()?;
    if !image.is_ready() || image.error().is_some() {
        return None;
    }
    let fade = t.sb.sidebar;
    // Web masks the art out from 55% to 92% and washes it toward the sidebar colour from 28%.
    Some(
        el().key("sb-stage")
            .role("presentation")
            .absolute(0.0, 0.0)
            .w(width)
            .h(HEIGHT)
            .pixels_fit(image.pixels(), rune::SurfaceFit::Fit)
            .child(el().absolute(0.0, HEIGHT * 0.28).w(width).h(HEIGHT * 0.64).gradient(fade.alpha(0.0), fade))
            .child(el().absolute(0.0, HEIGHT * 0.92).w(width).h(HEIGHT * 0.08).fill(fade.alpha_mul(1.0))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_art_follows_the_stage_unless_a_mode_fixes_it() {
        assert_eq!(variant_for("Dev", "artwork"), Some(Variant::Dev));
        assert_eq!(variant_for(" nightly ", "artwork"), Some(Variant::Nightly));
        assert_eq!(variant_for("Beta", "artwork"), None);
        assert_eq!(variant_for("Beta", "nightly-artwork"), Some(Variant::Nightly));
        assert_eq!(variant_for("Nightly", "pill"), None);
        assert_eq!(variant_for("Dev", "none"), None);
        assert!(nightly_version("0.4.0-nightly.20261121.3"));
        assert!(nightly_version("1.0.0-preview.20260101.12"));
        assert!(!nightly_version("0.4.0"));
        assert!(!nightly_version("0.4.0-beta.1"));
    }

    #[test]
    fn the_svg_has_concrete_colours_and_slices_by_height() {
        let art = svg(Variant::Nightly, true, "t3-code", 258.0);
        assert!(!art.contains('{'), "every placeholder is filled");
        assert!(art.contains(r#"viewBox="0 0 309.6 96""#));
        assert_eq!(oklch(1.0, 0.0, 0.0), "#ffffff");
        assert!(!svg(Variant::Dev, false, "t3-code", 258.0).contains('{'));
    }

    #[test]
    fn built_in_themes_tint_the_art_as_web_s_theme_rules_do() {
        let get = |variant, dark, theme, name: &str| colors(variant, dark, theme).into_iter().find(|(n, _)| *n == name).unwrap().1;
        assert_eq!(get(Variant::Nightly, true, "grove", "top"), oklch(0.398_632, 0.065_534, 158.601), "grove's own night");
        assert_eq!(get(Variant::Dev, false, "ember", "grid-line"), oklch(0.970_313, 0.020_52, 67.583), "a theme's grid is its line");
        assert_eq!(get(Variant::Dev, true, "custom-theme", "top"), oklch(0.581_473, 0.149_124, 256.9), "other themes keep the default art");
        // Ocean's night mixes its art 38% into the default night base.
        let (l, c, h) = mix(THEMES[2].1[0], NIGHT[0], 0.38);
        assert!((l - 0.477_29).abs() < 1e-4 && (c - 0.106_21).abs() < 1e-4 && (h - 268.025).abs() < 1e-2, "{l} {c} {h}");
        assert_eq!(get(Variant::Nightly, false, "ocean", "top"), oklch(l, c, h));
        assert_eq!(mix((0.9, 0.03, 212.0), (1.0, 0.0, 0.0), 0.82).2, 212.0, "white takes the other hue");
        assert_eq!(mix((0.5, 0.1, 350.0), (0.5, 0.1, 10.0), 0.5).2, 0.0, "the shorter arc");
    }

    #[test]
    fn legacy_theme_ids_name_the_built_in_palettes() {
        let mut s = App::new();
        s.client_settings.values.insert("themeDark".into(), serde_json::json!("t3-grove"));
        s.client_settings.values.insert("themeLight".into(), serde_json::json!("t3-chat-dark"));
        assert_eq!((theme_id(&s, true), theme_id(&s, false)), ("grove", "t3-chat"));
    }
}
