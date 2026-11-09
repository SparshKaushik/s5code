//! Thread notifications (apps/web `ThreadNotificationCoordinator`): a thread finishing or asking
//! for input plays a sound, shows an in-app toast while the window is focused on another thread,
//! or a desktop notification while the window is in the background, and the dock badge counts
//! the desktop notifications not yet seen. Settings > General's "Notifications" and "In-app
//! notifications" drive it.

use std::collections::{HashMap, HashSet};

use rune::{Color, Cursor, El, Tx, col, el, icon, row, text};

use crate::data;
use crate::shell::sidebar::logic::{self, Status, Thread};
use crate::theme::{self, AlphaMul, T, layout};
use crate::{App, Route, icons, kit};

/// Base UI's default toast timeout.
const TOAST_MS: i64 = 5000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Input,
    Completion,
}

/// `ThreadNotificationObservation`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Observation {
    /// Run and status while the thread needs attention.
    pub attention: Option<String>,
    /// Idle, and the latest run completed.
    pub settled: bool,
    pub completed_at: Option<i64>,
}

/// `createThreadNotificationGate`: decides which transitions notify, each completion once.
#[derive(Default)]
pub struct Gate {
    handled: HashMap<String, i64>,
}

impl Gate {
    fn skip(&mut self, key: &str, at: Option<i64>) {
        if let Some(at) = at
            && self.handled.get(key).is_none_or(|h| at > *h)
        {
            self.handled.insert(key.to_string(), at);
        }
    }

    pub fn observe(&mut self, key: &str, prior: Option<&Observation>, next: &Observation, archived: bool, quiet: bool) -> Option<Kind> {
        // The first sighting, archived threads, and stopped runs never notify completion.
        if prior.is_none() || archived || quiet {
            self.skip(key, next.completed_at);
        }
        let prior = prior.filter(|_| !archived)?;
        if next.attention.is_some() && next.attention != prior.attention {
            return Some(Kind::Input);
        }
        let at = next.completed_at.filter(|_| next.settled)?;
        if self.handled.get(key).is_some_and(|h| at <= *h) {
            return None;
        }
        self.handled.insert(key.to_string(), at);
        Some(Kind::Completion)
    }

    pub fn retain(&mut self, keys: &HashSet<String>) {
        self.handled.retain(|k, _| keys.contains(k));
    }
}

/// The observation a shell row gives, with the status the toast's title and icon follow.
pub fn observe_row(row: &serde_json::Value) -> (Thread, Status, Observation) {
    let thread = Thread::from_shell(row);
    let run_status = thread.run.as_ref().map(|r| r.status.as_str());
    let mut status = logic::status(&thread);
    if status == Status::Ready && run_status == Some("failed") {
        status = Status::Failed;
    }
    let attention = matches!(status, Status::Input | Status::Request | Status::Approval | Status::Failed | Status::Limited)
        .then(|| format!("{}:{status:?}", thread.run.as_ref().map(|r| r.id.as_str()).unwrap_or_default()));
    let settled = status == Status::Ready && run_status == Some("completed");
    let completed_at = thread.run.as_ref().and_then(|r| r.completed_at).filter(|_| settled);
    (thread, status, Observation { attention, settled, completed_at })
}

struct ThreadToast {
    id: u64,
    thread: String,
    title: &'static str,
    description: String,
    kind: Kind,
    status: Status,
    until: i64,
}

#[derive(Default)]
pub struct Ui {
    gate: Gate,
    previous: HashMap<String, Observation>,
    sequence: Option<u64>,
    /// The native window is in the background (`document.hasFocus()` is false).
    pub background: bool,
    toasts: Vec<ThreadToast>,
    next_toast: u64,
    /// Threads with a desktop notification the user has not come back for: the badge count.
    pending: Vec<String>,
    badge: usize,
    applied_mode: Option<String>,
    sounds: Option<[rune::Player; 2]>,
}

