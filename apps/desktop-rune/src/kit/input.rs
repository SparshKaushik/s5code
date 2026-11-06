//! Styled input shell with web sizes; TextEdit retains selection, clipboard, undo and IME.
use crate::theme::{self, AlphaMul, T};
use rune::{El, TextEdit, Tx, text_input};
pub struct Input {
    edit: TextEdit,
    placeholder: String,
    size: String,
    mono: bool,
    invalid: bool,
    disabled: bool,
    actions: Vec<Box<dyn FnOnce(El) -> El>>,
}
impl Input {
    pub fn new(edit: &TextEdit) -> Self {
        Self {
            edit: edit.clone(),
            placeholder: String::new(),
            size: "default".into(),
            mono: false,
            invalid: false,
            disabled: false,
            actions: vec![],
        }
    }
    pub fn placeholder(mut self, label: impl Into<String>) -> Self {
        self.placeholder = label.into();
        self
    }
    pub fn size(mut self, size: &str) -> Self {
        self.size = size.into();
        self
    }
    pub fn font(mut self, font: &str) -> Self {
        self.mono = font == "mono";
        self
    }
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn on_edit<S: 'static>(
        mut self,
        f: impl for<'a> Fn(&'a mut S) -> &'a mut TextEdit + 'static,
    ) -> Self {
        self.actions.push(Box::new(move |e| e.on_edit(f)));
        self
    }
    pub fn on_change<S: 'static>(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.actions.push(Box::new(move |e| e.on_input_change(f)));
        self
    }
    pub fn on_submit<S: 'static>(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.actions.push(Box::new(move |e| e.on_submit(f)));
        self
    }
    pub fn build(self, t: &T) -> El {
        let (h, padding) = match self.size.as_str() {
            "sm" => (26., 9.),
            "compact" => (28., 9.),
            "lg" => (34., 11.),
            _ => (30., 11.),
        };
        let font = if self.size == "compact" {
            theme::TEXT_XS
        } else {
            theme::TEXT_SM
        };
        let font = if self.mono { theme::mono(font) } else { font };
        let mut e = text_input(&self.edit, font)
            .role("textbox")
            .label(self.placeholder.clone())
            .placeholder(self.placeholder, t.c.contrast_placeholder)
            .w_full()
            .h(h)
            .px(padding)
            .py((h - font.line_height) / 2.)
            .radius(if self.size == "compact" { 6. } else { 8. })
            .fill(if t.dark {
                t.c.input.alpha_mul(0.32)
            } else {
                t.c.background
            })
            .border(
                1.,
                if self.invalid {
                    t.c.destructive.alpha_mul(0.36)
                } else {
                    t.c.contrast_input
                },
            )
            .color(t.c.contrast_foreground)
            .focus_ring(3., -3., t.c.ring.alpha_mul(0.24));
        if self.disabled {
            return e.disabled(true).opacity(0.64);
        }
        for action in self.actions {
            e = action(e);
        }
        e
    }
}
