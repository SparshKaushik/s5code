//! Settings › Devices (web `DevicesSettingsPanel`): the device hub and agent access switches, and
//! `DeviceSourcesSettings`' source list with its add menu. The host editors live in
//! `devices_editor`. Rune talks to one environment, so web's per-environment breakdowns collapse
//! to that environment.

use super::devices_editor::{self as editor, Editor};
use super::{controls as c, request, server_settings, value};
use crate::kit::{self, Menu, MenuItem, Placement};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, TextEdit, col, el, icon, row, text};
use serde_json::{Value, json};
use std::cmp::Ordering;

static APPLE: rune::Icon = rune::Icon {
    name: "device-apple",
    view: 24.0,
    body: r#"<path d="M12.152 6.896c-.948 0-2.415-1.078-3.96-1.04-2.04.027-3.91 1.183-4.961 3.014-2.117 3.675-.546 9.103 1.519 12.09 1.013 1.454 2.208 3.09 3.792 3.039 1.52-.065 2.09-.987 3.935-.987 1.831 0 2.35.987 3.96.948 1.637-.026 2.676-1.48 3.676-2.948 1.156-1.688 1.636-3.325 1.662-3.415-.039-.013-3.182-1.221-3.22-4.857-.026-3.04 2.48-4.494 2.597-4.559-1.429-2.09-3.623-2.324-4.39-2.376-2-.156-3.675 1.09-4.61 1.09zM15.53 3.83c.843-1.012 1.4-2.427 1.245-3.83-1.207.052-2.662.805-3.532 1.818-.78.896-1.454 2.338-1.273 3.714 1.338.104 2.715-.688 3.559-1.701" fill="currentColor"/>"#,
};

static ANDROID: rune::Icon = rune::Icon {
    name: "device-android",
    view: 24.0,
    body: r#"<path d="m8 3-1-2m9 2 1-2" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/><path fill-rule="evenodd" d="M5 10a7 7 0 0 1 14 0H5Zm4-4a.8.8 0 1 0 0 1.6A.8.8 0 0 0 9 6Zm6 0a.8.8 0 1 0 0 1.6A.8.8 0 0 0 15 6Z" fill="currentColor"/><path d="M5 11h14v7a2 2 0 0 1-2 2v2a1.5 1.5 0 0 1-3 0v-2h-4v2a1.5 1.5 0 0 1-3 0v-2a2 2 0 0 1-2-2v-7Z" fill="currentColor"/><rect x="1" y="11" width="3" height="8" rx="1.5" fill="currentColor"/><rect x="20" y="11" width="3" height="8" rx="1.5" fill="currentColor"/>"#,
};

static PLATFORMS: [(&str, &str, &rune::Icon); 2] = [("ios", "iOS", &APPLE), ("android", "Android", &ANDROID)];

/// The host editors' text fields; they live outside `Editor` so a field's `on_edit` always has
/// somewhere to write, even on the frame after the dialog closes.
#[derive(Default)]
pub struct Fields {
    pub label: TextEdit,
    pub target: TextEdit,
    pub identity: TextEdit,
    pub port: TextEdit,
    pub token: TextEdit,
    pub sessions: TextEdit,
    pub session_minutes: TextEdit,
    pub idle_minutes: TextEdit,
}

