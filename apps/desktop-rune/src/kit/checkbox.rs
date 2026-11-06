//! Checkbox (`@opencode/ui` forms/checkbox).

use rune::{Color, El, Icon, Tx, col, el, icon, row, text};

use super::actions::Acts;
use crate::{
    icons,
    theme::{self, AlphaMul},
};
use rune_kit::{Kit, State};
static CHECK: Icon = Icon {
    name: "web-checkbox-check",
    view: 24.,
    body: r#"<path d="M5.252 12.7 10.2 18.63 18.748 5.37" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// Web checkbox: `Checkbox::new(true).label("Enabled").build(&kit)`.
pub struct Checkbox {
    label: Option<String>,
    checked: bool,
    indeterminate: bool,
    hide_label: bool,
    description: Option<String>,
    glyph: Option<&'static Icon>,
    state: State,
    acts: Acts,
}

impl Checkbox {
    pub fn new(checked: bool) -> Checkbox {
        Checkbox {
            label: None,
            checked,
            indeterminate: false,
            hide_label: false,
            description: None,
            glyph: None,
            state: State::Rest,
            acts: Acts::default(),
        }
    }
    pub fn label(mut self, l: impl Into<String>) -> Self {
        self.label = Some(l.into());
        self
    }
    pub fn hide_label(mut self) -> Self {
        self.hide_label = true;
        self
    }
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.description = Some(d.into());
        self
    }
    pub fn indeterminate(mut self, on: bool) -> Self {
        self.indeterminate = on;
        self
    }
    /// Replace the check mark (the web's `icon` prop), drawn at 12 points.
    pub fn icon(mut self, i: &'static Icon) -> Self {
        self.glyph = Some(i);
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
    pub fn on_toggle<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.acts.press(f);
        self
    }
    pub fn on_toggle_tx<S: 'static>(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.acts.press_tx(f);
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let on = self.checked || self.indeterminate;
        let disabled = self.state == State::Disabled;
        let mut control = el()
            .key("control")
            .size(16., 16.)
            .radius(4.)
            .center()
            .fill(if on { k.c.bg_accent } else { k.c.bg_base })
            .border(1., if on { k.c.bg_accent } else { k.c.border_weak })
            .focus_ring(2., -2., k.c.border_focus);
        if on {
            control = control.child(
                icon(
                    self.glyph.unwrap_or(if self.indeterminate {
                        &icons::MINUS
                    } else {
                        &CHECK
                    }),
                    12.,
                )
                .color(k.c.text_contrast),
            );
        }
        if self.state == State::Focus {
            control = control.outline(2., -2., k.c.border_focus);
        }
        let control = if disabled {
            control.opacity(0.64)
        } else {
            control
        };
        let mut root = row().role("checkbox").gap(8.).center_y().child(control);
        root = if self.indeterminate {
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
