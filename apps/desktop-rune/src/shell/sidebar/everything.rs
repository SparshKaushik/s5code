//! Web's "update everything" (`useUpdateEverything`, `UpdateEverythingDialog`,
//! `SidebarUpdateCapsule`) for this app and the providers of every connected environment: one
//! confirm, the providers update, then this app restarts into its update after a 5 s countdown
//! the user can postpone. Remote servers keep their own update flow (`chat::server_update`).
//! Also the app pill's release notes popover (`SidebarUpdateReleaseNotes`) and the launch notices
//! (`startDesktopUpdateLaunchNotice`).

use rune::{Cursor, El, ScrollSpec, Span, Tx, col, el, icon, rich, row, spacer, text};
use serde_json::{Value, json};

use crate::desktop::updates::{self, Status};
use crate::icons as i;
use crate::settings::providers::updates as providers;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, env, kit};

/// `UPDATE_RESTART_DELAY_MS`.
const RESTART_DELAY_MS: i64 = 5_000;
/// `UPDATE_SLOW_AFTER_MS`.
const SLOW_AFTER_MS: i64 = 3 * 60 * 1000;
/// `SAVED_RESULT_MAX_AGE_MS`.
const SAVED_MAX_AGE_MS: i64 = 10 * 60 * 1000;
/// Web localStorage `t3code:update-everything-result:v1`.
const SAVED_RESULT_KEY: &str = "updateEverythingResult";
/// The version an install restarted into, to say when it didn't take (Electron's
/// `notAppliedVersion`).
const INSTALLING_KEY: &str = "appUpdateInstalling";

#[derive(Default)]
pub struct Ui {
    confirming: bool,
    run: Option<Run>,
    capsule_open: bool,
    /// The release notes popover: the pill or the popover is hovered.
    notes: (bool, bool),
    launch_checked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Providers,
    /// Providers are done; this app downloads before it can restart.
    Downloading,
    Countdown(i64),
    Installing,
    /// Only failures remain to show.
    Settled,
}

struct Run {
    /// This app's version the run restarts into (`localVersion`).
    local: Option<String>,
    phase: Phase,
    machines: Vec<Machine>,
    requests: Vec<Request>,
    local_failures: Vec<String>,
}

struct Machine {
    key: String,
    label: String,
    started_at: i64,
    /// `None` while updating; then the failure lines, empty when it succeeded.
    result: Option<Vec<String>>,
}

struct Request {
    key: String,
    driver: String,
    instance: String,
    id: u64,
    reply: Option<Result<Value, Value>>,
}

// What would update.

/// This app's pending update, when it has one to download or install.
fn local_version(s: &App) -> Option<String> {
    let ui = &s.ui.desktop.updates;
    (updates::action(ui) != updates::Action::None).then(|| ui.release.as_ref().map(|r| r.version.clone())).flatten()
}

fn downloading(s: &App) -> bool {
    matches!(s.ui.desktop.updates.status, Status::Downloading(_))
}

/// `runsEverything`: providers wait somewhere and this app isn't mid-download.
pub fn available(s: &App) -> bool {
    !downloading(s) && !providers::machines(s).is_empty()
}

fn count(n: usize, noun: &str) -> Option<String> {
    (n > 0).then(|| format!("{n} {noun}{}", if n == 1 { "" } else { "s" }))
}

