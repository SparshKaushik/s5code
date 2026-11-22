//! Electron's `MacPermissionHelper`: while System Settings shows a privacy list, a small panel
//! sits over the bottom of its window with an app row to drag into that list, or click to
//! reveal the bundle in Finder. A JXA watcher reports where Settings is; the panel hides while
//! Settings isn't frontmost and closes once the grant lands or Settings closes.

use std::cell::RefCell;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rune::{Color, Cursor, El, OverlayHandle, OverlayPlacement, col, row, text};

use crate::theme;

const APP: &str = "S5 Code";
pub const FULL_DISK_ACCESS_URL: &str = "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_AllFiles";

// Window bounds and pids need no Screen Recording grant. The bundle id avoids localized titles,
// and one osascript process follows Settings as it moves.
const SETTINGS_WINDOW_SCRIPT: &str = r#"
ObjC.import("CoreGraphics");
ObjC.import("AppKit");
ObjC.import("unistd");
function run() {
  let previous = "";
  while ($.getppid() !== 1) {
    const apps = $.NSRunningApplication.runningApplicationsWithBundleIdentifier("com.apple.systempreferences");
    let result = null;
    if (apps.count > 0) {
      const pid = apps.objectAtIndex(0).processIdentifier;
      const front = $.NSWorkspace.sharedWorkspace.frontmostApplication;
      const list = $.CGWindowListCopyWindowInfo(
        $.kCGWindowListOptionOnScreenOnly | $.kCGWindowListExcludeDesktopElements,
        $.kCGNullWindowID
      );
      if (list) {
        $.CFMakeCollectable(list);
        const count = $.CFArrayGetCount(list);
        for (let i = 0; i < count; i++) {
          const w = ObjC.castRefToObject($.CFArrayGetValueAtIndex(list, i));
          if (w.objectForKey("kCGWindowOwnerPID").js !== pid || w.objectForKey("kCGWindowLayer").js !== 0) continue;
          const b = ObjC.deepUnwrap(w.objectForKey("kCGWindowBounds"));
          if (b.Width < 500 || b.Height < 350) continue;
          result = { x: b.X, y: b.Y, width: b.Width, height: b.Height, frontmost: !front.isNil() && front.processIdentifier === pid };
          break;
        }
      }
    }
    const line = JSON.stringify(result);
    if (line !== previous) {
      const data = $(line + "\n").dataUsingEncoding($.NSUTF8StringEncoding);
      $.NSFileHandle.fileHandleWithStandardOutput.writeData(data);
      previous = line;
    }
    $.NSThread.sleepForTimeInterval(result && result.frontmost ? 0.5 : 1);
  }
}"#;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettingsWindow {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub frontmost: bool,
}

/// One watcher line: `Ok(None)` while Settings has no window, `Err` for anything unreadable.
pub fn parse_window(line: &str) -> Result<Option<SettingsWindow>, ()> {
    let v: serde_json::Value = serde_json::from_str(line).map_err(drop)?;
    if v.is_null() {
        return Ok(None);
    }
    let n = |k: &str| v[k].as_f64().filter(|n| n.is_finite()).ok_or(());
    Ok(Some(SettingsWindow { x: n("x")?, y: n("y")?, width: n("width")?, height: n("height")?, frontmost: v["frontmost"].as_bool().ok_or(())? }))
}

/// `settingsHelperBounds`: centred over the pane right of Settings' sidebar, on its bottom edge.
pub fn helper_bounds(w: SettingsWindow) -> [f32; 4] {
    const SIDEBAR: f64 = 216.0;
    const INSET: f64 = 16.0;
    let width = 560f64.min(w.width - SIDEBAR - INSET * 2.0);
    [(w.x + SIDEBAR + (w.width - SIDEBAR - width) / 2.0).round() as f32, (w.y + w.height - 140.0 - INSET).round() as f32, width.round() as f32, 140.0]
}

/// The `.app` an executable runs from, as `macAppBundlePath`.
pub fn bundle_path(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let named = |p: &Path, name: &str| p.file_name().is_some_and(|n| n == name);
    let app = bundle.extension().is_some_and(|e| e == "app") && bundle.file_stem().is_some_and(|s| !s.is_empty());
    (named(macos, "MacOS") && named(contents, "Contents") && app).then(|| bundle.to_owned())
}

#[derive(Debug, PartialEq)]
pub enum Sync {
    Hide,
    Show([f32; 4]),
    Finish,
}

