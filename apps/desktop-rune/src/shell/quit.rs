//! Quit confirmation, as the Electron shell's `QuitHold` and the web's `QuitHoldOverlay`:
//! Cmd/Ctrl+Q shows a hint, and quits once held (auto-repeat proves the hold) or pressed twice.
//! The `confirmQuit` client setting picks "hold", "double-click" or "direct".

use rune::{El, Key, KeyEvent, Tx, col, el, row, text};

use crate::App;
use crate::theme::{self, AlphaMul, T};

const HOLD_MS: i64 = 1200;
const DOUBLE_PRESS_MS: i64 = 500;
const RELEASE_GRACE_MS: i64 = 600;
const LINGER_MS: i64 = 1200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Hold,
    DoubleClick,
    Direct,
}

#[derive(Default)]
pub struct QuitHold {
    hint: Option<Mode>,
    held_since: Option<i64>,
    last_press: i64,
    hide_at: Option<i64>,
}

impl QuitHold {
    /// A Cmd/Ctrl+Q press; true quits.
    pub fn press(&mut self, now: i64, repeat: bool, mode: Mode) -> bool {
        if repeat {
            return mode == Mode::Hold && self.held_since.is_some_and(|at| now - at >= HOLD_MS);
        }
        let twice = self.last_press != 0 && now - self.last_press <= DOUBLE_PRESS_MS;
        self.last_press = now;
        if twice || mode == Mode::Direct {
            *self = QuitHold::default();
            return true;
        }
        self.hint = Some(mode);
        let (held_since, shown_for) = match mode {
            Mode::Hold => (Some(now), HOLD_MS + RELEASE_GRACE_MS),
            _ => (None, DOUBLE_PRESS_MS),
        };
        self.held_since = held_since;
        self.hide_at = Some(now + shown_for);
        false
    }

    /// Q released: a hold hint lingers a moment; a double-press hint keeps its deadline.
    pub fn release(&mut self, now: i64) {
        if self.held_since.take().is_some() {
            self.hide_at = Some(now + LINGER_MS);
        }
    }

    /// Any other fresh key cancels the hint and the pending double press.
    pub fn cancel(&mut self) {
        *self = QuitHold::default();
    }

    /// Hides an expired hint; true when that changed what draws.
    pub fn tick(&mut self, now: i64) -> bool {
        if self.hide_at.is_some_and(|at| now >= at) {
            let shown = self.hint.is_some();
            let last_press = self.last_press;
            *self = QuitHold { last_press, ..QuitHold::default() };
            return shown;
        }
        false
    }

    pub fn hint(&self) -> Option<Mode> {
        self.hint
    }
}

fn mode(s: &App) -> Mode {
    let value = s.client_settings.values.get("confirmQuit").or_else(|| crate::settings::client::defaults().get("confirmQuit"));
    match value.and_then(serde_json::Value::as_str) {
        Some("double-click") => Mode::DoubleClick,
        Some("direct") => Mode::Direct,
        // The legacy boolean: true held, false quit at once.
        _ if value.and_then(serde_json::Value::as_bool) == Some(false) => Mode::Direct,
        _ => Mode::Hold,
    }
}

fn is_quit(e: &KeyEvent) -> bool {
    e.key == Key::Command('q') && !e.mods.alt && !e.mods.shift
}

/// Electron's `before-input-event` guard: a held Cmd/Ctrl+W closes one tab or terminal, not
/// each one behind it, so its auto-repeats go nowhere.
pub fn held_close(e: &KeyEvent, repeat: bool) -> bool {
    repeat && e.key == Key::Command('w') && !e.mods.alt && !e.mods.shift
}

/// The native app menu's Quit (`MenuItem::new(title, quit::menu_quit)`). With the item's
/// `keys_first`, Cmd+Q reaches `on_key` and its hold first, so this runs only when Quit is picked
/// in the menu, which quits at once as Electron's `role: "quit"` does.
pub fn menu_quit(s: &mut App, tx: &mut Tx) {
    s.ui.shell.quit.cancel();
    tx.quit();
}

/// Runs before every other key handler, so the press never reaches a text field.
pub fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if held_close(e, tx.key_repeat()) {
        return true;
    }
    let now = crate::data::now_ms();
    if !is_quit(e) {
        if !tx.key_repeat() && s.ui.shell.quit.hint().is_some() {
            s.ui.shell.quit.cancel();
        }
        return false;
    }
    if s.ui.shell.quit.press(now, tx.key_repeat(), mode(s)) {
        tx.quit();
    } else {
        tx.wake_after(std::time::Duration::from_millis((HOLD_MS + RELEASE_GRACE_MS) as u64));
    }
    true
}

