//! Settings range control: six-pixel track, sixteen-pixel thumb; pointer and native slider keys.
use crate::theme::{AlphaMul, T};
use rune::{El, Tx, el, row};
use std::rc::Rc;
pub struct Slider {
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    width: f32,
    label: String,
    disabled: bool,
    bind: Option<Box<dyn FnOnce(El) -> El>>,
}
impl Slider {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            min: 0.,
            max: 100.,
            step: 1.,
            width: 240.,
            label: String::new(),
            disabled: false,
            bind: None,
        }
    }
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = min;
        self.max = max.max(min);
        self
    }
    pub fn step(mut self, step: f32) -> Self {
        self.step = step.max(f32::EPSILON);
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(16.);
        self
    }
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn on_change<S: 'static>(mut self, f: impl Fn(&mut S, f32, &mut Tx) + 'static) -> Self {
        let (min, max, step, value) = (self.min, self.max, self.step, self.value);
        let f = Rc::new(f);
        self.bind = Some(Box::new(move |e| {
            let drag = f.clone();
            let keys = f.clone();
            let limit = f.clone();
            e.on_drag(move |s: &mut S, d, tx| {
                if !d.cancelled() {
                    let raw = min + (max - min) * ((d.x - 8.) / (d.w - 16.).max(1.)).clamp(0., 1.);
                    drag(
                        s,
                        (min + ((raw - min) / step).round() * step).clamp(min, max),
                        tx,
                    );
                }
            })
            .on_step(move |s: &mut S, delta, tx| {
                keys(s, (value + delta as f32 * step).clamp(min, max), tx)
            })
            .on_step_limit(move |s: &mut S, end, tx| limit(s, if end { max } else { min }, tx))
        }));
        self
    }
    pub fn build(self, t: &T) -> El {
        let fraction =
            ((self.value - self.min) / (self.max - self.min).max(f32::EPSILON)).clamp(0., 1.);
        let x = fraction * (self.width - 16.);
        let thumb = el()
            .key("thumb")
            .absolute(x, 4.)
            .size(16., 16.)
            .radius(8.)
            .fill(t.c.background)
            .border(2., t.c.primary)
            .shadow(1., 2., t.c.contrast_foreground.alpha_mul(0.12));
        let mut e = row()
            .role("slider")
            .label(self.label)
            .range_value(self.value as f64, self.min as f64, self.max as f64)
            .w(self.width)
            .h(24.)
            .center_y()
            .focus_ring(2., -2., t.c.ring)
            .child(
                el().w_full()
                    .h(6.)
                    .radius(999.)
                    .fill(t.c.contrast_muted_foreground.alpha_mul(0.22))
                    .border(1., t.c.contrast_border.alpha_mul(0.55))
                    .child(el().w(x + 8.).h(6.).radius(999.).fill(t.c.primary)),
            )
            .child(thumb);
        if self.disabled {
            return e.disabled(true).opacity(0.64);
        }
        if let Some(bind) = self.bind {
            e = bind(e);
        }
        e
    }
}
