//! What the Electron main process gives the app (apps/desktop/src) beyond the web UI: the
//! application menu and its accelerators, app zoom, reload, window chrome, size, position and
//! state, network access (`exposure`) and the single-instance lock. The quit hold is `shell::quit`.
//!
//! Rune's native menus exist only on macOS. Electron hides the menu bar elsewhere, so there
//! only the accelerators matter: `on_key` runs them last, after every surface had the key.

#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(unix)]
pub mod cli;
pub mod exposure;
pub mod policy;
#[cfg(unix)]
pub mod telemetry;
pub mod locale;
pub mod recovery;
#[cfg(unix)]
pub mod shell_path;
#[cfg(unix)]
pub mod activation;
#[cfg(unix)]
pub mod signals;
pub mod single;
#[cfg(target_os = "linux")]
mod url_handler;

use std::cell::Cell;

use rune::{App as RuneApp, Cx, El, Key, KeyEvent, Tx, el};

use crate::data::{self, Source};
use crate::{App, Route};

#[cfg(target_os = "macos")]
pub const APP_NAME: &str = "S5 Code";
/// The window's Wayland `app_id` / X11 `WM_CLASS`, the macOS bundle id, and the name of the
/// Linux launcher entry (`assets/<APP_ID>.desktop`), so docks match the window to it.
pub const APP_ID: &str = "in.newsa.s5code.rune-prototype";
/// Electron's `minWidth`/`minHeight`.
pub const MIN_SIZE: (f32, f32) = (840.0, 620.0);
/// Electron's default `width`/`height`.
const DEFAULT_SIZE: (f32, f32) = (1100.0, 780.0);
/// Larger than any title bar or border: the window manager placed the window elsewhere.
const MAX_FRAME_OFFSET: i32 = 200;
const MAX_SIZE: f32 = 8192.0;
const FRAME: &str = "app-frame";
#[cfg(target_os = "macos")]
const SETTINGS_TITLE: &str = "Settings…";
#[cfg(target_os = "macos")]
const QUIT_TITLE: &str = "Quit S5 Code";
#[cfg(target_os = "macos")]
const UPDATES_TITLE: &str = "Check for Updates...";
const UPDATES_UNAVAILABLE: &str = "Automatic updates are not available right now.";
/// Electron's reason for a build without `app-update.yml`, which every Rune build is.
pub const NO_UPDATE_FEED: &str = "Automatic updates are not available because no update feed is configured.";
/// Chromium's zoom: each step is half a level, a level is a factor of 1.2.
const ZOOM_STEP: f32 = 0.5;
const ZOOM_LEVELS: (f32, f32) = (-7.5, 8.5);
const SAVE_DEBOUNCE_MS: i64 = 500;

#[derive(Default)]
pub struct Ui {
    /// The bundled server's URL and token, kept for Reload.
    pub server: Option<(String, String)>,
    pub exposure: exposure::State,
    /// Why the bundled server keeps failing before it ever started, for the recovery screen.
    pub startup_failure: Option<crate::host::StartupFailure>,
    /// `localEnvironmentEnabled` is off, so this machine runs no server.
    pub local_off: bool,
    #[cfg(unix)]
    pub activation: activation::State,
    #[cfg(unix)]
    pub cli: cli::State,
    zoom_level: f32,
    /// The window's size as the last view laid it out, for saving.
    window: Cell<Option<[f32; 2]>>,
    /// The content's desktop pixel origin as last reported; unknown on Wayland.
    origin: Option<[i32; 2]>,
    /// The outer position restored at launch, until the first origin shows the frame around it.
    restored: Option<[i32; 2]>,
    /// The window manager's frame: content origin minus outer position.
    frame_offset: Option<[i32; 2]>,
    saved: Option<Bounds>,
    due: Option<(i64, Bounds)>,
    prefs_loaded: bool,
    /// The appearance last handed to AppKit.
    #[cfg(target_os = "macos")]
    appearance: Option<crate::Appearance>,
}

/// What a launch restores: the size in points, and off macOS and Wayland the outer position in
/// desktop pixels. macOS saves its frame itself (`macos::autosave_frame`).
#[derive(Clone, Copy, PartialEq, Debug)]
struct Bounds {
    size: [f32; 2],
    position: Option<[i32; 2]>,
}

impl Ui {
    pub(crate) fn zoom_factor(&self) -> f32 {
        1.2f32.powf(self.zoom_level)
    }