/// `joinList`.
fn join(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

/// `describeUpdateEverything`'s summary, the button's tooltip.
fn summary(local: bool, providers: usize) -> String {
    let parts: Vec<String> = [local.then(|| "this app".to_owned()), count(providers, "provider")].into_iter().flatten().collect();
    format!("Update {}", join(&parts))
}

struct DialogRow {
    label: String,
    this_app: bool,
    providers: Vec<String>,
    /// Threads the restart interrupts on this machine.
    running: usize,
}

fn running_threads(s: &App, key: &str) -> usize {
    env::source(s, key).map_or(0, |src| {
        src.state().threads.iter().filter(|t| matches!(t["status"].as_str(), Some("preparing" | "starting" | "running"))).count()
    })
}

/// One row per machine, this app's first (`describeUpdateEverything`'s rows).
fn rows(s: &App, local: bool) -> Vec<DialogRow> {
    let mut rows: Vec<(String, DialogRow)> = Vec::new();
    if local {
        let restarts = env::keys(s).contains(&env::LOCAL.to_owned()) && env::connected(s, env::LOCAL);
        rows.push((
            env::LOCAL.to_owned(),
            DialogRow { label: env::label(s, env::LOCAL), this_app: true, providers: vec![], running: if restarts { running_threads(s, env::LOCAL) } else { 0 } },
        ));
    }
    for m in providers::machines(s) {
        let names = m.candidates.iter().map(|(driver, _)| providers::driver_name(driver).to_owned()).collect();
        match rows.iter_mut().find(|(key, _)| *key == m.key) {
            Some((_, row)) => row.providers = names,
            None => rows.push((m.key.clone(), DialogRow { label: m.label, this_app: false, providers: names, running: 0 })),
        }
    }
    rows.into_iter().map(|(_, row)| row).collect()
}

// The run.

fn start(s: &mut App) {
    s.ui.shell.sidebar.everything.confirming = false;
    let local = local_version(s);
    if local.is_some() && updates::action(&s.ui.desktop.updates) == updates::Action::Download {
        updates::download_update(s);
    }
    let now = crate::data::now_ms();
    let mut machines = Vec::new();
    let mut requests = Vec::new();
    let mut unreachable = Vec::new();
    for m in providers::machines(s) {
        for (driver, instance) in &m.candidates {
            let input = json!({"provider": driver, "instanceId": instance});
            match env::net_of(s, &m.key) {
                Some(net) => requests.push(Request { key: m.key.clone(), driver: driver.clone(), instance: instance.clone(), id: net.request("server.updateProvider", input), reply: None }),
                None => unreachable.push(m.label.clone()),
            }
        }
        machines.push(Machine { key: m.key, label: m.label, started_at: now, result: None });
    }
    if let Some(label) = unreachable.first() {
        crate::shell::palette::toast_error(s, "Could not update everything", Some(&format!("{label} is no longer connected.")));
    }
    s.ui.shell.sidebar.everything.run = Some(Run { local, phase: Phase::Providers, machines, requests, local_failures: vec![] });
}

/// `postponeUpdateRestart`: providers stay updated; this app installs later from its button.
fn postpone(s: &mut App) {
    let Some(run) = s.ui.shell.sidebar.everything.run.as_mut() else { return };
    run.local = None;
    finish(s);
}

/// `dismissSettledUpdateRunMachines`.
fn dismiss(s: &mut App) {
    let ui = &mut s.ui.shell.sidebar.everything;
    ui.capsule_open = false;
    if let Some(run) = ui.run.as_mut() {
        run.machines.retain(|m| m.result.is_none());
        if run.machines.is_empty() && run.local.is_none() {
            ui.run = None;
        }
    }
}

/// `UpdateEverythingResult`, saved as web's `SavedResult`.
#[derive(Debug, PartialEq)]
struct Outcome {
    saved_at: i64,
    local: Option<String>,
    providers: usize,
    failures: Vec<String>,
}

impl Outcome {
    fn to_json(&self) -> Value {
        json!({"savedAt": self.saved_at, "localVersion": self.local, "updatedProviders": self.providers, "failures": self.failures})
    }
    fn from_json(v: &Value) -> Option<Outcome> {
        Some(Outcome {
            saved_at: v["savedAt"].as_i64()?,
            local: v["localVersion"].as_str().map(str::to_owned),
            providers: v["updatedProviders"].as_u64()? as usize,
            failures: v["failures"].as_array()?.iter().filter_map(|f| f.as_str().map(str::to_owned)).collect(),
        })
    }
}

fn outcome(run: &Run) -> Outcome {
    let failures: Vec<String> =
        run.local_failures.iter().map(|f| format!("This app: {f}")).chain(run.machines.iter().flat_map(|m| m.result.clone().unwrap_or_default())).collect();
    let providers = run.requests.iter().filter(|r| r.reply.as_ref().is_some_and(|reply| providers::outcome("", &r.driver, &r.instance, reply).is_none())).count();
    Outcome { saved_at: crate::data::now_ms(), local: run.local.clone(), providers, failures }
}

/// `getUpdateEverythingToastView`: success, or an error listing the failures.
fn toast_view(result: &Outcome, app_version: &str) -> Option<(bool, String, Option<String>)> {
    let parts: Vec<String> =
        [result.local.as_deref().filter(|v| *v == app_version).map(|_| "this app".to_owned()), count(result.providers, "provider")].into_iter().flatten().collect();
    if result.failures.is_empty() {
        return (!parts.is_empty()).then(|| (true, format!("Updated {}", join(&parts)), None));
    }
    let title = if parts.is_empty() { "Update failed".to_owned() } else { format!("Updated {}", join(&parts)) };
    Some((false, title, Some(result.failures.join("\n"))))
}

fn show(s: &mut App, result: &Outcome) {
    match toast_view(result, updates::version()) {
        Some((true, title, description)) => crate::shell::palette::toast(s, &title, description.as_deref()),
        Some((false, title, description)) => crate::shell::palette::toast_error(s, &title, description.as_deref()),
        None => {}
    }
}

/// Providers settled without a restart: report, and keep the run only for its failures.
fn finish(s: &mut App) {
    let ui = &mut s.ui.shell.sidebar.everything;
    let Some(run) = ui.run.as_mut() else { return };
    run.phase = Phase::Settled;
    let result = outcome(run);
    // `closeFinishedUpdateRun`.
    if run.local.is_none() && run.machines.iter().all(|m| m.result.as_ref().is_some_and(Vec::is_empty)) {
        ui.run = None;
    }
    show(s, &Outcome { local: None, ..result });
}

/// Restarts into this app's update, leaving the outcome for the relaunched app to report.
fn install(s: &mut App, result: Outcome) {
    save(SAVED_RESULT_KEY, &result.to_json());
    updates::install(s);
}

fn settle_replies(s: &mut App, run: &mut Run) -> bool {
    let mut changed = false;
    for r in run.requests.iter_mut().filter(|r| r.reply.is_none()) {
        match env::net_of(s, &r.key) {
            Some(net) => {
                if let Some(reply) = net.take_reply(r.id) {
                    r.reply = Some(reply);
                    changed = true;
                }
            }
            None => {
                r.reply = Some(Err(json!({"message": "Provider update interrupted"})));
                changed = true;
            }
        }
    }
    for m in run.machines.iter_mut().filter(|m| m.result.is_none()) {
        let mine: Vec<&Request> = run.requests.iter().filter(|r| r.key == m.key).collect();
        if mine.iter().all(|r| r.reply.is_some()) {
            m.result = Some(mine.iter().filter_map(|r| providers::outcome(&m.label, &r.driver, &r.instance, r.reply.as_ref().expect("replied"))).collect());
            changed = true;
        }
    }
    changed
}

pub fn poll(s: &mut App, tx: &mut Tx) -> bool {
    let mut changed = launch_notices(s);
    let Some(mut run) = s.ui.shell.sidebar.everything.run.take() else { return changed };
    let now = crate::data::now_ms();
    changed |= settle_replies(s, &mut run);
    let providers_done = run.machines.iter().all(|m| m.result.is_some());
    let status = s.ui.desktop.updates.status.clone();
    let staged = updates::action(&s.ui.desktop.updates) == updates::Action::Install;
    let mut next = run.phase;
    match run.phase {
        Phase::Providers | Phase::Downloading if providers_done => match (run.local.is_some(), &status) {
            (true, _) if staged => next = Phase::Countdown(now + RESTART_DELAY_MS),
            (true, Status::Error(_, message)) => {
                run.local_failures.push(format!("Could not download update: {message}"));
                run.local = None;
                next = Phase::Settled;
            }
            (true, _) => next = Phase::Downloading,
            (false, _) => next = Phase::Settled,
        },
        Phase::Countdown(at) if now >= at => {
            next = Phase::Installing;
            run.phase = next;
            let result = outcome(&run);
            s.ui.shell.sidebar.everything.run = Some(run);
            install(s, result);
            return true;
        }
        // The label counts the seconds down: one repaint per second for five seconds.
        Phase::Countdown(at) => {
            tx.wake_after(std::time::Duration::from_millis((at - now).clamp(1, 1000) as u64));
            changed = true;
        }
        Phase::Installing => {
            if let Status::Error(_, message) = &status {
                run.local_failures.push(format!("Could not install update: {message}"));
                run.local = None;
                save(SAVED_RESULT_KEY, &Value::Null);
                next = Phase::Settled;
            }
        }
        _ => {}
    }
    let settled = next == Phase::Settled && run.phase != Phase::Settled;
    changed |= next != run.phase;
    run.phase = next;
    s.ui.shell.sidebar.everything.run = Some(run);
    if settled {
        finish(s);
    }
    changed
}

// Launch notices.

fn prefs() -> rune::prefs::Prefs {
    if rune::is_headless() || cfg!(test) {
        return rune::prefs::Prefs::memory();
    }
    rune::prefs::Prefs::at(crate::host::t3_home().join("userdata").join("desktop-rune-ui.json"))
}

fn save(key: &str, value: &Value) {
    let _ = prefs().set(key, value);
}

/// Reads `key` once: it is cleared as it is read.
fn take(key: &str) -> Option<Value> {
    let mut prefs = prefs();
    let value: Value = prefs.get(key)?;
    if value.is_null() {
        return None;
    }
    let _ = prefs.set(key, &Value::Null);
    Some(value)
}

/// Called as this app hands over to its installer.
pub fn note_install(version: &str) {
    save(INSTALLING_KEY, &json!(version));
}

/// "Update did not apply": the relaunched app still runs the version it restarted from.
fn not_applied(installing: Option<String>, running: &str) -> Option<String> {
    installing.filter(|v| v != running).map(|v| format!("S5 Code is still on {running}. Install {v} again from the update button."))
}

/// `startDesktopUpdateLaunchNotice`, once per launch.
fn launch_notices(s: &mut App) -> bool {
    if std::mem::replace(&mut s.ui.shell.sidebar.everything.launch_checked, true) {
        return false;
    }
    let mut changed = false;
    if let Some(saved) = take(SAVED_RESULT_KEY).as_ref().and_then(Outcome::from_json).filter(|o| crate::data::now_ms() - o.saved_at < SAVED_MAX_AGE_MS) {
        show(s, &saved);
        changed = true;
    }
    let installing = take(INSTALLING_KEY).and_then(|v| v.as_str().map(str::to_owned));
    if let Some(description) = not_applied(installing, updates::version()) {
        crate::shell::palette::toast_error(s, "Update did not apply", Some(&description));
        changed = true;
    }
    changed
}

// The view.

/// The footer's update control: the capsule while a run is on, else the combined button when
/// providers wait, else `None` for the app's own pill.
pub fn control(s: &App, t: &T) -> Option<El> {
    if let Some(run) = &s.ui.shell.sidebar.everything.run {
        return Some(capsule(s, t, run));
    }
    if !available(s) {
        return None;
    }
    let local = local_version(s).is_some();
    let count: usize = providers::machines(s).iter().map(|m| m.candidates.len()).sum();
    let tip = summary(local, count);
    Some(
        el().key("sb-update-everything")
            .size(32.0, 32.0)
            .radius(radius::FULL)
            .center()
            .role("button")
            .label(tip.clone())
            .help(tip)
            .fill(t.sb.sidebar_control_surface)
            .hover(t.sb.sidebar_row_hover)
            .press(t.sb.sidebar_row_active)
            .cursor(Cursor::Pointer)
            .child(icon(&i::CIRCLE_ARROW_UP, 16.0).color(t.sb.sidebar_foreground))
            .on_press(|s: &mut App| s.ui.shell.sidebar.everything.confirming = true),
    )
}

/// `formatUpdateElapsed`.
fn elapsed(ms: i64) -> String {
    let minutes = ms / 60_000;
    if minutes < 1 {
        "<1m".to_owned()
    } else if minutes < 60 {
        format!("{minutes}m")
    } else {
        format!("{}h {}m", minutes / 60, minutes % 60)
    }
}

/// `RUN_STATUS_LABELS` for a machine.
fn machine_status(run: &Run, m: &Machine) -> &'static str {
    match &m.result {
        Some(failures) if failures.is_empty() => "done",
        Some(_) => "failed",
        None if run.phase == Phase::Providers => "updating providers",
        None => "waiting",
    }
}

