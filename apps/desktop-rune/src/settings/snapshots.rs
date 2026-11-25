//! Web `SnapShotSettings` and its `SnapShotSetupDialog`, through `desktop::capture` on macOS and
//! `desktop::capture_linux` on Wayland. Elsewhere this client has no capture backend, so web's
//! `SettingsUnavailableGroup` shows instead, as web shows it without a bridge.

use super::controls as c;
use crate::desktop::capture::{self, State, Test};
use crate::desktop::capture_config::{self as config, Desktop, Operation};
use crate::desktop::capture_linux::{self as linux, Backend, ExtensionStatus as Ext};
use crate::theme::{self, AlphaMul, T};
use crate::{App, Route, icons, kit};
use rune::{El, col, el, icon, row, text};
use serde_json::{Value, json};

/// `snapShotUnavailableMessage` inside the desktop app.
const UNAVAILABLE: &str = "Update the desktop app to use snapshots.";
/// `snapShotDescription` for direct capture.
const DESCRIPTION: &str = "Capture a window and attach it to your current draft.";
const PICKER_DESCRIPTION: &str = "Automatic capture isn't available here. Choose a window instead.";
/// `snapShotFeedbackUnavailableMessage`: this client draws no capture effects on Linux.
const NO_EFFECTS: &str = "Capture effects aren't available on this desktop.";
const SOUNDS: [(&str, &str); 3] = [("off", "Off"), ("soft-pop", "Whoosh (Default)"), ("camera-shutter", "Click")];
const SOUND_WIDTH: f32 = 176.0;
/// `US_SHIFTED` in the keybindings recorder: a shifted symbol records as its key.
const US_SHIFTED: [(char, char); 21] = [
    ('!', '1'), ('@', '2'), ('#', '3'), ('$', '4'), ('%', '5'), ('^', '6'), ('&', '7'), ('*', '8'), ('(', '9'), (')', '0'),
    ('_', '-'), ('+', '='), ('{', '['), ('}', ']'), ('|', '\\'), (':', ';'), ('"', '\''), ('<', ','), ('>', '.'), ('?', '/'), ('~', '`'),
];

#[derive(Default)]
pub struct Ui {
    /// Scenes stand in for the macOS backend.
    forced: Option<State>,
    wizard: Option<Wizard>,
    recording: bool,
    held: Vec<rune::Modifier>,
    /// An unsaved shortcut and whether it can be saved.
    candidate: Option<(Value, Result<(), String>)>,
    sound_open: bool,
    advanced: bool,
    config: Config,
}

type Slot<O> = std::sync::Arc<std::sync::Mutex<Option<O>>>;

/// `CaptureShortcutConfig` on Niri and Hyprland: keys, the reviewed change, and its result.
#[derive(Default)]
struct Config {
    /// Recorded keys; unset means the preview's or Ctrl+Shift+2.
    keys: Option<Value>,
    preview: Option<config::Preview>,
    applied: Option<config::Applied>,
    /// The message, and Advanced's troubleshooting detail.
    error: Option<(&'static str, Option<String>)>,
    /// A file chosen in Advanced, read again for later reviews.
    file: Option<std::path::PathBuf>,
    reading: Option<Slot<Result<config::Preview, String>>>,
    writing: Option<Slot<Result<config::Applied, String>>>,
    advanced: bool,
    copied_until: i64,
}

impl Config {
    fn working(&self) -> bool {
        self.reading.is_some() || self.writing.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    Access,
    Shortcut,
}

#[derive(Clone, Copy, Debug)]
struct Wizard {
    step: Step,
    was_enabled: bool,
}

fn state(s: &App) -> State {
    s.ui.settings.snapshots.forced.clone().unwrap_or_else(capture::state)
}

fn enabled(s: &App) -> bool {
    super::value(s, "snapShotEnabled") == true
}

fn saved(s: &App) -> Value {
    let v = super::value(s, "snapShotShortcut");
    if v.is_null() { capture::pair_shortcut("shift") } else { v }
}

/// `captureSetupBackend`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Direct,
    Portal,
    Gnome,
    Picker,
    Hyprland,
    Niri,
    Kde,
}

/// `captureSetupDesktopName`.
fn desktop_name(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::Gnome => Some("GNOME"),
        Kind::Hyprland => Some("Hyprland"),
        Kind::Niri => Some("Niri"),
        Kind::Kde => Some("KDE Plasma"),
        _ => None,
    }
}

/// `snapShotFeedbackUnavailableMessage`: GNOME's extension draws them; on Hyprland and KDE
/// (one output) the app does.
fn effects_status(state: &State) -> Option<&'static str> {
    match kind(state) {
        Kind::Direct | Kind::Hyprland | Kind::Kde => None,
        Kind::Niri => Some("Capture effects aren't available on Niri."),
        Kind::Gnome if extension(state).map(|(status, _)| status) == Some(Ext::Enabled) => None,
        Kind::Gnome => Some("Finish extension setup to enable effects."),
        Kind::Portal | Kind::Picker => Some(NO_EFFECTS),
    }
}

/// Linux portal mode; scenes force it through `State::linux`.
fn on_linux(state: &State) -> bool {
    state.linux.is_some() || (state.supported && capture::linux())
}

fn extension(state: &State) -> Option<(Ext, &str)> {
    state.linux.as_ref()?.extension.as_ref().map(|(status, message)| (*status, message.as_str()))
}

fn kind(state: &State) -> Kind {
    if !on_linux(state) {
        return Kind::Direct;
    }
    let Some(setup) = &state.linux else { return Kind::Picker };
    let ext = extension(state).map(|(status, _)| status);
    match setup.backend {
        Backend::Hyprland => Kind::Hyprland,
        Backend::Niri => Kind::Niri,
        Backend::Kde => Kind::Kde,
        Backend::ScreenshotPortal => Kind::Portal,
        Backend::Picker if ext == Some(Ext::Unsupported) => Kind::Picker,
        Backend::GnomeExtension => Kind::Gnome,
        _ if ext.is_some() => Kind::Gnome,
        _ => Kind::Picker,
    }
}

/// Niri and Hyprland bind capture in their own config instead of a recorded shortcut.
fn managed(state: &State) -> Option<Desktop> {
    match kind(state) {
        Kind::Niri => Some(Desktop::Niri),
        Kind::Hyprland => Some(Desktop::Hyprland),
        _ => None,
    }
}

/// `captureSetupAccessReady`.
fn access_ready(state: &State) -> bool {
    if state.message.is_some() || (on_linux(state) && state.linux.is_none()) {
        return false;
    }
    kind(state) != Kind::Gnome || (extension(state).map(|(status, _)| status) == Some(Ext::Enabled) && state.linux.as_ref().is_some_and(|l| l.backend == Backend::GnomeExtension))
}

/// `captureSetupMacPermissionsReady`: Screen Recording, and Accessibility when app text is on.
fn permissions_ready(state: &State, include_text: bool) -> bool {
    state.permissions.is_some_and(|p| p.screen_recording && (!include_text || p.accessibility))
}

fn include_text(s: &App) -> bool {
    super::value(s, "snapShotIncludeAccessibility") != false
}

/// `snapShotStatus`.
fn status(state: &State, enabled: bool) -> &'static str {
    if on_linux(state) {
        return linux_status(state, enabled);
    }
    if !enabled {
        "Turn this on to set up snapshots."
    } else if state.permissions.is_none() {
        "Checking snapshots…"
    } else if state.message.is_some() {
        "Capture needs attention"
    } else if state.shortcut_verified {
        "Ready to capture"
    } else if state.shortcut_registered {
        "Shortcut saved"
    } else {
        "Finish shortcut setup"
    }
}

/// `snapShotStatus` and `snapShotSetupSummary` in portal mode.
fn linux_status(state: &State, enabled: bool) -> &'static str {
    if state.linux.is_none() && state.message.is_none() {
        "Checking snapshots…"
    } else if !enabled {
        "Turn this on to set up snapshots."
    } else if state.message.is_some() {
        "Capture needs attention"
    } else if kind(state) == Kind::Gnome && extension(state).map(|(status, _)| status) != Some(Ext::Enabled) {
        "Set up active-window snapshots"
    } else if kind(state) == Kind::Picker {
        "Manual capture only. You'll choose a window each time"
    } else if state.shortcut_pending {
        if kind(state) == Kind::Hyprland { "Connecting your shortcut…" } else { "Waiting for shortcut permission" }
    } else if state.shortcut_verified {
        "Ready to capture"
    } else if (kind(state) == Kind::Niri && state.shortcut_binding.is_some()) || (kind(state) == Kind::Hyprland && state.shortcut_action_registered == Some(true)) {
        "Use your shortcut from another app"
    } else if state.shortcut_registered {
        "Shortcut saved"
    } else {
        "Finish shortcut setup"
    }
}

/// `snapShotSetupComplete`: web reads it from macOS permissions, so portal mode never is.
fn setup_complete(state: &State, include_text: bool) -> bool {
    !on_linux(state) && state.permissions.is_some() && access_ready(state) && permissions_ready(state, include_text) && state.shortcut_registered
}

/// `snapShotSetupButtonLabel`.
fn setup_label(state: &State) -> &'static str {
    match desktop_name(kind(state)) {
        _ if access_ready(state) => "Manage capture",
        Some("GNOME") => "Set up GNOME capture",
        Some("Hyprland") => "Set up Hyprland capture",
        Some("Niri") => "Set up Niri capture",
        Some(_) => "Set up KDE Plasma capture",
        None => "Continue setup",
    }
}

