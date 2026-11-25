//! Thread notifications (apps/web `ThreadNotificationCoordinator`): a thread finishing or asking
//! for input plays a sound, shows an in-app toast while the window is focused on another thread,
//! or a desktop notification while the window is in the background, and the dock badge counts
//! the desktop notifications not yet seen. Settings > General's "Notifications" and "In-app
//! notifications" drive it.

use std::collections::{HashMap, HashSet};

use rune::Tx;

use crate::shell::sidebar::logic::{self, Status, Thread};
use crate::{App, Route, icons};

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

/// One environment's watch (`WatchedEnvironment`): each keeps its own gate, so an environment that
/// drops leaves the others notifying.
#[derive(Default)]
struct Watched {
    gate: Gate,
    previous: HashMap<String, Observation>,
    revision: Option<u64>,
}

#[derive(Default)]
pub struct Ui {
    watched: HashMap<String, Watched>,
    /// The native window is in the background (`document.hasFocus()` is false).
    pub background: bool,
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
    // A modifier released in another app never reaches this window.
    crate::shell::sidebar::release_jump(s);
    if focused {
        clear_pending(s);
    }
}

fn clear_pending(s: &mut App) {
    os::withdraw(&s.ui.notify.pending);
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

/// Electron's `windows.reveal`: show, restore and focus the main window.
pub fn reveal_window() {
    WINDOW.with(|w| w.get().map(|h| h.open()));
}

/// Threads whose desktop notification was clicked, from the helper processes' threads.
static CLICKED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// A clicked notification brings the window forward on its thread, as Electron's does.
fn open_clicked(s: &mut App) -> bool {
    let Some(id) = CLICKED.lock().map(|mut c| c.pop()).ok().flatten() else { return false };
    CLICKED.lock().map(|mut c| c.clear()).ok();
    reveal_window();
    match id.strip_prefix(crate::pages::pr_guide::NOTIFICATION) {
        Some(url) => crate::pages::pr_guide::open_guide(s, url),
        None => crate::shell::sidebar::open_thread(s, &id),
    }
    true
}

/// The notification sound for something other than a thread (a review guide run settling).
pub fn sound(s: &mut App, kind: Kind) {
    if has_sound(mode(s)) {
        play_sound(s, kind);
    }
}

/// A desktop notification for something other than a thread; a click hands `id` back here.
pub fn desktop(s: &App, title: &str, body: &str, id: &str) {
    if has_desktop(mode(s)) {
        os::notify(title, body, id, s.ui.panel.waker.clone());
    }
}

pub fn poll(s: &mut App, tx: &mut Tx) -> bool {
    if let Some(waker) = &s.ui.panel.waker {
        os::listen(waker.clone());
    }
    let _ = tx;
    open_clicked(s) | observe(s)
}

fn observe(s: &mut App) -> bool {
    let mode = mode(s).to_string();
    if s.ui.notify.applied_mode.as_deref() != Some(mode.as_str()) {
        s.ui.notify.applied_mode = Some(mode.clone());
        clear_pending(s);
    }
    // Notifications off: forget everything, so turning them on starts from the current state.
    if mode == "off" && !in_app(s) {
        s.ui.notify.watched.clear();
        return false;
    }
    let keys = crate::env::keys(s);
    s.ui.notify.watched.retain(|k, _| keys.contains(k));
    let mut presented = vec![];
    for key in keys {
        let mut watched = s.ui.notify.watched.remove(&key).unwrap_or_default();
        // A reconnecting shell keeps its last observations, as the web's `reconnecting` entry does.
        if let Some(st) = crate::env::source(s, &key).map(|src| src.state())
            && st.shell_ready
            && watched.revision != Some(st.shell_revision)
        {
            watched.revision = Some(st.shell_revision);
            let mut next = HashMap::new();
            // The focused environment also holds the other environments' copied rows; those are theirs.
            for row in st.threads.iter().chain(st.archived.iter()).filter(|r| r.get(crate::env::TAG).is_none()) {
                let (thread, status, observation) = observe_row(row);
                // Subagents and side chats surface on their parent.
                if thread.hidden {
                    continue;
                }
                let quiet = row["latestRunStopRequested"] == true;
                if let Some(kind) = watched.gate.observe(&thread.id, watched.previous.get(&thread.id), &observation, thread.archived, quiet) {
                    presented.push((thread.id.clone(), thread.title.clone(), status, kind));
                }
                next.insert(thread.id, observation);
            }
            watched.gate.retain(&next.keys().cloned().collect());
            watched.previous = next;
        }
        s.ui.notify.watched.insert(key, watched);
    }
    let changed = !presented.is_empty();
    for (id, title, status, kind) in presented {
        present(s, &id, title, status, kind);
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

fn present(s: &mut App, id: &str, description: String, status: Status, kind: Kind) {
    let title = title_for(kind, status);
    let mode = mode(s).to_string();
    if has_sound(&mode) {
        play_sound(s, kind);
    }
    let active = matches!(&s.route, Route::Thread(open) if open == id);
    let focused = !s.ui.notify.background;
    if in_app(s) && focused && !active {
        toast(s, id, title, description, kind, status);
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

/// The in-app notification, a toast in the app's stack as web's coordinator raises it.
fn toast(s: &mut App, id: &str, title: &'static str, description: String, kind: Kind, status: Status) {
    use crate::shell::toast::{self as stack, Tone};
    let (glyph, tone): (&'static rune::Icon, Tone) = match (kind, status) {
        (Kind::Completion, _) => (&icons::CIRCLE_CHECK, Tone::Success),
        (_, Status::Approval) => (&icons::SHIELD_QUESTION_MARK, Tone::Warning),
        (_, Status::Limited) => (&icons::CIRCLE_ALERT, Tone::Warning),
        (_, Status::Failed) => (&icons::CIRCLE_ALERT, Tone::Destructive),
        _ => (&icons::MESSAGE_CIRCLE_QUESTION_MARK, Tone::Info),
    };
    let level = match (kind, status) {
        (Kind::Completion, _) => stack::Kind::Success,
        (_, Status::Failed) => stack::Kind::Error,
        _ => stack::Kind::Warning,
    };
    let mut toast = stack::Toast::new(title, Some(&description), level);
    toast.lead = Some((glyph, tone));
    toast.hide_copy = true;
    let (thread, at) = (id.to_owned(), stack::next_id(s));
    toast.actions = vec![(
        "Open thread".into(),
        true,
        std::rc::Rc::new(move |s: &mut App, _: &mut Tx| {
            stack::close(s, at);
            crate::shell::sidebar::open_thread(s, &thread);
        }),
    )];
    stack::add(s, toast, Some(stack::TIMEOUT_MS));
}

/// Desktop notifications and the unread badge. macOS posts through Notification Center when the
/// app runs from its bundle (else `osascript`, which can't report clicks) and badges the Dock tile;
/// Linux uses `notify-send` and the launcher entry's count; Windows shows a PowerShell toast and
/// the taskbar button's overlay icon.
mod os {
    /// macOS hands a clicked notification's thread back from any thread.
    pub fn listen(wake: rune::Waker) {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            rune_mac::notify::on_click(move |click| {
                if let Some(thread) = click.get("thread") {
                    super::CLICKED.lock().map(|mut c| c.push(thread.to_owned())).ok();
                    wake();
                }
            })
        });
    }

    /// Linux notifications on screen, by the thread they're for: a newer one for the same thread
    /// replaces it, as web's notification `tag` does, and focusing the window closes them all.
    #[derive(Default)]
    pub struct Shown(Vec<(String, u32)>);

    impl Shown {
        pub fn id(&self, thread: &str) -> Option<u32> {
            self.0.iter().find(|(t, _)| t == thread).map(|&(_, id)| id)
        }

        pub fn record(&mut self, thread: &str, id: u32) {
            self.0.retain(|(t, _)| t != thread);
            self.0.push((thread.to_owned(), id));
        }

        /// The notification closed (its helper exited); a replacement keeps its id.
        pub fn forget(&mut self, thread: &str, id: u32) {
            self.0.retain(|(t, i)| !(t == thread && *i == id));
        }

        pub fn take(&mut self, threads: &[String]) -> Vec<u32> {
            let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.0).into_iter().partition(|(t, _)| threads.contains(t));
            self.0 = kept;
            gone.into_iter().map(|(_, id)| id).collect()
        }
    }

    static SHOWN: std::sync::Mutex<Shown> = std::sync::Mutex::new(Shown(Vec::new()));

    pub fn notify_send_args(title: &str, body: &str, replace: Option<u32>) -> Vec<String> {
        let mut args = vec!["--app-name".to_owned(), "S5 Code".to_owned(), "--action=default=Open".to_owned(), "--wait".to_owned(), "--print-id".to_owned()];
        args.extend(replace.map(|id| format!("--replace-id={id}")));
        args.extend([title.to_owned(), body.to_owned()]);
        args
    }

    /// Closes the desktop notifications still showing for these threads.
    pub fn withdraw(threads: &[String]) {
        if cfg!(target_os = "linux") {
            let ids = SHOWN.lock().map(|mut s| s.take(threads)).unwrap_or_default();
            if ids.is_empty() {
                return;
            }
            std::thread::spawn(move || {
                for id in ids {
                    let _ = std::process::Command::new("gdbus")
                        .args(["call", "--session", "--dest", "org.freedesktop.Notifications", "--object-path", "/org/freedesktop/Notifications", "--method", "org.freedesktop.Notifications.CloseNotification", &id.to_string()])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                }
            });
            return;
        }
        let ids: Vec<&str> = threads.iter().map(String::as_str).collect();
        rune_mac::notify::remove(&ids);
    }

    /// On Linux, `notify-send --wait --print-id` prints the notification's id, then the action
    /// the user clicked; older libnotify without `--action` fails, and the plain notification
    /// shows instead.
    pub fn notify(title: &str, body: &str, thread: &str, wake: Option<rune::Waker>) {
        if cfg!(target_os = "linux") {
            use std::io::BufRead;
            let (title, body, thread) = (title.to_owned(), body.to_owned(), thread.to_owned());
            let replace = SHOWN.lock().ok().and_then(|s| s.id(&thread));
            std::thread::spawn(move || {
                let child = std::process::Command::new("notify-send")
                    .args(notify_send_args(&title, &body, replace))
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                let Ok(mut child) = child else { return };
                let (mut id, mut clicked) = (None, false);
                if let Some(out) = child.stdout.take() {
                    for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                        match (id, line.trim().parse::<u32>()) {
                            (None, Ok(n)) => {
                                id = Some(n);
                                SHOWN.lock().map(|mut s| s.record(&thread, n)).ok();
                            }
                            _ => clicked |= line.trim() == "default",
                        }
                    }
                }
                let ok = child.wait().is_ok_and(|s| s.success());
                if let Some(n) = id {
                    SHOWN.lock().map(|mut s| s.forget(&thread, n)).ok();
                }
                if clicked {
                    super::CLICKED.lock().map(|mut c| c.push(thread)).ok();
                    if let Some(wake) = wake {
                        wake();
                    }
                } else if !ok && id.is_none() {
                    let _ = std::process::Command::new("notify-send").args(["--app-name", "S5 Code", &title, &body]).status();
                }
            });
            return;
        }
        // The toast's PowerShell stays to hear a click, which Windows raises only in the process that
        // showed it; with no app id of our own, a later click in Action Center opens nothing.
        if cfg!(target_os = "windows") {
            use std::io::BufRead;
            let (title, body, thread) = (title.to_owned(), body.to_owned(), thread.to_owned());
            std::thread::spawn(move || {
                let mut command = std::process::Command::new("powershell");
                command.args(["-NoProfile", "-NonInteractive", "-Command", WINDOWS_TOAST]).env("S5_TOAST_TITLE", &title).env("S5_TOAST_BODY", &body);
                command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
                #[cfg(windows)]
                std::os::windows::process::CommandExt::creation_flags(&mut command, 0x0800_0000);
                let Ok(mut child) = command.spawn() else { return };
                let clicked = child.stdout.take().is_some_and(|out| std::io::BufReader::new(out).lines().map_while(Result::ok).any(|line| line.trim() == "activated"));
                let _ = child.wait();
                if clicked {
                    super::CLICKED.lock().map(|mut c| c.push(thread)).ok();
                    if let Some(wake) = wake {
                        wake();
                    }
                }
            });
            return;
        }
        if rune_mac::notify::available() {
            rune_mac::notify::post(&rune_mac::notify::Notification::new(thread, title).body(body).info("thread", thread));
            return;
        }
        let mut command = if cfg!(target_os = "macos") {
            let quote = |v: &str| format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""));
            let mut c = std::process::Command::new("osascript");
            c.arg("-e").arg(format!("display notification {} with title {}", quote(body), quote(title)));
            c
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

    /// The toast template, filled from the environment so no text is parsed as script. It posts as
    /// PowerShell, the one app id Windows accepts unregistered, then prints `activated` if it is
    /// clicked within ten minutes. Moving to Action Center (a `TimedOut` dismissal) keeps waiting.
    pub(super) const WINDOWS_TOAST: &str = "[void][Windows.UI.Notifications.ToastNotificationManager,Windows.UI.Notifications,ContentType=WindowsRuntime];\
        $x=[Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02);\
        $t=$x.GetElementsByTagName('text');[void]$t.Item(0).AppendChild($x.CreateTextNode($env:S5_TOAST_TITLE));\
        [void]$t.Item(1).AppendChild($x.CreateTextNode($env:S5_TOAST_BODY));\
        $n=[Windows.UI.Notifications.ToastNotification]::new($x);\
        [void](Register-ObjectEvent -InputObject $n -EventName Activated -SourceIdentifier a);\
        [void](Register-ObjectEvent -InputObject $n -EventName Dismissed -SourceIdentifier d);\
        [void](Register-ObjectEvent -InputObject $n -EventName Failed -SourceIdentifier f);\
        [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe').Show($n);\
        $end=(Get-Date).AddMinutes(10);\
        while(($left=[int]($end-(Get-Date)).TotalSeconds) -gt 0){$e=Wait-Event -Timeout $left;if(-not $e){break};Remove-Event -EventIdentifier $e.EventIdentifier;\
        if($e.SourceIdentifier -eq 'a'){'activated';break};if($e.SourceIdentifier -ne 'd' -or [int]$e.SourceEventArgs.Reason -ne 2){break}}";

    /// `com.canonical.Unity.LauncherEntry.Update` for the app's `.desktop` file, which docks such as
    /// KDE's task manager and Dash to Dock draw as a count on the launcher.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn launcher_entry(count: usize) -> Vec<String> {
        [
            "emit",
            "--session",
            "--object-path",
            "/com/canonical/unity/launcherentry/s5code",
            "--signal",
            "com.canonical.Unity.LauncherEntry.Update",
            &format!("application://{}.desktop", crate::APP_ID),
            &format!("{{'count': <int64 {count}>, 'count-visible': <{}>}}", count > 0),
        ]
        .map(str::to_owned)
        .to_vec()
    }

    #[cfg(target_os = "linux")]
    pub fn set_badge(count: usize) {
        let mut command = std::process::Command::new("gdbus");
        command.args(launcher_entry(count)).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        std::thread::spawn(move || {
            let _ = command.status();
        });
    }

    /// Electron's `setOverlayIcon`: web's red count badge on the taskbar button, cleared at zero.
    #[cfg(windows)]
    pub fn set_badge(count: usize) {
        let icon = (count > 0).then(|| icon_resource(&badge_pixels(count), BADGE_SIDE));
        let label = if count > 0 { format!("{count} threads with new notifications") } else { String::new() };
        let label: Vec<u16> = label.encode_utf16().chain([0]).collect();
        let _ = crate::desktop::with_window(|window| {
            window.with_raw_handle(|handle| {
                let rune::raw_window_handle::RawWindowHandle::Win32(h) = handle.as_raw() else { return };
                // SAFETY: on the UI thread, with the live window's HWND and a NUL-terminated label.
                unsafe { taskbar::set_overlay(h.hwnd.get(), icon.as_deref(), BADGE_SIDE as i32, &label) };
            })
        });
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    pub fn set_badge(_count: usize) {}

    pub(super) const BADGE_SIDE: usize = 32;
    /// 3×5 digits, a row's three bits left to right.
    const DIGITS: [[u8; 5]; 10] = [
        [0b111, 0b101, 0b101, 0b101, 0b111],
        [0b010, 0b110, 0b010, 0b010, 0b111],
        [0b111, 0b001, 0b111, 0b100, 0b111],
        [0b111, 0b001, 0b111, 0b001, 0b111],
        [0b101, 0b101, 0b111, 0b001, 0b001],
        [0b111, 0b100, 0b111, 0b001, 0b111],
        [0b111, 0b100, 0b111, 0b101, 0b111],
        [0b111, 0b001, 0b010, 0b010, 0b010],
        [0b111, 0b101, 0b111, 0b101, 0b111],
        [0b111, 0b101, 0b111, 0b001, 0b111],
    ];
    const PLUS: [u8; 5] = [0b000, 0b010, 0b111, 0b010, 0b000];

    /// Web's `setNotificationBadge` canvas as RGBA: a #e5484d disc with the count in white, "9+"
    /// past nine.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(super) fn badge_pixels(count: usize) -> Vec<u8> {
        let side = BADGE_SIDE;
        let glyphs = if count > 9 { vec![DIGITS[9], PLUS] } else { vec![DIGITS[count]] };
        let scale = if glyphs.len() > 1 { 3 } else { 4 };
        let (width, height) = ((glyphs.len() * 4 - 1) * scale, 5 * scale);
        let (left, top) = ((side - width) / 2, (side - height) / 2);
        let (center, radius) = (side as f32 / 2.0, side as f32 * 28.0 / 64.0);
        let mut out = vec![0u8; side * side * 4];
        for y in 0..side {
            for x in 0..side {
                let distance = ((x as f32 + 0.5 - center).powi(2) + (y as f32 + 0.5 - center).powi(2)).sqrt();
                let coverage = (radius - distance + 0.5).clamp(0.0, 1.0);
                if coverage == 0.0 {
                    continue;
                }
                let ink = (left..left + width).contains(&x) && (top..top + height).contains(&y) && {
                    let (column, row) = ((x - left) / scale, (y - top) / scale);
                    column % 4 != 3 && glyphs[column / 4][row] >> (2 - column % 4) & 1 == 1
                };
                let i = (y * side + x) * 4;
                let rgb: [u8; 3] = if ink { [255, 255, 255] } else { [0xe5, 0x48, 0x4d] };
                out[i..i + 3].copy_from_slice(&rgb);
                out[i + 3] = (coverage * 255.0).round() as u8;
            }
        }
        out
    }

    /// RGBA as an icon resource for `CreateIconFromResourceEx`: a 32-bit DIB, bottom-up BGRA,
    /// then an all-clear AND mask so alpha alone decides.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(super) fn icon_resource(rgba: &[u8], side: usize) -> Vec<u8> {
        let mask = side * side.div_ceil(32) * 4;
        let mut out = Vec::with_capacity(40 + rgba.len() + mask);
        for value in [40, side as u32, 2 * side as u32] {
            out.extend(value.to_le_bytes());
        }
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        // No compression, size, resolution or palette.
        out.extend([0u8; 24]);
        for row in rgba.chunks_exact(side * 4).rev() {
            for p in row.chunks_exact(4) {
                out.extend([p[2], p[1], p[0], p[3]]);
            }
        }
        out.resize(out.len() + mask, 0);
        out
    }

    /// `ITaskbarList3::SetOverlayIcon`, called through its vtable.
    #[cfg(windows)]
    mod taskbar {
        use std::ffi::c_void;

        #[repr(C)]
        struct Guid(u32, u16, u16, [u8; 8]);
        const CLSID_TASKBAR_LIST: Guid = Guid(0x56FD_F344, 0xFD6D, 0x11D0, [0x95, 0x8A, 0x00, 0x60, 0x97, 0xC9, 0xA0, 0x90]);
        const IID_TASKBAR_LIST3: Guid = Guid(0xEA1A_FB91, 0x9E28, 0x4B86, [0x90, 0xE9, 0x9E, 0x9F, 0x8A, 0x5E, 0xEF, 0xAF]);
        // IUnknown (0-2), ITaskbarList (3-7), ITaskbarList2 (8), then ITaskbarList3's own.
        const RELEASE: usize = 2;
        const HR_INIT: usize = 3;
        const SET_OVERLAY_ICON: usize = 18;

        #[link(name = "ole32")]
        unsafe extern "system" {
            fn CoInitializeEx(reserved: *const c_void, model: u32) -> i32;
            fn CoUninitialize();
            fn CoCreateInstance(clsid: *const Guid, outer: *mut c_void, context: u32, iid: *const Guid, out: *mut *mut c_void) -> i32;
        }
        #[link(name = "user32")]
        unsafe extern "system" {
            fn CreateIconFromResourceEx(bits: *const u8, size: u32, icon: i32, version: u32, cx: i32, cy: i32, flags: u32) -> *mut c_void;
            fn DestroyIcon(icon: *mut c_void) -> i32;
        }

        /// # Safety
        /// `hwnd` is a live top-level window and `label` ends in NUL.
        pub unsafe fn set_overlay(hwnd: isize, icon: Option<&[u8]>, side: i32, label: &[u16]) {
            unsafe {
                // Apartment-threaded, as winit's OLE setup already made the UI thread.
                let initialized = CoInitializeEx(std::ptr::null(), 0x2) >= 0;
                let mut list: *mut c_void = std::ptr::null_mut();
                if CoCreateInstance(&CLSID_TASKBAR_LIST, std::ptr::null_mut(), 0x1, &IID_TASKBAR_LIST3, &mut list) >= 0 && !list.is_null() {
                    let vtable = *(list as *const *const usize);
                    let release: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(*vtable.add(RELEASE));
                    let hr_init: unsafe extern "system" fn(*mut c_void) -> i32 = std::mem::transmute(*vtable.add(HR_INIT));
                    let set: unsafe extern "system" fn(*mut c_void, isize, *mut c_void, *const u16) -> i32 = std::mem::transmute(*vtable.add(SET_OVERLAY_ICON));
                    if hr_init(list) >= 0 {
                        let icon = icon.map_or(std::ptr::null_mut(), |bits| CreateIconFromResourceEx(bits.as_ptr(), bits.len() as u32, 1, 0x0003_0000, side, side, 0));
                        set(list, hwnd, icon, label.as_ptr());
                        if !icon.is_null() {
                            DestroyIcon(icon);
                        }
                    }
                    release(list);
                }
                if initialized {
                    CoUninitialize();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data;
    use serde_json::json;

    /// Rune opens them on this platform: AVFoundation on macOS, its own decoder on Linux.
    #[test]
    fn the_notification_sounds_open() {
        for name in ["notification-completion.mp3", "notification-input.mp3"] {
            let clip = rune::Player::audio(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(name));
            assert_eq!(clip.state(), rune::PlayerState::Ready, "{name}");
            assert!(clip.duration().is_some_and(|d| d > 0.1), "{name}: {:?}", clip.duration());
        }
    }

    #[test]
    fn the_windows_badge_draws_webs_disc_and_count_as_an_icon() {
        let side = os::BADGE_SIDE;
        let at = |pixels: &[u8], x: usize, y: usize| pixels[(y * side + x) * 4..][..4].to_vec();
        let one = os::badge_pixels(1);
        assert_eq!(at(&one, 0, 0), [0, 0, 0, 0], "outside the disc is clear");
        assert_eq!(at(&one, 16, 4), [0xe5, 0x48, 0x4d, 255], "the disc is web's red");
        // "1" at 4x from (10, 6): its stem is the middle column, its foot the full bottom row.
        assert_eq!(at(&one, 15, 8), [255, 255, 255, 255]);
        assert_eq!(at(&one, 11, 8)[..3], [0xe5, 0x48, 0x4d], "left of the stem's top is disc");
        assert_eq!(at(&one, 11, 24), [255, 255, 255, 255]);
        let many = os::badge_pixels(12);
        assert_ne!(many, os::badge_pixels(9), "past nine reads 9+");
        assert_eq!(many, os::badge_pixels(40));

        let icon = os::icon_resource(&one, side);
        assert_eq!(icon.len(), 40 + side * side * 4 + side * 4);
        assert_eq!(icon[..16], [40, 0, 0, 0, 32, 0, 0, 0, 64, 0, 0, 0, 1, 0, 32, 0], "a 32-bit DIB twice as tall, for its AND mask");
        let disc_top_stored_near_the_end = 40 + (side - 4) * side * 4 + 16 * 4;
        assert_eq!(icon[disc_top_stored_near_the_end..][..4], [0x4d, 0x48, 0xe5, 255], "rows bottom-up, BGRA");
        assert!(icon[40 + side * side * 4..].iter().all(|b| *b == 0));
    }

    #[cfg(windows)]
    #[test]
    fn the_windows_toast_script_parses() {
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", "$null=[scriptblock]::Create($env:S5_TOAST_SCRIPT); 'parsed'"])
            .env("S5_TOAST_SCRIPT", os::WINDOWS_TOAST)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "parsed", "{}", String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn the_linux_badge_counts_on_the_apps_launcher_entry() {
        let args = os::launcher_entry(3);
        assert_eq!(args[6], format!("application://{}.desktop", crate::APP_ID));
        assert_eq!(args[7], "{'count': <int64 3>, 'count-visible': <true>}");
        assert_eq!(os::launcher_entry(0)[7], "{'count': <int64 0>, 'count-visible': <false>}");
    }

    #[test]
    fn a_threads_linux_notification_replaces_its_last_until_focus_closes_them() {
        let mut shown = os::Shown::default();
        assert!(!os::notify_send_args("Thread completed", "Fix tests", None).iter().any(|a| a.starts_with("--replace-id")));
        shown.record("t1", 7);
        shown.record("t2", 9);
        let args = os::notify_send_args("Input needed", "Fix tests", shown.id("t1"));
        assert!(args.contains(&"--replace-id=7".to_owned()) && args.contains(&"--print-id".to_owned()), "{args:?}");
        assert_eq!(args[args.len() - 2..], ["Input needed".to_owned(), "Fix tests".to_owned()]);

        // The replaced helper exits with the id the replacement now holds: the newer one stays.
        shown.record("t1", 7);
        shown.forget("t1", 6);
        assert_eq!(shown.id("t1"), Some(7));
        assert_eq!(shown.take(&["t1".to_owned()]), vec![7]);
        assert_eq!((shown.id("t1"), shown.id("t2")), (None, Some(9)));
        shown.forget("t2", 9);
        assert_eq!(shown.take(&["t2".to_owned()]), Vec::<u32>::new());
    }

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

    /// One replay turn's `thread.updated` rows, as a server streamed them on `orchestration.subscribeShell`.
    fn turn(id: &str, run: &str, done_at: &str) -> Vec<serde_json::Value> {
        let row = |status: &str, activity: Option<&str>, done: bool| {
            json!({
                "id": id, "projectId": "p", "title": format!("Title {id}"), "lineage": {}, "archivedAt": null,
                "latestRunId": run, "activeRunId": if done || status == "waiting" { json!(null) } else { json!(run) },
                "status": status, "activityRunStatus": activity, "latestRunCompletedAt": if done { json!(done_at) } else { json!(null) },
                "latestRunStopRequested": false, "pendingRuntimeRequest": null, "pendingBackgroundTasks": [],
                "activeProviderThreadId": "provider-thread:1", "updatedAt": done_at,
            })
        };
        vec![row("starting", Some("starting"), false), row("running", Some("running"), false), row("waiting", Some("waiting"), false), row("completed", None, true)]
    }

    #[test]
    fn a_finished_turn_toasts_once_on_every_environment_even_when_the_focused_one_drops() {
        let mut s = App::new();
        s.client_settings.values.insert("inAppNotificationsEnabled".into(), json!(true));
        let mut local = data::empty_state();
        local.shell_ready = true;
        s.src = data::Source::Fixture(local);
        let titles = |s: &App| crate::shell::toast::titles(s).into_iter().map(|(t, d)| (t, d.unwrap_or_default())).collect::<Vec<_>>();
        let here = |s: &mut App, row: &serde_json::Value| {
            if let data::Source::Fixture(st) = &mut s.src {
                st.threads.retain(|t| t["id"] != row["id"]);
                st.threads.push(row.clone());
                st.shell_revision += 1;
            }
            observe(s);
        };

        for row in turn("t1", "run:1", "2026-11-11T17:56:04.759Z") {
            here(&mut s, &row);
        }
        assert_eq!(titles(&s), [("Thread completed".to_string(), "Title t1".to_string())]);
        here(&mut s, &turn("t1", "run:1", "2026-11-11T17:56:04.759Z")[3]);
        assert_eq!(titles(&s).len(), 1, "each completion notifies once");

        // A thread on a parked environment finishing while this machine is focused.
        s.ui.env.saved.push(crate::env::Saved { id: "box".into(), label: "Box".into(), base: "https://box.example".into(), bearer: String::new() });
        let mut remote = data::empty_state();
        remote.shell_ready = true;
        crate::env::park(&mut s, "box", data::Source::Fixture(remote));
        for row in turn("t2", "run:2", "2026-11-11T17:57:00.000Z") {
            let Some(data::Source::Fixture(st)) = crate::env::source_mut(&mut s, "box") else { panic!("parked") };
            st.threads = vec![row];
            st.shell_revision += 1;
            crate::env::drain(&mut s);
            observe(&mut s);
        }
        assert_eq!(titles(&s)[1], ("Thread completed".to_string(), "Title t2".to_string()));

        // The thread on screen never toasts (`present`'s active-thread check).
        s.route = Route::Thread("t1".into());
        for row in turn("t1", "run:3", "2026-11-11T17:58:00.000Z") {
            here(&mut s, &row);
        }
        assert_eq!(titles(&s).len(), 2);

        // In the background, toasts give way to desktop notifications, which this mode leaves off.
        s.route = Route::Home { project_id: None };
        window_focus(&mut s, false);
        for row in turn("t1", "run:4", "2026-11-11T17:59:00.000Z") {
            here(&mut s, &row);
        }
        assert_eq!(titles(&s).len(), 2);
        assert!(s.ui.notify.pending.is_empty());

        // The focused remote drops; this machine's threads still notify.
        window_focus(&mut s, true);
        crate::env::switch(&mut s, "box");
        if let data::Source::Fixture(st) = &mut s.src {
            st.shell_ready = false;
            st.conn = s5_net::Conn::Closed("connection lost".into());
        }
        for row in turn("t5", "run:5", "2026-11-11T18:00:00.000Z") {
            let Some(data::Source::Fixture(st)) = crate::env::source_mut(&mut s, crate::env::LOCAL) else { panic!("parked") };
            st.threads.retain(|t| t["id"] != row["id"]);
            st.threads.push(row);
            st.shell_revision += 1;
            observe(&mut s);
        }
        assert_eq!(titles(&s)[2], ("Thread completed".to_string(), "Title t5".to_string()));
    }
}
