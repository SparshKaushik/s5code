//! The web client's design system in Rune terms: its resolved colour tokens (`tokens.rs`,
//! generated), Tailwind's type scale and radii, and the font families the platform renders.
//!
//! Screens take a `&T` and read colours by their CSS names (`t.c.muted_foreground`), so a class
//! like `text-foreground/75` ports as `t.c.foreground.alpha_mul(0.75)`.

use rune::Color;
use rune::tok::{Family, TextStyle};

#[allow(unused_imports)]
pub use crate::tokens::{Tokens, tw};

#[derive(Clone, Copy)]
pub struct T {
    pub dark: bool,
    /// Tokens as the document root resolves them.
    pub c: Tokens,
    /// Tokens inside the sidebar (`[data-app-sidebar]` re-points foreground, rows, labels).
    pub sb: Tokens,
    /// `--font-size-code` for chat code blocks, in layout points.
    pub code: f32,
}

pub fn theme(dark: bool) -> T {
    use crate::tokens::*;
    if dark { T { dark, c: DARK, sb: SIDEBAR_DARK, code: 13.0 } } else { T { dark, c: LIGHT, sb: SIDEBAR_LIGHT, code: 13.0 } }
}

/// Tailwind's `color/NN` opacity modifier: multiplies the token's own alpha.
pub trait AlphaMul {
    fn alpha_mul(self, k: f32) -> Color;
}

impl AlphaMul for Color {
    fn alpha_mul(self, k: f32) -> Color {
        self.alpha(self.a * k)
    }
}

/// `--radius` is 0.5rem; Tailwind's `rounded-*` steps derive from it.
pub mod radius {
    pub const XS: f32 = 2.0;
    pub const SM: f32 = 4.0;
    pub const MD: f32 = 6.0;
    pub const LG: f32 = 8.0;
    pub const XL: f32 = 12.0;
    pub const XL2: f32 = 16.0;
    pub const FULL: f32 = 9999.0;
}

/// Tailwind's `--spacing` (0.25rem): `p-3` is `sp(3.0)`.
pub const fn sp(n: f32) -> f32 {
    n * 4.0
}

/// The sans slot is Inter off macOS and San Francisco on it, as the CSS stack
/// `-apple-system, BlinkMacSystemFont, "Inter Variable", ...` resolves.
pub const SANS: &str = if cfg!(target_os = "macos") { "System Font" } else { "Inter" };

const fn ts(size: f32, line_height: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).lh(line_height)
}

/// `font-mono`: the mono slot, which holds the code font setting ahead of `ui-monospace` (SF Mono
/// on macOS, fontconfig's monospace elsewhere).
pub const fn mono(style: TextStyle) -> TextStyle {
    TextStyle { family: Family::Mono, ..style }
}

pub const fn weight(style: TextStyle, w: f32) -> TextStyle {
    TextStyle { weight: w, ..style }
}

// Tailwind's scale plus the app's own `text-2xs`..`text-5xs` (index.css @theme).
pub const TEXT_5XS: TextStyle = ts(7.0, 7.0, 400.0);
pub const TEXT_4XS: TextStyle = ts(8.0, 8.0, 400.0);
pub const TEXT_3XS: TextStyle = ts(10.0, 14.0, 400.0);
pub const TEXT_2XS: TextStyle = ts(11.0, 16.0, 400.0);
pub const TEXT_XS: TextStyle = ts(12.0, 16.0, 400.0);
pub const TEXT_SM: TextStyle = ts(14.0, 20.0, 400.0);
pub const TEXT_BASE: TextStyle = ts(16.0, 24.0, 400.0);
pub const TEXT_LG: TextStyle = ts(18.0, 28.0, 400.0);
pub const TEXT_XL: TextStyle = ts(20.0, 28.0, 400.0);
pub const TEXT_2XL: TextStyle = ts(24.0, 32.0, 400.0);
pub const TEXT_3XL: TextStyle = ts(30.0, 36.0, 400.0);

pub const MEDIUM: f32 = 500.0;
pub const SEMIBOLD: f32 = 600.0;
pub const BOLD: f32 = 700.0;

pub const XS_MEDIUM: TextStyle = weight(TEXT_XS, MEDIUM);
pub const SM_MEDIUM: TextStyle = weight(TEXT_SM, MEDIUM);
pub const MONO_XS: TextStyle = mono(TEXT_XS);
pub const MONO_SM: TextStyle = mono(TEXT_SM);

/// Layout constants from index.css `:root`.
pub mod layout {
    pub const TOPBAR_H: f32 = 52.0;
    pub const SIDEBAR_W: f32 = 256.0;
    pub const CHAT_MAX_W: f32 = 736.0;
    pub const THREAD_DETAILS_W: f32 = 280.0;
    pub const CONTROL_RADIUS: f32 = 8.0;
    pub const TITLEBAR_CONTROL: f32 = 28.0;
}