/// The shortcut status line: recording, the candidate's check, else `snapShotShortcutStatus`.
fn shortcut_status(ui: &Ui, state: &State) -> Option<String> {
    if ui.recording {
        return Some("Press your shortcut. Esc cancels.".into());
    }
    match &ui.candidate {
        Some((_, Ok(()))) => Some("Ready to save.".into()),
        Some((_, Err(message))) => Some(message.clone()),
        None if state.shortcut_pending => Some("Approve the shortcut permission prompt to continue.".into()),
        None if state.shortcut_registered => (!on_linux(state)).then(|| "Shortcut saved.".into()),
        None => state.shortcut_message.clone(),
    }
}


/// `formatShortcutKeyLabel`.
fn key_label(key: &str) -> String {
    match key {
        " " => "Space".into(),
        "escape" => "Esc".into(),
        "arrowup" => "Up".into(),
        "arrowdown" => "Down".into(),
        "arrowleft" => "Left".into(),
        "arrowright" => "Right".into(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => k.chars().take(1).flat_map(char::to_uppercase).chain(k.chars().skip(1)).collect(),
    }
}

const MAC: bool = cfg!(target_os = "macos");

fn key_labels(shortcut: &Value) -> Vec<String> {
    key_labels_for(shortcut, MAC)
}

/// `snapShotShortcutKeyLabels`: one keycap per key, names instead of glyphs off macOS.
fn key_labels_for(shortcut: &Value, mac: bool) -> Vec<String> {
    let cap = |modifier| super::controls::modifier_cap(modifier, mac).to_owned();
    if let Some(modifier) = capture::pair_modifier(shortcut) {
        return vec![cap(modifier); 2];
    }
    let flag = |k: &str| shortcut[k].as_bool().unwrap_or(false);
    let mod_key = flag("modKey");
    let mut labels: Vec<String> = [(flag("ctrlKey") || (mod_key && !mac), "control"), (flag("altKey"), "alt"), (flag("shiftKey"), "shift"), (flag("metaKey") || (mod_key && mac), "meta")]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, modifier)| cap(modifier))
        .collect();
    labels.push(key_label(shortcut["key"].as_str().unwrap_or_default()));
    labels
}

fn shortcut_label(shortcut: &Value) -> String {
    shortcut_label_for(shortcut, MAC)
}

/// `formatSnapShotShortcutLabel`: "⌃⇧2" on macOS, "Ctrl+Shift+2" elsewhere.
fn shortcut_label_for(shortcut: &Value, mac: bool) -> String {
    match capture::pair_modifier(shortcut) {
        Some(modifier) => capture::pair_label(modifier),
        None if mac => key_labels_for(shortcut, mac).concat(),
        None => key_labels_for(shortcut, mac).join("+"),
    }
}

/// `sameSnapShotShortcut`.
fn same(a: &Value, b: &Value) -> bool {
    match (capture::pair_modifier(a), capture::pair_modifier(b)) {
        (Some(x), Some(y)) => x == y,
        // `shortcutConflictKey`: mod is ⌘ on macOS and Ctrl elsewhere.
        (None, None) => key_labels(a) == key_labels(b),
        _ => false,
    }
}

fn char_key(c: char, shift: bool) -> String {
    let c = if shift { US_SHIFTED.iter().find(|(shifted, _)| *shifted == c).map_or(c, |(_, plain)| *plain) } else { c };
    c.to_lowercase().collect()
}

/// `keybindingFromKeyboardEvent` then `parseKeybindingShortcut`, as the shortcut's JSON.
fn chord(e: &rune::KeyEvent, base: Option<char>) -> Option<Value> {
    use rune::Key;
    let single = |text: &str| {
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        }
    };
    let key = match &e.key {
        Key::Text(text) => char_key(base.or_else(|| single(text.as_str()))?, e.mods.shift),
        Key::Command(c) => char_key(base.unwrap_or(*c), e.mods.shift),
        Key::Enter => "enter".into(),
        Key::Backspace => "backspace".into(),
        Key::Delete => "delete".into(),
        Key::Home => "home".into(),
        Key::End => "end".into(),
        Key::PageUp => "pageup".into(),
        Key::PageDown => "pagedown".into(),
        Key::Up => "arrowup".into(),
        Key::Down => "arrowdown".into(),
        Key::Left => "arrowleft".into(),
        Key::Right => "arrowright".into(),
        Key::Function(n) => format!("f{n}"),
        _ => return None,
    };
    let m = e.mods;
    Some(json!({"key": key, "metaKey": false, "ctrlKey": m.ctrl, "altKey": m.alt, "shiftKey": m.shift, "modKey": m.cmd || m.logo}))
}

/// Recording takes keys while its page is open; the registered shortcut is released meanwhile.
pub fn recording(s: &App) -> bool {
    s.ui.settings.snapshots.recording && matches!(&s.route, Route::Settings(p) if p == "snap-shot")
}

fn start_recording(s: &mut App) {
    let ui = &mut s.ui.settings.snapshots;
    if ui.config.working() {
        return;
    }
    ui.recording = true;
    ui.held.clear();
    ui.candidate = None;
    ui.config.error = None;
}

fn stop_recording(s: &mut App) {
    let ui = &mut s.ui.settings.snapshots;
    ui.recording = false;
    ui.held.clear();
}

/// The wizard's config recorder is open instead of the settings shortcut's.
fn config_recording(s: &App) -> bool {
    s.ui.settings.snapshots.wizard.is_some_and(|w| w.step == Step::Shortcut) && managed(&state(s)).is_some()
}

fn record(s: &mut App, shortcut: Value) {
    stop_recording(s);
    if config_recording(s) {
        // `allowModifierPairs: false`: a compositor binding needs a key.
        if capture::pair_modifier(&shortcut).is_none() {
            let c = &mut s.ui.settings.snapshots.config;
            c.keys = Some(shortcut);
            c.preview = None;
            c.error = None;
        }
        return;
    }
    let unchanged = same(&shortcut, &saved(s));
    let check = capture::availability(&shortcut);
    s.ui.settings.snapshots.candidate = (!unchanged).then_some((shortcut, check));
}

/// The recorder's keys; Escape cancels and Tab leaves, as the web recorder's button does.
pub fn on_key(s: &mut App, e: &rune::KeyEvent, tx: &mut rune::Tx) -> bool {
    match &e.key {
        rune::Key::Escape => stop_recording(s),
        rune::Key::Tab => {
            stop_recording(s);
            return false;
        }
        _ => {
            let m = e.mods;
            // A global shortcut without a modifier would take that key from every app.
            if (m.cmd || m.ctrl || m.alt || m.shift || m.logo)
                && let Some(shortcut) = chord(e, tx.key_base())
            {
                record(s, shortcut);
            }
        }
    }
    true
}

/// Both keys of one modifier record a modifier pair.
pub fn on_modifier(s: &mut App, e: &rune::KeyEvent) -> bool {
    use rune::Modifier as M;
    let rune::Key::Modifier { modifier, pressed } = &e.key else { return false };
    if !recording(s) || config_recording(s) {
        return false;
    }
    let held = &mut s.ui.settings.snapshots.held;
    if !*pressed {
        held.retain(|m| m != modifier);
        return true;
    }
    if !held.contains(modifier) {
        held.push(*modifier);
    }
    let pair = [(M::ShiftLeft, M::ShiftRight, "shift"), (M::SuperLeft, M::SuperRight, "meta"), (M::ControlLeft, M::ControlRight, "control"), (M::AltLeft, M::AltRight, "alt")]
        .into_iter()
        .find(|(left, right, _)| held.contains(left) && held.contains(right))
        .map(|(.., modifier)| modifier);
    if let Some(modifier) = pair {
        record(s, capture::pair_shortcut(modifier));
    }
    true
}

fn cancel_candidate(s: &mut App) {
    stop_recording(s);
    s.ui.settings.snapshots.candidate = None;
}

fn save_candidate(s: &mut App) -> bool {
    match s.ui.settings.snapshots.candidate.take() {
        Some((shortcut, Ok(()))) => {
            super::update(s, "snapShotShortcut", shortcut);
            true
        }
        other => {
            s.ui.settings.snapshots.candidate = other;
            false
        }
    }
}

/// `openSetup`: macOS always starts on Access, and enabling waits for a passing test.
fn open_setup(s: &mut App) {
    open_setup_at(s, Step::Access);
}

/// `captureSetupInitialStep`: a requested step past Access waits for access to be ready.
fn open_setup_at(s: &mut App, step: Step) {
    cancel_candidate(s);
    capture::reset_test();
    capture::check_permissions(s, false);
    capture::refresh_linux(s);
    let was_enabled = enabled(s);
    let step = if access_ready(&state(s)) { step } else { Step::Access };
    let ui = &mut s.ui.settings.snapshots;
    ui.advanced = false;
    ui.config = Config::default();
    ui.wizard = Some(Wizard { step, was_enabled });
}

/// `closeSetup`: leaving before finishing turns off what the wizard turned on.
fn close_setup(s: &mut App, completed: bool) {
    let Some(wizard) = s.ui.settings.snapshots.wizard.take() else { return };
    s.ui.settings.snapshots.config = Config::default();
    if enabled(s) && !wizard.was_enabled && !completed {
        super::update(s, "snapShotEnabled", json!(false));
    }
    stop_recording(s);
}

