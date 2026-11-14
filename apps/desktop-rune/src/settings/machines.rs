//! Web Connections' sections across machines: `LoadBalancingSettings`,
//! `PullRequestRoutingSettings`, and the Environments header's `ServerUpdatesAction`.

use super::connections::{machine_icon, machine_kind, route_presentation};
use crate::env;
use crate::kit;
use crate::shell::palette::{toast, toast_error};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, col, el, icon, row, text};
use serde_json::{Value, json};

#[derive(Default)]
pub struct Ui {
    balancing_open: bool,
    routing_open: bool,
    /// The open per-machine select: `balance:` or `routing:` and the environment key.
    pub(super) select: Option<String>,
    /// Pull request sharing grants, read once (web localStorage `t3code:github-routing:*`).
    grants: Option<Vec<Grant>>,
    /// Update all: the requests in flight, and desktop apps waiting on the confirmation.
    updating: Vec<(u64, Target)>,
    pub(super) confirm: Option<Vec<Target>>,
    /// A saved row's own Update, waiting on its desktop app confirmation.
    pub(super) confirm_row: Option<Target>,
    /// Each environment's last failed update, until it is retried (web `ServerUpdateProgress`).
    failed: std::collections::HashMap<String, String>,
}

#[derive(Clone, Debug, PartialEq)]
struct Grant {
    environment_id: String,
    connection_key: String,
    permission: String,
}

/// Web `ServerUpdateTarget` for Update all.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Target {
    key: String,
    label: String,
    self_update: String,
    target_version: String,
}

const PREFERENCES: [(u8, &str); 4] = [(100, "Prefer"), (50, "Normal"), (25, "Less often"), (0, "Manual only")];
const ROUTING: [(&str, &str); 3] = [("off", "Off"), ("read", "Read PRs"), ("read-write", "Read and act")];

/// Web `loadBalancingEnvironments`: this machine, then the saved environments switched on.
fn machines(s: &App) -> Vec<String> {
    let local = (!s.ui.desktop.local_off && env::source(s, env::LOCAL).is_some()).then(|| env::LOCAL.to_owned());
    local.into_iter().chain(s.ui.env.saved.iter().filter(|e| !s.ui.env.off.contains(&e.id)).map(|e| e.id.clone())).collect()
}

/// The environment id a machine's settings are keyed by.
fn environment_id(s: &App, key: &str) -> Option<String> {
    if key == env::LOCAL { env::local_id(s) } else { Some(key.to_owned()) }
}

/// Web `environmentTransportLabel`.
fn transport(s: &App, key: &str) -> String {
    if key == env::LOCAL {
        return "This machine".into();
    }
    let routes = env::routes(s, key);
    if routes.len() > 1 {
        return match env::route_in_use(s, key) {
            Some(i) => format!("via {}", route_presentation(s, key, &routes[i]).0),
            None => route_presentation(s, key, &routes[0]).0,
        };
    }
    env::ssh::transport(s, key).unwrap_or_else(|| routes.first().map(|r| r.base.clone()).unwrap_or_else(|| "Remote link".into()))
}

/// Web `EnvironmentRow` with a trailing control.
fn machine_row(s: &App, t: &T, key: &str, control: El) -> El {
    let config = env::source(s, key).and_then(|src| src.state().config.as_ref());
    let kind = machine_kind(
        config.and_then(|c| c["settings"]["environmentIcon"].as_str()),
        config.and_then(|c| c["environment"]["platform"]["machine"].as_str()),
    );
    row()
        .w_full()
        .px(16.0)
        .py(10.0)
        .gap(12.0)
        .center_y()
        .child(icon(machine_icon(kind), 16.0).color(t.c.muted_foreground))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text(env::label(s, key)).font(theme::SM_MEDIUM).color(t.c.foreground).truncate())
                .child(text(transport(s, key)).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate()),
        )
        .child(control)
}

