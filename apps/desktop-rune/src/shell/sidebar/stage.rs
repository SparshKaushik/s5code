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

/// The `--stage-*` colours of web's default palette.
fn colors(variant: Variant, dark: bool) -> Vec<(&'static str, String)> {
    match variant {
        Variant::Nightly => vec![
            ("top", oklch(0.283_792, 0.117_327, 297.201)),
            ("mid", oklch(0.227_147, 0.086_086, 277.99)),
            ("bottom", oklch(0.200_528, 0.055_699, 261.216)),
            ("highlight", oklch(0.707_246, 0.157_418, 252.091)),
            ("secondary", oklch(0.600_473, 0.182_225, 277.296)),
            ("tertiary", oklch(0.625_83, 0.210_886, 305.994)),
            ("line", oklch(0.938_794, 0.029_114, 273.103)),
            ("glow-highlight", oklch(0.553_749, 0.176_543, 271.958)),
            ("glow-secondary", oklch(0.345_571, 0.117_466, 273.568)),
            ("sparkle", oklch(0.880_867, 0.057_747, 269.011)),
        ],
        Variant::Dev => {
            let (top, mid, bottom) = if dark {
                (oklch(0.581_473, 0.149_124, 256.9), oklch(0.456_509, 0.159_377, 261.945), oklch(0.291_327, 0.136_578, 267.649))
            } else {
                (oklch(0.782_169, 0.123_386, 240.226), oklch(0.616_111, 0.195_824, 259.735), oklch(0.441_553, 0.232_394, 265.474))
            };
            vec![
                ("top", top),
                ("mid", mid),
                ("bottom", bottom),
                ("highlight", oklch(0.951_597, 0.037_289, 215.482)),
                ("secondary", oklch(0.794_668, 0.121_36, 235.46)),
                ("tertiary", oklch(0.678_991, 0.170_261, 275.365)),
                ("line", oklch(0.959_666, 0.029_238, 218.179)),
                ("celeste-highlight", oklch(0.968_763, 0.045_822, 196.42)),
                ("celeste-secondary", oklch(0.827_395, 0.126_071, 211.26)),
                ("violet-highlight", oklch(0.895_381, 0.053_248, 286.447)),
                ("grid-line", oklch(0.966_822, 0.017_57, 239.99)),
            ]
        }
    }
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
pub fn svg(variant: Variant, dark: bool, width: f32) -> String {
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
    for (name, hex) in colors(variant, dark) {
        body = body.replace(&format!("{{{name}}}"), &hex);
    }
    let view = width * 96.0 / HEIGHT;
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{HEIGHT}" viewBox="0 0 {view} 96" fill="none">{body}</svg>"#)
}

/// `SidebarStageBackdrop`: the art across the header's top, fading into the sidebar.
pub fn backdrop(s: &App, t: &T, variant: Variant, width: f32, scale: f32) -> Option<El> {
    let art = &s.ui.shell.sidebar.stage;
    let key = format!("{variant:?}:{}:{width}:{scale}", t.dark);
    let stale = art.image.borrow().as_ref().is_none_or(|(k, _)| *k != key);
    if stale {
        let waker = s.ui.panel.waker.clone()?;
        let mut cache = art.cache.borrow_mut();
        let cache = cache.get_or_insert_with(|| ImageCache::new(waker));
        let image = cache.svg(svg(variant, t.dark, width).into_bytes(), width, scale);
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
        let art = svg(Variant::Nightly, true, 258.0);
        assert!(!art.contains('{'), "every placeholder is filled");
        assert!(art.contains(r#"viewBox="0 0 309.6 96""#));
        assert_eq!(oklch(1.0, 0.0, 0.0), "#ffffff");
        assert!(!svg(Variant::Dev, false, 258.0).contains('{'));
    }
}
