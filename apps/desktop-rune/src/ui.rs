//! Small pieces the screens share: ghost icon buttons, chips and project favicons.

use rune::{Color, Cursor, El, Icon, Tx, el, icon, row, text};

use crate::App;
use crate::theme::{self, P};

/// A square ghost button with a Lucide icon, as `Button size="icon-sm" variant="ghost"`.
pub fn icon_button(p: P, key: &str, i: &'static Icon, size: f32, label: &str, f: impl Fn(&mut App, &mut Tx) + 'static) -> El {
    icon_button_tinted(p, key, i, size, label, p.muted, f)
}

pub fn icon_button_tinted(p: P, key: &str, i: &'static Icon, size: f32, label: &str, tint: Color, f: impl Fn(&mut App, &mut Tx) + 'static) -> El {
    row()
        .key(key.to_string())
        .role("button")
        .label(label.to_string())
        .help(label.to_string())
        .size(size, size)
        .center()
        .radius(6.0)
        .hover(p.row_hover)
        .press(p.row_active)
        .cursor(Cursor::Pointer)
        .on_press_tx(f)
        .child(icon(i, (size * 0.5).round()).color(tint))
}

pub fn disabled_icon(p: P, key: &str, i: &'static Icon, size: f32) -> El {
    row().key(key.to_string()).size(size, size).center().child(icon(i, (size * 0.5).round()).color(p.faint.alpha(0.6)))
}

/// A composer footer chip: 28 points, rounded, icon and label at 80% foreground.
pub fn chip(p: P, key: &str, i: Option<&'static Icon>, label: &str) -> El {
    let ink = p.fg.alpha(0.8);
    let mut c = row().key(key.to_string()).h(28.0).px(8.0).gap(6.0).center_y().radius(8.0).hover(p.row_hover).cursor(Cursor::Pointer);
    if let Some(i) = i {
        c = c.child(icon(i, 14.0).color(ink));
    }
    c.child(text(label.to_string()).font(theme::SM).color(ink))
}

/// The project's favicon stand-in: a rounded monogram in the project's colour.
pub fn favicon(name: &str, color: Color, size: f32) -> El {
    let letter = name.chars().next().unwrap_or('?').to_ascii_uppercase().to_string();
    row()
        .size(size, size)
        .center()
        .radius((size * 0.28).round())
        .fill(color.alpha(0.9))
        .child(text(letter).font(rune::tok::TextStyle::new((size * 0.62).round(), 600.0).lh(size)).color(Color::WHITE))
}

pub fn hairline(p: P) -> El {
    el().w_full().h(1.0).fill(p.border)
}

pub fn relative(now_ms: u64, then_ms: u64) -> String {
    let s = now_ms.saturating_sub(then_ms) / 1000;
    match s {
        0..=59 => "now".into(),
        60..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

pub fn duration(now_ms: u64, since_ms: u64) -> String {
    let s = now_ms.saturating_sub(since_ms) / 1000;
    if s < 60 { format!("{s}s") } else { format!("{}m {}s", s / 60, s % 60) }
}
