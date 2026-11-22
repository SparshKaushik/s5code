//! Saved environments: other T3 servers this window pairs with (apps/web Settings > Connections >
//! Environments, "Add environment"). Each keeps its own s5_net client and stays connected.
//!
//! Like the web, the sidebar, palette and search list every environment at once. `App::src` is the
//! environment that owns what's on screen (the route's thread or project; this machine otherwise),
//! so the surfaces that read `s.state()` or send through `s.src.net()` reach the right server. The
//! other environments' project and thread rows are copied into its lists, tagged with their owner
//! ([`TAG`]); thread views stay with their own server. Request ids are process-unique, and replies
//! from every environment are handed to `App::src`, where the UI polls for them.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use rune::{El, TextEdit, col, row, text};
use serde_json::{Value, json};

use crate::data::Source;
use crate::theme::{self, T};
use crate::{App, Route, kit};

pub mod ssh;
mod catalog;

/// The key of this machine's own server in `active` and `parked`.
pub const LOCAL: &str = "";
/// Marks a row copied in from another environment, holding that environment's key.
pub(crate) const TAG: &str = "_environment";
/// The credential of a route reached through T3 Connect (web profile `authorization:
/// "t3-connect"`): its socket mints a DPoP ticket per attempt instead of sending a bearer.
pub const T3_CONNECT: &str = "t3-connect";

#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    pub id: String,
    pub label: String,
    pub base: String,
    pub bearer: String,
}

/// Another way to reach a saved environment, after its [`Saved::base`] (web `alternateRoutes`).
#[derive(Clone, Debug, PartialEq)]
pub struct Alt {
    pub base: String,
    pub bearer: String,
}

#[derive(Default)]
pub struct Ui {
    pub saved: Vec<Saved>,
    /// Each saved environment's routes after its first, preferred first.
    pub alternates: HashMap<String, Vec<Alt>>,
    /// When each environment last fell back to another route.
    fell_back: HashMap<String, std::time::Instant>,
    /// Checks of the routes preferred over the one in use, by environment.
    better: HashMap<String, Better>,
    /// The environment a pairing in flight adds a route to, and the label of one just added.
    pub route_for: Option<String>,
    pub route_added: Option<String>,
    /// Connected environments that are not on screen, by key.
    parked: Vec<(String, Source)>,
    /// The environment `App::src` belongs to.
    pub active: String,
    pub host: TextEdit,
    pub code: TextEdit,
    pairing: Option<Receiver<Result<(Saved, Option<Value>), String>>>,
    pub error: Option<String>,
    /// The environment the last finished pairing added, for the setup wizard to select.
    pub last_paired: Option<String>,
    /// Each parked environment's `shell_revision` when its rows were last copied in.
    merged: HashMap<String, u64>,
    /// Rows copied into `App::src`, and which environment owns each copied id.
    foreign: usize,
    owners: HashMap<String, String>,
    pub ssh: ssh::Ui,
    /// Saved environments switched off in Settings: kept, but not connected.
    pub off: Vec<String>,
    /// Saved servers older than this client that can update themselves (web `serverUpdateRequired`),
    /// with the descriptor they paired with. They connect for requests only, so they can update.
    pub outdated: HashMap<String, Value>,
    /// Each outdated server's descriptor, fetched again once per reconnect: `None` once read.
    recheck: HashMap<String, Option<Receiver<Result<Value, String>>>>,
}

fn path() -> PathBuf {
    match std::env::var_os("S5_ENVIRONMENTS") {
        Some(path) => path.into(),
        None => crate::settings::client::sibling_path("environments.json"),
    }
}

fn decode(value: &Value) -> Vec<Saved> {
    let field = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_owned();
    value["environments"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| Saved { id: field(v, "environmentId"), label: field(v, "label"), base: field(v, "httpBaseUrl"), bearer: field(v, "bearerToken") })
        .filter(|e| !e.id.is_empty() && !e.base.is_empty() && !e.bearer.is_empty())
        .collect()
}

/// Web's catalog `enabled: false` rows.
fn decode_off(value: &Value) -> Vec<String> {
    value["environments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["enabled"] == false)
        .filter_map(|v| v["environmentId"].as_str().map(str::to_owned))
        .collect()
}

fn decode_alternates(value: &Value) -> HashMap<String, Vec<Alt>> {
    let field = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_owned();
    value["environments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let routes: Vec<Alt> = v["alternateRoutes"]
                .as_array()?
                .iter()
                .map(|r| Alt { base: field(r, "httpBaseUrl"), bearer: field(r, "bearerToken") })
                .filter(|r| !r.base.is_empty() && !r.bearer.is_empty())
                .collect();
            (!routes.is_empty()).then(|| (field(v, "environmentId"), routes))
        })
        .collect()
}

/// Web's catalog `serverUpdateRequired` rows, with the descriptor Rune keeps to offer the update.
fn decode_outdated(value: &Value) -> HashMap<String, Value> {
    value["environments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["serverUpdateRequired"] == true && v["descriptor"].is_object())
        .filter_map(|v| Some((v["environmentId"].as_str()?.to_owned(), v["descriptor"].clone())))
        .collect()
}

fn encode(saved: &[Saved], off: &[String], alternates: &HashMap<String, Vec<Alt>>, outdated: &HashMap<String, Value>) -> Value {
    let rows: Vec<Value> = saved
        .iter()
        .map(|e| {
            let mut row = json!({"environmentId": e.id, "label": e.label, "httpBaseUrl": e.base, "bearerToken": e.bearer});
            if off.contains(&e.id) {
                row["enabled"] = json!(false);
            }
            if let Some(routes) = alternates.get(&e.id).filter(|r| !r.is_empty()) {
                row["alternateRoutes"] = routes.iter().map(|r| json!({"httpBaseUrl": r.base, "bearerToken": r.bearer})).collect();
            }
            if let Some(descriptor) = outdated.get(&e.id) {
                row["unsupportedReason"] = json!(outdated_reason(descriptor, &e.label));
                row["serverUpdateRequired"] = json!(true);
                row["descriptor"] = descriptor.clone();
            }
            row
        })
        .collect();
    json!({ "version": 1, "environments": rows })
}

/// Bearer tokens are credentials: the file is the user's alone. Electron's catalog follows.
fn save(s: &App) -> Result<(), String> {
    let ui = &s.ui.env;
    let path = path();
    let old = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()).map(|v| decode(&v)).unwrap_or_default();
    write(&path, &ui.saved, &ui.off, &ui.alternates, &ui.outdated)?;
    let saved = &ui.saved;
    catalog::export(&old, saved);
    Ok(())
}

fn write(path: &std::path::Path, saved: &[Saved], off: &[String], alternates: &HashMap<String, Vec<Alt>>, outdated: &HashMap<String, Value>) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let temporary = path.with_extension("json.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut file, serde_json::to_string_pretty(&encode(saved, off, alternates, outdated)).unwrap_or_default().as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())
}

fn connect(s: &App, env: &Saved) -> Source {
    connect_to(s, &env.id, &env.base, &env.bearer)
}

/// An outdated server would refuse this client's subscriptions, so it only carries requests.
fn connect_to(s: &App, id: &str, base: &str, bearer: &str) -> Source {
    let waker = s.ui.panel.waker.clone();
    let wake = move || {
        if let Some(w) = &waker {
            w()
        }
    };
    Source::Live(if bearer == T3_CONNECT {
        s5_net::Net::connect_with(base, crate::cloud::socket_auth(id), wake)
    } else if s.ui.env.outdated.contains_key(id) {
        s5_net::Net::connect_requests_only(base, bearer, wake)
    } else {
        s5_net::Net::connect(base, bearer, wake)
    })
}

