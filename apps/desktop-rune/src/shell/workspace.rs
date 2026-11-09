//! `workspace.focusNext` / `workspace.focusPrevious` (the web's workspace focus controller), in
//! the web's area order: sidebar, composer, panel, terminal. Rune focuses only text fields by
//! hand, so each area lands on its field (the sidebar's search, the composer, the browser's
//! address bar or the Files search, a terminal) and the conversation and details are passed over.

use rune::{KeyEvent, Tx};

use crate::chat::composer;
use crate::panel::Region;
use crate::{App, Route};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Area {
    Sidebar,
    Composer,
    Panel,
    Terminal,
}

fn areas(s: &App) -> Vec<Area> {
    let mut out = vec![];
    if !s.ui.shell.sidebar.collapsed {
        out.push(Area::Sidebar);
    }
    if matches!(s.frame_route(), Route::Thread(_) | Route::Home { .. }) {
        out.push(Area::Composer);
    }
    if crate::panel::region_available(s, Region::Panel) {
        out.push(Area::Panel);
    }
    if crate::panel::region_available(s, Region::Terminal) {
        out.push(Area::Terminal);
    }
    out
}

fn composer_key(s: &App) -> String {
    composer::draft_key(s.frame_route())
}

fn current(s: &App) -> Option<Area> {
    if s.ui.shell.sidebar.search.focused {
        Some(Area::Sidebar)
    } else if s.ui.chat.composer.drafts.get(&composer_key(s)).is_some_and(|d| d.edit.focused) {
        Some(Area::Composer)
    } else if crate::panel::region_focused(s, Region::Panel) {
        Some(Area::Panel)
    } else if crate::panel::region_focused(s, Region::Terminal) {
        Some(Area::Terminal)
    } else {
        None
    }
}

/// From no area, forward starts at the first and backward at the last; otherwise it wraps.
fn step(current: Option<usize>, n: usize, forward: bool) -> usize {
    match current {
        None if forward => 0,
        None => n - 1,
        Some(i) if forward => (i + 1) % n,
        Some(i) => (i + n - 1) % n,
    }
}

pub fn on_key(s: &mut App, e: &KeyEvent, _tx: &mut Tx) -> bool {
    if s.ui.shell.palette.open || s.ui.search.open {
        return false;
    }
    let forward = if crate::panel::shortcut(s, e, "workspace.focusNext") {
        true
    } else if crate::panel::shortcut(s, e, "workspace.focusPrevious") {
        false
    } else {
        return false;
    };
    let areas = areas(s);
    if areas.is_empty() {
        return false;
    }
    let at = current(s).and_then(|a| areas.iter().position(|v| *v == a));
    let target = areas[step(at, areas.len(), forward)];
    s.ui.shell.sidebar.search.focused = target == Area::Sidebar;
    let key = composer_key(s);
    s.ui.chat.composer.drafts.entry(key).or_default().edit.focused = target == Area::Composer;
    crate::panel::focus_region(
        s,
        match target {
            Area::Panel => Some(Region::Panel),
            Area::Terminal => Some(Region::Terminal),
            _ => None,
        },
    );
    true
}

#[cfg(test)]
mod tests {
    use super::step;

    #[test]
    fn focus_steps_wrap_and_start_from_the_near_end() {
        assert_eq!(step(None, 2, true), 0);
        assert_eq!(step(None, 2, false), 1);
        assert_eq!(step(Some(1), 2, true), 0);
        assert_eq!(step(Some(0), 2, false), 1);
        assert_eq!(step(Some(0), 1, true), 0);
    }
}
