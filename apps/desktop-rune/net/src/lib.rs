//! The S5 Code server, as the Rune client sees it: one WebSocket speaking Effect-RPC JSON, the
//! shell/config/thread streams folded into plain state, and the commands the UI sends.
//!
//! Rows stay `serde_json::Value` exactly as the server encodes them (see
//! `packages/contracts/src/orchestrationV2.ts`), so the UI reads the same field names the web does.
//! Network I/O runs on background threads; the UI thread calls [`Net::drain`] from its poll hook
//! and gets woken through the callback passed to [`Net::connect`].

use std::collections::HashMap;

use serde_json::Value;

/// Connection lifecycle, as the UI shows it.
#[derive(Clone, Debug, PartialEq)]
pub enum Conn {
    Connecting,
    Open,
    /// Lost or refused; the client keeps retrying with backoff and resubscribes on reconnect.
    Closed(String),
}

/// One subscribed thread: the projection (`ThreadProjection` from `orchestration.subscribeThread`)
/// with every later event already folded in, mirroring
/// `packages/client-runtime/src/state/orchestrationV2Projection.ts`.
#[derive(Clone, Debug, Default)]
pub struct ThreadView {
    pub projection: Value,
    pub synchronized: bool,
    pub sequence: u64,
}

#[derive(Clone, Debug)]
pub struct State {
    pub conn: Conn,
    /// `ServerConfig` from the first `subscribeServerConfig` snapshot, kept current by its later
    /// items (`settingsUpdated`, `providerStatuses`, `keybindingsUpdated`, ...).
    pub config: Option<Value>,
    /// `ProjectShell` rows, server order.
    pub projects: Vec<Value>,
    /// Active `ThreadShell` rows (location "active").
    pub threads: Vec<Value>,
    /// Archived `ThreadShell` rows.
    pub archived: Vec<Value>,
    /// True once the shell snapshot has arrived and `synchronized` was seen.
    pub shell_ready: bool,
    /// Last authoritative shell snapshot or row sequence (enrichment does not advance it).
    pub shell_sequence: u64,
    pub thread_views: HashMap<String, ThreadView>,
    /// `subscribeVcsStatus` state per cwd: the latest merged snapshot.
    pub vcs: HashMap<String, Value>,
}

/// A finished one-shot request: `Ok(value)` for `Exit Success`, `Err(cause)` for a failure.
pub type Reply = Result<Value, Value>;

mod projection;
mod transport;
use serde_json::json;
use std::sync::{
    Arc,
    mpsc::{self, Receiver, Sender},
};
use transport::{Command, Incoming, Route};

