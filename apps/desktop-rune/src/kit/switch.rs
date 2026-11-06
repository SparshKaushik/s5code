//! Desktop breakpoint switch metrics from the web kit.

use rune::{El, Tx, el, row, text};

use super::actions::Acts;
use crate::theme::{self, AlphaMul};
use rune_kit::{Kit, State};

/// Web switch: `Switch::new(true).size("sm").build(&kit)`.
pub struct Switch {
    label: Option<String>,
    checked: bool,
    hide_label: bool,
    description: Option<String>,
    state: State,
    acts: Acts,
    small: bool,
    mixed: bool,
}

impl Switch {
    pub fn new(checked: bool) -> Switch {
        Switch {
            label: None,
            checked,
            hide_label: false,
            description: None,
            state: State::Rest,
            acts: Acts::default(),
            small: false,
            mixed: false,
        }
    }
    pub fn label(mut self, l: impl Into<String>) -> Self {
        self.label = Some(l.into());
        self
    }
    /// The visible label stays for accessibility only.
    pub fn hide_label(mut self) -> Self {
        self.hide_label = true;
        self
    }
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.description = Some(d.into());
        self
    }
    pub fn state(mut self, s: State) -> Self {
        self.state = s;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        if d {
            self.state = State::Disabled;
        }
        self
    }
    /// Called with the switch's press (flip your own state there).
    pub fn on_toggle<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.acts.press(f);
        self
    }
    pub fn on_toggle_tx<S: 'static>(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.acts.press_tx(f);
        self
    }

    pub fn size(mut self, size: &str) -> Self {
        self.small = size == "sm";
        self
    }
    pub fn mixed(mut self, mixed: bool) -> Self {
        self.mixed = mixed;
        self
    }
    pub fn build(self, k: &Kit) -> El {
        let disabled = self.state == State::Disabled;
        let thumb = if self.small { 14. } else { 16. };
        let distance = thumb - 4.;
        let x = if self.mixed {
            distance / 2.
        } else if self.checked {
            distance
        } else {
            0.
        };
        let control = el()
            .key("control")
            .size(thumb * 2. - 2., thumb + 2.)
            .radius(thumb)
            .fill(if self.checked && !self.mixed {
                k.c.bg_accent
            } else {
                k.c.border_weak
            })
            .focus_ring(2., -2., k.c.border_focus)
            .child(
                el().key("thumb")
                    .absolute(2. + x, 2.)
                    .size(thumb - 2., thumb - 2.)
                    .radius(thumb)
                    .fill(if k.dark && !self.checked {
                        k.c.text_muted
                    } else {
                        k.c.page
                    })
                    .opacity(if self.mixed { 0.7 } else { 1. })
                    .token_motion(k.motion.control),
            );
        let control = if self.state == State::Focus {
            control.outline(2., -2., k.c.border_focus)
        } else {
            control
        };
        let control = if disabled {
            control.opacity(0.64)
        } else {
            control
        };
        let mut root = row().role("switch").gap(8.).center_y().child(control);
        root = if self.mixed {
            root.checked_mixed()
        } else {
            root.checked(self.checked)
        };
        if let Some(label) = self.label {
            root = root.label(label.clone());
            if !self.hide_label {
                root = root.child(text(label).font(theme::TEXT_SM).color(k.c.text_base));
            }
        }
        if let Some(description) = self.description {
            root = root.description(description);
        }

        self.acts
            .apply(root.focusable(!disabled).disabled(disabled), !disabled)
    }
}
