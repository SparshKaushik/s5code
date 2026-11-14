//! Select fields compose the same list renderer as dropdown menus.
use super::{Menu, MenuItem};
use crate::{icons, theme};
use rune::{El, Tx, icon, row, text};
use rune_kit::{Kit, Placement, State};

type Choose = Box<dyn Fn(&str) -> Box<dyn FnOnce(El) -> El>>;

fn text_width(label: &str, font: rune::TextStyle) -> f32 {
    rune::text::measure(label, &[], font, None).0
}

/// Menu padding (5 + 5), item padding (8 + 8), the 16 px check slot and its 8 px gap, borders.
const MENU_CHROME: f32 = 52.;
/// `Select::new("theme", &["System","Light","Dark"]).current(Some("System")).build(&kit, &mut layer)`.
pub struct Select {
    key: String,
    options: Vec<String>,
    current: Option<String>,
    display: Option<String>,
    leading: Option<(El, f32)>,
    details: Vec<(String, &'static rune::Icon, String)>,
    disabled_options: Vec<String>,
    placeholder: String,
    label: Option<String>,
    open: bool,
    state: State,
    width: f32,
    size: String,
    ghost: bool,
    invalid: bool,
    align_end: bool,
    toggle: Option<Box<dyn FnOnce(El) -> El>>,
    choose: Option<Choose>,
    close: Option<Box<dyn FnOnce(Menu) -> Menu>>,
}
impl Select {
    pub fn new<S: AsRef<str>>(key: impl Into<String>, options: &[S]) -> Self {
        Self {
            key: key.into(),
            options: options.iter().map(|v| v.as_ref().into()).collect(),
            current: None,
            display: None,
            leading: None,
            details: vec![],
            disabled_options: vec![],
            placeholder: "Select".into(),
            label: None,
            open: false,
            state: State::Rest,
            width: 180.,
            size: "default".into(),
            ghost: false,
            invalid: false,
            align_end: false,
            toggle: None,
            choose: None,
            close: None,
        }
    }
    pub fn disabled_option(mut self, value: impl Into<String>) -> Self {
        self.disabled_options.push(value.into());
        self
    }
    pub fn current(mut self, value: Option<&str>) -> Self {
        self.current = value.map(str::to_owned);
        self
    }
    /// Trigger text when it differs from the chosen option's label (web's `SelectValue` children).
    pub fn display(mut self, value: impl Into<String>) -> Self {
        self.display = Some(value.into());
        self
    }
    /// Shown before the trigger text, such as a status icon or colour dots.
    /// Content before the value, `width` wide, which auto-sized triggers make room for.
    pub fn leading(mut self, value: El, width: f32) -> Self {
        self.leading = Some((value, width));
        self
    }
    /// An option's icon and description line in the popup, which then is at least 256 wide
    /// (web `SelectItem className="min-w-64"` with a `grid gap-0.5` body).
    pub fn option_detail(mut self, value: impl Into<String>, glyph: &'static rune::Icon, description: impl Into<String>) -> Self {
        self.details.push((value.into(), glyph, description.into()));
        self
    }
    pub fn placeholder(mut self, value: impl Into<String>) -> Self {
        self.placeholder = value.into();
        self
    }
    pub fn label(mut self, value: impl Into<String>) -> Self {
        self.label = Some(value.into());
        self
    }
    pub fn open(mut self, value: bool) -> Self {
        self.open = value;
        self
    }
    /// `0.` sizes the trigger to its label, like web's `w-auto`.
    pub fn width(mut self, value: f32) -> Self {
        self.width = value;
        self
    }
    pub fn size(mut self, value: &str) -> Self {
        self.size = value.into();
        self
    }
    pub fn variant(mut self, value: &str) -> Self {
        self.ghost = value == "ghost";
        self
    }
    /// Web `SelectPopup align="end"`: a popup wider than its trigger grows leftwards.
    pub fn align_end(mut self) -> Self {
        self.align_end = true;
        self
    }
    pub fn invalid(mut self, value: bool) -> Self {
        self.invalid = value;
        self
    }
    pub fn state(mut self, value: State) -> Self {
        self.state = value;
        self
    }
    pub fn disabled(mut self, value: bool) -> Self {
        if value {
            self.state = State::Disabled;
        }
        self
    }
    pub fn on_toggle<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.toggle = Some(Box::new(move |e| e.on_press(f)));
        self
    }
    pub fn on_close<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.close = Some(Box::new(move |m| m.on_close(f)));
        self
    }
    pub fn on_choose<S: 'static>(
        mut self,
        f: impl Fn(&mut S, &str, &mut Tx) + Clone + 'static,
    ) -> Self {
        self.choose = Some(Box::new(move |value| {
            let f = f.clone();
            let value = value.to_owned();
            Box::new(move |e| e.on_press_tx(move |s: &mut S, tx| f(s, &value, tx)))
        }));
        self
    }
    pub fn trigger(mut self, k: &Kit) -> El {
        self.open = false;
        self.build(k, &mut vec![])
    }
    pub fn build(self, k: &Kit, layer: &mut Vec<El>) -> El {
        let disabled = self.state == State::Disabled;
        let (height, padding, gap, radius, font) = match self.size.as_str() {
            "compact" => (28., 7., 4., 6., theme::TEXT_XS),
            "xs" => (24., 7., 4., 6., theme::TEXT_XS),
            "sm" => (28., 9., 6., 8., theme::TEXT_SM),
            "lg" => (36., 11., 8., 8., theme::TEXT_SM),
            _ => (32., 11., 8., 8., theme::TEXT_SM),
        };
        let name = self.display.as_deref().or(self.current.as_deref()).unwrap_or(&self.placeholder);
        let width = if self.width > 0. {
            self.width
        } else {
            text_width(name, font) + padding * 2. + gap + 14. + 2. + self.leading.as_ref().map_or(0., |(_, w)| w + gap)
        };
        let mut trigger = row()
            .role("combobox")
            .label(self.label.unwrap_or_else(|| name.into()))
            .expanded(self.open)
            .focusable(!disabled)
            .disabled(disabled)
            .w(width)
            .h(height)
            .px(padding)
            .gap(gap)
            .center_y()
            .radius(radius)
            .fill(if self.ghost {
                rune::Color::TRANSPARENT
            } else {
                k.c.bg_base
            })
            .border(
                1.,
                if self.invalid {
                    k.c.danger.border
                } else if self.ghost {
                    rune::Color::TRANSPARENT
                } else {
                    k.c.border_weak
                },
            )
            .focus_ring(2., -2., k.c.border_focus)
            .hover(k.c.hover)
            .children(self.leading.map(|(e, _)| e))
            .child(
                text(name)
                    .font(font)
                    .color(k.c.text_base)
                    .grow(1.)
                    .truncate(),
            )
            .child(icon(&icons::CHEVRON_DOWN, 14.).color(k.c.icon_muted));
        match self.state {
            State::Hover | State::Pressed => {
                trigger = trigger.hover(k.c.hover).fill(if self.ghost {
                    k.c.surface_hover
                } else {
                    k.c.bg_base
                })
            }
            State::Focus => trigger = trigger.outline(3., -3., k.c.border_focus),
            _ => {}
        }
        if disabled {
            trigger = trigger.opacity(0.64);
        } else if let Some(toggle) = self.toggle {
            trigger = toggle(trigger);
        }
        // Web's popup grows to its longest option; measure only while it is shown.
        let fit = if self.open && !disabled {
            self.options
                .iter()
                .map(|o| text_width(o, theme::TEXT_SM) + MENU_CHROME)
                .fold(width, f32::max)
                .max(if self.details.is_empty() { 0. } else { 256. })
                // The popup sizes to unwrapped text, so a description must fit on one line.
                .max(self.details.iter().map(|(_, _, d)| text_width(d, theme::TEXT_XS) + MENU_CHROME + 4.).fold(0., f32::max))
        } else {
            width
        };
        let mut menu = Menu::new(self.key)
            .role("listbox")
            .trigger(trigger)
            .min_width(fit)
            .placement(if self.align_end {
                Placement::BottomEnd
            } else {
                Placement::BottomStart
            })
            .open(self.open && !disabled);
        for option in self.options {
            let mut item = MenuItem::new(&option)
                .option(self.current.as_ref() == Some(&option))
                .disabled(self.disabled_options.contains(&option));
            if let Some((_, glyph, description)) = self.details.iter().find(|(value, ..)| *value == option) {
                item = item.icon(*glyph).description(description.clone());
            }
            if let Some(choose) = &self.choose {
                let action = choose(&option);
                // The shared row renderer drops this transformation for disabled rows.
                item = item.transform(action);
            }
            menu = menu.item(item);
        }
        if let Some(close) = self.close {
            menu = close(menu);
        }
        menu.build(k, layer)
    }
}