pub struct Net {
    state: State,
    tx: Sender<Command>,
    rx: Receiver<Incoming>,
    replies: HashMap<u64, Reply>,
    next_id: u64,
    resources: HashMap<Route, Value>,
    resource_events: HashMap<Route, Vec<Value>>,
    base_url: String,
    bearer: String,
    incoming_tx: Sender<Incoming>,
    wake: Arc<dyn Fn() + Send + Sync>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            conn: Conn::Connecting,
            config: None,
            projects: vec![],
            threads: vec![],
            archived: vec![],
            shell_ready: false,
            shell_sequence: 0,
            thread_views: HashMap::new(),
            vcs: HashMap::new(),
        }
    }
}
impl Net {
    pub fn connect(base_url: &str, token: &str, wake: impl Fn() + Send + Sync + 'static) -> Net {
        let (tx, commands) = mpsc::channel();
        let (incoming, rx) = mpsc::channel();
        let incoming_tx = incoming.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let worker_wake = wake.clone();
        let base = base_url.to_owned();
        let worker_token = token.to_owned();
        std::thread::spawn(move || {
            transport::run(base, worker_token, commands, incoming, move || {
                worker_wake()
            })
        });
        Self {
            state: State::default(),
            tx,
            rx,
            replies: HashMap::new(),
            next_id: 0,
            resources: HashMap::new(),
            resource_events: HashMap::new(),
            base_url: base_url.to_owned(),
            bearer: token.to_owned(),
            incoming_tx,
            wake,
        }
    }
    /// Folds decoded messages without waiting for network I/O.
    pub fn drain(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Incoming::Connection(conn) => {
                    if conn != Conn::Open {
                        self.state.shell_ready = false;
                        for v in self.state.thread_views.values_mut() {
                            v.synchronized = false;
                        }
                    }
                    changed |= self.state.conn != conn;
                    self.state.conn = conn;
                }
                Incoming::Item(route, item) => {
                    if let Route::Resource(method, _) = &route {
                        if method == "terminal.attach" {
                            self.resource_events.entry(route.clone()).or_default().push(item.clone());
                        }
                        if method == "subscribeAuthAccess" {
                            let snapshot = self.resources.entry(route).or_insert(Value::Null);
                            fold_access(snapshot, item);
                        } else {
                            self.resources.insert(route, item);
                        }
                    } else {
                        self.state.fold(route, item);
                    }
                    changed = true;
                }
                Incoming::Reply(id, reply) => {
                    self.replies.insert(id, reply);
                }
            }
        }
        changed
    }
    pub fn state(&self) -> &State {
        &self.state
    }
    pub fn watch_thread(&mut self, thread_id: &str) {
        if self.state.thread_views.contains_key(thread_id) {
            return;
        }
        self.state
            .thread_views
            .insert(thread_id.to_owned(), ThreadView::default());
        let _ = self
            .tx
            .send(Command::Watch(Route::Thread(thread_id.to_owned())));
    }
    pub fn unwatch_thread(&mut self, thread_id: &str) {
        self.state.thread_views.remove(thread_id);
        let _ = self
            .tx
            .send(Command::Unwatch(Route::Thread(thread_id.to_owned())));
    }
    pub fn watch_vcs(&mut self, cwd: &str) {
        let _ = self.tx.send(Command::Watch(Route::Vcs(cwd.to_owned())));
    }
    pub fn unwatch_vcs(&mut self, cwd: &str) {
        self.state.vcs.remove(cwd);
        let _ = self.tx.send(Command::Unwatch(Route::Vcs(cwd.to_owned())));
    }
    /// Subscribe to a snapshot stream; the transport reconnects and reapplies its payload.
    pub fn watch_resource(&mut self, method: &str, payload: Value) {
        let route = Route::Resource(method.into(), payload.to_string());
        if self.resources.contains_key(&route) {
            return;
        }
        self.resources.insert(route.clone(), Value::Null);
        let _ = self.tx.send(Command::Watch(route));
    }
    pub fn resource(&self, method: &str, payload: Value) -> Option<&Value> {
        self.resources
            .get(&Route::Resource(method.into(), payload.to_string()))
            .filter(|v| !v.is_null())
    }
    /// Drains ordered terminal events; snapshots alone cannot preserve burst output.
    pub fn take_resource_events(&mut self, method: &str, payload: Value) -> Vec<Value> {
        self.resource_events.remove(&Route::Resource(method.into(), payload.to_string())).unwrap_or_default()
    }
    pub fn unwatch_resource(&mut self, method: &str, payload: Value) {
        let route = Route::Resource(method.into(), payload.to_string());
        self.resource_events.remove(&route);
        if self.resources.remove(&route).is_some() {
            let _ = self.tx.send(Command::Unwatch(route));
        }
    }
    /// Authenticated HTTP operations share the RPC reply channel and never block the UI.
    pub fn http_request(&mut self, method: &str, path: &str, payload: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let base = self.base_url.clone();
        let bearer = self.bearer.clone();
        let method = method.to_owned();
        let path = path.to_owned();
        let tx = self.incoming_tx.clone();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let result = (|| {
                if !path.starts_with("/api/") {
                    return Err(json!({"message":"Unsupported API path"}));
                }
                let mut url =
                    url::Url::parse(&base).map_err(|e| json!({"message":e.to_string()}))?;
                url.set_path(&path);
                url.set_query(None);
                url.set_fragment(None);
                let req = ureq::AgentBuilder::new()
                    .redirects(0)
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .request(&method, url.as_str())
                    .set("Authorization", &format!("Bearer {bearer}"));
                let response = if method == "GET" {
                    req.call()
                } else {
                    req.send_json(payload)
                };
                match response {
                    Ok(response) => response
                        .into_json::<Value>()
                        .map_err(|e| json!({"message":e.to_string()})),
                    Err(ureq::Error::Status(code, response)) => Err(response
                        .into_json::<Value>()
                        .unwrap_or_else(|_| json!({"message":format!("HTTP {code}")}))),
                    Err(error) => Err(json!({"message":error.to_string()})),
                }
            })();
            let _ = tx.send(Incoming::Reply(id, result));
            wake();
        });
        id
    }
    pub fn request(&mut self, tag: &str, payload: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        if self
            .tx
            .send(Command::Request(id, tag.to_owned(), payload))
            .is_err()
        {
            self.replies
                .insert(id, Err(json!({"message":"network worker stopped"})));
        }
        id
    }
    pub fn take_reply(&mut self, id: u64) -> Option<Reply> {
        self.replies.remove(&id)
    }
    pub fn dispatch(&mut self, mut command: Value) -> u64 {
        if command.get("commandId").is_none() {
            command["commandId"] = json!(uuid());
        }
        self.request("orchestration.dispatchCommand", command)
    }
    /// Accepts the launchThread payload; supplies a fresh command id if omitted.
    pub fn launch_thread(&mut self, mut payload: Value) -> u64 {
        if payload.get("commandId").is_none() {
            payload["commandId"] = json!(uuid());
        }
        self.request("orchestration.launchThread", payload)
    }
    /// Drops this client's socket and exercises the normal reconnect/resume path.
    pub fn reconnect(&mut self) {
        let _ = self.tx.send(Command::Reconnect);
    }
}
impl Drop for Net {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Stop);
    }
}
pub fn uuid() -> String {
    ::uuid::Uuid::new_v4().to_string()
}

