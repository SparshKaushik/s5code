//! SSH environments, as Electron's `packages/ssh` and the web's SSH add flow: hosts come from
//! `~/.ssh/config` and `known_hosts`, `ssh` launches or reuses t3 on the host with the same remote
//! scripts, `ssh -N -L` forwards a loopback port to it, and the environment pairs through that
//! port and is saved like any other. A saved one re-ensures its tunnel at startup and when the
//! tunnel exits. Passwords and verification codes go to `ssh` through `SSH_ASKPASS`.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rune::{El, TextEdit, Waker, col, row, text};
use serde_json::{Value, json};

use super::Saved;
use crate::theme::{self, T};
use crate::{App, icons, kit};

const DEFAULT_REMOTE_PORT: u16 = 3773;
const REMOTE_PORT_SCAN_WINDOW: u16 = 200;
const REMOTE_READY_TIMEOUT_MS: u32 = 60_000;
const REMOTE_REUSE_READY_TIMEOUT_MS: u32 = 2_000;
const READY_PROBE_TIMEOUT_MS: u32 = 1_000;
const ARCHIVE_CHECKSUMS_SECONDS: u32 = 30;
const ARCHIVE_DOWNLOAD_SECONDS: u32 = 240;
const ARCHIVE_LOCK_WAIT_SECONDS: u32 = 360;
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(900);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const TUNNEL_READY_TIMEOUT: Duration = Duration::from_secs(20);
const TUNNEL_REUSE_TIMEOUT: Duration = Duration::from_secs(2);
const STDERR_TAIL: usize = 16_384;
const RELEASE_BASE_URL: &str = "https://github.com/SparshKaushik/s5code/releases/download";
/// The job slot of the add form; environment ids name the others.
const ADD: &str = "\0add";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub alias: String,
    pub hostname: String,
    pub username: Option<String>,
    pub port: Option<u16>,
}

impl Target {
    /// `targetConnectionKey`.
    fn key(&self) -> String {
        let port = self.port.map(|p| p.to_string()).unwrap_or_default();
        format!("{}\0{}\0{}\0{port}", self.alias, self.hostname, self.username.as_deref().unwrap_or(""))
    }

    /// `remoteStateKey`: names the remote launch state directory, shared with Electron.
    fn state_key(&self) -> String {
        let digest = ring::digest::digest(&ring::digest::SHA256, self.key().as_bytes());
        digest.as_ref().iter().take(8).map(|b| format!("{b:02x}")).collect()
    }

    fn host_spec(&self) -> String {
        let destination = if self.alias.trim().is_empty() { self.hostname.trim() } else { self.alias.trim() };
        match &self.username {
            Some(user) => format!("{user}@{destination}"),
            None => destination.to_owned(),
        }
    }

    /// `formatDesktopSshTarget`.
    pub fn display(&self) -> String {
        let authority = match &self.username {
            Some(user) => format!("{user}@{}", self.hostname),
            None => self.hostname.clone(),
        };
        match self.port {
            Some(port) => format!("{authority}:{port}"),
            None => authority,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Host {
    alias: String,
    hostname: String,
}

#[derive(Clone, PartialEq)]
enum Kind {
    Add,
    Reconnect(String),
}

#[derive(Clone)]
struct Job {
    kind: Kind,
    target: Target,
    secret: Option<String>,
    prompts: u8,
    /// Started by the user, so an auth failure asks for a password instead of failing.
    interactive: bool,
    version: Option<String>,
    preferred: Option<u16>,
    /// The saved environment's bearer, checked before it is reused.
    bearer: Option<String>,
}

impl Job {
    fn slot(&self) -> &str {
        match &self.kind {
            Kind::Add => ADD,
            Kind::Reconnect(id) => id,
        }
    }
}

enum Done {
    Added(Target, Saved),
    /// `bearer` is a fresh one when the saved session no longer worked.
    Reconnected { id: String, base: String, bearer: Option<String> },
}

struct Finished {
    job: Job,
    result: Result<Done, String>,
}

#[derive(Default)]
pub struct Ui {
    /// Saved SSH environments by environment id, with the target that reaches each.
    targets: Vec<(String, Target)>,
    hosts: Vec<Host>,
    pub host: TextEdit,
    pub username: TextEdit,
    pub port: TextEdit,
    pub password: TextEdit,
    jobs: Vec<(String, Receiver<Finished>)>,
    /// A job waiting on the password dialog.
    prompt: Option<Job>,
    pub error: Option<String>,
    notes: HashMap<String, String>,
    revived: HashMap<String, Instant>,
}

fn busy(ui: &Ui, slot: &str) -> bool {
    ui.jobs.iter().any(|(k, _)| k == slot) || ui.prompt.as_ref().is_some_and(|j| j.slot() == slot)
}

// ---------------------------------------------------------------------------------------------
// Persistence and lifecycle

fn targets_path() -> PathBuf {
    super::path().with_extension("ssh.json")
}

fn decode_targets(value: &Value) -> Vec<(String, Target)> {
    let field = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_owned();
    value["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| {
            let target = Target {
                alias: field(v, "alias"),
                hostname: field(v, "hostname"),
                username: v["username"].as_str().filter(|u| !u.is_empty()).map(str::to_owned),
                port: v["port"].as_u64().and_then(|p| u16::try_from(p).ok()).filter(|p| *p > 0),
            };
            (field(v, "environmentId"), target)
        })
        .filter(|(id, t)| !id.is_empty() && !(t.alias.is_empty() && t.hostname.is_empty()))
        .collect()
}

fn encode_targets(targets: &[(String, Target)]) -> Value {
    let rows: Vec<Value> = targets
        .iter()
        .map(|(id, t)| json!({"environmentId": id, "alias": t.alias, "hostname": t.hostname, "username": t.username, "port": t.port}))
        .collect();
    json!({ "version": 1, "targets": rows })
}

fn save_targets(targets: &[(String, Target)]) -> Result<(), String> {
    let path = targets_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&encode_targets(targets)).unwrap_or_default()).map_err(|e| e.to_string())
}

/// Loads the saved SSH targets and discovered hosts, and re-ensures each saved tunnel.
pub(super) fn start(s: &mut App) {
    let loaded = std::fs::read_to_string(targets_path()).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|v| decode_targets(&v)).unwrap_or_default();
    let saved = &s.ui.env.saved;
    s.ui.env.ssh.targets = loaded.into_iter().filter(|(id, _)| saved.iter().any(|e| &e.id == id)).collect();
    s.ui.env.ssh.hosts = std::env::var_os("HOME").map(|home| discover(Path::new(&home))).unwrap_or_default();
    if rune::is_headless() {
        return;
    }
    let ids: Vec<String> = s.ui.env.ssh.targets.iter().map(|(id, _)| id.clone()).filter(|id| !s.ui.env.off.contains(id)).collect();
    for id in ids {
        reconnect(s, &id, false);
    }
}

static EXITED: AtomicBool = AtomicBool::new(false);

/// Lands finished SSH jobs and re-ensures tunnels that exited.
pub(super) fn poll(s: &mut App) -> bool {
    let mut changed = EXITED.swap(false, Ordering::Relaxed) && revive(s);
    changed |= repair(s);
    if s.ui.env.ssh.jobs.is_empty() {
        return changed;
    }
    let mut finished = Vec::new();
    s.ui.env.ssh.jobs.retain(|(_, rx)| match rx.try_recv() {
        Ok(done) => {
            finished.push(done);
            false
        }
        Err(TryRecvError::Empty) => true,
        Err(TryRecvError::Disconnected) => false,
    });
    for done in finished {
        land(s, done);
        changed = true;
    }
    changed
}

/// Removing an environment closes its tunnel and stops the server this app launched there.
pub(super) fn forget(s: &mut App, id: &str) {
    let ui = &mut s.ui.env.ssh;
    let Some(i) = ui.targets.iter().position(|(k, _)| k == id) else { return };
    let (_, target) = ui.targets.remove(i);
    ui.notes.remove(id);
    ui.error = save_targets(&ui.targets).err().map(|e| format!("Could not save SSH environments: {e}"));
    close(&target.key());
    std::thread::spawn(move || {
        let resolved = resolve(&target);
        let secret = secret_for(&resolved.key());
        let _ = run_ssh(&resolved, secret.as_deref(), &[], &["sh", "-s"], Some(stop_script(&resolved.state_key()).as_str()), COMMAND_TIMEOUT);
    });
}

/// How long quitting waits for remote servers to stop.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Electron's tunnel finalizers on quit: close each forward, then stop the server this app
/// launched there, with the password it last used or in batch mode.
pub fn shutdown() {
    let tunnels: Vec<(String, Tunnel)> = lock(&TUNNELS).drain(..).collect();
    let (tx, rx) = channel();
    let count = tunnels.len();
    for (key, mut tunnel) in tunnels {
        let _ = tunnel.child.kill();
        let _ = tunnel.child.wait();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let secret = secret_for(&key);
            let script = stop_script(&tunnel.target.state_key());
            let _ = run_ssh(&tunnel.target, secret.as_deref(), &[], &["sh", "-s"], Some(script.as_str()), SHUTDOWN_GRACE);
            let _ = tx.send(());
        });
    }
    let deadline = Instant::now() + SHUTDOWN_GRACE;
    for _ in 0..count {
        if rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).is_err() {
            return;
        }
    }
}

fn version(s: &App) -> Option<String> {
    super::source(s, super::LOCAL)?.state().config.as_ref()?["environment"]["serverVersion"].as_str().map(str::to_owned)
}

fn port_of(base: &str) -> Option<u16> {
    base.trim_end_matches('/').rsplit_once(':')?.1.parse().ok()
}

fn spawn(s: &mut App, job: Job) {
    if busy(&s.ui.env.ssh, job.slot()) {
        return;
    }
    let slot = job.slot().to_owned();
    let (tx, rx) = channel();
    let waker = s.ui.panel.waker.clone();
    std::thread::spawn(move || {
        let result = run(&job, waker.clone());
        let _ = tx.send(Finished { job, result });
        if let Some(w) = waker {
            w();
        }
    });
    s.ui.env.ssh.jobs.push((slot, rx));
}

pub fn add(s: &mut App) {
    if busy(&s.ui.env.ssh, ADD) {
        return;
    }
    let ui = &s.ui.env.ssh;
    match parse_manual(&ui.host.text, &ui.username.text, &ui.port.text) {
        Err(error) => s.ui.env.ssh.error = Some(error),
        Ok(target) => {
            s.ui.env.ssh.error = None;
            let job = Job { kind: Kind::Add, target, secret: None, prompts: 0, interactive: true, version: version(s), preferred: None, bearer: None };
            spawn(s, job);
        }
    }
}

