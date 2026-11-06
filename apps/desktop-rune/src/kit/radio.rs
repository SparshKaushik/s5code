//! Web radio items with Rune arrow-key group navigation.

use rune::{El, Shadow, TextStyle, Tx, col, el, row, text};

use super::actions::Acts;
use crate::theme::AlphaMul;
use rune_kit::{Kit, State};

/// `RadioItem::new("Default", true).on_select(handler)`.
pub struct RadioItem {
    label: String,
    description: Option<String>,
    checked: bool,
    state: State,
    invalid: bool,
    acts: Acts,
}

impl RadioItem {
    pub fn new(label: impl Into<String>, checked: bool) -> RadioItem {
        RadioItem {
            label: label.into(),
            description: None,
            checked,
            state: State::Rest,
            invalid: false,
            acts: Acts::default(),
        }
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
    pub fn on_select<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.acts.press(f);
        self
    }
    pub fn on_select_tx<S: 'static>(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.acts.press_tx(f);
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let c = &k.c;
        let disabled = self.state == State::Disabled;
        let mut control = el()
            .key("control")
            .size(16., 16.)
            .absolute(0., 2.)
            .radius(8.)
            .fill(if self.checked { c.bg_accent } else { c.bg_base })
            .border(
                1.,
                if self.checked {
                    c.bg_accent
                } else {
                    c.border_weak
                },
            )
            .focus_ring(2., -2., c.border_focus);
        if self.checked {
            control = control.child(
                el().size(6., 6.)
                    .absolute(5., 5.)
                    .radius(3.)
                    .fill(c.text_contrast),
            );
        }
        if self.state == State::Focus {
            control = control.outline(2., -2., c.border_focus);
        }
        let control = if disabled {
            control.opacity(0.64)
        } else {
            control
        };
        let label_color = c.text_base;
        let mut words = col().key("text").gap(6.0).child(
            text(self.label.clone())
                .key("label")
                .font(TextStyle {
                    line_height: 20.0,
                    ..k.ty.ui
                })
                .color(label_color),
        );
        if let Some(d) = &self.description {
            let color = if disabled { c.text_faint } else { c.text_muted };
            words = words.child(
                text(d.clone())
                    .key("description")
                    .font(k.ty.small.lh(11.0))
                    .color(color),
            );
        }
        // Checked follows `focus_selected`; the label and description say what the words do.
        let root = row()
            .role("radio")
            .checked(self.checked)
            .label(self.label)
            .description(self.description.unwrap_or_default())
            .gap(8.0)
            .items_start()
            .child(
                el().key("stack")
                    .size(16.0, 20.0)
                    .child(control)
                    .a11y_hidden(),
            )
            .child(words.a11y_hidden());
        self.acts.apply(
            root.focusable(!disabled)
                .focus_selected(self.checked)
                .disabled(disabled),
            !disabled,
        )
    }
}

/// `RadioGroup::new().item(RadioItem::new("Default", true)).build(&kit)`.
pub struct RadioGroup {
    label: Option<String>,
    description: Option<String>,
    items: Vec<RadioItem>,
    disabled: bool,
    invalid: bool,
}

impl RadioGroup {
    pub fn new() -> RadioGroup {
        RadioGroup {
            label: None,
            description: None,
            items: Vec::new(),
            disabled: false,
            invalid: false,
        }
    }
    pub fn label(mut self, l: impl Into<String>) -> Self {
        self.label = Some(l.into());
        self
    }
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.description = Some(d.into());
        self
    }
    pub fn item(mut self, i: RadioItem) -> Self {
        self.items.push(i);
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    pub fn invalid(mut self, i: bool) -> Self {
        self.invalid = i;
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let c = &k.c;
        let mut root = col().role("radiogroup").focus_group().gap(8.0);
        // The group's name and description say its label and the line under it.
        if let Some(l) = &self.label {
            root = root.label(l.clone()).child(
                text(l.clone())
                    .key("label")
                    .font(k.ty.small.lh(11.0))
                    .color(c.text_faint)
                    .a11y_hidden(),
            );
        }
        if let Some(d) = &self.description {
            root = root.description(d.clone()).child(
                text(d.clone())
                    .key("description")
                    .font(k.ty.small.lh(13.2))
                    .color(c.text_faint)
                    .a11y_hidden(),
            );
        }
        let mut items = col().key("items").gap(12.0);
        let n = self.items.len();
        for (i, mut item) in self.items.into_iter().enumerate() {
            if self.disabled {
                item = item.disabled(true);
            }
            item.invalid = self.invalid;
            items = items.child(
                item.build(k)
                    .key(format!("item{i}"))
                    .position_in_set(i + 1, n),
            );
        }
        root.child(items).disabled(self.disabled)
    }
}

impl Default for RadioGroup {
    fn default() -> Self {
        Self::new()
    }
}