    fn set_zoom_level(&mut self, level: f32) {
        self.zoom_level = level;
        ZOOM.with(|z| z.set(self.zoom_factor()));
    }
}

thread_local! {
    /// The app zoom, for code that holds window points but no `App`.
    static ZOOM: Cell<f32> = const { Cell::new(1.0) };
}

/// Window points (`PointerPress`, a context menu's anchor) to the zoomed app's layout points.
pub fn from_window(x: f32, y: f32) -> (f32, f32) {
    let k = ZOOM.with(Cell::get);
    (x / k, y / k)
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

/// Where the window opened last, for X11 (Wayland places windows itself).
fn saved_position(prefs: &rune::prefs::Prefs) -> Option<[i32; 2]> {
    if cfg!(target_os = "macos") || rune::is_headless() {
        return None;
    }
    prefs.get("windowPosition")
}

/// The outer position the content origin implies, once the frame around it is known.
fn outer_position(origin: Option<[i32; 2]>, frame_offset: Option<[i32; 2]>) -> Option<[i32; 2]> {
    let [x, y] = origin.filter(|o| *o != [0, 0])?;
    let [dx, dy] = frame_offset.unwrap_or_default();
    Some([x - dx, y - dy])
}

/// The frame a restored window's first origin shows, if it opened where it was asked to.
fn frame_offset(origin: [i32; 2], restored: [i32; 2]) -> Option<[i32; 2]> {
    let offset = [origin[0] - restored[0], origin[1] - restored[1]];
    offset.iter().all(|d| (0..=MAX_FRAME_OFFSET).contains(d)).then_some(offset)
}

fn restore_size(saved: Option<[f32; 2]>) -> (f32, f32) {
    match saved {
        Some([w, h]) if w.is_finite() && h.is_finite() => (w.clamp(MIN_SIZE.0, MAX_SIZE), h.clamp(MIN_SIZE.1, MAX_SIZE)),
        _ => DEFAULT_SIZE,
    }
}

/// The app's menu, chrome and window behaviour.
pub fn configure(app: RuneApp<App>) -> RuneApp<App> {
    rune::set_app_id(APP_ID);
    // Electron's `trafficLightPosition` under its 52 pt topbar; the titlebar band drags.
    let app = app.chrome(rune::Chrome { full_size_content: true, traffic_lights: Some((16.0, 19.0)), drag_regions: false });
    if let Some([x, y]) = saved_position(&prefs()) {
        app.handle().set_position((x, y));
    }
    #[cfg(target_os = "linux")]
    url_handler::register();
    #[cfg(target_os = "macos")]
    let app = mac_menu(app)
        // As Electron on macOS: closing the window keeps the app; the Dock icon reopens it.
        .keep_running()
        .on_window(|window| {
            thread_local! { static FIXED: Cell<bool> = const { Cell::new(false) }; }
            if !FIXED.with(|f| f.replace(true)) {
                macos::fix_menus(SETTINGS_TITLE, UPDATES_TITLE, QUIT_TITLE);
            }
            macos::autosave_frame(window);
            macos::install_open_documents();
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
        .menu_item(MenuItem::new(UPDATES_TITLE, |s: &mut App, _: &mut Tx| check_for_updates(s)).id("check-for-updates"))
        .menu_item(MenuItem::new(UPDATES_TITLE, |s: &mut App, _: &mut Tx| check_for_updates(s)).id("help-check-for-updates").menu("Help"))
        .menu_item(MenuItem::new(QUIT_TITLE, |s: &mut App, tx: &mut Tx| crate::shell::quit::menu_quit(s, tx)).id("quit").key_equivalent(key("cmd+q")).keys_first())
        .menu_item(MenuItem::new("Close Window", |_: &mut App, tx: &mut Tx| tx.close_window()).id("close-window").menu("File"))
        .menu_item(MenuItem::new("Reload", |s: &mut App, _: &mut Tx| reload(s, false)).id("reload").menu("View").key_equivalent(key("cmd+r")).keys_first())
        .menu_item(MenuItem::new("Force Reload", |s: &mut App, _: &mut Tx| reload(s, true)).id("force-reload").menu("View").key_equivalent(key("cmd+shift+r")).keys_first())
        .menu_item(MenuItem::separator().menu("View"))
        .menu_item(MenuItem::new("Actual Size", |s: &mut App, tx: &mut Tx| zoom(s, tx, None)).id("zoom-reset").menu("View").key_equivalent(key("cmd+0")).keys_first())
        .menu_item(MenuItem::new("Zoom In", |s: &mut App, tx: &mut Tx| zoom(s, tx, Some(ZOOM_STEP))).id("zoom-in").menu("View").key_equivalent(key("cmd+=")).keys_first())
        .menu_item(MenuItem::new("Zoom Out", |s: &mut App, tx: &mut Tx| zoom(s, tx, Some(-ZOOM_STEP))).id("zoom-out").menu("View").key_equivalent(key("cmd+-")).keys_first())
}

/// Check for Updates, as Electron's with no feed configured: its reason, in an alert on macOS
/// (App and Help menus) and in the app's toast elsewhere (About and the command palette, since
/// Rune has no menu bar there).
pub fn check_for_updates(s: &mut App) {
    #[cfg(target_os = "macos")]
    {
        let _ = s;
        macos::alert(UPDATES_UNAVAILABLE, NO_UPDATE_FEED);
    }
    #[cfg(not(target_os = "macos"))]
    crate::shell::palette::toast(s, UPDATES_UNAVAILABLE, Some(NO_UPDATE_FEED));
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
    if let (Some((url, token)), Some(waker)) = (reload_target(s), s.ui.panel.waker.clone()) {
        s.src = Source::Live(s5_net::Net::connect(&url, &token, move || waker()));
    }
    let route = s.route.clone();
    s.navigate(route);
}

/// The server on screen (`App::src`): this machine's, or the saved environment switched to.
fn reload_target(s: &App) -> Option<(String, String)> {
    if s.ui.env.active == crate::env::LOCAL {
        return s.ui.desktop.server.clone();
    }
    s.ui.env.saved.iter().find(|e| e.id == s.ui.env.active).map(|e| (e.base.clone(), e.bearer.clone()))
}

/// View › Actual Size (`None`), Zoom In and Zoom Out.
fn zoom(s: &mut App, tx: &mut Tx, step: Option<f32>) {
    let level = step.map_or(0.0, |d| (s.ui.desktop.zoom_level + d).clamp(ZOOM_LEVELS.0, ZOOM_LEVELS.1));
    if level == s.ui.desktop.zoom_level {
        return;
    }
    s.ui.desktop.set_zoom_level(level);
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
        ui.set_zoom_level(if level.is_finite() { level.clamp(ZOOM_LEVELS.0, ZOOM_LEVELS.1) } else { 0.0 });
        ui.restored = saved_position(&prefs);
        ui.frame_offset = prefs.get("windowFrameOffset");
        ui.saved = prefs.get("windowSize").map(|size| Bounds { size, position: ui.restored });
        ui.exposure.load();
        #[cfg(unix)]
        ui.cli.load();
        changed = ui.zoom_level != 0.0;
        #[cfg(unix)]
        shell_path::warm(s.ui.panel.waker.clone());
    }
    #[cfg(unix)]
    if !s.ui.desktop.cli.login_path && shell_path::get().is_some() {
        s.ui.desktop.cli.load();
        changed = true;
    }
    changed |= exposure::poll(s);
    #[cfg(unix)]
    {
        changed |= activation::poll(s);
    }
    let ui = &mut s.ui.desktop;
    #[cfg(target_os = "macos")]
    if ui.appearance != Some(s.appearance) && !rune::is_headless() {
        ui.appearance = Some(s.appearance);
        macos::set_appearance(match s.appearance {
            crate::Appearance::System => None,
            crate::Appearance::Light => Some(false),
            crate::Appearance::Dark => Some(true),
        });
    }
    if let Some(origin) = ui.origin.filter(|o| *o != [0, 0])
        && let Some(restored) = ui.restored.take()
        && let Some(offset) = frame_offset(origin, restored)
        && ui.frame_offset != Some(offset)
    {
        ui.frame_offset = Some(offset);
        let _ = prefs().set("windowFrameOffset", &offset);
    }
    if !cfg!(target_os = "macos") && !rune::is_headless() {
        save_bounds(ui, now, tx);
    }
    changed
}

fn save_bounds(ui: &mut Ui, now: i64, tx: &mut Tx) {
    let Some([w, h]) = ui.window.get() else { return };
    // Until the restored window reports in, its offset is unknown: keep the saved position.
    let position = if ui.restored.is_some() { ui.saved.and_then(|b| b.position) } else { outer_position(ui.origin, ui.frame_offset) };
    let bounds = Bounds { size: [w.round(), h.round()], position };
    if ui.saved == Some(bounds) {
        ui.due = None;
        return;
    }
    match ui.due {
        Some((due, pending)) if pending == bounds => {
            if now >= due {
                ui.due = None;
                let mut prefs = prefs();
                let saved = prefs.set("windowSize", &bounds.size).is_ok() && bounds.position.is_none_or(|p| prefs.set("windowPosition", &p).is_ok());
                if saved {
                    ui.saved = Some(bounds);
                }
            } else {
                tx.wake_after(std::time::Duration::from_millis((due - now) as u64));
            }
        }
        _ => {
            ui.due = Some((now + SAVE_DEBOUNCE_MS, bounds));
            tx.wake_after(std::time::Duration::from_millis(SAVE_DEBOUNCE_MS as u64));
        }
    }
}

/// The window's content: the app at its zoom. Zoom draws the app
/// `k` times larger over a window `1/k` the size, as Chromium's zoom factor lays a page out.
pub fn frame(s: &App, cx: &mut Cx<App>, mut app: El) -> El {
    let size = cx.layout_size(FRAME);
    s.ui.desktop.window.set(size);
    let k = s.ui.desktop.zoom_factor() * crate::settings::fonts::interface_scale(&s.client_settings.values);
    if let Some([w, h]) = size
        && (k - 1.0).abs() > 1e-3
    {
        app = app.absolute(0.0, 0.0).size(w / k, h / k).scale(k).origin(0.0, 0.0);
    }
    let frame = el().key(FRAME).w_full().h_full().child(app);
    if cfg!(target_os = "macos") {
        return frame;
    }
    let scale = cx.env().scale;
    frame.on_screen_layout(move |s: &mut App, [x, y, _, _]| {
        s.ui.desktop.origin = Some([(x * scale).round() as i32, (y * scale).round() as i32]);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_reconnects_the_environment_on_screen() {
        let mut s = App::new();
        s.ui.desktop.server = Some(("http://127.0.0.1:1".into(), "local".into()));
        let remote = crate::env::Saved { id: "a".into(), label: "A".into(), base: "https://a.example".into(), bearer: "remote".into() };
        s.ui.env.saved.push(remote);
        assert_eq!(reload_target(&s), Some(("http://127.0.0.1:1".into(), "local".into())));
        s.ui.env.active = "a".into();
        assert_eq!(reload_target(&s), Some(("https://a.example".into(), "remote".into())));
    }

    #[test]
    fn pointer_menus_map_window_points_into_the_zoomed_app() {
        let mut ui = Ui::default();
        ui.set_zoom_level(1.0);
        let (x, y) = from_window(1200.0, 240.0);
        assert!((x - 1000.0).abs() < 1e-3 && (y - 200.0).abs() < 1e-3);
        ui.set_zoom_level(0.0);
        assert_eq!(from_window(1200.0, 240.0), (1200.0, 240.0));
    }

    #[test]
    fn restored_size_keeps_the_minimum_and_ignores_garbage() {
        assert_eq!(restore_size(None), DEFAULT_SIZE);
        assert_eq!(restore_size(Some([1200.0, 800.0])), (1200.0, 800.0));
        assert_eq!(restore_size(Some([300.0, 20000.0])), (MIN_SIZE.0, MAX_SIZE));
        assert_eq!(restore_size(Some([f32::NAN, 800.0])), DEFAULT_SIZE);
    }

    #[test]
    fn restored_position_learns_the_window_frame_once() {
        // Restored at (100, 50); the title bar puts the content 30 px lower.
        let offset = frame_offset([100, 80], [100, 50]);
        assert_eq!(offset, Some([0, 30]));
        assert_eq!(outer_position(Some([400, 230]), offset), Some([400, 200]), "saves where it restores to");
        assert_eq!(frame_offset([900, 600], [100, 50]), None, "the window manager put it elsewhere");
        assert_eq!(frame_offset([90, 80], [100, 50]), None);
        assert_eq!(outer_position(Some([0, 0]), offset), None, "Wayland hides the position");
        assert_eq!(outer_position(Some([400, 230]), None), Some([400, 230]));
    }
}