fn reconnect(s: &mut App, id: &str, interactive: bool) {
    let Some(target) = s.ui.env.ssh.targets.iter().find(|(k, _)| k == id).map(|(_, t)| t.clone()) else { return };
    let tunnel = tunnel_route(s, id);
    let preferred = tunnel.as_ref().and_then(|r| port_of(&r.base));
    let bearer = tunnel.map(|r| r.bearer).filter(|b| !b.is_empty());
    s.ui.env.ssh.notes.remove(id);
    let job = Job { kind: Kind::Reconnect(id.to_owned()), target, secret: None, prompts: 0, interactive, version: version(s), preferred, bearer };
    spawn(s, job);
}

/// Re-ensures saved environments whose tunnel exited, at most once a minute each.
fn revive(s: &mut App) -> bool {
    let dead: Vec<String> = {
        let mut tunnels = lock(&TUNNELS);
        let mut dead = Vec::new();
        tunnels.retain_mut(|(key, tunnel)| {
            let alive = matches!(tunnel.child.try_wait(), Ok(None));
            if !alive {
                dead.push(key.clone());
            }
            alive
        });
        dead
    };
    let now = Instant::now();
    let ui = &s.ui.env.ssh;
    let ids: Vec<String> = ui
        .targets
        .iter()
        .filter(|(id, t)| dead.contains(&t.key()) && ui.revived.get(id).is_none_or(|at| now.duration_since(*at) > Duration::from_secs(60)))
        .map(|(id, _)| id.clone())
        .collect();
    for id in &ids {
        s.ui.env.ssh.revived.insert(id.clone(), now);
        reconnect(s, id, false);
    }
    !ids.is_empty()
}

/// A saved SSH environment whose server refuses its bearer (the session expired or was revoked)
/// reconnects, which re-pairs through the tunnel; at most once a minute each, like `revive`.
fn repair(s: &mut App) -> bool {
    let now = Instant::now();
    let ui = &s.ui.env.ssh;
    let ids: Vec<String> = ui
        .targets
        .iter()
        .map(|(id, _)| id)
        .filter(|id| !busy(ui, id) && ui.revived.get(*id).is_none_or(|at| now.duration_since(*at) > Duration::from_secs(60)))
        .filter(|id| super::source(s, id).is_some_and(|src| matches!(&src.state().conn, s5_net::Conn::Closed(reason) if super::rejected(reason))))
        .cloned()
        .collect();
    for id in &ids {
        s.ui.env.ssh.revived.insert(id.clone(), now);
        reconnect(s, id, false);
    }
    !ids.is_empty()
}

fn fail(s: &mut App, job: &Job, error: String) {
    if job.kind == Kind::Add && s.ui.env.pairing.is_none() {
        s.ui.env.route_for = None;
    }
    let ui = &mut s.ui.env.ssh;
    match &job.kind {
        Kind::Add => ui.error = Some(error),
        Kind::Reconnect(id) => {
            ui.notes.insert(id.clone(), error);
        }
    }
}

fn land(s: &mut App, Finished { job, result }: Finished) {
    match result {
        Ok(Done::Added(target, env)) => {
            let id = env.id.clone();
            let route = s.ui.env.route_for.clone();
            let previous = s.ui.env.ssh.targets.iter().find(|(k, _)| *k == id).cloned();
            if route.as_deref() == Some(id.as_str()) {
                // Known before it joins, so the tunnel ranks as SSH among the routes.
                s.ui.env.ssh.targets.retain(|(k, _)| *k != id);
                s.ui.env.ssh.targets.push((id.clone(), target.clone()));
            }
            super::paired(s, Ok(env));
            let landed = if route.is_some() { s.ui.env.route_added.is_some() } else { s.ui.env.saved.iter().any(|e| e.id == id) };
            if !landed && route.is_some() {
                s.ui.env.ssh.targets.retain(|(k, _)| *k != id);
                s.ui.env.ssh.targets.extend(previous);
            }
            if landed {
                let ui = &mut s.ui.env.ssh;
                ui.targets.retain(|(k, _)| *k != id);
                ui.targets.push((id.clone(), target));
                ui.notes.remove(&id);
                ui.host.clear();
                ui.username.clear();
                ui.port.clear();
                ui.error = save_targets(&ui.targets).err().map(|e| format!("Could not save SSH environments: {e}"));
            } else {
                close(&target.key());
                s.ui.env.ssh.error = s.ui.env.error.take();
            }
        }
        Ok(Done::Reconnected { id, base, bearer }) => {
            s.ui.env.ssh.notes.remove(&id);
            rebase(s, &id, base, bearer);
        }
        Err(error) if job.interactive && job.prompts < 2 && is_auth_failure(&error) && !is_agent_signing_failure(&error) => {
            s.ui.env.ssh.password = TextEdit::new(String::new()).secure();
            s.ui.env.ssh.prompt = Some(Job { secret: None, ..job });
        }
        Err(error) => fail(s, &job, error),
    }
}

/// Re-ensures a switched-on SSH environment's tunnel.
pub(super) fn resume(s: &mut App, id: &str) {
    if s.ui.env.ssh.targets.iter().any(|(k, _)| k == id) && !rune::is_headless() {
        reconnect(s, id, false);
    }
}

/// A tunnel that came back on another local port, or a session paired again, moves the saved
/// environment with it.
fn rebase(s: &mut App, id: &str, base: String, bearer: Option<String>) {
    let mut routes = super::routes(s, id);
    // The tunnel is the environment's loopback route; it is not always the preferred one.
    let Some(route) = routes.iter_mut().find(|r| is_tunnel(s, id, &r.base)) else { return };
    if route.base == base && bearer.is_none() {
        return;
    }
    route.base = base;
    if let Some(bearer) = bearer {
        route.bearer = bearer;
    }
    // Reconnects when the tunnel is the first route, or was the one in use.
    super::set_routes(s, id, routes);
}

/// Whether `base` is the SSH tunnel of the saved environment `id`.
pub(crate) fn is_tunnel(s: &App, id: &str, base: &str) -> bool {
    s.ui.env.ssh.targets.iter().any(|(k, _)| k == id) && super::route_kind(base) == "loopback"
}

fn tunnel_route(s: &App, id: &str) -> Option<super::Alt> {
    super::routes(s, id).into_iter().find(|r| is_tunnel(s, id, &r.base))
}

/// Switching an SSH environment off closes its forward; switching it on reconnects ([`resume`]).
pub(super) fn suspend(s: &App, id: &str) {
    if let Some((_, target)) = s.ui.env.ssh.targets.iter().find(|(k, _)| k == id) {
        close(&target.key());
    }
}

fn answer(s: &mut App) {
    let ui = &mut s.ui.env.ssh;
    if ui.password.text.is_empty() {
        return;
    }
    let Some(mut job) = ui.prompt.take() else { return };
    job.secret = Some(ui.password.text.clone());
    ui.password = TextEdit::default();
    job.prompts += 1;
    spawn(s, job);
}

fn cancel(s: &mut App) {
    s.ui.env.ssh.password = TextEdit::default();
    let Some(job) = s.ui.env.ssh.prompt.take() else { return };
    fail(s, &job, format!("SSH authentication cancelled for {}.", job.target.host_spec()));
}

// ---------------------------------------------------------------------------------------------
// Settings UI

/// An SSH environment's connection state when its tunnel says more than the server's status:
/// connecting, or the last reconnect's failure.
pub fn state(s: &App, id: &str) -> Option<String> {
    let ui = &s.ui.env.ssh;
    if busy(ui, id) {
        return Some("Connecting over SSH…".into());
    }
    ui.notes.get(id).cloned()
}

/// Reconnect, on an SSH environment's row.
pub fn row_action(s: &App, t: &T, id: &str) -> Option<El> {
    let ui = &s.ui.env.ssh;
    ui.targets.iter().any(|(k, _)| k == id).then(|| {
        let target = id.to_owned();
        kit::icon_button(&icons::REFRESH_CW, "Reconnect over SSH")
            .variant("ghost")
            .size("icon-xs")
            .disabled(busy(ui, id))
            .on_press(move |s: &mut App| reconnect(s, &target, true))
            .build(t)
    })
}

/// Add environment's SSH mode: an add is connecting.
pub fn adding(s: &App) -> bool {
    busy(&s.ui.env.ssh, ADD)
}

/// SSH config hosts not saved yet that match the host field: alias and hostname.
pub fn suggestions(s: &App) -> Vec<(String, String)> {
    let ui = &s.ui.env.ssh;
    let saved: Vec<&str> = ui.targets.iter().map(|(_, t)| t.alias.as_str()).collect();
    let unsaved: Vec<Host> = ui.hosts.iter().filter(|h| !saved.contains(&h.alias.as_str())).cloned().collect();
    filter_hosts(&unsaved, &ui.host.text).into_iter().map(|h| (h.alias, h.hostname)).collect()
}

/// Web `environmentTransportLabel` for an SSH environment: "SSH user@host:port".
pub fn transport(s: &App, id: &str) -> Option<String> {
    s.ui.env.ssh.targets.iter().find(|(k, _)| k == id).map(|(_, t)| format!("SSH {}", t.display()))
}

/// The SSH alias that reaches a saved environment.
pub fn alias<'a>(s: &'a App, id: &str) -> Option<&'a str> {
    s.ui.env.ssh.targets.iter().find(|(k, _)| k == id).map(|(_, t)| t.alias.as_str())
}

/// Web `handleSelectSshHostSuggestion`: a picked host fills the fields; the caller connects.
pub fn pick(s: &mut App, alias: &str) {
    let ui = &mut s.ui.env.ssh;
    ui.host = TextEdit::new(alias.to_owned());
    ui.username.clear();
    ui.port.clear();
}

/// The web's SshPasswordPromptDialog.
pub fn password_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let ui = &s.ui.env.ssh;
    let Some(job) = &ui.prompt else { return };
    let spec = job.target.host_spec();
    kit::Dialog::new("ssh-password", "SSH verification required")
        .open(true)
        .alert()
        .description(format!(
            "Enter the password or verification code required by {}. S5 Code passes your response to the local SSH process and keeps it in memory to reuse for this host. S5 Code does not write it to disk.",
            job.target.display()
        ))
        .on_close(cancel)
        .child(
            col()
                .gap(8.0)
                .child(text(format!("Enter the SSH password or verification code for {spec}.")).font(theme::TEXT_SM).color(t.c.foreground).wrap())
                .child(
                    kit::Input::new(&ui.password)
                        .placeholder("Password or verification code")
                        .on_edit(|s: &mut App| &mut s.ui.env.ssh.password)
                        .on_submit(|s: &mut App, _| answer(s))
                        .build(t)
                        .single_line()
                        .secure(),
                )
                .child(text("SSH keys may replace a password, but your server can still require a verification code.").font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()),
        )
        .child(
            row()
                .gap(8.0)
                .child(kit::Button::new("Cancel").variant("outline").on_press(cancel).build(t))
                .child(kit::Button::new("Continue").disabled(ui.password.text.is_empty()).on_press(answer).build(t)),
        )
        .build(t, layers);
}

