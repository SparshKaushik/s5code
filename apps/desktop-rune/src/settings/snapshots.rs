//! Web `SnapShotSettings` and its `SnapShotSetupDialog`, on macOS through `desktop::capture`.
//! Elsewhere this client has no capture backend, so web's `SettingsUnavailableGroup` shows
//! instead and the capture rows and wizard stay hidden, as web hides them without a bridge.
//! Include app text and Capture animations stay hidden on macOS too: Electron reads app text
//! through a native accessibility module and flies captures in with a continuous animation.

use super::controls as c;
use crate::desktop::capture::{self, State, Test};
use crate::theme::{self, AlphaMul, T};
use crate::{App, Route, icons, kit};
use rune::{El, col, el, icon, row, text};
use serde_json::{Value, json};

/// `snapShotUnavailableMessage` inside the desktop app.
const UNAVAILABLE: &str = "Update the desktop app to use snapshots.";
/// `snapShotDescription` for direct capture.
const DESCRIPTION: &str = "Capture a window and attach it to your current draft.";
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

/// `captureSetupAccessReady` for direct capture.
fn access_ready(state: &State) -> bool {
    state.message.is_none()
}

fn permissions_ready(state: &State) -> bool {
    state.permissions.is_some_and(|p| p.screen_recording)
}

/// `snapShotStatus` on macOS.
fn status(state: &State, enabled: bool) -> &'static str {
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

/// `snapShotSetupComplete` without app text.
fn setup_complete(state: &State) -> bool {
    state.permissions.is_some() && access_ready(state) && permissions_ready(state) && state.shortcut_registered
}

