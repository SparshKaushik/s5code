//! Dropdowns use web geometry; context menus share entries and use macOS geometry.
use super::actions::Acts;
use crate::theme::{self, AlphaMul};
use rune::{El, Icon, PointerPress, Shadow, Tx, col, el, icon, row, text};
use rune_kit::{Kit, Placement, State};
use std::rc::Rc;

/// A compositional menu row. `MenuEntry::item(MenuItem::new("Open"))`.
pub enum MenuEntry {
    Item(Box<MenuItem>),
    Separator,
    Label(String),
    /// Custom inline content, such as a row of zoom buttons.
    Row(El),
}
impl MenuEntry {
    pub fn item(item: MenuItem) -> Self {
        Self::Item(Box::new(item))
    }
}
/// `MenuItem::new("Delete").variant("destructive").on_select(|s: &mut App| ...)`.
pub struct MenuItem {
    label: String,
    glyph: Option<&'static Icon>,
    shortcut: Option<String>,
    checked: Option<bool>,
    radio: bool,
    option: bool,
    inset: bool,
    touch: bool,
    danger: bool,
    state: State,
    sub: Option<(Vec<MenuEntry>, bool)>,
    sub_width: f32,
    sub_keys: Option<Box<dyn FnOnce(El) -> El>>,
    acts: Acts,
}
impl MenuItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            glyph: None,
            shortcut: None,
            checked: None,
            radio: false,
            option: false,
            inset: false,
            touch: false,
            danger: false,
            state: State::Rest,
            sub: None,
            sub_width: 160.,
            sub_keys: None,
            acts: Acts::default(),
        }
    }
    pub fn icon(mut self, glyph: &'static Icon) -> Self {
        self.glyph = Some(glyph);
        self
    }
    pub fn shortcut(mut self, value: impl Into<String>) -> Self {
        self.shortcut = Some(value.into());
        self
    }
    pub fn variant(mut self, value: &str) -> Self {
        self.danger = value == "destructive";
        self
    }
    pub fn danger(mut self, value: bool) -> Self {
        self.danger = value;
        self
    }
    pub fn checked(mut self, value: bool) -> Self {
        self.checked = Some(value);
        self
    }
    pub fn option(mut self, value: bool) -> Self {
        self.checked = Some(value);
        self.radio = true;
        self.option = true;
        self
    }
    pub fn radio(mut self, value: bool) -> Self {
        self.checked = Some(value);
        self.radio = true;
        self
    }
    pub fn inset(mut self, value: bool) -> Self {
        self.inset = value;
        self
    }
    pub fn density(mut self, value: &str) -> Self {
        self.touch = value == "touch";
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
    pub fn submenu(mut self, entries: Vec<MenuEntry>, open: bool) -> Self {
        self.sub = Some((entries, open));
        self
    }
    /// The submenu's least width; truncating labels otherwise let it shrink to the default.
    pub fn submenu_min_width(mut self, width: f32) -> Self {
        self.sub_width = width;
        self
    }
    pub(super) fn transform(mut self, f: Box<dyn FnOnce(El) -> El>) -> Self {
        self.acts.transform(f);
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

    /// Clear sibling submenu state when the pointer enters a leaf row.
    pub fn on_hover<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.acts.transform(Box::new(move |e| e.on_hover(f)));
        self
    }
    /// Drive the supplied submenu's `open` state from pointer and arrow-key navigation.
    pub fn on_submenu_change<S: 'static>(mut self, f: impl Fn(&mut S, bool) + 'static) -> Self {
        let f = Rc::new(f);
        let close = f.clone();
        self.sub_keys = Some(Box::new(move |e| {
            e.on_key(move |s: &mut S, key, _| {
                if matches!(key.key, rune::Key::Left | rune::Key::Escape) {
                    close(s, false);
                    true
                } else {
                    false
                }
            })
        }));
        let hover = f.clone();
        let press = f.clone();
        self.acts.transform(Box::new(move |e| {
            e.hover_delays(
                std::time::Duration::from_millis(100),
                std::time::Duration::ZERO,
            )
            .on_hover_change(move |s: &mut S, inside, _| {
                if inside {
                    hover(s, true);
                }
            })
            .on_press(move |s: &mut S| press(s, true))
            .on_key(move |s: &mut S, key, _| match key.key {
                rune::Key::Right => {
                    f(s, true);
                    true
                }
                rune::Key::Left => {
                    f(s, false);
                    true
                }
                _ => false,
            })
        }));
        self
    }

    fn build(self, k: &Kit, native: bool, key: String, layer: &mut Vec<El>) -> El {
        let disabled = self.state == State::Disabled;
        let highlighted = matches!(self.state, State::Hover | State::Pressed | State::Focus)
            || self.sub.as_ref().is_some_and(|v| v.1);
        let hover = if native {
            rune::Color {
                r: 0.,
                g: 0.127438,
                b: 0.693872,
                a: 1.,
            }
        } else {
            k.c.surface_hover
        };
        let fg = if native && highlighted {
            rune::Color::WHITE
        } else if self.danger {
            k.c.danger.fg
        } else {
            if native {
                native_colors(k.dark).text
            } else {
                k.c.text_base
            }
        };
        let muted = if native && highlighted {
            fg
        } else if self.danger {
            fg
        } else {
            k.c.icon_muted
        };
        let font = if native {
            rune::TextStyle {
                size: 13.,
                line_height: 18.,
                ..theme::TEXT_SM
            }
        } else {
            theme::TEXT_SM
        };
        let mut e = row()
            .key(key.clone())
            .role(if self.option {
                "option"
            } else if self.radio {
                "menuitemradio"
            } else if self.checked.is_some() {
                "menuitemcheckbox"
            } else {
                "menuitem"
            })
            .label(self.label.clone())
            .w_full()
            .min_h(if native {
                22.
            } else if self.touch {
                40.
            } else {
                28.
            })
            .px(if native { 4. } else { 8. })
            .py(if native { 2. } else { 4. })
            .gap(if native { 4. } else { 8. })
            .center_y()
            .radius(4.)
            .focusable(!disabled)
            .disabled(disabled)
            .focus_selected(self.checked == Some(true));
        if let Some(on) = self.checked {
            e = e.checked(on);
        }
        if highlighted {
            e = e.fill(hover);
        } else if self.radio && self.checked == Some(true) {
            e = e.fill(k.c.text_base.alpha_mul(0.08)).focus_highlight(hover);
        } else if !disabled {
            e = e.focus_highlight(hover);
        }
        let mark = |on| {
            el().size(if native { 12. } else { 16. }, 16.)
                .center()
                .child(
                    icon(&crate::icons::CHECK, 14.)
                        .color(fg)
                        .opacity(if on { 1. } else { 0. }),
                )
        };
        if native || (self.checked.is_some() && !self.radio) {
            e = e.child(mark(self.checked == Some(true)));
        } else if self.inset {
            e = e.child(el().w(16.));
        }
        if let Some(glyph) = self.glyph {
            e = e.child(
                el().size(12., 16.).child(
                    icon(glyph, 16.)
                        .absolute(-2., 0.)
                        .color(muted)
                        .group_color(if native { rune::Color::WHITE } else { muted })
                        .opacity(0.8),
                ),
            );
        }
        e = e.child(
            text(self.label)
                .font(font)
                .color(fg)
                .grow(1.)
                .truncate()
                .group_color(if native {
                    rune::Color::WHITE
                } else {
                    k.c.text_base
                }),
        );
        if let Some(shortcut) = self.shortcut {
            e = e.shortcut_hint(shortcut.clone()).child(
                text(shortcut)
                    .font(rune::TextStyle {
                        tracking: 1.2,
                        ..theme::XS_MEDIUM
                    })
                    .color(k.c.text_faint)
                    .group_color(if native {
                        rune::Color::WHITE
                    } else {
                        k.c.text_faint
                    }),
            );
        }
        if self.radio && !native {
            e = e.child(mark(self.checked == Some(true)));
        }
        if let Some((entries, open)) = self.sub {
            e = e
                .expanded(open)
                .child(icon(&crate::icons::CHEVRON_RIGHT, 16.).color(muted));
            if open {
                let sub = Menu::new(format!("{key}-sub"))
                    .entries(entries)
                    .min_width(self.sub_width)
                    .native(native);
                let sub = if native { sub.width(198.) } else { sub };
                let mut deeper = vec![];
                let mut surface = sub.surface(k, &mut deeper);
                if let Some(keys) = self.sub_keys {
                    surface = keys(surface);
                }

                layer.push(super::motion::popup(rune_kit::popover::anchored(
                    k,
                    surface.mt(-5.),
                    &key,
                    Placement::RightStart,
                    0.,
                )));
                layer.extend(deeper);
            }
        }
        if disabled {
            e = e.opacity(if native { 0.4 } else { 0.64 });
        }
        self.acts.apply(e, !disabled)
    }
}