// ---------------------------------------------------------------------------------------------
// Targets and discovery

/// `parseManualDesktopSshTarget`.
fn parse_manual(host: &str, username: &str, port_field: &str) -> Result<Target, String> {
    const REQUIRED: &str = "SSH host or alias is required.";
    const BAD_PORT: &str = "SSH port must be between 1 and 65535.";
    let mut hostname = host.trim().to_owned();
    if hostname.is_empty() {
        return Err(REQUIRED.into());
    }
    let mut username = Some(username.trim().to_owned()).filter(|u| !u.is_empty());
    let mut port: Option<u64> = None;
    if let Some(at) = hostname.rfind('@').filter(|at| *at > 0) {
        let inline = hostname[..at].trim().to_owned();
        hostname = hostname[at + 1..].trim().to_owned();
        if username.is_none() && !inline.is_empty() {
            username = Some(inline);
        }
    }
    let bracketed = hostname.strip_prefix('[').and_then(|rest| rest.split_once(']')).filter(|(inner, tail)| !inner.is_empty() && (tail.is_empty() || tail.strip_prefix(':').is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))));
    if let Some((inner, tail)) = bracketed {
        if let Some(digits) = tail.strip_prefix(':') {
            port = Some(digits.parse().unwrap_or(u64::MAX));
        }
        hostname = inner.trim().to_owned();
    } else if let Some((name, digits)) = hostname.split_once(':').filter(|(_, d)| !d.contains(':') && !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())) {
        port = Some(digits.parse().unwrap_or(u64::MAX));
        hostname = name.trim().to_owned();
    }
    let raw_port = port_field.trim();
    if !raw_port.is_empty() {
        // Like `Number.parseInt`: leading digits count, none at all is not a port.
        let digits: String = raw_port.chars().take_while(char::is_ascii_digit).collect();
        port = Some(digits.parse().map_err(|_| BAD_PORT.to_owned())?);
    }
    if hostname.is_empty() {
        return Err(REQUIRED.into());
    }
    let port = match port {
        None => None,
        Some(p) => Some(u16::try_from(p).ok().filter(|p| *p > 0).ok_or_else(|| BAD_PORT.to_owned())?),
    };
    Ok(Target { alias: hostname.clone(), hostname, username, port })
}

/// `parseSshResolveOutput`: `ssh -G` prints one `key value` per line; the first of each wins.
fn parse_resolve_output(alias: &str, stdout: &str) -> Target {
    let mut values: HashMap<&str, String> = HashMap::new();
    for line in stdout.lines() {
        let mut words = line.split_whitespace();
        let Some(key) = words.next() else { continue };
        let rest: Vec<&str> = words.collect();
        if rest.is_empty() || values.contains_key(key) {
            continue;
        }
        values.insert(key, rest.join(" "));
    }
    let get = |k: &str| values.get(k).map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    Target {
        alias: alias.to_owned(),
        hostname: get("hostname").unwrap_or_else(|| alias.to_owned()),
        username: get("user"),
        port: get("port").and_then(|p| p.parse().ok()),
    }
}

/// `resolveSshTarget` plus the caller's username and port, which win over the config's.
fn resolve(target: &Target) -> Target {
    let alias = if target.alias.trim().is_empty() { target.hostname.trim() } else { target.alias.trim() };
    let bare = Target { alias: alias.to_owned(), hostname: alias.to_owned(), username: None, port: None };
    let mut resolved = run_ssh(&bare, None, &["-G"], &[], None, COMMAND_TIMEOUT).map(|out| parse_resolve_output(alias, &out)).unwrap_or(bare);
    if target.username.is_some() {
        resolved.username = target.username.clone();
    }
    if target.port.is_some() {
        resolved.port = target.port;
    }
    resolved
}

/// `filterDiscoveredSshHosts`: alias prefix matches first, then alias or hostname substrings.
fn filter_hosts(hosts: &[Host], query: &str) -> Vec<Host> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return hosts.to_vec();
    }
    let (mut prefix, mut substring) = (Vec::new(), Vec::new());
    for host in hosts {
        let alias = host.alias.to_lowercase();
        if alias.starts_with(&query) {
            prefix.push(host.clone());
        } else if alias.contains(&query) || host.hostname.to_lowercase().contains(&query) {
            substring.push(host.clone());
        }
    }
    prefix.extend(substring);
    prefix
}

/// OpenSSH readconf's line split: a keyword, one optional `=`, then arguments with quotes, the
/// escapes `\\ \" \'` and an unquoted `\ `, and `#` starting a comment only at a token's start.
/// Unbalanced quotes leave the arguments unknown.
fn parse_config_line(line: &str) -> Option<(String, Option<Vec<String>>)> {
    let line = line.trim_start();
    let end = line.find(|c: char| c.is_whitespace() || c == '=').unwrap_or(line.len());
    if end == 0 {
        return None;
    }
    let directive = line[..end].to_lowercase();
    let mut rest = line[end..].trim_start();
    if let Some(after) = rest.strip_prefix('=') {
        rest = after.trim_start();
    }
    let chars: Vec<char> = rest.chars().collect();
    let (mut args, mut token, mut quote): (Vec<String>, Option<String>, Option<char>) = (Vec::new(), None, None);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '\\' && (matches!(next, Some('"' | '\'' | '\\')) || (quote.is_none() && next == Some(' '))) {
            token.get_or_insert_with(String::new).extend(next);
            i += 2;
            continue;
        }
        let open = quote;
        match open {
            Some(q) if c == q => quote = None,
            Some(_) => token.get_or_insert_with(String::new).push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                token.get_or_insert_with(String::new);
            }
            None if c.is_whitespace() => args.extend(token.take()),
            None if c == '#' && token.is_none() => break,
            None => token.get_or_insert_with(String::new).push(c),
        }
        i += 1;
    }
    if quote.is_some() {
        return Some((directive, None));
    }
    args.extend(token);
    Some((directive, Some(args)))
}

fn has_pattern(value: &str) -> bool {
    value.contains('*') || value.contains('?') || value.starts_with('!')
}

/// `*` and `?` globbing over the whole string.
fn glob(pattern: &str, value: &str) -> bool {
    let (p, v): (Vec<char>, Vec<char>) = (pattern.chars().collect(), value.chars().collect());
    let (mut pi, mut vi, mut star, mut mark) = (0, 0, None, 0);
    while vi < v.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == v[vi]) {
            pi += 1;
            vi += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = vi;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            vi = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

fn matches_host_patterns(alias: &str, patterns: &[String]) -> bool {
    let mut matched = false;
    for pattern in patterns {
        let (negated, host) = match pattern.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, pattern.as_str()),
        };
        let matches = host == "*" || if has_pattern(host) { glob(host, alias) } else { host == alias };
        if !matches {
            continue;
        }
        if negated {
            return false;
        }
        matched = true;
    }
    matched
}

/// A `HostName` with the `Host` conditions it sits under; `None` marks an unknown (Match) block.
struct Rule {
    conditions: Vec<Option<Vec<String>>>,
    hostname: Option<String>,
}

fn resolve_configured_hostname(alias: &str, rules: &[Rule]) -> Option<String> {
    for rule in rules {
        if rule.conditions.iter().any(|c| c.as_ref().is_some_and(|p| !matches_host_patterns(alias, p))) {
            continue;
        }
        // SSH takes the first value; an uncertain earlier value blocks later fallbacks.
        let hostname = rule.hostname.as_ref().filter(|_| rule.conditions.iter().all(Option::is_some))?;
        if hostname.contains('$') || hostname.match_indices('%').any(|(i, _)| !hostname[i + 1..].starts_with('h')) {
            return None;
        }
        return Some(hostname.replace("%h", alias));
    }
    None
}

fn expand_home(input: &str, home: &Path) -> String {
    match input.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{}{rest}", home.display()),
        _ => input.to_owned(),
    }
}

fn expand_glob(pattern: &Path) -> Vec<PathBuf> {
    let name = pattern.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !name.contains('*') && !name.contains('?') {
        return if pattern.exists() { vec![pattern.to_owned()] } else { vec![] };
    }
    let Some(dir) = pattern.parent() else { return vec![] };
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| glob(&name, &entry.file_name().to_string_lossy()))
        .map(|entry| entry.path())
        .collect();
    found.sort();
    found
}

struct Collect<'a> {
    home: &'a Path,
    visited: Vec<PathBuf>,
    aliases: Vec<String>,
    rules: Vec<Rule>,
}

