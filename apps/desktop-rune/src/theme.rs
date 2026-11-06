//! S5 Code's tokens (apps/web/src/index.css) as Rune colours, plus the kit the stock components
//! paint with.

use rune::color::hex;
use rune::tok::TextStyle;
use rune::{Color, oklch};
use rune_kit::{Kit, PaletteSeed};

#[derive(Clone, Copy)]
pub struct P {
    pub dark: bool,
    pub bg: Color,
    pub sidebar: Color,
    pub fg: Color,
    pub muted: Color,
    pub secondary: Color,
    pub faint: Color,
    pub border: Color,
    pub input: Color,
    pub card: Color,
    pub accent: Color,
    pub row_hover: Color,
    pub row_active: Color,
    pub row_selected: Color,
    pub primary: Color,
    pub on_primary: Color,
    pub message: Color,
    pub info: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub scrim: Color,
}

fn h(s: &str) -> Color {
    hex(s).unwrap_or(Color::BLACK)
}

pub fn palette(dark: bool) -> P {
    if dark {
        let fg = h("#f5f5f5");
        P {
            dark,
            bg: h("#000000"),
            sidebar: h("#000000"),
            fg,
            muted: h("#a3a3a3"),
            secondary: h("#8f8f8f"),
            faint: h("#6b6b6b"),
            border: Color::WHITE.alpha(0.08),
            input: Color::WHITE.alpha(0.11),
            card: h("#111111"),
            accent: h("#191a1d"),
            row_hover: fg.alpha(0.08),
            row_active: fg.alpha(0.11),
            row_selected: fg.alpha(0.07),
            primary: oklch(0.571, 0.21, 264.0),
            on_primary: Color::WHITE,
            message: h("#1e1e1e"),
            info: h("#60a5fa"),
            success: h("#34d399"),
            warning: h("#fbbf24"),
            error: h("#f87171"),
            scrim: Color::BLACK.alpha(0.6),
        }
    } else {
        let fg = h("#171717");
        P {
            dark,
            bg: h("#ffffff"),
            sidebar: h("#f7f7f8"),
            fg,
            muted: h("#737373"),
            secondary: h("#6b6b6b"),
            faint: h("#a3a3a3"),
            border: Color::BLACK.alpha(0.08),
            input: Color::BLACK.alpha(0.12),
            card: h("#fafafa"),
            accent: h("#ececee"),
            row_hover: fg.alpha(0.05),
            row_active: fg.alpha(0.08),
            row_selected: fg.alpha(0.05),
            primary: oklch(0.571, 0.21, 264.0),
            on_primary: Color::WHITE,
            message: h("#f0f0f1"),
            info: h("#2563eb"),
            success: h("#059669"),
            warning: h("#d97706"),
            error: h("#dc2626"),
            scrim: Color::BLACK.alpha(0.3),
        }
    }
}

pub const XS2: TextStyle = TextStyle::new(11.0, 400.0).lh(14.0);
pub const XS: TextStyle = TextStyle::new(12.0, 400.0).lh(16.0);
pub const XS_MEDIUM: TextStyle = TextStyle::new(12.0, 500.0).lh(16.0);
pub const SM: TextStyle = TextStyle::new(14.0, 400.0).lh(20.0);
pub const SM_MEDIUM: TextStyle = TextStyle::new(14.0, 500.0).lh(20.0);
pub const BASE_SEMIBOLD: TextStyle = TextStyle::new(16.0, 600.0).lh(24.0);
pub const HERO: TextStyle = TextStyle::new(30.0, 400.0).lh(36.0).tracking(-0.6);
pub const MONO_XS: TextStyle = TextStyle::new(12.0, 400.0).lh(16.0).mono();

thread_local! {
    static KITS: [&'static Kit; 2] = [Box::leak(Box::new(make_kit(false))), Box::leak(Box::new(make_kit(true)))];
}

pub fn kit(dark: bool) -> &'static Kit {
    KITS.with(|k| k[dark as usize])
}

fn make_kit(dark: bool) -> Kit {
    let p = palette(dark);
    let mut k = Kit::from_palette(PaletteSeed { bg: p.card, fg: p.fg, accent: p.primary });
    k.c.bg_accent = p.primary;
    k.c.border_focus = p.primary;
    k.radius.sm = 6.0;
    k.radius.md = 8.0;
    k
}