/// The test capture finished: a pass enables capture and moves to the shortcut.
pub fn tested(s: &mut App, passed: bool) {
    if !passed || s.ui.settings.snapshots.wizard.is_none() {
        return;
    }
    if !enabled(s) {
        super::update(s, "snapShotEnabled", json!(true));
    }
    if let Some(wizard) = &mut s.ui.settings.snapshots.wizard {
        wizard.step = Step::Shortcut;
    }
}

/// The Access step watches permissions while it is open, as `usePermissionStatus` polls.
pub fn checking_access(s: &App) -> bool {
    s.ui.settings.snapshots.wizard.is_some_and(|w| w.step == Step::Access) && capture::state().test != Test::Running
}

fn sound_selection(s: &App) -> &'static str {
    if super::value(s, "snapShotPlaySound") == false {
        "off"
    } else if super::value(s, "snapShotSound") == "camera-shutter" {
        "camera-shutter"
    } else {
        "soft-pop"
    }
}

fn choose_sound(s: &mut App, label: &str) {
    match SOUNDS.iter().find(|(_, l)| *l == label).map(|(id, _)| *id) {
        Some("off") => super::update(s, "snapShotPlaySound", json!(false)),
        Some(id) => {
            super::update(s, "snapShotPlaySound", json!(true));
            super::update(s, "snapShotSound", json!(id));
        }
        None => {}
    }
    s.ui.settings.snapshots.sound_open = false;
}

fn sound_select(s: &App) -> kit::Select {
    let labels: Vec<&str> = SOUNDS.iter().map(|(_, l)| *l).collect();
    let current = SOUNDS.iter().find(|(id, _)| *id == sound_selection(s)).map(|(_, l)| *l);
    kit::Select::new("setting-snap-shot-sound", &labels).current(current).width(SOUND_WIDTH).size("sm")
}

fn title(label: &str, keyed: bool, t: &T) -> El {
    let title = text(label.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground);
    if keyed { title.key(super::title_key(label)) } else { title }
}

/// Web `SettingsRow`: title, description and status beside the control.
fn setting(t: &T, heading: El, description: &str, status: Option<String>, control: El) -> El {
    let line = |copy: String| text(copy).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap();
    let mut copy = col().grow(1.0).min_w(0.0).gap(4.0).child(heading).child(line(description.to_owned()));
    if let Some(status) = status {
        copy = copy.child(line(status));
    }
    row().w_full().px(16.0).py(12.0).gap(32.0).center_y().child(copy).child(control)
}

/// A row web shows off and disabled here, with its status saying why.
fn locked_switch(t: &T, label: &str) -> El {
    kit::Switch::new(false).label(label).hide_label().disabled(true).build(&kit::theme(t))
}

/// `snapShotAccessibilityUnavailableMessage`: only backends that name the window can read its text.
fn linux_text_status(state: &State) -> Option<&'static str> {
    matches!(kind(state), Kind::Portal | Kind::Picker).then_some("This desktop only provides a screenshot.")
}

fn switch(t: &T, on: bool, label: &str, f: impl Fn(&mut App) + 'static) -> El {
    kit::Switch::new(on).label(label).hide_label().on_toggle(f).build(&kit::theme(t))
}

/// `useSnapShotShortcutRecorder`'s button: the keycaps, or the prompt while recording.
fn recorder(ui: &Ui, shortcut: &Value, t: &T) -> El {
    let mut content = row().gap(2.0).center_y();
    if ui.recording {
        content = content.child(text("Press shortcut…").font(theme::XS_MEDIUM).color(t.c.secondary_foreground));
    } else {
        for label in key_labels(shortcut) {
            content = content.child(kit::kbd(&label, t));
        }
    }
    row()
        .key("snap-shot-shortcut-recorder")
        .role("button")
        .label(format!("Record snapshot shortcut, currently {}", shortcut_label(shortcut)))
        .h(28.0)
        .px(6.0)
        .radius(8.0)
        .center_y()
        .border(1.0, t.c.border)
        .fill(if ui.recording { t.c.secondary } else { t.c.background })
        .cursor(rune::Cursor::Pointer)
        .on_press(start_recording)
        .child(content)
}

fn shortcut_controls(ui: &Ui, shortcut: &Value, t: &T) -> El {
    let mut controls = row().gap(8.0).center_y().child(recorder(ui, shortcut, t));
    if let Some((_, check)) = &ui.candidate {
        controls = controls
            .child(kit::Button::new("Save").size("xs").disabled(check.is_err()).on_press(|s: &mut App| {
                save_candidate(s);
            }).build(t))
            .child(kit::Button::new("Cancel").variant("ghost").size("xs").on_press(cancel_candidate).build(t));
    }
    controls
}

fn display_shortcut(s: &App) -> Value {
    s.ui.settings.snapshots.candidate.as_ref().map_or_else(|| saved(s), |(shortcut, _)| shortcut.clone())
}

pub fn view(s: &App, t: &T) -> El {
    let state = state(s);
    if !state.supported {
        return unavailable(s, t);
    }
    let ui = &s.ui.settings.snapshots;
    let on = enabled(s);
    let mut toggle = row().gap(8.0).center_y();
    if on && !setup_complete(&state, include_text(s)) {
        toggle = toggle.child(kit::Button::new(setup_label(&state)).variant("outline").size("xs").on_press(open_setup).build(t));
    }
    toggle = toggle.child(switch(t, on || ui.wizard.is_some(), "Enable snapshots", move |s: &mut App| {
        if on {
            super::update(s, "snapShotEnabled", json!(false));
        } else {
            open_setup(s);
        }
    }));
    let picker = kind(&state) == Kind::Picker;
    let linux = on_linux(&state);
    let description = if picker && state.linux.is_some() { PICKER_DESCRIPTION } else { DESCRIPTION };
    let mut rows = vec![setting(t, title("SnapShots", false, t), description, Some(status(&state, on).into()), toggle)];
    if on {
        let text_on = include_text(s);
        let text_control = if linux_text_status(&state).is_some() {
            locked_switch(t, "Include app text in snapshots")
        } else {
            switch(t, text_on, "Include app text in snapshots", move |s: &mut App| {
                // Linux reads AT-SPI, which asks for no permission.
                if !text_on && !linux {
                    capture::request_accessibility(s);
                }
                super::update(s, "snapShotIncludeAccessibility", json!(!text_on));
            })
        };
        rows.push(setting(t, title("Include app text", true, t), "Include text and controls when the app makes them available.", linux_text_status(&state).map(str::to_owned), text_control));
        let description = if picker && linux { "Choose a window to capture from any app." } else { "Capture the window you're using without switching apps." };
        rows.push(if managed(&state).is_some() {
            let busy = state.linux_busy || state.test == Test::Running;
            let change = kit::Button::new("Change shortcut").variant("outline").size("xs").disabled(busy).on_press(|s: &mut App| open_setup_at(s, Step::Shortcut));
            setting(t, title("Capture shortcut", true, t), description, None, change.build(t))
        } else {
            setting(t, title("Capture shortcut", true, t), description, shortcut_status(ui, &state), shortcut_controls(ui, &display_shortcut(s), t))
        });
        let sound = sound_select(s).label("Capture sound").on_toggle(|s: &mut App| s.ui.settings.snapshots.sound_open ^= true).trigger(&kit::theme(t));
        rows.push(setting(t, title("Capture sound", true, t), "Choose the sound played when capture starts.", None, row().child(sound)));
        let effects = effects_status(&state).map(str::to_owned);
        let locked = effects.is_some();
        let flash = super::value(s, "snapShotFlash") != false;
        let flash_control = if locked { locked_switch(t, "Flash captured window") } else { switch(t, flash, "Flash captured window", move |s: &mut App| super::update(s, "snapShotFlash", json!(!flash))) };
        rows.push(setting(t, title("Capture flash", true, t), "Show a gentle cue on the captured window.", effects.clone(), flash_control));
        let animate = super::value(s, "snapShotAnimations") != false;
        let animate_control = if locked { locked_switch(t, "Animate snapshots") } else { switch(t, animate, "Animate snapshots", move |s: &mut App| super::update(s, "snapShotAnimations", json!(!animate))) };
        rows.push(setting(t, title("Capture animations", true, t), "Animate captured windows into your draft.", effects, animate_control));
    }
    c::section("SnapShots", vec![c::card(rows, t)], t)
}