impl Collect<'_> {
    fn file(&mut self, path: &Path, inherited: &[Option<Vec<String>>]) {
        if self.visited.iter().any(|p| p == path) {
            // Read each file once, but never reuse its rules under a different Include context.
            self.rules.push(Rule { conditions: inherited.to_vec(), hostname: None });
            return;
        }
        if !path.is_file() {
            return;
        }
        self.visited.push(path.to_owned());
        let Ok(raw) = std::fs::read_to_string(path) else { return };
        let mut condition: Option<Vec<String>> = Some(vec!["*".into()]);
        for line in raw.lines() {
            let Some((directive, args)) = parse_config_line(line) else { continue };
            let mut conditions = inherited.to_vec();
            conditions.push(condition.clone());
            match directive.as_str() {
                "host" => {
                    for alias in args.iter().flatten() {
                        if !alias.is_empty() && !has_pattern(alias) && !self.aliases.contains(alias) {
                            self.aliases.push(alias.clone());
                        }
                    }
                    condition = args;
                }
                "match" => condition = None,
                "hostname" => {
                    let hostname = args.filter(|a| a.len() == 1).map(|mut a| a.remove(0));
                    self.rules.push(Rule { conditions, hostname });
                }
                "include" => {
                    let Some(patterns) = args else {
                        self.rules.push(Rule { conditions, hostname: None });
                        continue;
                    };
                    for pattern in patterns {
                        let expanded = expand_home(&pattern, self.home);
                        let resolved = if Path::new(&expanded).is_absolute() { PathBuf::from(&expanded) } else { self.home.join(".ssh").join(&expanded) };
                        let uncertain = pattern.contains('%')
                            || pattern.contains("${")
                            || pattern.contains('[')
                            || (pattern.starts_with('~') && !(pattern.len() == 1 || pattern.starts_with("~/")))
                            || resolved.parent().is_some_and(|dir| has_pattern(&dir.to_string_lossy()));
                        if uncertain {
                            self.rules.push(Rule { conditions: conditions.clone(), hostname: None });
                            continue;
                        }
                        for included in expand_glob(&resolved) {
                            self.file(&included, &conditions);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// `parseKnownHostsHostnames`: plain hostnames only, without hashed entries or patterns.
fn parse_known_hosts(raw: &str) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        if line.starts_with('@') {
            fields.next();
        }
        let Some(field) = fields.next().filter(|f| !f.starts_with('|')) else { continue };
        for raw_host in field.split(',') {
            let host = if let Some(inner) = raw_host.strip_prefix('[').and_then(|r| r.split_once("]:")).filter(|(h, p)| !h.is_empty() && !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())).map(|(h, _)| h) {
                inner
            } else if raw_host.matches(':').count() == 1 {
                raw_host.split_once(':').map_or(raw_host, |(h, _)| h)
            } else {
                raw_host
            };
            let host = host.trim();
            if !host.is_empty() && !has_pattern(host) && !hosts.iter().any(|h| h == host) {
                hosts.push(host.to_owned());
            }
        }
    }
    hosts
}

/// `discoverSshHosts`: `Host` aliases from the SSH config, then known hosts it does not cover.
fn discover(home: &Path) -> Vec<Host> {
    let ssh = home.join(".ssh");
    let mut collect = Collect { home, visited: Vec::new(), aliases: Vec::new(), rules: Vec::new() };
    collect.file(&ssh.join("config"), &[]);
    let mut hosts: Vec<Host> = Vec::new();
    let mut configured: Vec<String> = Vec::new();
    for alias in &collect.aliases {
        let hostname = resolve_configured_hostname(alias, &collect.rules);
        configured.extend(hostname.as_ref().map(|h| h.to_lowercase()));
        hosts.push(Host { alias: alias.clone(), hostname: hostname.unwrap_or_else(|| alias.clone()) });
    }
    let known = std::fs::read_to_string(ssh.join("known_hosts")).map(|raw| parse_known_hosts(&raw)).unwrap_or_default();
    for hostname in known {
        if hosts.iter().any(|h| h.alias == hostname) || configured.contains(&hostname.to_lowercase()) {
            continue;
        }
        hosts.push(Host { alias: hostname.clone(), hostname });
    }
    hosts.sort_by(|a, b| a.alias.to_lowercase().cmp(&b.alias.to_lowercase()).then_with(|| a.alias.cmp(&b.alias)));
    hosts
}

// ---------------------------------------------------------------------------------------------
// Remote scripts, ported from packages/ssh/src/tunnel.ts in its release-archive mode

const RUNNER_SCRIPT: &str = r#"#!/bin/sh
set -eu
T3_ARCHIVE_VERSION=@@T3_ARCHIVE_VERSION@@
if [ -z "$T3_ARCHIVE_VERSION" ]; then
  printf 'No t3 release version was provided for the remote runtime.\n' >&2
  exit 1
fi
# Self-contained release archive: no Node, npm, or compiler on the remote.
# Unpacked into the pinned-runtime layout so `t3 service install` reuses it.
T3_RELEASE_BASE_URL=@@T3_RELEASE_BASE_URL@@
T3_RUNTIME_DIR="$HOME/.s5code/runtime/versions/$T3_ARCHIVE_VERSION"
t3_runtime_ready() {
  [ -x "$T3_RUNTIME_DIR/t3" ] && [ "$(cat "$T3_RUNTIME_DIR/.install-complete" 2>/dev/null)" = "$T3_ARCHIVE_VERSION" ]
}
if ! t3_runtime_ready; then
  mkdir -p "$HOME/.s5code/runtime/versions"
  T3_LOCK="$HOME/.s5code/runtime/versions/.$T3_ARCHIVE_VERSION.install.lock"
  # uutils mkdir can report success on EEXIST, so use shell noclobber.
  # An empty lock gets a short grace period for its owner to publish a PID.
  T3_LOCK_WAITED=0
  T3_LOCK_UNOWNED=0
  while ! (set -C; printf '%s\n' "$$" > "$T3_LOCK") 2>/dev/null; do
    T3_LOCK_OWNER="$(cat "$T3_LOCK" 2>/dev/null || true)"
    if [ -n "$T3_LOCK_OWNER" ]; then
      T3_LOCK_UNOWNED=0
      if ! kill -0 "$T3_LOCK_OWNER" 2>/dev/null; then
        rm -rf "$T3_LOCK"
        continue
      fi
    else
      T3_LOCK_UNOWNED=$((T3_LOCK_UNOWNED + 1))
      if [ "$T3_LOCK_UNOWNED" -ge 5 ]; then
        rm -rf "$T3_LOCK"
        continue
      fi
    fi
    if [ "$T3_LOCK_WAITED" -ge @@T3_ARCHIVE_LOCK_WAIT_SECONDS@@ ]; then
      printf 'Another t3 %s installation has held %s for too long.\n' "$T3_ARCHIVE_VERSION" "$T3_LOCK" >&2
      exit 1
    fi
    sleep 1
    T3_LOCK_WAITED=$((T3_LOCK_WAITED + 1))
  done
  trap 'rm -rf "$T3_LOCK"' EXIT
fi
if ! t3_runtime_ready; then
  # Releases publish server archives for Linux only (release.yml `server` job).
  case "$(uname -s)" in
    Linux) T3_PLATFORM="linux" ;;
    *) printf 'S5 Code publishes SSH server archives for Linux hosts only, not %s.\n' "$(uname -s)" >&2; exit 1 ;;
  esac
  case "$(uname -m)" in
    arm64 | aarch64) T3_ARCH="arm64" ;;
    x86_64 | amd64) T3_ARCH="x64" ;;
    *) printf 'Remote host %s has no t3 release archive.\n' "$(uname -m)" >&2; exit 1 ;;
  esac
  T3_ARCHIVE="t3-$T3_ARCHIVE_VERSION-$T3_PLATFORM-$T3_ARCH.tar.gz"
  T3_STAGING="$(mktemp -d "$HOME/.s5code/runtime/versions/.staging-XXXXXX")"
  trap 'rm -rf "$T3_STAGING" "$T3_LOCK"' EXIT
  t3_fetch() {
    if command -v curl >/dev/null 2>&1; then curl -fsSL --connect-timeout 30 --max-time "$3" "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then wget -q --timeout=30 --tries=1 "$1" -O "$2"
    else printf 'Remote host needs curl or wget to download %s.\n' "$T3_ARCHIVE" >&2; exit 1
    fi
  }
  t3_fetch "$T3_RELEASE_BASE_URL/v$T3_ARCHIVE_VERSION/SHA256SUMS" "$T3_STAGING/SHA256SUMS" @@T3_ARCHIVE_CHECKSUMS_SECONDS@@
  t3_fetch "$T3_RELEASE_BASE_URL/v$T3_ARCHIVE_VERSION/$T3_ARCHIVE" "$T3_STAGING/$T3_ARCHIVE" @@T3_ARCHIVE_DOWNLOAD_SECONDS@@
  T3_EXPECTED="$(grep " \*\{0,1\}$T3_ARCHIVE$" "$T3_STAGING/SHA256SUMS" | cut -d' ' -f1)"
  if command -v sha256sum >/dev/null 2>&1; then
    T3_ACTUAL="$(sha256sum "$T3_STAGING/$T3_ARCHIVE" | cut -d' ' -f1)"
  else
    T3_ACTUAL="$(shasum -a 256 "$T3_STAGING/$T3_ARCHIVE" | cut -d' ' -f1)"
  fi
  if [ -z "$T3_EXPECTED" ] || [ "$T3_ACTUAL" != "$T3_EXPECTED" ]; then
    printf 'Checksum mismatch for %s.\n' "$T3_ARCHIVE" >&2; exit 1
  fi
  tar -xzf "$T3_STAGING/$T3_ARCHIVE" -C "$T3_STAGING" --strip-components=1
  rm -f "$T3_STAGING/$T3_ARCHIVE" "$T3_STAGING/SHA256SUMS"
  # Prove the binary runs here (libc, arch) before marking it ready, or every
  # later launch would exec a broken install instead of retrying.
  if ! "$T3_STAGING/t3" --version >/dev/null 2>&1; then
    printf 'The t3 %s executable does not run on this host.\n' "$T3_ARCHIVE_VERSION" >&2; exit 1
  fi
  printf '%s\n' "$T3_ARCHIVE_VERSION" > "$T3_STAGING/.install-complete"
  rm -rf "$T3_RUNTIME_DIR"
  mv "$T3_STAGING" "$T3_RUNTIME_DIR"
fi
if [ -n "${T3_LOCK:-}" ]; then
  rm -rf "$T3_LOCK"
  trap - EXIT
fi
exec "$T3_RUNTIME_DIR/t3" "$@"
"#;

const LAUNCH_SCRIPT: &str = r#"set -eu
STATE_KEY="$1"
STATE_DIR="$HOME/.s5code/ssh-launch/$STATE_KEY"
DEFAULT_SERVER_HOME="$HOME/.s5code"
DEFAULT_RUNTIME_FILE="$DEFAULT_SERVER_HOME/userdata/server-runtime.json"
PORT_FILE="$STATE_DIR/port"
PID_FILE="$STATE_DIR/pid"
MANAGED_FILE="$STATE_DIR/managed"
LOG_FILE="$STATE_DIR/server.log"
RUNNER_FILE="$STATE_DIR/run-t3.sh"
RUNNER_NEXT="$STATE_DIR/run-t3.next.$$"
mkdir -p "$STATE_DIR"
cleanup_runner_next() {
  rm -f "$RUNNER_NEXT"
}
trap cleanup_runner_next EXIT
cat >"$RUNNER_NEXT" <<'SH'
@@T3_RUNNER_SCRIPT@@
SH
RUNNER_CHANGED=0
if [ ! -f "$RUNNER_FILE" ] || ! cmp -s "$RUNNER_NEXT" "$RUNNER_FILE"; then
  RUNNER_CHANGED=1
fi
mv "$RUNNER_NEXT" "$RUNNER_FILE"
chmod 700 "$RUNNER_FILE"
# The archive ships the helpers below inside the executable; the remote
# needs no Node at all. Resolving the runner once here also downloads the
# archive before the port and readiness probes rely on it.
"$RUNNER_FILE" --version >/dev/null
pick_port() {
  "$RUNNER_FILE" __ssh-helper pick-port "$PORT_FILE" "@@T3_DEFAULT_REMOTE_PORT@@" "@@T3_REMOTE_PORT_SCAN_WINDOW@@"
}
wait_ready() {
  "$RUNNER_FILE" __ssh-helper wait-ready "$REMOTE_PORT" "$1" "@@T3_READY_PROBE_TIMEOUT_MS@@"
}
wait_for_pid_exit() {
  PID_TO_WAIT="$1"
  WAIT_COUNT=0
  while kill -0 "$PID_TO_WAIT" 2>/dev/null && [ "$WAIT_COUNT" -lt 20 ]; do
    WAIT_COUNT=$((WAIT_COUNT + 1))
    sleep 0.1
  done
}
# kill -0 also succeeds for a server that exited but was not reaped yet, so
# a zombie does not count as running. Without ps to tell, trust kill -0.
server_running() {
  [ -n "$1" ] && kill -0 "$1" 2>/dev/null || return 1
  case "$(ps -o stat= -p "$1" 2>/dev/null || true)" in
    *Z*) return 1 ;;
  esac
}
resolve_default_runtime_port() {
  "$RUNNER_FILE" __ssh-helper runtime-port "$DEFAULT_RUNTIME_FILE"
}
REMOTE_PID="$(cat "$PID_FILE" 2>/dev/null || true)"
REMOTE_PORT="$(cat "$PORT_FILE" 2>/dev/null || true)"
REMOTE_MANAGED="$(cat "$MANAGED_FILE" 2>/dev/null || true)"
DEFAULT_RUNTIME_INFO="$(resolve_default_runtime_port 2>/dev/null || true)"
DEFAULT_RUNTIME_PID=""
DEFAULT_REMOTE_PORT=""
ADOPTED_PID=""
if [ -n "$DEFAULT_RUNTIME_INFO" ]; then
  DEFAULT_RUNTIME_PID="${DEFAULT_RUNTIME_INFO%% *}"
  DEFAULT_REMOTE_PORT="${DEFAULT_RUNTIME_INFO#* }"