/// Web `FoldedSettingsSection`, header inside the card.
fn folded(t: &T, title: &str, summary: Option<String>, open: bool, toggle: fn(&mut App), control: Option<El>, body: Vec<El>) -> El {
    let mut trigger = row()
        .grow(1.0)
        .min_w(0.0)
        .min_h(44.0)
        .py(8.0)
        .gap(8.0)
        .center_y()
        .focusable(true)
        .role("button")
        .label(title.to_owned())
        .expanded(open)
        .child(icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 16.0).color(t.c.muted_foreground))
        .child(text(title.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground));
    if let Some(summary) = summary {
        trigger = trigger.child(text(summary).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().shrink().min_w(0.0));
    }
    let header = row().w_full().px(16.0).gap(16.0).center_y().child(trigger.on_press(toggle)).children(control);
    let mut card = vec![header];
    if open {
        let mut list = col().w_full();
        for (i, entry) in body.into_iter().enumerate() {
            if i > 0 {
                list = list.child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.5)));
            }
            list = list.child(entry);
        }
        card.push(col().w_full().child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.5))).child(list));
    }
    super::controls::card(card, t)
}

fn note(t: &T, copy: &str) -> El {
    text(copy.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().px(16.0).py(10.0)
}

/// A per-machine select, its popup in `layers`.
fn machine_select(s: &App, t: &T, id: String, options: Vec<(String, &'static str)>, current: String, disabled: bool, label: String, choose: fn(&mut App, &str, &str), layers: &mut Vec<El>) -> El {
    let values: Vec<String> = options.iter().map(|(v, _)| v.clone()).collect();
    let open = s.ui.settings.connections.machines.select.as_deref() == Some(id.as_str());
    let mut select = kit::Select::new(format!("machine-select-{id}"), &values).current(Some(current.as_str())).label(label).size("xs").width(128.0).align_end().open(open && !disabled).disabled(disabled);
    for (value, name) in &options {
        select = select.option_name(value.clone(), *name);
    }
    let toggled = id.clone();
    let key = id.split_once(':').map(|(_, k)| k.to_owned()).unwrap_or_default();
    select
        .on_toggle(move |s: &mut App| {
            let ui = &mut s.ui.settings.connections.machines;
            ui.select = if ui.select.as_deref() == Some(toggled.as_str()) { None } else { Some(toggled.clone()) };
        })
        .on_close(|s: &mut App| s.ui.settings.connections.machines.select = None)
        .on_choose(move |s: &mut App, value: &str, _: &mut rune::Tx| {
            s.ui.settings.connections.machines.select = None;
            choose(s, &key, value);
        })
        .build(&kit::theme(t), layers)
}

// ---------------------------------------------------------------------------------------------
// Load balancing

/// Web `loadPreferenceForWeight`.
fn preference(weight: Option<f64>) -> u8 {
    match weight {
        None => 50,
        Some(w) if w == 50.0 => 50,
        Some(w) if w == 0.0 => 0,
        Some(w) if w < 50.0 => 25,
        Some(_) => 100,
    }
}

fn preference_label(preference: u8) -> &'static str {
    PREFERENCES.iter().find(|(v, _)| *v == preference).map_or("Normal", |(_, l)| l)
}

/// Web `summarizeLoadPreferences`.
fn balance_summary(machines: &[(String, Option<f64>)]) -> Option<String> {
    let parts: Vec<String> = machines
        .iter()
        .filter_map(|(label, weight)| {
            let preference = preference(*weight);
            (preference != 50).then(|| format!("{label} {}", preference_label(preference).to_lowercase()))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn weight(s: &App, key: &str) -> Option<f64> {
    let id = environment_id(s, key)?;
    super::value(s, "loadBalancingWeights")[id.as_str()].as_f64()
}

fn set_weight(s: &mut App, key: &str, value: &str) {
    let (Some(id), Ok(weight)) = (environment_id(s, key), value.parse::<u8>()) else { return };
    let mut weights = super::value(s, "loadBalancingWeights");
    if !weights.is_object() {
        weights = json!({});
    }
    weights[id.as_str()] = json!(weight);
    super::update(s, "loadBalancingWeights", weights);
}

fn load_balancing(s: &App, t: &T, keys: &[String], layers: &mut Vec<El>) -> El {
    let enabled = super::value(s, "loadBalancingEnabled") == true;
    let summary = if enabled {
        balance_summary(&keys.iter().map(|k| (env::label(s, k), weight(s, k))).collect::<Vec<_>>())
    } else {
        Some("Off".into())
    };
    let switch = kit::Switch::new(enabled)
        .label("Automatically balance load")
        .hide_label()
        .on_toggle(move |s: &mut App| super::update(s, "loadBalancingEnabled", json!(!enabled)))
        .build(&kit::theme(t));
    let open = s.ui.settings.connections.machines.balancing_open;
    let mut body = vec![note(t, "New threads in shared projects start on the machine with the most free CPU and memory, weighted by each machine's preference.")];
    if open {
        for key in keys {
            let options = PREFERENCES.iter().map(|(v, l)| (v.to_string(), *l)).collect();
            let control = machine_select(
                s,
                t,
                format!("balance:{key}"),
                options,
                preference(weight(s, key)).to_string(),
                !enabled || environment_id(s, key).is_none(),
                format!("{} load preference", env::label(s, key)),
                set_weight,
                layers,
            );
            body.push(machine_row(s, t, key, control));
        }
    }
    folded(t, "Load balancing", summary, open, |s| s.ui.settings.connections.machines.balancing_open ^= true, Some(switch), body)
}

// ---------------------------------------------------------------------------------------------
// Pull request sharing

fn grants_path() -> std::path::PathBuf {
    super::client::sibling_path("pull-request-routing.json")
}

fn decode_grants(value: &Value) -> Vec<Grant> {
    value["grants"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            let field = |k: &str| g[k].as_str().filter(|v| !v.is_empty()).map(str::to_owned);
            let permission = field("permission").filter(|p| p == "read" || p == "read-write")?;
            Some(Grant { environment_id: field("environmentId")?, connection_key: field("connectionKey")?, permission })
        })
        .collect()
}

fn encode_grants(grants: &[Grant]) -> Value {
    json!({ "version": 1, "grants": grants.iter().map(|g| json!({"environmentId": g.environment_id, "connectionKey": g.connection_key, "permission": g.permission})).collect::<Vec<_>>() })
}

/// Web `pullRequestRoutingConnectionKey`: trust belongs to the saved addresses, sorted, so
/// reordering keeps it but adding or changing an address revokes it.
fn connection_key(id: &str, routes: &[String]) -> Option<String> {
    if routes.is_empty() || routes.iter().any(|r| r.is_empty() || r.split_once("://").is_some_and(|(_, rest)| rest.split('/').next().unwrap_or_default().contains('@'))) {
        return None;
    }
    let mut keys: Vec<String> = routes.iter().map(|r| r.trim_end_matches('/').to_owned()).collect();
    keys.sort();
    Some(json!([id, keys]).to_string())
}

fn machine_connection_key(s: &App, key: &str) -> Option<String> {
    let id = environment_id(s, key)?;
    if key == env::LOCAL {
        return connection_key(&id, &["local".to_owned()]);
    }
    let routes: Vec<String> = env::routes(s, key)
        .iter()
        .map(|r| if env::ssh::is_tunnel(s, key, &r.base) { env::ssh::transport(s, key).unwrap_or_default() } else { r.base.clone() })
        .collect();
    connection_key(&id, &routes)
}

/// Web `pullRequestRoutingPermissionFor`.
fn permission_for<'a>(grants: &'a [Grant], key: Option<&str>) -> &'a str {
    key.and_then(|key| grants.iter().find(|g| g.connection_key == key)).map_or("off", |g| g.permission.as_str())
}

/// Web `summarizePullRequestRouting`.
fn routing_summary(entries: &[(String, &str)]) -> Option<String> {
    let groups: Vec<String> = [("read-write", "read and act"), ("read", "read PRs")]
        .iter()
        .filter_map(|(permission, phrase)| {
            let labels: Vec<&str> = entries.iter().filter(|(_, p)| p == permission).map(|(l, _)| l.as_str()).collect();
            (!labels.is_empty()).then(|| format!("{} {phrase}", labels.join(", ")))
        })
        .collect();
    (!groups.is_empty()).then(|| groups.join(" · "))
}

/// Web `PullRequestRoutingPermissions.set`: one grant per environment, none for Off.
fn granted(mut grants: Vec<Grant>, environment_id: &str, connection_key: String, permission: &str) -> Vec<Grant> {
    grants.retain(|g| g.environment_id != environment_id);
    if permission != "off" {
        grants.push(Grant { environment_id: environment_id.to_owned(), connection_key, permission: permission.to_owned() });
    }
    grants
}

fn set_permission(s: &mut App, key: &str, permission: &str) {
    let (Some(id), Some(connection_key)) = (environment_id(s, key), machine_connection_key(s, key)) else { return };
    let current = s.ui.settings.connections.machines.grants.clone().unwrap_or_default();
    let next = granted(current, &id, connection_key, permission);
    let saved = if rune::is_headless() {
        Ok(())
    } else {
        std::fs::write(grants_path(), serde_json::to_string_pretty(&encode_grants(&next)).unwrap_or_default()).map_err(|e| e.to_string())
    };
    match saved {
        Ok(()) => s.ui.settings.connections.machines.grants = Some(next),
        Err(_) => toast_error(s, "Could not save pull request routing permission", None),
    }
}

fn pull_request_routing(s: &App, t: &T, keys: &[String], layers: &mut Vec<El>) -> El {
    let grants = s.ui.settings.connections.machines.grants.as_deref().unwrap_or_default();
    let entries: Vec<(String, &str)> =
        keys.iter().map(|k| (env::label(s, k), permission_for(grants, machine_connection_key(s, k).as_deref()))).collect();
    let open = s.ui.settings.connections.machines.routing_open;
    let mut body = vec![note(t, "Machines you trust here can read PR data through each other's source control access. Enable both machines. Read and act may use broader permissions than the machine that owns them. This applies only to this device.")];
    if open {
        for (key, (label, permission)) in keys.iter().zip(&entries) {
            let options = ROUTING.iter().map(|(v, l)| ((*v).to_owned(), *l)).collect();
            let control = machine_select(
                s,
                t,
                format!("routing:{key}"),
                options,
                (*permission).to_owned(),
                s.ui.settings.connections.machines.grants.is_none() || machine_connection_key(s, key).is_none(),
                format!("{label} pull request routing"),
                set_permission,
                layers,
            );
            body.push(machine_row(s, t, key, control));
        }
    }
    let summary = routing_summary(&entries).unwrap_or_else(|| "Off".into());
    folded(t, "Pull request sharing", Some(summary), open, |s| s.ui.settings.connections.machines.routing_open ^= true, None, body)
}

/// Web's two sections under Environments, shown with two machines or more.
pub(super) fn sections(s: &App, t: &T, layers: &mut Vec<El>) -> Vec<El> {
    let keys = machines(s);
    if keys.len() < 2 {
        return Vec::new();
    }
    vec![load_balancing(s, t, &keys, layers), pull_request_routing(s, t, &keys, layers)]
}

// ---------------------------------------------------------------------------------------------
// Update all servers

fn semver(version: &str) -> Option<(Vec<u64>, Vec<String>)> {
    let core = version.split('+').next()?;
    let (numbers, pre) = core.split_once('-').map_or((core, None), |(n, p)| (n, Some(p)));
    let numbers: Vec<u64> = numbers.split('.').map(|n| n.parse().ok()).collect::<Option<_>>()?;
    (numbers.len() == 3).then(|| (numbers, pre.map(|p| p.split('.').map(str::to_owned).collect()).unwrap_or_default()))
}

/// Semver precedence; a version that doesn't parse compares by equality only.
fn compare(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    let ((an, ap), (bn, bp)) = (semver(a)?, semver(b)?);
    let by_core = an.cmp(&bn);
    if by_core != Ordering::Equal {
        return Some(by_core);
    }
    Some(match (ap.is_empty(), bp.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => {
            for (x, y) in ap.iter().zip(&bp) {
                let order = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                };
                if order != Ordering::Equal {
                    return Some(order);
                }
            }
            ap.len().cmp(&bp.len())
        }
    })
}

