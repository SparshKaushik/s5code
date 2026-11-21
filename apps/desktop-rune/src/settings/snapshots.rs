//! Web `SnapShotSettings` and its `SnapShotSetupDialog`, through `desktop::capture` on macOS and
//! `desktop::capture_linux` on Wayland. Elsewhere this client has no capture backend, so web's
//! `SettingsUnavailableGroup` shows instead, as web shows it without a bridge.

use super::controls as c;
use crate::desktop::capture::{self, State, Test};
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
        Backend::ScreenshotPortal => Kind::Portal,
        Backend::Picker if ext == Some(Ext::Unsupported) => Kind::Picker,
        Backend::GnomeExtension => Kind::Gnome,
        _ if ext.is_some() => Kind::Gnome,
        _ => Kind::Picker,
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
        "Waiting for shortcut permission"
    } else if state.shortcut_verified {
        "Ready to capture"
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
    if access_ready(state) {
        "Manage capture"
    } else if kind(state) == Kind::Gnome {
        "Set up GNOME capture"
    } else {
        "Continue setup"
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

fn modifier_symbol(modifier: &str) -> &'static str {
    match modifier {
        "meta" => "⌘",
        "control" => "⌃",
        "alt" => "⌥",
        _ => "⇧",
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

/// `snapShotShortcutKeyLabels` on macOS: one keycap per key.
fn key_labels(shortcut: &Value) -> Vec<String> {
    if let Some(modifier) = capture::pair_modifier(shortcut) {
        return vec![modifier_symbol(modifier).into(); 2];
    }
    let flag = |k: &str| shortcut[k].as_bool().unwrap_or(false);
    let mut labels: Vec<String> = [(flag("ctrlKey"), "⌃"), (flag("altKey"), "⌥"), (flag("shiftKey"), "⇧"), (flag("metaKey") || flag("modKey"), "⌘")]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, l)| l.to_owned())
        .collect();
    labels.push(key_label(shortcut["key"].as_str().unwrap_or_default()));
    labels
}

/// `formatSnapShotShortcutLabel` on macOS.
fn shortcut_label(shortcut: &Value) -> String {
    capture::pair_modifier(shortcut).map_or_else(|| key_labels(shortcut).concat(), capture::pair_label)
}

/// `sameSnapShotShortcut`.
fn same(a: &Value, b: &Value) -> bool {
    match (capture::pair_modifier(a), capture::pair_modifier(b)) {
        (Some(x), Some(y)) => x == y,
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
    ui.recording = true;
    ui.held.clear();
    ui.candidate = None;
}

fn stop_recording(s: &mut App) {
    let ui = &mut s.ui.settings.snapshots;
    ui.recording = false;
    ui.held.clear();
}

fn record(s: &mut App, shortcut: Value) {
    stop_recording(s);
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
    if !recording(s) {
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
    cancel_candidate(s);
    capture::reset_test();
    capture::check_permissions(s, false);
    capture::refresh_linux(s);
    let was_enabled = enabled(s);
    let ui = &mut s.ui.settings.snapshots;
    ui.advanced = false;
    ui.wizard = Some(Wizard { step: Step::Access, was_enabled });
}

/// `closeSetup`: leaving before finishing turns off what the wizard turned on.
fn close_setup(s: &mut App, completed: bool) {
    let Some(wizard) = s.ui.settings.snapshots.wizard.take() else { return };
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

/// `snapShotAccessibilityUnavailableMessage`; the extension's app text (AT-SPI) isn't read here.
fn linux_text_status(state: &State) -> Option<&'static str> {
    match kind(state) {
        Kind::Direct => None,
        Kind::Portal | Kind::Picker => Some("This desktop only provides a screenshot."),
        Kind::Gnome => Some("App text isn't available on Linux in this app yet."),
    }
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
        let text_control = if linux {
            locked_switch(t, "Include app text in snapshots")
        } else {
            switch(t, text_on, "Include app text in snapshots", move |s: &mut App| {
                if !text_on {
                    capture::request_accessibility(s);
                }
                super::update(s, "snapShotIncludeAccessibility", json!(!text_on));
            })
        };
        rows.push(setting(t, title("Include app text", true, t), "Include text and controls when the app makes them available.", linux_text_status(&state).map(str::to_owned), text_control));
        rows.push(setting(
            t,
            title("Capture shortcut", true, t),
            if picker && linux { "Choose a window to capture from any app." } else { "Capture the window you're using without switching apps." },
            shortcut_status(ui, &state),
            shortcut_controls(ui, &display_shortcut(s), t),
        ));
        let sound = sound_select(s).label("Capture sound").on_toggle(|s: &mut App| s.ui.settings.snapshots.sound_open ^= true).trigger(&kit::theme(t));
        rows.push(setting(t, title("Capture sound", true, t), "Choose the sound played when capture starts.", None, row().child(sound)));
        let effects = linux.then(|| NO_EFFECTS.to_owned());
        let flash = super::value(s, "snapShotFlash") != false;
        let flash_control = if linux { locked_switch(t, "Flash captured window") } else { switch(t, flash, "Flash captured window", move |s: &mut App| super::update(s, "snapShotFlash", json!(!flash))) };
        rows.push(setting(t, title("Capture flash", true, t), "Show a gentle cue on the captured window.", effects.clone(), flash_control));
        let animate = super::value(s, "snapShotAnimations") != false;
        let animate_control = if linux { locked_switch(t, "Animate snapshots") } else { switch(t, animate, "Animate snapshots", move |s: &mut App| super::update(s, "snapShotAnimations", json!(!animate))) };
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
    footer = footer.child(match wizard.step {
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
    let title = if kind == Kind::Gnome { "Set up snapshots for GNOME" } else { "Set up snapshots" };
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
        let portal = linux_state(Backend::ScreenshotPortal, None);
        assert_eq!((kind(&portal), linux_text_status(&portal)), (Kind::Portal, Some("This desktop only provides a screenshot.")));
        assert_eq!(shortcut_status(&Ui::default(), &State { shortcut_registered: true, ..portal }), None, "a portal shortcut needs no status");
    }

    #[test]
    fn records_and_labels_shortcuts_like_web() {
        let event = |key: Key, mods: Mods| KeyEvent { key, mods };
        let cmd_shift = Mods { cmd: true, shift: true, ..Mods::default() };
        let recorded = chord(&event(Key::Text("@".into()), cmd_shift), None).unwrap();
        assert_eq!(recorded, json!({"key": "2", "metaKey": false, "ctrlKey": false, "altKey": false, "shiftKey": true, "modKey": true}));
        assert_eq!(shortcut_label(&recorded), "⇧⌘2");
        assert_eq!(key_labels(&json!({"key": " ", "ctrlKey": true, "altKey": true})), ["⌃", "⌥", "Space"]);
        assert_eq!(shortcut_label(&json!({"kind": "both-shift-keys"})), "Shift + Shift");
        assert_eq!(key_labels(&json!({"kind": "modifier-pair", "modifier": "meta"})), ["⌘", "⌘"]);
        assert!(same(&recorded, &json!({"key": "2", "metaKey": true, "shiftKey": true})), "mod and meta are one key on macOS");
        assert!(!same(&recorded, &json!({"kind": "both-shift-keys"})));
        assert_eq!(chord(&event(Key::Function(5), Mods { alt: true, ..Mods::default() }), None).unwrap()["key"], "f5");
    }
}