/// Loads the saved environments and connects each in the background; this machine stays active.
pub fn start(s: &mut App) {
    let file = std::fs::read_to_string(path()).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let saved = file.as_ref().map(decode).unwrap_or_default();
    let off = file.as_ref().map(decode_off).unwrap_or_default();
    s.ui.env.alternates.extend(file.as_ref().map(decode_alternates).unwrap_or_default());
    s.ui.env.outdated.extend(file.as_ref().map(decode_outdated).unwrap_or_default());
    for env in saved.iter().filter(|e| !off.contains(&e.id)) {
        let src = connect(s, env);
        s.ui.env.parked.push((env.id.clone(), src));
    }
    s.ui.env.saved.extend(saved);
    s.ui.env.off = off;
    ssh::start(s);
    if !rune::is_headless() {
        std::thread::spawn(|| *CATALOG.lock().unwrap() = Some(catalog::read()));
    }
}

/// Electron's saved connections, read off the UI thread at launch.
static CATALOG: std::sync::Mutex<Option<Vec<Saved>>> = std::sync::Mutex::new(None);

/// Connections paired in the Electron app that this list lacks join it, and connect.
fn adopt_catalog(s: &mut App) -> bool {
    let Some(found) = CATALOG.lock().unwrap().take() else { return false };
    let new: Vec<Saved> = found.into_iter().filter(|e| !s.ui.env.saved.iter().any(|x| x.id == e.id)).collect();
    if new.is_empty() {
        return false;
    }
    for env in new {
        let src = connect(s, &env);
        s.ui.env.parked.push((env.id.clone(), src));
        s.ui.env.saved.push(env);
    }
    s.ui.env.error = save(s).err().map(|e| format!("Could not save environments: {e}"));
    true
}

/// This machine's server, wherever it is: `App::src` while active, else parked.
pub fn local_mut(s: &mut App) -> &mut Source {
    if s.ui.env.active == LOCAL {
        return &mut s.src;
    }
    let ui = &mut s.ui.env;
    let i = match ui.parked.iter().position(|(k, _)| k == LOCAL) {
        Some(i) => i,
        None => {
            ui.parked.push((LOCAL.into(), Source::Fixture(crate::data::empty_state())));
            ui.parked.len() - 1
        }
    };
    &mut ui.parked[i].1
}

pub fn source<'a>(s: &'a App, key: &str) -> Option<&'a Source> {
    if s.ui.env.active == key {
        return Some(&s.src);
    }
    s.ui.env.parked.iter().find(|(k, _)| k == key).map(|(_, src)| src)
}

pub fn source_mut<'a>(s: &'a mut App, key: &str) -> Option<&'a mut Source> {
    if s.ui.env.active == key {
        return Some(&mut s.src);
    }
    s.ui.env.parked.iter_mut().find(|(k, _)| k == key).map(|(_, src)| src)
}

/// Scenes stand a fixture in for a saved environment's connection.
pub fn park(s: &mut App, key: &str, src: Source) {
    s.ui.env.parked.push((key.into(), src));
}

pub fn local_id(s: &App) -> Option<String> {
    source(s, LOCAL)?.state().config.as_ref()?["environment"]["environmentId"].as_str().map(str::to_owned)
}

pub fn label(s: &App, key: &str) -> String {
    if key == LOCAL {
        let label = source(s, LOCAL).and_then(|src| src.state().config.as_ref()).and_then(|c| c["environment"]["label"].as_str()).unwrap_or("This machine");
        return label.to_owned();
    }
    s.ui.env.saved.iter().find(|e| e.id == key).map(|e| e.label.clone()).unwrap_or_default()
}

/// What the row says about an environment's connection.
pub fn status(s: &App, key: &str) -> String {
    if let Some(descriptor) = s.ui.env.outdated.get(key) {
        let label = s.ui.env.saved.iter().find(|e| e.id == key).map_or("", |e| e.label.as_str());
        return outdated_reason(descriptor, label);
    }
    let Some(src) = source(s, key) else { return "Not connected".into() };
    let st = src.state();
    match &st.conn {
        s5_net::Conn::Open if st.shell_ready => "Connected".into(),
        s5_net::Conn::Open | s5_net::Conn::Connecting => "Connecting...".into(),
        // connectionStatusText (client-runtime/connection/presentation.ts); a rejected credential is
        // the web's blocked "authentication" failure, which retrying cannot fix, so it asks to pair again.
        s5_net::Conn::Closed(reason) if rejected(reason) => format!("Connection failed. Reason: {reason}. Pair this environment again."),
        s5_net::Conn::Closed(reason) if reason.is_empty() => "Reconnecting...".into(),
        s5_net::Conn::Closed(reason) => format!("Failed to connect. Reconnecting... Reason: {reason}"),
    }
}

/// The setup wizard's one-word state (`presentSavedCloudEnvironmentConnection`'s `buttonLabel`).
pub fn short_status(s: &App, key: &str) -> &'static str {
    if s.ui.env.outdated.contains_key(key) {
        return "Client not supported";
    }
    let Some(src) = source(s, key) else { return "Not connected" };
    let st = src.state();
    match &st.conn {
        s5_net::Conn::Open if st.shell_ready => "Connected",
        s5_net::Conn::Open | s5_net::Conn::Connecting => "Connecting…",
        s5_net::Conn::Closed(reason) if rejected(reason) => "Connection failed",
        s5_net::Conn::Closed(_) => "Reconnecting…",
    }
}

/// Some route of the saved environment goes through T3 Connect.
pub fn reached_by_relay(s: &App, id: &str) -> bool {
    routes(s, id).iter().any(|r| r.bearer == T3_CONNECT)
}

/// Saves an account computer reached through T3 Connect: `Some(true)` for a new environment,
/// `Some(false)` for a fallback route on one saved over another address, `None` when it
/// already had one.
pub(crate) fn add_relay(s: &mut App, linked: &s5_net::cloud::relay::Linked) -> Option<bool> {
    let base = linked.http.trim_end_matches('/').to_owned();
    if s.ui.env.saved.iter().any(|e| e.id == linked.id) {
        if reached_by_relay(s, &linked.id) {
            return None;
        }
        let mut all = routes(s, &linked.id);
        all.push(Alt { base, bearer: T3_CONNECT.into() });
        set_routes(s, &linked.id, all);
        return Some(false);
    }
    let label = if linked.label.is_empty() { linked.id.clone() } else { linked.label.clone() };
    let env = Saved { id: linked.id.clone(), label, base, bearer: T3_CONNECT.into() };
    let src = connect(s, &env);
    s.ui.env.parked.push((env.id.clone(), src));
    s.ui.env.saved.push(env);
    s.ui.env.error = persist(s);
    Some(true)
}

/// The base URL a saved environment was paired at.
pub fn base_url(s: &App, key: &str) -> Option<String> {
    s.ui.env.saved.iter().find(|e| e.id == key).map(|e| e.base.clone())
}

/// The server refused the bearer during the WebSocket upgrade (tungstenite's "HTTP error: 401 …").
pub(crate) fn rejected(reason: &str) -> bool {
    reason.starts_with("HTTP error: 401") || reason.starts_with("HTTP error: 403")
}

/// Makes `key`'s environment the one `App::src` talks to; the others' rows are copied back in.
pub fn switch(s: &mut App, key: &str) {
    // An outdated server carries only update requests, so it has no state to show.
    if s.ui.env.active == key || s.ui.env.outdated.contains_key(key) {
        return;
    }
    let Some(i) = s.ui.env.parked.iter().position(|(k, _)| k == key) else { return };
    let (_, next) = s.ui.env.parked.remove(i);
    let mut previous = std::mem::replace(&mut s.src, next);
    strip(state_mut(&mut previous));
    let old = std::mem::replace(&mut s.ui.env.active, key.to_owned());
    s.ui.env.parked.push((old, previous));
    s.ui.env.merged.clear();
    merge(s);
}

/// The environment that owns `route`: its thread's or project's, else this machine's.
pub fn follow(s: &mut App, route: &Route) {
    let key = match route {
        Route::Thread(id) | Route::Home { project_id: Some(id) } => owner(s, id),
        Route::Home { project_id: None } => crate::chat::home::default_project(s).map_or_else(|| LOCAL.to_owned(), |p| of(s, p)),
        _ => LOCAL.to_owned(),
    };
    switch(s, &key);
}