#[derive(Debug, PartialEq)]
enum Tone {
    Default,
    Slow,
    Failed,
}

/// `describeUpdateCapsule`: the label, its tone, the slow machine's status, and the action.
fn describe(run: &Run, percent: Option<f32>, now: i64) -> (Tone, String, Option<String>, Option<&'static str>) {
    match run.phase {
        Phase::Countdown(at) => return (Tone::Default, format!("Restart in {}s", ((at - now).max(0) + 999) / 1000), None, Some("Later")),
        Phase::Installing => return (Tone::Default, "Restarting…".to_owned(), None, None),
        _ => {}
    }
    let active: Vec<&Machine> = run.machines.iter().filter(|m| m.result.is_none() && machine_status(run, m) != "waiting").collect();
    if let Some(slowest) = active.iter().min_by_key(|m| m.started_at)
        && now - slowest.started_at >= SLOW_AFTER_MS
    {
        return (Tone::Slow, slowest.label.clone(), Some(format!("{} {}", machine_status(run, slowest), elapsed(now - slowest.started_at))), None);
    }
    let done = run.machines.iter().filter(|m| machine_status(run, m) == "done").count();
    let of = format!("{done} of {} updated", run.machines.len());
    if run.machines.iter().any(|m| m.result.is_none()) {
        return (Tone::Default, of, None, None);
    }
    if run.phase == Phase::Downloading {
        return (Tone::Default, percent.map_or("Downloading".to_owned(), |p| format!("Downloading {}%", p.floor() as u32)), None, None);
    }
    let failed: Vec<&Machine> = run.machines.iter().filter(|m| machine_status(run, m) == "failed").collect();
    match failed.as_slice() {
        [] => (Tone::Default, of, None, None),
        [one] => (Tone::Failed, format!("{} failed", one.label), None, Some("Dismiss")),
        many => (Tone::Failed, format!("{} failed", many.len()), None, Some("Dismiss")),
    }
}