#[derive(Default)]
pub struct Ui {
    /// Web's `pending`: what the in-flight `device.configure` / `device.list` call is for.
    pending: Option<(&'static str, u64)>,
    update_error: Option<(&'static str, String)>,
    /// The open tool versions popover: `hub` or `agent`.
    versions: Option<&'static str>,
    /// The open menu: `add`, or a source row's key.
    menu: Option<String>,
    /// The open platform popover: a source row's key and the platform.
    platform: Option<(String, &'static str)>,
    test: Option<Test>,
    pub editor: Option<Editor>,
    pub fields: Fields,
}

/// A source row's "Test connection": web toasts the outcome; Rune shows it on the row.
struct Test {
    host: String,
    pending: Option<u64>,
    outcome: Option<Result<(), String>>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tone {
    Pending,
    Warning,
    Error,
}

/// What a source is doing when it is not simply working (web `SourceStatus`).
#[derive(Clone, PartialEq, Debug)]
struct Status {
    tone: Tone,
    label: &'static str,
    detail: Option<String>,
}

#[derive(Clone)]
enum Act {
    Refresh,
    Retry(String),
}

struct Source {
    key: String,
    title: String,
    kind: Option<&'static str>,
    description: String,
    /// One machine or account reached the same way everywhere; local simulators differ per machine.
    shared: bool,
    status: Option<Status>,
    action: Option<(&'static str, Act)>,
    platforms: Option<Vec<Value>>,
    notice: Option<Result<(), String>>,
}

fn state(s: &App) -> Option<&Value> {
    super::resource(s, "subscribeDeviceState", json!({})).filter(|v| v.get("error").is_none())
}

/// Every host the state lists: the wire splits EAS and Simlock hosts from `hosts`.
fn hosts(state: &Value) -> impl Iterator<Item = &Value> {
    ["hosts", "easHosts", "simlockHosts"]
        .into_iter()
        .flat_map(move |key| state[key].as_array().into_iter().flatten())
}

fn local_host(state: &Value) -> Option<&Value> {
    hosts(state).find(|host| host["kind"] == "local")
}

fn host_platforms(state: Option<&Value>, id: &str) -> Option<Vec<Value>> {
    let host = hosts(state?).find(|host| host["id"] == id)?;
    host["platforms"].as_array().cloned()
}

pub fn settings_list(s: &App, field: &str) -> Vec<Value> {
    server_settings(s)[field].as_array().cloned().unwrap_or_default()
}

pub fn environment_label(s: &App) -> String {
    s.state()
        .config
        .as_ref()
        .and_then(|c| c["environment"]["label"].as_str())
        .unwrap_or("this environment")
        .to_owned()
}

/// `hostStatus` in `DeviceSourcesSettings`.
fn host_status(state: Option<&Value>, host_id: &str) -> Option<Status> {
    let Some(state) = state else {
        return Some(Status { tone: Tone::Pending, label: "Loading", detail: None });
    };
    let status = &state["hostStatuses"][host_id];
    let detail = status["detail"].as_str().map(str::to_owned);
    match status["status"].as_str() {
        Some("ready") if status["androidDiscoveryIncomplete"] == true => {
            Some(Status { tone: Tone::Warning, label: "Android list may be incomplete", detail })
        }
        Some("installing" | "starting") => Some(Status { tone: Tone::Pending, label: "Starting", detail }),
        Some("failed") => Some(Status { tone: Tone::Error, label: "Failed", detail }),
        _ => None,
    }
}

/// `deviceHubReadiness(state).busy`.
fn hub_busy(state: &Value) -> bool {
    let statuses: Vec<&str> = state["hostStatuses"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(_, host)| host["status"].as_str())
        .collect();
    let hub = state["hostStatus"].as_str().unwrap_or("");
    let ready = hub == "ready" || statuses.contains(&"ready");
    !ready && std::iter::once(hub).chain(statuses).any(|status| status == "installing" || status == "starting")
}

/// The compact `DeviceHubSetupStatus` / `AgentDeviceSetupStatus` label while a change is pending.
fn pending_label(hub_status: &str) -> &'static str {
    match hub_status {
        "installing" => "Installing…",
        "starting" => "Starting…",
        _ => "Updating…",
    }
}

/// `localeCompare(…, {numeric: true})` for version strings: digit runs compare as numbers.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a, b);
    loop {
        match (a.chars().next(), b.chars().next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let na = a.find(|ch: char| !ch.is_ascii_digit()).unwrap_or(a.len());
                let nb = b.find(|ch: char| !ch.is_ascii_digit()).unwrap_or(b.len());
                let (da, db) = (a[..na].trim_start_matches('0'), b[..nb].trim_start_matches('0'));
                let order = da.len().cmp(&db.len()).then_with(|| da.cmp(db));
                if order != Ordering::Equal {
                    return order;
                }
                a = &a[na..];
                b = &b[nb..];
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                a = &a[x.len_utf8()..];
                b = &b[y.len_utf8()..];
            }
        }
    }
}

fn installed(tool: &Value) -> Vec<&str> {
    tool["installedVersions"].as_array().into_iter().flatten().filter_map(Value::as_str).collect()
}

/// `DeviceToolVersions`' version: the running one, else the required one when installed, else
/// the newest installed.
fn tool_version(tool: &Value) -> Option<String> {
    if let Some(running) = tool["runningVersion"].as_str() {
        return Some(running.to_owned());
    }
    let versions = installed(tool);
    if let Some(required) = tool["requiredVersion"].as_str().filter(|r| versions.contains(r)) {
        return Some(required.to_owned());
    }
    versions.into_iter().max_by(|a, b| natural_cmp(a, b)).map(str::to_owned)
}

fn version_label(tools: Option<&Value>, kind: &str) -> String {
    match tools.map(|tools| &tools[kind]).filter(|tool| tool.is_object()) {
        Some(tool) => tool_version(tool).map_or_else(|| "Not installed".to_owned(), |v| format!("v{v}")),
        None => "Version unknown".to_owned(),
    }
}

/// The readable message in a device RPC failure. Device errors build their message from fields
/// on the client, so the wire carries only `reason` (and the host's label).
pub fn error_text(cause: &Value) -> String {
    if let Some(detail) = super::failure_detail(cause) {
        return detail;
    }
    let reasons = cause.as_array().map(Vec::as_slice).unwrap_or(std::slice::from_ref(cause));
    reasons
        .iter()
        .find_map(|reason| {
            let error = if reason["_tag"] == "Fail" { &reason["error"] } else { reason };
            let why = error["reason"].as_str()?;
            Some(if error["_tag"] == "DeviceHostUnavailableError" {
                let host = error["hostLabel"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("Device host {}", error["hostId"].as_str().unwrap_or("")));
                format!("{host} is unavailable: {why}")
            } else {
                why.to_owned()
            })
        })
        .unwrap_or_else(|| "The request failed. Try reconnecting and trying again.".into())
}

fn same_destination(a: &Value, b: &Value) -> bool {
    a["target"] == b["target"] && a["port"] == b["port"] && a["identityFile"] == b["identityFile"]
}

fn find_destination<'a>(hosts: &'a [Value], destination: &Value) -> Result<Option<&'a Value>, String> {
    let mut matches = hosts.iter().filter(|candidate| same_destination(candidate, destination));
    let first = matches.next();
    if matches.next().is_some() {
        return Err("Multiple hosts match this SSH destination. Select the environment to edit its hosts.".into());
    }
    Ok(first)
}

/// `updateDeviceHosts`: adds, replaces (keeping the saved id), or removes one SSH host.
pub fn update_device_hosts(hosts: &[Value], host: &Value, remove: bool, original: &Value) -> Result<Vec<Value>, String> {
    let existing = match hosts.iter().find(|candidate| candidate["id"] == original["id"]) {
        Some(found) => Some(found),
        None => match find_destination(hosts, original)? {
            Some(found) => Some(found),
            None if remove => None,
            None => find_destination(hosts, host)?,
        },
    };
    let existing_id = existing.map(|found| found["id"].clone());
    if remove {
        return Ok(hosts.iter().filter(|candidate| Some(&candidate["id"]) != existing_id.as_ref()).cloned().collect());
    }
    Ok(match existing_id {
        Some(id) => hosts
            .iter()
            .map(|candidate| {
                if candidate["id"] == id {
                    let mut next = host.clone();
                    next["id"] = id.clone();
                    next
                } else {
                    candidate.clone()
                }
            })
            .collect(),
        None => hosts.iter().cloned().chain(std::iter::once(host.clone())).collect(),
    })
}