/// Web's `SettingsUnavailableGroup` with the bridge missing: the notice over the dimmed row.
fn unavailable(s: &App, t: &T) -> El {
    let notice = row()
        .w_full()
        .px(16.0)
        .py(8.0)
        .gap(8.0)
        .items_start()
        .child(icon(&icons::INFO, 14.0).color(t.c.warning).mt(2.0))
        .child(text(UNAVAILABLE).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap().grow(1.0).min_w(0.0));
    let on = enabled(s);
    let line = row()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(32.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .opacity(0.64)
                .child(text("SnapShots").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(text(DESCRIPTION).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap()),
        )
        .child(kit::Switch::new(on).label("Enable snapshots").hide_label().disabled(true).build(&kit::theme(t)));
    let group = col().w_full().py(6.0).fill(t.c.muted.alpha_mul(0.2)).child(notice).child(line);
    c::section("SnapShots", vec![group], t)
}

pub fn overlays(s: &App, t: &T, layer: &mut Vec<El>) {
    if !matches!(&s.route, Route::Settings(p) if p == "snap-shot") {
        return;
    }
    if s.ui.settings.snapshots.sound_open {
        let _ = sound_select(s)
            .align_end()
            .open(true)
            .on_choose(|s: &mut App, label: &str, _: &mut rune::Tx| choose_sound(s, label))
            .on_close(|s: &mut App| s.ui.settings.snapshots.sound_open = false)
            .build(&kit::theme(t), layer);
    }
    wizard(s, t, layer);
}

fn steps(current: Step, busy: bool, t: &T) -> El {
    let mut steps = row().gap(6.0).center_y();
    for (i, (step, name)) in [(Step::Access, "Access"), (Step::Shortcut, "Shortcut")].into_iter().enumerate() {
        if i > 0 {
            steps = steps.child(el().w(16.0).h(1.0).fill(t.c.border));
        }
        let reached = step == Step::Access || current == Step::Shortcut;
        let tone = if step == current { t.c.foreground } else { t.c.muted_foreground };
        let mut chip = row()
            .gap(6.0)
            .center_y()
            .child(row().size(18.0, 18.0).center().radius(9.0).fill(if reached { t.c.primary } else { t.c.muted }).child(text((i + 1).to_string()).font(theme::XS_MEDIUM).color(if reached { t.c.primary_foreground } else { t.c.muted_foreground })))
            .child(text(name).font(theme::XS_MEDIUM).color(tone));
        if step == Step::Access && current == Step::Shortcut && !busy {
            chip = chip.role("button").label(name).cursor(rune::Cursor::Pointer).on_press(|s: &mut App| leave_to(s, Step::Access));
        }
        steps = steps.child(chip);
    }
    steps
}

fn leave_to(s: &mut App, step: Step) {
    stop_recording(s);
    if let Some(wizard) = &mut s.ui.settings.snapshots.wizard {
        wizard.step = step;
    }
}

/// Web `PermissionChecklist` on macOS: Screen Recording, and Accessibility for app text.
fn checklist(state: &State, include_text: bool, busy: bool, t: &T) -> El {
    let p = state.permissions.unwrap_or_default();
    let text_copy = if include_text { "Include text and controls from the captured app." } else { "Optional. Include text and controls from the captured app." };
    let item = |icon_: &'static rune::Icon, name: &str, copy: &str, granted: bool, allow: fn(&mut App)| {
        let action = if granted {
            row().gap(4.0).center_y().role("status").child(icon(&icons::CIRCLE_CHECK, 16.0).color(t.c.success)).child(text("Allowed").font(theme::TEXT_XS).color(t.c.success))
        } else {
            kit::Button::new("Allow").variant("outline").size("xs").disabled(busy).on_press(allow).build(t)
        };
        row()
            .w_full()
            .gap(12.0)
            .px(12.0)
            .py(8.0)
            .center_y()
            .radius(8.0)
            .border(1.0, t.c.border)
            .child(icon(icon_, 24.0).color(t.c.muted_foreground))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .child(text(name.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground))
                    .child(text(copy.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()),
            )
            .child(action)
    };
    col()
        .w_full()
        .gap(8.0)
        .child(item(&icons::MONITOR, "Screen Recording", "Capture the window you're using.", p.screen_recording, |s| capture::check_permissions(s, true)))
        .child(item(&icons::TEXT_SEARCH, "Accessibility", text_copy, p.accessibility, |s| capture::request_accessibility(s)))
}

/// The Access step's heading and copy in portal mode (`accessCopy`, `GNOME_ACCESS_COPY`).
fn linux_access_copy(state: &State, ready: bool) -> (&'static str, &'static str) {
    if state.message.is_some() {
        return ("Let's try that again", "Couldn't check snapshots. Try again to continue.");
    }
    match (kind(state), extension(state).map(|(status, _)| status)) {
        (Kind::Gnome, Some(Ext::Enabled)) if !ready => ("Check capture access", "The extension isn't ready yet. Try again in a moment."),
        (Kind::Gnome, Some(status)) => match status {
            Ext::NotInstalled => ("Install the extension", "The T3 Code GNOME extension lets you capture other windows and bring them into your draft. Sign out once after installing."),
            Ext::RestartRequired => ("Extension installed", "Save your work, then sign out and back in. Your setup will be waiting here."),
            Ext::UpdateRequired => ("Update the extension", "Install the update, then sign out and back in."),
            Ext::ExtensionsDisabled => ("Allow GNOME extensions", "Open GNOME Extensions and turn on extensions, then check again."),
            Ext::Disabled => ("Enable the extension", "Enable T3 Code SnapShots to start capturing windows."),
            Ext::Enabled => ("Capture is ready", "Next, choose your shortcut."),
            Ext::Unsupported => ("Automatic capture isn't available", "Use Take snapshot from the command palette to choose a window."),
            Ext::Error => ("Couldn't set up the extension", "Check T3 Code SnapShots in GNOME Extensions, then try again."),
        },
        (Kind::Hyprland | Kind::Niri, _) => ("Capture is ready", "Next, choose your shortcut."),
        (Kind::Kde, _) => ("Capture is ready", "T3 Code registers with KDE Plasma on your first snapshot. Next, choose your shortcut."),
        (Kind::Picker, _) if state.linux.is_some() => ("Choose a window each time", "Your desktop doesn't support automatic capture. You'll choose the window to capture instead."),
        _ => ("Allow snapshots", "Your desktop may ask for permission when you first capture."),
    }
}

fn wizard(s: &App, t: &T, layer: &mut Vec<El>) {
    let ui = &s.ui.settings.snapshots;
    let Some(wizard) = ui.wizard else { return };
    let state = state(s);
    let linux = on_linux(&state);
    let kind = kind(&state);
    let ext = extension(&state).map(|(status, _)| status);
    let busy = state.test == Test::Running || state.linux_busy;
    let error = match &state.test {
        Test::Failed(message) => Some(message.clone()),
        _ => None,
    }
    .or_else(|| state.linux_error.clone());
    let ready = access_ready(&state);
    let text_on = include_text(s);
    let small = |copy: String, color| text(copy).font(theme::TEXT_XS.lh(18.0)).color(color).wrap();
    let (heading, description) = match wizard.step {
        Step::Access if linux => linux_access_copy(&state, ready),
        Step::Shortcut if managed(&state).is_some() => ("Choose your shortcut", "Click the shortcut, then press the keys you want."),
        Step::Shortcut if linux => ("Choose your shortcut", "Choose your keys, then approve the permission prompt if asked."),
        Step::Access if state.message.is_some() && state.permissions.is_none() => ("Let's try that again", "Couldn't check snapshots. Try again to continue."),
        Step::Access if permissions_ready(&state, text_on) => ("Allow snapshots", "Test a snapshot of the current window. If macOS asks to bypass its window picker, choose Allow. The test image is discarded."),
        Step::Access => ("Allow snapshots", "Allow each permission, then continue."),
        Step::Shortcut => ("Choose your shortcut", "Use both Shift keys, or record a different shortcut."),
    };
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(col().gap(8.0).child(text(heading).font(theme::SM_MEDIUM).color(t.c.foreground)).child(small(description.into(), t.c.muted_foreground)));
    match wizard.step {
        Step::Access if linux => {}
        Step::Access => body = body.child(checklist(&state, text_on, busy, t)),
        Step::Shortcut if managed(&state).is_some() => {
            if let Some(desktop) = managed(&state) {
                body = body.child(config_panel(s, &state, desktop, busy || !ready, t));
            }
        }
        Step::Shortcut => {
            let mut shortcut = col().gap(12.0).child(row().child(recorder(ui, &display_shortcut(s), t)));
            if let Some(status) = shortcut_status(ui, &state) {
                shortcut = shortcut.child(small(status, t.c.muted_foreground));
            }
            let saved = saved(s);
            if ui.candidate.is_none() && !state.shortcut_registered && !state.shortcut_pending && capture::pair_modifier(&saved).is_none() {
                let label = if linux { "Shortcut permissions" } else { "Try again" };
                shortcut = shortcut.child(row().child(kit::Button::new(label).variant("ghost").size("sm").disabled(busy).on_press(|_: &mut App| capture::retry()).build(t)));
            }
            body = body.child(shortcut);
            if !ready {
                body = body.child(small("Capture needs attention. Go back to check access.".into(), t.c.destructive));
            }
        }
    }
    if error.is_some() {
        body = body.child(small("Couldn't finish this step. Try again or check Advanced for help.".into(), t.c.destructive));
    }
    let mut details: Vec<String> = error.into_iter().collect();
    if wizard.step == Step::Access {
        if let Some(message) = state.message.clone().filter(|m| !details.contains(m)) {
            details.push(message);
        }
        if let Some((Ext::Error | Ext::Unsupported, message)) = extension(&state).filter(|_| kind == Kind::Gnome)
            && !details.iter().any(|d| d == message)
        {
            details.push(message.to_owned());
        }
    }
    let gnome_access = wizard.step == Step::Access && kind == Kind::Gnome;
    if !details.is_empty() || gnome_access {
        let open = ui.advanced;
        let mut advanced = col().gap(12.0).child(
            row()
                .gap(4.0)
                .center_y()
                .role("button")
                .label("Advanced")
                .cursor(rune::Cursor::Pointer)
                .on_press(|s: &mut App| s.ui.settings.snapshots.advanced ^= true)
                .child(icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 12.0).color(t.c.muted_foreground))
                .child(text("Advanced").font(theme::TEXT_XS).color(t.c.muted_foreground)),
        );
        if open {
            for detail in details {
                advanced = advanced.child(small(detail, t.c.muted_foreground));
            }
            if gnome_access {
                advanced = advanced.child(small("Included with T3 Code. No download needed.".into(), t.c.muted_foreground));
            }
            if gnome_access && ext == Some(Ext::Enabled) {
                let disable = kit::Button::new("Disable extension").variant("ghost").size("xs").disabled(busy).on_press(|s: &mut App| capture::linux_action(s, linux::Action::Disable));
                advanced = advanced.child(row().child(disable.build(t)));
            }
        }
        body = body.child(advanced);
    }
    let mut footer = row().w_full().gap(8.0).child(el().grow(1.0));
    if wizard.step != Step::Access {
        footer = footer.child(kit::Button::new("Back").variant("ghost").disabled(busy).on_press(|s: &mut App| leave_to(s, Step::Access)).build(t));
    }
    let later = if wizard.was_enabled { "Close" } else { "Finish later" };
    footer = footer.child(kit::Button::new(later).variant("ghost").disabled(busy).on_press(|s: &mut App| close_setup(s, false)).build(t));
    let config_shortcut = wizard.step == Step::Shortcut && managed(&state).is_some();
    footer = footer.child(match wizard.step {
        // `CaptureShortcutConfig` saves and finishes on its own.
        Step::Shortcut if config_shortcut => el(),
        Step::Access if linux && kind == Kind::Gnome && !ready && ext != Some(Ext::Enabled) => {
            let install = matches!(ext, Some(Ext::NotInstalled | Ext::UpdateRequired));
            let enable = ext == Some(Ext::Disabled);
            let label = match (state.linux_busy, install, enable) {
                (true, true, _) => "Installing…",
                (true, _, true) => "Enabling…",
                (true, ..) => "Checking…",
                (false, true, _) if ext == Some(Ext::UpdateRequired) => "Update extension",
                (false, true, _) => "Install extension",
                (false, _, true) => "Enable extension",
                _ => "Check again",
            };
            kit::Button::new(label)
                .disabled(busy)
                .on_press(move |s: &mut App| match (install, enable) {
                    (true, _) => capture::linux_action(s, linux::Action::Install),
                    (_, true) => capture::linux_action(s, linux::Action::Enable),
                    _ => capture::refresh_linux(s),
                })
                .build(t)
        }
        Step::Access if linux => kit::Button::new(if busy { "Working…" } else if ready { "Continue" } else { "Try again" })
            .disabled(busy)
            .on_press(move |s: &mut App| if ready { tested(s, true) } else { capture::refresh_linux(s) })
            .build(t),
        Step::Access => kit::Button::new(if busy { "Working…" } else { "Test capture and continue" })
            .disabled(busy || !permissions_ready(&state, text_on))
            .on_press(|s: &mut App| capture::test(s))
            .build(t),
        Step::Shortcut => {
            let changed = ui.candidate.is_some();
            let can_save = matches!(ui.candidate, Some((_, Ok(()))));
            // `captureSetupShortcutReady`: a portal prompt still open counts.
            let shortcut_ready = ready && (state.shortcut_registered || state.shortcut_pending);
            kit::Button::new(if changed { "Save and finish" } else { "Done" })
                .disabled(busy || !ready || if changed { !can_save } else { !shortcut_ready })
                .on_press(move |s: &mut App| {
                    if !changed || save_candidate(s) {
                        close_setup(s, true);
                    }
                })
                .build(t)
        }
    });
    let title = desktop_name(kind).map_or_else(|| "Set up snapshots".to_owned(), |desktop| format!("Set up snapshots for {desktop}"));
    kit::Dialog::new("snap-shot-setup", title)
        .width(512.0)
        .on_close(|s: &mut App| {
            if capture::state().test != Test::Running {
                close_setup(s, false);
            }
        })
        .child(steps(wizard.step, busy, t))
        .child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.8)))
        .child(body)
        .child(footer)
        .open(true)
        .build(t, layer);
}