/// `localStatusLabel`.
fn local_status(run: &Run, percent: Option<f32>, now: i64) -> String {
    match run.phase {
        Phase::Countdown(at) => format!("restarts in {}s", ((at - now).max(0) + 999) / 1000),
        Phase::Installing => "restarting".to_owned(),
        _ => match percent {
            Some(p) => format!("downloading {}%", p.floor() as u32),
            None => "restarts after the servers".to_owned(),
        },
    }
}

fn percent(s: &App) -> Option<f32> {
    match s.ui.desktop.updates.status {
        Status::Downloading(p) => Some(p),
        _ => None,
    }
}

fn capsule(s: &App, t: &T, run: &Run) -> El {
    let now = crate::data::now_ms();
    let (tone, label, slow, action) = describe(run, percent(s), now);
    let aria = slow.as_ref().map_or(label.clone(), |slow| format!("{label} {slow}"));
    let mut body = row()
        .key("sb-update-capsule")
        .h(32.0)
        .grow(1.0)
        .min_w(0.0)
        .radius(radius::FULL)
        .fill(t.sb.sidebar_control_surface)
        .center_y()
        .child(
            row()
                .grow(1.0)
                .w(0.0)
                .min_w(0.0)
                .h_full()
                .pl(12.0)
                .center_y()
                .role("button")
                .label(aria)
                .cursor(Cursor::Pointer)
                .hover(t.sb.sidebar_row_hover)
                .radius(radius::FULL)
                .child(text(label).font(theme::TEXT_XS).color(if tone == Tone::Failed { t.c.destructive } else { t.sb.sidebar_foreground }).truncate())
                .children(slow.map(|slow| text(format!(" {slow}")).font(theme::TEXT_XS).color(t.sb.warning_foreground)))
                .on_press(|s: &mut App| s.ui.shell.sidebar.everything.capsule_open ^= true),
        );
    body = match tone {
        Tone::Slow => body.border(1.0, t.sb.warning_foreground.alpha_mul(0.4)),
        Tone::Failed => body.border(1.0, t.c.destructive.alpha_mul(0.4)),
        Tone::Default => body,
    };
    match action {
        Some(action) => body.child(
            el().pl(6.0)
                .pr(12.0)
                .role("button")
                .label(action)
                .cursor(Cursor::Pointer)
                .child(rich(action, vec![Span::new(0, action.len()).underline(t.c.muted_foreground.alpha_mul(0.6))]).font(theme::TEXT_XS).color(t.sb.sidebar_foreground))
                .on_press(move |s: &mut App| if action == "Later" { postpone(s) } else { dismiss(s) }),
        ),
        None => body.child(el().size(32.0, 32.0).center().child(icon(&i::CIRCLE_ARROW_UP, 16.0).color(t.sb.sidebar_foreground))),
    }
}