/// `updateEasDeviceHosts`: with `keep_token`, the saved (redacted) token stays as it is.
pub fn update_eas_hosts(hosts: &[Value], host: &Value, remove: bool, keep_token: bool) -> Vec<Value> {
    if remove {
        return hosts.iter().filter(|candidate| candidate["id"] != host["id"]).cloned().collect();
    }
    let existing = hosts.iter().find(|candidate| candidate["id"] == host["id"]);
    let mut next = host.clone();
    if keep_token {
        next["accessToken"] = existing.map(|found| found["accessToken"].clone()).filter(|t| !t.is_null()).unwrap_or(json!(""));
    }
    match existing {
        Some(_) => hosts.iter().map(|candidate| if candidate["id"] == host["id"] { next.clone() } else { candidate.clone() }).collect(),
        None => hosts.iter().cloned().chain(std::iter::once(next)).collect(),
    }
}

fn eas_session_limits(host: &Value) -> String {
    let sessions = match host["maxConcurrentSessions"].as_i64().unwrap_or(2) {
        1 => "1 session".to_owned(),
        n => format!("{n} sessions"),
    };
    format!(
        "Up to {sessions} at once, each stopped after {} min or {} idle min",
        host["maxSessionMinutes"].as_i64().unwrap_or(30),
        host["idleMinutes"].as_i64().unwrap_or(10)
    )
}

fn simlock_description(host: &Value) -> String {
    let gateway = host["gatewayUrl"].as_str().unwrap_or("");
    let gateway = gateway.strip_prefix("https://").or_else(|| gateway.strip_prefix("http://")).unwrap_or(gateway);
    let devices = match host["maxLeases"].as_i64().unwrap_or(2) {
        1 => "1 device".to_owned(),
        n => format!("{n} devices"),
    };
    format!("{gateway} · Up to {devices} at once, each released after {} min", host["maxLeaseMinutes"].as_i64().unwrap_or(60))
}

/// The platform popover's note: a shared source's own reason, or why this machine lacks it.
fn platform_note(shared: bool, available: bool, reason: Option<&str>) -> Option<String> {
    if available {
        return None;
    }
    if shared { reason.map(str::to_owned) } else { Some(reason.unwrap_or("Not set up on this machine.").to_owned()) }
}

/// `update` in `DeviceIntegrationControls`: one `device.configure` call, its outcome in `poll`.
fn send(s: &mut App, kind: &'static str, method: &str, input: Value) {
    let Some(net) = s.src.net() else { return };
    let id = net.request(method, input);
    s.ui.settings.devices.pending = Some((kind, id));
}

/// Writes a settings patch through the shared optimistic path; returns the request to watch.
pub fn save_settings(s: &mut App, patch: Value) -> Option<u64> {
    s.ui.settings.error = None;
    let before: Vec<u64> = s.ui.settings.pending.keys().copied().collect();
    request(s, "server.updateSettings", json!({"patch": patch}));
    s.ui.settings.pending.keys().copied().find(|id| !before.contains(id))
}

pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    if let Some((kind, id)) = s.ui.settings.devices.pending
        && let Some(reply) = s.src.net().and_then(|net| net.take_reply(id))
    {
        s.ui.settings.devices.pending = None;
        if let Err(cause) = reply {
            let message = error_text(&cause);
            match kind {
                "update-hub" => s.ui.settings.devices.update_error = Some(("hub", message)),
                "update-agent" => s.ui.settings.devices.update_error = Some(("agent", message)),
                "hub" | "agent" => {
                    s.ui.settings.error = Some(format!(
                        "Device settings not saved on all environments. Could not update {}.",
                        environment_label(s)
                    ));
                }
                _ => {}
            }
        }
        changed = true;
    }
    if let Some(id) = s.ui.settings.devices.test.as_ref().and_then(|test| test.pending)
        && let Some(reply) = s.src.net().and_then(|net| net.take_reply(id))
    {
        if let Some(test) = s.ui.settings.devices.test.as_mut() {
            test.pending = None;
            test.outcome = Some(reply.map(|_| ()).map_err(|cause| error_text(&cause)));
        }
        changed = true;
    }
    changed | editor::poll(s)
}

/// Escape closes the innermost devices overlay first.
pub fn escape(s: &mut App) -> bool {
    if !on_page(s) {
        return false;
    }
    let d = &mut s.ui.settings.devices;
    if d.menu.take().is_some() || d.versions.take().is_some() || d.platform.take().is_some() {
        return true;
    }
    editor::escape(s)
}

pub fn view(s: &App, t: &T) -> El {
    let state = state(s);
    col().w_full().gap(32.0).child(c::section("Devices", device_rows(s, t, state), t)).child(sources(s, t, state))
}

fn copy(t: &T, key: &str, title: &str, reset: Option<El>, description: El, note: Option<&str>) -> El {
    col()
        .key(format!("copy-{key}"))
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(row().gap(6.0).center_y().child(text(title.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground)).children(reset))
        .child(description)
        .children(note.map(|note| text(note.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground)))
}

/// A description followed by its tool's version, which opens the version details.
fn described(s: &App, t: &T, body: &str, kind: &'static str, tools: Option<&Value>) -> El {
    let open = s.ui.settings.devices.versions == Some(kind);
    let label = version_label(tools, kind);
    let name = if kind == "hub" { "Device hub" } else { "Agent device" };
    row()
        .flex_wrap()
        .gap(4.0)
        .center_y()
        .child(text(body.to_owned()).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground))
        .child(
            row().key(format!("devices-versions-{kind}-trigger")).child(
                text(label.clone())
                    .font(theme::XS_MEDIUM.lh(18.0))
                    .color(t.c.foreground)
                    .focusable(true)
                    .role("button")
                    .label(format!("{name}: {label}. Show details"))
                    .expanded(open)
                    .cursor(rune::Cursor::Pointer)
                    .on_press(move |s: &mut App| {
                        let d = &mut s.ui.settings.devices;
                        d.versions = if d.versions == Some(kind) { None } else { Some(kind) };
                    }),
            ),
        )
}

fn spinner_label(t: &T, label: &str) -> El {
    row()
        .gap(6.0)
        .center_y()
        .role("status")
        .child(kit::Spinner::new().size("xs").tone("muted").build(&kit::theme(t)))
        .child(text(label.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground))
}

