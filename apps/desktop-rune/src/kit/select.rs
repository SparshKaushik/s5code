//! Select fields compose the same list renderer as dropdown menus.
use super::{Menu, MenuItem};
use crate::{icons, theme};
use rune::{El, Tx, icon, row, text};
use rune_kit::{Kit, Placement, State};

type Choose = Box<dyn Fn(&str) -> Box<dyn FnOnce(El) -> El>>;
/// `Select::new("theme", &["System","Light","Dark"]).current(Some("System")).build(&kit, &mut layer)`.
pub struct Select {
    key: String,
    options: Vec<String>,
    current: Option<String>,
    disabled_options: Vec<String>,
    placeholder: String,
    label: Option<String>,
    open: bool,
    state: State,
    width: f32,
    size: String,
    ghost: bool,
    invalid: bool,
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
            disabled_options: vec![],
            placeholder: "Select".into(),
            label: None,
            open: false,
            state: State::Rest,
            width: 180.,
            size: "default".into(),
            ghost: false,
            invalid: false,
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
        let name = self.current.as_deref().unwrap_or(&self.placeholder);
        let mut trigger = row()
            .role("combobox")
            .label(self.label.unwrap_or_else(|| name.into()))
            .expanded(self.open)
            .focusable(!disabled)
            .disabled(disabled)
            .w(self.width)
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
        let mut menu = Menu::new(self.key)
            .role("listbox")
            .trigger(trigger)
            .min_width(self.width)
            .placement(Placement::BottomStart)
            .open(self.open && !disabled);
        for option in self.options {
            let mut item = MenuItem::new(&option)
                .option(self.current.as_ref() == Some(&option))
                .disabled(self.disabled_options.contains(&option));
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
