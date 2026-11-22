//! HistorySwipeNavigation: a two-finger horizontal trackpad swipe goes back or forward, with
//! web's circle peeking in from the edge of the main column. Rune's `on_wheel` sees every
//! scroll before the elements, as web's passive capture listener does; the fingers lifting
//! is the wheel's `Ended` phase where Electron has `onTrackpadScrollEnd`.

use crate::theme::{AlphaMul, T};
use crate::{App, icons};
use rune::{El, GesturePhase, Tx, WheelWatch, el, icon, row};

const TRIGGER_DISTANCE: f32 = 120.0;
/// Movement needed before the gesture picks an axis; first events are often diagonal.
const AXIS_LOCK_DISTANCE: f32 = 8.0;
const MOMENTUM_SAMPLE_SIZE: usize = 5;
/// A scroll without phases (X11) ends after this long without a delta.
const IDLE_MS: i64 = 150;
const SIZE: f32 = 56.0;

#[derive(Clone, Debug, PartialEq)]
enum Gesture {
    Pending { y: f32, sum_x: f32, sum_y: f32, in_dialog: bool, scrolls_x: bool },
    Tracking { back: bool, distance: f32, deltas: Vec<f32>, y: f32 },
    /// Not a swipe, or one already decided: the rest of its events are ignored.
    Ignored,
}

#[derive(Default)]
pub struct Swipe {
    gesture: Option<Gesture>,
    /// When the last delta arrived, for scrolls that report no end.
    last: i64,
}

/// What `on_wheel` asks of the app's history.
pub trait History {
    fn can_go(&self, back: bool) -> bool;
    fn go(&mut self, back: bool);
}

impl History for App {
    fn can_go(&self, back: bool) -> bool {
        let ui = &self.ui.shell.sidebar;
        if back { !ui.back.is_empty() } else { !ui.forward.is_empty() }
    }
    fn go(&mut self, back: bool) {
        if back { super::sidebar::go_back(self) } else { super::sidebar::go_forward(self) }
    }
}

/// `isMomentumTail`: macOS momentum after the fingers lift decays by a near-constant ratio per
/// event; finger movement is noisier. Deltas are signed along the swipe.
fn is_momentum_tail(deltas: &[f32]) -> bool {
    if deltas.len() < MOMENTUM_SAMPLE_SIZE {
        return false;
    }
    let recent = &deltas[deltas.len() - MOMENTUM_SAMPLE_SIZE..];
    if recent.iter().any(|d| *d <= 0.0) {
        return false;
    }
    let ratios: Vec<f32> = recent.windows(2).map(|w| w[1] / w[0]).collect();
    let (lo, hi) = ratios.iter().fold((f32::MAX, f32::MIN), |(lo, hi), r| (lo.min(*r), hi.max(*r)));
    ratios.iter().all(|r| (0.7..1.0).contains(r)) && hi - lo < 0.1
}

/// The fingers lifted: navigate if the swipe went far enough, then ignore its momentum.
fn release<H: History>(swipe: &mut Swipe, history: &mut H) {
    if let Some(Gesture::Tracking { back, distance, .. }) = &swipe.gesture
        && *distance >= TRIGGER_DISTANCE
    {
        history.go(*back);
    }
    swipe.gesture = Some(Gesture::Ignored);
}

/// One scroll. Deltas are taken as a browser's: positive `x` scrolls right, the fingers moving
/// left with natural scrolling, which is Rune's `-dx`.
pub fn watch<H: History>(swipe: &mut Swipe, history: &mut H, w: &WheelWatch, now: i64) {
    let ev = &w.event;
    if swipe.gesture.is_some() && ev.phase == GesturePhase::Moved && now - swipe.last > IDLE_MS && !ev.momentum {
        finish(swipe, history);
    }
    swipe.last = now;
    match ev.phase {
        GesturePhase::Began if !ev.momentum => {
            if swipe.gesture == Some(Gesture::Ignored) {
                swipe.gesture = None;
            }
            return;
        }
        // The fingers lifted; the momentum that follows belongs to the same swipe.
        GesturePhase::Ended if !ev.momentum => {
            if matches!(swipe.gesture, Some(Gesture::Tracking { .. })) {
                release(swipe, history);
            }
            swipe.gesture = Some(Gesture::Ignored);
            return;
        }
        GesturePhase::Ended => {
            swipe.gesture = None;
            return;
        }
        _ => {}
    }
    let (dx, dy) = (-ev.dx, -ev.dy);
    let gesture = swipe.gesture.get_or_insert_with(|| {
        if ev.mods.ctrl || !ev.precise || ev.momentum {
            Gesture::Ignored
        } else {
            Gesture::Pending { y: ev.y, sum_x: 0.0, sum_y: 0.0, in_dialog: false, scrolls_x: false }
        }
    });
    let mut lifted = false;
    match gesture {
        Gesture::Ignored => {}
        Gesture::Pending { y, sum_x, sum_y, in_dialog, scrolls_x } => {
            *sum_x += dx;
            *sum_y += dy;
            *in_dialog |= w.in_dialog;
            *scrolls_x |= w.scrolls_x;
            let (y, sum_x, sum_y, refused) = (*y, *sum_x, *sum_y, *in_dialog || *scrolls_x);
            if sum_x.abs() + sum_y.abs() < AXIS_LOCK_DISTANCE {
                return;
            }
            let back = sum_x < 0.0;
            *gesture = if sum_x.abs() <= sum_y.abs() || refused || !history.can_go(back) {
                Gesture::Ignored
            } else {
                Gesture::Tracking { back, distance: sum_x.abs(), deltas: Vec::new(), y }
            };
        }
        Gesture::Tracking { back, distance, deltas, .. } => {
            let delta = if *back { -dx } else { dx };
            if dx == 0.0 && dy == 0.0 {
                lifted = true;
            } else {
                if deltas.len() >= MOMENTUM_SAMPLE_SIZE {
                    deltas.remove(0);
                }
                deltas.push(delta);
                lifted = is_momentum_tail(deltas);
                if !lifted {
                    *distance = (*distance + delta).max(0.0);
                }
            }
        }
    }
    if lifted {
        release(swipe, history);
    }
}

