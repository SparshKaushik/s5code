//! Web dialog glass and 200 ms scale/fade; Rune owns modal focus trapping and dismissal.
use crate::{
    kit::Button,
    theme::{self, AlphaMul, T},
};
use rune::{El, Transition, col, el, row, text};
pub struct Dialog {
    key: String,
    title: String,
    description: Option<String>,
    body: Vec<El>,
    open: bool,
    width: f32,
    outside: bool,
    close: Option<Box<dyn FnOnce(El) -> El>>,
    button: Option<Box<dyn FnOnce(El) -> El>>,
}
impl Dialog {
    pub fn new(key: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            title: title.into(),
            description: None,
            body: vec![],
            open: false,
            width: 512.,
            outside: true,
            close: None,
            button: None,
        }
    }
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
    pub fn child(mut self, body: El) -> Self {
        self.body.push(body);
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
    /// Alert dialogs require an explicit choice; Escape still dismisses through Rune.
    pub fn alert(mut self) -> Self {
        self.outside = false;
        self
    }
    pub fn on_close<S: 'static>(mut self, f: impl Fn(&mut S) + Clone + 'static) -> Self {
        let button = f.clone();
        self.button = Some(Box::new(move |e| e.on_press(button)));
        let outside = self.outside;
        self.close = Some(Box::new(move |e| e.modal(outside, f)));
        self
    }
    pub fn build(self, t: &T, layer: &mut Vec<El>) {
        if !self.open {
            return;
        }
        let Some(close) = self.close else {
            return;
        };
        let dismiss = Button::new("Close")
            .size("icon-xs")
            .variant("ghost-muted")
            .icon(&crate::icons::X)
            .build(t);
        let dismiss = if let Some(f) = self.button {
            f(dismiss)
        } else {
            dismiss
        };
        let mut heading = col().grow(1.).gap(8.).child(
            text(&self.title)
                .font(theme::weight(theme::TEXT_LG, 600.))
                .color(t.c.contrast_foreground),
        );
        if let Some(description) = self.description {
            heading = heading.child(
                text(description)
                    .font(theme::TEXT_SM)
                    .color(t.c.contrast_muted_foreground),
            );
        }
        let panel = col()
            .key(format!("{}-dialog", self.key))
            .role(if self.outside {
                "dialog"
            } else {
                "alertdialog"
            })
            .label(self.title)
            .w_full()
            .max_w(self.width)
            .radius(16.)
            .padding(24.)
            .gap(24.)
            .fill(t.c.background.alpha_mul(0.96))
            .backdrop_blur(16.)
            .border(1., t.c.contrast_foreground.alpha_mul(0.1))
            .transition(Transition::scale(0.98).combined(Transition::opacity()))
            .child(
                row()
                    .w_full()
                    .items_start()
                    .gap(16.)
                    .child(heading)
                    .child(dismiss),
            )
            .children(self.body);
        layer.push(
            el().key(format!("{}-backdrop", self.key))
                .absolute(0., 0.)
                .w_full()
                .h_full()
                .fill(t.c.background.alpha_mul(if t.dark { 0.64 } else { 0.60 }))
                .backdrop_blur(4.),
        );
        layer.push(
            el().key(format!("{}-layer", self.key))
                .absolute(0., 0.)
                .w_full()
                .h_full()
                .padding(16.)
                .center()
                .child(close(panel).modal_click_outside(self.outside)),
        );
    }
}