/// `syncPosition` over the watcher's lines; `None` is the watcher failing or exiting.
#[derive(Default)]
pub struct Track {
    found: bool,
}

impl Track {
    pub fn next(&mut self, line: Option<&str>) -> Sync {
        let Some(Ok(window)) = line.map(parse_window) else { return Sync::Hide };
        self.found |= window.is_some();
        match window {
            None if self.found => Sync::Finish,
            Some(w) if w.frontmost => Sync::Show(helper_bounds(w)),
            _ => Sync::Hide,
        }
    }
}

#[derive(Default)]
pub struct Helper {
    bundle: PathBuf,
    dark: bool,
}

thread_local! {
    static OVERLAY: RefCell<Option<OverlayHandle<Helper>>> = const { RefCell::new(None) };
}

/// Each `show` owns a generation; the watcher and the grant poll stop once it moves on.
static GENERATION: AtomicU64 = AtomicU64::new(0);
static WATCHER: Mutex<Option<Child>> = Mutex::new(None);

/// Adds the hidden panel when this is a macOS app bundle, the only place Settings takes a drag.
pub fn register(app: &mut rune::App<crate::App>) {
    if !cfg!(target_os = "macos") || std::env::current_exe().ok().and_then(|e| bundle_path(&e)).is_none() {
        return;
    }
    let spec = rune::OverlaySpec { name: "Set up Full Disk Access", size: (560.0, 140.0), edge: rune::ScreenEdge::Bottom, margin: 0.0, align: rune::Align::Center, inset: 0.0, interactive: true, view };
    let options = rune::OverlayOptions { input: rune::OverlayInput::Regions, ..Default::default() };
    let handle = app.overlay(spec.options(options), Helper::default());
    OVERLAY.with(|o| *o.borrow_mut() = Some(handle));
}

/// Opens System Settings at `url` and, from a bundle, the helper over it until `granted`.
pub fn open_settings(url: &str, dark: bool, granted: fn() -> bool) {
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(url).spawn();
    #[cfg(not(target_os = "macos"))]
    let _ = url;
    show(dark, granted);
}

fn show(dark: bool, granted: fn() -> bool) {
    close();
    let Some(overlay) = OVERLAY.with(|o| o.borrow().clone()) else { return };
    let Some(bundle) = std::env::current_exe().ok().and_then(|e| bundle_path(&e)) else { return };
    if granted() {
        return;
    }
    let generation = GENERATION.load(Ordering::SeqCst);
    overlay.update(move |h, _| *h = Helper { bundle, dark });
    let current = move || GENERATION.load(Ordering::SeqCst) == generation;
    let poll = overlay.clone();
    std::thread::spawn(move || {
        while current() {
            std::thread::sleep(Duration::from_secs(1));
            if current() && granted() {
                finish(&poll, generation);
            }
        }
    });
    std::thread::spawn(move || {
        let child = Command::new("/usr/bin/osascript").args(["-l", "JavaScript", "-e", SETTINGS_WINDOW_SCRIPT]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let Ok(mut child) = child else { return overlay.hide() };
        let stdout = child.stdout.take();
        if let Ok(mut slot) = WATCHER.lock() {
            if !current() {
                let _ = child.kill();
                return;
            }
            *slot = Some(child);
        }
        let mut track = Track::default();
        for line in stdout.into_iter().flat_map(|out| BufReader::new(out).lines()) {
            let Ok(line) = line else { break };
            if !current() {
                return;
            }
            match track.next(Some(&line)) {
                Sync::Hide => overlay.hide(),
                Sync::Show([x, y, width, height]) => {
                    overlay.set_placement(Some(OverlayPlacement::Rect { x, y, width, height }));
                    overlay.show();
                }
                Sync::Finish => return finish(&overlay, generation),
            }
        }
        if current() {
            overlay.hide();
        }
    });
}

/// Closes the helper for `generation` and brings the app back, as the grant or the close button.
fn finish(overlay: &OverlayHandle<Helper>, generation: u64) {
    if GENERATION.compare_exchange(generation, generation + 1, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return;
    }
    stop(overlay);
    overlay.update(|_, _| crate::notify::reveal_window());
}

pub fn close() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Some(overlay) = OVERLAY.with(|o| o.borrow().clone()) {
        stop(&overlay);
    }
}