/// Which environment a thread or project id belongs to.
pub fn owner(s: &App, id: &str) -> String {
    s.ui.env.owners.get(id).cloned().unwrap_or_else(|| s.ui.env.active.clone())
}

/// The environment a sidebar row came from: its tag, or the one on screen.
pub fn of(s: &App, row: &Value) -> String {
    row[TAG].as_str().map_or_else(|| s.ui.env.active.clone(), str::to_owned)
}

/// Whether rows from more than one environment are listed (the web's `projectGroupsSpanEnvironments`).
pub fn spans(s: &App) -> bool {
    s.ui.env.foreign > 0
}

/// The web's command palette location: "Local" for this machine, else the environment's label.
pub fn location(s: &App, row: &Value) -> String {
    match of(s, row) {
        key if key == LOCAL => "Local".into(),
        key => label(s, &key),
    }
}

/// `ProjectEnvironmentBadge`: "On X" when every member project is remote, "Also on X" when one
/// lives on this machine; None when none is remote.
pub fn badge(s: &App, members: &[String]) -> Option<String> {
    let keys: Vec<String> = members.iter().map(|m| owner(s, m)).collect();
    let mut labels: Vec<String> = keys.iter().filter(|k| *k != LOCAL).map(|k| Some(label(s, k)).filter(|l| !l.is_empty()).unwrap_or_else(|| "Remote".into())).collect();
    if labels.is_empty() {
        return None;
    }
    labels.sort();
    labels.dedup();
    let also = keys.iter().any(|k| k == LOCAL);
    Some(format!("{} {}", if also { "Also on" } else { "On" }, labels.join(", ")))
}

/// The environment a row lives on, when that's not this machine.
#[cfg(test)]
pub fn remote_label(s: &App, row: &Value) -> Option<String> {
    let key = of(s, row);
    (key != LOCAL).then(|| label(s, &key))
}

/// Every environment this window knows: this machine first, then the saved ones by label. With
/// the local environment off there is no primary environment, as web's `readPrimaryEnvironmentTarget`.
pub fn keys(s: &App) -> Vec<String> {
    let mut saved: Vec<&Saved> = s.ui.env.saved.iter().collect();
    saved.sort_by(|a, b| a.label.cmp(&b.label));
    let local = (!s.ui.desktop.local_off).then(|| LOCAL.to_owned());
    local.into_iter().chain(saved.into_iter().map(|e| e.id.clone())).collect()
}

/// Whether the environment is connected and has its shell (`canCreateProjectInEnvironment`).
pub fn connected(s: &App, key: &str) -> bool {
    source(s, key).is_some_and(|src| matches!(src.state().conn, s5_net::Conn::Open) && src.state().shell_ready)
}

/// The client of the environment `key`, on screen or parked.
pub fn net_of<'a>(s: &'a mut App, key: &str) -> Option<&'a mut s5_net::Net> {
    source_mut(s, key)?.net()
}

/// Records that `id` belongs to `key` before that environment's shell lists it, so navigating to
/// a project a flow just created there lands on the right server.
pub fn adopt(s: &mut App, id: &str, key: &str) {
    s.ui.env.owners.insert(id.to_owned(), key.to_owned());
}

/// Sends one request to every connected environment; each reply reaches `App::src` (see [`drain`]).
pub fn request_all(s: &mut App, method: &str, params: &Value) -> Vec<u64> {
    let mut ids: Vec<u64> = s.src.net().map(|net| net.request(method, params.clone())).into_iter().collect();
    for (_, src) in &mut s.ui.env.parked {
        if let Some(net) = src.net() {
            ids.push(net.request(method, params.clone()));
        }
    }
    ids
}

/// The client to send a command about `id` through; an id no environment lists goes to `App::src`.
pub fn net_for<'a>(s: &'a mut App, id: &str) -> Option<&'a mut s5_net::Net> {
    let key = owner(s, id);
    if key == s.ui.env.active {
        return s.src.net();
    }
    s.ui.env.parked.iter_mut().find(|(k, _)| *k == key).and_then(|(_, src)| src.net())
}

fn state_mut(src: &mut Source) -> &mut s5_net::State {
    match src {
        Source::Live(net) => net.state_mut(),
        Source::Fixture(state) => state,
    }
}

fn lists(state: &mut s5_net::State) -> [&mut Vec<Value>; 3] {
    [&mut state.projects, &mut state.threads, &mut state.archived]
}

fn strip(state: &mut s5_net::State) {
    for rows in lists(state) {
        rows.retain(|r| r.get(TAG).is_none());
    }
}

/// Copies the parked environments' rows into `App::src` when any of them moved, or when a shell
/// snapshot on `App::src` replaced its lists and dropped them.
fn merge(s: &mut App) -> bool {
    let env = &mut s.ui.env;
    if env.parked.is_empty() && env.merged.is_empty() {
        return false;
    }
    let moved = env.merged.len() != env.parked.len() || env.parked.iter().any(|(k, src)| env.merged.get(k) != Some(&src.state().shell_revision));
    let present = s.src.state().projects.iter().chain(&s.src.state().threads).chain(&s.src.state().archived).filter(|r| r.get(TAG).is_some()).count();
    if !moved && present == env.foreign {
        return false;
    }
    let state = state_mut(&mut s.src);
    strip(state);
    env.owners.clear();
    env.merged.clear();
    env.foreign = 0;
    for (key, src) in &env.parked {
        let theirs = src.state();
        env.merged.insert(key.clone(), theirs.shell_revision);
        let [projects, threads, archived] = lists(state);
        for (from, into) in [(&theirs.projects, projects), (&theirs.threads, threads), (&theirs.archived, archived)] {
            for row in from {
                let mut row = row.clone();
                row[TAG] = Value::String(key.clone());
                env.owners.insert(crate::data::s(&row, "id").to_owned(), key.clone());
                into.push(row);
                env.foreign += 1;
            }
        }
    }
    true
}

/// Drains every environment, hands their replies to `App::src`, and refreshes the merged lists.
pub fn drain(s: &mut App) -> bool {
    let mut changed = s.src.net().is_some_and(|net| net.drain());
    for (_, src) in &mut s.ui.env.parked {
        if let Some(net) = src.net() {
            changed |= net.drain();
            if let Source::Live(to) = &mut s.src {
                net.hand_off(to);
            }
        }
    }
    merge(s) | changed
}

pub fn remove(s: &mut App, id: &str) {
    if s.ui.env.active == id {
        s.navigate(Route::Home { project_id: None });
        switch(s, LOCAL);
    }
    s.ui.env.parked.retain(|(k, _)| k != id);
    s.ui.env.saved.retain(|e| e.id != id);
    s.ui.env.off.retain(|k| k != id);
    s.ui.env.alternates.remove(id);
    s.ui.env.outdated.remove(id);
    s.ui.env.error = persist(s);
    ssh::forget(s, id);
    saving_failed(s, "Could not remove backend");
}

/// Web toasts a failed save from the Connections list, where no dialog shows the error.
fn saving_failed(s: &mut App, title: &str) {
    if let Some(error) = s.ui.env.error.clone() {
        crate::shell::palette::toast_error(s, title, Some(&error));
    }
}

/// Writes the saved environments, returning the error the settings page shows.
pub(crate) fn persist(s: &App) -> Option<String> {
    save(s).err().map(|e| format!("Could not save environments: {e}"))
}

/// Web `setEnabled` on a saved environment: off disconnects it and keeps it saved; on reconnects.
pub fn set_enabled(s: &mut App, id: &str, enabled: bool) {
    let Some(env) = s.ui.env.saved.iter().find(|e| e.id == id).cloned() else { return };
    if enabled == !s.ui.env.off.iter().any(|k| k == id) {
        return;
    }
    if enabled {
        s.ui.env.off.retain(|k| k != id);
        let src = connect(s, &env);
        s.ui.env.parked.push((env.id.clone(), src));
        ssh::resume(s, id);
    } else {
        if s.ui.env.active == id {
            s.navigate(Route::Home { project_id: None });
            switch(s, LOCAL);
        }
        s.ui.env.parked.retain(|(k, _)| k != id);
        ssh::suspend(s, id);
        s.ui.env.off.push(id.to_owned());
    }
    s.ui.env.error = persist(s);
    saving_failed(s, if enabled { "Could not switch backend on" } else { "Could not switch backend off" });
}

