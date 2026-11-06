//! Deterministic visual states; pass either theme to snap.sh for the same scene.
use crate::{
    App, kit,
    theme::{self, T},
};
use rune::{Cx, El, col, row, text};
pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("kit-dialog", |s| {
        s.ui.shell.palette.gallery = Some("dialog")
    }),
    ("kit-alert-dialog", |s| {
        s.ui.shell.palette.gallery = Some("alert-dialog")
    }),
    ("kit-popover", |s| {
        s.ui.shell.palette.gallery = Some("popover")
    }),
    ("kit-tooltip", |s| {
        s.ui.shell.palette.gallery = Some("tooltip")
    }),
    ("kit-buttons", |s| {
        s.ui.shell.palette.gallery = Some("buttons")
    }),
    ("kit-button-sizes", |s| {
        s.ui.shell.palette.gallery = Some("sizes")
    }),
    ("kit-controls", |s| {
        s.ui.shell.palette.gallery = Some("controls")
    }),
    ("kit-overlays", |s| {
        s.ui.shell.palette.gallery = Some("overlays")
    }),
    ("kit-badges", |s| {
        s.ui.shell.palette.gallery = Some("badges")
    }),
    ("kit-palette", |s| s.ui.shell.palette.open = true),
    ("kit-palette-query", |s| {
        s.ui.shell.palette.open = true;
        s.ui.shell.palette.query = rune::TextEdit::new("replay");
    }),
    ("kit-palette-light", |s| {
        s.appearance = crate::Appearance::Light;
        s.ui.shell.palette.open = true;
    }),
];
pub fn view(s: &App, _cx: &mut Cx<App>, t: &T) -> El {
    let which = s.ui.shell.palette.gallery.unwrap_or("buttons");
    let mut body = col()
        .w_full()
        .h_full()
        .padding(32.)
        .gap(12.)
        .fill(t.c.background)
        .child(
            text(format!("UI kit · {which}"))
                .font(theme::TEXT_XL)
                .color(t.c.contrast_foreground),
        );
    let k = kit::theme(t);
    match which {
        "dialog" | "alert-dialog" => {
            let mut layer = vec![];
            let dialog = kit::Dialog::new("gallery-dialog", "Delete thread?")
                .open(true)
                .description("This action cannot be undone.")
                .child(
                    kit::Button::new("Delete thread")
                        .variant("destructive")
                        .build(t),
                );
            let dialog = if which == "alert-dialog" {
                dialog.alert()
            } else {
                dialog
            };
            dialog
                .on_close(|s: &mut App| s.ui.shell.palette.gallery = None)
                .build(t, &mut layer);
            return rune_kit::root(body, layer);
        }
        "popover" | "tooltip" => {
            let mut layer = vec![];
            let trigger = kit::Button::new("More information")
                .variant("outline")
                .build(t);
            let trigger = if which == "popover" {
                kit::Popover::new("gallery-popover")
                    .trigger(trigger)
                    .open(true)
                    .title("Preferences")
                    .description("Configure this environment.")
                    .build(&k, &mut layer)
            } else {
                kit::Tooltip::new("gallery-tooltip")
                    .trigger(trigger)
                    .text(&k, "More information")
                    .open(true)
                    .build(&k, &mut layer)
            };
            return rune_kit::root(body.child(trigger), layer);
        }
        "buttons" | "sizes" => {
            body=body.child(text("Default                 Hover                 Pressed                 Focus                 Disabled").font(theme::TEXT_XS).color(t.c.contrast_muted_foreground));
            for name in if which == "sizes" {
                kit::button::SIZES
            } else {
                kit::button::VARIANTS
            } {
                let mut line = row().h(40.).gap(12.).center_y().child(
                    text(*name)
                        .w(145.)
                        .font(theme::TEXT_XS)
                        .color(t.c.contrast_muted_foreground),
                );
                for state in [
                    kit::State::Rest,
                    kit::State::Hover,
                    kit::State::Pressed,
                    kit::State::Focus,
                    kit::State::Disabled,
                ] {
                    let b = kit::Button::new("Button")
                        .state(state)
                        .icon(&crate::icons::PLUS);
                    line = line.child(
                        if which == "sizes" {
                            b.size(name)
                        } else {
                            b.variant(name)
                        }
                        .build(t),
                    );
                }
                body = body.child(line);
            }
        }
        "badges" => {
            for variant in [
                "default",
                "secondary",
                "outline",
                "destructive",
                "error",
                "info",
                "success",
                "warning",
                "label",
            ] {
                let mut line = row().gap(16.).center_y().child(
                    text(variant)
                        .w(140.)
                        .font(theme::TEXT_XS)
                        .color(t.c.contrast_muted_foreground),
                );
                for size in ["default", "sm", "lg", "control"] {
                    line = line.child(kit::badge(variant, variant, size, t));
                }
                body = body.child(line);
            }
        }
        "controls" => {
            for state in [
                kit::State::Rest,
                kit::State::Hover,
                kit::State::Pressed,
                kit::State::Focus,
                kit::State::Disabled,
            ] {
                body = body.child(
                    row()
                        .gap(24.)
                        .child(
                            kit::Checkbox::new(false)
                                .label("Checkbox")
                                .state(state)
                                .build(&k),
                        )
                        .child(
                            kit::Checkbox::new(true)
                                .label("Checked")
                                .state(state)
                                .build(&k),
                        )
                        .child(kit::Switch::new(false).label("Off").state(state).build(&k))
                        .child(kit::Switch::new(true).label("On").state(state).build(&k)),
                );
            }
            for size in ["sm", "compact", "default", "lg"] {
                body = body.child(
                    kit::input(&s.ui.shell.palette.gallery_edit, size, size, t)
                        .on_edit(|s: &mut App| &mut s.ui.shell.palette.gallery_edit),
                );
            }
            body = body.child(
                kit::TextArea::new(&s.ui.shell.palette.gallery_edit)
                    .placeholder("Textarea")
                    .rows(3)
                    .on_edit(|s: &mut App| &mut s.ui.shell.palette.gallery_edit)
                    .build(&k),
            );
            body = body.child(kit::Slider::new(65.).label("Contrast").build(t));
            body = body
                .child(
                    kit::SegmentedControl::new(&["General", "Appearance", "Advanced"], Some(0))
                        .build(&k),
                )
                .child(
                    kit::RadioGroup::new()
                        .label("Radio group")
                        .item(kit::RadioItem::new("Default", true))
                        .item(kit::RadioItem::new("Alternative", false))
                        .build(&k),
                )
                .child(kit::Progress::new(Some(0.65)).build(&k))
                .child(kit::skeleton(240., 16., t))
                .child(kit::empty(
                    "No threads yet",
                    "Start a conversation to get going.",
                    t,
                ));
        }
        _ => {
            let mut layer = vec![];
            let trigger = kit::Button::new("Open menu").variant("outline").build(t);
            body = body.child(
                kit::Menu::new("gallery-menu")
                    .trigger(trigger)
                    .open(true)
                    .item(kit::MenuItem::new("New thread").shortcut("⌘N"))
                    .separator()
                    .item(kit::MenuItem::new("Show archived").checked(true))
                    .item(kit::MenuItem::new("Disabled").disabled(true))
                    .item(kit::MenuItem::new("Delete thread").danger(true))
                    .build(&k, &mut layer),
            );
            body = body.child(
                kit::Select::new("gallery-select", &["System", "Light", "Dark"])
                    .current(Some("System"))
                    .build(&k, &mut layer),
            );
            body = body.child(
                kit::Toast::new("saved", "Settings saved")
                    .description("Your preferences have been updated.")
                    .build(&k),
            );
            return rune_kit::root(body, layer);
        }
    }
    body
}