/// The capsule's popover, the confirm dialog and the release notes.
pub fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    let ui = &s.ui.shell.sidebar.everything;
    if ui.confirming {
        dialog(s, t, layers);
    }
    if let Some(run) = ui.run.as_ref().filter(|_| ui.capsule_open) {
        capsule_popover(s, t, run, layers);
    }
    if ui.run.is_none() && (ui.notes.0 || ui.notes.1) {
        release_notes(s, t, layers);
    }
}

fn capsule_popover(s: &App, t: &T, run: &Run, layers: &mut Vec<El>) {
    let k = kit::theme(t);
    let now = crate::data::now_ms();
    let muted = t.c.muted_foreground;
    let line = |left: El, right: El| row().w_full().gap(12.0).center_y().child(left).child(spacer()).child(right);
    let mut body = col().w(256.0).gap(4.0);
    if run.local.is_some() {
        body = body.child(line(text("This app").font(theme::TEXT_XS), text(local_status(run, percent(s), now)).font(theme::TEXT_XS).color(muted)));
    } else if updates::action(&s.ui.desktop.updates) == updates::Action::Install {
        body = body.child(line(
            text("This app").font(theme::TEXT_XS),
            kit::Button::new("Restart to update")
                .size("xs")
                .variant("outline")
                .on_press(|s: &mut App| {
                    let version = local_version(s);
                    install(s, Outcome { saved_at: crate::data::now_ms(), local: version, providers: 0, failures: vec![] });
                })
                .build(t),
        ));
    }
    for m in &run.machines {
        let status = machine_status(run, m);
        let shown = if matches!(status, "done" | "failed" | "waiting") { status.to_owned() } else { format!("{status} {}", elapsed(now - m.started_at)) };
        let mut entry = col().w_full().min_w(0.0).child(line(
            text(m.label.clone()).font(theme::TEXT_XS).truncate().shrink(),
            text(shown).font(theme::TEXT_XS).color(if status == "failed" { t.c.destructive } else { muted }),
        ));
        for failure in m.result.iter().flatten() {
            entry = entry.child(text(failure.clone()).font(theme::TEXT_XS).color(muted).wrap());
        }
        body = body.child(entry);
    }
    let settled = run.machines.iter().all(|m| m.result.is_some()) && run.local.is_none();
    if settled && run.machines.iter().any(|m| machine_status(run, m) == "failed") {
        body = body.child(row().w_full().mt(4.0).gap(4.0).justify_end().child(kit::Button::new("Dismiss").size("xs").variant("ghost").on_press(dismiss).build(t)));
    }
    let surface = kit::menu::glass(body.padding(8.0), &k, false).modal(true, |s: &mut App| s.ui.shell.sidebar.everything.capsule_open = false);
    layers.push(kit::motion::popup(rune_kit::popover::anchored(&k, surface, "sb-update-capsule", rune_kit::Placement::TopEnd, 4.0)));
}

