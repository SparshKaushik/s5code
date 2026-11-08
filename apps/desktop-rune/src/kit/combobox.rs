//! Searchable select with virtual rows and Rune's IME-safe list keyboard behavior.
use super::{Popover, Select};
use crate::{
    icons,
    theme::{self, AlphaMul},
};
use rune::{Color, El, Key, ScrollSpec, TextEdit, Tx, col, el, icon, row, text, text_input};
use rune_kit::{Kit, Placement, ScrollBar};
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct ComboboxOption {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub disabled: bool,
}
impl ComboboxOption {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            detail: String::new(),
            disabled: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ComboboxState {
    pub open: bool,
    pub search: TextEdit,
    pub active: Option<String>,
    query: String,
}
impl ComboboxState {
    pub fn close(&mut self) {
        self.open = false;
        self.search.focused = false;
        self.search.preedit = None;
        self.search.break_history_group();
    }
    fn active_index(&self, options: &[ComboboxOption], filtered: &[usize]) -> Option<usize> {
        if self.query == self.search.text
            && let Some(index) = filtered
                .iter()
                .copied()
                .find(|i| !options[*i].disabled && Some(&options[*i].id) == self.active.as_ref())
        {
            return Some(index);
        }
        filtered.iter().copied().find(|i| !options[*i].disabled)
    }
}

// Case-insensitive subsequence matching; contiguous matches and word starts rank first.
fn score(query: &str, value: &str) -> Option<i32> {
    let needle: Vec<_> = query.chars().collect();
    let mut n = 0;
    let mut score = 0;
    let mut previous = false;
    let mut boundary = true;
    for c in value.to_lowercase().chars() {
        if n < needle.len() && c == needle[n] {
            score += 10 + if previous { 12 } else { 0 } + if boundary { 8 } else { 0 };
            previous = true;
            n += 1;
        } else {
            previous = false;
            score -= 1;
        }
        boundary = !c.is_alphanumeric();
    }
    (n == needle.len()).then_some(score)
}
fn filtered(options: &[ComboboxOption], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return (0..options.len()).collect();
    }
    let mut matches: Vec<_> = options
        .iter()
        .enumerate()
        .filter_map(|(i, o)| {
            score(&query, &o.label)
                .into_iter()
                .chain(score(&query, &o.detail))
                .max()
                .map(|s| (i, s))
        })
        .collect();
    matches.sort_by_key(|(i, s)| (std::cmp::Reverse(*s), *i));
    matches.into_iter().map(|(i, _)| i).collect()
}

type Access<S> = Rc<dyn for<'a> Fn(&'a mut S) -> &'a mut ComboboxState>;
type Choose<S> = Rc<dyn Fn(&mut S, &str, &mut Tx)>;
type Trailing = Rc<dyn Fn(&ComboboxOption) -> Option<El>>;

