//! Web dialog glass with the kit's popup motion; Rune owns modal focus trapping and dismissal.
use crate::{
    kit::Button,
    theme::{self, AlphaMul, T},
};
use rune::{El, col, row, text};
pub struct Dialog {
    key: String,
    title: String,
    description: Option<String>,
    body: Vec<El>,
    open: bool,
    width: f32,
    outside: bool,
    scroll_all: bool,
    close: Option<Box<dyn FnOnce(El) -> El>>,
    button: Option<Box<dyn FnOnce(El) -> El>>,
    glyph: Option<El>,
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
            scroll_all: false,
            close: None,
            button: None,
            glyph: None,
        }
    }
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }
    /// Shown before the title (web `DialogHeader`'s leading icon).
    pub fn title_icon(mut self, glyph: El) -> Self {
        self.glyph = Some(glyph);
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
    /// Web's `WizardPopup`: header, body and footer scroll together once the window cuts the panel.
    pub fn scroll_all(mut self) -> Self {
        self.scroll_all = true;
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
        // Web `DialogPopup`'s close button: `size="icon" variant="ghost"` at `absolute end-2 top-2`;
        // alert dialogs (`AlertDialogPopup`) have none.
        let dismiss = (self.outside).then(|| {
            let dismiss = Button::new("Close").size("icon").variant("ghost").icon(&crate::icons::X).build(t);
            match self.button {
                Some(f) => f(dismiss),
                None => dismiss,
            }
            .pin(None, Some(8.), Some(8.), None)
        });
        let title = text(&self.title)
            .font(theme::weight(theme::TEXT_LG, 600.))
            .color(t.c.contrast_foreground)
            .wrap();
        let mut heading = col().grow(1.).min_w(0.).gap(8.).child(match self.glyph {
            Some(glyph) => row().gap(8.).center_y().child(glyph).child(title.shrink()),
            None => title,
        });
        if let Some(description) = self.description {
            heading = heading.child(
                text(description)
                    .font(theme::TEXT_SM)
                    .color(t.c.contrast_muted_foreground)
                    .wrap(),
            );
        }
        let content = row().w_full().items_start().child(heading);
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
            // Web's `max-h-full`: the panel shrinks to the window and a scrolling body takes the cut.
            .shrink()
            .min_h(0.)
            .radius(16.)
            // Web's dialog-glass without backdrop blur falls back to an opaque popover; Rune has no blur.
            .fill(t.c.popover)
            .border(1., t.c.contrast_foreground.alpha_mul(0.1));
        let panel = if self.scroll_all {
            let scroll = col()
                .key(format!("{}-scroll", self.key))
                .w_full()
                .shrink()
                .min_h(0.)
                .scroll(rune::ScrollSpec { stick_bottom: false })
                // Padding on the scroller itself is not part of its scroll extent.
                .child(col().w_full().padding(24.).gap(24.).child(content).children(self.body));
            panel.child(scroll)
        } else {
            panel.padding(24.).gap(24.).child(content).children(self.body)
        }
        .children(dismiss);
        layer.push(super::motion::scrim(t).key(format!("{}-backdrop", self.key)));
        layer.push(
            super::motion::layer()
                .key(format!("{}-layer", self.key))
                .padding(16.)
                .child(col().w_full().h_full().center().child(super::motion::popup(close(panel).modal_click_outside(self.outside)))),
        );
    }
}