/// `UpdateEverythingDialog`.
fn dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let local = local_version(s);
    let rows = rows(s, local.is_some());
    let providers: usize = rows.iter().map(|r| r.providers.len()).sum();
    let description = [local.as_ref().map(|v| format!("S5 Code {v}")), (providers > 0).then(|| format!("{providers} provider {}", if providers == 1 { "update" } else { "updates" }))]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let muted = t.c.muted_foreground;
    let cell = |w: f32, e: El| el().w(w).min_w(0.0).child(e);
    let mut table = col().w_full().child(
        row().w_full().gap(16.0).pb(6.0).child(cell(128.0, text("Machine").font(theme::TEXT_XS).color(muted))).child(cell(96.0, text("S5 Code").font(theme::TEXT_XS).color(muted))).child(text("Providers").font(theme::TEXT_XS).color(muted)),
    ).child(el().w_full().h(1.0).fill(t.c.border));
    for row_ in &rows {
        let app = if row_.this_app {
            col().child(text("This app").font(theme::TEXT_SM)).child(text("restarts last").font(theme::TEXT_XS).color(muted))
        } else {
            col()
        };
        let mut line = row()
            .w_full()
            .gap(16.0)
            .py(6.0)
            .child(cell(128.0, text(row_.label.clone()).font(theme::TEXT_SM).truncate()))
            .child(cell(96.0, app))
            .child(text(row_.providers.join(", ")).font(theme::TEXT_SM).wrap().grow(1.0).shrink());
        if row_.running > 0 {
            line = line.child(text(format!("{} running", row_.running)).font(theme::TEXT_SM).color(t.sb.warning_foreground));
        }
        table = table.child(line);
    }
    let running: usize = rows.iter().map(|r| r.running).sum();
    let continues = crate::env::source(s, env::LOCAL).and_then(|src| src.state().config.as_ref()).is_some_and(|c| c["settings"]["continueThreadsAfterServerUpdate"] == true);
    let threads = format!("{running} running {}", if running == 1 { "thread" } else { "threads" });
    let note = (running > 0).then(|| if continues { format!("{threads} will be interrupted and can resume after the restart.") } else { format!("{threads} will be interrupted.") });
    let close = |s: &mut App| s.ui.shell.sidebar.everything.confirming = false;
    let n = rows.len();
    kit::Dialog::new("update-everything", format!("Update {n} {}?", if n == 1 { "machine" } else { "machines" }))
        .open(true)
        .alert()
        .width(576.0)
        .description(description)
        .on_close(close)
        .child(table)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .children(note.map(|note| text(note).font(theme::TEXT_SM).color(muted).wrap().grow(1.0).shrink()))
                .child(spacer())
                .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                .child(kit::Button::new("Update").on_press(start).build(t)),
        )
        .build(t, layers);
}