/// A scroll that reported no end went quiet.
fn finish<H: History>(swipe: &mut Swipe, history: &mut H) {
    if matches!(swipe.gesture, Some(Gesture::Tracking { .. })) {
        release(swipe, history);
    }
    swipe.gesture = None;
}

pub fn tracking(s: &App) -> bool {
    matches!(s.ui.shell.swipe.gesture, Some(Gesture::Tracking { .. }))
}

/// `AppSpec::on_wheel`. Settings and Usage open as route modals over the inert frame.
pub fn on_wheel(s: &mut App, w: &WheelWatch, tx: &mut Tx) {
    let mut w = *w;
    w.in_dialog |= matches!(s.route, crate::Route::Settings(_) | crate::Route::Usage);
    let w = &w;
    let mut swipe = std::mem::take(&mut s.ui.shell.swipe);
    let tracking = matches!(swipe.gesture, Some(Gesture::Tracking { .. }));
    watch(&mut swipe, s, w, crate::data::now_ms());
    if swipe.gesture.is_some() && w.event.phase == GesturePhase::Moved {
        tx.wake_after(std::time::Duration::from_millis(IDLE_MS as u64 + 10));
    }
    let changed = tracking || matches!(swipe.gesture, Some(Gesture::Tracking { .. }));
    s.ui.shell.swipe = swipe;
    if changed {
        tx.instant();
    }
}

/// Ends a phaseless scroll once it has gone quiet; true when the indicator changed.
pub fn poll(s: &mut App) -> bool {
    let quiet = crate::data::now_ms() - s.ui.shell.swipe.last > IDLE_MS;
    if !quiet || !matches!(s.ui.shell.swipe.gesture, Some(Gesture::Tracking { .. } | Gesture::Pending { .. })) {
        return false;
    }
    let mut swipe = std::mem::take(&mut s.ui.shell.swipe);
    finish(&mut swipe, s);
    s.ui.shell.swipe = swipe;
    true
}

/// `SwipeIndicator`: a circle pinned to the main column's edge that peeks in up to half its
/// width, its arrow full strength once the swipe would navigate.
pub fn indicator(s: &App, main: Option<[f32; 4]>, t: &T) -> Option<El> {
    let Some(Gesture::Tracking { back, distance, y, .. }) = &s.ui.shell.swipe.gesture else { return None };
    let [left, top, width, height] = main?;
    let progress = (distance / TRIGGER_DISTANCE).min(1.0);
    let hidden = SIZE * (1.0 - progress * 0.5);
    let x = if *back { -hidden } else { width - SIZE + hidden };
    let (_, y) = crate::desktop::from_window(0.0, *y);
    let y = (y - top - SIZE / 2.0).clamp(0.0, (height - SIZE).max(0.0));
    let (fill, color) = if t.dark { (rune::Color::BLACK, rune::Color::WHITE) } else { (t.c.muted, t.c.foreground) };
    let glyph = if *back { &icons::ARROW_LEFT } else { &icons::ARROW_RIGHT };
    let arrow = icon(glyph, 16.0).color(color.alpha_mul(if progress >= 1.0 { 1.0 } else { 0.4 }));
    let circle = row()
        .key("history-swipe")
        .size(SIZE, SIZE)
        .radius(SIZE / 2.0)
        .fill(fill)
        .center_y()
        .child(if *back { el().grow(1.0) } else { el().w(8.0) })
        .child(arrow)
        .child(if *back { el().w(8.0) } else { el().grow(1.0) })
        .absolute(x, y);
    Some(el().pin(Some(left), Some(top), None, None).size(width, height).clip_rounded(0.0).child(circle))
}