/// Exchanges a one-time pairing/bootstrap credential for a reusable bearer token.
pub fn exchange_bootstrap(base: &str, credential: &str) -> Result<String, String> {
    let response: Value = ureq::post(&format!("{}/oauth/token", base.trim_end_matches('/')))
        .send_form(&[
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:token-exchange",
            ),
            ("subject_token", credential),
            (
                "subject_token_type",
                "urn:t3:params:oauth:token-type:environment-bootstrap",
            ),
            (
                "requested_token_type",
                "urn:ietf:params:oauth:token-type:access_token",
            ),
        ])
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    response["access_token"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "token response has no access_token".into())
}
/// Splits a /pair#token= URL, decoding its credential without sending it to the server.
pub fn parse_pairing_url(input: &str) -> Result<(String, String), String> {
    let mut url = url::Url::parse(input).map_err(|e| e.to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || url.path() != "/pair"
    {
        return Err("expected an http(s) /pair URL".into());
    }
    let token = url::form_urlencoded::parse(url.fragment().unwrap_or("").as_bytes())
        .find(|(k, _)| k == "token")
        .map(|(_, v)| v.into_owned())
        .filter(|v| !v.is_empty())
        .ok_or("pairing URL has no token")?;
    url.set_fragment(None);
    url.set_query(None);
    url.set_path("");
    Ok((url.as_str().trim_end_matches('/').to_owned(), token))
}
fn fold_access(state: &mut Value, event: Value) {
    if event["type"] == "snapshot" {
        *state = event;
        return;
    }
    if event["error"].is_object() {
        *state = event;
        return;
    }
    if event["revision"].as_u64() <= state["revision"].as_u64() {
        return;
    }
    let (field, key, remove) = match event["type"].as_str() {
        Some("clientUpserted") => ("clientSessions", "sessionId", false),
        Some("clientRemoved") => ("clientSessions", "sessionId", true),
        Some("pairingLinkUpserted") => ("pairingLinks", "id", false),
        Some("pairingLinkRemoved") => ("pairingLinks", "id", true),
        _ => return,
    };
    if let Some(rows) = state["payload"][field].as_array_mut() {
        rows.retain(|v| v[key] != event["payload"][key]);
        if !remove {
            rows.push(event["payload"].clone());
        }
    }
    state["revision"] = event["revision"].clone();
}

impl State {
    fn fold(&mut self, route: Route, item: Value) {
        match route {
            Route::Resource(..) => {}
            Route::Config => {
                if item["type"] == "snapshot" {
                    self.config = Some(item["config"].clone());
                    return;
                }
                if let Some(config) = &mut self.config {
                    let payload = item.get("payload").unwrap_or(&item);
                    match item["type"].as_str().unwrap_or("") {
                        "environmentThemesUpdated" => {
                            config["environmentThemes"] = payload["themes"].clone()
                        }
                        "usageLimitSourcesUpdated" => {
                            config["usageLimitSources"] = payload["sources"].clone()
                        }
                        _ => {
                            if let Some(fields) = payload.as_object() {
                                for (k, v) in fields {
                                    if k != "type" && k != "version" {
                                        config[k] = v.clone();
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Route::Shell => {
                if let Some(sequence) = item["sequence"].as_u64() {
                    if sequence <= self.shell_sequence {
                        return;
                    }
                    self.shell_sequence = sequence;
                }
                match item["kind"].as_str().unwrap_or("") {
                    "snapshot" => {
                        let s = &item["snapshot"];
                        if let Some(roots) = item["resolvedRepositoryIdentityRoots"].as_array() {
                            for project in &mut self.projects {
                                if let Some(candidate) =
                                    projection::rows(s, "projects").iter().find(|p| {
                                        p["id"] == project["id"]
                                            && p["workspaceRoot"] == project["workspaceRoot"]
                                    })
                                {
                                    if roots.contains(&project["workspaceRoot"])
                                        || (project["repositoryIdentity"].is_null()
                                            && !candidate["repositoryIdentity"].is_null())
                                    {
                                        project["repositoryIdentity"] =
                                            candidate["repositoryIdentity"].clone();
                                    }
                                }
                            }
                            return;
                        }
                        let mut projects = projection::rows(s, "projects").to_vec();
                        for project in &mut projects {
                            retain_identity(&self.projects, project);
                        }
                        self.projects = projects;
                        self.threads = projection::rows(s, "threads").to_vec();
                        self.archived = projection::rows(s, "archivedThreads").to_vec();
                        self.shell_sequence = s["snapshotSequence"].as_u64().unwrap_or(0);
                        self.shell_ready = false;
                    }
                    "synchronized" => self.shell_ready = true,
                    "project.updated" => {
                        let mut project = item["project"].clone();
                        retain_identity(&self.projects, &mut project);
                        projection::upsert(&mut self.projects, project);
                    }
                    "project.removed" => self.projects.retain(|p| p["id"] != item["projectId"]),
                    "thread.updated" => {
                        let row = item["thread"].clone();
                        if item["location"] == "archive" {
                            self.threads.retain(|t| t["id"] != row["id"]);
                            projection::upsert(&mut self.archived, row);
                        } else {
                            self.archived.retain(|t| t["id"] != row["id"]);
                            projection::upsert(&mut self.threads, row);
                        }
                    }
                    "thread.removed" => {
                        let rows = if item["location"] == "archive" {
                            &mut self.archived
                        } else {
                            &mut self.threads
                        };
                        rows.retain(|t| t["id"] != item["threadId"]);
                    }
                    _ => {}
                }
            }
            Route::Thread(id) => {
                let Some(view) = self.thread_views.get_mut(&id) else {
                    return;
                };
                match item["kind"].as_str().unwrap_or("") {
                    "snapshot" => {
                        view.projection = item["projection"].clone();
                        view.sequence = item["snapshotSequence"].as_u64().unwrap_or(0);
                        view.synchronized = false;
                    }
                    "synchronized" => view.synchronized = true,
                    "event" => {
                        let seq = item["sequence"].as_u64().unwrap_or(0);
                        if seq > view.sequence {
                            projection::apply(&mut view.projection, &item["event"]);
                            view.sequence = seq;
                        }
                    }
                    _ => {}
                }
            }
            Route::Vcs(cwd) => {
                let value = self
                    .vcs
                    .entry(cwd)
                    .or_insert(json!({"local":null,"remote":null}));
                for key in ["local", "remote"] {
                    if let Some(part) = item.get(key) {
                        value[key] = part.clone();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;

fn retain_identity(previous: &[Value], project: &mut Value) {
    if project["repositoryIdentity"].is_null() {
        if let Some(old) = previous
            .iter()
            .find(|p| p["id"] == project["id"] && p["workspaceRoot"] == project["workspaceRoot"])
        {
            if let Some(identity) = old.get("repositoryIdentity") {
                project["repositoryIdentity"] = identity.clone();
            }
        }
    }
}