/// Hovering the app's pill shows its release notes, as web's hover popover.
pub fn notes_trigger(pill: El) -> El {
    pill.hover_delays(std::time::Duration::ZERO, std::time::Duration::from_millis(150))
        .on_hover_change(|s: &mut App, on, _: &mut Tx| s.ui.shell.sidebar.everything.notes.0 = on)
}

/// `shouldUseSidebarUpdateReleaseNotesPopover`: an update to show with notes to read.
fn notes_shown(s: &App) -> bool {
    let ui = &s.ui.desktop.updates;
    updates::action(ui) != updates::Action::None || matches!(ui.status, Status::Downloading(_))
}

/// `SidebarUpdateReleaseNotes`.
fn release_notes(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(release) = s.ui.desktop.updates.release.as_ref().filter(|r| !r.notes.trim().is_empty() && notes_shown(s)) else { return };
    let k = kit::theme(t);
    let head = if s.ui.desktop.updates.status == Status::Available {
        col().child(text("Update ready to download").font(theme::SM_MEDIUM)).child(text(release.version.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground).mt(2.0))
    } else {
        col().child(text(updates::summary(s)).font(theme::SM_MEDIUM).wrap())
    };
    let mut notes = col().gap(6.0).pt(12.0);
    for line in release.notes.lines().map(str::trim).filter(|l| !l.is_empty()) {
        notes = notes.child(match line.strip_prefix("- ").or(line.strip_prefix("* ")) {
            Some(item) => row().gap(6.0).child(text("•").font(theme::TEXT_XS)).child(text(item.to_owned()).font(theme::TEXT_XS).wrap().grow(1.0).shrink()),
            None => row().child(text(line.trim_start_matches('#').trim().to_owned()).font(theme::XS_MEDIUM).wrap()),
        });
    }
    let page = updates::release_page(&release.version);
    notes = notes.child(
        row()
            .mt(4.0)
            .gap(4.0)
            .center_y()
            .role("link")
            .cursor(Cursor::Pointer)
            .child(text("View release on GitHub").font(theme::TEXT_XS).color(t.c.muted_foreground))
            .child(icon(&i::EXTERNAL_LINK, 12.0).color(t.c.muted_foreground))
            .on_press_tx(move |_: &mut App, tx: &mut Tx| {
                let _ = tx.open_url(&page);
            }),
    );
    let body = col().max_w(384.0).padding(8.0).role("dialog").label("Update release notes").child(head).child(kit::scroll_area("sb-release-notes", notes.max_h(448.0).scroll(ScrollSpec { stick_bottom: false }), t));
    let surface = kit::menu::glass(body, &k, false)
        .hover_delays(std::time::Duration::ZERO, std::time::Duration::from_millis(150))
        .on_hover_change(|s: &mut App, on, _: &mut Tx| s.ui.shell.sidebar.everything.notes.1 = on);
    layers.push(kit::motion::popup(rune_kit::popover::anchored(&k, surface, "sb-app-update", rune_kit::Placement::Top, 4.0)));
}

/// Scenes: the confirm dialog, the capsule counting down with its popover, and the app pill's
/// release notes.
pub fn scene(s: &mut App, which: &str) {
    match which {
        "confirm" => {
            app_update(s);
            s.ui.shell.sidebar.everything.confirming = true;
        }
        "capsule" => {
            app_update(s);
            let now = crate::data::now_ms();
            let machine = |label: &str, result: Option<Vec<String>>| Machine { key: env::LOCAL.to_owned(), label: label.to_owned(), started_at: now - 90_000, result };
            s.ui.shell.sidebar.everything.run = Some(Run {
                local: local_version(s),
                phase: Phase::Countdown(now + 4_000),
                machines: vec![machine("This machine", Some(vec![])), machine("Build box", Some(vec!["Build box · Codex: npm ERR! EACCES".into()]))],
                requests: vec![],
                local_failures: vec![],
            });
            s.ui.shell.sidebar.everything.capsule_open = true;
        }
        _ => {
            app_update(s);
            // Providers up to date, so the app's own pill shows.
            if let crate::data::Source::Fixture(state) = &mut s.src
                && let Some(config) = state.config.as_mut()
            {
                for p in config["providers"].as_array_mut().into_iter().flatten() {
                    p["versionAdvisory"] = Value::Null;
                }
            }
            s.ui.shell.sidebar.everything.notes.0 = true;
        }
    }
}