fi
# The managed server runs on the default home, so it writes the default
# runtime file too. When that file names the PID this launcher recorded as
# managed, it describes our own server and its real port, not one to adopt.
if [ -n "$DEFAULT_REMOTE_PORT" ] && [ "$REMOTE_MANAGED" = "managed" ] && [ "$DEFAULT_RUNTIME_PID" = "$REMOTE_PID" ]; then
  REMOTE_PORT="$DEFAULT_REMOTE_PORT"
  printf '%s\n' "$REMOTE_PORT" >"$PORT_FILE"
elif [ -n "$DEFAULT_REMOTE_PORT" ]; then
  REMOTE_PORT="$DEFAULT_REMOTE_PORT"
  if wait_ready "@@T3_REUSE_READY_TIMEOUT_MS@@"; then
    ADOPTED_PID="$DEFAULT_RUNTIME_PID"
    if [ "$REMOTE_MANAGED" = "managed" ]; then
      if [ -n "$REMOTE_PID" ] && kill -0 "$REMOTE_PID" 2>/dev/null; then
        kill "$REMOTE_PID" 2>/dev/null || true
        wait_for_pid_exit "$REMOTE_PID"
      fi
      REMOTE_PID=""
      REMOTE_PORT="$DEFAULT_REMOTE_PORT"
      REMOTE_MANAGED="external"
      rm -f "$PID_FILE"
      printf '%s\n' "$REMOTE_PORT" >"$PORT_FILE"
      printf 'external\n' >"$MANAGED_FILE"
    else
      printf '%s\n' "$REMOTE_PORT" >"$PORT_FILE"
      printf 'external\n' >"$MANAGED_FILE"
      REMOTE_PID=""
      REMOTE_MANAGED="external"
    fi
  else
    REMOTE_PID="$(cat "$PID_FILE" 2>/dev/null || true)"
    REMOTE_PORT="$(cat "$PORT_FILE" 2>/dev/null || true)"
    REMOTE_MANAGED="$(cat "$MANAGED_FILE" 2>/dev/null || true)"
  fi
fi
if [ "$REMOTE_MANAGED" = "external" ]; then
  # A server adopted above has just answered. While it is still alive, probing
  # it again could catch a momentary stall and launch a second server beside it.
  if server_running "$ADOPTED_PID"; then
    :
  elif [ -z "$REMOTE_PORT" ] || ! wait_ready "@@T3_REUSE_READY_TIMEOUT_MS@@"; then
    REMOTE_PID=""
    REMOTE_PORT=""
    REMOTE_MANAGED=""
  fi
elif [ -n "$REMOTE_PID" ] && [ -n "$REMOTE_PORT" ] && server_running "$REMOTE_PID"; then
  # Reconnect owns the local tunnel, not the lifetime of an already running
  # server. A changed runner is the update path and still replaces it.
  if [ "$RUNNER_CHANGED" -eq 1 ]; then
    kill "$REMOTE_PID" 2>/dev/null || true
    wait_for_pid_exit "$REMOTE_PID"
    REMOTE_PID=""
    REMOTE_PORT=""
    REMOTE_MANAGED=""
  elif ! wait_ready "@@T3_READY_TIMEOUT_MS@@"; then
    printf 'Remote T3 server is still running but did not become ready. Retry the connection; the server was left running.\n' >&2
    exit 1
  fi
else
  REMOTE_PID=""
  REMOTE_PORT=""
  REMOTE_MANAGED=""
fi
if [ -z "$REMOTE_PORT" ]; then
  REMOTE_PORT="$(pick_port)" || true
  if [ -z "$REMOTE_PORT" ]; then
    printf 'Failed to find an available port on the remote host.\n' >&2
    exit 1
  fi
  nohup env T3CODE_NO_BROWSER=1 "$RUNNER_FILE" serve --host 127.0.0.1 --port "$REMOTE_PORT" --base-dir "$DEFAULT_SERVER_HOME" >>"$LOG_FILE" 2>&1 < /dev/null &
  REMOTE_PID="$!"
  printf '%s\n' "$REMOTE_PID" >"$PID_FILE"
  printf '%s\n' "$REMOTE_PORT" >"$PORT_FILE"
  printf 'managed\n' >"$MANAGED_FILE"
  if ! wait_ready "@@T3_READY_TIMEOUT_MS@@"; then
    printf 'Remote T3 server did not become ready on 127.0.0.1:%s.\n' "$REMOTE_PORT" >&2
    if [ -s "$LOG_FILE" ]; then
      tail -n 80 "$LOG_FILE" >&2 2>/dev/null || true
    else
      printf 'It wrote nothing to %s, so it exited before producing any output.\n' "$LOG_FILE" >&2
    fi
    kill "$REMOTE_PID" 2>/dev/null || true
    wait_for_pid_exit "$REMOTE_PID"
    rm -f "$PID_FILE" "$PORT_FILE" "$MANAGED_FILE"
    exit 1
  fi
fi
printf '{"remotePort":%s,"serverKind":"%s"}\n' "$REMOTE_PORT" "${REMOTE_MANAGED:-managed}"
"#;

const PAIRING_SCRIPT: &str = r#"set -eu
STATE_DIR="$HOME/.s5code/ssh-launch/@@T3_STATE_KEY@@"
DEFAULT_SERVER_HOME="$HOME/.s5code"
RUNNER_FILE="$STATE_DIR/run-t3.sh"
mkdir -p "$STATE_DIR"
cat >"$RUNNER_FILE" <<'SH'
@@T3_RUNNER_SCRIPT@@
SH
chmod 700 "$RUNNER_FILE"
PAIRING_BASE_DIR="$DEFAULT_SERVER_HOME"
"$RUNNER_FILE" auth pairing create --base-dir "$PAIRING_BASE_DIR" --json
"#;

const STOP_SCRIPT: &str = r#"set -eu
STATE_DIR="$HOME/.s5code/ssh-launch/@@T3_STATE_KEY@@"
PID_FILE="$STATE_DIR/pid"
PORT_FILE="$STATE_DIR/port"
MANAGED_FILE="$STATE_DIR/managed"
REMOTE_MANAGED="$(cat "$MANAGED_FILE" 2>/dev/null || true)"
REMOTE_PID="$(cat "$PID_FILE" 2>/dev/null || true)"
if [ "$REMOTE_MANAGED" != "external" ] && [ -n "$REMOTE_PID" ] && kill -0 "$REMOTE_PID" 2>/dev/null; then
  kill "$REMOTE_PID" 2>/dev/null || true
  WAIT_COUNT=0
  while kill -0 "$REMOTE_PID" 2>/dev/null && [ "$WAIT_COUNT" -lt 20 ]; do
    WAIT_COUNT=$((WAIT_COUNT + 1))
    sleep 0.1
  done
  if kill -0 "$REMOTE_PID" 2>/dev/null; then
    printf 'Remote T3 server with PID %s did not stop within 2 seconds. Its ownership files were kept.\n' "$REMOTE_PID" >&2
    exit 1
  fi
fi
rm -f "$PID_FILE" "$PORT_FILE" "$MANAGED_FILE"
printf '{"stopped":true}\n'
"#;

const ASKPASS_SCRIPT: &str = r#"#!/bin/sh
# Invoked by ssh via SSH_ASKPASS when S5 Code re-runs ssh with a password from
# its in-app prompt. Without T3_SSH_AUTH_SECRET that's a caller bug: fail loudly.
if [ "${T3_SSH_AUTH_SECRET+x}" = "x" ]; then
  printf "%s\n" "$T3_SSH_AUTH_SECRET"
  exit 0
fi
printf 'T3 Code ssh-askpass invoked without T3_SSH_AUTH_SECRET.\n' >&2
exit 1
"#;

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn fill(template: &str, values: &[(&str, &str)]) -> String {
    values.iter().fold(template.to_owned(), |text, (token, value)| text.replace(&format!("@@{token}@@"), value))
}

/// `EXACT_ARCHIVE_VERSION`: the version names a directory the runner removes and recreates.
fn exact_version(version: &str) -> bool {
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    numbers.len() == 3
        && numbers.iter().all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && (n.len() == 1 || !n.starts_with('0')))
        && pre.is_none_or(|pre| pre.split('.').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')))
}

fn release_base_url() -> String {
    let base = std::env::var("T3CODE_RELEASE_BASE_URL").ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| RELEASE_BASE_URL.to_owned());
    base.trim().trim_end_matches('/').to_owned()
}

fn runner_script(version: &str, release_base: &str) -> Result<String, String> {
    if !exact_version(version) {
        return Err(format!("'{version}' is not an exact t3 version and cannot name a runtime directory."));
    }
    let (lock, download, checksums) = (ARCHIVE_LOCK_WAIT_SECONDS.to_string(), ARCHIVE_DOWNLOAD_SECONDS.to_string(), ARCHIVE_CHECKSUMS_SECONDS.to_string());
    let script = fill(
        RUNNER_SCRIPT,
        &[
            ("T3_ARCHIVE_VERSION", &shell_quote(version)),
            ("T3_RELEASE_BASE_URL", &shell_quote(release_base)),
            ("T3_ARCHIVE_LOCK_WAIT_SECONDS", &lock),
            ("T3_ARCHIVE_DOWNLOAD_SECONDS", &download),
            ("T3_ARCHIVE_CHECKSUMS_SECONDS", &checksums),
        ],
    );
    Ok(script.trim_end_matches('\n').to_owned())
}

