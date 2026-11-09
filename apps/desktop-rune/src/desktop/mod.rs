//! What the Electron main process gives the app (apps/desktop/src) beyond the web UI: the
//! application menu and its accelerators, app zoom, reload, window chrome, size and state,
//! and the single-instance lock. The quit hold is `shell::quit`.
//!
//! Rune's native menus exist only on macOS. Electron hides the menu bar elsewhere, so there
//! only the accelerators matter: `on_key` runs them last, after every surface had the key.

#[cfg(target_os = "macos")]
mod macos;
pub mod single;

use std::cell::Cell;

use rune::{App as RuneApp, Cx, El, Key, KeyEvent, Tx, el};

use crate::data::{self, Source};
use crate::{App, Route};

#[cfg(target_os = "macos")]
pub const APP_NAME: &str = "S5 Code";
/// Electron's `minWidth`/`minHeight`.
pub const MIN_SIZE: (f32, f32) = (840.0, 620.0);
const DEFAULT_SIZE: (f32, f32) = (1440.0, 900.0);
const MAX_SIZE: f32 = 8192.0;
const FRAME: &str = "app-frame";
#[cfg(target_os = "macos")]
const SETTINGS_TITLE: &str = "Settings…";
#[cfg(target_os = "macos")]
const QUIT_TITLE: &str = "Quit S5 Code";
/// Chromium's zoom: each step is half a level, a level is a factor of 1.2.
const ZOOM_STEP: f32 = 0.5;
const ZOOM_LEVELS: (f32, f32) = (-7.5, 8.5);
const SAVE_DEBOUNCE_MS: i64 = 500;

#[derive(Default)]
pub struct Ui {
    /// The bundled server's URL and token, kept for Reload.
    pub server: Option<(String, String)>,
    zoom_level: f32,
    /// The window's size as the last view laid it out, for saving.
    window: Cell<Option<[f32; 2]>>,
    saved_size: Option<[f32; 2]>,
    size_due: Option<(i64, [f32; 2])>,
    prefs_loaded: bool,
}

impl Ui {
    fn zoom_factor(&self) -> f32 {
        1.2f32.powf(self.zoom_level)
    }
}

/// Window state that outlives a launch: the app zoom everywhere, the window size where Rune
/// can't hand it to AppKit's frame autosave. In the T3 home, so a dev worktree keeps its own.
fn prefs() -> rune::prefs::Prefs {
    if rune::is_headless() {
        return rune::prefs::Prefs::memory();
    }
    rune::prefs::Prefs::at(crate::host::t3_home().join("userdata").join("desktop-rune-window.json"))
}

/// The window's opening size: the last one saved, within the minimum and any screen.
pub fn initial_size() -> (f32, f32) {
    if cfg!(target_os = "macos") {
        return DEFAULT_SIZE;
    }
    restore_size(prefs().get::<[f32; 2]>("windowSize"))
}

fn restore_size(saved: Option<[f32; 2]>) -> (f32, f32) {
    match saved {
        Some([w, h]) if w.is_finite() && h.is_finite() => (w.clamp(MIN_SIZE.0, MAX_SIZE), h.clamp(MIN_SIZE.1, MAX_SIZE)),
        _ => DEFAULT_SIZE,
    }
}

/// The app's menu, chrome and window behaviour.
pub fn configure(app: RuneApp<App>) -> RuneApp<App> {
    // Electron's `trafficLightPosition` under its 52 pt topbar; the titlebar band drags.
    let app = app.chrome(rune::Chrome { full_size_content: true, traffic_lights: Some((16.0, 19.0)), drag_regions: false });
    #[cfg(target_os = "macos")]
    let app = mac_menu(app)
        // As Electron on macOS: closing the window keeps the app; the Dock icon reopens it.
        .keep_running()
        .on_window(|window| {
            thread_local! { static FIXED: Cell<bool> = const { Cell::new(false) }; }
            if !FIXED.with(|f| f.replace(true)) {
                macos::fix_menus(SETTINGS_TITLE, QUIT_TITLE);
            }
            macos::autosave_frame(window);
        });
    app
}

/// DesktopApplicationMenu.ts's items Rune can make; `macos::fix_menus` does the rest.
#[cfg(target_os = "macos")]
fn mac_menu(app: RuneApp<App>) -> RuneApp<App> {
    use rune::{MenuItem, Shortcut};
    let key = |s: &str| s.parse::<Shortcut>().expect("valid accelerator");
    app.standard_menus()
        .menu_item(MenuItem::new(SETTINGS_TITLE, |s: &mut App, _: &mut Tx| open_settings(s)).id("settings").key_equivalent(key("cmd+,")).keys_first())
        .menu_item(MenuItem::new(QUIT_TITLE, |_: &mut App, tx: &mut Tx| tx.quit()).id("quit").key_equivalent(key("cmd+q")).keys_first())
        .menu_item(MenuItem::new("Close Window", |_: &mut App, tx: &mut Tx| tx.close_window()).id("close-window").menu("File"))
        .menu_item(MenuItem::new("Reload", |s: &mut App, _: &mut Tx| reload(s, false)).id("reload").menu("View").key_equivalent(key("cmd+r")).keys_first())
        .menu_item(MenuItem::new("Force Reload", |s: &mut App, _: &mut Tx| reload(s, true)).id("force-reload").menu("View").key_equivalent(key("cmd+shift+r")).keys_first())
        .menu_item(MenuItem::separator().menu("View"))
        .menu_item(MenuItem::new("Actual Size", |s: &mut App, tx: &mut Tx| zoom(s, tx, None)).id("zoom-reset").menu("View").key_equivalent(key("cmd+0")).keys_first())
        .menu_item(MenuItem::new("Zoom In", |s: &mut App, tx: &mut Tx| zoom(s, tx, Some(ZOOM_STEP))).id("zoom-in").menu("View").key_equivalent(key("cmd+=")).keys_first())
        .menu_item(MenuItem::new("Zoom Out", |s: &mut App, tx: &mut Tx| zoom(s, tx, Some(-ZOOM_STEP))).id("zoom-out").menu("View").key_equivalent(key("cmd+-")).keys_first())
}