/// The shortcut status line: recording, the candidate's check, else `snapShotShortcutStatus`.
fn shortcut_status(ui: &Ui, state: &State) -> Option<String> {
    if ui.recording {
        return Some("Press your shortcut. Esc cancels.".into());
    }
    match &ui.candidate {
        Some((_, Ok(()))) => Some("Ready to save.".into()),
        Some((_, Err(message))) => Some(message.clone()),
        None if state.shortcut_registered => Some("Shortcut saved.".into()),
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
    if on && !setup_complete(&state) {
        let label = if access_ready(&state) { "Manage capture" } else { "Continue setup" };
        toggle = toggle.child(kit::Button::new(label).variant("outline").size("xs").on_press(open_setup).build(t));
    }
    toggle = toggle.child(switch(t, on || ui.wizard.is_some(), "Enable snapshots", move |s: &mut App| {
        if on {
            super::update(s, "snapShotEnabled", json!(false));
        } else {
            open_setup(s);
        }
    }));
    let mut rows = vec![setting(t, title("SnapShots", false, t), DESCRIPTION, Some(status(&state, on).into()), toggle)];
    if on {
        rows.push(setting(
            t,
            title("Capture shortcut", true, t),
            "Capture the window you're using without switching apps.",
            shortcut_status(ui, &state),
            shortcut_controls(ui, &display_shortcut(s), t),
        ));
        let sound = sound_select(s).label("Capture sound").on_toggle(|s: &mut App| s.ui.settings.snapshots.sound_open ^= true).trigger(&kit::theme(t));
        rows.push(setting(t, title("Capture sound", true, t), "Choose the sound played when capture starts.", None, row().child(sound)));
        let flash = super::value(s, "snapShotFlash") != false;
        rows.push(setting(
            t,
            title("Capture flash", true, t),
            "Show a gentle cue on the captured window.",
            None,
            switch(t, flash, "Flash captured window", move |s: &mut App| super::update(s, "snapShotFlash", json!(!flash))),
        ));
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

/// Web `PermissionChecklist` with Screen Recording, the one permission capture uses here.
fn checklist(state: &State, busy: bool, t: &T) -> El {
    let granted = permissions_ready(state);
    let action = if granted {
        row().gap(4.0).center_y().role("status").child(icon(&icons::CIRCLE_CHECK, 16.0).color(t.c.success)).child(text("Allowed").font(theme::TEXT_XS).color(t.c.success))
    } else {
        kit::Button::new("Allow").variant("outline").size("xs").disabled(busy).on_press(|s: &mut App| capture::check_permissions(s, true)).build(t)
    };
    row()
        .w_full()
        .gap(12.0)
        .px(12.0)
        .py(8.0)
        .center_y()
        .radius(8.0)
        .border(1.0, t.c.border)
        .child(icon(&icons::MONITOR, 24.0).color(t.c.muted_foreground))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .child(text("Screen Recording").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(text("Capture the window you're using.").font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()),
        )
        .child(action)
}

fn wizard(s: &App, t: &T, layer: &mut Vec<El>) {
    let ui = &s.ui.settings.snapshots;
    let Some(wizard) = ui.wizard else { return };
    let state = state(s);
    let busy = state.test == Test::Running;
    let error = match &state.test {
        Test::Failed(message) => Some(message.clone()),
        _ => None,
    };
    let ready = access_ready(&state);
    let small = |copy: String, color| text(copy).font(theme::TEXT_XS.lh(18.0)).color(color).wrap();
    let (heading, description) = match wizard.step {
        Step::Access if state.message.is_some() && state.permissions.is_none() => ("Let's try that again", "Couldn't check snapshots. Try again to continue."),
        Step::Access if permissions_ready(&state) => ("Allow snapshots", "Test a snapshot of the current window. If macOS asks to bypass its window picker, choose Allow. The test image is discarded."),
        Step::Access => ("Allow snapshots", "Allow each permission, then continue."),
        Step::Shortcut => ("Choose your shortcut", "Use both Shift keys, or record a different shortcut."),
    };
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(col().gap(8.0).child(text(heading).font(theme::SM_MEDIUM).color(t.c.foreground)).child(small(description.into(), t.c.muted_foreground)));
    match wizard.step {
        Step::Access => body = body.child(checklist(&state, busy, t)),
        Step::Shortcut => {
            let mut shortcut = col().gap(12.0).child(row().child(recorder(ui, &display_shortcut(s), t)));
            if let Some(status) = shortcut_status(ui, &state) {
                shortcut = shortcut.child(small(status, t.c.muted_foreground));
            }
            let saved = saved(s);
            if ui.candidate.is_none() && !state.shortcut_registered && capture::pair_modifier(&saved).is_none() {
                shortcut = shortcut.child(row().child(kit::Button::new("Try again").variant("ghost").size("sm").disabled(busy).on_press(|_: &mut App| capture::retry()).build(t)));
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
    }
    if !details.is_empty() {
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
        Step::Access => kit::Button::new(if busy { "Working…" } else { "Test capture and continue" })
            .disabled(busy || !permissions_ready(&state))
            .on_press(|s: &mut App| capture::test(s))
            .build(t),
        Step::Shortcut => {
            let changed = ui.candidate.is_some();
            let can_save = matches!(ui.candidate, Some((_, Ok(()))));
            let shortcut_ready = ready && state.shortcut_registered;
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
    kit::Dialog::new("snap-shot-setup", "Set up snapshots")
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
        permissions: Some(capture::Permissions { screen_recording }),
        shortcut_registered: screen_recording,
        shortcut_message: screen_recording.then(|| "Shift + Shift is observed and cannot be reserved exclusively.".into()),
        message: (!screen_recording).then(|| capture::SCREEN_RECORDING_MESSAGE.into()),
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
        assert!(setup_complete(&ready));
        assert!(!setup_complete(&scene_state(false)));
        assert_eq!(shortcut_status(&Ui::default(), &ready).as_deref(), Some("Shortcut saved."));
        let taken = State { shortcut_registered: false, shortcut_message: Some(capture::SHORTCUT_TAKEN.into()), ..ready };
        assert_eq!(shortcut_status(&Ui::default(), &taken).as_deref(), Some(capture::SHORTCUT_TAKEN));
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