fn launch_script(runner: &str) -> String {
    fill(
        LAUNCH_SCRIPT,
        &[
            ("T3_RUNNER_SCRIPT", runner),
            ("T3_DEFAULT_REMOTE_PORT", &DEFAULT_REMOTE_PORT.to_string()),
            ("T3_REMOTE_PORT_SCAN_WINDOW", &REMOTE_PORT_SCAN_WINDOW.to_string()),
            ("T3_READY_TIMEOUT_MS", &REMOTE_READY_TIMEOUT_MS.to_string()),
            ("T3_REUSE_READY_TIMEOUT_MS", &REMOTE_REUSE_READY_TIMEOUT_MS.to_string()),
            ("T3_READY_PROBE_TIMEOUT_MS", &READY_PROBE_TIMEOUT_MS.to_string()),
        ],
    )
}

fn pairing_script(state_key: &str, runner: &str) -> String {
    fill(PAIRING_SCRIPT, &[("T3_STATE_KEY", state_key), ("T3_RUNNER_SCRIPT", runner)])
}

fn stop_script(state_key: &str) -> String {
    fill(STOP_SCRIPT, &[("T3_STATE_KEY", state_key)])
}

/// The first JSON object in a command's output, past any login banner.
fn json_object(stdout: &str) -> Option<Value> {
    stdout
        .match_indices('{')
        .find_map(|(i, _)| serde_json::Deserializer::from_str(&stdout[i..]).into_iter::<Value>().next()?.ok().filter(Value::is_object))
}

fn parse_launch(stdout: &str) -> Result<u16, String> {
    json_object(stdout)
        .and_then(|v| v["remotePort"].as_u64())
        .and_then(|p| u16::try_from(p).ok())
        .ok_or_else(|| format!("The remote host returned no server port: {}", stdout.trim()))
}

fn parse_credential(stdout: &str) -> Result<String, String> {
    json_object(stdout).and_then(|v| v["credential"].as_str().map(str::to_owned)).ok_or_else(|| "The remote host did not issue a pairing code.".to_owned())
}

/// The release this app's own server runs, which is the one to run on the host.
fn bundled_version() -> Option<String> {
    let out = Command::new(crate::host::bundled_binary()?).arg("--version").stdin(Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace().last().map(|v| v.trim_start_matches('v').to_owned())
}

// ---------------------------------------------------------------------------------------------
// ssh processes

fn is_auth_failure(message: &str) -> bool {
    let message = message.to_lowercase();
    let denied = message.match_indices("permission denied (").any(|(i, m)| {
        let rest = &message[i + m.len()..];
        ["publickey", "password", "keyboard-interactive", "hostbased", "gssapi-with-mic"].iter().any(|method| rest.starts_with(method)) && rest.contains(')')
    });
    denied || message.contains("authentication failed") || message.contains("too many authentication failures")
}

/// A local agent signing failure cannot be remedied by the remote account password.
fn is_agent_signing_failure(message: &str) -> bool {
    message.split("sign_and_send_pubkey: signing failed").skip(1).any(|rest| {
        rest.starts_with(": agent refused operation") || rest.starts_with(": communication with agent failed") || (rest.starts_with(" for ") && rest.lines().next().is_some_and(|l| l.contains(" from agent:")))
    })
}

static SECRETS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

fn lock<V>(mutex: &Mutex<V>) -> MutexGuard<'_, V> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn secret_for(key: &str) -> Option<String> {
    lock(&SECRETS).iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

fn remember_secret(key: &str, secret: Option<&str>) {
    let mut secrets = lock(&SECRETS);
    secrets.retain(|(k, _)| k != key);
    if let Some(secret) = secret {
        secrets.push((key.to_owned(), secret.to_owned()));
    }
}

fn askpass() -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    let path = crate::host::t3_home().join("bin").join("ssh-askpass.sh");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(ASKPASS_SCRIPT) {
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
        std::fs::write(&path, ASKPASS_SCRIPT).map_err(|e| e.to_string())?;
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    Ok(path)
}

/// `baseSshArgs` plus `buildSshChildEnvironment`: batch mode until there is a password to give.
fn ssh(target: &Target, secret: Option<&str>) -> Result<Command, String> {
    let mut command = Command::new("ssh");
    // The login shell's agent socket and PATH, which an app opened from Finder lacks.
    command.envs(crate::desktop::shell_path::server_env().0.iter().map(|(k, v)| (k, v)));
    command.args(["-o", if secret.is_some() { "BatchMode=no" } else { "BatchMode=yes" }, "-o", "ConnectTimeout=10"]);
    if let Some(port) = target.port {
        command.arg("-p").arg(port.to_string());
    }
    if let Some(secret) = secret {
        command.env("SSH_ASKPASS", askpass().map_err(|e| format!("Failed to prepare SSH authentication helpers: {e}"))?);
        command.env("SSH_ASKPASS_REQUIRE", "force").env("T3_SSH_AUTH_SECRET", secret);
        if std::env::var_os("DISPLAY").is_none() {
            command.env("DISPLAY", "t3code");
        }
    }
    Ok(command)
}

fn read_all(pipe: Option<impl Read + Send + 'static>) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// `runSshCommand`: stdout on success, else stderr (or stdout) as the message.
fn run_ssh(target: &Target, secret: Option<&str>, pre_host: &[&str], remote: &[&str], input: Option<&str>, timeout: Duration) -> Result<String, String> {
    let host = target.host_spec();
    if host.is_empty() {
        return Err("SSH target is missing its alias/hostname.".into());
    }
    let mut command = ssh(target, secret)?;
    command.args(["-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3"]).args(pre_host).arg("--").arg(&host).args(remote);
    command.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| format!("Failed to spawn SSH command for {host}: {e}"))?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        let input = input.to_owned();
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let (stdout, stderr) = (read_all(child.stdout.take()), read_all(child.stderr.take()));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("SSH command for {host} timed out after {} seconds.", timeout.as_secs()));
            }
            Err(e) => return Err(format!("Failed to run SSH command for {host}: {e}")),
        }
    };
    let (stdout, stderr) = (stdout.join().unwrap_or_default(), stderr.join().unwrap_or_default());
    if status.success() {
        return Ok(stdout);
    }
    let message = [stderr.trim(), stdout.trim()].into_iter().find(|m| !m.is_empty()).map(str::to_owned);
    Err(message.unwrap_or_else(|| format!("SSH command failed for {host} (exit {}).", status.code().unwrap_or(-1))))
}

struct Tunnel {
    child: Child,
    local: u16,
    target: Target,
}

static TUNNELS: Mutex<Vec<(String, Tunnel)>> = Mutex::new(Vec::new());

fn base_url(local: u16) -> String {
    format!("http://127.0.0.1:{local}")
}

fn ready(base: &str, timeout: Duration) -> bool {
    ureq::AgentBuilder::new().timeout(timeout).build().get(&format!("{base}/.well-known/t3/environment")).call().is_ok()
}

fn close(key: &str) {
    let removed: Vec<Tunnel> = {
        let mut tunnels = lock(&TUNNELS);
        let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut *tunnels).into_iter().partition(|(k, _)| k == key);
        *tunnels = kept;
        gone.into_iter().map(|(_, t)| t).collect()
    };
    for mut tunnel in removed {
        let _ = tunnel.child.kill();
        let _ = tunnel.child.wait();
    }
}

/// An existing forward that still answers; a stale one is closed.
fn reuse(key: &str) -> Option<u16> {
    let local = lock(&TUNNELS).iter_mut().find(|(k, _)| k == key).and_then(|(_, t)| matches!(t.child.try_wait(), Ok(None)).then_some(t.local));
    match local {
        Some(local) if ready(&base_url(local), TUNNEL_REUSE_TIMEOUT) => Some(local),
        _ => {
            close(key);
            None
        }
    }
}

/// The saved environment's port when it is free, so its address survives a restart.
fn local_port(preferred: Option<u16>) -> Result<u16, String> {
    if let Some(port) = preferred.filter(|p| *p > 0) {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("Could not reserve a local port: {e}"))?;
    listener.local_addr().map(|a| a.port()).map_err(|e| e.to_string())
}

/// `startSshTunnel`: `ssh -N -L`, ready once the server answers through it.
fn start_tunnel(target: &Target, secret: Option<&str>, local: u16, remote: u16, waker: Option<Waker>) -> Result<Child, String> {
    let host = target.host_spec();
    let forward = format!("{local}:127.0.0.1:{remote}");
    let mut command = ssh(target, secret)?;
    command.args(["-o", "ExitOnForwardFailure=yes", "-o", "ControlMaster=no", "-o", "ControlPath=none", "-o", "ControlPersist=no"]);
    command.args(["-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3", "-n", "-N", "-L", forward.as_str(), "--", host.as_str()]);
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| format!("Failed to start the SSH tunnel to {host}: {e}"))?;
    let tail = Arc::new((Mutex::new(String::new()), AtomicBool::new(false)));
    if let Some(mut stderr) = child.stderr.take() {
        let tail = tail.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                let mut text = lock(&tail.0);
                text.push_str(&String::from_utf8_lossy(&buffer[..n]));
                if text.len() > STDERR_TAIL {
                    let cut = text.len() - STDERR_TAIL;
                    let cut = (cut..text.len()).find(|i| text.is_char_boundary(*i)).unwrap_or(text.len());
                    text.drain(..cut);
                }
            }
            tail.1.store(true, Ordering::Relaxed);
            EXITED.store(true, Ordering::Relaxed);
            if let Some(w) = waker {
                w();
            }
        });
    }
    let base = base_url(local);
    let deadline = Instant::now() + TUNNEL_READY_TIMEOUT;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let settle = Instant::now() + Duration::from_secs(1);
            while !tail.1.load(Ordering::Relaxed) && Instant::now() < settle {
                std::thread::sleep(Duration::from_millis(20));
            }
            let stderr = lock(&tail.0).trim().to_owned();
            return Err(if stderr.is_empty() { format!("The SSH tunnel to {host} exited ({status}).") } else { stderr });
        }
        if ready(&base, Duration::from_millis(u64::from(READY_PROBE_TIMEOUT_MS))) {
            return Ok(child);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("Timed out waiting {}ms for backend readiness at {base}/.", TUNNEL_READY_TIMEOUT.as_millis()));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

struct Ensured {
    target: Target,
    base: String,
    credential: Option<String>,
}