/// `ORCHESTRATION_PROTOCOL_VERSION`, which the transport asks for.
const PROTOCOL: u64 = 2;

/// A saved environment as pairing sees it: id, label and the addresses of its routes.
type Known = (String, String, Vec<String>);

/// Why `preparePairingRegistration` refuses a server before spending the code: an address
/// claiming a saved environment it isn't a route of (`assertKnownAddress`, skipped when adding
/// a route), or another orchestration protocol (`orchestrationProtocolCompatibilityError`).
fn pairing_refusal(descriptor: &Value, base: &str, known: &[Known], adding_route: bool) -> Option<String> {
    let id = crate::data::s(descriptor, "environmentId");
    let same = |a: &str| a.trim_end_matches('/') == base.trim_end_matches('/');
    if !adding_route
        && let Some((_, label, bases)) = known.iter().find(|(k, ..)| k == id)
        && !bases.iter().any(|b| same(b))
    {
        return Some(format!("That address claims to be {label}, which is already saved. Add it as a route from that environment's settings instead."));
    }
    let label = descriptor["label"].as_str().filter(|l| !l.is_empty()).unwrap_or(base);
    match protocol(descriptor) {
        PROTOCOL => None,
        v if v > PROTOCOL => Some(format!("This client is not supported by this server. Update your app or use a compatible release to connect to {label}.")),
        // An outdated server is still saved so it can be updated from this client.
        _ if can_self_update(descriptor) => None,
        _ => Some(outdated_reason(descriptor, label)),
    }
}

/// Servers shipped before negotiation speak the first protocol.
fn protocol(descriptor: &Value) -> u64 {
    descriptor["orchestrationProtocolVersion"].as_u64().unwrap_or(1)
}

/// Web `canSelfUpdate` on a descriptor.
fn can_self_update(descriptor: &Value) -> bool {
    let capabilities = &descriptor["capabilities"];
    capabilities["serverSelfUpdate"].as_str().is_some_and(|m| m != "desktop-managed" || capabilities["desktopAppUpdate"] == true)
}

fn outdated_reason(descriptor: &Value, fallback: &str) -> String {
    let label = descriptor["label"].as_str().filter(|l| !l.is_empty()).unwrap_or(fallback);
    format!("This client requires a newer server. Update T3 Code on {label} to connect.")
}

/// The descriptor of a saved server that needs updating before it can connect, if `key` is one.
pub(crate) fn outdated<'a>(s: &'a App, key: &str) -> Option<&'a Value> {
    s.ui.env.outdated.get(key)
}

/// An outdated server finished updating: it connects as any other from now on.
pub(crate) fn server_updated(s: &mut App, key: &str) {
    if s.ui.env.outdated.remove(key).is_none() {
        return;
    }
    if let Some(env) = s.ui.env.saved.iter().find(|e| e.id == key).cloned()
        && !s.ui.env.off.contains(&env.id)
    {
        let src = connect(s, &env);
        if let Some(slot) = source_mut(s, key) {
            *slot = src;
        }
    }
    s.ui.env.error = persist(s);
}

/// Web `updateOutdatedHost`'s compatible check: a server updated by hand connects again. Each time
/// an outdated server's socket opens, its descriptor is read once more.
fn recheck_outdated(s: &mut App) -> bool {
    let keys: Vec<String> = s.ui.env.outdated.keys().cloned().collect();
    s.ui.env.recheck.retain(|k, _| keys.contains(k));
    let mut changed = false;
    for key in keys {
        let base = match source(s, &key) {
            Some(Source::Live(net)) if net.state().conn == s5_net::Conn::Open => net.base_url().to_owned(),
            _ => {
                s.ui.env.recheck.remove(&key);
                continue;
            }
        };
        match s.ui.env.recheck.get(&key).map(Option::is_some) {
            None if !rune::is_headless() => {
                let (tx, rx) = channel();
                let waker = s.ui.panel.waker.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(s5_net::fetch_descriptor(&base));
                    if let Some(w) = waker {
                        w();
                    }
                });
                s.ui.env.recheck.insert(key, Some(rx));
            }
            Some(true) => {
                let Some(result) = s.ui.env.recheck.get(&key).and_then(Option::as_ref).and_then(|rx| rx.try_recv().ok()) else { continue };
                s.ui.env.recheck.insert(key.clone(), None);
                changed |= recheck_landed(s, &key, result);
            }
            _ => {}
        }
    }
    changed
}

fn recheck_landed(s: &mut App, key: &str, result: Result<Value, String>) -> bool {
    let Ok(descriptor) = result else { return false };
    if descriptor["environmentId"] == key && protocol(&descriptor) == PROTOCOL {
        server_updated(s, key);
        return true;
    }
    false
}

/// `preparePairingRegistration`: the descriptor first, so a wrong address never spends the code.
/// An outdated server pairs with its descriptor, to be updated before it connects.
fn pair(host: String, code: String, known: Vec<Known>, adding_route: bool) -> Result<(Saved, Option<Value>), String> {
    let (base, credential) = s5_net::resolve_pairing(&host, &code)?;
    let descriptor = s5_net::fetch_descriptor(&base)?;
    if let Some(refusal) = pairing_refusal(&descriptor, &base, &known, adding_route) {
        return Err(refusal);
    }
    let bearer = s5_net::exchange_bootstrap(&base, &credential).map_err(|e| format!("The pairing code was refused: {e}"))?;
    let id = descriptor["environmentId"].as_str().unwrap_or_default().to_owned();
    let label = descriptor["label"].as_str().filter(|l| !l.is_empty()).unwrap_or(&base).to_owned();
    let outdated = (protocol(&descriptor) < PROTOCOL).then_some(descriptor);
    Ok((Saved { id, label, base, bearer }, outdated))
}

pub fn pairing(s: &App) -> bool {
    s.ui.env.pairing.is_some()
}

/// Web's Add route: pairs `id`'s machine again at another address.
pub(crate) fn add_route(s: &mut App, id: &str) {
    if s.ui.env.pairing.is_some() {
        return;
    }
    s.ui.env.route_for = Some(id.to_owned());
    add(s);
    if s.ui.env.pairing.is_none() {
        s.ui.env.route_for = None;
    }
}

/// Web's Add route over SSH: the tunnel joins `id`'s routes when it reaches the same machine.
pub(crate) fn add_ssh_route(s: &mut App, id: &str) {
    if s.ui.env.pairing.is_some() || ssh::adding(s) {
        return;
    }
    s.ui.env.route_for = Some(id.to_owned());
    ssh::add(s);
    if !ssh::adding(s) {
        s.ui.env.route_for = None;
    }
}

pub(crate) fn add(s: &mut App) {
    let (host, code) = (s.ui.env.host.text.clone(), s.ui.env.code.text.clone());
    start_pairing(s, host, code);
}

/// Pairs from a pairing link alone, as the setup wizard's single field does.
pub(crate) fn pair_link(s: &mut App, link: &str) {
    start_pairing(s, link.trim().to_owned(), String::new());
}

fn start_pairing(s: &mut App, host: String, code: String) {
    if s.ui.env.pairing.is_some() {
        return;
    }
    s.ui.env.last_paired = None;
    if let Err(e) = s5_net::resolve_pairing(&host, &code) {
        s.ui.env.error = Some(e);
        return;
    }
    let known: Vec<Known> = s.ui.env.saved.iter().map(|e| (e.id.clone(), e.label.clone(), routes(s, &e.id).into_iter().map(|r| r.base).collect())).collect();
    let adding_route = s.ui.env.route_for.is_some();
    let (tx, rx) = channel();
    let waker = s.ui.panel.waker.clone();
    std::thread::spawn(move || {
        let _ = tx.send(pair(host, code, known, adding_route));
        if let Some(w) = waker {
            w();
        }
    });
    s.ui.env.error = None;
    s.ui.env.pairing = Some(rx);
}