fn device_rows(s: &App, t: &T, state: Option<&Value>) -> Vec<El> {
    let project = s.ui.settings.project.is_some();
    let connected = s.state().config.is_some();
    let loaded = state.is_some();
    let busy = state.is_some_and(hub_busy);
    let pending = s.ui.settings.devices.pending.map(|(kind, _)| kind);
    let hub_status = state.and_then(|st| st["hostStatus"].as_str()).unwrap_or("");
    let tools = state.and_then(local_host).and_then(|host| host.get("tools"));
    let any_hub = server_settings(s)["enableDeviceSupport"] == true;
    let k = kit::theme(t);
    let control = |status: Option<El>, switch: El| row().w(160.0).gap(8.0).center_y().child(el().grow(1.0)).children(status).child(switch);

    let enabled = value(s, "enableDeviceSupport") == true;
    let hub_switch = kit::Switch::new(enabled)
        .label("Device hub")
        .hide_label()
        .disabled(project || !loaded || !connected || busy || pending.is_some())
        .on_toggle(move |s: &mut App| {
            let mut input = json!({"enabled": !enabled});
            if enabled {
                input["agentAccessEnabled"] = json!(false);
            } else {
                input["onboardingCompleted"] = json!(true);
            }
            send(s, "hub", "device.configure", input);
        })
        .build(&k);
    let hub_ctrl = control((pending == Some("hub")).then(|| spinner_label(t, pending_label(hub_status))), hub_switch);
    let hub = row()
        .key("row-device-hub")
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(32.0)
        .center_y()
        .child(copy(
            t,
            "device-hub",
            "Use devices",
            None,
            described(s, t, "Open simulators and emulators in the Device panel. Hub:", "hub", tools),
            None,
        ))
        .child(hub_ctrl);

    let agent_on = value(s, "enableAgentDeviceAccess") == true;
    let agent_switch = kit::Switch::new(agent_on)
        .label("Agent device access")
        .hide_label()
        .disabled(!connected || (!project && (!loaded || !any_hub || busy)) || pending.is_some())
        .on_toggle(move |s: &mut App| {
            if s.ui.settings.project.is_some() {
                super::update(s, "enableAgentDeviceAccess", json!(!agent_on));
            } else {
                send(s, "agent", "device.configure", json!({"agentAccessEnabled": !agent_on}));
            }
        })
        .build(&k);
    let key = "enableAgentDeviceAccess";
    let reset = (project && super::scope::customized(s, key)).then(|| {
        kit::Button::new("Reset Agents can use devices")
            .variant("ghost-muted")
            .size("icon-micro")
            .icon(&icons::UNDO_2)
            .on_press(move |s: &mut App| super::scope::reset(s, key))
            .build(t)
    });
    let note = project.then(|| {
        if super::scope::customized(s, key) { "Overridden for this project" } else { "Inherited from environment" }
    });
    let agent = row()
        .key("row-agent-device-access")
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(32.0)
        .center_y()
        .child(copy(
            t,
            "agent-device-access",
            "Agents can use devices",
            reset,
            described(s, t, "New agent sessions can open and drive these devices. agent-device:", "agent", tools),
            note,
        ))
        .child(control((pending == Some("agent")).then(|| spinner_label(t, pending_label(hub_status))), agent_switch));

    let mut rows = vec![hub, agent];
    if let Some(updates) = state.and_then(|state| host_updates(t, state)) {
        rows.push(updates);
    }
    rows
}

/// `DeviceHostUpdates` for this machine: tools installing, starting, or failed to start.
fn host_updates(t: &T, state: &Value) -> Option<El> {
    if state["hostStatus"] == "disabled" {
        return None;
    }
    let local = local_host(state)?;
    let id = local["id"].as_str()?.to_owned();
    let status = &state["hostStatuses"][id.as_str()];
    let kind = status["status"].as_str()?;
    if !matches!(kind, "installing" | "starting" | "failed") {
        return None;
    }
    let failed = kind == "failed";
    let detail = status["detail"].as_str().map(str::to_owned).unwrap_or_else(|| {
        match kind {
            "failed" => "Device support could not start.",
            "installing" => "Installing device tools…",
            _ => "Starting device tools…",
        }
        .to_owned()
    });
    let mut body = col()
        .grow(1.0)
        .min_w(0.0)
        .child(text(local["label"].as_str().unwrap_or("This machine").to_owned()).font(theme::XS_MEDIUM).color(t.c.foreground))
        .child(text(detail).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap());
    if failed {
        body = body.child(
            text("Check the host connection and network access, then retry. Your device settings are saved.")
                .font(theme::TEXT_XS)
                .color(t.c.muted_foreground)
                .wrap()
                .mt(4.0),
        );
    }
    let retry = (failed && state["supportsHostRetry"] == true).then(|| {
        kit::Button::new("Retry")
            .variant("outline")
            .size("compact")
            .on_press(move |s: &mut App| request(s, "device.list", json!({"retryHostId": id})))
            .build(t)
    });
    Some(
        col().w_full().px(16.0).py(12.0).child(
            row()
                .w_full()
                .role(if failed { "alert" } else { "status" })
                .items_start()
                .gap(12.0)
                .px(12.0)
                .py(8.0)
                .radius(6.0)
                .border(1.0, t.c.border.alpha_mul(0.6))
                .child(body)
                .children(retry),
        ),
    )
}

fn run(s: &mut App, act: &Act) {
    match act {
        Act::Refresh => request(s, "device.list", json!({})),
        Act::Retry(id) => request(s, "device.list", json!({"retryHostId": id})),
    }
}

