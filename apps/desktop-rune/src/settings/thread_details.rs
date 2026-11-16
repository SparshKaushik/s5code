//! Web `ThreadDetailsItemsRow`: which items the thread details card shows. The list, its rules
//! and its menu are the card's own (`chat::details::items`).

use crate::chat::details::items;
use crate::kit::{self, Menu, Placement};
use crate::theme::T;
use crate::{App, icons};
use rune::{El, row};

/// Web `RESET_THREAD_DETAILS_ITEMS`.
pub fn reset(s: &mut App) {
    items::reset(s);
}

pub fn customized(s: &App) -> bool {
    !items::is_default(&s.client_settings.values)
}

pub fn control(s: &App, t: &T) -> El {
    let summary = items::summary(s);
    let open = s.ui.settings.thread_details_open;
    row().key("thread-details-trigger").child(
        kit::Button::new(summary.clone())
            .variant("outline")
            .size("sm")
            .on_press(move |s: &mut App| s.ui.settings.thread_details_open = !open)
            .build(t)
            .w(160.0)
            .label(format!("Thread details: {summary}"))
            .child(rune::icon(&icons::CHEVRON_DOWN, 14.0).color(t.c.muted_foreground))
            .expanded(open),
    )
}

/// Web `ThreadDetailsItemsMenuItems`.
pub fn overlay(s: &App, t: &T, layers: &mut Vec<El>) {
    if !s.ui.settings.thread_details_open {
        return;
    }
    Menu::new("thread-details-menu")
        .anchor("thread-details-trigger")
        .placement(Placement::BottomEnd)
        .gutter(4.0)
        .width(232.0)
        .open(true)
        .on_close(|s: &mut App| s.ui.settings.thread_details_open = false)
        .entries(items::entries(s))
        .overlay(&kit::theme(t), layers);
}