/// Folds a finished pairing in, replacing an environment saved before (a fresh session).
fn paired(s: &mut App, result: Result<(Saved, Option<Value>), String>) {
    if let Some(id) = s.ui.env.route_for.take() {
        match result.and_then(|(env, _)| route_paired(s, &id, env)) {
            Ok(label) => {
                s.ui.env.host.clear();
                s.ui.env.code.clear();
                s.ui.env.route_added = Some(label);
            }
            Err(e) => s.ui.env.error = Some(e),
        }
        return;
    }
    let env = match result {
        Ok((env, _)) if Some(&env.id) == local_id(s).as_ref() => Err("That address reaches this machine's own server.".to_owned()),
        other => other,
    };
    let (env, outdated) = match env {
        Ok(paired) => paired,
        Err(e) => {
            s.ui.env.error = Some(e);
            return;
        }
    };
    if s.ui.env.active == env.id {
        s.navigate(Route::Home { project_id: None });
        switch(s, LOCAL);
    }
    s.ui.env.parked.retain(|(k, _)| *k != env.id);
    match outdated {
        Some(descriptor) => s.ui.env.outdated.insert(env.id.clone(), descriptor),
        None => s.ui.env.outdated.remove(&env.id),
    };
    let src = connect(s, &env);
    s.ui.env.parked.push((env.id.clone(), src));
    s.ui.env.saved.retain(|e| e.id != env.id);
    s.ui.env.off.retain(|k| *k != env.id);
    s.ui.env.last_paired = Some(env.id.clone());
    s.ui.env.saved.push(env);
    s.ui.env.host.clear();
    s.ui.env.code.clear();
    s.ui.env.error = persist(s);
}

/// Web `upsertRoute`: the route joins `id`'s, first among its kind, or replaces one at the same
/// address with its fresh credential.
fn route_paired(s: &mut App, id: &str, paired: Saved) -> Result<String, String> {
    let Some(saved) = s.ui.env.saved.iter().find(|e| e.id == id) else { return Err("That environment is no longer saved.".into()) };
    if paired.id != id {
        return Err(format!("That address reaches {}, a different machine. Add it as its own environment instead.", paired.label));
    }
    let label = saved.label.clone();
    let route = Alt { base: paired.base, bearer: paired.bearer };
    let routes = upsert_route(routes(s, id), route, &|base| ssh::is_tunnel(s, id, base));
    set_routes(s, id, routes);
    Ok(label)
}

fn upsert_route(mut routes: Vec<Alt>, route: Alt, tunnel: &dyn Fn(&str) -> bool) -> Vec<Alt> {
    let same = |r: &Alt| r.base.trim_end_matches('/') == route.base.trim_end_matches('/');
    if let Some(existing) = routes.iter_mut().find(|r| same(r)) {
        *existing = route;
        return routes;
    }
    let rank = |r: &Alt| if tunnel(&r.base) { 4 } else { route_rank(&r.base) };
    let new = rank(&route);
    let at = routes.iter().position(|r| rank(r) >= new).unwrap_or(routes.len());
    routes.insert(at, route);
    routes
}

/// Web `ROUTE_KIND_RANK` for a bearer route; an SSH tunnel ranks 4.
fn route_rank(base: &str) -> u8 {
    match route_kind(base) {
        "loopback" => 0,
        "lan" => 1,
        "tailnet" | "vpn" => 2,
        _ => 3,
    }
}

/// Web `connectionRouteKind` for a bearer route's host.
pub fn route_kind(base: &str) -> &'static str {
    let host = route_host(base);
    let ipv4 = host.parse::<std::net::Ipv4Addr>().ok().map(|ip| ip.octets());
    let loopback = host == "localhost" || host == "::1" || ipv4.is_some_and(|o| o[0] == 127);
    let tailnet = host.ends_with(".ts.net")
        || host.parse::<std::net::Ipv6Addr>().is_ok_and(|ip| ip.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0]);
    let shared = ipv4.is_some_and(|o| o[0] == 100 && (64..=127).contains(&o[1]));
    let private = host.ends_with(".local")
        || host.ends_with(".localhost")
        || host == "home.arpa"
        || host.ends_with(".home.arpa")
        || (!host.contains('.') && !host.contains(':'))
        || ipv4.is_some_and(|o| {
            o[0] == 0
                || o[0] == 10
                || (o[0] == 172 && (16..=31).contains(&o[1]))
                || (o[0] == 192 && o[1] == 168)
                || (o[0] == 169 && o[1] == 254)
                || (o[0] == 198 && (18..=19).contains(&o[1]))
        });
    if host.is_empty() {
        "public"
    } else if loopback {
        "loopback"
    } else if tailnet {
        "tailnet"
    } else if shared {
        "vpn"
    } else if private {
        "lan"
    } else {
        "public"
    }
}

