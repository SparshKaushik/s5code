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
    /// One CSS `px` in layout points: px-literal sizes ignore the interface size, as on web.
    pub px: f32,
    /// `--chat-content-max-width` under the Chat width setting.
    pub chat_w: f32,
    /// The Render math setting.
    pub math: bool,
    /// The prompt editor's type under the Prompt font setting.
    pub prompt: TextStyle,
}

pub fn theme(dark: bool) -> T {
    use crate::tokens::*;
    let (c, sb) = if dark { (DARK, SIDEBAR_DARK) } else { (LIGHT, SIDEBAR_LIGHT) };
    T { dark, c, sb, code: 13.0, px: 1.0, chat_w: layout::CHAT_MAX_W, math: false, prompt: TEXT_SM.lh(22.75) }
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

/// CSS `color-mix(in srgb, a p, b)`: `p` of `a`, interpolated premultiplied in gamma-encoded sRGB.
pub fn mix_srgb(a: Color, b: Color, p: f32) -> Color {
    use rune::color::{srgb_decode, srgb_encode};
    let (wa, wb) = (a.a * p, b.a * (1.0 - p));
    let alpha = wa + wb;
    if alpha <= 0.0 {
        return Color::TRANSPARENT;
    }
    let ch = |x: f32, y: f32| srgb_decode((srgb_encode(x) * wa + srgb_encode(y) * wb) / alpha);
    Color { r: ch(a.r, b.r), g: ch(a.g, b.g), b: ch(a.b, b.b), a: alpha }
}

/// `--guest-surface` (`bg-guest-surface`, a guest's bubble): `color-mix(in srgb, var(--guest)
/// 14%, var(--message-surface))`, so it follows whatever message surface the theme sets.
pub fn guest_surface(c: &Tokens) -> Color {
    mix_srgb(c.guest, c.message_surface, 0.14)
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

pub(crate) const fn ts(size: f32, line_height: f32, weight: f32) -> TextStyle {
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

#[cfg(test)]
mod tests {
    #[test]
    fn the_guest_surface_mixes_like_the_web_app() {
        // The generated tokens are the browser's own computed `--guest-surface`.
        for dark in [false, true] {
            let c = super::theme(dark).c;
            let mixed = super::guest_surface(&c);
            let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
            assert!(close(mixed.r, c.guest_surface.r) && close(mixed.g, c.guest_surface.g) && close(mixed.b, c.guest_surface.b) && close(mixed.a, 1.0), "{dark}: {mixed:?}");
        }
    }
}