fn open_settings(s: &mut App) {
    if !matches!(s.route, Route::Settings(_)) {
        crate::shell::sidebar::go(s, Route::Settings("general".into()));
    }
}

/// View › Reload: Rune has no page to reload, so this reconnects to the server and reloads its
/// state. Force Reload also drops the surfaces' cached UI state; drafts and panels stay.
fn reload(s: &mut App, force: bool) {
    if force {
        let ui = &mut s.ui;
        ui.chat.timeline = Default::default();
        ui.chat.details = Default::default();
        ui.chat.topbar = Default::default();
        ui.chat.home = Default::default();
        ui.search = Default::default();
        ui.shell.palette = Default::default();
        let main_route = ui.pages.main_route.take();
        ui.pages = Default::default();
        ui.pages.main_route = main_route;
        let return_route = ui.settings.return_route.take();
        ui.settings = Default::default();
        ui.settings.return_route = return_route;
    }
    if let (Some((url, token)), Some(waker)) = (s.ui.desktop.server.clone(), s.ui.panel.waker.clone()) {
        s.src = Source::Live(s5_net::Net::connect(&url, &token, move || waker()));
    }
    let route = s.route.clone();
    s.navigate(route);
}

/// View › Actual Size (`None`), Zoom In and Zoom Out.
fn zoom(s: &mut App, tx: &mut Tx, step: Option<f32>) {
    let level = step.map_or(0.0, |d| (s.ui.desktop.zoom_level + d).clamp(ZOOM_LEVELS.0, ZOOM_LEVELS.1));
    if level == s.ui.desktop.zoom_level {
        return;
    }
    s.ui.desktop.zoom_level = level;
    tx.instant();
    let _ = prefs().set("zoomLevel", &level);
}

/// The menu's accelerators, after every surface had the key (the preview panel takes its own
/// reload and zoom first). Cmd/Ctrl+W nothing took is swallowed: it never closes the window.
pub fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    let Key::Command(c) = e.key else { return false };
    if e.mods.alt || (cfg!(target_os = "macos") && e.mods.ctrl) {
        return false;
    }
    match c {
        ',' if !e.mods.shift => open_settings(s),
        'r' => reload(s, e.mods.shift),
        '0' => zoom(s, tx, None),
        '=' | '+' => zoom(s, tx, Some(ZOOM_STEP)),
        '-' | '_' => zoom(s, tx, Some(-ZOOM_STEP)),
        'w' => {}
        _ => return false,
    }
    true
}

pub fn poll(s: &mut App, tx: &mut Tx) -> bool {
    let ui = &mut s.ui.desktop;
    let now = data::now_ms();
    let mut changed = false;
    if !ui.prefs_loaded {
        ui.prefs_loaded = true;
        let prefs = prefs();
        let level = prefs.get::<f32>("zoomLevel").unwrap_or(0.0);
        ui.zoom_level = if level.is_finite() { level.clamp(ZOOM_LEVELS.0, ZOOM_LEVELS.1) } else { 0.0 };
        ui.saved_size = prefs.get("windowSize");
        changed = ui.zoom_level != 0.0;
    }
    // macOS saves the frame itself (macos::autosave_frame).
    if !cfg!(target_os = "macos") && !rune::is_headless() {
        save_size(ui, now, tx);
    }
    changed
}

fn save_size(ui: &mut Ui, now: i64, tx: &mut Tx) {
    let Some(size) = ui.window.get().map(|[w, h]| [w.round(), h.round()]) else { return };
    if ui.saved_size == Some(size) {
        ui.size_due = None;
        return;
    }
    match ui.size_due {
        Some((due, pending)) if pending == size => {
            if now >= due {
                ui.size_due = None;
                if prefs().set("windowSize", &size).is_ok() {
                    ui.saved_size = Some(size);
                }
            } else {
                tx.wake_after(std::time::Duration::from_millis((due - now) as u64));
            }
        }
        _ => {
            ui.size_due = Some((now + SAVE_DEBOUNCE_MS, size));
            tx.wake_after(std::time::Duration::from_millis(SAVE_DEBOUNCE_MS as u64));
        }
    }
}

/// The window's content: the app at its zoom. Zoom draws the app
/// `k` times larger over a window `1/k` the size, as Chromium's zoom factor lays a page out.
pub fn frame(s: &App, cx: &mut Cx<App>, mut app: El) -> El {
    let size = cx.layout_size(FRAME);
    s.ui.desktop.window.set(size);
    let k = s.ui.desktop.zoom_factor();
    if let Some([w, h]) = size
        && (k - 1.0).abs() > 1e-3
    {
        app = app.absolute(0.0, 0.0).size(w / k, h / k).scale(k).origin(0.0, 0.0);
    }
    el().key(FRAME).w_full().h_full().child(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_size_keeps_the_minimum_and_ignores_garbage() {
        assert_eq!(restore_size(None), DEFAULT_SIZE);
        assert_eq!(restore_size(Some([1200.0, 800.0])), (1200.0, 800.0));
        assert_eq!(restore_size(Some([300.0, 20000.0])), (MIN_SIZE.0, MAX_SIZE));
        assert_eq!(restore_size(Some([f32::NAN, 800.0])), DEFAULT_SIZE);
    }
}