fn source_list(s: &App, state: Option<&Value>) -> Vec<Source> {
    let settings = server_settings(s);
    let active = settings["enableDeviceSupport"] == true;
    let retry = state.is_some_and(|st| st["supportsHostRetry"] == true);
    let test = s.ui.settings.devices.test.as_ref();
    let mut rows = Vec::new();

    let local_id = state.and_then(local_host).and_then(|host| host["id"].as_str()).unwrap_or("").to_owned();
    let status = if active { host_status(state, &local_id) } else { None };
    rows.push(Source {
        key: "local".into(),
        title: "Local simulators".into(),
        kind: None,
        description: "Simulators and emulators installed on this environment's machine.".into(),
        shared: false,
        action: status.as_ref().filter(|st| st.tone == Tone::Error).map(|_| ("Refresh", Act::Refresh)),
        status,
        platforms: if active { state.and_then(local_host).and_then(|host| host["platforms"].as_array().cloned()) } else { None },
        notice: None,
    });

    for host in settings["deviceHosts"].as_array().into_iter().flatten() {
        let id = host["id"].as_str().unwrap_or("").to_owned();
        let testing = test.filter(|test| test.host == id);
        let status = if !active {
            None
        } else if testing.is_some_and(|test| test.pending.is_some()) {
            Some(Status { tone: Tone::Pending, label: "Testing connection", detail: None })
        } else {
            host_status(state, &id).map(|st| if st.tone == Tone::Error && retry { Status { label: "Can't connect", ..st } } else { st })
        };
        rows.push(Source {
            key: format!("ssh:{id}"),
            title: host["label"].as_str().unwrap_or("").to_owned(),
            kind: Some("SSH"),
            description: host["target"].as_str().unwrap_or("").to_owned(),
            shared: true,
            action: status.as_ref().filter(|st| st.tone == Tone::Error && retry).map(|_| ("Retry", Act::Retry(id.clone()))),
            status,
            platforms: if active { host_platforms(state, &id) } else { None },
            notice: testing.and_then(|test| test.outcome.clone()),
        });
    }

    if state.is_some_and(|st| st["supportsEasHosts"] == true) {
        for host in settings["easDeviceHosts"].as_array().into_iter().flatten() {
            let id = host["id"].as_str().unwrap_or("").to_owned();
            rows.push(Source {
                key: format!("eas:{id}"),
                title: host["label"].as_str().unwrap_or("").to_owned(),
                kind: Some("EAS"),
                description: eas_session_limits(host),
                shared: true,
                status: if active { host_status(state, &id) } else { None },
                action: None,
                platforms: if active { host_platforms(state, &id) } else { None },
                notice: None,
            });
        }
    }

    if state.is_some_and(|st| st["supportsSimlockHosts"] == true) && settings["simlockDeviceHostsEnabled"] == true {
        for host in settings["simlockDeviceHosts"].as_array().into_iter().flatten() {
            let id = host["id"].as_str().unwrap_or("").to_owned();
            rows.push(Source {
                key: format!("simlock:{id}"),
                title: host["label"].as_str().unwrap_or("").to_owned(),
                kind: Some("Simlock"),
                description: simlock_description(host),
                shared: true,
                status: if active { host_status(state, &id) } else { None },
                action: None,
                platforms: if active { host_platforms(state, &id) } else { None },
                notice: None,
            });
        }
    }
    rows
}

/// Whether the source list can change: web's `disabled` (project scope, busy, no environment).
fn sources_disabled(s: &App) -> bool {
    s.ui.settings.project.is_some() || s.state().config.is_none() || editor::saving(s)
}

fn sources(s: &App, t: &T, state: Option<&Value>) -> El {
    let disabled = sources_disabled(s);
    let open = s.ui.settings.devices.menu.as_deref() == Some("add");
    let add = kit::Button::new("Add source")
        .variant("outline")
        .size("sm")
        .icon(&icons::PLUS)
        .disabled(disabled || s.ui.settings.devices.editor.is_some())
        .on_press(move |s: &mut App| s.ui.settings.devices.menu = if open { None } else { Some("add".into()) })
        .build(t)
        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(t.c.foreground).a11y_hidden())
        .expanded(open);
    let rows = source_list(s, state).into_iter().map(|src| source_row(s, t, src, disabled)).collect();
    c::section_with_actions("Device sources", vec![row().key("devices-add-source").child(add)], rows, t)
}

fn kind_badge(t: &T, kind: &str) -> El {
    row()
        .px(4.0)
        .radius(4.0)
        .border(1.0, t.c.border)
        .child(text(kind.to_uppercase()).font(theme::TEXT_3XS).color(t.c.muted_foreground))
}

fn inline_action(t: &T, label: &str, act: Act) -> El {
    text(label.to_owned())
        .font(theme::XS_MEDIUM)
        .color(t.c.foreground)
        .focusable(true)
        .role("button")
        .cursor(rune::Cursor::Pointer)
        .on_press(move |s: &mut App| run(s, &act))
}