/// `Menu::new("file").trigger(button).entries(items).open(open).build(&kit, &mut overlays)`.
/// Callers own open/checked state and close in action handlers; mount overlays in `rune_kit::root`.
pub struct Menu {
    key: String,
    role: &'static str,
    trigger: Option<El>,
    entries: Vec<MenuEntry>,
    open: bool,
    native: bool,
    placement: Placement,
    gutter: f32,
    min_width: f32,
    at: Option<(f32, f32)>,
    anchor: Option<String>,
    width: Option<f32>,
    dismiss: Option<(El, Box<dyn FnOnce(El) -> El>)>,
}
impl Menu {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            role: "menu",
            trigger: None,
            entries: vec![],
            open: false,
            native: false,
            placement: Placement::Bottom,
            gutter: 4.,
            min_width: 160.,
            at: None,
            anchor: None,
            width: None,
            dismiss: None,
        }
    }
    pub fn role(mut self, role: &'static str) -> Self {
        self.role = role;
        self
    }
    pub fn trigger(mut self, value: El) -> Self {
        self.trigger = Some(value);
        self
    }
    pub fn entries(mut self, value: Vec<MenuEntry>) -> Self {
        self.entries = value;
        self
    }
    pub fn item(mut self, value: MenuItem) -> Self {
        self.entries.push(MenuEntry::item(value));
        self
    }
    pub fn separator(mut self) -> Self {
        self.entries.push(MenuEntry::Separator);
        self
    }
    pub fn row(mut self, value: El) -> Self {
        self.entries.push(MenuEntry::Row(value));
        self
    }
    pub fn group_label(mut self, value: impl Into<String>) -> Self {
        self.entries.push(MenuEntry::Label(value.into()));
        self
    }
    pub fn open(mut self, value: bool) -> Self {
        self.open = value;
        self
    }
    pub fn placement(mut self, value: Placement) -> Self {
        self.placement = value;
        self
    }
    pub fn gutter(mut self, value: f32) -> Self {
        self.gutter = value;
        self
    }
    pub fn min_width(mut self, value: f32) -> Self {
        self.min_width = value;
        self
    }
    pub fn at(mut self, x: f32, y: f32) -> Self {
        self.at = Some((x, y));
        self
    }
    /// An already-mounted row anchor, such as the sidebar's snooze button.
    pub fn anchor(mut self, key: impl Into<String>) -> Self {
        self.anchor = Some(key.into());
        self
    }
    /// Fixed width for native menus: the sidebar uses 198 points.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self.min_width = width;
        self
    }
    pub fn native(mut self, value: bool) -> Self {
        self.native = value;
        self
    }
    pub fn on_close<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        let close = Rc::new(f);
        let outside = close.clone();
        self.dismiss = Some((
            el().key(format!("{}-scrim", self.key))
                .absolute(0., 0.)
                .w_full()
                .h_full()
                .on_press(move |s: &mut S| outside(s)),
            Box::new(move |e| e.modal(true, move |s: &mut S| close(s))),
        ));
        self
    }
    fn surface(self, k: &Kit, layer: &mut Vec<El>) -> El {
        let mut rows = col().w_full().padding(5.).items_stretch();
        let native_ink = native_colors(k.dark);
        for (i, entry) in self.entries.into_iter().enumerate() {
            rows = rows.child(match entry {
                MenuEntry::Item(item) => {
                    item.build(k, self.native, format!("{}-item{i}", self.key), layer)
                }
                MenuEntry::Separator => el()
                    .key(format!("sep{i}"))
                    .role("separator")
                    .h(1.)
                    .mx(8.)
                    .my(if self.native { 5. } else { 4. })
                    .fill(if self.native {
                        native_ink.separator
                    } else {
                        k.c.border_base
                    }),
                MenuEntry::Label(label) => row()
                    .key(format!("label{i}"))
                    .px(8.)
                    .py(6.)
                    .child(text(label).font(theme::XS_MEDIUM).color(k.c.text_muted)),
                MenuEntry::Row(content) => row().key(format!("row{i}")).child(content),
            });
        }
        let frame = col()
            .key(format!("{}-menu", self.key))
            .role(self.role)
            .focus_group()
            .min_w(self.min_width)
            .min_h(0.)
            .child(rune_kit::popover::fit_scroll(rows));
        let frame = if let Some(width) = self.width {
            frame.w(width)
        } else {
            frame
        };
        glass(frame, k, self.native)
    }
    /// Mount a menu anchored to an existing row without mounting another trigger.
    pub fn overlay(self, k: &Kit, layer: &mut Vec<El>) {
        self.build(k, layer);
    }
    pub fn build(mut self, k: &Kit, layer: &mut Vec<El>) -> El {
        let key = self
            .anchor
            .clone()
            .unwrap_or_else(|| format!("{}-trigger", self.key));
        let trigger = row()
            .self_start()
            .key(key.clone())
            .children(self.trigger.take().map(|t| t.expanded(self.open)));
        if !self.open {
            return trigger;
        }
        let dismiss = self.dismiss.take();
        let (anchor, place, gap) = if let Some((x, y)) = self.at {
            let point = format!("{}-point", self.key);
            layer.push(el().key(point.clone()).absolute(x, y).size(0., 0.));
            (point, Placement::BottomStart, 0.)
        } else {
            (key.clone(), self.placement, self.gutter)
        };
        let mut subs = vec![];
        let mut surface = self.surface(k, &mut subs).focus_return(key);
        if let Some((scrim, modal)) = dismiss {
            layer.push(scrim);
            surface = modal(surface);
        }
        layer.push(super::motion::popup(rune_kit::popover::anchored(k, surface, &anchor, place, gap)));
        layer.extend(subs);
        trigger
    }
}

