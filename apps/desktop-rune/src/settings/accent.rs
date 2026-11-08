//! Web `ProviderAccentColorPicker` (inline layout): a swatch button beside the display name that
//! opens a saturation/value plane, a hue slider and a hex field. Drags preview and commit once on
//! release; the colour is the instance's top-level `accentColor`, omitted when cleared.
use crate::theme::{AlphaMul, T};
use crate::{App, icons, kit};
use rune::{Color, Cursor, El, Phase, TextEdit, col, el, row};
use serde_json::json;

const FALLBACK: &str = "#2563eb";
const PLANE_W: f32 = 224.0;
const PLANE_H: f32 = 144.0;

pub struct Draft {
    id: String,
    hsv: (f32, f32, f32),
    hex: TextEdit,
    /// A dragged colour not yet committed; closing the popover commits it.
    previewing: bool,
}

/// `normalizeProviderAccentColor`: `#rrggbb` or nothing.
pub fn normalize(value: &str) -> Option<String> {
    let value = value.trim();
    let digits = value.strip_prefix('#')?;
    (digits.len() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit())).then(|| value.to_owned())
}

fn stored(s: &App, id: &str) -> Option<String> {
    super::server_settings(s)["providerInstances"][id]["accentColor"].as_str().and_then(normalize)
}

pub fn hex_to_hsv(hex: &str) -> (f32, f32, f32) {
    let n = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x2563eb);
    let [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map(|c| c as f32 / 255.0);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, if max == 0.0 { 0.0 } else { d / max }, max)
}

pub fn hsv_to_hex(h: f32, s: f32, v: f32) -> String {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let byte = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
}

fn color(hex: &str) -> Color {
    rune::color::hex(hex).unwrap_or(Color::BLACK)
}

fn commit(s: &mut App, id: &str, value: &str) {
    super::providers::update(s, id, "accentColor", json!(normalize(value).unwrap_or_default()));
}

fn draft(s: &mut App) -> Option<&mut Draft> {
    s.ui.settings.accent.as_mut()
}

fn set_hsv(s: &mut App, hsv: (f32, f32, f32), release: bool) {
    let Some(d) = draft(s) else { return };
    d.hsv = hsv;
    let hex = hsv_to_hex(hsv.0, hsv.1, hsv.2);
    d.hex = TextEdit::new(hex.clone());
    d.previewing = !release;
    let id = d.id.clone();
    if release {
        commit(s, &id, &hex);
    }
}

/// Closing commits a colour still being previewed, as web's `onClose`.
pub fn close(s: &mut App) -> bool {
    let Some(d) = s.ui.settings.accent.take() else { return false };
    if d.previewing {
        commit(s, &d.id, &hsv_to_hex(d.hsv.0, d.hsv.1, d.hsv.2));
    }
    true
}

/// The swatch trigger: the colour, or a plus while none is set.
pub fn trigger(s: &App, t: &T, id: &str, display_name: &str) -> El {
    let open = s.ui.settings.accent.as_ref().filter(|d| d.id == id);
    let shown = match open {
        Some(d) if d.previewing => Some(hsv_to_hex(d.hsv.0, d.hsv.1, d.hsv.2)),
        _ => stored(s, id),
    };
    let owned = id.to_owned();
    let mut button = kit::Button::new(format!("{} accent color for {display_name}", if shown.is_some() { "Change" } else { "Add" }))
        .size("icon-sm")
        .variant("ghost-muted");
    if shown.is_none() {
        button = button.icon(&icons::PLUS);
    }
    let mut button = button
        .on_press(move |s: &mut App| {
            if !close(s) {
                let hex = stored(s, &owned).unwrap_or_else(|| FALLBACK.into());
                s.ui.settings.accent = Some(Draft { id: owned.clone(), hsv: hex_to_hsv(&hex), hex: TextEdit::new(hex), previewing: false });
            }
        })
        .build(t)
        .expanded(open.is_some());
    if let Some(hex) = &shown {
        button = button.fill(color(hex));
    }
    row().key(format!("accent-{id}-trigger")).child(button)
}

fn thumb(size: f32) -> El {
    el().size(size, size)
        .radius(size / 2.0)
        .border(2.0, Color::WHITE)
        .shadow(0.0, 1.0, Color::BLACK.alpha(0.4))
}

