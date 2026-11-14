//! Web button variants and desktop breakpoint dimensions. Attach actions through the builder;
//! disabled controls drop handlers, including keyboard activation.
use crate::theme::{self, AlphaMul, T};
use rune::{Color, El, Icon, el, icon, row, text};
use rune_kit::State;

pub const VARIANTS: &[&str] = &[
    "default",
    "destructive",
    "destructive-outline",
    "ghost",
    "ghost-muted",
    "ghost-destructive",
    "glass",
    "link",
    "media-close",
    "media-navigation",
    "outline",
    "overlay",
    "secondary",
    "warning-outline",
];
pub const SIZES: &[&str] = &[
    "compact",
    "default",
    "icon",
    "icon-lg",
    "icon-micro",
    "icon-tiny",
    "icon-sm",
    "icon-xl",
    "icon-xs",
    "lg",
    "micro",
    "multiline",
    "sm",
    "sm-multiline",
    "xl",
    "xs",
];

pub struct Button {
    label: String,
    variant: String,
    size: String,
    glyph: Option<&'static Icon>,
    state: State,
    actions: Vec<Box<dyn FnOnce(El) -> El>>,
}
impl Button {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            variant: "default".into(),
            size: "default".into(),
            glyph: None,
            state: State::Rest,
            actions: vec![],
        }
    }
    pub fn variant(mut self, variant: &str) -> Self {
        self.variant = variant.into();
        self
    }
    pub fn size(mut self, size: &str) -> Self {
        self.size = size.into();
        self
    }
    pub fn icon(mut self, glyph: &'static Icon) -> Self {
        self.glyph = Some(glyph);
        self
    }
    pub fn state(mut self, state: State) -> Self {
        self.state = state;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        if disabled {
            self.state = State::Disabled;
        }
        self
    }
    pub fn on_press<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.actions.push(Box::new(move |e| e.on_press(f)));
        self
    }
    pub fn build(self, t: &T) -> El {
        let c = t.c;
        let (h, px, gap, radius, font, glyph_size) = match self.size.as_str() {
            "compact" => (28., 7., 4., 6., theme::XS_MEDIUM, 14.),
            "micro" | "icon-micro" => {
                let font = theme::weight(theme::TEXT_2XS, 500.);
                (20., 5., 4., 4., rune::TextStyle { size: 11.0 * t.px, ..font }, 12.)
            }
            "icon-tiny" => (16., 0., 0., 8., theme::SM_MEDIUM, 12.),
            "xs" | "icon-xs" => (24., 7., 4., 8., theme::XS_MEDIUM, 14.),
            "sm" | "sm-multiline" | "icon-sm" => (28., 9., 6., 8., theme::SM_MEDIUM, 16.),
            "lg" | "icon-lg" => (36., 13., 8., 8., theme::SM_MEDIUM, 16.),
            "xl" | "icon-xl" => (40., 15., 8., 8., theme::weight(theme::TEXT_BASE, 500.), 18.),
            _ => (32., 11., 8., 8., theme::SM_MEDIUM, 16.),
        };
        let clear = Color::TRANSPARENT;
        let (bg, fg, border, hover) = match self.variant.as_str() {
            "destructive" => (
                c.destructive,
                Color::WHITE,
                c.destructive,
                c.destructive.alpha_mul(0.9),
            ),
            "destructive-outline" => (
                if t.dark {
                    c.input.alpha_mul(0.32)
                } else {
                    c.popover
                },
                c.destructive_foreground,
                c.input,
                c.destructive.alpha_mul(0.04),
            ),
            "outline" => (
                if t.dark {
                    c.input.alpha_mul(0.32)
                } else {
                    c.popover
                },
                c.contrast_foreground,
                c.contrast_input,
                c.accent.alpha_mul(0.5),
            ),
            "secondary" => (
                c.secondary,
                c.contrast_secondary_foreground,
                clear,
                c.secondary.alpha_mul(0.9),
            ),
            "ghost-muted" | "ghost-destructive" => {
                (clear, c.contrast_muted_foreground, clear, c.accent)
            }
            "ghost" | "media-navigation" => (clear, c.contrast_foreground, clear, c.accent),
            "glass" => (
                c.surface_raised,
                c.contrast_foreground,
                c.contrast_border.alpha_mul(0.6),
                c.accent,
            ),
            "link" => (clear, c.contrast_foreground, clear, clear),
            "media-close" => (
                Color::BLACK.alpha(0.65),
                Color::WHITE,
                clear,
                Color::BLACK.alpha(0.8),
            ),
            "overlay" => (
                Color::BLACK.alpha(0.7),
                Color::WHITE.alpha(0.65),
                clear,
                Color::BLACK.alpha(0.9),
            ),
            "warning-outline" => (
                c.warning_surface,
                c.warning_foreground,
                c.warning.alpha_mul(0.32),
                c.warning.alpha_mul(0.16),
            ),
            _ => (
                c.primary,
                c.primary_foreground,
                c.primary,
                c.primary.alpha_mul(0.9),
            ),
        };
        let active = matches!(self.state, State::Hover | State::Pressed);
        let fg = if active && self.variant == "ghost-destructive" {
            c.destructive
        } else if active && self.variant == "ghost-muted" {
            c.contrast_foreground
        } else {
            fg
        };
        let mut e = row()
            .role("button")
            .label(self.label.clone())
            .h(h)
            .gap(gap)
            .center()
            .radius(if self.variant == "glass" {
                999.
            } else {
                radius
            })
            .fill(if active { hover } else { bg })
            .border(1., border)
            .color(fg);
        if self.size.starts_with("icon") {
            e = e.w(h);
        } else {
            e = e.px(px);
        }
        if let Some(g) = self.glyph {
            let glyph = icon(g, glyph_size).mx(-2.).color(fg).a11y_hidden();
            e = e.child(if std::ptr::eq(g, &crate::icons::LOADER_CIRCLE) { glyph.looping(rune::LoopSpec::rotation(1.)) } else { glyph });
        }
        if !self.size.starts_with("icon") {
            e = e.child(text(self.label).font(font).color(fg).a11y_hidden());
        }
        if self.state == State::Disabled {
            return e.disabled(true).opacity(0.64);
        }
        e = e
            .press_scale(0.97)
            .focusable(true)
            .focus_ring(2., -2., c.ring)
            .hover(if bg.a > 0. && hover.a < 1. {
                rune::Color {
                    r: hover.r * hover.a + t.c.background.r * (1. - hover.a),
                    g: hover.g * hover.a + t.c.background.g * (1. - hover.a),
                    b: hover.b * hover.a + t.c.background.b * (1. - hover.a),
                    a: 1.,
                }
            } else {
                hover
            })
            .press(c.contrast_foreground.alpha_mul(0.04));
        if self.state == State::Pressed {
            e = e.scale(0.97);
        }
        if self.state == State::Focus {
            e = e.outline(2., -2., c.ring);
        }
        for action in self.actions {
            e = action(e);
        }
        e
    }
}
/// Icon-only buttons use the same variants and sizes as Button, with an accessible label.
pub fn icon_button(glyph: &'static Icon, label: &str) -> Button {
    Button::new(label).icon(glyph).size("icon").variant("ghost")
}
