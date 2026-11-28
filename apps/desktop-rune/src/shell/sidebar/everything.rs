//! Web's "update everything" (`useUpdateEverything`, `UpdateEverythingDialog`,
//! `SidebarUpdateCapsule`) for this app, the providers of every connected environment and the
//! remote servers behind this app's bundled server: one confirm, the providers update, then the
//! servers (each through `chat::server_update`'s flow, retried from the capsule if one fails),
//! then this app restarts into its update after a 5 s countdown the user can postpone.
//! Also the app pill's release notes popover (`SidebarUpdateReleaseNotes`) and the launch notices
//! (`startDesktopUpdateLaunchNotice`).

use rune::{Cursor, El, Span, Tx, col, el, icon, rich, row, spacer, text};
use serde_json::{Value, json};

use crate::chat::server_update::{self, Stage, Status as ServerStatus};
use crate::desktop::updates::{self, Status};
use crate::icons as i;
use crate::settings::machines::{self, Target};
use crate::settings::providers::updates as providers;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, env, kit};

/// `UPDATE_RESTART_DELAY_MS`.
const RESTART_DELAY_MS: i64 = 5_000;
/// `UPDATE_SLOW_AFTER_MS`.
const SLOW_AFTER_MS: i64 = 3 * 60 * 1000;
/// `HANDOFF_TIMEOUT_MS`: covers a background service's 30-minute trial start.
const HANDOFF_TIMEOUT_MS: i64 = 32 * 60 * 1000;
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
    skipped_open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Providers,
    /// Providers are done; the servers update, until each restarted or failed.
    Servers,
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
    servers_updated: usize,
    /// A retry settles its machines without reporting the run again.
    quiet: bool,
}