pub fn route_host(base: &str) -> String {
    let rest = base.split_once("://").map_or(base, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    host.trim_end_matches('.').to_lowercase()
}

/// A saved environment's routes, preferred first.
pub fn routes(s: &App, id: &str) -> Vec<Alt> {
    let Some(saved) = s.ui.env.saved.iter().find(|e| e.id == id) else { return Vec::new() };
    let first = Alt { base: saved.base.clone(), bearer: saved.bearer.clone() };
    std::iter::once(first).chain(s.ui.env.alternates.get(id).into_iter().flatten().cloned()).collect()
}

/// The route the live connection uses, when connected.
pub fn route_in_use(s: &App, id: &str) -> Option<usize> {
    let Some(Source::Live(net)) = source(s, id) else { return None };
    if !connected(s, id) {
        return None;
    }
    let base = net.base_url().trim_end_matches('/');
    routes(s, id).iter().position(|r| r.base.trim_end_matches('/') == base)
}

/// Saves `routes` (web `reorderRoutes` / `removeRoute`); a new first route connects at once.
pub fn set_routes(s: &mut App, id: &str, routes: Vec<Alt>) {
    let Some(first) = routes.first().cloned() else { return };
    let Some(saved) = s.ui.env.saved.iter_mut().find(|e| e.id == id) else { return };
    let moved = saved.base != first.base;
    saved.base = first.base;
    saved.bearer = first.bearer;
    let env = saved.clone();
    s.ui.env.alternates.insert(id.to_owned(), routes[1..].to_vec());
    let in_use = source(s, id).and_then(|src| match src {
        Source::Live(net) => Some(net.base_url().trim_end_matches('/').to_owned()),
        Source::Fixture(_) => None,
    });
    let still_saved = in_use.is_some_and(|b| routes.iter().any(|r| r.base.trim_end_matches('/') == b));
    if (moved || !still_saved) && !s.ui.env.off.contains(&env.id) {
        let src = connect(s, &env);
        if let Some(slot) = source_mut(s, id) {
            *slot = src;
        }
    }
    s.ui.env.error = persist(s);
}

/// Web supervisor's `BETTER_ROUTE_CHECK_INTERVAL_MS`, `SILENT_ROUTE_BACKOFF_MAX_MS`,
/// `BETTER_ROUTE_COOLDOWN_MS` and the driver's `ROUTE_CHECK_TIMEOUT_MS`.
const BETTER_ROUTE_CHECK: Duration = Duration::from_secs(60);
const SILENT_ROUTE_BACKOFF_MAX: Duration = Duration::from_secs(30 * 60);
const BETTER_ROUTE_COOLDOWN: Duration = Duration::from_secs(5 * 60);
const ROUTE_CHECK_TIMEOUT: Duration = Duration::from_millis(2500);

/// Web supervisor's `checkBetterRoutes` state for one environment.
#[derive(Default)]
struct Better {
    /// The last check, or when the connection settled on a fallback route.
    last: Option<Instant>,
    /// Routes that stayed silent: failures in a row, and when to check again.
    silent: HashMap<String, (u32, Instant)>,
    /// Routes that answered but then failed to connect.
    cooldown: HashMap<String, Instant>,
    /// The route a check moved to, until it connects.
    switched: Option<String>,
    checking: Option<(Vec<Alt>, Receiver<Vec<bool>>)>,
}

/// Web `backOffSilentRoute`: each silent check doubles the wait, up to half an hour.
fn back_off(previous: Option<u32>) -> (u32, Duration) {
    let failures = previous.unwrap_or(0) + 1;
    let delay = BETTER_ROUTE_CHECK.saturating_mul(2u32.saturating_pow(failures)).min(SILENT_ROUTE_BACKOFF_MAX);
    (failures, delay)
}

fn trimmed(base: &str) -> &str {
    base.trim_end_matches('/')
}

/// The routes preferred over the one at `current` that are due a check.
fn candidates(routes: &[Alt], current: usize, better: &Better, now: Instant) -> Vec<Alt> {
    routes[..current.min(routes.len())]
        .iter()
        .filter(|r| {
            let key = trimmed(&r.base);
            better.cooldown.get(key).is_none_or(|until| *until <= now) && better.silent.get(key).is_none_or(|(_, until)| *until <= now)
        })
        .cloned()
        .collect()
}

/// Records a check's answers; the first route that answered is the one to move to.
fn record(better: &mut Better, checked: &[Alt], answered: &[bool], now: Instant) -> Option<Alt> {
    for (route, ok) in checked.iter().zip(answered) {
        let key = trimmed(&route.base).to_owned();
        if *ok {
            better.silent.remove(&key);
        } else {
            let (failures, delay) = back_off(better.silent.get(&key).map(|(f, _)| *f));
            better.silent.insert(key, (failures, now + delay));
        }
    }
    checked.iter().zip(answered).find(|(_, ok)| **ok).map(|(route, _)| route.clone())
}

/// Web driver `checkDescriptor`: the route answers as this environment, uncached.
fn answers(base: &str, id: &str) -> bool {
    let url = format!("{}/.well-known/t3/environment", trimmed(base));
    ureq::AgentBuilder::new()
        .timeout(ROUTE_CHECK_TIMEOUT)
        .build()
        .get(&url)
        .set("Cache-Control", "no-store")
        .call()
        .ok()
        .and_then(|r| r.into_json::<Value>().ok())
        .is_some_and(|v| v["environmentId"] == id)
}

/// Web supervisor's better-route checks: while connected over a fallback route, the routes
/// preferred over it are checked every minute, and the first that answers takes over.
fn return_to_preferred(s: &mut App) -> bool {
    let now = Instant::now();
    let ids: Vec<String> = s.ui.env.alternates.iter().filter(|(_, a)| !a.is_empty()).map(|(id, _)| id.clone()).collect();
    s.ui.env.better.retain(|id, _| ids.contains(id));
    let mut changed = false;
    for id in ids {
        if s.ui.env.off.contains(&id) {
            s.ui.env.better.remove(&id);
            continue;
        }
        let current = route_in_use(s, &id);
        let routes = routes(s, &id);
        let better = s.ui.env.better.entry(id.clone()).or_default();
        if let Some((checked, rx)) = &better.checking {
            match rx.try_recv() {
                Ok(answered) => {
                    let checked = checked.clone();
                    better.checking = None;
                    let next = record(better, &checked, &answered, now).filter(|_| current.is_some_and(|c| c > 0));
                    if let Some(route) = next {
                        better.switched = Some(trimmed(&route.base).to_owned());
                        let src = connect_to(s, &id, &route.base, &route.bearer);
                        if let Some(slot) = source_mut(s, &id) {
                            *slot = src;
                        }
                        changed = true;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => better.checking = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
            continue;
        }
        let Some(current) = current else { continue };
        if better.switched.as_deref() == Some(trimmed(&routes[current].base)) {
            better.switched = None;
        }
        if current == 0 {
            better.last = None;
            continue;
        }
        let last = *better.last.get_or_insert(now);
        if now.duration_since(last) < BETTER_ROUTE_CHECK {
            continue;
        }
        better.last = Some(now);
        let checked = candidates(&routes, current, better, now);
        if checked.is_empty() {
            continue;
        }
        let (tx, rx) = channel();
        let (bases, owner, waker) = (checked.iter().map(|r| r.base.clone()).collect::<Vec<_>>(), id.clone(), s.ui.panel.waker.clone());
        better.checking = Some((checked, rx));
        std::thread::spawn(move || {
            let answered = bases.iter().map(|base| answers(base, &owner)).collect();
            let _ = tx.send(answered);
            if let Some(w) = waker {
                w();
            }
        });
    }
    changed
}

/// Web's supervisor tries an environment's routes in order: one that fails hands over to the next,
/// at most every ten seconds each.
fn fall_back(s: &mut App) -> bool {
    let now = std::time::Instant::now();
    let mut moves = Vec::new();
    for (id, alternates) in &s.ui.env.alternates {
        if alternates.is_empty() || s.ui.env.off.contains(id) {
            continue;
        }
        let Some(Source::Live(net)) = source(s, id) else { continue };
        if !matches!(&net.state().conn, s5_net::Conn::Closed(reason) if !reason.is_empty()) {
            continue;
        }
        if s.ui.env.fell_back.get(id).is_some_and(|at| now.duration_since(*at).as_secs() < 10) {
            continue;
        }
        let routes = routes(s, id);
        let current = net.base_url().trim_end_matches('/');
        let at = routes.iter().position(|r| r.base.trim_end_matches('/') == current).unwrap_or(0);
        moves.push((id.clone(), routes[(at + 1) % routes.len()].clone(), current.to_owned()));
    }
    for (id, route, from) in &moves {
        // A better route that answered its check but then failed rests a while.
        if let Some(better) = s.ui.env.better.get_mut(id).filter(|b| b.switched.as_deref() == Some(from.as_str())) {
            better.switched = None;
            better.cooldown.insert(from.clone(), now + BETTER_ROUTE_COOLDOWN);
        }
        s.ui.env.fell_back.insert(id.clone(), now);
        let src = connect_to(s, id, &route.base, &route.bearer);
        if let Some(slot) = source_mut(s, id) {
            *slot = src;
        }
    }
    !moves.is_empty()
}

/// Lands a pairing that finished, and SSH environments' tunnel work.
pub fn poll(s: &mut App) -> bool {
    let changed = ssh::poll(s) | adopt_catalog(s) | fall_back(s) | return_to_preferred(s) | recheck_outdated(s);
    let Some(result) = s.ui.env.pairing.as_ref().and_then(|rx| rx.try_recv().ok()) else { return changed };
    s.ui.env.pairing = None;
    paired(s, result);
    true
}

/// The web's remote pairing fields: a host (or a whole pairing URL) and a pairing code.
pub fn pairing_form(s: &App, t: &T) -> El {
    let pairing = s.ui.env.pairing.is_some();
    let field = |label: &str, input: El| col().gap(6.0).child(text(label.to_owned()).font(theme::XS_MEDIUM).color(t.c.foreground)).child(input);
    let mut form = col()
        .key("env-add")
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .child(text("Add environment").font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(
            row()
                .w_full()
                .gap(12.0)
                .child(field("Host", kit::Input::new(&s.ui.env.host).placeholder("backend.example.com").on_edit(|s: &mut App| &mut s.ui.env.host).on_submit(|s: &mut App, _| add(s)).build(t).single_line()).grow(1.0).shrink().min_w(0.0))
                .child(
                    field("Pairing code", kit::Input::new(&s.ui.env.code).placeholder("PAIRCODE").on_edit(|s: &mut App| &mut s.ui.env.code).on_submit(|s: &mut App, _| add(s)).build(t).single_line())
                        .w(160.0),
                ),
        )
        .child(text("Paste a full pairing URL here to fill both fields automatically.").font(theme::TEXT_XS).color(t.c.muted_foreground));
    if let Some(error) = &s.ui.env.error {
        form = form.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive_foreground));
    }
    form = form.child(
        row().child(
            kit::Button::new(if pairing { "Adding…" } else { "Add environment" })
                .size("sm")
                .disabled(pairing || s.ui.env.host.text.trim().is_empty())
                .on_press(add)
                .build(t),
        ),
    );
    form
}

/// Two saved environments that cannot connect: one refused, one whose credential was rejected.
fn scene_failing(s: &mut App) {
    crate::scene_clock(s);
    for (id, label, reason) in [("box", "Build box", "Connection refused (os error 111)"), ("lab", "Lab", "HTTP error: 401 Unauthorized")] {
        s.ui.env.saved.push(Saved { id: id.into(), label: label.into(), base: format!("https://{id}.example.com"), bearer: String::new() });
        let mut state = crate::data::empty_state();
        state.conn = s5_net::Conn::Closed(reason.into());
        park(s, id, Source::Fixture(state));
    }
}

pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("environments-failing", |s| {
        scene_failing(s);
        s.route = Route::Settings("connections".into());
    }),
    ("wizard-connect-environments", |s| {
        scene_failing(s);
        crate::wizard::scene_computers(s);
    }),
    ("wizard-import-denied", crate::wizard::scene_import_denied),
    ("wizard-setup-terminal", crate::wizard::scene_setup_terminal),
    ("wizard-pairing-link", |s| {
        scene_failing(s);
        crate::wizard::scene_pairing(s);
    }),
    ("wizard-t3-connect", |s| {
        scene_failing(s);
        crate::wizard::scene_computers(s);
        crate::cloud::scene_signed_in(s);
    }),
    ("wizard-t3-connect-signed-out", |s| {
        scene_failing(s);
        crate::wizard::scene_computers(s);
    }),
    ("wizard-chatgpt-account", crate::wizard::scene_chatgpt),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(id: &str) -> Saved {
        Saved { id: id.into(), label: format!("Box {id}"), base: format!("https://{id}.example"), bearer: "token".into() }
    }

    #[test]
    fn pairing_refuses_a_saved_machine_at_a_new_address_and_other_protocols() {
        let descriptor = |id: &str, version: Option<u64>| json!({"environmentId": id, "label": "Box", "orchestrationProtocolVersion": version});
        let known = vec![("box".to_string(), "Box".to_string(), vec!["https://box.example".to_string(), "http://10.0.0.2:3773".to_string()])];
        assert_eq!(pairing_refusal(&descriptor("box", Some(2)), "https://box.example/", &known, false), None, "FAILED: a saved route's address pairs again");
        assert_eq!(pairing_refusal(&descriptor("new", Some(2)), "https://new.example", &known, false), None);
        assert_eq!(
            pairing_refusal(&descriptor("box", Some(2)), "https://elsewhere.example", &known, false).as_deref(),
            Some("That address claims to be Box, which is already saved. Add it as a route from that environment's settings instead.")
        );
        assert_eq!(pairing_refusal(&descriptor("box", Some(2)), "https://elsewhere.example", &known, true), None, "FAILED: adding a route is how a new address joins");
        assert_eq!(
            pairing_refusal(&descriptor("new", Some(3)), "https://new.example", &known, false).as_deref(),
            Some("This client is not supported by this server. Update your app or use a compatible release to connect to Box.")
        );
        assert_eq!(
            pairing_refusal(&descriptor("new", None), "https://new.example", &known, false).as_deref(),
            Some("This client requires a newer server. Update T3 Code on Box to connect."),
            "FAILED: a server from before negotiation speaks protocol 1"
        );
        let older = |update: &str, desktop: bool| json!({"environmentId": "new", "label": "Box", "orchestrationProtocolVersion": 1, "capabilities": {"serverSelfUpdate": update, "desktopAppUpdate": desktop}});
        assert_eq!(pairing_refusal(&older("boot-service", false), "https://new.example", &known, false), None, "FAILED: an outdated server that updates itself is saved to be updated");
        assert_eq!(pairing_refusal(&older("desktop-managed", true), "https://new.example", &known, false), None);
        assert!(pairing_refusal(&older("desktop-managed", false), "https://new.example", &known, false).is_some(), "a desktop host that can't update its app needs a manual update");
    }

    #[test]
    fn an_outdated_server_is_not_switched_to_and_connects_again_once_current() {
        // SAFETY: the same file every_environment_is_listed_and_commands_follow_the_owner uses.
        unsafe { std::env::set_var("S5_ENVIRONMENTS", std::env::temp_dir().join(format!("s5-env-{}.json", std::process::id()))) };
        let mut s = App::new();
        s.ui.env.saved.push(saved("a"));
        // Switched off, so coming back does not dial a real socket.
        s.ui.env.off.push("a".into());
        s.ui.env.parked.push(("a".into(), Source::Fixture(crate::data::empty_state())));
        s.ui.env.outdated.insert("a".into(), json!({"environmentId": "a"}));
        let home = s.ui.env.active.clone();
        switch(&mut s, "a");
        assert_eq!(s.ui.env.active, home, "FAILED: switched to a server that can only be updated");
        assert!(!recheck_landed(&mut s, "a", Ok(json!({"environmentId": "a"}))), "still on the first protocol");
        assert!(!recheck_landed(&mut s, "a", Ok(json!({"environmentId": "b", "orchestrationProtocolVersion": PROTOCOL}))), "another machine answers there");
        assert!(!recheck_landed(&mut s, "a", Err("refused".into())));
        assert!(recheck_landed(&mut s, "a", Ok(json!({"environmentId": "a", "orchestrationProtocolVersion": PROTOCOL}))));
        assert!(outdated(&s, "a").is_none(), "FAILED: a server updated by hand stays flagged");
        switch(&mut s, "a");
        assert_eq!(s.ui.env.active, "a");
    }

    #[test]
    fn saved_environments_round_trip_and_drop_incomplete_rows() {
        let list = vec![saved("a"), saved("b")];
        let off = vec!["b".to_owned()];
        let none = HashMap::new();
        assert_eq!(decode(&encode(&list, &off, &none, &HashMap::new())), list);
        assert_eq!(decode_off(&encode(&list, &off, &none, &HashMap::new())), off, "a switched-off environment stays off");
        assert!(encode(&list, &[], &none, &HashMap::new()).to_string().find("enabled").is_none(), "on is the default and omitted");
        let alternates = HashMap::from([("a".to_owned(), vec![Alt { base: "https://a.ts.net".into(), bearer: "t2".into() }])]);
        assert_eq!(decode_alternates(&encode(&list, &off, &alternates, &HashMap::new())), alternates, "web's alternateRoutes survive a save");
        assert!(encode(&list, &off, &none, &HashMap::new()).to_string().find("alternateRoutes").is_none());
        let outdated = HashMap::from([("a".to_owned(), json!({"serverVersion": "0.1.5", "capabilities": {"serverSelfUpdate": "boot-service"}}))]);
        let encoded = encode(&list, &off, &none, &outdated);
        assert_eq!(decode_outdated(&encoded), outdated, "FAILED: an outdated server stays outdated across launches");
        assert_eq!(encoded["environments"][0]["serverUpdateRequired"], true, "web's catalog field");
        assert_eq!(decode(&encoded), list);
        let partial = json!({"environments": [{"environmentId": "c", "label": "C", "httpBaseUrl": "https://c"}]});
        assert!(decode(&partial).is_empty(), "a row without a bearer cannot connect");
    }

    #[test]
    fn fallback_routes_return_to_a_preferred_route_that_answers_like_the_web_supervisor() {
        let alt = |base: &str| Alt { base: base.into(), bearer: "b".into() };
        let routes = [alt("http://192.168.1.20:3773/"), alt("https://box.ts.net"), alt("https://box.example.com")];
        let now = Instant::now();
        let mut better = Better::default();
        // On the third route both preferred ones are checked; on the first there is nothing better.
        assert_eq!(candidates(&routes, 2, &better, now).len(), 2);
        assert!(candidates(&routes, 0, &better, now).is_empty());
        // The first route that answered takes over; a silent one waits twice the interval.
        let checked = candidates(&routes, 2, &better, now);
        assert_eq!(record(&mut better, &checked, &[false, true], now), Some(routes[1].clone()));
        assert_eq!(better.silent["http://192.168.1.20:3773"], (1, now + Duration::from_secs(120)));
        assert_eq!(candidates(&routes, 2, &better, now), [routes[1].clone()], "the silent route rests");
        assert_eq!(candidates(&routes, 2, &better, now + Duration::from_secs(120)).len(), 2);
        // Each silent check doubles the wait, up to half an hour; an answer clears it.
        assert_eq!(back_off(Some(1)), (2, Duration::from_secs(240)));
        assert_eq!(back_off(Some(9)).1, SILENT_ROUTE_BACKOFF_MAX);
        assert_eq!(record(&mut better, &checked, &[true, false], now), Some(routes[0].clone()));
        assert!(!better.silent.contains_key("http://192.168.1.20:3773"));
        // A route that answered but failed to connect rests five minutes.
        better.cooldown.insert("https://box.ts.net".into(), now + BETTER_ROUTE_COOLDOWN);
        assert_eq!(candidates(&routes, 2, &better, now).len(), 1);
    }

    #[test]
    fn a_new_route_goes_first_among_its_kind_and_one_at_the_same_address_is_replaced() {
        let alt = |base: &str, bearer: &str| Alt { base: base.into(), bearer: bearer.into() };
        let none = |_: &str| false;
        let routes = vec![alt("http://192.168.1.4:3773", "lan"), alt("https://box.example.com", "public")];
        let added = upsert_route(routes.clone(), alt("https://box.tail1.ts.net", "tail"), &none);
        assert_eq!(added.iter().map(|r| r.bearer.as_str()).collect::<Vec<_>>(), ["lan", "tail", "public"]);
        let lan = upsert_route(routes.clone(), alt("http://10.0.0.4:3773", "lan2"), &none);
        assert_eq!(lan[0].bearer, "lan2", "a fresh credential connects before its kind");
        let renewed = upsert_route(routes.clone(), alt("https://box.example.com/", "fresh"), &none);
        assert_eq!((renewed.len(), renewed[1].bearer.as_str()), (2, "fresh"));
        // An SSH tunnel ranks after every address route, though its host is loopback.
        let tunnel = |base: &str| base.starts_with("http://127.0.0.1");
        let ssh = vec![alt("http://127.0.0.1:41000", "ssh")];
        let with_lan = upsert_route(ssh, alt("http://192.168.1.4:3773", "lan"), &tunnel);
        assert_eq!(with_lan[0].bearer, "lan");
        assert_eq!(route_kind("http://[fd7a:115c:a1e0::1]:3773"), "tailnet");
        assert_eq!(route_kind("http://100.64.0.2"), "vpn");
        assert_eq!(route_kind("http://localhost:3773"), "loopback");
    }

    #[test]
    fn status_reads_like_the_web_and_a_rejected_credential_asks_to_pair_again() {
        let mut s = App::new();
        let mut remote = crate::data::empty_state();
        s.ui.env.saved.push(saved("a"));
        for (conn, expected) in [
            (s5_net::Conn::Connecting, "Connecting..."),
            (s5_net::Conn::Closed("Connection refused (os error 111)".into()), "Failed to connect. Reconnecting... Reason: Connection refused (os error 111)"),
            (s5_net::Conn::Closed("HTTP error: 401 Unauthorized".into()), "Connection failed. Reason: HTTP error: 401 Unauthorized. Pair this environment again."),
        ] {
            remote.conn = conn;
            s.ui.env.parked = vec![("a".into(), Source::Fixture(remote.clone()))];
            assert_eq!(status(&s, "a"), expected);
        }
        let mut short = |s: &mut App, conn: s5_net::Conn| {
            remote.conn = conn;
            s.ui.env.parked = vec![("a".into(), Source::Fixture(remote.clone()))];
            short_status(s, "a")
        };
        assert_eq!(short(&mut s, s5_net::Conn::Connecting), "Connecting…");
        assert_eq!(short(&mut s, s5_net::Conn::Closed("refused".into())), "Reconnecting…");
        assert_eq!(short(&mut s, s5_net::Conn::Closed("HTTP error: 401 Unauthorized".into())), "Connection failed");
        assert_eq!(status(&s, "gone"), "Not connected");
    }

    #[test]
    fn every_environment_is_listed_and_commands_follow_the_owner() {
        // SAFETY: only this test reads S5_ENVIRONMENTS; keeps `remove` off the real config dir.
        unsafe { std::env::set_var("S5_ENVIRONMENTS", std::env::temp_dir().join(format!("s5-env-{}.json", std::process::id()))) };
        let mut s = App::new();
        let mut local = crate::data::empty_state();
        local.shell_ready = true;
        local.projects.push(json!({"id": "p-here", "title": "Here"}));
        local.threads.push(json!({"id": "t-here", "projectId": "p-here"}));
        s.src = Source::Fixture(local);
        let mut remote = crate::data::empty_state();
        remote.projects.push(json!({"id": "p-there", "title": "There"}));
        remote.threads.push(json!({"id": "t-there", "projectId": "p-there"}));
        remote.shell_revision = 1;
        s.ui.env.saved.push(saved("a"));
        s.ui.env.parked.push(("a".into(), Source::Fixture(remote)));
        assert_eq!(keys(&s), [LOCAL, "a"]);
        s.ui.desktop.local_off = true;
        assert_eq!(keys(&s), ["a"], "web drops the primary environment while it is off");
        s.ui.desktop.local_off = false;
        let ids = |s: &App| s.state().threads.iter().map(|t| crate::data::s(t, "id").to_owned()).collect::<Vec<_>>();

        assert!(drain(&mut s));
        assert_eq!(ids(&s), ["t-here", "t-there"]);
        assert_eq!((owner(&s, "t-there"), owner(&s, "p-there"), owner(&s, "t-here")), ("a".into(), "a".into(), LOCAL.into()));
        assert_eq!(remote_label(&s, &s.state().projects[1]), Some("Box a".into()));
        // The scope menu's server-icon tooltip (ProjectEnvironmentBadge).
        let members = |ids: &[&str]| ids.iter().map(|i| i.to_string()).collect::<Vec<_>>();
        assert_eq!(badge(&s, &members(&["p-here", "p-there"])).as_deref(), Some("Also on Box a"));
        assert_eq!(badge(&s, &members(&["p-there"])).as_deref(), Some("On Box a"));
        assert_eq!(badge(&s, &members(&["p-here"])), None);
        assert!(!drain(&mut s), "nothing moved, so nothing is copied again");

        s.navigate(Route::Thread("t-there".into()));
        assert_eq!(s.ui.env.active, "a", "the thread's own server is the one on screen");
        assert!(!s.state().shell_ready);
        assert_eq!(ids(&s), ["t-there", "t-here"]);
        assert_eq!(owner(&s, "t-here"), LOCAL);
        assert_eq!(location(&s, &s.state().threads[1]), "Local");

        // A shell snapshot replaces the lists and drops the copies; the next drain restores them.
        if let Source::Fixture(state) = &mut s.src {
            state.threads.retain(|t| t.get(TAG).is_none());
        }
        assert!(drain(&mut s));
        assert_eq!(ids(&s), ["t-there", "t-here"]);

        s.navigate(Route::Settings("general".into()));
        assert_eq!(s.ui.env.active, LOCAL);
        assert!(s.state().shell_ready);
        remove(&mut s, "a");
        drain(&mut s);
        assert_eq!(ids(&s), ["t-here"]);
        assert!(s.ui.env.parked.is_empty());
    }
}