/// Same entries as a dropdown: `context_menu("thread", trigger, point).entries(items)`.
/// Store `PointerPress` from `on_secondary_press`; clear it on close and selection.
pub fn context_menu(key: impl Into<String>, trigger: El, point: Option<PointerPress>) -> Menu {
    let menu = Menu::new(key)
        .trigger(trigger)
        .open(point.is_some())
        .native(true)
        .width(198.);
    if let Some(p) = point {
        menu.at(p.x, p.y)
    } else {
        menu
    }
}

struct NativeColors {
    fill: rune::Color,
    border: rune::Color,
    separator: rune::Color,
    text: rune::Color,
}
fn native_colors(dark: bool) -> NativeColors {
    let hex = |s| rune::color::hex(s).unwrap();
    if dark {
        NativeColors {
            fill: hex("#262628"),
            border: hex("#373738"),
            separator: hex("#3c3c3d"),
            text: hex("#dededf"),
        }
    } else {
        NativeColors {
            fill: hex("#f6f6f6"),
            border: hex("#d6d6d6"),
            separator: hex("#dedede"),
            text: hex("#1c1c1e"),
        }
    }
}
/// Appearance > Glass opacity (`--glass-opacity`, percent ×100); settings set it each frame.
static GLASS_OPACITY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(8000);
pub fn set_glass_opacity(percent: f32) {
    GLASS_OPACITY.store((percent.clamp(0.0, 100.0) * 100.0) as u32, std::sync::atomic::Ordering::Relaxed);
}
/// The glass opacity as a fraction (0.8 by default).
pub fn glass_opacity() -> f32 {
    GLASS_OPACITY.load(std::sync::atomic::Ordering::Relaxed) as f32 / 10000.0
}
pub(crate) fn glass(e: El, k: &Kit, native: bool) -> El {
    if native {
        let c = native_colors(k.dark);
        return e
            .radius(6.)
            .fill(c.fill.alpha_mul(0.94))
            .backdrop_blur(16.)
            .border(1., c.border)
            .shadow(8., 24., rune::Color::BLACK.alpha_mul(0.35));
    }
    // dropdown-glass: popover 18% over popover at the glass opacity; 100% also drops the blur.
    let opacity = glass_opacity();
    e.radius(8.)
        .fill(k.c.layer_01.alpha_mul(0.18 + 0.82 * opacity))
        .backdrop_blur(if opacity >= 1.0 { 0. } else if k.dark { 16. } else { 12. })
        .border(1., k.c.text_base.alpha_mul(0.1))
        .box_shadow(Shadow {
            y: if k.dark { 18. } else { 16. },
            blur: if k.dark { 44. } else { 40. },
            spread: -18.,
            color: rune::Color::BLACK.alpha_mul(if k.dark { 0.8 } else { 0.55 }),
            ..Default::default()
        })
}