fn mode(s: &App) -> &str {
    s.client_settings.values.get("notificationMode").and_then(|v| v.as_str()).unwrap_or("off")
}
fn in_app(s: &App) -> bool {
    s.client_settings.values.get("inAppNotificationsEnabled").and_then(|v| v.as_bool()).unwrap_or(false)
}
fn has_sound(mode: &str) -> bool {
    mode == "sound" || mode == "notifications-and-sound"
}
fn has_desktop(mode: &str) -> bool {
    mode == "notifications" || mode == "notifications-and-sound"
}

pub fn window_focus(s: &mut App, focused: bool) {
    s.ui.notify.background = !focused;
    if focused {
        clear_pending(s);
    }
}

fn clear_pending(s: &mut App) {
    s.ui.notify.pending.clear();
    sync_badge(s);
}

fn sync_badge(s: &mut App) {
    let count = s.ui.notify.pending.len();
    if count != s.ui.notify.badge {
        s.ui.notify.badge = count;
        os::set_badge(count);
    }
}

thread_local! {
    /// The main window, raised when a desktop notification is clicked (UI thread only).
    static WINDOW: std::cell::OnceCell<rune::AppHandle<App>> = const { std::cell::OnceCell::new() };
}

pub fn set_window(handle: rune::AppHandle<App>) {
    WINDOW.with(|w| {
        let _ = w.set(handle);
    });
}

/// Threads whose desktop notification was clicked, from the helper processes' threads.
static CLICKED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// A clicked notification brings the window forward on its thread, as Electron's does.
fn open_clicked(s: &mut App) -> bool {
    let Some(id) = CLICKED.lock().map(|mut c| c.pop()).ok().flatten() else { return false };
    CLICKED.lock().map(|mut c| c.clear()).ok();
    WINDOW.with(|w| w.get().map(|h| h.open()));
    crate::shell::sidebar::open_thread(s, &id);
    true
}

pub fn poll(s: &mut App, tx: &mut Tx) -> bool {
    let now = data::now_ms();
    let changed = open_clicked(s) | observe(s, now);
    let before = s.ui.notify.toasts.len();
    s.ui.notify.toasts.retain(|t| t.until > now);
    // Nothing else may wake the loop while a toast is up, so its timeout schedules its own poll.
    if let Some(until) = s.ui.notify.toasts.iter().map(|t| t.until).min() {
        tx.wake_after(std::time::Duration::from_millis((until - now).max(1) as u64));
    }
    changed || before != s.ui.notify.toasts.len()
}

fn observe(s: &mut App, now: i64) -> bool {
    let mode = mode(s).to_string();
    if s.ui.notify.applied_mode.as_deref() != Some(mode.as_str()) {
        s.ui.notify.applied_mode = Some(mode.clone());
        clear_pending(s);
    }
    // Notifications off: forget everything, so turning them on starts from the current state.
    if mode == "off" && !in_app(s) {
        s.ui.notify.previous.clear();
        s.ui.notify.gate = Gate::default();
        s.ui.notify.sequence = None;
        return false;
    }
    let st = s.state();
    if !matches!(s.src, data::Source::Live(_)) || !st.shell_ready || s.ui.notify.sequence == Some(st.shell_sequence) {
        return false;
    }
    let sequence = st.shell_sequence;
    let rows: Vec<_> = st.threads.iter().chain(st.archived.iter()).map(|row| (observe_row(row), row["latestRunStopRequested"] == true)).collect();
    let mut presented = vec![];
    let ui = &mut s.ui.notify;
    ui.sequence = Some(sequence);
    let mut next = HashMap::new();
    for ((thread, status, observation), quiet) in rows {
        // Subagents and side chats surface on their parent.
        if thread.hidden {
            continue;
        }
        if let Some(kind) = ui.gate.observe(&thread.id, ui.previous.get(&thread.id), &observation, thread.archived, quiet) {
            presented.push((thread.id.clone(), thread.title.clone(), status, kind));
        }
        next.insert(thread.id, observation);
    }
    ui.gate.retain(&next.keys().cloned().collect());
    ui.previous = next;
    let changed = !presented.is_empty();
    for (id, title, status, kind) in presented {
        present(s, &id, title, status, kind, now);
    }
    changed
}

