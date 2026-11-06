//! SegmentedControl (`@opencode/ui` navigation/segmented-control): equal segments over a layer
//! with a sliding raised indicator.

/// How wide the control is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Width {
    Points(f32),
    /// The parent's width.
    Fill,
    /// Equal segments, each as wide as the widest label needs.
    Fit,
}

use rune::{El, TextStyle, Tx, el, row, text};

use crate::theme::AlphaMul;
use rune_kit::{Kit, State};

type Pick = Box<dyn Fn(usize) -> Box<dyn FnOnce(El) -> El>>;

/// `SegmentedControl::new(&["General","Advanced"], Some(0)).build(&kit)`.
pub struct SegmentedControl {
    labels: Vec<String>,
    selected: Option<usize>,
    width: Width,
    states: Vec<State>,
    pick: Option<Pick>,
    label: Option<String>,
}

impl SegmentedControl {
    pub fn new<S: AsRef<str>>(labels: &[S], selected: Option<usize>) -> SegmentedControl {
        SegmentedControl {
            labels: labels.iter().map(|l| l.as_ref().to_owned()).collect(),
            selected,
            width: Width::Points(232.0),
            states: Vec::new(),
            pick: None,
            label: None,
        }
    }
    /// The accessible name of the choice the segments make ("View").
    pub fn label(mut self, l: impl Into<String>) -> Self {
        self.label = Some(l.into());
        self
    }
    /// Width in points (default 232); `None` fills the parent.
    pub fn width(mut self, w: Option<f32>) -> Self {
        self.width = w.map_or(Width::Fill, Width::Points);
        self
    }
    /// Equal segments, each as wide as the widest label needs (any language, any font).
    pub fn fit(mut self) -> Self {
        self.width = Width::Fit;
        self
    }
    /// Force a segment's visual state (hover, focus, disabled).
    pub fn segment_state(mut self, i: usize, s: State) -> Self {
        if self.states.len() <= i {
            self.states.resize(i + 1, State::Rest);
        }
        self.states[i] = s;
        self
    }
    pub fn on_pick<S: 'static>(
        mut self,
        f: impl Fn(&mut S, usize, &mut Tx) + Clone + 'static,
    ) -> Self {
        self.pick = Some(Box::new(move |i| {
            let f = f.clone();
            Box::new(move |e: El| e.on_press_tx(move |s: &mut S, tx: &mut Tx| f(s, i, tx)))
        }));
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let c = &k.c;
        let n = self.labels.len().max(1);
        let font = TextStyle {
            line_height: 16.0,
            ..k.ty.small_medium
        };
        let mut root = row()
            .role("radiogroup")
            .focus_group()
            .h(28.0)
            .padding(2.)
            .gap(2.)
            .center_y()
            .radius(8.0)
            .fill(c.border_weak.alpha_mul(0.4));
        if let Some(l) = self.label {
            root = root.label(l);
        }
        root = match self.width {
            Width::Points(w) => root.w(w),
            Width::Fill => root.w_full(),
            Width::Fit => root.equal_columns(n),
        };
        let indicator = || {
            el().key("indicator")
                .h(24.0)
                .radius(6.0)
                .fill(if k.dark {
                    c.border_weak.alpha_mul(0.72)
                } else {
                    c.page
                })
                .box_shadow(rune::Shadow {
                    y: 1.,
                    blur: 2.,
                    color: rune::Color::BLACK.alpha_mul(0.1),
                    ..Default::default()
                })
                .token_motion(k.motion.control)
                .a11y_hidden()
        };
        match (self.selected, self.width) {
            (Some(sel), Width::Points(w)) => {
                let seg = (w - 4. - 2. * (n - 1) as f32) / n as f32;
                root = root.child(
                    indicator()
                        .w(seg)
                        .absolute(2. + (seg + 2.) * sel as f32, 2.0),
                );
            }
            // The indicator shares the selected segment's cell and slides between cells.
            (Some(sel), Width::Fit) => root = root.child(indicator().grid_cell(sel)),
            _ => {}
        }
        for (i, l) in self.labels.into_iter().enumerate() {
            let st = self.states.get(i).copied().unwrap_or_default();
            let on = self.selected == Some(i);
            let ink = if on || st == State::Hover {
                c.text_base
            } else {
                c.text_muted
            };
            // An unselected segment lights under the pointer on the first frame and fades as it
            // leaves (the runtime's default hover motion); the selected one already reads raised.
            let hovers = st == State::Rest && !on && self.pick.is_some();
            let mut label = text(l.clone())
                .key("label")
                .font(font)
                .color(ink)
                .truncate()
                .a11y_hidden();
            if hovers {
                label = label.group_color(c.text_base);
            }
            let mut seg = row()
                .key(format!("segment{i}"))
                .role("radio")
                .focusable(st != State::Disabled)
                .focus_selected(on)
                .focus_ring(2.0, 1.0, c.border_focus)
                .disabled(st == State::Disabled)
                .label(l.clone())
                .position_in_set(i + 1, n)
                .grow(1.0)
                .shrink()
                .h(24.0)
                .px(10.0)
                .center()
                .radius(6.0)
                .child(label);
            if self.width == Width::Fit {
                seg = seg.grid_cell(i);
            }
            if self.width == Width::Fill && on {
                seg = seg
                    .fill(if k.dark {
                        c.border_weak.alpha_mul(0.72)
                    } else {
                        c.page
                    })
                    .box_shadow(rune::Shadow {
                        spread: 0.5,
                        color: c.border_strong,
                        ..Default::default()
                    });
            }
            match st {
                State::Focus => seg = seg.outline(2.0, 1.0, c.border_focus),
                State::Disabled => seg = seg.dim(0.45),
                State::Hover if !on => seg = seg.fill(c.hover),
                _ => {}
            }
            if hovers {
                seg = seg.hover(c.hover).press(c.pressed);
            }
            if st != State::Disabled
                && let Some(p) = &self.pick
            {
                seg = p(i)(seg);
            }
            root = root.child(seg);
        }
        root
    }
}