/// The root's key-up handler: the logical key is `Command('q')` while Cmd/Ctrl is still down.
pub fn on_key_up(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if !matches!(&e.key, Key::Command('q')) && e.key != Key::Text("q".into()) {
        return false;
    }
    s.ui.shell.quit.release(crate::data::now_ms());
    tx.wake_after(std::time::Duration::from_millis(LINGER_MS as u64));
    false
}

pub fn poll(s: &mut App) -> bool {
    s.ui.shell.quit.tick(crate::data::now_ms())
}

pub fn overlay(s: &App, t: &T) -> Option<El> {
    let hint = s.ui.shell.quit.hint()?;
    let shortcut = if cfg!(target_os = "macos") { "⌘Q" } else { "Ctrl+Q" };
    let message = match hint {
        Mode::Hold => format!("Hold {shortcut} or press twice to quit"),
        _ => format!("Press {shortcut} again to quit"),
    };
    let pill = el()
        .px(32.0)
        .py(16.0)
        .radius(999.0)
        .fill(t.c.foreground.alpha_mul(0.95))
        .shadow(20.0, 25.0, rune::Color::BLACK.alpha_mul(0.1))
        .shadow(8.0, 10.0, rune::Color::BLACK.alpha_mul(0.1))
        .child(text(message).font(theme::weight(theme::TEXT_2XL, theme::BOLD)).color(t.c.background));
    Some(
        col()
            .key("quit-hold")
            .transition(rune::Transition::none())
            .role("status")
            .inert()
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .child(el().grow(22.0))
            .child(row().w_full().justify_center().child(crate::kit::motion::popup(pill)))
            .child(el().grow(78.0)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_repeats_of_a_held_close_chord_are_swallowed() {
        let mut w = KeyEvent::from(Key::Command('w'));
        w.mods.cmd = true;
        w.mods.ctrl = !cfg!(target_os = "macos");
        assert!(!held_close(&w, false), "the first press closes");
        assert!(held_close(&w, true));
        w.mods.shift = true;
        assert!(!held_close(&w, true), "Shift+W is another chord");
        assert!(!held_close(&KeyEvent::from(Key::Command('q')), true));
    }

    #[test]
    fn a_held_q_quits_after_the_hold_and_a_release_cancels_it() {
        let mut q = QuitHold::default();
        assert!(!q.press(1_000, false, Mode::Hold));
        assert_eq!(q.hint(), Some(Mode::Hold));
        assert!(!q.press(1_500, true, Mode::Hold), "a repeat before the hold elapses waits");
        assert!(q.press(2_200, true, Mode::Hold), "a repeat after 1.2 s quits");

        let mut q = QuitHold::default();
        q.press(1_000, false, Mode::Hold);
        q.release(1_100);
        assert!(!q.press(2_300, true, Mode::Hold), "released, a stray repeat never quits");
        assert!(!q.tick(2_299));
        assert!(q.tick(2_300), "the hint lingers 1.2 s after release");
        assert_eq!(q.hint(), None);
    }

    #[test]
    fn two_quick_presses_quit_and_other_keys_reset() {
        let mut q = QuitHold::default();
        assert!(!q.press(1_000, false, Mode::DoubleClick));
        assert_eq!(q.hint(), Some(Mode::DoubleClick));
        assert!(q.press(1_400, false, Mode::DoubleClick));

        let mut q = QuitHold::default();
        q.press(1_000, false, Mode::Hold);
        q.release(1_050);
        assert!(q.press(1_300, false, Mode::Hold), "hold mode also quits on a double press");

        let mut q = QuitHold::default();
        q.press(1_000, false, Mode::DoubleClick);
        assert!(q.tick(1_500), "the double-press hint ends with its window");
        assert!(!q.press(1_600, false, Mode::DoubleClick), "too slow for a double press");

        let mut q = QuitHold::default();
        q.press(1_000, false, Mode::Hold);
        q.cancel();
        assert!(!q.press(1_200, false, Mode::Hold), "another key between presses breaks the pair");

        assert!(QuitHold::default().press(1_000, false, Mode::Direct));
    }
}