fn title_for(kind: Kind, status: Status) -> &'static str {
    match (kind, status) {
        (Kind::Completion, _) => "Thread completed",
        (_, Status::Approval) => "Approval needed",
        (_, Status::Limited) => "Usage limit reached",
        (_, Status::Failed) => "Thread failed",
        (_, Status::Request) => "Guest request",
        _ => "Input needed",
    }
}

fn present(s: &mut App, id: &str, description: String, status: Status, kind: Kind, now: i64) {
    let title = title_for(kind, status);
    let mode = mode(s).to_string();
    if has_sound(&mode) {
        play_sound(s, kind);
    }
    let active = matches!(&s.route, Route::Thread(open) if open == id);
    let focused = !s.ui.notify.background;
    if in_app(s) && focused && !active {
        let ui = &mut s.ui.notify;
        ui.next_toast += 1;
        ui.toasts.push(ThreadToast { id: ui.next_toast, thread: id.to_string(), title, description, kind, status, until: now + TOAST_MS });
        return;
    }
    if !has_desktop(&mode) || focused {
        return;
    }
    os::notify(title, &description, id, s.ui.panel.waker.clone());
    let ui = &mut s.ui.notify;
    ui.pending.retain(|p| p != id);
    ui.pending.push(id.to_string());
    sync_badge(s);
}

fn play_sound(s: &mut App, kind: Kind) {
    let sounds = s.ui.notify.sounds.get_or_insert_with(|| {
        let dir = std::env::temp_dir().join("s5code-rune-sounds");
        let _ = std::fs::create_dir_all(&dir);
        let clip = |name: &str, bytes: &[u8]| {
            let path = dir.join(name);
            if std::fs::metadata(&path).map(|m| m.len()).ok() != Some(bytes.len() as u64) {
                let _ = std::fs::write(&path, bytes);
            }
            rune::Player::audio(path)
        };
        [
            clip("notification-completion.mp3", include_bytes!("../assets/notification-completion.mp3")),
            clip("notification-input.mp3", include_bytes!("../assets/notification-input.mp3")),
        ]
    });
    let player = &sounds[if kind == Kind::Completion { 0 } else { 1 }];
    // A sound already playing is not restarted on top of itself.
    if !player.is_playing() {
        player.play();
    }
}

/// The in-app toasts, top-right under the topbar, newest first.
pub fn overlays(s: &App, t: &T) -> Option<El> {
    let toasts = &s.ui.notify.toasts;
    if toasts.is_empty() {
        return None;
    }
    let k = kit::theme(t);
    let mut region = col().key("thread-toasts").role("region").label("Notifications").pin(None, Some(32.0 + layout::TOPBAR_H), Some(32.0), None).w(360.0).gap(12.0);
    for toast in toasts.iter().rev().take(3) {
        region = region.child(toast_view(toast, t, &k));
    }
    Some(region)
}

