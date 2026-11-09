//! Cmd/Ctrl+Q under the `confirmQuit` setting, as apps/desktop/src/window/QuitHold.ts. Rune
//! reports no key-up, so a hold ends when its auto-repeats stop for a grace period.

const HOLD_MS: i64 = 1200;
const DOUBLE_PRESS_MS: i64 = 500;
const RELEASE_GRACE_MS: i64 = 600;
/// QuitHoldOverlay keeps the hold hint up this long after the key is let go.
const HINT_LINGER_MS: i64 = 1200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Direct,
    Hold,
    Double,
}

impl Mode {
    pub fn from_setting(value: Option<&str>) -> Mode {
        match value {
            Some("direct") => Mode::Direct,
            Some("double-click") => Mode::Double,
            _ => Mode::Hold,
        }
    }
}

#[derive(Default, Debug)]
pub struct QuitHold {
    last_press: Option<i64>,
    held_since: Option<i64>,
    last_repeat: Option<i64>,
    cadence: i64,
    quitting: bool,
    hint: Option<(Mode, i64)>,
}

impl QuitHold {
    /// A Cmd/Ctrl+Q press or auto-repeat at `now`; true when the app should quit now.
    pub fn press(&mut self, mode: Mode, repeat: bool, now: i64) -> bool {
        if repeat {
            let Some(since) = self.held_since else { return false };
            self.cadence = now - self.last_repeat.unwrap_or(since);
            self.last_repeat = Some(now);
            if !self.quitting && now - since >= HOLD_MS {
                self.quitting = true;
            }
            self.hint = Some((Mode::Hold, now + self.quiet() + HINT_LINGER_MS));
            return false;
        }
        if self.quitting {
            return false;
        }
        let previous = self.last_press.replace(now);
        self.held_since = None;
        self.last_repeat = None;
        self.cadence = 0;
        // Every mode quits on two quick presses.
        if mode == Mode::Direct || previous.is_some_and(|p| now - p <= DOUBLE_PRESS_MS) {
            *self = QuitHold::default();
            return true;
        }
        if mode == Mode::Hold {
            self.held_since = Some(now);
        }
        let until = if mode == Mode::Hold { now + RELEASE_GRACE_MS + HINT_LINGER_MS } else { now + DOUBLE_PRESS_MS };
        self.hint = Some((mode, until));
        false
    }

    /// Any other key press: it ends the hold and the double-press window. A hold already past
    /// its threshold still quits.
    pub fn other_key(&mut self, now: i64) {
        if self.quitting {
            return;
        }
        self.last_press = None;
        self.held_since = None;
        self.last_repeat = None;
        self.hint = match self.hint {
            Some((Mode::Hold, _)) => Some((Mode::Hold, now + HINT_LINGER_MS)),
            _ => None,
        };
    }

    /// Advances to `now`; true when a completed hold's repeats have gone quiet and the app
    /// should quit.
    pub fn tick(&mut self, now: i64) -> bool {
        if self.quitting && self.quit_at().is_some_and(|at| now >= at) {
            *self = QuitHold::default();
            return true;
        }
        if self.hint.is_some_and(|(_, until)| now >= until) {
            self.hint = None;
        }
        false
    }

    /// When `tick` next has something to do.
    pub fn deadline(&self) -> Option<i64> {
        if self.quitting {
            return self.quit_at();
        }
        self.hint.map(|(_, until)| until)
    }

    pub fn hint(&self) -> Option<Mode> {
        self.hint.map(|(mode, _)| mode)
    }

    /// A slow repeat rate can outlast the fixed grace: wait two observed cadences.
    fn quiet(&self) -> i64 {
        RELEASE_GRACE_MS.max(self.cadence * 2)
    }

    fn quit_at(&self) -> Option<i64> {
        self.last_repeat.or(self.held_since).map(|last| last + self.quiet())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hold_with_repeats(q: &mut QuitHold, from: i64, to: i64, every: i64) {
        let mut t = from;
        while t <= to {
            assert!(!q.press(Mode::Hold, true, t));
            t += every;
        }
    }

    #[test]
    fn direct_quits_on_the_first_press() {
        let mut q = QuitHold::default();
        assert!(q.press(Mode::Direct, false, 0));
        assert_eq!(q.hint(), None);
    }

    #[test]
    fn double_press_quits_only_within_the_window() {
        let mut q = QuitHold::default();
        assert!(!q.press(Mode::Double, false, 0));
        assert_eq!(q.hint(), Some(Mode::Double));
        assert!(!q.tick(500));
        assert_eq!(q.hint(), None);
        assert!(!q.press(Mode::Double, false, 700));
        assert!(q.press(Mode::Double, false, 1100));
    }

    #[test]
    fn hold_mode_also_quits_on_two_quick_presses() {
        let mut q = QuitHold::default();
        assert!(!q.press(Mode::Hold, false, 0));
        assert_eq!(q.hint(), Some(Mode::Hold));
        assert!(q.press(Mode::Hold, false, 300));
    }

    #[test]
    fn another_key_cancels_the_double_press() {
        let mut q = QuitHold::default();
        assert!(!q.press(Mode::Hold, false, 0));
        q.other_key(100);
        assert!(!q.press(Mode::Hold, false, 300));
    }

    #[test]
    fn a_full_hold_quits_once_the_repeats_stop() {
        let mut q = QuitHold::default();
        assert!(!q.press(Mode::Hold, false, 0));
        hold_with_repeats(&mut q, 500, 1250, 50);
        // Still repeating within the grace: no quit yet.
        assert!(!q.tick(1260));
        assert_eq!(q.deadline(), Some(1850));
        assert!(q.tick(1850));
    }

    #[test]
    fn a_short_hold_never_quits_and_its_hint_fades() {
        let mut q = QuitHold::default();
        assert!(!q.press(Mode::Hold, false, 0));
        hold_with_repeats(&mut q, 500, 800, 50);
        for t in [1000, 2000, 2599] {
            assert!(!q.tick(t));
        }
        assert_eq!(q.hint(), Some(Mode::Hold));
        assert!(!q.tick(2600));
        assert_eq!(q.hint(), None);
    }

    #[test]
    fn a_slow_repeat_rate_waits_two_cadences() {
        let mut q = QuitHold::default();
        assert!(!q.press(Mode::Hold, false, 0));
        hold_with_repeats(&mut q, 400, 1200, 400);
        assert!(!q.tick(1200 + 700));
        assert!(q.tick(1200 + 800));
    }
}