/// `ensureEnvironment`: the remote server running, a forward to it, and a pairing code if asked.
fn ensure(job: &Job, waker: Option<Waker>) -> Result<Ensured, String> {
    let target = resolve(&job.target);
    let key = target.key();
    if let Some(secret) = &job.secret {
        remember_secret(&key, Some(secret));
    }
    let secret = secret_for(&key);
    let attempt = || -> Result<Ensured, String> {
        let version = job.version.clone().or_else(bundled_version).ok_or("S5 Code could not tell which t3 release to run on the SSH host.")?;
        let runner = runner_script(&version, &release_base_url())?;
        let local = match reuse(&key) {
            Some(local) => local,
            None => {
                let out = run_ssh(&target, secret.as_deref(), &[], &["sh", "-l", "-s", "--", &target.state_key()], Some(launch_script(&runner).as_str()), LAUNCH_TIMEOUT)?;
                let remote = parse_launch(&out)?;
                let local = local_port(job.preferred)?;
                let child = start_tunnel(&target, secret.as_deref(), local, remote, waker)?;
                close(&key);
                lock(&TUNNELS).push((key.clone(), Tunnel { child, local, target: target.clone() }));
                local
            }
        };
        let credential = match job.kind {
            Kind::Add => Some(issue_credential(&target, secret.as_deref(), &runner)?),
            Kind::Reconnect(_) => None,
        };
        Ok(Ensured { target: target.clone(), base: base_url(local), credential })
    };
    attempt().inspect_err(|error| {
        if is_auth_failure(error) {
            remember_secret(&key, None);
        }
    })
}

/// `auth pairing create` on the SSH host, through the runner.
fn issue_credential(target: &Target, secret: Option<&str>, runner: &str) -> Result<String, String> {
    parse_credential(&run_ssh(target, secret, &[], &["sh", "-s"], Some(pairing_script(&target.state_key(), runner).as_str()), LAUNCH_TIMEOUT)?)
}

/// contracts' `AuthStandardClientScopes`.
const STANDARD_SCOPES: &[&str] = &[
    "orchestration:read",
    "orchestration:operate",
    "settings:write",
    "providers:manage",
    "environment:maintain",
    "preview:operate",
    "diagnostics:read",
    "terminal:read",
    "terminal:operate",
    "source-control:write",
    "filesystem:read",
    "filesystem:write",
    "relay:read",
    "access:read",
    "access:write",
    "relay:write",
];

/// Web's `prepareDesktopSshEnvironment`: a saved bearer stays while its session is authenticated
/// and holds every standard scope. Stored grants never widen, so one missing a newer scope re-pairs.
fn session_usable(session: &Value) -> bool {
    let granted = session["permissions"].as_array().or_else(|| session["scopes"].as_array());
    session["authenticated"] == true && granted.is_some_and(|granted| STANDARD_SCOPES.iter().all(|scope| granted.iter().any(|g| g == scope)))
}

/// `GET /api/auth/session`; a refused bearer is a session that no longer exists.
fn fetch_session(base: &str, bearer: &str) -> Result<Value, String> {
    let agent = ureq::AgentBuilder::new().timeout(COMMAND_TIMEOUT).build();
    match agent.get(&format!("{base}/api/auth/session")).set("authorization", &format!("Bearer {bearer}")).call() {
        Ok(response) => response.into_json().map_err(|e| e.to_string()),
        Err(ureq::Error::Status(401 | 403, _)) => Ok(serde_json::json!({ "authenticated": false })),
        Err(e) => Err(format!("Could not check the SSH environment's session: {e}")),
    }
}

/// A fresh bearer when the saved one no longer works, through a new pairing code.
fn renew_bearer(job: &Job, target: &Target, base: &str) -> Result<Option<String>, String> {
    if let Some(bearer) = &job.bearer
        && session_usable(&fetch_session(base, bearer)?)
    {
        return Ok(None);
    }
    let version = job.version.clone().or_else(bundled_version).ok_or("S5 Code could not tell which t3 release to run on the SSH host.")?;
    let runner = runner_script(&version, &release_base_url())?;
    let credential = issue_credential(target, secret_for(&target.key()).as_deref(), &runner)?;
    s5_net::exchange_bootstrap(base, &credential).map(Some).map_err(|e| format!("The pairing code was refused: {e}"))
}

fn run(job: &Job, waker: Option<Waker>) -> Result<Done, String> {
    let Ensured { target, base, credential } = ensure(job, waker)?;
    let descriptor = s5_net::fetch_descriptor(&base)?;
    let id = descriptor["environmentId"].as_str().unwrap_or_default().to_owned();
    let label = descriptor["label"].as_str().filter(|l| !l.is_empty()).unwrap_or(&target.alias).to_owned();
    match &job.kind {
        Kind::Add => {
            let credential = credential.unwrap_or_default();
            let bearer = s5_net::exchange_bootstrap(&base, &credential).map_err(|e| format!("The pairing code was refused: {e}"))?;
            Ok(Done::Added(target, Saved { id, label, base, bearer }))
        }
        Kind::Reconnect(saved) if *saved != id => Err(format!("That host reaches {label}, a different machine. Add it as its own environment instead.")),
        Kind::Reconnect(saved) => Ok(Done::Reconnected { id: saved.clone(), bearer: renew_bearer(job, &target, &base)?, base }),
    }
}

// ---------------------------------------------------------------------------------------------
// Remote editors (Electron's probeRemoteEditors and contracts' buildRemoteOpenUrl)

struct RemoteEditor {
    id: &'static str,
    commands: &'static [&'static str],
    scheme: &'static str,
    jetbrains: Option<&'static str>,
}

const fn jetbrains(id: &'static str, commands: &'static [&'static str], code: &'static str) -> RemoteEditor {
    RemoteEditor { id, commands, scheme: "jetbrains", jetbrains: Some(code) }
}

const REMOTE_EDITORS: &[RemoteEditor] = &[
    RemoteEditor { id: "cursor", commands: &["cursor"], scheme: "cursor", jetbrains: None },
    RemoteEditor { id: "vscode", commands: &["code"], scheme: "vscode", jetbrains: None },
    RemoteEditor { id: "vscode-insiders", commands: &["code-insiders"], scheme: "vscode-insiders", jetbrains: None },
    RemoteEditor { id: "vscodium", commands: &["codium"], scheme: "vscodium", jetbrains: None },
    RemoteEditor { id: "zed", commands: &["zed", "zeditor"], scheme: "zed", jetbrains: None },
    jetbrains("idea", &["idea"], "IU"),
    jetbrains("aqua", &["aqua"], "QA"),
    jetbrains("clion", &["clion"], "CL"),
    jetbrains("datagrip", &["datagrip"], "DB"),
    jetbrains("dataspell", &["dataspell"], "DS"),
    jetbrains("goland", &["goland"], "GO"),
    jetbrains("phpstorm", &["phpstorm"], "PS"),
    jetbrains("pycharm", &["pycharm"], "PY"),
    jetbrains("rider", &["rider"], "RD"),
    jetbrains("rubymine", &["rubymine"], "RM"),
    jetbrains("rustrover", &["rustrover"], "RR"),
    jetbrains("webstorm", &["webstorm"], "WS"),
];

/// Editors on this machine that can open a folder on an SSH host.
pub fn probe_remote_editors(path_env: &str) -> Vec<&'static str> {
    use std::os::unix::fs::PermissionsExt;
    let executable = |p: PathBuf| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    let dirs: Vec<&str> = path_env.split(':').filter(|d| !d.is_empty()).collect();
    REMOTE_EDITORS.iter().filter(|e| e.commands.iter().any(|c| dirs.iter().any(|d| executable(Path::new(d).join(c))))).map(|e| e.id).collect()
}

/// `remoteOpen.ts`'s SSH host for an environment: its alias, split at a `user@`.
pub fn remote_host(s: &App, id: &str) -> Option<(String, Option<String>)> {
    let target = &s.ui.env.ssh.targets.iter().find(|(k, _)| k == id)?.1;
    let (inline, host) = match target.alias.rsplit_once('@') {
        Some((user, host)) => (Some(user.to_owned()), host.to_owned()),
        None => (None, target.alias.clone()),
    };
    Some((host, target.username.clone().filter(|u| !u.trim().is_empty()).or(inline)))
}

fn editor_label(id: &str) -> &'static str {
    match id {
        "cursor" => "Cursor",
        "vscode" => "VS Code",
        "vscode-insiders" => "VS Code Insiders",
        "vscodium" => "VSCodium",
        "zed" => "Zed",
        "idea" => "IntelliJ IDEA",
        "aqua" => "Aqua",
        "clion" => "CLion",
        "datagrip" => "DataGrip",
        "dataspell" => "DataSpell",
        "goland" => "GoLand",
        "phpstorm" => "PhpStorm",
        "pycharm" => "PyCharm",
        "rider" => "Rider",
        "rubymine" => "RubyMine",
        "rustrover" => "RustRover",
        "webstorm" => "WebStorm",
        _ => "Editor",
    }
}

/// Chat's Open in editor on an SSH environment: this machine's editors that open the host over
/// SSH, as web's OpenInPicker in remote mode. None when the active environment is not SSH.
pub fn editors(s: &App) -> Option<Vec<(String, String)>> {
    static PROBED: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    remote_host(s, &s.ui.env.active)?;
    // Rendering asks every frame, so probe once, after the login shell's PATH is known.
    let ids = match crate::desktop::shell_path::get() {
        Some(path) => PROBED.get_or_init(|| probe_remote_editors(path)).clone(),
        None => Vec::new(),
    };
    Some(ids.into_iter().map(|id| (id.to_owned(), editor_label(id).to_owned())).collect())
}

/// Opens `path` on the active SSH environment's host in `editor`; false off SSH.
pub fn open_in_editor(s: &mut App, editor: &str, path: &str) -> bool {
    let Some((host, user)) = remote_host(s, &s.ui.env.active) else { return false };
    let Some(url) = remote_open_url(editor, &host, user.as_deref(), path, false) else { return true };
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    if let Err(e) = Command::new(opener).arg(&url).stdin(Stdio::null()).spawn() {
        crate::shell::palette::toast(s, "Could not open the editor.", Some(&e.to_string()));
    }
    true
}