struct Machine {
    key: String,
    label: String,
    started_at: i64,
    /// `None` while updating; then the failure lines, empty when it succeeded.
    result: Option<Vec<String>>,
    updates_providers: bool,
    /// The server to update and, once started, its update; cleared once it updated.
    server: Option<(Target, Option<server_update::State>)>,
    /// Provider failures on a machine whose server updates after them (`providerFailure`).
    provider_failures: Vec<String>,
    /// A server still restarting when this app restarted, followed to this version (`handedOffVersion`).
    handed_off: Option<String>,
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

/// `collectServerUpdateTargets`: remote servers behind this app's bundled server, and the ones
/// skipped with why. A server this app runs updates with the app, so it is never a target.
fn servers(s: &App) -> (Vec<Target>, Vec<(String, String, String)>) {
    let (mut targets, mut skipped) = (Vec::new(), Vec::new());
    for (key, label, verdict) in machines::assessed(s, true) {
        let busy = || machines::row_updating(s, &key) || following(s, &key) || (s.ui.env.active == key && matches!(server_update::status(s), ServerStatus::Running { .. }));
        let maintains = || env::outdated(s, &key).is_some() || crate::settings::permissions::can_on(s, &key, "environment:maintain");
        // `unavailableReason` comes before the server's own reasons.
        let reason = match verdict {
            Err(reason) if reason == "not connected" => reason,
            _ if !maintains() => "no permission".to_owned(),
            _ if busy() => "already updating".to_owned(),
            Err(reason) => reason,
            Ok(target) => {
                targets.push(target);
                continue;
            }
        };
        skipped.push((key, label, reason));
    }
    (targets, skipped)
}

/// `runsEverything`: servers or providers wait somewhere and this app isn't mid-download.
pub fn available(s: &App) -> bool {
    !downloading(s) && (!providers::machines(s).is_empty() || !servers(s).0.is_empty())
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
fn summary(local: bool, servers: usize, providers: usize) -> String {
    let parts: Vec<String> = [local.then(|| "this app".to_owned()), count(servers, "server"), count(providers, "provider")].into_iter().flatten().collect();
    format!("Update {}", join(&parts))
}

#[derive(Default)]
struct DialogRow {
    label: String,
    /// `APP_LABELS`: "This app", "Server" or "Desktop app".
    app: Option<&'static str>,
    skipped: Option<String>,
    providers: Vec<String>,
    /// Threads the restart interrupts on this machine, and whether they resume after it.
    running: usize,
    continues: bool,
}

fn running_threads(s: &App, key: &str) -> usize {
    env::source(s, key).map_or(0, |src| {
        src.state().threads.iter().filter(|t| matches!(t["status"].as_str(), Some("preparing" | "starting" | "running"))).count()
    })
}

/// `continueThreadsAfterServerUpdate` on environment `key`.
fn continues(s: &App, key: &str) -> bool {
    env::source(s, key).and_then(|src| src.state().config.as_ref()).is_some_and(|c| c["settings"]["continueThreadsAfterServerUpdate"] == true)
}

/// One row per machine: this app's, then servers, provider-only, and skipped
/// (`describeUpdateEverything`'s rows), with `collectRestartingMachines`' running threads.
fn rows(s: &App, local: bool) -> Vec<(String, DialogRow)> {
    let mut rows: Vec<(String, DialogRow)> = Vec::new();
    let restarts = |s: &App, key: &str| if env::connected(s, key) { running_threads(s, key) } else { 0 };
    if local {
        let running = if env::keys(s).contains(&env::LOCAL.to_owned()) { restarts(s, env::LOCAL) } else { 0 };
        rows.push((env::LOCAL.to_owned(), DialogRow { label: env::label(s, env::LOCAL), app: Some("This app"), running, continues: continues(s, env::LOCAL), ..DialogRow::default() }));
    }
    let (targets, skipped) = servers(s);
    for t in targets {
        let app = if t.self_update == "desktop-managed" { "Desktop app" } else { "Server" };
        rows.push((t.key.clone(), DialogRow { label: t.label, app: Some(app), running: restarts(s, &t.key), continues: continues(s, &t.key), ..DialogRow::default() }));
    }
    for m in providers::machines(s) {
        let names = m.candidates.iter().map(|(driver, _)| providers::driver_name(driver).to_owned()).collect();
        match rows.iter_mut().find(|(key, _)| *key == m.key) {
            Some((_, row)) => row.providers = names,
            None => rows.push((m.key.clone(), DialogRow { label: m.label, providers: names, ..DialogRow::default() })),
        }
    }
    for (key, label, reason) in skipped {
        match rows.iter_mut().find(|(k, _)| *k == key) {
            Some((_, row)) => row.skipped = Some(reason),
            None => rows.push((key, DialogRow { label, skipped: Some(reason), ..DialogRow::default() })),
        }
    }
    rows
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
        machines.push(Machine { key: m.key, label: m.label, started_at: now, result: None, updates_providers: true, server: None, provider_failures: vec![], handed_off: None });
    }
    for target in servers(s).0 {
        match machines.iter_mut().find(|m| m.key == target.key) {
            Some(m) => {
                m.label = target.label.clone();
                m.server = Some((target, None));
            }
            None => machines.push(Machine {
                key: target.key.clone(),
                label: target.label.clone(),
                started_at: now,
                result: None,
                updates_providers: false,
                server: Some((target, None)),
                provider_failures: vec![],
                handed_off: None,
            }),
        }
    }
    if let Some(label) = unreachable.first() {
        crate::shell::palette::toast_error(s, "Could not update everything", Some(&format!("{label} is no longer connected.")));
    }
    s.ui.shell.sidebar.everything.run = Some(Run { local, phase: Phase::Providers, machines, requests, local_failures: vec![], servers_updated: 0, quiet: false });
}

/// Starts each waiting server's update once the providers are done.
fn start_servers(s: &mut App, run: &mut Run, now: i64) {
    for index in 0..run.machines.len() {
        let failures = provider_lines(&run.requests, &run.machines[index]);
        let m = &mut run.machines[index];
        let Some((target, update)) = m.server.as_mut() else { continue };
        if update.is_some() {
            continue;
        }
        m.provider_failures = failures;
        m.started_at = now;
        let environment = match env::outdated(s, &m.key) {
            Some(descriptor) => descriptor.clone(),
            None => env::source(s, &m.key).and_then(|src| src.state().config.as_ref()).map_or(Value::Null, |c| c["environment"].clone()),
        };
        // A retry that can no longer reach or maintain the server can't start (web's "skipped").
        let maintains = env::outdated(s, &m.key).is_some() || crate::settings::permissions::can_on(s, &m.key, "environment:maintain");
        match env::net_of(s, &m.key).filter(|_| maintains || !run.quiet) {
            Some(net) => *update = Some(server_update::begin(&environment, net, &target.target_version)),
            None if run.quiet => m.result = Some(vec![format!("{}: Could not start the update", m.label)]),
            None => m.result = Some(vec![format!("{}: Not connected", m.label)]),
        }
    }
}

/// `retryFailedUpdateRunServers`: the failed servers update again; their providers stay as they ended.
fn retry(s: &mut App) {
    let Some(mut run) = s.ui.shell.sidebar.everything.run.take() else { return };
    let now = crate::data::now_ms();
    for m in run.machines.iter_mut().filter(|m| m.result.as_ref().is_some_and(|r| !r.is_empty())) {
        if let Some((_, update)) = m.server.as_mut() {
            *update = None;
            m.result = None;
        }
    }
    run.phase = Phase::Servers;
    run.quiet = true;
    start_servers(s, &mut run, now);
    s.ui.shell.sidebar.everything.run = Some(run);
}

/// Moves each started server update along: done once its server is back on the target (or, for
/// an outdated server that can't subscribe, once it accepted the restart), failed with why.
fn advance_servers(s: &mut App, run: &mut Run, now: i64) -> bool {
    let mut lifecycle = std::mem::take(&mut s.ui.shell.lifecycle);
    let mut changed = false;
    for m in run.machines.iter_mut().filter(|m| m.result.is_none()) {
        let Some((_, Some(update))) = m.server.as_mut() else { continue };
        // A fixture has no connection to update over; its scene stages the update itself.
        if matches!(env::source(s, &m.key), Some(crate::data::Source::Fixture(_))) {
            continue;
        }
        let Some(net) = env::net_of(s, &m.key) else {
            m.result = Some(vec![format!("{}: Server update interrupted", m.label)]);
            changed = true;
            continue;
        };
        changed |= server_update::advance(update, net, &mut lifecycle, &m.key, now);
        let done = match update.status() {
            ServerStatus::Idle => Some(Ok(())),
            ServerStatus::Failed { message, .. } => Some(Err(message.clone())),
            ServerStatus::Running { stage: Stage::Resuming, .. } if env::outdated(s, &m.key).is_some() => Some(Ok(())),
            ServerStatus::Running { .. } => None,
        };
        match done {
            Some(Ok(())) => {
                m.result = Some(std::mem::take(&mut m.provider_failures));
                m.server = None;
                run.servers_updated += 1;
                env::server_updated(s, &m.key);
            }
            Some(Err(message)) => m.result = Some([format!("{}: {message}", m.label)].into_iter().chain(m.provider_failures.iter().cloned()).collect()),
            None => continue,
        }
        changed = true;
    }
    s.ui.shell.lifecycle = lifecycle;
    changed
}

/// A machine's provider update failures, once its requests all replied.
fn provider_lines(requests: &[Request], m: &Machine) -> Vec<String> {
    requests.iter().filter(|r| r.key == m.key).filter_map(|r| r.reply.as_ref().and_then(|reply| providers::outcome(&m.label, &r.driver, &r.instance, reply))).collect()
}

fn replied(requests: &[Request], key: &str) -> bool {
    requests.iter().filter(|r| r.key == key).all(|r| r.reply.is_some())
}

/// The server update has handed off to the restarting server (`isServerUpdateHandedOff`).
fn handed_off(m: &Machine) -> bool {
    m.handed_off.is_some()
        || matches!(m.server.as_ref().and_then(|(_, u)| u.as_ref()).map(|u| u.status()), Some(ServerStatus::Running { stage: Stage::Resuming, .. }))
}

/// The version a handed-off server comes back on: only a desktop-managed server's update knows
/// its own feed version.
fn handed_off_to(m: &Machine) -> Option<HandedOff> {
    let target = m.handed_off.clone().or_else(|| {
        let (target, update) = m.server.as_ref()?;
        Some(match update.as_ref().map(|u| u.status()) {
            Some(ServerStatus::Running { target, .. }) => target.clone(),
            _ => target.target_version.clone(),
        })
    })?;
    Some(HandedOff { key: m.key.clone(), label: m.label.clone(), target, started_at: m.started_at })
}

/// A server the relaunched app follows back from its restart, so nothing offers its update again.
fn following(s: &App, key: &str) -> bool {
    s.ui.shell.sidebar.everything.run.as_ref().is_some_and(|run| run.machines.iter().any(|m| m.key == key && m.result.is_none() && m.handed_off.is_some()))
}

/// `followHandedOffServers`: done once the server reports the version it restarted into, failed
/// once it isn't back in time. Returns the next deadline to wake for.
fn follow_servers(s: &mut App, run: &mut Run, now: i64) -> (bool, Option<i64>) {
    let (mut changed, mut next) = (false, None::<i64>);
    for m in run.machines.iter_mut().filter(|m| m.result.is_none()) {
        let Some(target) = m.handed_off.as_deref() else { continue };
        let config = env::source(s, &m.key).and_then(|src| src.state().config.as_ref()).map(|c| &c["environment"]);
        let version = config.or_else(|| env::outdated(s, &m.key)).and_then(|e| e["serverVersion"].as_str());
        let deadline = m.started_at + HANDOFF_TIMEOUT_MS;
        if version.is_some_and(|v| !machines::behind(v, target)) {
            m.result = Some(vec![]);
            run.servers_updated += 1;
        } else if now >= deadline {
            m.result = Some(vec![format!("{}: Not back after {} minutes", m.label, HANDOFF_TIMEOUT_MS / 60_000)]);
        } else {
            next = Some(next.map_or(deadline, |n| n.min(deadline)));
            continue;
        }
        changed = true;
    }
    (changed, next)
}

/// `postponeUpdateRestart`: providers stay updated; this app installs later from its button.
fn postpone(s: &mut App) {
    let Some(run) = s.ui.shell.sidebar.everything.run.as_mut() else { return };
    run.local = None;
    // The servers may still be restarting; the run reports once they settle.
    run.phase = Phase::Servers;
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
#[derive(Debug, Default, PartialEq)]
struct Outcome {
    saved_at: i64,
    local: Option<String>,
    servers: usize,
    providers: usize,
    failures: Vec<String>,
    /// Servers still restarting as this app restarted (`handedOff`).
    restarting: Vec<HandedOff>,
}

/// Web's `HandedOffServer`: what the relaunched app follows the server to.
#[derive(Clone, Debug, Default, PartialEq)]
struct HandedOff {
    key: String,
    label: String,
    target: String,
    started_at: i64,
}

impl Outcome {
    fn to_json(&self) -> Value {
        let handed_off: Vec<Value> =
            self.restarting.iter().map(|h| json!({"environmentId": h.key, "label": h.label, "targetVersion": h.target, "startedAt": h.started_at})).collect();
        json!({"savedAt": self.saved_at, "localVersion": self.local, "updatedServers": self.servers, "updatedProviders": self.providers, "failures": self.failures, "handedOff": handed_off})
    }
    fn from_json(v: &Value) -> Option<Outcome> {
        Some(Outcome {
            saved_at: v["savedAt"].as_i64()?,
            local: v["localVersion"].as_str().map(str::to_owned),
            servers: v["updatedServers"].as_u64().unwrap_or(0) as usize,
            providers: v["updatedProviders"].as_u64()? as usize,
            failures: v["failures"].as_array()?.iter().filter_map(|f| f.as_str().map(str::to_owned)).collect(),
            restarting: v["handedOff"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|h| {
                    let text = |k: &str| h[k].as_str().map(str::to_owned);
                    Some(HandedOff { key: text("environmentId")?, label: text("label")?, target: text("targetVersion")?, started_at: h["startedAt"].as_i64()? })
                })
                .collect(),
        })
    }
}

fn outcome(run: &Run) -> Outcome {
    let failures: Vec<String> =
        run.local_failures.iter().map(|f| format!("This app: {f}")).chain(run.machines.iter().flat_map(|m| m.result.clone().unwrap_or_default())).collect();
    let providers = run.requests.iter().filter(|r| r.reply.as_ref().is_some_and(|reply| providers::outcome("", &r.driver, &r.instance, reply).is_none())).count();
    let restarting = run.machines.iter().filter(|m| m.result.is_none() && handed_off(m)).filter_map(handed_off_to).collect();
    Outcome { saved_at: crate::data::now_ms(), local: run.local.clone(), servers: run.servers_updated, providers, failures, restarting }
}

/// `getUpdateEverythingToastView`: success, or an error listing the failures.
fn toast_view(result: &Outcome, app_version: &str) -> Option<(bool, String, Option<String>)> {
    let parts: Vec<String> = [result.local.as_deref().filter(|v| *v == app_version).map(|_| "this app".to_owned()), count(result.servers, "server"), count(result.providers, "provider")]
        .into_iter()
        .flatten()
        .collect();
    let labels: Vec<String> = result.restarting.iter().map(|h| h.label.clone()).collect();
    let restarting = (!labels.is_empty()).then(|| format!("{} {} still restarting.", join(&labels), if labels.len() == 1 { "is" } else { "are" }));
    if result.failures.is_empty() {
        return (!parts.is_empty()).then(|| (true, format!("Updated {}", join(&parts)), restarting));
    }
    let title = if parts.is_empty() { "Update failed".to_owned() } else { format!("Updated {}", join(&parts)) };
    Some((false, title, Some(result.failures.iter().cloned().chain(restarting).collect::<Vec<_>>().join("\n"))))
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
    let quiet = std::mem::take(&mut run.quiet);
    // `closeFinishedUpdateRun`.
    if run.local.is_none() && run.machines.iter().all(|m| m.result.as_ref().is_some_and(Vec::is_empty)) {
        ui.run = None;
    }
    if !quiet {
        show(s, &Outcome { local: None, ..result });
    }
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
    // A machine with a server to update, or one followed back from its restart, settles after it.
    for m in run.machines.iter_mut().filter(|m| m.result.is_none() && m.server.is_none() && m.handed_off.is_none()) {
        if replied(&run.requests, &m.key) {
            m.result = Some(provider_lines(&run.requests, m));
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
    changed |= advance_servers(s, &mut run, now);
    let (followed, deadline) = follow_servers(s, &mut run, now);
    changed |= followed;
    if let Some(deadline) = deadline {
        tx.wake_after(std::time::Duration::from_millis((deadline - now).max(1) as u64));
    }
    let providers_done = run.machines.iter().all(|m| m.result.is_some() || m.handed_off.is_some() || (m.server.is_some() && replied(&run.requests, &m.key)));
    let servers_waiting = run.machines.iter().any(|m| m.result.is_none() && matches!(m.server, Some((_, None))));
    // The app may restart once every server has handed off; without a restart, the run waits for them.
    let handed_off = run.machines.iter().all(|m| m.result.is_some() || handed_off(m));
    let settled = run.machines.iter().all(|m| m.result.is_some());
    let status = s.ui.desktop.updates.status.clone();
    let staged = updates::action(&s.ui.desktop.updates) == updates::Action::Install;
    let mut next = run.phase;
    match run.phase {
        Phase::Providers if providers_done && servers_waiting => {
            start_servers(s, &mut run, now);
            next = Phase::Servers;
        }
        Phase::Providers | Phase::Servers | Phase::Downloading if providers_done && !servers_waiting && handed_off => match (run.local.is_some(), &status) {
            (true, _) if staged => next = Phase::Countdown(now + RESTART_DELAY_MS),
            (true, Status::Error(_, message)) => {
                run.local_failures.push(format!("Could not download update: {message}"));
                run.local = None;
                next = if settled { Phase::Settled } else { Phase::Servers };
            }
            (true, _) => next = Phase::Downloading,
            (false, _) if settled => next = Phase::Settled,
            (false, _) => next = Phase::Servers,
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
                next = Phase::Servers;
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

/// The relaunched app's run over the servers still restarting, reported once they all settle.
fn follow(s: &mut App, servers: &[HandedOff]) {
    if servers.is_empty() {
        return;
    }
    let machines = servers
        .iter()
        .map(|h| Machine {
            key: h.key.clone(),
            label: h.label.clone(),
            started_at: h.started_at,
            result: None,
            updates_providers: false,
            server: None,
            provider_failures: vec![],
            handed_off: Some(h.target.clone()),
        })
        .collect();
    s.ui.shell.sidebar.everything.run = Some(Run { local: None, phase: Phase::Servers, machines, requests: vec![], local_failures: vec![], servers_updated: 0, quiet: false });
}

/// `startDesktopUpdateLaunchNotice`, once per launch.
fn launch_notices(s: &mut App) -> bool {
    if std::mem::replace(&mut s.ui.shell.sidebar.everything.launch_checked, true) {
        return false;
    }
    let mut changed = false;
    if let Some(saved) = take(SAVED_RESULT_KEY).as_ref().and_then(Outcome::from_json).filter(|o| crate::data::now_ms() - o.saved_at < SAVED_MAX_AGE_MS) {
        show(s, &saved);
        follow(s, &saved.restarting);
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

/// A run is showing the capsule, which takes the footer's free room (`flex-1` on web).
pub fn running(s: &App) -> bool {
    s.ui.shell.sidebar.everything.run.is_some()
}

/// The footer's update control: the capsule while a run is on, else the combined button when
/// servers or providers wait, else `None` for the app's own pill.
pub fn control(s: &App, t: &T) -> Option<El> {
    if let Some(run) = &s.ui.shell.sidebar.everything.run {
        return Some(capsule(s, t, run));
    }
    if !available(s) {
        return None;
    }
    let local = local_version(s).is_some();
    let count: usize = providers::machines(s).iter().map(|m| m.candidates.len()).sum();
    let tip = summary(local, servers(s).0.len(), count);
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
        None if run.phase == Phase::Providers && m.updates_providers => "updating providers",
        None if m.handed_off.is_some() => "restarting",
        None => match m.server.as_ref().and_then(|(_, u)| u.as_ref()).map(|u| u.status()) {
            Some(ServerStatus::Running { stage: Stage::Resuming, .. }) => "restarting",
            Some(ServerStatus::Running { .. }) => "downloading",
            _ => "waiting",
        },
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
    let action = if failed.iter().any(|m| m.server.is_some()) { "Retry" } else { "Dismiss" };
    match failed.as_slice() {
        [] => (Tone::Default, of, None, None),
        [one] => (Tone::Failed, format!("{} failed", one.label), None, Some(action)),
        many => (Tone::Failed, format!("{} failed", many.len()), None, Some(action)),
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
                .on_press(move |s: &mut App| match action {
                    "Later" => postpone(s),
                    "Retry" => retry(s),
                    _ => dismiss(s),
                }),
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
    if (ui.notes.0 || ui.notes.1) && notes_apply(s) {
        super::release_notes::view(s, t, |s, on| s.ui.shell.sidebar.everything.notes.1 = on, layers);
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
                    install(s, Outcome { saved_at: crate::data::now_ms(), local: version, ..Outcome::default() });
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
        let retries = run.machines.iter().any(|m| machine_status(run, m) == "failed" && m.server.is_some());
        body = body.child(
            row()
                .w_full()
                .mt(4.0)
                .gap(4.0)
                .justify_end()
                .children(retries.then(|| kit::Button::new("Retry").size("xs").variant("outline").on_press(retry).build(t)))
                .child(kit::Button::new("Dismiss").size("xs").variant("ghost").on_press(dismiss).build(t)),
        );
    }
    let surface = kit::menu::glass(body.padding(8.0), &k, false).modal(true, |s: &mut App| s.ui.shell.sidebar.everything.capsule_open = false);
    layers.push(kit::motion::popup(rune_kit::popover::anchored(&k, surface, "sb-update-capsule", rune_kit::Placement::TopEnd, 4.0)));
}

/// `UpdateEverythingDialog`.
fn dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let local = local_version(s);
    let rows = rows(s, local.is_some());
    let providers: usize = rows.iter().map(|(_, r)| r.providers.len()).sum();
    let version = local.clone().or_else(|| servers(s).0.into_iter().find(|t| t.self_update != "desktop-managed").map(|t| t.target_version));
    let description = [version.map(|v| format!("S5 Code {v}")), (providers > 0).then(|| format!("{providers} provider {}", if providers == 1 { "update" } else { "updates" }))]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let muted = t.c.muted_foreground;
    let cell = |w: f32, e: El| el().w(w).min_w(0.0).child(e);
    let mut table = col().w_full().child(
        row().w_full().gap(16.0).pb(6.0).child(cell(128.0, text("Machine").font(theme::TEXT_XS).color(muted))).child(cell(96.0, text("S5 Code").font(theme::TEXT_XS).color(muted))).child(text("Providers").font(theme::TEXT_XS).color(muted)),
    ).child(el().w_full().h(1.0).fill(t.c.border));
    let updating: Vec<&DialogRow> = rows.iter().map(|(_, r)| r).filter(|r| r.app.is_some() || !r.providers.is_empty()).collect();
    let skipped: Vec<&DialogRow> = rows.iter().map(|(_, r)| r).filter(|r| r.skipped.is_some()).collect();
    for row_ in &updating {
        let app = match (row_.app, &row_.skipped) {
            (Some("This app"), _) => col().child(text("This app").font(theme::TEXT_SM)).child(text("restarts last").font(theme::TEXT_XS).color(muted)),
            (Some(app), _) => col().child(text(app).font(theme::TEXT_SM)),
            (None, Some(_)) => col().child(text("Skipped").font(theme::TEXT_SM).color(muted)),
            (None, None) => col(),
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
    if !skipped.is_empty() {
        let open = s.ui.shell.sidebar.everything.skipped_open;
        table = table.child(
            row()
                .pt(8.0)
                .gap(4.0)
                .center_y()
                .role("button")
                .cursor(Cursor::Pointer)
                .child(icon(if open { &i::CHEVRON_DOWN } else { &i::CHEVRON_RIGHT }, 14.0).color(muted))
                .child(text(format!("{} skipped", skipped.len())).font(theme::TEXT_XS).color(muted))
                .on_press(|s: &mut App| s.ui.shell.sidebar.everything.skipped_open ^= true),
        );
        if open {
            for row_ in &skipped {
                table = table.child(
                    row()
                        .pl(18.0)
                        .pt(2.0)
                        .gap(12.0)
                        .child(el().max_w(128.0).min_w(0.0).child(text(row_.label.clone()).font(theme::TEXT_XS).color(muted).truncate()))
                        .child(text(row_.skipped.clone().unwrap_or_default()).font(theme::TEXT_XS).color(muted).wrap().shrink()),
                );
            }
        }
    }
    let stopped: usize = updating.iter().filter(|r| !r.continues).map(|r| r.running).sum();
    let continuing: usize = updating.iter().filter(|r| r.continues).map(|r| r.running).sum();
    let note = interrupted(stopped, continuing);
    let close = |s: &mut App| s.ui.shell.sidebar.everything.confirming = false;
    let n = updating.len();
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

/// The dialog's footnote on the threads the restarts interrupt.
fn interrupted(stopped: usize, continuing: usize) -> Option<String> {
    let running = stopped + continuing;
    let threads = format!("{running} running {}", if running == 1 { "thread" } else { "threads" });
    (running > 0).then(|| match (stopped, continuing) {
        (0, _) => format!("{threads} will be interrupted and can resume after the restart."),
        (_, 0) => format!("{threads} will be interrupted."),
        _ => format!("{threads} will be interrupted. {continuing} can resume after the restart."),
    })
}

/// Hovering the app's pill shows its release notes, as web's hover popover.
pub fn notes_trigger(pill: El) -> El {
    pill.hover_delays(std::time::Duration::ZERO, std::time::Duration::from_millis(150))
        .on_hover_change(|s: &mut App, on, _: &mut Tx| s.ui.shell.sidebar.everything.notes.0 = on)
}

/// `shouldUseSidebarUpdateReleaseNotesPopover`: an update to show with notes to read, and no run.
pub fn notes_apply(s: &App) -> bool {
    let ui = &s.ui.desktop.updates;
    let details = updates::action(ui) != updates::Action::None || matches!(ui.status, Status::Downloading(_));
    s.ui.shell.sidebar.everything.run.is_none() && details && !super::release_notes::current(s).0.is_empty()
}

/// Two saved servers older than this app's bundled one: Hetzner box to update, and Mac mini,
/// whose session can't maintain it.
fn remote_servers(s: &mut App) {
    for (id, label) in [("env-box", "Hetzner box"), ("env-mini", "Mac mini")] {
        s.ui.env.saved.push(env::Saved { id: id.into(), label: label.into(), base: format!("https://{id}.example.com"), bearer: String::new() });
        let mut state = crate::data::fixture_state();
        // The local fixture's threads would list once per machine.
        state.threads.clear();
        if let Some(config) = state.config.as_mut() {
            config["environment"]["serverVersion"] = json!("0.1.5");
            config["environment"]["capabilities"]["serverSelfUpdate"] = json!("boot-service");
        }
        env::park(s, id, crate::data::Source::Fixture(state));
    }
    crate::settings::permissions::set_session(s, "env-mini", json!({"authenticated": true, "scopes": ["orchestration:read"]}));
}

/// Saved servers update everything skips, one per reason the server itself gives.
fn skipped_servers(s: &mut App) {
    s.ui.env.saved.push(env::Saved { id: "env-pi".into(), label: "Raspberry Pi".into(), base: "https://env-pi.example.com".into(), bearer: String::new() });
    let servers = [("env-old", "Old laptop", "0.1.5", None, false), ("env-studio", "Studio", "0.1.5", Some("desktop-managed"), false), ("env-edge", "Edge box", "0.1.5-nightly.20261101.1", Some("boot-service"), false)];
    for (id, label, version, self_update, desktop_app) in servers {
        s.ui.env.saved.push(env::Saved { id: id.into(), label: label.into(), base: format!("https://{id}.example.com"), bearer: String::new() });
        let mut state = crate::data::fixture_state();
        state.threads.clear();
        if let Some(config) = state.config.as_mut() {
            config["environment"]["serverVersion"] = json!(version);
            config["environment"]["capabilities"]["serverSelfUpdate"] = json!(self_update);
            config["environment"]["capabilities"]["desktopAppUpdate"] = json!(desktop_app);
        }
        env::park(s, id, crate::data::Source::Fixture(state));
    }
}

fn box_target() -> Target {
    Target { key: "env-box".into(), label: "Hetzner box".into(), self_update: "boot-service".into(), target_version: "0.1.7".into() }
}

/// Scenes: the confirm dialog, the capsule counting down with its popover, a failed server to
/// retry, and the app pill's release notes.
pub fn scene(s: &mut App, which: &str) {
    let now = crate::data::now_ms();
    let machine = |key: &str, label: &str, result: Option<Vec<String>>, server| Machine {
        key: key.to_owned(),
        label: label.to_owned(),
        started_at: now - 90_000,
        result,
        updates_providers: true,
        server,
        provider_failures: vec![],
        handed_off: None,
    };
    match which {
        "confirm" => {
            app_update(s);
            remote_servers(s);
            skipped_servers(s);
            s.ui.shell.sidebar.everything.confirming = true;
            s.ui.shell.sidebar.everything.skipped_open = true;
        }
        "capsule" => {
            app_update(s);
            remote_servers(s);
            let restarting = server_update::State::at(ServerStatus::Running { stage: Stage::Resuming, from: "0.1.5".into(), target: "0.1.7".into() });
            s.ui.shell.sidebar.everything.run = Some(Run {
                local: local_version(s),
                phase: Phase::Countdown(now + 4_000),
                machines: vec![machine(env::LOCAL, "This machine", Some(vec![]), None), machine("env-box", "Hetzner box", None, Some((box_target(), Some(restarting))))],
                requests: vec![],
                local_failures: vec![],
                servers_updated: 0,
                quiet: false,
            });
            s.ui.shell.sidebar.everything.capsule_open = true;
        }
        // The relaunched app follows Hetzner box, still restarting after 4 minutes.
        "relaunch" => {
            app_update(s);
            remote_servers(s);
            follow(s, &[HandedOff { key: "env-box".into(), label: "Hetzner box".into(), target: "0.1.7".into(), started_at: now - 240_000 }]);
            s.ui.shell.sidebar.everything.capsule_open = true;
        }
        "retry" => {
            app_update(s);
            remote_servers(s);
            s.ui.shell.sidebar.everything.run = Some(Run {
                local: None,
                phase: Phase::Settled,
                machines: vec![
                    machine(env::LOCAL, "This machine", Some(vec![]), None),
                    machine("env-box", "Hetzner box", Some(vec!["Hetzner box: Server update failed: npm is offline".into()]), Some((box_target(), None))),
                ],
                requests: vec![],
                local_failures: vec![],
                servers_updated: 0,
                quiet: false,
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
            s.ui.desktop.updates.history = vec![
                json!({"version": "0.1.3", "note": "<p>v0.1.3 · 14 PRs · 5 contributors</p><ul><li><strong>Speech menu</strong> dictates into the composer</li><li>Device streaming starts faster</li><li>Notifications open their thread</li></ul><h3>Plus 6 more fixes</h3><details><summary>Full changelog</summary></details>", "publishedAt": "2026-11-23T09:00:00.000Z"}),
                json!({"version": "0.1.2", "note": "## What's Changed\n* Sidebar footer keeps the update capsule full width\n* Release notes list older releases\n\n**Full Changelog**: https://github.com/o/r/compare/rune-v0.1.1...rune-v0.1.2"}),
            ];
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
        Machine { key: label.to_owned(), label: label.to_owned(), started_at, result, updates_providers: true, server: None, provider_failures: vec![], handed_off: None }
    }

    fn run(phase: Phase, machines: Vec<Machine>) -> Run {
        Run { local: Some("0.1.3".into()), phase, machines, requests: vec![], local_failures: vec![], servers_updated: 0, quiet: false }
    }

    #[test]
    fn the_summary_and_result_read_as_web_s() {
        assert_eq!(summary(true, 0, 2), "Update this app and 2 providers");
        assert_eq!(summary(false, 0, 1), "Update 1 provider");
        assert_eq!(summary(true, 1, 2), "Update this app, 1 server, and 2 providers");
        assert_eq!(join(&["a".into(), "b".into(), "c".into()]), "a, b, and c");
        let ok = Outcome { local: Some("0.1.3".into()), providers: 2, ..Outcome::default() };
        assert_eq!(toast_view(&ok, "0.1.3"), Some((true, "Updated this app and 2 providers".into(), None)));
        assert_eq!(toast_view(&ok, "0.1.2").map(|v| v.1), Some("Updated 2 providers".into()), "FAILED: an app still on the old version did not update");
        let failed = Outcome { failures: vec!["Box · Codex: denied".into()], ..Outcome::default() };
        assert_eq!(toast_view(&failed, "0.1.3"), Some((false, "Update failed".into(), Some("Box · Codex: denied".into()))));
        assert_eq!(toast_view(&Outcome { failures: vec![], ..failed }, "0.1.3"), None);
        let mini = HandedOff { key: "mini".into(), label: "Mini".into(), target: "0.1.7".into(), started_at: 5 };
        let handed_off = Outcome { servers: 1, restarting: vec![mini.clone()], ..Outcome::default() };
        assert_eq!(toast_view(&handed_off, "0.1.3"), Some((true, "Updated 1 server".into(), Some("Mini is still restarting.".into()))));
        assert_eq!(Outcome::from_json(&Outcome { saved_at: 1, ..handed_off }.to_json()).map(|o| o.restarting), Some(vec![mini]), "FAILED: the relaunched app follows the restarting server");
        assert_eq!(interrupted(2, 1).as_deref(), Some("3 running threads will be interrupted. 1 can resume after the restart."));
        assert_eq!(interrupted(0, 1).as_deref(), Some("1 running thread will be interrupted and can resume after the restart."));
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

    fn sent(s: &mut App, key: &str, method: &str) -> u64 {
        let net = env::net_of(s, key).expect("live");
        net.awaiting().filter(|r| r.1 == method).map(|r| r.0).max().expect("FAILED: the request went out")
    }

    #[test]
    fn the_relaunched_app_follows_a_server_still_restarting() {
        use crate::data::Source;
        let mut s = App::new();
        s.src = Source::Live(s5_net::Net::in_memory("http://local.test"));
        for id in ["box", "mini"] {
            s.ui.env.saved.push(env::Saved { id: id.into(), label: id.into(), base: format!("http://{id}.test"), bearer: String::new() });
            let mut net = s5_net::Net::in_memory(&format!("http://{id}.test"));
            net.state_mut().config = Some(json!({"environment": {"serverVersion": "0.1.5"}}));
            env::park(&mut s, id, Source::Live(net));
        }
        let now = crate::data::now_ms();
        let box_ = HandedOff { key: "box".into(), label: "Box".into(), target: "0.1.7".into(), started_at: now };
        let mini = HandedOff { key: "mini".into(), label: "Mini".into(), target: "0.1.7".into(), started_at: now - HANDOFF_TIMEOUT_MS };
        follow(&mut s, &[box_]);
        let tx = &mut Tx::default();
        poll(&mut s, tx);
        let r = s.ui.shell.sidebar.everything.run.as_ref().expect("FAILED: the relaunched app drops the restarting server");
        assert_eq!((r.phase, machine_status(r, &r.machines[0])), (Phase::Servers, "restarting"));
        assert!(following(&s, "box"), "FAILED: its update would be offered again");
        env::net_of(&mut s, "box").unwrap().state_mut().config = Some(json!({"environment": {"serverVersion": "0.1.7"}}));
        poll(&mut s, tx);
        assert!(s.ui.shell.sidebar.everything.run.is_none(), "FAILED: the server came back on 0.1.7, so the run closes");
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Updated 1 server"));

        follow(&mut s, &[mini]);
        poll(&mut s, tx);
        let r = s.ui.shell.sidebar.everything.run.as_ref().expect("the failure stays");
        assert_eq!(r.machines[0].result, Some(vec!["Mini: Not back after 32 minutes".to_owned()]));
        assert_eq!(describe(r, None, now), (Tone::Failed, "Mini failed".into(), None, Some("Dismiss")));
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Update failed"));
    }

    #[test]
    fn a_remote_server_updates_after_the_providers_and_a_failure_retries() {
        use crate::data::Source;
        let mut s = App::new();
        s.src = Source::Live(s5_net::Net::in_memory("http://local.test"));
        s.ui.env.saved.push(env::Saved { id: "box".into(), label: "Box".into(), base: "http://box.test".into(), bearer: String::new() });
        env::park(&mut s, "box", Source::Live(s5_net::Net::in_memory("http://box.test")));
        crate::settings::permissions::set_session(&mut s, "box", json!({"authenticated": true, "scopes": ["environment:maintain"]}));
        let target = Target { key: "box".into(), label: "Box".into(), self_update: "respawn".into(), target_version: "0.1.7".into() };
        let mut started = run(Phase::Providers, vec![Machine { server: Some((target, None)), updates_providers: false, ..machine("Box", 0, None) }]);
        started.machines[0].key = "box".into();
        started.local = None;
        s.ui.shell.sidebar.everything.run = Some(started);
        let tx = &mut Tx::default();

        poll(&mut s, tx);
        let ui = &s.ui.shell.sidebar.everything;
        let r = ui.run.as_ref().expect("running");
        assert_eq!((r.phase, machine_status(r, &r.machines[0])), (Phase::Servers, "downloading"), "FAILED: the server starts once the providers are done");
        let failed = sent(&mut s, "box", "server.updateServer");
        env::net_of(&mut s, "box").expect("live").put_reply(failed, Err(json!([{"_tag": "Fail", "error": {"_tag": "ServerSelfUpdateError", "reason": "npm is offline"}}])));
        poll(&mut s, tx);
        let r = s.ui.shell.sidebar.everything.run.as_ref().expect("a failure stays");
        assert_eq!(r.machines[0].result, Some(vec!["Box: Server update failed: npm is offline".to_owned()]));
        assert_eq!(describe(r, None, 0), (Tone::Failed, "Box failed".into(), None, Some("Retry")));
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Update failed"));

        retry(&mut s);
        let again = sent(&mut s, "box", "server.updateServer");
        assert_ne!(again, failed, "FAILED: Retry sends the update again");
        env::net_of(&mut s, "box").expect("live").put_reply(again, Ok(json!({"targetVersion": "0.1.7", "method": "respawn"})));
        poll(&mut s, tx);
        let r = s.ui.shell.sidebar.everything.run.as_ref().expect("restarting");
        assert_eq!(machine_status(r, &r.machines[0]), "restarting");
        let ready = json!({"type": "ready", "payload": {"environment": {"serverVersion": "0.1.7"}}});
        env::net_of(&mut s, "box").expect("live").put_resource_item(crate::shell::lifecycle::METHOD, json!({}), ready);
        crate::shell::lifecycle::poll(&mut s);
        poll(&mut s, tx);
        assert!(s.ui.shell.sidebar.everything.run.is_none(), "FAILED: the server came back on 0.1.7, so the run closes");

        // A retry without environment:maintain any more can't start.
        let mut lost = run(Phase::Settled, vec![Machine { key: "box".into(), server: Some((Target { key: "box".into(), label: "Box".into(), self_update: "respawn".into(), target_version: "0.1.7".into() }, None)), ..machine("Box", 0, Some(vec!["Box: denied".into()])) }]);
        lost.local = None;
        s.ui.shell.sidebar.everything.run = Some(lost);
        crate::settings::permissions::set_session(&mut s, "box", json!({"authenticated": true, "scopes": ["orchestration:read"]}));
        retry(&mut s);
        let r = s.ui.shell.sidebar.everything.run.as_ref().expect("still failed");
        assert_eq!(r.machines[0].result, Some(vec!["Box: Could not start the update".to_owned()]));
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Update failed"), "FAILED: a retry settles without a second report");
    }
}