/// Render scenes: a swipe `distance` points along, the pointer at mid height.
pub(crate) fn scene(s: &mut App, back: bool, distance: f32) {
    s.ui.shell.swipe.gesture = Some(Gesture::Tracking { back, distance, deltas: Vec::new(), y: 420.0 });
    // Never quiet, so `poll` doesn't end the gesture before the scene draws.
    s.ui.shell.swipe.last = i64::MAX;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Nav {
        back: bool,
        forward: bool,
        went: Vec<bool>,
    }
    impl History for Nav {
        fn can_go(&self, back: bool) -> bool {
            if back { self.back } else { self.forward }
        }
        fn go(&mut self, back: bool) {
            self.went.push(back);
        }
    }

    fn scroll(dx: f32, dy: f32, phase: GesturePhase) -> WheelWatch {
        let mut w = WheelWatch::default();
        w.event = rune::WheelEvent::new(dx, dy).phase(phase).precise(true);
        w
    }

    /// Fingers down, `deltas` of Rune's `dx` (positive moves content right: a swipe back), up.
    fn swipe(s: &mut Swipe, nav: &mut Nav, deltas: &[f32], patch: impl Fn(&mut WheelWatch)) {
        watch(s, nav, &scroll(0.0, 0.0, GesturePhase::Began), 0);
        for (i, dx) in deltas.iter().enumerate() {
            let mut w = scroll(*dx, 0.0, GesturePhase::Moved);
            patch(&mut w);
            watch(s, nav, &w, 1 + i as i64);
        }
        watch(s, nav, &scroll(0.0, 0.0, GesturePhase::Ended), 100);
    }

    #[test]
    fn a_long_enough_horizontal_swipe_navigates_once_the_fingers_lift() {
        let mut nav = Nav { back: true, forward: true, ..Nav::default() };
        let mut s = Swipe::default();
        swipe(&mut s, &mut nav, &[10.0, 40.0, 50.0, 30.0], |_| {});
        assert_eq!(nav.went, [true], "fingers moving right go back");
        swipe(&mut s, &mut nav, &[-10.0, -40.0, -30.0], |_| {});
        assert_eq!(nav.went, [true], "80 points is short of the trigger");
        swipe(&mut s, &mut nav, &[-60.0, -70.0], |_| {});
        assert_eq!(nav.went, [true, false]);
    }

    #[test]
    fn scrollers_dialogs_vertical_scrolls_and_empty_history_are_not_swipes() {
        let mut s = Swipe::default();
        let mut nav = Nav { back: true, forward: true, ..Nav::default() };
        swipe(&mut s, &mut nav, &[20.0, 60.0, 60.0], |w| w.scrolls_x = true);
        swipe(&mut s, &mut nav, &[20.0, 60.0, 60.0], |w| w.in_dialog = true);
        swipe(&mut s, &mut nav, &[20.0, 60.0, 60.0], |w| w.event.dy = 40.0);
        swipe(&mut s, &mut nav, &[20.0, 60.0, 60.0], |w| w.event.mods.ctrl = true);
        assert!(nav.went.is_empty());
        let mut none = Nav::default();
        swipe(&mut s, &mut none, &[20.0, 60.0, 60.0], |_| {});
        assert!(none.went.is_empty());
    }

    #[test]
    fn momentum_after_the_fingers_lift_is_not_a_new_swipe() {
        let mut nav = Nav { back: true, forward: true, ..Nav::default() };
        let mut s = Swipe::default();
        swipe(&mut s, &mut nav, &[3.0, 2.0], |_| {});
        let mut coast = scroll(80.0, 0.0, GesturePhase::Began);
        coast.event.momentum = true;
        watch(&mut s, &mut nav, &coast, 101);
        coast.event.phase = GesturePhase::Moved;
        for i in 0..5 {
            watch(&mut s, &mut nav, &coast, 102 + i);
        }
        coast.event.phase = GesturePhase::Ended;
        watch(&mut s, &mut nav, &coast, 110);
        assert!(nav.went.is_empty());
        assert!(is_momentum_tail(&[20.0, 16.0, 13.0, 10.5, 8.5]));
        assert!(!is_momentum_tail(&[20.0, 25.0, 13.0, 10.5, 8.5]));
    }

    #[test]
    fn a_scroll_without_phases_ends_when_it_goes_quiet() {
        let mut nav = Nav { back: true, ..Nav::default() };
        let mut s = Swipe::default();
        for (i, dx) in [20.0, 60.0, 60.0].iter().enumerate() {
            watch(&mut s, &mut nav, &scroll(*dx, 0.0, GesturePhase::Moved), i as i64 * 10);
        }
        assert!(nav.went.is_empty());
        finish(&mut s, &mut nav);
        assert_eq!(nav.went, [true]);
        watch(&mut s, &mut nav, &scroll(3.0, 0.0, GesturePhase::Moved), 1000);
        assert!(matches!(s.gesture, Some(Gesture::Pending { .. })), "the next scroll starts afresh");
    }
}
