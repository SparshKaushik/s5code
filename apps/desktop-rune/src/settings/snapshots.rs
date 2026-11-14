//! Web `SnapShotSettings` as it renders without a desktop capture bridge: this client has no
//! window capture yet, so web shows its `SettingsUnavailableGroup` and hides the Capture rows and
//! the setup wizard, which need the bridge's state.

use super::controls as c;
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, col, icon, row, text};

/// `snapShotUnavailableMessage` inside the desktop app.
const UNAVAILABLE: &str = "Update the desktop app to use snapshots.";
/// `snapShotDescription` without bridge state.
const DESCRIPTION: &str = "Capture a window and attach it to your current draft.";

pub fn view(s: &App, t: &T) -> El {
    let notice = row()
        .w_full()
        .px(16.0)
        .py(8.0)
        .gap(8.0)
        .items_start()
        .child(icon(&icons::INFO, 14.0).color(t.c.warning).mt(2.0))
        .child(text(UNAVAILABLE).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap().grow(1.0).min_w(0.0));
    let on = super::value(s, "snapShotEnabled") == true;
    let line = row()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(32.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .opacity(0.64)
                .child(text("SnapShots").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(text(DESCRIPTION).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap()),
        )
        .child(
            crate::kit::Switch::new(on)
                .label("Enable snapshots")
                .hide_label()
                .disabled(true)
                .build(&crate::kit::theme(t)),
        );
    let group = col().w_full().py(6.0).fill(t.c.muted.alpha_mul(0.2)).child(notice).child(line);
    c::section("SnapShots", vec![group], t)
}