fn toast_view(toast: &ThreadToast, t: &T, k: &rune_kit::Kit) -> El {
    let (glyph, color): (&'static rune::Icon, Color) = match (toast.kind, toast.status) {
        (Kind::Completion, _) => (&icons::CIRCLE_CHECK, t.c.success_foreground),
        (_, Status::Approval) => (&icons::SHIELD_QUESTION_MARK, t.c.warning_foreground),
        (_, Status::Limited) => (&icons::CIRCLE_ALERT, t.c.warning_foreground),
        (_, Status::Failed) => (&icons::CIRCLE_ALERT, t.c.destructive_foreground),
        _ => (&icons::MESSAGE_CIRCLE_QUESTION_MARK, t.c.info_foreground),
    };
    let id = toast.id;
    let thread = toast.thread.clone();
    let open = kit::Button::new("Open thread")
        .size("xs")
        .on_press(move |s: &mut App| {
            s.ui.notify.toasts.retain(|t| t.id != id);
            crate::shell::sidebar::open_thread(s, &thread);
        })
        .build(t);
    let body = col()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(2.0)
        .child(
            row()
                .gap(8.0)
                .min_w(0.0)
                .child(el().w(16.0).h(20.0).center().child(icon(glyph, 16.0).color(color)))
                .child(text(toast.title).font(theme::SM_MEDIUM).color(t.c.popover_foreground).wrap().shrink().min_w(0.0)),
        )
        .child(text(toast.description.clone()).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap().max_lines(3));
    let dismiss = el()
        .key("toast-dismiss")
        .role("button")
        .label("Dismiss notification")
        .pin(None, Some(-6.0), Some(-6.0), None)
        .size(24.0, 24.0)
        .center()
        .radius(theme::radius::FULL)
        .fill(t.c.popover.alpha_mul(0.92))
        .border(1.0, t.c.border.alpha_mul(0.6))
        .cursor(Cursor::Pointer)
        .child(icon(&icons::X, 12.0).color(t.c.muted_foreground))
        .on_press(move |s: &mut App| s.ui.notify.toasts.retain(|t| t.id != id));
    let content = row().w_full().pl(14.0).pr(24.0).py(12.0).gap(6.0).center_y().child(body).child(open);
    kit::motion::popup(kit::menu::glass(col().key(format!("thread-toast-{id}")).role("status").w_full().child(content).child(dismiss), k, false))
}