fn channel(version: &str) -> &'static str {
    let pre = version.split_once('-').map(|(_, p)| p).unwrap_or_default();
    let parts: Vec<&str> = pre.split('.').collect();
    match parts.as_slice() {
        [name, date, build] if date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()) && build.parse::<u64>().is_ok() => match *name {
            "nightly" => "nightly",
            "preview" => "preview",
            _ => "stable",
        },
        _ => "stable",
    }
}

/// Web `resolveVersionMismatch`: the server is behind the client.
fn behind(server: &str, client: &str) -> bool {
    let (server, client) = (server.trim(), client.trim());
    if server.is_empty() || client.is_empty() {
        return false;
    }
    let core = |v: &str| v.split(['-', '+']).next().unwrap_or(v).to_owned();
    let nightly = channel(server) == "nightly" && channel(client) == "nightly";
    let (a, b) = if nightly { (server.to_owned(), client.to_owned()) } else { (core(server), core(client)) };
    match compare(&a, &b) {
        Some(order) => order == std::cmp::Ordering::Less,
        None => server != client,
    }
}

/// `crossesReleaseTrack`: Stable and Nightly; preview builds cross nothing.
fn crosses_track(from: &str, to: &str) -> bool {
    let (from, to) = (channel(from), channel(to));
    from != to && from != "preview" && to != "preview"
}

