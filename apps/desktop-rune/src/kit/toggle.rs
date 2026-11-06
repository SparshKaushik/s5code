//! Toggle groups and tabs share spacing but expose their distinct accessibility roles.
use crate::theme::{self, AlphaMul};
use rune::{Color, El, Tx, col, row, text};
use rune_kit::{Kit, State};
type Pick = Box<dyn Fn(usize) -> Box<dyn FnOnce(El) -> El>>;
/// `ToggleGroup::new(&["Code","Diff"], &[0]).variant("segmented").build(&kit)`.
/// Multiple selected indexes implement a multi-select group; callers own selection.
pub struct ToggleGroup {
    labels: Vec<String>,
    selected: Vec<usize>,
    states: Vec<State>,
    variant: String,
    size: String,
    vertical: bool,
    tabs: bool,
    pick: Option<Pick>,
}
impl ToggleGroup {
    pub fn new<S: AsRef<str>>(labels: &[S], selected: &[usize]) -> Self {
        Self {
            labels: labels.iter().map(|v| v.as_ref().into()).collect(),
            selected: selected.into(),
            states: vec![],
            variant: "segmented".into(),
            size: "segmented".into(),
            vertical: false,
            tabs: false,
            pick: None,
        }
    }
    pub fn variant(mut self, value: &str) -> Self {
        self.variant = value.into();
        self
    }
    pub fn size(mut self, value: &str) -> Self {
        self.size = value.into();
        self
    }
    pub fn orientation(mut self, value: &str) -> Self {
        self.vertical = value == "vertical";
        self
    }
    pub fn item_state(mut self, index: usize, value: State) -> Self {
        self.states
            .resize(self.states.len().max(index + 1), State::Rest);
        self.states[index] = value;
        self
    }
    pub fn on_pick<S: 'static>(
        mut self,
        f: impl Fn(&mut S, usize, &mut Tx) + Clone + 'static,
    ) -> Self {
        self.pick = Some(Box::new(move |index| {
            let f = f.clone();
            Box::new(move |e| e.on_press_tx(move |s: &mut S, tx| f(s, index, tx)))
        }));
        self
    }
    pub fn build(self, k: &Kit) -> El {
        let segmented = self.variant == "segmented";
        let mut root = if self.vertical { col() } else { row() };
        root = root
            .role(if self.tabs { "tablist" } else { "group" })
            .focus_group()
            .gap(if self.variant == "outline" { 0. } else { 2. });
        if segmented {
            root = root
                .padding(2.)
                .radius(8.)
                .fill(k.c.border_weak.alpha_mul(0.4));
        }
        let height = match self.size.as_str() {
            "segmented" | "xs" => 24.,
            "compact" | "sm" => 28.,
            "lg" => 36.,
            _ => 32.,
        };
        for (index, label) in self.labels.into_iter().enumerate() {
            let state = self.states.get(index).copied().unwrap_or(State::Rest);
            let selected = self.selected.contains(&index);
            let disabled = state == State::Disabled;
            let fill = if selected {
                if segmented {
                    if k.dark {
                        k.c.border_weak.alpha_mul(0.72)
                    } else {
                        k.c.page
                    }
                } else if self.variant == "ghost" {
                    k.c.surface_hover
                } else {
                    k.c.border_weak.alpha_mul(0.64)
                }
            } else {
                Color::TRANSPARENT
            };
            let mut item = row()
                .key(format!("toggle-{index}"))
                .role(if self.tabs { "tab" } else { "button" })
                .label(label.clone())
                .checked(selected)
                .focus_selected(selected)
                .focusable(!disabled)
                .disabled(disabled)
                .h(height)
                .px(if self.variant == "pill" {
                    12.
                } else if self.size == "segmented" {
                    10.
                } else {
                    7.
                })
                .radius(if self.variant == "pill" {
                    999.
                } else if self.size == "segmented" {
                    6.
                } else {
                    8.
                })
                .fill(fill)
                .border(
                    1.,
                    if self.variant == "outline" {
                        k.c.border_weak
                    } else {
                        Color::TRANSPARENT
                    },
                )
                .center()
                .hover(k.c.surface_hover)
                .focus_ring(2., -2., k.c.border_focus)
                .child(
                    text(label)
                        .font(if height <= 28. {
                            if self.variant == "pill" {
                                theme::TEXT_XS
                            } else {
                                theme::XS_MEDIUM
                            }
                        } else {
                            theme::SM_MEDIUM
                        })
                        .color(if selected {
                            k.c.text_base
                        } else {
                            k.c.text_muted
                        }),
                );
            match state {
                State::Hover | State::Pressed => item = item.fill(k.c.surface_hover),
                State::Focus => item = item.outline(2., -2., k.c.border_focus),
                State::Disabled => item = item.opacity(0.64),
                _ => {}
            }
            if !disabled && let Some(pick) = &self.pick {
                item = pick(index)(item);
            }
            root = root.child(item);
        }
        root
    }
}
/// `Tabs::new(&["General","Advanced"], 0).build(&kit)`; mount the selected panel separately.
pub struct Tabs(ToggleGroup);
impl Tabs {
    pub fn new<S: AsRef<str>>(labels: &[S], selected: usize) -> Self {
        let mut group = ToggleGroup::new(labels, &[selected]);
        group.tabs = true;
        Self(group)
    }
    pub fn on_pick<S: 'static>(
        mut self,
        f: impl Fn(&mut S, usize, &mut Tx) + Clone + 'static,
    ) -> Self {
        self.0 = self.0.on_pick(f);
        self
    }
    pub fn build(self, k: &Kit) -> El {
        self.0.build(k)
    }
}