/// Desktop notifications and the dock badge, through what each OS offers without a bundle
/// entitlement: `osascript` and the Dock tile on macOS, `notify-send` elsewhere.
mod os {
    /// On Linux, `notify-send --wait` prints the action the user clicked; older libnotify
    /// without `--action` fails, and the plain notification shows instead.
    pub fn notify(title: &str, body: &str, thread: &str, wake: Option<rune::Waker>) {
        if cfg!(target_os = "linux") {
            let (title, body, thread) = (title.to_owned(), body.to_owned(), thread.to_owned());
            std::thread::spawn(move || {
                let out = std::process::Command::new("notify-send")
                    .args(["--app-name", "S5 Code", "--action=default=Open", "--wait", &title, &body])
                    .stdin(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .output();
                match out {
                    Ok(out) if out.status.success() => {
                        if String::from_utf8_lossy(&out.stdout).trim() == "default" {
                            super::CLICKED.lock().map(|mut c| c.push(thread)).ok();
                            if let Some(wake) = wake {
                                wake();
                            }
                        }
                    }
                    _ => {
                        let _ = std::process::Command::new("notify-send").args(["--app-name", "S5 Code", &title, &body]).status();
                    }
                }
            });
            return;
        }
        let mut command = if cfg!(target_os = "macos") {
            let quote = |v: &str| format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""));
            let mut c = std::process::Command::new("osascript");
            c.arg("-e").arg(format!("display notification {} with title {}", quote(body), quote(title)));
            c
        } else if cfg!(target_os = "windows") {
            return;
        } else {
            let mut c = std::process::Command::new("notify-send");
            c.args(["--app-name", "S5 Code", title, body]);
            c
        };
        command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        // Waited on a thread so finished helpers never linger as zombies.
        std::thread::spawn(move || {
            let _ = command.status();
        });
    }

    #[cfg(target_os = "macos")]
    pub fn set_badge(count: usize) {
        use std::ffi::{CString, c_char, c_void};
        #[link(name = "objc")]
        unsafe extern "C" {
            fn objc_getClass(name: *const c_char) -> *mut c_void;
            fn sel_registerName(name: *const c_char) -> *mut c_void;
            fn objc_msgSend();
        }
        type Send0 = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
        type Send1 = unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void) -> *mut c_void;
        // SAFETY: AppKit is loaded by the window; these are the documented NSApplication,
        // NSDockTile and NSString selectors, called on the main thread with matching signatures.
        unsafe {
            let send0: Send0 = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let send1: Send1 = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let app = send0(objc_getClass(c"NSApplication".as_ptr()), sel_registerName(c"sharedApplication".as_ptr()));
            if app.is_null() {
                return;
            }
            let tile = send0(app, sel_registerName(c"dockTile".as_ptr()));
            if tile.is_null() {
                return;
            }
            let label = if count == 0 {
                std::ptr::null_mut()
            } else {
                let value = CString::new(count.to_string()).expect("digits");
                send1(objc_getClass(c"NSString".as_ptr()), sel_registerName(c"stringWithUTF8String:".as_ptr()), value.as_ptr().cast())
            };
            send1(tile, sel_registerName(c"setBadgeLabel:".as_ptr()), label.cast_const());
        }
    }

    /// Linux has no common launcher badge; Windows would need a taskbar overlay icon.
    #[cfg(not(target_os = "macos"))]
    pub fn set_badge(_count: usize) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obs(attention: Option<&str>, completed_at: Option<i64>) -> Observation {
        Observation { attention: attention.map(str::to_string), settled: completed_at.is_some(), completed_at }
    }

    #[test]
    fn the_gate_notifies_each_completion_once_and_never_on_first_sight() {
        let mut gate = Gate::default();
        assert_eq!(gate.observe("t", None, &obs(None, Some(10)), false, false), None);
        let seen = obs(None, Some(10));
        assert_eq!(gate.observe("t", Some(&seen), &seen, false, false), None);
        let working = obs(None, None);
        assert_eq!(gate.observe("t", Some(&seen), &working, false, false), None);
        assert_eq!(gate.observe("t", Some(&working), &obs(None, Some(20)), false, false), Some(Kind::Completion));
        assert_eq!(gate.observe("t", Some(&obs(None, Some(20))), &obs(None, Some(20)), false, false), None);
        // A stopped run settles quietly; archived threads never notify.
        assert_eq!(gate.observe("t", Some(&working), &obs(None, Some(30)), false, true), None);
        assert_eq!(gate.observe("t", Some(&working), &obs(Some("r:Input"), None), true, false), None);
    }

    #[test]
    fn the_gate_notifies_when_a_new_request_arrives() {
        let mut gate = Gate::default();
        let working = obs(None, None);
        assert_eq!(gate.observe("t", Some(&working), &obs(Some("r1:Approval"), None), false, false), Some(Kind::Input));
        let asking = obs(Some("r1:Approval"), None);
        assert_eq!(gate.observe("t", Some(&asking), &asking, false, false), None);
        assert_eq!(gate.observe("t", Some(&asking), &obs(Some("r1:Input"), None), false, false), Some(Kind::Input));
    }

    #[test]
    fn rows_map_to_attention_and_settled_completion() {
        let base = json!({"id": "t", "latestRunId": "r1", "updatedAt": "2026-01-01T00:00:10.000Z"});
        let mut done = base.clone();
        done["status"] = json!("completed");
        done["latestRunCompletedAt"] = json!("2026-01-01T00:00:09.000Z");
        let (_, status, o) = observe_row(&done);
        assert_eq!(status, Status::Ready);
        assert!(o.settled && o.attention.is_none());
        assert_eq!(o.completed_at, data::iso_ms("2026-01-01T00:00:09.000Z"));
        let mut asking = base.clone();
        asking["status"] = json!("running");
        asking["pendingRuntimeRequest"] = json!({"kind": "user_input"});
        let (_, status, o) = observe_row(&asking);
        assert_eq!(status, Status::Input);
        assert_eq!(o.attention.as_deref(), Some("r1:Input"));
        let mut failed = base;
        failed["status"] = json!("failed");
        let (_, status, o) = observe_row(&failed);
        assert_eq!(status, Status::Failed);
        assert_eq!(title_for(Kind::Input, status), "Thread failed");
        assert!(!o.settled);
    }
}