/// Web `savedServerUpdateTargets` and `ServerUpdatesAction`'s `eligible`, for one environment's config.
fn target_for(key: &str, label: &str, config: &Value, client: &str, connected: bool) -> Option<Target> {
    let environment = &config["environment"];
    let server = environment["serverVersion"].as_str()?;
    let self_update = environment["capabilities"]["serverSelfUpdate"].as_str()?;
    let desktop_app = environment["capabilities"]["desktopAppUpdate"] == true;
    if !connected || !behind(server, client) || (self_update == "desktop-managed" && !desktop_app) {
        return None;
    }
    if config["managedPolicy"]["limits"]["updates"]["mode"] == "off" || crosses_track(server, client) {
        return None;
    }
    Some(Target { key: key.to_owned(), label: label.to_owned(), self_update: self_update.to_owned(), target_version: client.trim().to_owned() })
}

/// The version this client ships: the server it bundles.
fn client_version(s: &App) -> Option<String> {
    env::source(s, env::LOCAL)?.state().config.as_ref()?["environment"]["serverVersion"].as_str().map(str::to_owned)
}

fn targets(s: &App) -> Vec<Target> {
    let Some(client) = client_version(s) else { return Vec::new() };
    s.ui.env
        .saved
        .iter()
        .filter(|e| !s.ui.env.off.contains(&e.id))
        .filter_map(|e| {
            let config = env::source(s, &e.id)?.state().config.as_ref()?;
            target_for(&e.id, &e.label, config, &client, env::connected(s, &e.id))
        })
        .collect()
}