fn spawn<O: Send + 'static>(s: &App, work: impl FnOnce() -> O + Send + 'static) -> Slot<O> {
    let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let (out, waker) = (slot.clone(), s.ui.panel.waker.clone());
    std::thread::spawn(move || {
        *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(work());
        if let Some(waker) = waker {
            waker();
        }
    });
    slot
}

fn take<O>(slot: &Option<Slot<O>>) -> Option<O> {
    slot.as_ref().and_then(|slot| slot.lock().unwrap_or_else(|e| e.into_inner()).take())
}

/// The recorder's keys as `shortcutToKeybindingInput`; Rune records Super as the mod key.
fn keys_input(shortcut: &Value) -> String {
    let flag = |k: &str| shortcut[k].as_bool().unwrap_or(false);
    let mut parts: Vec<String> = [(flag("ctrlKey"), "Ctrl"), (flag("altKey"), "Alt"), (flag("shiftKey"), "Shift"), (flag("metaKey") || flag("modKey"), "Super")]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, name)| name.to_owned())
        .collect();
    parts.push(shortcut["key"].as_str().unwrap_or_default().to_uppercase());
    parts.join("+")
}

/// A label like "Ctrl+Shift+2" as the recorder's shortcut.
fn keys_value(label: &str) -> Value {
    let mut shortcut = json!({"key": "", "metaKey": false, "ctrlKey": false, "altKey": false, "shiftKey": false, "modKey": false});
    for part in label.split('+').map(|p| p.trim().to_lowercase()) {
        match part.as_str() {
            "ctrl" | "control" => shortcut["ctrlKey"] = json!(true),
            "alt" => shortcut["altKey"] = json!(true),
            "shift" => shortcut["shiftKey"] = json!(true),
            "super" | "meta" | "mod" => shortcut["modKey"] = json!(true),
            _ => shortcut["key"] = json!(part),
        }
    }
    shortcut
}

fn config_keys(c: &Config) -> Value {
    c.keys.clone().unwrap_or_else(|| keys_value(c.preview.as_ref().map_or("Ctrl+Shift+2", |p| p.shortcut.as_str())))
}

const PREPARE_FAILED: &str = "Couldn't prepare the changes. Check Advanced for help.";
const SAVE_FAILED: &str = "Couldn't save your shortcut. Review the changes and try again.";

/// `CaptureShortcutConfig.read`: previews the change to `file` (or the desktop's own config).
fn read_config(s: &mut App, file: Option<std::path::PathBuf>, operation: Operation) {
    let Some(desktop) = managed(&state(s)) else { return };
    if s.ui.settings.snapshots.config.working() || s.ui.settings.snapshots.recording {
        return;
    }
    let requested = s.ui.settings.snapshots.config.keys.as_ref().map(keys_input);
    let path = file.clone().unwrap_or_else(|| config::default_path(desktop).0);
    let slot = spawn(s, move || config::preview(desktop, &path, linux::APP_ID, operation, requested.as_deref()));
    let c = &mut s.ui.settings.snapshots.config;
    c.preview = None;
    c.applied = None;
    c.error = None;
    c.file = file;
    c.reading = Some(slot);
}

fn choose_config_file(s: &mut App, tx: &mut rune::Tx) {
    let state = state(s);
    let Some(desktop) = managed(&state) else { return };
    if s.ui.settings.snapshots.config.working() {
        return;
    }
    let current = s.ui.settings.snapshots.config.file.clone().or_else(|| state.shortcut_config_path.map(Into::into));
    let mut options = rune::FileDialogOptions { title: "Choose your desktop config".into(), directory: current.as_ref().and_then(|p| p.parent()).map(Into::into), ..Default::default() };
    options = match desktop {
        Desktop::Niri => options.filter("Desktop config", &["kdl"]),
        Desktop::Hyprland => options.filter("Desktop config", &["conf", "lua"]),
    };
    tx.open_file(options, |s: &mut App, picked: rune::FileDialogResult, _| {
        if let Ok(Some(path)) = picked {
            read_config(s, Some(path), Operation::Install);
        }
    });
}

fn apply_config(s: &mut App) {
    let c = &s.ui.settings.snapshots.config;
    let Some(preview) = c.preview.clone().filter(|_| !c.working()) else { return };
    let slot = spawn(s, move || config::apply(&preview));
    s.ui.settings.snapshots.config.writing = Some(slot);
}

/// Picks up finished reviews and saves; a clean install closes the wizard as web's `onComplete`.
pub fn poll(s: &mut App) -> bool {
    let c = &mut s.ui.settings.snapshots.config;
    let mut changed = false;
    if let Some(result) = take(&c.reading) {
        c.reading = None;
        match result {
            Ok(preview) => c.preview = Some(preview),
            Err(detail) => c.error = Some((PREPARE_FAILED, Some(detail))),
        }
        changed = true;
    }
    if let Some(result) = take(&c.writing) {
        c.writing = None;
        changed = true;
        match result {
            Ok(applied) => {
                let finished = applied.warning.is_none() && c.preview.as_ref().is_some_and(|p| p.operation == Operation::Install);
                let shortcut = c.preview.as_ref().map(|p| p.shortcut.clone()).unwrap_or_default();
                c.applied = Some(applied);
                if finished && s.ui.settings.snapshots.wizard.is_some() {
                    crate::shell::palette::toast(s, "Shortcut saved", Some(&format!("Use {shortcut} from another app.")));
                    close_setup(s, true);
                }
            }
            Err(detail) => {
                c.error = Some((SAVE_FAILED, Some(detail)));
                c.preview = None;
            }
        }
    }
    changed
}

