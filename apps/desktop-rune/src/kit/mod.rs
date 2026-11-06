//! Shared web-shaped controls. `Button::new("Save").variant("outline").size("sm").build(t)`.
//! Stateful Rune controls use `theme(t)`; mount their overlay vector in `rune_kit::root`.
#![allow(dead_code, unused_imports)]
pub mod button;
mod actions;
mod checkbox;
mod switch;
mod radio;
mod segmented;
mod toggle;
pub use checkbox::Checkbox;
pub use switch::Switch;
pub use radio::{RadioGroup, RadioItem};
pub use segmented::SegmentedControl;
pub use toggle::{ToggleGroup, Tabs};
pub mod menu;
pub use menu::{Menu, MenuEntry, MenuItem, context_menu};
mod dialog;
mod input;
mod slider;
pub use dialog::Dialog;
pub use input::Input;
pub use slider::Slider;
pub mod gallery;
use crate::{
    App,
    theme::{self, AlphaMul, T},
};
pub use button::{Button, icon_button};
pub use gallery::SCENES;
use rune::{El, TextEdit, col, el, row, text};
use rune_kit::ScrollBar;
pub use rune_kit::{
    Combobox, ComboboxOption, ComboboxState, Placement,
    Popover, Progress, Select, Spinner, State,
    TextArea, Toast, ToastRegion, Tooltip,
};

/// Rune's behavior with S5's semantic palette and typography, shared by all overlay controls.
pub fn theme(t: &T) -> rune_kit::Kit {
    let mut k = rune_kit::Kit::neutral(t.dark);
    let c = t.c;
    k.c.page = c.background;
    k.c.bg_base = if t.dark {
        c.input.alpha_mul(0.32)
    } else {
        c.background
    };
    k.c.layer_01 = c.popover;
    k.c.layer_02 = c.secondary;
    k.c.layer_03 = c.muted;
    k.c.surface_float = c.popover;
    k.c.surface_base = c.background;
    k.c.surface_weak = c.primary;
    k.c.surface_hover = c.accent;
    k.c.surface_raised_base = c.popover;
    k.c.text_base = c.contrast_foreground;
    k.c.text_muted = c.contrast_muted_foreground;
    k.c.text_faint = c.contrast_placeholder;
    k.c.std_text = c.contrast_foreground;
    k.c.std_icon = c.primary_foreground;
    k.c.text_strong = c.contrast_foreground;
    k.c.text_weak = c.contrast_muted_foreground;
    k.c.icon_base = c.contrast_foreground;
    k.c.icon_muted = c.contrast_icon_muted;
    k.c.border_base = c.contrast_border;
    k.c.border_weak = c.contrast_input;
    k.c.border_strong = c.contrast_input;
    k.c.border_focus = c.ring;
    k.c.border_active = c.ring;
    k.c.bg_accent = c.primary;
    k.c.text_contrast = c.primary_foreground;
    k.c.hover = c.contrast_foreground.alpha_mul(0.06);
    k.c.pressed = c.contrast_foreground.alpha_mul(0.09);
    k.c.scrim = c.background.alpha_mul(if t.dark { 0.64 } else { 0.6 });
    k.c.danger.fg = c.destructive_foreground;
    k.c.danger.bg = c.destructive.alpha_mul(0.08);
    k.ty.ui = theme::TEXT_SM;
    k.ty.ui_medium = theme::SM_MEDIUM;
    k.ty.ui_compact = theme::TEXT_SM;
    k.ty.small = theme::TEXT_XS;
    k.ty.small_medium = theme::XS_MEDIUM;
    k.ty.text_small = theme::TEXT_SM;
    k.ty.text_small_medium = theme::SM_MEDIUM;
    k.ty.text_xsmall = theme::TEXT_XS;
    k.ty.mono = theme::MONO_SM;
    k.radius.sm = 4.;
    k.radius.md = 8.;
    k.radius.lg = 12.;
    k.radius.xl = 16.;
    k.glyph.check = &crate::icons::CHECK;
    k.glyph.close = &crate::icons::X;
    k
}
/// Text input sizes follow the web's desktop breakpoint; native editing/IME stays in Rune.
pub fn input(edit: &TextEdit, placeholder: &str, size: &str, t: &T) -> El {
    Input::new(edit)
        .placeholder(placeholder)
        .size(size)
        .build(t)
}
pub fn separator(t: &T) -> El {
    el().role("separator")
        .w_full()
        .h(1.)
        .fill(t.c.contrast_border)
}
pub fn kbd(label: &str, t: &T) -> El {
    row()
        .role("note")
        .center()
        .h(20.)
        .px(4.)
        .radius(4.)
        .fill(t.c.contrast_foreground.alpha_mul(0.08))
        .child(
            text(label)
                .font(theme::XS_MEDIUM)
                .color(t.c.contrast_foreground),
        )
}
pub fn skeleton(w: f32, h: f32, t: &T) -> El {
    el().size(w, h).radius(6.).fill(t.c.accent)
}
pub fn empty(title: &str, description: &str, t: &T) -> El {
    col()
        .w_full()
        .center()
        .padding(24.)
        .gap(8.)
        .child(
            text(title)
                .font(theme::SM_MEDIUM)
                .color(t.c.contrast_foreground),
        )
        .child(
            text(description)
                .font(theme::TEXT_SM)
                .color(t.c.contrast_muted_foreground),
        )
}
/// Six-pixel app scrollbar, including the distinct hover token.
pub fn scroll_area(area: &str, viewport: El, t: &T) -> El {
    viewport.key(area).scroll_bar_styled(
        rune_kit::ScrollBarStyle::from_kit(&theme(t))
            .thumb(t.c.app_scrollbar_thumb)
            .thumb_hover(t.c.app_scrollbar_thumb_hover)
            .width(6.)
            .hover_width(6.),
    )
}
pub fn badge(label: &str, variant: &str, size: &str, t: &T) -> El {
    let c = t.c;
    let (bg, fg) = match variant {
        "secondary" => (c.secondary, c.contrast_secondary_foreground),
        "outline" => (c.background, c.contrast_foreground),
        "destructive" => (c.destructive, rune::Color::WHITE),
        "error" => (c.destructive.alpha_mul(0.16), c.destructive_foreground),
        "success" => (c.success.alpha_mul(0.16), c.success_foreground),
        "info" => (c.info.alpha_mul(0.16), c.info_foreground),
        "warning" => (c.warning.alpha_mul(0.16), c.warning_foreground),
        "label" => (c.muted, c.contrast_muted_foreground),
        _ => (c.primary, c.primary_foreground),
    };
    let (h, px, font) = match size {
        "control" => (24., 7., theme::XS_MEDIUM),
        "lg" => (22., 5., theme::SM_MEDIUM),
        "sm" => (16., 3., theme::weight(theme::TEXT_3XS, 500.)),
        _ => (18., 3., theme::XS_MEDIUM),
    };
    row()
        .role("status")
        .center()
        .h(h)
        .px(px)
        .radius(4.)
        .fill(bg)
        .border(
            1.,
            if variant == "outline" {
                c.contrast_input
            } else {
                rune::Color::TRANSPARENT
            },
        )
        .child(text(label).font(font).color(fg))
}