/// The desktop's "update available" scene: the thread showcase and a 0.1.3 release.
fn app_update(s: &mut App) {
    if let Some((_, f)) = updates::SCENES.iter().find(|(name, _)| *name == "app-update-available") {
        f(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(label: &str, started_at: i64, result: Option<Vec<String>>) -> Machine {
        Machine { key: label.to_owned(), label: label.to_owned(), started_at, result }
    }

    fn run(phase: Phase, machines: Vec<Machine>) -> Run {
        Run { local: Some("0.1.3".into()), phase, machines, requests: vec![], local_failures: vec![] }
    }

    #[test]
    fn the_summary_and_result_read_as_web_s() {
        assert_eq!(summary(true, 2), "Update this app and 2 providers");
        assert_eq!(summary(false, 1), "Update 1 provider");
        assert_eq!(join(&["a".into(), "b".into(), "c".into()]), "a, b, and c");
        let ok = Outcome { saved_at: 0, local: Some("0.1.3".into()), providers: 2, failures: vec![] };
        assert_eq!(toast_view(&ok, "0.1.3"), Some((true, "Updated this app and 2 providers".into(), None)));
        assert_eq!(toast_view(&ok, "0.1.2").map(|v| v.1), Some("Updated 2 providers".into()), "FAILED: an app still on the old version did not update");
        let failed = Outcome { saved_at: 0, local: None, providers: 0, failures: vec!["Box · Codex: denied".into()] };
        assert_eq!(toast_view(&failed, "0.1.3"), Some((false, "Update failed".into(), Some("Box · Codex: denied".into()))));
        assert_eq!(toast_view(&Outcome { failures: vec![], ..failed }, "0.1.3"), None);
        assert_eq!(not_applied(Some("0.1.3".into()), "0.1.2").as_deref(), Some("S5 Code is still on 0.1.2. Install 0.1.3 again from the update button."));
        assert_eq!(not_applied(Some("0.1.3".into()), "0.1.3"), None);
        assert_eq!(not_applied(None, "0.1.3"), None);
    }

    #[test]
    fn the_capsule_counts_down_then_reports_progress_and_failures() {
        let now = 1_000_000;
        let countdown = run(Phase::Countdown(now + 4_200), vec![machine("Box", now, Some(vec![]))]);
        assert_eq!(describe(&countdown, None, now), (Tone::Default, "Restart in 5s".into(), None, Some("Later")));
        assert_eq!(local_status(&countdown, None, now), "restarts in 5s");

        let busy = run(Phase::Providers, vec![machine("Box", now, Some(vec![])), machine("Mini", now, None)]);
        assert_eq!(describe(&busy, None, now).1, "1 of 2 updated");
        assert_eq!(machine_status(&busy, &busy.machines[1]), "updating providers");
        let slow = describe(&busy, None, now + SLOW_AFTER_MS);
        assert_eq!((slow.0, slow.1.as_str(), slow.2.as_deref()), (Tone::Slow, "Mini", Some("updating providers 3m")));

        let downloading = run(Phase::Downloading, vec![machine("Box", now, Some(vec![]))]);
        assert_eq!(describe(&downloading, Some(42.7), now).1, "Downloading 42%");

        let failed = run(Phase::Settled, vec![machine("Box", now, Some(vec!["Box · Codex: denied".into()])), machine("Mini", now, Some(vec![]))]);
        assert_eq!(describe(&failed, None, now), (Tone::Failed, "Box failed".into(), None, Some("Dismiss")));
        assert_eq!(elapsed(30_000), "<1m");
        assert_eq!(elapsed(125 * 60_000), "2h 5m");
    }

    #[test]
    fn a_provider_only_run_closes_once_every_machine_is_done() {
        let mut s = App::new();
        let mut done = run(Phase::Providers, vec![machine("Box", 0, Some(vec![]))]);
        done.local = None;
        s.ui.shell.sidebar.everything.run = Some(done);
        finish(&mut s);
        assert!(s.ui.shell.sidebar.everything.run.is_none(), "FAILED: closeFinishedUpdateRun");
        let mut failed = run(Phase::Providers, vec![machine("Box", 0, Some(vec!["Box · Codex: denied".into()]))]);
        failed.local = None;
        s.ui.shell.sidebar.everything.run = Some(failed);
        finish(&mut s);
        assert!(s.ui.shell.sidebar.everything.run.is_some(), "FAILED: a failure stays to read and dismiss");
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Update failed"));
        dismiss(&mut s);
        assert!(s.ui.shell.sidebar.everything.run.is_none());
    }
}
