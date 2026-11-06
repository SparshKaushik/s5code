use rune::{El, Tx};
type Apply = Box<dyn FnOnce(El) -> El>;

/// Handlers collected by a builder and attached to its root once built (unless disabled).
#[derive(Default)]
pub(crate) struct Acts(Vec<Apply>);

impl Acts {
    pub(super) fn transform(&mut self, f: Box<dyn FnOnce(El) -> El>) {
        self.0.push(f);
    }
    pub(crate) fn press<S: 'static>(&mut self, f: impl Fn(&mut S) + 'static) {
        self.0.push(Box::new(move |e: El| e.on_press(f)));
    }
    pub(crate) fn press_tx<S: 'static>(&mut self, f: impl Fn(&mut S, &mut Tx) + 'static) {
        self.0.push(Box::new(move |e: El| e.on_press_tx(f)));
    }
    pub(crate) fn apply(self, mut e: El, enabled: bool) -> El {
        if enabled {
            for f in self.0 {
                e = f(e);
            }
        }
        e
    }
}