fn update_all(s: &mut App) {
    let pending: Vec<String> = s.ui.settings.connections.machines.updating.iter().map(|(_, t)| t.key.clone()).collect();
    let available: Vec<Target> = targets(s).into_iter().filter(|t| !pending.contains(&t.key)).collect();
    if available.iter().any(|t| t.self_update == "desktop-managed") {
        s.ui.settings.connections.machines.confirm = Some(available);
        return;
    }
    start(s, available);
}

/// Web `resolveServerConfigVersionMismatch`: `(server, client)` while this server is behind.
pub(super) fn version_gap(s: &App, key: &str) -> Option<(String, String)> {
    let client = client_version(s)?;
    let server = env::source(s, key)?.state().config.as_ref()?["environment"]["serverVersion"].as_str()?.trim().to_owned();
    behind(&server, &client).then(|| (server, client.trim().to_owned()))
}

/// Web's per-row `ServerUpdateAction`: whether this saved server can update to this client.
pub(super) fn row_update(s: &App, key: &str) -> bool {
    targets(s).iter().any(|t| t.key == key)
}

pub(super) fn row_updating(s: &App, key: &str) -> bool {
    s.ui.settings.connections.machines.updating.iter().any(|(_, t)| t.key == key)
}

pub(super) fn row_failure<'a>(s: &'a App, key: &str) -> Option<&'a str> {
    s.ui.settings.connections.machines.failed.get(key).map(String::as_str)
}

pub(super) fn update_row(s: &mut App, key: &str) {
    if row_updating(s, key) {
        return;
    }
    let Some(target) = targets(s).into_iter().find(|t| t.key == key) else { return };
    if target.self_update == "desktop-managed" {
        s.ui.settings.connections.machines.confirm_row = Some(target);
        return;
    }
    start(s, vec![target]);
}

fn start(s: &mut App, targets: Vec<Target>) {
    for target in targets {
        s.ui.settings.connections.machines.failed.remove(&target.key);
        // Web `canUpdateServer`: the session there must hold environment:maintain.
        if !super::permissions::can_on(s, &target.key, "environment:maintain") {
            continue;
        }
        let Some(id) = env::net_of(s, &target.key).map(|net| net.request("server.updateServer", json!({"targetVersion": target.target_version}))) else { continue };
        s.ui.settings.connections.machines.updating.push((id, target));
    }
}

