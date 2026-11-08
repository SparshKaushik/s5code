//! The app's motion budget. Rune springs every change by default (300 ms layout and colour
//! springs, a 300 ms fade on every node that mounts, growth rows capped in points per second);
//! the web animates almost nothing. Content lands in the frame it commits; only popups (menus,
//! popovers, tooltips, dialogs, toasts) keep the web's short fade and 0.98 scale.
use crate::theme::{AlphaMul, T};
use rune::{El, Spring, SpringPair, Transition, el};

/// Colour changes (`transition-colors`, 150 ms): hover fills, selected rows, control thumbs.
pub const COLOR: Spring = Spring::new(0.15, 0.0);
/// Popups in 150 ms, out in 100 ms (web: `transition-[scale,opacity]`, 150 ms).
const POP: SpringPair = SpringPair::new(Spring::new(0.15, 0.0), Spring::new(0.1, 0.0));

/// Content that already exists (thread timeline, sidebar, panels): every layout, mount, exit and
/// growth change inside lands at once, so switching or loading a thread never slides text in.
/// Hover and colour changes keep a 150 ms fade.
pub fn content(e: El) -> El {
    e.instant_changes().motion(COLOR)
}

/// A popup surface: fades and scales in on `POP`, and what it holds changes at once (a menu
/// filtering, a settings section switching).
pub fn popup(e: El) -> El {
    e.transition(Transition::scale(0.98).springs(POP)).instant_changes()
}

/// A full-window layer that only positions a popup: present at once, so the popup inside plays
/// its own entrance instead of riding a 300 ms default fade.
pub fn layer() -> El {
    el().absolute(0., 0.).w_full().h_full().transition(Transition::none())
}

/// The web's modal dim (`bg-background/60`, 64% dark), shown the frame the modal opens. No
/// backdrop blur: a full-window blur painted late on Retina and the web's is only 4 px.
pub fn scrim(t: &T) -> El {
    layer().fill(t.c.background.alpha_mul(if t.dark { 0.64 } else { 0.6 }))
}