/// The unified diff's lines: `+`, `-`, ` ` with three lines of context, `@` between hunks.
fn diff_lines<'a>(before: &'a str, after: &'a str) -> Vec<(char, &'a str)> {
    let (a, b): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..].iter().rev().zip(b[prefix..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (old, new) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    let mut all: Vec<(char, &str)> = a[..prefix].iter().map(|l| (' ', *l)).collect();
    if old.len() * new.len() <= 250_000 {
        // Longest common subsequence over the changed middle only.
        let w = new.len() + 1;
        let mut lcs = vec![0u32; (old.len() + 1) * w];
        for i in (0..old.len()).rev() {
            for j in (0..new.len()).rev() {
                lcs[i * w + j] = if old[i] == new[j] { lcs[(i + 1) * w + j + 1] + 1 } else { lcs[(i + 1) * w + j].max(lcs[i * w + j + 1]) };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < old.len() || j < new.len() {
            if i < old.len() && j < new.len() && old[i] == new[j] {
                all.push((' ', old[i]));
                (i, j) = (i + 1, j + 1);
            } else if i < old.len() && (j == new.len() || lcs[(i + 1) * w + j] >= lcs[i * w + j + 1]) {
                all.push(('-', old[i]));
                i += 1;
            } else {
                all.push(('+', new[j]));
                j += 1;
            }
        }
    } else {
        all.extend(old.iter().map(|l| ('-', *l)));
        all.extend(new.iter().map(|l| ('+', *l)));
    }
    all.extend(a[a.len() - suffix..].iter().map(|l| (' ', *l)));
    let near = |i: usize| all[i.saturating_sub(3)..(i + 4).min(all.len())].iter().any(|(m, _)| *m != ' ');
    let mut out = Vec::new();
    let mut skipped = false;
    for (i, line) in all.iter().enumerate() {
        if near(i) {
            if skipped && !out.is_empty() {
                out.push(('@', ""));
            }
            skipped = false;
            out.push(*line);
        } else {
            skipped = true;
        }
    }
    out
}

fn diff_view(preview: &config::Preview, t: &T) -> El {
    let mut lines = col().w_full().py(4.0);
    for (mark, line) in diff_lines(&preview.before, &preview.after) {
        let (fill, color, sign) = match mark {
            '+' => (t.c.success.alpha_mul(0.12), t.c.foreground, "+"),
            '-' => (t.c.destructive.alpha_mul(0.12), t.c.foreground, "-"),
            '@' => (t.c.muted.alpha_mul(0.5), t.c.muted_foreground, "⋯"),
            _ => (t.c.background.alpha_mul(0.0), t.c.muted_foreground, " "),
        };
        lines = lines.child(
            row()
                .w_full()
                .px(8.0)
                .gap(8.0)
                .fill(fill)
                .child(text(sign).font(theme::MONO_XS).color(color))
                .child(text(line.to_owned()).font(theme::MONO_XS).color(color).wrap().grow(1.0).min_w(0.0)),
        );
    }
    let header = row().w_full().px(8.0).py(6.0).fill(t.c.muted.alpha_mul(0.4)).child(text(preview.path.display().to_string()).font(theme::MONO_XS).color(t.c.muted_foreground).wrap());
    kit::scroll_area(
        "snap-shot-config-diff",
        col().w_full().max_h(320.0).radius(8.0).border(1.0, t.c.border).label("Shortcut changes").scroll(rune::ScrollSpec { stick_bottom: false }).child(header).child(lines),
        t,
    )
}

/// Web `CaptureShortcutConfig`: record keys, review the exact change, then save it with a backup.
fn config_panel(s: &App, state: &State, desktop: Desktop, disabled: bool, t: &T) -> El {
    let ui = &s.ui.settings.snapshots;
    let c = &ui.config;
    let busy = disabled || c.working();
    let action_busy = busy || ui.recording;
    let niri = desktop == Desktop::Niri;
    let small = |copy: String, color| text(copy).font(theme::TEXT_XS.lh(18.0)).color(color).wrap();
    let body_text = |copy: String, color| text(copy).font(theme::TEXT_SM).color(color).wrap();
    let mut panel = col().w_full().gap(16.0);
    if c.applied.is_none() {
        panel = panel.child(row().w_full().center_y().child(body_text("Shortcut".into(), t.c.foreground)).child(el().grow(1.0)).child(recorder(ui, &config_keys(c), t)));
    }
    if ui.recording {
        panel = panel.child(small("Press your shortcut. Esc cancels.".into(), t.c.muted_foreground).role("status"));
    }
    let preview = c.preview.as_ref();
    if let Some(applied) = &c.applied {
        let copy = if applied.warning.is_some() {
            "Saved, but the shortcut needs attention. Check Advanced for help.".to_owned()
        } else if preview.is_some_and(|p| p.operation == Operation::Remove) {
            "Shortcut removed.".to_owned()
        } else {
            format!("Use {} from another app to capture a window.", preview.map_or("", |p| p.shortcut.as_str()))
        };
        panel = panel.child(body_text(copy, t.c.foreground).role("status"));
    } else if let Some(preview) = preview {
        let changed = preview.changed();
        let install = preview.operation == Operation::Install;
        let copy = match (changed, install) {
            (true, false) => "Review the change below to remove your shortcut.",
            (true, true) => "Review the change below, then save your shortcut.",
            (false, false) => "There's no capture shortcut to remove.",
            (false, true) => "This shortcut is already set up.",
        };
        panel = panel.child(body_text(copy.into(), t.c.muted_foreground));
        if changed {
            panel = panel.child(diff_view(preview, t)).child(small("Only these changes will be saved. We'll keep a backup.".into(), t.c.muted_foreground));
        }
        let mut buttons = row().gap(8.0);
        if changed || install {
            let label = match (c.writing.is_some(), changed, install) {
                (true, ..) => "Saving…",
                (_, true, true) => "Save shortcut",
                (_, true, false) => "Remove shortcut",
                _ => "Done",
            };
            let save = kit::Button::new(label).disabled(action_busy || (install && state.shortcut_action_registered == Some(false))).on_press(apply_config);
            buttons = buttons.child(save.build(t));
        }
        buttons = buttons.child(kit::Button::new("Cancel").variant("ghost").disabled(action_busy).on_press(|s: &mut App| s.ui.settings.snapshots.config.preview = None).build(t));
        panel = panel.child(buttons);
    } else {
        panel = panel.child(body_text("Allow T3 Code to read your desktop settings. You'll review any changes here before saving.".into(), t.c.muted_foreground)).child(
            row().child(
                kit::Button::new(if c.reading.is_some() { "Preparing changes…" } else { "Review changes" })
                    .disabled(action_busy)
                    .on_press(|s: &mut App| {
                        let file = s.ui.settings.snapshots.config.file.clone();
                        read_config(s, file, Operation::Install);
                    })
                    .build(t),
            ),
        );
    }
    if let Some((message, _)) = &c.error {
        panel = panel.child(body_text((*message).into(), t.c.destructive).role("alert"));
    }
    if state.shortcut_action_registered == Some(false) && state.shortcut_message.is_some() {
        let copy = if state.shortcut_pending { "Connecting to your desktop…" } else { "Restart T3 Code to finish connecting your shortcut." };
        panel = panel.child(body_text(copy.into(), t.c.muted_foreground).role("status"));
    }
    let open = c.advanced;
    let mut advanced = col().w_full().gap(12.0).child(
        row()
            .gap(4.0)
            .center_y()
            .role("button")
            .label("Advanced")
            .cursor(rune::Cursor::Pointer)
            .on_press(|s: &mut App| s.ui.settings.snapshots.config.advanced ^= true)
            .child(icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 12.0).color(t.c.muted_foreground))
            .child(text("Advanced").font(theme::TEXT_XS).color(t.c.muted_foreground)),
    );
    if open {
        let strong = |copy: &str| text(copy.to_owned()).font(theme::XS_MEDIUM).color(t.c.foreground);
        let detail = c.error.as_ref().and_then(|(_, d)| d.clone()).or_else(|| c.applied.as_ref().and_then(|a| a.warning.clone()));
        if let Some(detail) = detail {
            advanced = advanced.child(col().gap(4.0).child(strong("Troubleshooting")).child(small(detail, t.c.muted_foreground)));
        }
        let path = preview.map(|p| p.path.display().to_string()).or_else(|| state.shortcut_config_path.clone()).unwrap_or_else(|| (if niri { "~/.config/niri/config.kdl" } else { "~/.config/hypr/hyprland.conf" }).into());
        let mut file = col().gap(4.0).child(strong("Settings file")).child(text(path).font(theme::MONO_XS).color(t.c.muted_foreground).wrap());
        if niri {
            file = file.child(small("T3 Code also reads any files included by this file.".into(), t.c.muted_foreground));
        }
        if let Some(p) = preview.filter(|p| p.resolved != p.path) {
            file = file.child(small(format!("Linked to {}. The link will be kept.", p.resolved.display()), t.c.muted_foreground));
        }
        advanced = advanced.child(file);
        let mut actions = row().gap(8.0)
            .child(kit::Button::new("Choose a different file…").variant("outline").size("sm").disabled(action_busy).build(t).on_press_tx(choose_config_file))
            .child(kit::Button::new("Remove shortcut…").variant("ghost").size("sm").disabled(action_busy).on_press(|s: &mut App| {
                let file = s.ui.settings.snapshots.config.file.clone();
                read_config(s, file, Operation::Remove);
            }).build(t));
        if c.applied.is_some() {
            actions = actions.child(kit::Button::new("Review changes").variant("outline").size("sm").disabled(action_busy).on_press(|s: &mut App| {
                let file = s.ui.settings.snapshots.config.file.clone();
                read_config(s, file, Operation::Install);
            }).build(t));
        }
        advanced = advanced.child(actions);
        let location = if niri { "A custom --config or NIRI_CONFIG can change its location." } else { "On Omarchy, use your own bindings file, not its defaults." };
        advanced = advanced.child(small(format!("Use your desktop's shortcut settings file. {location}"), t.c.muted_foreground));
        if let Some(backup) = c.applied.as_ref().and_then(|a| a.backup.as_ref()) {
            advanced = advanced.child(small(format!("Backup: {}", backup.display()), t.c.muted_foreground));
        }
        let manual = if niri { "Paste this inside binds { … } in your Niri config, then save." } else { "Add this binding to your Hyprland config, then save." };
        let binding = state.shortcut_binding.clone().unwrap_or_default();
        let copied = crate::data::now_ms() < c.copied_until;
        let copy_binding = binding.clone();
        advanced = advanced
            .child(strong("Manual setup"))
            .child(small(format!("{manual} Change the keys if needed."), t.c.muted_foreground))
            .child(col().w_full().max_h(160.0).radius(12.0).padding(12.0).fill(t.c.muted.alpha_mul(0.5)).child(text(binding.clone()).font(theme::MONO_XS).color(t.c.foreground).wrap()))
            .child(row().child(kit::Button::new(if copied { "Copied" } else { "Copy shortcut" }).variant("outline").size("sm").disabled(action_busy || binding.is_empty()).build(t).on_press_tx(move |s: &mut App, tx: &mut rune::Tx| {
                if copy_binding.is_empty() {
                    return;
                }
                tx.copy(copy_binding.clone());
                s.ui.settings.snapshots.config.copied_until = crate::data::now_ms() + 2000;
                tx.wake_after(std::time::Duration::from_millis(2000));
            })))
            .child(small(format!("Turn capture off in T3 Code to stop it. Remove the shortcut from {} to free up the keys.", desktop.name()), t.c.muted_foreground));
        if state.shortcut_action_registered == Some(false)
            && let Some(message) = &state.shortcut_message
        {
            advanced = advanced.child(small(message.clone(), t.c.muted_foreground).role("status"));
        }
        let done = kit::Button::new("I've added the shortcut").variant("outline").size("sm").disabled(action_busy || state.shortcut_action_registered == Some(false)).on_press(|s: &mut App| close_setup(s, true));
        advanced = advanced.child(row().child(done.build(t)));
    }
    panel.child(advanced)
}

fn scene_state(screen_recording: bool) -> State {
    State {
        supported: true,
        permissions: Some(capture::Permissions { screen_recording, accessibility: screen_recording }),
        shortcut_registered: screen_recording,
        shortcut_message: screen_recording.then(|| "Shift + Shift is observed and cannot be reserved exclusively.".into()),
        message: (!screen_recording).then(|| capture::SCREEN_RECORDING_MESSAGE.into()),
        ..State::default()
    }
}

/// Portal mode with `setup`, forced on any platform for scenes.
fn linux_state(backend: Backend, ext: Option<Ext>) -> State {
    State {
        supported: true,
        permissions: Some(capture::Permissions { screen_recording: true, accessibility: true }),
        linux: Some(linux::scene_setup(backend, ext)),
        ..State::default()
    }
}

/// Hyprland serving the capture action its config binding calls.
fn hyprland_state() -> State {
    State {
        shortcut_action_registered: Some(true),
        shortcut_message: Some("Managed by Hyprland. Add the binding to your config and save it.".into()),
        shortcut_binding: Some(config::manual_binding(Desktop::Hyprland, linux::APP_ID, false)),
        shortcut_config_path: Some("/home/you/.config/hypr/hyprland.conf".into()),
        ..linux_state(Backend::Hyprland, None)
    }
}

/// The page on macOS, forced on any platform for scenes.
fn mac_scene(s: &mut App, enabled: bool, state: State) {
    super::scene(s, "snap-shot");
    s.client_settings.values.insert("snapShotEnabled".into(), json!(enabled));
    s.ui.settings.snapshots.forced = Some(state);
}

pub(crate) const SCENES: &[(&str, fn(&mut App))] = &[
    ("settings-snap-shot-linux", |s| {
        let state = State { shortcut_registered: true, ..linux_state(Backend::GnomeExtension, Some(Ext::Enabled)) };
        mac_scene(s, true, state);
        let shortcut = json!({"key": "2", "metaKey": false, "ctrlKey": true, "altKey": false, "shiftKey": true, "modKey": false});
        s.client_settings.values.insert("snapShotShortcut".into(), shortcut);
    }),
    ("settings-snap-shot-hyprland", |s| mac_scene(s, true, hyprland_state())),
    ("settings-snap-shot-wizard-hyprland-config", |s| {
        mac_scene(s, true, hyprland_state());
        let ui = &mut s.ui.settings.snapshots;
        ui.wizard = Some(Wizard { step: Step::Shortcut, was_enabled: true });
        ui.config.advanced = true;
    }),
    ("settings-snap-shot-wizard-niri-config", |s| {
        let binding = config::niri_binding(linux::APP_ID);
        let state = State { shortcut_action_registered: Some(true), shortcut_message: Some("Set up the shortcut to add it to your Niri config.".into()), shortcut_config_path: Some("/home/you/.config/niri/config.kdl".into()), shortcut_binding: Some(binding.clone()), ..linux_state(Backend::Niri, None) };
        mac_scene(s, true, state);
        let before = "input {\n    keyboard {\n        xkb {\n            layout \"us\"\n        }\n    }\n}\n\nbinds {\n    Mod+T { spawn \"alacritty\"; }\n    Mod+Q { close-window; }\n}\n";
        let after = before.replace("    Mod+Q { close-window; }\n", &format!("    Mod+Q {{ close-window; }}\n    {}\n", binding.replace("Ctrl+Shift+2", "Ctrl+Alt+S")));
        let ui = &mut s.ui.settings.snapshots;
        ui.wizard = Some(Wizard { step: Step::Shortcut, was_enabled: true });
        ui.config.keys = Some(json!({"key": "s", "metaKey": false, "ctrlKey": true, "altKey": true, "shiftKey": false, "modKey": false}));
        ui.config.preview = Some(config::scene_preview(Desktop::Niri, "/home/you/.config/niri/config.kdl", before, &after, "Ctrl+Alt+S"));
    }),
    ("settings-snap-shot-wizard-niri", |s| {
        mac_scene(s, false, linux_state(Backend::Niri, None));
        s.ui.settings.snapshots.wizard = Some(Wizard { step: Step::Access, was_enabled: false });
    }),
    ("settings-snap-shot-wizard-kde", |s| {
        mac_scene(s, false, linux_state(Backend::Kde, None));
        s.ui.settings.snapshots.wizard = Some(Wizard { step: Step::Access, was_enabled: false });
    }),
    ("settings-snap-shot-linux-picker", |s| mac_scene(s, true, State { shortcut_message: Some(linux::MODIFIER_PAIR_UNAVAILABLE.into()), ..linux_state(Backend::Picker, None) })),
    ("settings-snap-shot-wizard-gnome", |s| {
        mac_scene(s, false, linux_state(Backend::Picker, Some(Ext::NotInstalled)));
        s.ui.settings.snapshots.wizard = Some(Wizard { step: Step::Access, was_enabled: false });
    }),
    ("settings-snap-shot-wizard-gnome-restart", |s| {
        mac_scene(s, false, linux_state(Backend::Picker, Some(Ext::RestartRequired)));
        let ui = &mut s.ui.settings.snapshots;
        ui.wizard = Some(Wizard { step: Step::Access, was_enabled: false });
        ui.advanced = true;
    }),
    ("settings-snap-shot-wizard-linux-shortcut", |s| {
        mac_scene(s, true, State { shortcut_pending: true, ..linux_state(Backend::GnomeExtension, Some(Ext::Enabled)) });
        let shortcut = json!({"key": "2", "metaKey": false, "ctrlKey": true, "altKey": false, "shiftKey": true, "modKey": false});
        s.client_settings.values.insert("snapShotShortcut".into(), shortcut);
        s.ui.settings.snapshots.wizard = Some(Wizard { step: Step::Shortcut, was_enabled: false });
    }),
    ("settings-snap-shot-mac", |s| mac_scene(s, true, State { shortcut_verified: true, ..scene_state(true) })),
    ("settings-snap-shot-mac-recording", |s| {
        mac_scene(s, true, scene_state(true));
        let shortcut = json!({"key": "2", "metaKey": false, "ctrlKey": false, "altKey": false, "shiftKey": true, "modKey": true});
        s.ui.settings.snapshots.candidate = Some((shortcut, Ok(())));
    }),
    ("settings-snap-shot-wizard-access", |s| {
        mac_scene(s, false, scene_state(false));
        s.ui.settings.snapshots.wizard = Some(Wizard { step: Step::Access, was_enabled: false });
    }),
    ("settings-snap-shot-wizard-accessibility", |s| {
        let permissions = Some(capture::Permissions { screen_recording: true, accessibility: false });
        mac_scene(s, false, State { permissions, ..scene_state(true) });
        s.ui.settings.snapshots.wizard = Some(Wizard { step: Step::Access, was_enabled: false });
    }),
    ("settings-snap-shot-wizard-test-failed", |s| {
        let failed = Test::Failed(capture::test_message(capture::Step::CaptureWindow, capture::Status::Exited(1)));
        mac_scene(s, false, State { test: failed, ..scene_state(true) });
        let ui = &mut s.ui.settings.snapshots;
        ui.wizard = Some(Wizard { step: Step::Access, was_enabled: false });
        ui.advanced = true;
    }),
    ("settings-snap-shot-wizard-shortcut", |s| {
        mac_scene(s, true, scene_state(true));
        s.ui.settings.snapshots.wizard = Some(Wizard { step: Step::Shortcut, was_enabled: false });
    }),
];

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Key, KeyEvent, Mods};

    #[test]
    fn config_shortcuts_review_and_read_like_web() {
        let ctrl_shift_2 = json!({"key": "2", "metaKey": false, "ctrlKey": true, "altKey": false, "shiftKey": true, "modKey": false});
        assert_eq!(keys_input(&ctrl_shift_2), "Ctrl+Shift+2");
        assert_eq!(keys_input(&json!({"key": "f5", "altKey": true, "modKey": true})), "Alt+Super+F5", "Rune records Super as the mod key");
        assert_eq!(keys_input(&keys_value("Ctrl+Shift+2")), "Ctrl+Shift+2");
        assert_eq!(keys_input(&keys_value("Super+A")), "Super+A");

        let before = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";
        let after = "a\nb\nc\nd\ne\nf\ng\nh\nbind\ni\nj\n";
        assert_eq!(diff_lines(before, after), vec![(' ', "f"), (' ', "g"), (' ', "h"), ('+', "bind"), (' ', "i"), (' ', "j")], "three lines of context");
        let two = diff_lines("x\n1\n2\n3\n4\n5\n6\n7\n8\ny\n", "1\n2\n3\n4\n5\n6\n7\n8\n");
        assert_eq!(two.iter().filter(|(m, _)| *m == '@').count(), 1, "far-apart changes are separate hunks");
        assert_eq!((two[0], *two.last().unwrap()), (('-', "x"), ('-', "y")));
        assert_eq!(diff_lines("a\nold\nb\n", "a\nnew\nb\n"), vec![(' ', "a"), ('-', "old"), ('+', "new"), (' ', "b")], "removals before additions");

        let niri = State { shortcut_binding: Some("Ctrl+Shift+2 { … }".into()), ..linux_state(Backend::Niri, None) };
        assert_eq!(status(&niri, true), "Use your shortcut from another app");
        let connecting = State { shortcut_pending: true, ..linux_state(Backend::Hyprland, None) };
        assert_eq!(status(&connecting, true), "Connecting your shortcut…");
        let refused = State { shortcut_action_registered: Some(false), ..linux_state(Backend::Hyprland, None) };
        assert_eq!(status(&refused, true), "Finish shortcut setup");
        assert_eq!(status(&State { shortcut_action_registered: Some(true), ..refused }, true), "Use your shortcut from another app");
        assert_eq!(managed(&linux_state(Backend::Kde, None)), None);
    }

    #[test]
    fn statuses_read_like_web() {
        let ready = scene_state(true);
        assert_eq!(status(&ready, false), "Turn this on to set up snapshots.");
        assert_eq!(status(&State::default(), true), "Checking snapshots…");
        assert_eq!(status(&scene_state(false), true), "Capture needs attention");
        assert_eq!(status(&ready, true), "Shortcut saved");
        assert_eq!(status(&State { shortcut_verified: true, ..ready.clone() }, true), "Ready to capture");
        assert!(setup_complete(&ready, true));
        assert!(!setup_complete(&scene_state(false), false));
        let no_text = State { permissions: Some(capture::Permissions { screen_recording: true, accessibility: false }), ..ready.clone() };
        assert!(setup_complete(&no_text, false), "Accessibility is optional without app text");
        assert!(!setup_complete(&no_text, true), "app text needs Accessibility");
        assert_eq!(shortcut_status(&Ui::default(), &ready).as_deref(), Some("Shortcut saved."));
        let taken = State { shortcut_registered: false, shortcut_message: Some(capture::SHORTCUT_TAKEN.into()), ..ready };
        assert_eq!(shortcut_status(&Ui::default(), &taken).as_deref(), Some(capture::SHORTCUT_TAKEN));
    }

    #[test]
    fn portal_mode_reads_like_web() {
        let enabled = linux_state(Backend::GnomeExtension, Some(Ext::Enabled));
        assert_eq!(kind(&enabled), Kind::Gnome);
        assert!(access_ready(&enabled));
        assert_eq!(linux_status(&enabled, true), "Finish shortcut setup");
        assert_eq!(linux_status(&State { shortcut_pending: true, ..enabled.clone() }, true), "Waiting for shortcut permission");
        assert!(!setup_complete(&State { shortcut_registered: true, ..enabled.clone() }, false), "web never calls portal setup complete");
        let installing = linux_state(Backend::Picker, Some(Ext::NotInstalled));
        assert_eq!(kind(&installing), Kind::Gnome, "GNOME sets up the extension before falling back");
        assert!(!access_ready(&installing));
        assert_eq!(setup_label(&installing), "Set up GNOME capture");
        assert_eq!(linux_status(&installing, true), "Set up active-window snapshots");
        assert_eq!(linux_access_copy(&installing, false).0, "Install the extension");
        let unsupported = linux_state(Backend::Picker, Some(Ext::Unsupported));
        assert_eq!(kind(&unsupported), Kind::Picker);
        assert_eq!(linux_status(&unsupported, true), "Manual capture only. You'll choose a window each time");
        let hyprland = linux_state(Backend::Hyprland, None);
        assert_eq!((kind(&hyprland), access_ready(&hyprland), linux_text_status(&hyprland), effects_status(&hyprland)), (Kind::Hyprland, true, None, None));
        assert_eq!(linux_access_copy(&hyprland, true).0, "Capture is ready");
        let niri = linux_state(Backend::Niri, None);
        assert_eq!(effects_status(&niri), Some("Capture effects aren't available on Niri."));
        assert_eq!(setup_label(&State { message: Some("x".into()), ..niri }), "Set up Niri capture");
        assert_eq!(effects_status(&installing), Some("Finish extension setup to enable effects."));
        assert_eq!(effects_status(&enabled), None, "the extension draws them");
        let kde = linux_state(Backend::Kde, None);
        assert_eq!((kind(&kde), access_ready(&kde), desktop_name(Kind::Kde)), (Kind::Kde, true, Some("KDE Plasma")), "registration waits for the first capture");
        assert_eq!((effects_status(&kde), linux_text_status(&kde)), (None, None), "the KWin script names the window and its frame");
        assert_eq!(setup_label(&State { message: Some("x".into()), ..kde.clone() }), "Set up KDE Plasma capture");
        assert_eq!(linux_access_copy(&kde, true).0, "Capture is ready");
        let portal = linux_state(Backend::ScreenshotPortal, None);
        assert_eq!(effects_status(&portal), Some(NO_EFFECTS));
        assert_eq!((kind(&portal), linux_text_status(&portal)), (Kind::Portal, Some("This desktop only provides a screenshot.")));
        assert_eq!(shortcut_status(&Ui::default(), &State { shortcut_registered: true, ..portal }), None, "a portal shortcut needs no status");
    }

    #[test]
    fn records_and_labels_shortcuts_like_web() {
        let event = |key: Key, mods: Mods| KeyEvent { key, mods };
        let cmd_shift = Mods { cmd: true, shift: true, ..Mods::default() };
        let recorded = chord(&event(Key::Text("@".into()), cmd_shift), None).unwrap();
        assert_eq!(recorded, json!({"key": "2", "metaKey": false, "ctrlKey": false, "altKey": false, "shiftKey": true, "modKey": true}));
        assert_eq!(shortcut_label_for(&recorded, true), "⇧⌘2");
        assert_eq!(shortcut_label_for(&recorded, false), "Ctrl+Shift+2", "Linux and Windows spell modifiers out");
        assert_eq!(key_labels_for(&json!({"key": " ", "ctrlKey": true, "altKey": true}), true), ["⌃", "⌥", "Space"]);
        assert_eq!(key_labels_for(&json!({"key": " ", "ctrlKey": true, "altKey": true}), false), ["Ctrl", "Alt", "Space"]);
        assert_eq!(shortcut_label_for(&json!({"kind": "both-shift-keys"}), false), "Shift + Shift");
        assert_eq!(key_labels_for(&json!({"kind": "modifier-pair", "modifier": "meta"}), true), ["⌘", "⌘"]);
        assert_eq!(key_labels_for(&json!({"kind": "modifier-pair", "modifier": "control"}), false), ["Ctrl", "Ctrl"]);
        let meta_shift_2 = json!({"key": "2", "metaKey": true, "shiftKey": true});
        assert_eq!(key_labels_for(&recorded, true), key_labels_for(&meta_shift_2, true), "mod and meta are one key on macOS");
        assert_ne!(key_labels_for(&recorded, false), key_labels_for(&meta_shift_2, false), "mod is Ctrl elsewhere");
        assert!(!same(&recorded, &json!({"kind": "both-shift-keys"})));
        assert_eq!(chord(&event(Key::Function(5), Mods { alt: true, ..Mods::default() }), None).unwrap()["key"], "f5");
    }
}