/// `Combobox::new("models", &s.combo, &options, |s: &mut App| &mut s.combo)`.
/// Selection is app-controlled through `current` and `on_choose`; search/open/highlight
/// state belongs to the supplied ComboboxState. Only mounted rows are laid out/shaped.
pub struct Combobox<S> {
    key: String,
    state: ComboboxState,
    options: Rc<Vec<ComboboxOption>>,
    access: Access<S>,
    choose: Option<Choose<S>>,
    current: Option<String>,
    placeholder: String,
    search_placeholder: String,
    empty: String,
    width: f32,
    max_height: f32,
    placement: Placement,
    trigger: Option<El>,
    disabled: bool,
    label: Option<String>,
    trailing: Option<Trailing>,
    header: Option<El>,
}
impl<S: 'static> Combobox<S> {
    pub fn new(
        key: impl Into<String>,
        state: &ComboboxState,
        options: &[ComboboxOption],
        access: impl for<'a> Fn(&'a mut S) -> &'a mut ComboboxState + 'static,
    ) -> Self {
        Self {
            key: key.into(),
            state: state.clone(),
            options: Rc::new(options.to_vec()),
            access: Rc::new(access),
            choose: None,
            current: None,
            placeholder: String::new(),
            search_placeholder: "Search".into(),
            empty: "No results".into(),
            width: 280.0,
            max_height: 324.0,
            placement: Placement::BottomStart,
            trigger: None,
            disabled: false,
            label: None,
            trailing: None,
            header: None,
        }
    }
    /// A control after an option's label, such as a switch; its own press doesn't choose the row.
    pub fn trailing(mut self, trailing: impl Fn(&ComboboxOption) -> Option<El> + 'static) -> Self {
        self.trailing = Some(Rc::new(trailing));
        self
    }
    /// A row between the search and the results, such as column titles for `trailing` controls.
    pub fn header(mut self, header: El) -> Self {
        self.header = Some(header);
        self
    }
    /// The accessible name: what the choice is for ("Model"). The chosen option is read as its
    /// value; without it the placeholder names the field.
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }
    pub fn current(mut self, id: Option<&str>) -> Self {
        self.current = id.map(str::to_owned);
        self
    }
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }
    pub fn search_placeholder(mut self, text: impl Into<String>) -> Self {
        self.search_placeholder = text.into();
        self
    }
    pub fn empty_message(mut self, text: impl Into<String>) -> Self {
        self.empty = text.into();
        self
    }
    /// The popover's width, 280 by default; the trigger keeps its own.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
    /// The tallest the result list grows, 192 by default. A window with less room below (or
    /// above) the trigger shortens it further, and it scrolls.
    pub fn max_height(mut self, height: f32) -> Self {
        self.max_height = height;
        self
    }
    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }
    /// Replaces the default field trigger (the chosen label and a chevron) with any element;
    /// pressing it opens and closes the popover.
    pub fn trigger(mut self, trigger: El) -> Self {
        self.trigger = Some(trigger);
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn on_choose(mut self, choose: impl Fn(&mut S, &str, &mut Tx) + 'static) -> Self {
        self.choose = Some(Rc::new(choose));
        self
    }
    pub fn build(self, k: &Kit, layer: &mut Vec<El>) -> El {
        let selected = self
            .options
            .iter()
            .find(|o| Some(&o.id) == self.current.as_ref());
        let access = self.access.clone();
        let current = self.current.clone();
        let key = self.key.clone();
        let open_key = key.clone();
        let name = self
            .label
            .clone()
            .unwrap_or_else(|| self.placeholder.clone());
        let mut trigger = match self.trigger {
            Some(trigger) => trigger,
            None => Select::new::<&str>(format!("{}-value", self.key), &[])
                .current(selected.map(|o| o.label.as_str()))
                .placeholder(self.placeholder)
                .label(name.clone())
                .width(self.width)
                .disabled(self.disabled)
                .trigger(k),
        };
        if !self.disabled {
            trigger = trigger.on_press_tx(move |s: &mut S, tx| {
                let state = access(s);
                state.open = !state.open;
                state.search.clear();
                state.search.focused = state.open;
                state.query.clear();
                state.active = current.clone();
                if state.open
                    && let Some(id) = &current
                {
                    tx.scroll_into_view(format!("{open_key}-option-{id}"));
                }
            });
        }
        let mut popover = Popover::new(&self.key)
            .trigger(trigger)
            .label(name.clone())
            .placement(self.placement)
            .padding(0.0)
            .open(self.state.open && !self.disabled);
        let access = self.access.clone();
        popover = popover.on_close(move |s: &mut S| access(s).close());
        if !self.state.open || self.disabled {
            return popover.build(k, layer);
        }
        let choices = filtered(&self.options, &self.state.search.text);
        let active = self.state.active_index(&self.options, &choices);
        let access = self.access.clone();
        let key_access = self.access.clone();
        let page = ((self.max_height / 28.0) as usize).saturating_sub(1).max(1);
        let options = self.options.clone();
        let choose = self.choose.clone();
        let change_access = self.access.clone();
        let change_options = self.options.clone();
        let change_key = self.key.clone();
        // The search field sits borderless over a divider: the open popover is its frame, and the
        // caret shows its focus.
        let mut field = text_input(&self.state.search, theme::TEXT_SM)
            .w_full()
            .h(26.)
            .pl(20.)
            .py(3.)
            .color(k.c.text_base)
            .placeholder(self.search_placeholder, k.c.text_faint)
            .label(if name.is_empty() {
                "Search".into()
            } else {
                format!("Search {name}")
            });
        if let Some(i) = active {
            field = field.active_descendant(format!("{}-option-{}", self.key, self.options[i].id));
        }
        let field = field
            .on_edit(move |s: &mut S| &mut access(s).search)
            .on_input_change(move |s: &mut S, tx| {
                let state = change_access(s);
                state.active = None;
                state.query = state.search.text.clone();
                if let Some(i) = filtered(&change_options, &state.search.text)
                    .into_iter()
                    .find(|i| !change_options[*i].disabled)
                {
                    tx.scroll_into_view(format!("{change_key}-option-{}", change_options[i].id));
                }
            })
            .on_input_key(move |s: &mut S, ev, tx| {
                let state = key_access(s);
                if state.search.preedit.is_some() {
                    return false;
                }
                if ev.key == Key::Escape {
                    if !tx.key_repeat() {
                        state.close();
                    }
                    return true;
                }
                if ev.mods.alt || ev.mods.logo {
                    return false;
                }
                let choices = filtered(&options, &state.search.text);
                let enabled: Vec<_> = choices
                    .into_iter()
                    .filter(|i| !options[*i].disabled)
                    .collect();
                let active = state.active_index(&options, &enabled);
                if ev.key == Key::Enter {
                    if tx.key_repeat() {
                        return true;
                    }
                    let selected = active.map(|i| options[i].id.clone());
                    state.close();
                    if let (Some(selected), Some(choose)) = (selected, &choose) {
                        choose(s, &selected, tx);
                    }
                    return true;
                }
                let control = ev.mods.ctrl && !ev.mods.shift;
                // Page Up and Down move by the rows the list shows, stopping at the ends; Home
                // and End jump there while the search is empty, else they move the caret.
                let jump = match &ev.key {
                    Key::PageDown | Key::PageUp | Key::Home | Key::End
                        if ev.mods.cmd || ev.mods.ctrl || ev.mods.shift =>
                    {
                        None
                    }
                    Key::PageDown => Some(page as isize),
                    Key::PageUp => Some(-(page as isize)),
                    Key::End if state.search.text.is_empty() => Some(isize::MAX),
                    Key::Home if state.search.text.is_empty() => Some(isize::MIN),
                    _ => None,
                };
                if let Some(jump) = jump {
                    if !enabled.is_empty() {
                        let index = active
                            .and_then(|i| enabled.iter().position(|v| *v == i))
                            .unwrap_or(0) as isize;
                        let next = index
                            .saturating_add(jump)
                            .clamp(0, enabled.len() as isize - 1)
                            as usize;
                        let id = options[enabled[next]].id.clone();
                        tx.scroll_into_view(format!("{key}-option-{id}"));
                        state.active = Some(id);
                        state.query = state.search.text.clone();
                    }
                    return true;
                }
                let delta = match &ev.key {
                    Key::Down if !ev.mods.cmd && !ev.mods.ctrl => 1,
                    Key::Up if !ev.mods.cmd && !ev.mods.ctrl => -1,
                    Key::Text(c) if control && c == "n" => 1,
                    Key::Text(c) if control && c == "p" => -1,
                    Key::Command('n') if control => 1,
                    Key::Command('p') if control => -1,
                    _ => return false,
                };
                if !enabled.is_empty() {
                    let index = active
                        .and_then(|i| enabled.iter().position(|v| *v == i))
                        .unwrap_or(0);
                    let next = (index as isize + delta).rem_euclid(enabled.len() as isize) as usize;
                    let id = options[enabled[next]].id.clone();
                    tx.scroll_into_view(format!("{key}-option-{id}"));
                    state.active = Some(id);
                    state.query = state.search.text.clone();
                }
                true
            })
            .key("search");
        let mut rows = col()
            .w_full()
            .growth(rune::GrowthSpec::default())
            .virtualize(28.)
            .reveal_selected(true);
        for (position, original) in choices.iter().copied().enumerate() {
            let option = &self.options[original];
            let selected = self.current.as_deref() == Some(&option.id);
            let highlighted = active == Some(original);
            let mut entry = row()
                .key(format!("{}-option-{}", self.key, option.id))
                .role("option")
                .label(&option.label)
                .description(&option.detail)
                .checked(selected)
                .position_in_set(position + 1, choices.len())
                .focus_selected(highlighted)
                .w_full()
                .h(28.)
                .px(8.)
                .gap(8.)
                .center_y()
                .radius(4.)
                .fill(if highlighted {
                    k.c.text_base.alpha_mul(0.09)
                } else if selected {
                    k.c.text_base.alpha_mul(0.06)
                } else {
                    Color::TRANSPARENT
                })
                .child(
                    text(&option.label)
                        .font(theme::TEXT_SM)
                        .color(k.c.text_base)
                        .grow(1.)
                        .truncate(),
                );
            if !option.detail.is_empty() {
                entry = entry.child(
                    text(&option.detail)
                        .font(theme::TEXT_SM)
                        .color(k.c.text_muted)
                        .truncate(),
                );
            }
            if let Some(control) = self.trailing.as_ref().and_then(|f| f(option)) {
                entry = entry.child(control);
            }
            entry = entry.child(
                icon(&icons::CHECK, 14.)
                    .color(k.c.text_base)
                    .opacity(if selected { 1. } else { 0. }),
            );
            if option.disabled {
                entry = entry.opacity(0.64).disabled(true);
            } else {
                let access = self.access.clone();
                let id = option.id.clone();
                entry = entry.on_hover(move |s: &mut S| {
                    let state = access(s);
                    state.active = Some(id.clone());
                    state.query = state.search.text.clone();
                });
                let access = self.access.clone();
                let choose = self.choose.clone();
                let id = option.id.clone();
                entry = entry.on_press_tx(move |s: &mut S, tx| {
                    access(s).close();
                    if let Some(choose) = &choose {
                        choose(s, &id, tx);
                    }
                });
            }
            rows = rows.child(entry);
        }
        let count = choices.len();
        let list = col()
            .key(format!("{}-results", self.key))
            .w_full()
            .min_h(0.)
            .shrink()
            .h((count.max(1) as f32 * 28.).min(self.max_height))
            .role("listbox")
            .label(name)
            .preserve_focus_on_press(true)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(if count == 0 {
                row()
                    .w_full()
                    .h(28.)
                    .center()
                    .child(text(self.empty).font(theme::TEXT_SM).color(k.c.text_muted))
            } else {
                rows
            });
        let scrollbar = rune_kit::ScrollBarStyle::from_kit(k)
            .thumb(k.c.text_base.alpha_mul(0.14))
            .thumb_hover(k.c.text_base.alpha_mul(0.25))
            .width(6.)
            .hover_width(6.);
        let search = col()
            .w_full()
            .child(
                row()
                    .w_full()
                    .pb(6.)
                    .child(
                        icon(&icons::SEARCH, 16.)
                            .absolute(0., 6.)
                            .color(k.c.text_muted.alpha_mul(0.55)),
                    )
                    .child(field),
            )
            .child(el().w_full().h(1.).fill(if self.state.search.focused {
                k.c.border_focus
            } else {
                k.c.border_base.alpha_mul(0.7)
            }));
        let body = col()
            .w(self.width)
            .min_h(0.)
            .shrink()
            .child(col().w_full().px(12.).pt(9.).child(search))
            .children(self.header)
            .child(
                col()
                    .w_full()
                    .padding(4.)
                    .shrink()
                    .min_h(0.)
                    .child(list.scroll_bar_styled(scrollbar)),
            );
        popover.child(body).build(k, layer)
    }
}