/// Update all's replies toast like web `useServerUpdate`.
pub(super) fn poll(s: &mut App) -> bool {
    if s.ui.settings.connections.machines.grants.is_none() && s.ui.settings.connections.page.as_deref() == Some("connections") {
        let file = (!rune::is_headless()).then(|| std::fs::read_to_string(grants_path()).ok()).flatten();
        let mut grants = file.and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|v| decode_grants(&v)).unwrap_or_default();
        let local = env::local_id(s);
        grants.retain(|g| s.ui.env.saved.iter().any(|e| e.id == g.environment_id) || local.as_deref() == Some(g.environment_id.as_str()));
        s.ui.settings.connections.machines.grants = Some(grants);
    }
    let mut changed = false;
    let updating = std::mem::take(&mut s.ui.settings.connections.machines.updating);
    for (id, target) in updating {
        let reply = env::net_of(s, &target.key).and_then(|net| net.take_reply(id)).or_else(|| s.src.net().and_then(|net| net.take_reply(id)));
        let Some(reply) = reply else {
            s.ui.settings.connections.machines.updating.push((id, target));
            continue;
        };
        changed = true;
        match reply {
            Ok(result) => {
                let version = result["targetVersion"].as_str().unwrap_or(&target.target_version).to_owned();
                let description = if target.self_update == "desktop-managed" {
                    format!("Desktop app relaunched on {version}.")
                } else {
                    format!("Reconnected on t3@{version}.")
                };
                toast(s, &format!("{} updated", target.label), Some(&description));
            }
            Err(error) => {
                let message = error["message"].as_str().unwrap_or("Server update failed.").to_owned();
                toast_error(s, &format!("{} update failed", target.label), Some(&message));
                s.ui.settings.connections.machines.failed.insert(target.key.clone(), message);
            }
        }
    }
    changed
}

/// The Environments header's Update all, while a saved server is behind this client.
pub(super) fn update_all_button(s: &App, t: &T) -> Option<El> {
    let targets = targets(s);
    if targets.is_empty() {
        return None;
    }
    let pending = !s.ui.settings.connections.machines.updating.is_empty();
    Some(kit::Button::new("Update all").variant("ghost-muted").size("xs").disabled(pending).on_press(update_all).build(t))
}

/// Web `requestConfirmDialog` before desktop apps relaunch.
pub(super) fn confirm_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    if let Some(target) = &s.ui.settings.connections.machines.confirm_row {
        let close = |s: &mut App| s.ui.settings.connections.machines.confirm_row = None;
        kit::Dialog::new("update-row-confirm", format!("Update the T3 Code desktop app that runs the {} server?", target.label))
            .open(true)
            .alert()
            .description("It will close and relaunch on that machine.")
            .on_close(close)
            .child(
                row()
                    .gap(8.0)
                    .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                    .child(
                        kit::Button::new("Confirm")
                            .on_press(|s: &mut App| {
                                if let Some(target) = s.ui.settings.connections.machines.confirm_row.take() {
                                    start(s, vec![target]);
                                }
                            })
                            .build(t),
                    ),
            )
            .build(t, layers);
        return;
    }
    let Some(targets) = &s.ui.settings.connections.machines.confirm else { return };
    let desktops: Vec<&str> = targets.iter().filter(|t| t.self_update == "desktop-managed").map(|t| t.label.as_str()).collect();
    let close = |s: &mut App| s.ui.settings.connections.machines.confirm = None;
    kit::Dialog::new("update-all-confirm", format!("Update the T3 Code desktop apps on {}?", desktops.join(", ")))
        .open(true)
        .alert()
        .description("They will close and relaunch on those machines.")
        .on_close(close)
        .child(
            row()
                .gap(8.0)
                .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                .child(
                    kit::Button::new("Confirm")
                        .on_press(|s: &mut App| {
                            if let Some(targets) = s.ui.settings.connections.machines.confirm.take() {
                                start(s, targets);
                            }
                        })
                        .build(t),
                ),
        )
        .build(t, layers);
}