fn panel(s: &App, t: &T, d: &Draft) -> El {
    let (h, sat, v) = d.hsv;
    let current = hsv_to_hex(h, sat, v);
    let (white, black) = (Color::WHITE, Color::BLACK);
    let plane = el()
        .key("accent-plane")
        .role("slider")
        .label("Accent color")
        .size(PLANE_W, PLANE_H)
        .cursor(Cursor::Crosshair)
        .fill(color(&hsv_to_hex(h, 1.0, 1.0)))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).linear_gradient(std::f32::consts::FRAC_PI_2, [(0.0, white), (1.0, white.alpha(0.0))]))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).gradient(black.alpha(0.0), black))
        .child(thumb(12.0).absolute(sat * PLANE_W - 6.0, (1.0 - v) * PLANE_H - 6.0))
        .on_drag(move |s: &mut App, d: &mut rune::Drag, _| {
            if d.cancelled() {
                return;
            }
            let (sat, v) = ((d.x / d.w).clamp(0.0, 1.0), 1.0 - (d.y / d.h).clamp(0.0, 1.0));
            let hue = s.ui.settings.accent.as_ref().map_or(0.0, |d| d.hsv.0);
            set_hsv(s, (hue, sat, v), matches!(d.phase, Phase::Up));
        });
    let stops: Vec<(f32, Color)> =
        (0..=6).map(|i| (i as f32 / 6.0, color(&hsv_to_hex(i as f32 * 60.0, 1.0, 1.0)))).collect();
    let track_w = PLANE_W - 24.0;
    let hue = row()
        .key("accent-hue")
        .role("slider")
        .label("Accent color hue")
        .range_value(h as f64, 0.0, 360.0)
        .w_full()
        .h(24.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .child(el().w_full().h(10.0).radius(5.0).linear_gradient(std::f32::consts::FRAC_PI_2, stops).border(1.0, black.alpha(0.12)))
        .child(thumb(16.0).absolute(h / 360.0 * track_w - 8.0, 4.0))
        .on_drag(move |s: &mut App, d: &mut rune::Drag, _| {
            if d.cancelled() {
                return;
            }
            let Some((_, sat, v)) = s.ui.settings.accent.as_ref().map(|d| d.hsv) else { return };
            set_hsv(s, ((d.x / d.w).clamp(0.0, 1.0) * 360.0, sat, v), matches!(d.phase, Phase::Up));
        });
    let hex = kit::Input::new(&d.hex)
        .size("compact")
        .font("mono")
        .on_edit(|s: &mut App| &mut s.ui.settings.accent.get_or_insert_with(|| Draft::new("")).hex)
        .on_change(|s: &mut App, _: &mut rune::Tx| {
            let Some(d) = draft(s) else { return };
            if let Some(hex) = normalize(&d.hex.text).filter(|h| h.len() == d.hex.text.len()) {
                d.hsv = hex_to_hsv(&hex);
                d.previewing = false;
                let id = d.id.clone();
                commit(s, &id, &hex);
            }
        })
        .build(t)
        .label("Custom hex accent color")
        .single_line();
    let set = stored(s, &d.id).is_some();
    let mut controls = col().w_full().gap(12.0).padding(12.0).child(hue).child(hex);
    if !set {
        controls = controls.child(
            kit::Button::new("Add color")
                .size("compact")
                .on_press(move |s: &mut App| {
                    if let Some(d) = s.ui.settings.accent.take() {
                        commit(s, &d.id, &current);
                    }
                })
                .build(t),
        );
    }
    let mut body = col().w(PLANE_W).child(plane).child(controls);
    if set {
        body = body.child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.6))).child(
            col().w_full().padding(4.0).child(
                kit::Button::new("Clear color")
                    .size("compact")
                    .variant("ghost-muted")
                    .icon(&icons::X)
                    .on_press(|s: &mut App| {
                        if let Some(d) = s.ui.settings.accent.take() {
                            commit(s, &d.id, "");
                        }
                    })
                    .build(t)
                    .w_full()
                    .justify_start(),
            ),
        );
    }
    body
}

impl Draft {
    fn new(id: &str) -> Self {
        Draft { id: id.into(), hsv: hex_to_hsv(FALLBACK), hex: TextEdit::new(FALLBACK), previewing: false }
    }
}

/// The open picker, anchored to its trigger like the settings selects.
pub fn popover(s: &App, t: &T, layer: &mut Vec<El>) {
    let Some(d) = &s.ui.settings.accent else { return };
    let _trigger = kit::Popover::new(format!("accent-{}", d.id))
        .open(true)
        .padding(0.0)
        .gutter(6.0)
        .placement(kit::Placement::BottomStart)
        .label("Accent color")
        .child(panel(s, t, d))
        .on_close(|s: &mut App| {
            close(s);
        })
        .build(&kit::theme(t), layer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_colors_round_trip_and_only_six_digit_hex_is_kept() {
        for hex in ["#2563eb", "#000000", "#ffffff", "#ff0000", "#00ff00", "#0000ff", "#a1b2c3"] {
            let (h, s, v) = hex_to_hsv(hex);
            assert_eq!(hsv_to_hex(h, s, v), hex);
        }
        assert_eq!(hsv_to_hex(360.0, 1.0, 1.0), "#ff0000");
        assert_eq!(normalize(" #AbCdEf "), Some("#AbCdEf".into()));
        assert_eq!(normalize("#abc"), None);
        assert_eq!(normalize("2563eb"), None);
        assert_eq!(normalize(""), None);
    }
}