fn source_row(s: &App, t: &T, src: Source, disabled: bool) -> El {
    let mut info = col()
        .grow(1.0)
        .min_w(0.0)
        .child(
            row()
                .min_w(0.0)
                .gap(6.0)
                .center_y()
                .child(text(src.title.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate().shrink())
                .children(src.kind.map(|kind| kind_badge(t, kind))),
        )
        .child(text(src.description.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate());
    if let Some(status) = src.status.as_ref().filter(|st| st.tone != Tone::Pending) {
        let color = if status.tone == Tone::Error { t.c.destructive } else { t.c.warning };
        info = info.child(
            row()
                .mt(4.0)
                .gap(6.0)
                .center_y()
                .role("status")
                .child(
                    text(status.detail.clone().unwrap_or_else(|| status.label.to_owned()))
                        .font(theme::TEXT_XS)
                        .color(color)
                        .wrap()
                        .shrink(),
                )
                .children(src.action.clone().map(|(label, act)| inline_action(t, label, act))),
        );
    }
    if let Some(outcome) = &src.notice {
        let (message, color) = match outcome {
            Ok(()) => ("Connection checks passed".to_owned(), t.c.muted_foreground),
            Err(error) => (error.clone(), t.c.destructive),
        };
        info = info.child(text(message).font(theme::TEXT_XS).color(color).wrap().mt(4.0));
    }
    let side = match src.status.as_ref().filter(|st| st.tone == Tone::Pending) {
        Some(pending) => spinner_label(t, pending.label),
        None => platform_icons(s, t, &src),
    };
    let mut line = row().key(format!("device-source-{}", src.key)).w_full().px(16.0).py(12.0).gap(12.0).center_y().child(info).child(side);
    if src.key != "local" {
        let key = src.key.clone();
        let menu_open = s.ui.settings.devices.menu.as_deref() == Some(key.as_str());
        let mut actions = row().gap(4.0).center_y();
        if !src.key.starts_with("simlock:") {
            let edit = key.clone();
            actions = actions.child(
                kit::Button::new("Edit")
                    .variant("outline")
                    .size("sm")
                    .disabled(disabled)
                    .on_press(move |s: &mut App| editor::edit(s, &edit))
                    .build(t),
            );
        }
        actions = actions.child(
            row().key(format!("devices-more-{key}")).child(
                kit::Button::new("More")
                    .variant("ghost-muted")
                    .size("icon-sm")
                    .icon(&icons::ELLIPSIS_VERTICAL)
                    .disabled(disabled)
                    .on_press(move |s: &mut App| {
                        s.ui.settings.devices.menu = if menu_open { None } else { Some(key.clone()) };
                    })
                    .build(t)
                    .expanded(menu_open),
            ),
        );
        line = line.child(actions);
    }
    line
}

/// `SourcePlatforms`: one icon per platform the source reports, solid where available.
fn platform_icons(s: &App, t: &T, src: &Source) -> El {
    let mut icons_row = row().gap(2.0).center_y();
    let Some(platforms) = &src.platforms else { return icons_row };
    for &(platform, name, glyph) in PLATFORMS.iter() {
        let Some(entry) = platforms.iter().find(|p| p["platform"] == platform) else { continue };
        let available = entry["available"] == true;
        let open = s.ui.settings.devices.platform.as_ref().is_some_and(|(key, p)| *key == src.key && *p == platform);
        let key = src.key.clone();
        icons_row = icons_row.child(
            row().key(format!("devices-platform-{}-{platform}-trigger", src.key)).child(
                el().size(24.0, 24.0)
                    .center()
                    .radius(6.0)
                    .hover(t.c.accent)
                    .focusable(true)
                    .role("button")
                    .label(format!("{}: {name} {}", src.title, if available { "available" } else { "not available" }))
                    .expanded(open)
                    .on_press(move |s: &mut App| {
                        let d = &mut s.ui.settings.devices;
                        d.platform = if open { None } else { Some((key.clone(), platform)) };
                    })
                    .child(icon(glyph, 14.0).color(if available { t.c.foreground } else { t.c.muted_foreground.alpha_mul(0.45) })),
            ),
        );
    }
    icons_row
}

fn close_menu(s: &mut App) {
    s.ui.settings.devices.menu = None;
}

fn start_test(s: &mut App, host_id: &str) {
    let Some(host) = settings_list(s, "deviceHosts").into_iter().find(|host| host["id"] == host_id) else { return };
    let Some(net) = s.src.net() else { return };
    let id = net.request("device.testHost", host);
    s.ui.settings.devices.test = Some(Test { host: host_id.to_owned(), pending: Some(id), outcome: None });
}

fn remove(s: &mut App, key: &str) {
    let Some((kind, id)) = key.split_once(':') else { return };
    let (field, next) = match kind {
        "ssh" => {
            let hosts = settings_list(s, "deviceHosts");
            let Some(host) = hosts.iter().find(|host| host["id"] == id).cloned() else { return };
            match update_device_hosts(&hosts, &host, true, &host) {
                Ok(next) => ("deviceHosts", next),
                Err(message) => {
                    s.ui.settings.error = Some(format!("Device host not removed everywhere. {message}"));
                    return;
                }
            }
        }
        "eas" => ("easDeviceHosts", settings_list(s, "easDeviceHosts").into_iter().filter(|host| host["id"] != id).collect()),
        "simlock" => ("simlockDeviceHosts", settings_list(s, "simlockDeviceHosts").into_iter().filter(|host| host["id"] != id).collect()),
        _ => return,
    };
    save_settings(s, json!({ field: next }));
}

fn on_page(s: &App) -> bool {
    matches!(&s.route, crate::Route::Settings(page) if page == "devices")
}

pub fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    if !on_page(s) {
        return;
    }
    let k = kit::theme(t);
    let state = state(s);
    let d = &s.ui.settings.devices;
    if let Some(kind) = d.versions {
        versions_popover(s, t, state, kind, layers);
    }
    match d.menu.as_deref() {
        Some("add") => {
            let mut menu = Menu::new("devices-add-menu")
                .anchor("devices-add-source")
                .placement(Placement::BottomEnd)
                .gutter(4.0)
                .open(true)
                .on_close(close_menu)
                .item(MenuItem::new("Machine over SSH").on_select(|s: &mut App| {
                    close_menu(s);
                    editor::open_ssh(s, None);
                }));
            if state.is_some_and(|st| st["supportsEasHosts"] == true) {
                menu = menu.item(MenuItem::new("EAS Simulator").on_select(|s: &mut App| {
                    close_menu(s);
                    editor::open_eas(s, None);
                }));
            }
            menu.overlay(&k, layers);
        }
        Some(key) => {
            let mut menu = Menu::new("devices-more-menu")
                .anchor(format!("devices-more-{key}"))
                .placement(Placement::BottomEnd)
                .gutter(4.0)
                .open(true)
                .on_close(close_menu);
            if let Some(id) = key.strip_prefix("ssh:") {
                let id = id.to_owned();
                menu = menu.item(MenuItem::new("Test connection").on_select(move |s: &mut App| {
                    close_menu(s);
                    start_test(s, &id);
                }));
            }
            let owned = key.to_owned();
            menu.item(MenuItem::new("Remove").variant("destructive").on_select(move |s: &mut App| {
                close_menu(s);
                remove(s, &owned);
            }))
            .overlay(&k, layers);
        }
        None => {}
    }
    if let Some((key, platform)) = &d.platform {
        platform_popover(s, t, state, key, platform, layers);
    }
    editor::dialog(s, t, layers);
}

fn platform_popover(s: &App, t: &T, state: Option<&Value>, key: &str, platform: &str, layers: &mut Vec<El>) {
    let Some(src) = source_list(s, state).into_iter().find(|src| src.key == key) else { return };
    let Some((_, name, _)) = PLATFORMS.iter().find(|(p, _, _)| *p == platform) else { return };
    let entry = src.platforms.as_ref().and_then(|list| list.iter().find(|p| p["platform"] == platform));
    let available = entry.is_some_and(|e| e["available"] == true);
    let note = platform_note(src.shared, available, entry.and_then(|e| e["reason"].as_str()));
    let mut popover = kit::Popover::new(format!("devices-platform-{key}-{platform}"))
        .open(true)
        .width("sm")
        .placement(Placement::TopEnd)
        .label(format!("{name} {}", if available { "available" } else { "not available" }))
        .child(
            text(format!("{name} {}", if available { "available" } else { "not available" }))
                .font(theme::SM_MEDIUM)
                .color(t.c.foreground),
        )
        .on_close(|s: &mut App| s.ui.settings.devices.platform = None);
    if let Some(note) = note {
        popover = popover.child(text(note).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap());
    }
    let _trigger = popover.build(&kit::theme(t), layers);
}

/// `DeviceToolVersions`' popover with `versionActions`: versions, then update and check buttons.
fn versions_popover(s: &App, t: &T, state: Option<&Value>, kind: &'static str, layers: &mut Vec<El>) {
    let d = &s.ui.settings.devices;
    let tools = state.and_then(local_host).and_then(|host| host.get("tools"));
    let name = if kind == "hub" { "Device hub" } else { "Agent device" };
    let mut body = col().w_full().gap(0.0);
    match tools.map(|tools| &tools[kind]).filter(|tool| tool.is_object()) {
        Some(tool) => {
            let versions = installed(tool);
            let line = |label: &str, v: String| {
                row()
                    .w_full()
                    .gap(24.0)
                    .items_start()
                    .child(text(label.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground))
                    .child(el().grow(1.0))
                    .child(text(v).font(theme::MONO_XS).color(t.c.foreground).wrap().shrink())
            };
            body = body.gap(4.0).mt(8.0)
                .child(line("Running", tool["runningVersion"].as_str().unwrap_or("Not running").to_owned()))
                .child(line("Required", tool["requiredVersion"].as_str().unwrap_or("").to_owned()))
                .child(line("Installed", if versions.is_empty() { "None".to_owned() } else { versions.join(", ") }));
        }
        None => {
            body = body.mt(4.0).child(text("Versions have not been checked.").font(theme::TEXT_XS).color(t.c.muted_foreground));
        }
    }
    let footer = col()
        .w_full()
        .mt(12.0)
        .child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.5)).mb(12.0))
        .child(text("Tools update automatically on this host when needed.").font(theme::TEXT_XS).color(t.c.muted_foreground).wrap());

    let busy = state.is_some_and(hub_busy);
    let blocked = s.state().config.is_none() || d.pending.is_some() || busy;
    let pending = d.pending.map(|(p, _)| p);
    let mut buttons = row().gap(8.0).flex_wrap();
    let tool = tools.map(|tools| &tools[kind]).filter(|tool| tool.is_object());
    let required = tool.and_then(|tool| tool["requiredVersion"].as_str()).map(str::to_owned);
    let needs_update = tool.is_some_and(|tool| required.as_deref().is_some_and(|r| !installed(tool).contains(&r)));
    let update_kind: &'static str = if kind == "hub" { "update-hub" } else { "update-agent" };
    let mut any = false;
    if state.is_some_and(|st| st["supportsToolUpdate"] == true) && needs_update {
        let label = if pending == Some(update_kind) {
            "Updating…".to_owned()
        } else {
            format!("Update to v{}", required.unwrap_or_default())
        };
        buttons = buttons.child(
            kit::Button::new(label)
                .size("sm")
                .disabled(blocked)
                .on_press(move |s: &mut App| {
                    s.ui.settings.devices.update_error = None;
                    send(s, update_kind, "device.list", json!({"updateTool": kind}));
                })
                .build(t),
        );
        any = true;
    }
    if state.is_some_and(|st| st["supportsToolInspection"] == true) {
        buttons = buttons.child(
            kit::Button::new(if pending == Some("check") { "Checking…" } else { "Check versions" })
                .variant("outline")
                .size("sm")
                .disabled(blocked)
                .on_press(|s: &mut App| send(s, "check", "device.list", json!({"inspectOnly": true})))
                .build(t),
        );
        any = true;
    }
    let mut popover = kit::Popover::new(format!("devices-versions-{kind}"))
        .open(true)
        .width("md")
        .placement(Placement::BottomEnd)
        .title(name)
        .child(body)
        .child(footer)
        .on_close(|s: &mut App| s.ui.settings.devices.versions = None);
    let error = d.update_error.as_ref().filter(|(tool, _)| *tool == kind).map(|(_, message)| message.clone());
    if any || error.is_some() {
        let mut action = col().w_full().gap(8.0).mt(4.0);
        if any {
            action = action.child(buttons);
        }
        if let Some(error) = error {
            action = action.child(text(error).font(theme::TEXT_XS).color(t.c.destructive).wrap().role("alert"));
        }
        popover = popover.child(action);
    }
    let _trigger = popover.build(&kit::theme(t), layers);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_status_follows_web_source_status() {
        assert_eq!(host_status(None, "local").map(|s| (s.tone, s.label)), Some((Tone::Pending, "Loading")));
        let state = json!({"hostStatuses": {
            "a": {"status": "ready"},
            "b": {"status": "ready", "androidDiscoveryIncomplete": true, "detail": "adb timed out"},
            "c": {"status": "installing"},
            "d": {"status": "failed", "detail": "ssh: connect refused"},
        }});
        assert_eq!(host_status(Some(&state), "a"), None);
        assert_eq!(host_status(Some(&state), "missing"), None);
        let b = host_status(Some(&state), "b").unwrap();
        assert_eq!((b.tone, b.label, b.detail.as_deref()), (Tone::Warning, "Android list may be incomplete", Some("adb timed out")));
        assert_eq!(host_status(Some(&state), "c").map(|s| (s.tone, s.label)), Some((Tone::Pending, "Starting")));
        assert_eq!(host_status(Some(&state), "d").map(|s| (s.tone, s.label)), Some((Tone::Error, "Failed")));
    }

    #[test]
    fn hub_is_busy_only_while_nothing_is_ready() {
        assert!(hub_busy(&json!({"hostStatus": "installing", "hostStatuses": {}})));
        assert!(hub_busy(&json!({"hostStatus": "idle", "hostStatuses": {"x": {"status": "starting"}}})));
        assert!(!hub_busy(&json!({"hostStatus": "installing", "hostStatuses": {"x": {"status": "ready"}}})));
        assert!(!hub_busy(&json!({"hostStatus": "failed", "hostStatuses": {}})));
        assert_eq!(pending_label("installing"), "Installing…");
        assert_eq!(pending_label("ready"), "Updating…");
    }

    #[test]
    fn version_labels_match_device_tool_versions() {
        assert_eq!(version_label(None, "hub"), "Version unknown");
        let tools = json!({
            "hub": {"requiredVersion": "1.2.0", "installedVersions": [], "runningVersion": null},
            "agent": {"requiredVersion": "0.9.0", "installedVersions": ["0.10.1", "0.9.0"], "runningVersion": null},
        });
        assert_eq!(version_label(Some(&tools), "hub"), "Not installed");
        assert_eq!(version_label(Some(&tools), "agent"), "v0.9.0");
        let newest = json!({"agent": {"requiredVersion": "2.0.0", "installedVersions": ["0.10.1", "0.9.0"], "runningVersion": null}});
        assert_eq!(version_label(Some(&newest), "agent"), "v0.10.1");
        let running = json!({"hub": {"requiredVersion": "1.0.0", "installedVersions": ["1.0.0"], "runningVersion": "0.8.0"}});
        assert_eq!(version_label(Some(&running), "hub"), "v0.8.0");
        assert_eq!(natural_cmp("1.10.0", "1.9.0"), Ordering::Greater);
        assert_eq!(natural_cmp("1.0", "1.0"), Ordering::Equal);
    }

    #[test]
    fn ssh_hosts_update_by_id_then_destination() {
        let a = json!({"kind": "ssh", "id": "a", "label": "Mini", "target": "me@mini"});
        let b = json!({"kind": "ssh", "id": "b", "label": "Studio", "target": "me@studio", "port": 2222});
        let hosts = vec![a.clone(), b.clone()];
        let renamed = json!({"kind": "ssh", "id": "a", "label": "Mac mini", "target": "me@mini"});
        assert_eq!(update_device_hosts(&hosts, &renamed, false, &a).unwrap(), vec![renamed.clone(), b.clone()]);
        // Another environment's id for the same destination keeps the saved id.
        let foreign = json!({"kind": "ssh", "id": "z", "label": "Studio 2", "target": "me@studio", "port": 2222});
        let next = update_device_hosts(&hosts, &foreign, false, &foreign).unwrap();
        assert_eq!(next[1]["id"], "b");
        assert_eq!(next[1]["label"], "Studio 2");
        let added = json!({"kind": "ssh", "id": "c", "label": "New", "target": "me@new"});
        assert_eq!(update_device_hosts(&hosts, &added, false, &added).unwrap().len(), 3);
        assert_eq!(update_device_hosts(&hosts, &a, true, &a).unwrap(), vec![b.clone()]);
        let twins = vec![a.clone(), json!({"id": "d", "label": "Twin", "target": "me@mini"})];
        let probe = json!({"id": "x", "label": "X", "target": "me@mini"});
        assert!(update_device_hosts(&twins, &probe, false, &probe).is_err());
    }

    #[test]
    fn eas_hosts_keep_the_saved_token_when_asked() {
        let saved = json!({"kind": "eas", "id": "e", "label": "EAS", "accessToken": "<redacted>", "maxSessionMinutes": 30, "idleMinutes": 10, "maxConcurrentSessions": 2});
        let edited = json!({"kind": "eas", "id": "e", "label": "Expo", "accessToken": "", "maxSessionMinutes": 40, "idleMinutes": 10, "maxConcurrentSessions": 1});
        let next = update_eas_hosts(std::slice::from_ref(&saved), &edited, false, true);
        assert_eq!(next[0]["accessToken"], "<redacted>");
        assert_eq!(next[0]["label"], "Expo");
        assert_eq!(update_eas_hosts(std::slice::from_ref(&saved), &edited, false, false)[0]["accessToken"], "");
        assert!(update_eas_hosts(std::slice::from_ref(&saved), &edited, true, false).is_empty());
        assert_eq!(eas_session_limits(&edited), "Up to 1 session at once, each stopped after 40 min or 10 idle min");
        assert_eq!(eas_session_limits(&saved), "Up to 2 sessions at once, each stopped after 30 min or 10 idle min");
        let pool = json!({"gatewayUrl": "https://gw.example.ts.net:4700", "maxLeases": 1, "maxLeaseMinutes": 60});
        assert_eq!(simlock_description(&pool), "gw.example.ts.net:4700 · Up to 1 device at once, each released after 60 min");
    }

    #[test]
    fn platform_notes_explain_what_is_missing() {
        assert_eq!(platform_note(false, true, None), None);
        assert_eq!(platform_note(false, false, None).as_deref(), Some("Not set up on this machine."));
        assert_eq!(platform_note(false, false, Some("Install Xcode")).as_deref(), Some("Install Xcode"));
        assert_eq!(platform_note(true, false, None), None);
        assert_eq!(platform_note(true, false, Some("No Android")).as_deref(), Some("No Android"));
    }

    #[test]
    fn device_errors_read_their_reason() {
        let unavailable = json!([{"_tag": "Fail", "error": {"_tag": "DeviceHostUnavailableError", "hostId": "mini", "hostLabel": "Mac mini", "reason": "ssh timed out"}}]);
        assert_eq!(error_text(&unavailable), "Mac mini is unavailable: ssh timed out");
        let eas = json!({"_tag": "DeviceEasAccountError", "reason": "Expo rejected the token."});
        assert_eq!(error_text(&eas), "Expo rejected the token.");
        assert_eq!(error_text(&json!({"_tag": "Die"})), "The request failed. Try reconnecting and trying again.");
    }
}