/// Scenes: two machines with both sections open, or Update all's confirmation.
pub(super) fn scene(s: &mut App, open: &str) {
    let ui = &mut s.ui.settings.connections.machines;
    ui.grants = Some(vec![Grant { environment_id: "env-box".into(), connection_key: connection_key("env-box", &["https://box.tail1234.ts.net".into()]).unwrap(), permission: "read".into() }]);
    match open {
        "update-confirm" => {
            ui.confirm = Some(vec![Target { key: "env-box".into(), label: "Hetzner box".into(), self_update: "desktop-managed".into(), target_version: "0.1.8".into() }]);
        }
        _ => {
            ui.balancing_open = true;
            ui.routing_open = true;
        }
    }
    s.client_settings.values.insert("loadBalancingEnabled".into(), json!(true));
    s.client_settings.values.insert("loadBalancingWeights".into(), json!({"env-box": 100}));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_preferences_and_summaries_read_like_web() {
        assert_eq!(preference(None), 50);
        assert_eq!(preference(Some(0.0)), 0);
        assert_eq!(preference(Some(10.0)), 25);
        assert_eq!(preference(Some(75.0)), 100);
        let machines = [("Box".to_owned(), Some(100.0)), ("Mini".to_owned(), None), ("Pi".to_owned(), Some(0.0))];
        assert_eq!(balance_summary(&machines).as_deref(), Some("Box prefer · Pi manual only"));
        assert_eq!(balance_summary(&machines[1..2]), None);
        let entries = [("Box".to_owned(), "read"), ("Mini".to_owned(), "read-write"), ("Pi".to_owned(), "read")];
        assert_eq!(routing_summary(&entries).as_deref(), Some("Mini read and act · Box, Pi read PRs"));
        assert_eq!(routing_summary(&[("Box".to_owned(), "off")]), None);
    }

    #[test]
    fn pull_request_trust_follows_the_saved_addresses() {
        let a = connection_key("env", &["https://a.ts.net/".into(), "http://10.0.0.2:3773".into()]);
        assert_eq!(a, connection_key("env", &["http://10.0.0.2:3773".into(), "https://a.ts.net".into()]), "reordering keeps trust");
        assert_ne!(a, connection_key("env", &["https://a.ts.net".into()]), "a changed route list revokes it");
        assert_eq!(connection_key("env", &["https://user:pw@a.ts.net".into()]), None);
        let grants = granted(Vec::new(), "env", a.clone().unwrap(), "read");
        assert_eq!(permission_for(&grants, a.as_deref()), "read");
        assert_eq!(permission_for(&grants, connection_key("env", &["https://b.ts.net".into()]).as_deref()), "off");
        let grants = granted(grants, "env", a.clone().unwrap(), "off");
        assert!(grants.is_empty(), "Off removes the grant");
        assert_eq!(decode_grants(&encode_grants(&granted(Vec::new(), "env", a.unwrap(), "read-write"))).len(), 1);
    }

    #[test]
    fn update_all_targets_servers_behind_this_client_like_web() {
        assert!(behind("0.1.6", "0.1.7"));
        assert!(!behind("0.1.7", "0.1.7-nightly.20261101.1"), "a stable core equal to the client's is current");
        assert!(behind("0.1.8-nightly.20261101.1", "0.1.8-nightly.20261102.1"));
        assert!(!behind("0.1.8", "0.1.7"));
        assert!(crosses_track("0.1.7", "0.1.8-nightly.20261101.1"));
        assert!(!crosses_track("0.1.7-preview.20261101.1", "0.1.8-nightly.20261101.1"));
        let config = |update: &str, desktop: bool, mode: &str| {
            json!({"environment": {"serverVersion": "0.1.6", "capabilities": {"serverSelfUpdate": update, "desktopAppUpdate": desktop}}, "managedPolicy": {"limits": {"updates": {"mode": mode}}}})
        };
        let target = target_for("k", "Box", &config("binary", false, "auto"), "0.1.7", true).unwrap();
        assert_eq!((target.target_version.as_str(), target.self_update.as_str()), ("0.1.7", "binary"));
        assert!(target_for("k", "Box", &config("binary", false, "auto"), "0.1.7", false).is_none(), "not connected");
        assert!(target_for("k", "Box", &config("desktop-managed", false, "auto"), "0.1.7", true).is_none());
        assert!(target_for("k", "Box", &config("desktop-managed", true, "auto"), "0.1.7", true).is_some());
        assert!(target_for("k", "Box", &config("binary", false, "off"), "0.1.7", true).is_none(), "managed updates off");
        assert!(target_for("k", "Box", &config("binary", false, "auto"), "0.1.6", true).is_none(), "current");
    }
}