/// `encodeURIComponent`.
fn encode_component(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// `URLSearchParams` form encoding.
fn encode_form(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => (b as char).to_string(),
            b' ' => "+".to_owned(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// `buildRemoteOpenUrl`.
pub fn remote_open_url(editor: &str, host: &str, username: Option<&str>, path: &str, is_file: bool) -> Option<String> {
    let editor = REMOTE_EDITORS.iter().find(|e| e.id == editor)?;
    let posix = path.replace('\\', "/");
    if let Some(code) = editor.jetbrains {
        let mut params = vec![("h", host.to_owned()), ("launchIde", "true".to_owned()), ("ideHint", code.to_owned()), ("projectHint", posix)];
        params.extend(username.map(|u| ("u", u.to_owned())));
        let query: Vec<String> = params.iter().map(|(k, v)| format!("{k}={}", encode_form(v))).collect();
        return Some(format!("{}://gateway/ssh/environment?{}", editor.scheme, query.join("&")));
    }
    let rooted = if posix.starts_with('/') { posix } else { format!("/{posix}") };
    let encode_path = |p: &str| p.split('/').map(encode_component).collect::<Vec<_>>().join("/");
    if editor.id == "zed" {
        let drive = path.len() >= 3 && path.as_bytes()[0].eq_ignore_ascii_case(&b'c') && path.as_bytes()[1] == b':' && matches!(path.as_bytes()[2], b'/' | b'\\');
        let zed_path = if drive { &rooted[3..] } else { &rooted[..] };
        let destination = match username {
            Some(user) => format!("{}@{}", encode_component(user), encode_component(host)),
            None => encode_component(host),
        };
        return Some(format!("{}://ssh/{destination}{}", editor.scheme, encode_path(zed_path)));
    }
    let destination = match username {
        Some(user) => format!("{user}@{host}"),
        None => host.to_owned(),
    };
    Some(format!("{}://vscode-remote/ssh-remote+{}{}{}", editor.scheme, encode_component(&destination), encode_path(rooted.as_str()), if is_file { ":1:1" } else { "" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(alias: &str, hostname: &str, username: Option<&str>, port: Option<u16>) -> Target {
        Target { alias: alias.into(), hostname: hostname.into(), username: username.map(Into::into), port }
    }

    #[test]
    fn a_saved_session_is_reused_only_while_it_holds_every_standard_scope() {
        let all: Vec<&str> = STANDARD_SCOPES.to_vec();
        assert!(session_usable(&json!({"authenticated": true, "permissions": all})));
        assert!(session_usable(&json!({"authenticated": true, "scopes": all})), "older servers answer scopes");
        assert!(!session_usable(&json!({"authenticated": false, "permissions": all})), "expired");
        assert!(!session_usable(&json!({"authenticated": true, "permissions": &all[..all.len() - 1]})), "a newer scope is missing");
        assert!(!session_usable(&json!({"authenticated": true})));
        // `permissions ?? scopes`: a narrower permissions list wins over a full scopes one.
        assert!(!session_usable(&json!({"authenticated": true, "permissions": ["orchestration:read"], "scopes": all})));
    }

    #[test]
    fn config_lines_split_like_openssh() {
        let line = |l: &str| parse_config_line(l);
        assert_eq!(line("  Host a b  # tail"), Some(("host".into(), Some(vec!["a".into(), "b".into()]))));
        assert_eq!(line("HostName = \"my box\""), Some(("hostname".into(), Some(vec!["my box".into()]))));
        assert_eq!(line("HostName=box\\ two"), Some(("hostname".into(), Some(vec!["box two".into()]))));
        assert_eq!(line("Host a#b"), Some(("host".into(), Some(vec!["a#b".into()]))));
        assert_eq!(line("Host \"open"), Some(("host".into(), None)), "unbalanced quotes are unknown");
        assert_eq!(line("   "), None);
        assert_eq!(line("=x"), None);
    }

    #[test]
    fn discovery_reads_config_includes_and_known_hosts() {
        let home = std::env::temp_dir().join(format!("s5-ssh-discover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".ssh/conf.d")).unwrap();
        let config = "Include conf.d/*.conf\nHost devbox\n  HostName box.example.com\nHost *.corp !skip web-? gpu\n  User me\nMatch user root\n  HostName root.example\nHost late\n";
        std::fs::write(home.join(".ssh/config"), config).unwrap();
        std::fs::write(home.join(".ssh/conf.d/extra.conf"), "Host extra\n  HostName %h.lan\n").unwrap();
        std::fs::write(home.join(".ssh/known_hosts"), "box.example.com ssh-ed25519 AAAA\n[10.0.0.9]:2222 ssh-ed25519 AAAA\n|1|hashed ssh-rsa AAAA\n@cert-authority *.corp ssh-rsa AAAA\nplain,alias:22 ssh-rsa AAAA\n").unwrap();
        let hosts = discover(&home);
        let pairs: Vec<(&str, &str)> = hosts.iter().map(|h| (h.alias.as_str(), h.hostname.as_str())).collect();
        assert_eq!(
            pairs,
            [("10.0.0.9", "10.0.0.9"), ("alias", "alias"), ("devbox", "box.example.com"), ("extra", "extra.lan"), ("gpu", "gpu"), ("late", "late"), ("plain", "plain")],
            "patterns and hashed entries are skipped; known hosts a HostName covers are dropped; a Match block leaves later hosts unresolved"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn hostname_rules_follow_ssh_first_match() {
        let rule = |hosts: &[&str], hostname: Option<&str>| Rule { conditions: vec![Some(hosts.iter().map(|h| h.to_string()).collect())], hostname: hostname.map(Into::into) };
        let rules = [rule(&["web-*", "!web-skip"], Some("%h.internal")), rule(&["*"], Some("fallback"))];
        assert_eq!(resolve_configured_hostname("web-1", &rules).as_deref(), Some("web-1.internal"));
        assert_eq!(resolve_configured_hostname("web-skip", &rules).as_deref(), Some("fallback"));
        assert_eq!(resolve_configured_hostname("x", &[rule(&["*"], Some("%p.box"))]), None, "tokens other than %h are unknown");
        assert!(glob("a*c?", "abbbcd") && !glob("a*c", "abd") && glob("*", ""));
    }

    #[test]
    fn ssh_g_output_resolves_with_the_first_value_winning() {
        let out = "user me\nhostname box.example.com\nport 2222\nhostname ignored\nidentityfile ~/.ssh/id\n";
        assert_eq!(parse_resolve_output("devbox", out), target("devbox", "box.example.com", Some("me"), Some(2222)));
        assert_eq!(parse_resolve_output("devbox", ""), target("devbox", "devbox", None, None));
    }

    #[test]
    fn manual_targets_parse_like_the_web() {
        let ok = |h: &str, u: &str, p: &str| parse_manual(h, u, p).unwrap();
        assert_eq!(ok("me@box:2222", "", ""), target("box", "box", Some("me"), Some(2222)));
        assert_eq!(ok("me@box", "you", "22"), target("box", "box", Some("you"), Some(22)));
        assert_eq!(ok("[fe80::1]:2200", "", ""), target("fe80::1", "fe80::1", None, Some(2200)));
        assert_eq!(ok("fe80::1", "", ""), target("fe80::1", "fe80::1", None, None));
        assert_eq!(parse_manual("  ", "", "").unwrap_err(), "SSH host or alias is required.");
        assert_eq!(parse_manual("box", "", "70000").unwrap_err(), "SSH port must be between 1 and 65535.");
        assert_eq!(parse_manual("box", "", "x").unwrap_err(), "SSH port must be between 1 and 65535.");
        assert_eq!(ok("me@box:22", "", "").display(), "me@box:22");
    }

    #[test]
    fn keys_match_electrons_remote_state() {
        let t = target("devbox", "box.example.com", Some("me"), Some(2222));
        assert_eq!(t.key(), "devbox\0box.example.com\0me\02222");
        assert_eq!(t.state_key(), "aeaebdfe029a0865");
        assert_eq!(t.host_spec(), "me@devbox");
    }

    #[test]
    fn suggestions_put_alias_prefixes_first() {
        let host = |a: &str, h: &str| Host { alias: a.into(), hostname: h.into() };
        let hosts = [host("prod-db", "db.example"), host("devbox", "box.dev"), host("dev", "x")];
        let aliases = |q: &str| filter_hosts(&hosts, q).into_iter().map(|h| h.alias).collect::<Vec<_>>();
        assert_eq!(aliases("dev"), ["devbox", "dev"]);
        assert_eq!(aliases("example"), ["prod-db"]);
        assert_eq!(aliases(""), ["prod-db", "devbox", "dev"]);
    }

    #[test]
    fn scripts_fill_every_placeholder() {
        assert!(exact_version("0.1.7") && exact_version("1.2.3-rc.1") && !exact_version("01.2.3") && !exact_version("1.2") && !exact_version("1.2.3/../x"));
        assert!(runner_script("../x", RELEASE_BASE_URL).is_err());
        let runner = runner_script("0.1.7", "https://mirror.example/dl").unwrap();
        assert!(runner.contains("T3_ARCHIVE_VERSION='0.1.7'\n") && runner.contains("T3_RELEASE_BASE_URL='https://mirror.example/dl'\n"));
        assert!(runner.ends_with("exec \"$T3_RUNTIME_DIR/t3\" \"$@\""));
        let launch = launch_script(&runner);
        let pairing = pairing_script("aeaebdfe029a0865", &runner);
        let stop = stop_script("aeaebdfe029a0865");
        for script in [&runner, &launch, &pairing, &stop] {
            assert!(!script.contains("@@"), "{script}");
        }
        assert!(launch.contains("<<'SH'\n#!/bin/sh\n") && launch.contains("wait-ready \"$REMOTE_PORT\" \"$1\" \"1000\""));
        assert!(pairing.contains("ssh-launch/aeaebdfe029a0865\""));
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn remote_output_is_read_past_banners() {
        assert_eq!(parse_launch("Welcome!\n{\"remotePort\":3774,\"serverKind\":\"managed\"}\n"), Ok(3774));
        assert!(parse_launch("no json").is_err());
        assert_eq!(parse_credential("motd {not json}\n{\n  \"credential\": \"ABC\"\n}\n"), Ok("ABC".into()));
    }

    #[test]
    fn auth_failures_are_told_apart() {
        assert!(is_auth_failure("me@box: Permission denied (publickey,password)."));
        assert!(is_auth_failure("Received disconnect: Too many authentication failures"));
        assert!(!is_auth_failure("ssh: connect to host box port 22: Connection refused"));
        assert!(is_agent_signing_failure("sign_and_send_pubkey: signing failed for ED25519 \"key\" from agent: agent refused operation"));
        assert!(is_agent_signing_failure("sign_and_send_pubkey: signing failed: agent refused operation"));
        assert!(!is_agent_signing_failure("Permission denied (publickey)."));
    }

    #[test]
    fn remote_open_urls_match_the_web() {
        assert_eq!(remote_open_url("vscode", "box", Some("me"), "/srv/my app", false).as_deref(), Some("vscode://vscode-remote/ssh-remote+me%40box/srv/my%20app"));
        assert_eq!(remote_open_url("cursor", "box", None, "/a/b.rs", true).as_deref(), Some("cursor://vscode-remote/ssh-remote+box/a/b.rs:1:1"));
        assert_eq!(remote_open_url("zed", "box", Some("me"), "C:\\work", false).as_deref(), Some("zed://ssh/me@box/work"));
        assert_eq!(
            remote_open_url("idea", "box", Some("me"), "/srv/app", false).as_deref(),
            Some("jetbrains://gateway/ssh/environment?h=box&launchIde=true&ideHint=IU&projectHint=%2Fsrv%2Fapp&u=me")
        );
        assert_eq!(remote_open_url("trae", "box", None, "/a", false), None);
    }

    #[test]
    fn saved_targets_round_trip() {
        let targets = vec![("env-1".to_owned(), target("devbox", "box.example.com", Some("me"), Some(2222))), ("env-2".to_owned(), target("b", "b", None, None))];
        assert_eq!(decode_targets(&encode_targets(&targets)), targets);
    }
}