fn stop(overlay: &OverlayHandle<Helper>) {
    if let Some(mut child) = WATCHER.lock().ok().and_then(|mut w| w.take()) {
        let _ = child.kill();
        let _ = child.wait();
    }
    overlay.hide();
}

fn hex(v: &str) -> Color {
    rune::color::hex(v).unwrap_or(Color::BLACK)
}

fn view(h: &Helper, _: &mut rune::Cx<Helper>) -> El {
    let [base, fill, fg, line] = if h.dark { ["#242424", "#383838", "#f5f5f5", "#484848"] } else { ["#ffffff", "#e7e7e7", "#292929", "#e3e3e3"] }.map(hex);
    let (drag, reveal) = (h.bundle.clone(), h.bundle.clone());
    let close = row()
        .key("permission-helper-close")
        .w(22.0)
        .h(22.0)
        .radius(11.0)
        .center()
        .role("button")
        .label("Close permission helper")
        .focusable(true)
        .cursor(Cursor::Pointer)
        .on_press(|_: &mut Helper| {
            if let Some(overlay) = OVERLAY.with(|o| o.borrow().clone()) {
                finish(&overlay, GENERATION.load(Ordering::SeqCst));
            }
        })
        .child(text("×").font(theme::TEXT_LG).color(fg));
    // No icon ships in the bundle yet, so the row shows the app's initials.
    let mark = row().w(32.0).h(32.0).radius(8.0).center().fill(fg).child(text("S5").font(theme::weight(theme::TEXT_SM, theme::BOLD)).color(base));
    let app = row()
        .key("permission-helper-app")
        .w_full()
        .h(52.0)
        .mt(20.0)
        .px(12.0)
        .gap(12.0)
        .center_y()
        .radius(10.0)
        .fill(fill)
        .role("button")
        .label(format!("Drag {APP} to System Settings, or click to reveal in Finder"))
        .focusable(true)
        .cursor(Cursor::Grab)
        .drag_out(move |_: &Helper| Some(rune::DragItem::new().file(drag.clone())))
        .on_press(move |_: &mut Helper| {
            let _ = crate::panel::preview::reveal(&reveal);
        })
        .child(mark)
        .child(text(APP).font(theme::weight(theme::TEXT_BASE, theme::SEMIBOLD)).color(fg));
    let header = row()
        .w_full()
        .center_y()
        .child(text(format!("↑ Drag {APP} into the list above")).font(theme::weight(theme::TEXT_SM, theme::SEMIBOLD)).color(fg).grow(1.0))
        .child(close);
    col().key("permission-helper").w_full().h_full().padding(2.0).child(col().hit().w_full().h_full().px(20.0).pt(14.0).radius(24.0).border(1.0, line).fill(base).child(header).child(app))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helper_sits_over_the_settings_pane_and_follows_it() {
        let w = SettingsWindow { x: 100.0, y: 50.0, width: 900.0, height: 600.0, frontmost: true };
        assert_eq!(helper_bounds(w), [378.0, 494.0, 560.0, 140.0], "centred right of the 216pt sidebar");
        assert_eq!(helper_bounds(SettingsWindow { width: 700.0, ..w }), [332.0, 494.0, 452.0, 140.0], "narrowed to the pane");

        let mut track = Track::default();
        assert_eq!(track.next(Some("null")), Sync::Hide, "Settings hasn't opened yet");
        let line = r#"{"x":100,"y":50,"width":900,"height":600,"frontmost":true}"#;
        assert_eq!(track.next(Some(line)), Sync::Show([378.0, 494.0, 560.0, 140.0]));
        assert_eq!(track.next(Some(&line.replace("true", "false"))), Sync::Hide, "another app is in front");
        assert_eq!(track.next(Some("{bad")), Sync::Hide);
        assert_eq!(track.next(None), Sync::Hide, "the watcher exited");
        assert_eq!(track.next(Some("null")), Sync::Finish, "Settings closed after it was found");
    }

    #[test]
    fn only_an_executable_inside_an_app_bundle_has_one() {
        assert_eq!(bundle_path(Path::new("/Applications/S5 Code.app/Contents/MacOS/s5code")), Some(PathBuf::from("/Applications/S5 Code.app")));
        assert_eq!(bundle_path(Path::new("/Users/me/s5code/target/release/s5code")), None);
        assert_eq!(bundle_path(Path::new("/Applications/S5 Code.app/Contents/Resources/s5code")), None);
        assert_eq!(bundle_path(Path::new("/x/.app/Contents/MacOS/s5code")), None);
    }
}
