//! Dropdowns use web geometry; context menus share entries and use macOS geometry.
use super::actions::Acts;
use crate::theme::{self, AlphaMul};
use rune::{El, Icon, PointerPress, Shadow, Tx, col, el, icon, row, text};
use rune_kit::{Kit, Placement, State};
use std::any::Any;
use std::rc::Rc;

/// An item's action, typed by the state it was given, for a native pop-up menu to call back.
type Pick = Rc<dyn Fn(&mut dyn Any, &mut Tx)>;
type NativePopup = Rc<dyn Fn(rune::PopupMenu, rune::PopupAnchor, Rc<Vec<(String, Pick)>>) -> El>;

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
    badge: Option<(&'static Icon, String)>,
    checked: Option<bool>,
    radio: bool,
    option: bool,
    hide_indicator: bool,
    inset: bool,
    touch: bool,
    danger: bool,
    state: State,
    sub: Option<(Vec<MenuEntry>, bool)>,
    sub_width: f32,
    sub_keys: Option<Box<dyn FnOnce(El) -> El>>,
    leading: Option<El>,
    trailing: Option<El>,
    description: Option<String>,
    acts: Acts,
    pick: Option<Pick>,
}
impl MenuItem {
    #[cfg(test)]
    pub fn label_text(&self) -> &str {
        &self.label
    }
    #[cfg(test)]
    pub fn is_disabled(&self) -> bool {
        self.state == State::Disabled
    }
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            glyph: None,
            shortcut: None,
            badge: None,
            checked: None,
            radio: false,
            option: false,
            hide_indicator: false,
            inset: false,
            touch: false,
            danger: false,
            state: State::Rest,
            sub: None,
            sub_width: 160.,
            sub_keys: None,
            leading: None,
            trailing: None,
            description: None,
            acts: Acts::default(),
            pick: None,
        }
    }
    pub fn icon(mut self, glyph: &'static Icon) -> Self {
        self.glyph = Some(glyph);
        self
    }
    /// Custom content before the label in place of an icon, such as a project favicon.
    pub fn leading(mut self, value: El) -> Self {
        self.leading = Some(value);
        self
    }
    /// Custom content after the label, such as a row's own pin button.
    /// A second, muted line under the label; the item's icon then sits inline with the label,
    /// like web's `SelectItem` with a `grid gap-0.5` body.
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = Some(value.into());
        self
    }
    pub fn trailing(mut self, value: El) -> Self {
        self.trailing = Some(value);
        self
    }
    pub fn shortcut(mut self, value: impl Into<String>) -> Self {
        self.shortcut = Some(value.into());
        self
    }
    /// A muted glyph after the label, described by `help` (the web's `ProjectEnvironmentBadge`).
    pub fn trailing_icon(mut self, glyph: &'static Icon, help: impl Into<String>) -> Self {
        self.badge = Some((glyph, help.into()));
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
    /// Web `SelectItem hideIndicator`: the picked option shows by its fill alone, without a check.
    pub fn hide_indicator(mut self) -> Self {
        self.hide_indicator = true;
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
        let f = Rc::new(f);
        let pick = f.clone();
        self.pick = Some(Rc::new(move |s: &mut dyn Any, _: &mut Tx| {
            if let Some(s) = s.downcast_mut::<S>() {
                pick(s);
            }
        }));
        self.acts.press(move |s: &mut S| f(s));
        self
    }
    pub fn on_select_tx<S: 'static>(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        let f = Rc::new(f);
        let pick = f.clone();
        self.pick = Some(Rc::new(move |s: &mut dyn Any, tx: &mut Tx| {
            if let Some(s) = s.downcast_mut::<S>() {
                pick(s, tx);
            }
        }));
        self.acts.press_tx(move |s: &mut S, tx: &mut Tx| f(s, tx));
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

    fn build(mut self, k: &Kit, native: bool, key: String, layer: &mut Vec<El>) -> El {
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
        let description = self.description.take();
        let inline = if description.is_some() { self.glyph.take() } else { None };
        if let Some(leading) = self.leading {
            e = e.child(leading);
        } else if let Some(glyph) = self.glyph {
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
        let label = text(self.label)
            .font(if description.is_some() { rune::TextStyle { weight: 500., ..font } } else { font })
            .color(fg)
            .grow(1.)
            .truncate()
            .group_color(if native {
                rune::Color::WHITE
            } else {
                k.c.text_base
            });
        e = match description {
            Some(description) => e.child(
                col()
                    .grow(1.)
                    .min_w(0.)
                    .gap(2.)
                    .child(
                        row()
                            .w_full()
                            .gap(6.)
                            .center_y()
                            .children(inline.map(|glyph| icon(glyph, 14.).color(muted)))
                            .child(label),
                    )
                    .child(text(description).font(theme::TEXT_XS.lh(16.)).color(k.c.text_muted).wrap()),
            ),
            None => e.child(label),
        };
        if let Some((glyph, help)) = self.badge {
            e = e.child(icon(glyph, 14.).color(muted).role("img").label(help.clone()).help(help));
        }
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
        if self.radio && !native && !self.hide_indicator {
            e = e.child(mark(self.checked == Some(true)));
        }
        if let Some(trailing) = self.trailing {
            // The row is the hover track, so a trailing control can reveal itself on row hover.
            e = e.hover_group().child(trailing);
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
    popup: Option<NativePopup>,
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
            popup: None,
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
        let dismissed = close.clone();
        self.popup = Some(Rc::new(move |menu: rune::PopupMenu, anchor: rune::PopupAnchor, picks: Rc<Vec<(String, Pick)>>| {
            let dismissed = dismissed.clone();
            let request = Rc::new(move |tx: &mut Tx| {
                let (dismissed, picks) = (dismissed.clone(), picks.clone());
                tx.popup_menu(menu.clone(), anchor.clone(), move |s: &mut S, picked, tx| {
                    match picked.and_then(|id| picks.iter().find(|(item, _)| item == id)) {
                        // The item's own handler closes the menu, as a drawn menu's row does.
                        Some((_, pick)) => pick(s, tx),
                        None => dismissed(s),
                    }
                });
            });
            el().absolute(0., 0.).size(0., 0.).on_layout_tx(move |_: &mut S, _, tx| request(tx))
        }));
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
    /// On macOS an open `native(true)` menu opens as an `NSMenu` through rune's pop-up menu: a
    /// zero-size probe asks once its first layout runs. Menus it can't express stay drawn.
    fn native_popup(&self, trigger: &str) -> Option<El> {
        let (menu, anchor, probe, picks) = self.native_request(trigger)?;
        let popup = self.popup.as_ref()?;
        Some(popup(menu, anchor, Rc::new(picks)).key(probe))
    }
    /// The menu, where it opens and its probe's key; `None` where it stays drawn.
    fn native_request(&self, trigger: &str) -> Option<(rune::PopupMenu, rune::PopupAnchor, String, Vec<(String, Pick)>)> {
        if !self.native || !rune::popup_menu_supported() || self.popup.is_none() {
            return None;
        }
        let (menu, picks) = native_menu(&self.entries)?;
        let (anchor, probe) = match self.at {
            Some((x, y)) => (rune::PopupAnchor::Point(x, y), format!("{}-native-{}-{}", self.key, x.round(), y.round())),
            None => (rune::PopupAnchor::Key(trigger.to_owned()), format!("{}-native", self.key)),
        };
        Some((menu, anchor, probe, picks))
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
        if let Some(probe) = self.native_popup(&key) {
            layer.push(probe);
            return trigger;
        }
        let dismiss = self.dismiss.take();
        let (anchor, place, gap) = if let Some((x, y)) = self.at {
            let (x, y) = crate::desktop::from_window(x, y);
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

/// The native form of a menu's entries, with each item's action by id; `None` when an entry
/// only a drawn menu shows (custom rows, or an enabled item with no action).
fn native_menu(entries: &[MenuEntry]) -> Option<(rune::PopupMenu, Vec<(String, Pick)>)> {
    fn walk(entries: &[MenuEntry], picks: &mut Vec<(String, Pick)>, next: &mut usize) -> Option<rune::PopupMenu> {
        let mut menu = rune::PopupMenu::new();
        for entry in entries {
            menu = match entry {
                MenuEntry::Separator => menu.separator(),
                MenuEntry::Label(label) => {
                    *next += 1;
                    menu.item(rune::PopupItem::new(format!("label{next}"), label).enabled(false))
                }
                MenuEntry::Row(_) => return None,
                MenuEntry::Item(item) => {
                    if let Some((sub, _)) = &item.sub {
                        menu.submenu(item.label.clone(), walk(sub, picks, next)?)
                    } else {
                        *next += 1;
                        let id = format!("item{next}");
                        let enabled = item.state != State::Disabled;
                        match (&item.pick, enabled) {
                            (Some(pick), true) => picks.push((id.clone(), pick.clone())),
                            (None, true) => return None,
                            _ => {}
                        }
                        let key = item.shortcut.as_deref().and_then(key_equivalent);
                        let title = match (&item.shortcut, &key) {
                            (Some(hint), None) => format!("{}  {hint}", item.label),
                            _ => item.label.clone(),
                        };
                        let mut native = rune::PopupItem::new(id, title).enabled(enabled).checked(item.checked == Some(true));
                        if let Some(key) = key {
                            native = native.key_equivalent(key);
                        }
                        menu.item(native)
                    }
                }
            };
        }
        Some(menu)
    }
    let mut picks = vec![];
    let menu = walk(entries, &mut picks, &mut 0)?;
    Some((menu, picks))
}

/// A shortcut hint as an accelerator: `⌘⇧N` or `Ctrl+N`; free text such as `1280 × 720` is not one.
fn key_equivalent(hint: &str) -> Option<rune::Shortcut> {
    let mut parts = vec![];
    let mut rest = String::new();
    for c in hint.chars() {
        match c {
            '⌘' => parts.push("cmd".to_owned()),
            '⇧' => parts.push("shift".to_owned()),
            '⌥' => parts.push("alt".to_owned()),
            '⌃' => parts.push("ctrl".to_owned()),
            c => rest.push(c),
        }
    }
    if !rest.is_empty() {
        parts.push(rest);
    }
    parts.join("+").parse().ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    struct App(Vec<&'static str>);


    fn titles(menu: &rune::PopupMenu, ids: &[&str]) -> Vec<(String, bool, bool)> {
        ids.iter().map(|id| {
            let item = menu.find(id).unwrap();
            (item.title().to_owned(), item.is_enabled(), item.is_checked())
        }).collect()
    }

    #[test]
    fn a_native_menu_carries_items_checks_submenus_and_actions() {
        let entries = vec![
            MenuEntry::item(MenuItem::new("Rename").shortcut("⌘R").on_select(|s: &mut App| s.0.push("rename"))),
            MenuEntry::Separator,
            MenuEntry::item(MenuItem::new("Pinned").checked(true).on_select_tx(|s: &mut App, _: &mut Tx| s.0.push("pin"))),
            MenuEntry::item(MenuItem::new("Desktop").shortcut("1280 × 720").disabled(true)),
            MenuEntry::item(MenuItem::new("Move to").submenu(
                vec![MenuEntry::item(MenuItem::new("Inbox").on_select(|s: &mut App| s.0.push("inbox")))],
                false,
            )),
        ];
        let (menu, picks) = native_menu(&entries).unwrap();
        assert_eq!(
            titles(&menu, &["item1", "item2", "item3", "item4"]),
            [
                ("Rename".into(), true, false),
                ("Pinned".into(), true, true),
                ("Desktop  1280 × 720".into(), false, false),
                ("Inbox".into(), true, false),
            ]
        );
        assert_eq!(picks.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["item1", "item2", "item4"]);
    }

    #[test]
    fn menus_with_custom_rows_or_inert_items_stay_drawn() {
        assert!(native_menu(&[MenuEntry::Row(el())]).is_none());
        assert!(native_menu(&[MenuEntry::item(MenuItem::new("Nothing"))]).is_none());
        assert!(native_menu(&[MenuEntry::item(MenuItem::new("Off").disabled(true))]).is_some());
    }

    fn menu(native: bool) -> Menu {
        Menu::new("m").native(native).open(true).at(10.0, 20.0).on_close(|_: &mut App| {}).item(MenuItem::new("Open").on_select(|s: &mut App| s.0.push("open")))
    }

    #[test]
    fn an_open_native_menu_asks_for_an_nsmenu_only_where_rune_has_one() {
        assert_eq!(menu(true).native_popup("m-trigger").is_some(), rune::popup_menu_supported());
        assert!(menu(false).native_popup("m-trigger").is_none(), "a dropdown stays drawn");
        assert!(menu(true).row(el()).native_popup("m-trigger").is_none(), "custom rows stay drawn");
        let unclosable = Menu::new("m").native(true).open(true).item(MenuItem::new("Open").on_select(|_: &mut App| {}));
        assert!(unclosable.native_popup("m-trigger").is_none(), "without on_close a dismissal has nowhere to go");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_opens_native_menus_at_the_press_point() {
        assert!(rune::popup_menu_supported());
        let (native, anchor, probe, picks) = menu(true).native_request("m-trigger").expect("an NSMenu request");
        assert_eq!((anchor, probe.as_str()), (rune::PopupAnchor::Point(10.0, 20.0), "m-native-10-20"));
        assert_eq!(native.find("item1").map(|i| i.title()), Some("Open"));
        assert_eq!(picks.len(), 1);
        let dropdown = Menu::new("d").native(true).open(true).on_close(|_: &mut App| {}).item(MenuItem::new("Open").on_select(|_: &mut App| {}));
        let (_, anchor, ..) = dropdown.native_request("d-trigger").expect("an NSMenu request");
        assert_eq!(anchor, rune::PopupAnchor::Key("d-trigger".into()), "a dropdown drops from its trigger");
    }

    #[test]
    fn shortcut_glyphs_become_accelerators() {
        assert_eq!(key_equivalent("⌘⇧N"), "shift+cmd+n".parse().ok());
        assert_eq!(key_equivalent("Ctrl+N"), "ctrl+n".parse().ok());
        assert_eq!(key_equivalent("1280 × 720"), None);
    }
}
