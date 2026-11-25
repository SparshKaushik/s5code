//! The S5 Code server, as the Rune client sees it: one WebSocket speaking Effect-RPC JSON, the
//! shell/config/thread streams folded into plain state, and the commands the UI sends.
//!
//! Rows stay `serde_json::Value` exactly as the server encodes them (see
//! `packages/contracts/src/orchestrationV2.ts`), so the UI reads the same field names the web does.
//! Network I/O runs on background threads; the UI thread calls [`Net::drain`] from its poll hook
//! and gets woken through the callback passed to [`Net::connect`].

use std::collections::{HashMap, HashSet};

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
    /// A bounded snapshot's opaque cursor for the page before its window.
    pub history_cursor: Option<String>,
    pub has_more_history: bool,
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
    /// Bumps whenever a shell item touches `projects`, `threads` or `archived`.
    pub shell_revision: u64,
    /// The trace ID of the failure behind `Conn::Closed`, from the relay or the server's
    /// `traceparent`, so a failure can be reported. Cleared once connected.
    pub conn_trace: Option<String>,
}

pub use auth::{Auth, Dpop};

/// A finished one-shot request: `Ok(value)` for `Exit Success`, `Err(cause)` for a failure.
pub type Reply = Result<Value, Value>;

mod auth;
pub mod cloud;
mod pairing;
mod projection;
pub use pairing::{fetch_descriptor, resolve_pairing};
pub use url;
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
    chunks: HashMap<u64, Vec<Value>>,
    resources: HashMap<Route, Value>,
    resource_events: HashMap<Route, Vec<Value>>,
    base_url: String,
    auth: Auth,
    incoming_tx: Sender<Incoming>,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// Threads followed only through [`Net::keep_thread`]; any [`Net::watch_thread`] adopts one.
    kept_only: HashSet<String>,
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
            shell_revision: 0,
            conn_trace: None,
        }
    }
}

/// Process-wide, so a reply can be handed between environments' clients without colliding.
fn next_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}
impl Net {
    pub fn connect(base_url: &str, token: &str, wake: impl Fn() + Send + Sync + 'static) -> Net {
        Self::spawn(base_url, token, true, wake)
    }
    /// A connection that subscribes to nothing and only carries requests, for a server whose
    /// orchestration protocol is older than this client's but whose self-update RPCs still answer.
    pub fn connect_requests_only(base_url: &str, token: &str, wake: impl Fn() + Send + Sync + 'static) -> Net {
        Self::spawn(base_url, token, false, wake)
    }
    /// A connection that authorizes some other way than a bearer token, such as T3 Connect's DPoP.
    pub fn connect_with(base_url: &str, auth: Auth, wake: impl Fn() + Send + Sync + 'static) -> Net {
        Self::spawn_with(base_url, auth, true, wake)
    }
    fn spawn(base_url: &str, token: &str, subscribe_shell: bool, wake: impl Fn() + Send + Sync + 'static) -> Net {
        Self::spawn_with(base_url, Auth::Bearer(token.to_owned()), subscribe_shell, wake)
    }
    fn spawn_with(base_url: &str, auth: Auth, subscribe_shell: bool, wake: impl Fn() + Send + Sync + 'static) -> Net {
        let (tx, commands) = mpsc::channel();
        let (incoming, rx) = mpsc::channel();
        let incoming_tx = incoming.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let worker_wake = wake.clone();
        let base = base_url.to_owned();
        let worker_auth = auth.clone();
        std::thread::spawn(move || {
            transport::run(base, worker_auth, subscribe_shell, commands, incoming, move || {
                worker_wake()
            })
        });
        Self {
            state: State::default(),
            tx,
            rx,
            replies: HashMap::new(),
            chunks: HashMap::new(),
            resources: HashMap::new(),
            resource_events: HashMap::new(),
            base_url: base_url.to_owned(),
            auth,
            incoming_tx,
            wake,
            kept_only: HashSet::new(),
        }
    }
    /// A client with no worker, for tests: each request is answered at once that the worker stopped.
    #[doc(hidden)]
    pub fn offline() -> Net {
        let (tx, _) = mpsc::channel();
        let (incoming_tx, rx) = mpsc::channel();
        Self {
            state: State::default(),
            tx,
            rx,
            replies: HashMap::new(),
            chunks: HashMap::new(),
            resources: HashMap::new(),
            resource_events: HashMap::new(),
            base_url: String::new(),
            auth: Auth::Bearer(String::new()),
            incoming_tx,
            wake: Arc::new(|| {}),
            kept_only: HashSet::new(),
        }
    }
    /// A connection over an in-memory transport that drops what it is sent and never answers,
    /// so client tests can drive the code that subscribes and unsubscribes.
    #[cfg(feature = "test-support")]
    pub fn in_memory(base_url: &str) -> Net {
        let (tx, commands) = mpsc::channel::<Command>();
        let (incoming_tx, rx) = mpsc::channel();
        std::thread::spawn(move || commands.into_iter().for_each(drop));
        Self {
            state: State::default(),
            tx,
            rx,
            replies: HashMap::new(),
            chunks: HashMap::new(),
            resources: HashMap::new(),
            resource_events: HashMap::new(),
            base_url: base_url.to_owned(),
            auth: Auth::Bearer(String::new()),
            incoming_tx,
            wake: Arc::new(|| {}),
            kept_only: HashSet::new(),
        }
    }
    /// [`Net::in_memory`] that keeps the requests it is sent, so a client test can check what
    /// went out and answer it with [`Net::put_reply`].
    #[cfg(feature = "test-support")]
    pub fn in_memory_sent(base_url: &str) -> (Net, Sent) {
        let (tx, commands) = mpsc::channel::<Command>();
        let mut net = Net::in_memory(base_url);
        net.tx = tx;
        (net, Sent(commands))
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
                    if conn == Conn::Open {
                        self.state.conn_trace = None;
                    }
                    changed |= self.state.conn != conn;
                    self.state.conn = conn;
                }
                Incoming::Trace(trace) => {
                    changed |= self.state.conn_trace != trace;
                    self.state.conn_trace = trace;
                }
                Incoming::Item(route, item) => {
                    if let Route::Resource(method, _) = &route {
                        // Ordered events: terminal output bursts, command output, preview sessions
                        // that open and close between two polls, and server lifecycle.
                        if matches!(
                            method.as_str(),
                            "terminal.attach" | "subscribeTerminalMetadata" | "subscribePreviewEvents" | "orchestration.subscribeCommandOutput" | "subscribeServerLifecycle"
                        ) {
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
                Incoming::Chunk(id, values) => {
                    self.chunks.entry(id).or_default().extend(values);
                    changed = true;
                }
            }
        }
        changed
    }
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
    /// How this connection authorizes, for requests of the client's own such as `api_get`.
    pub fn auth(&self) -> &Auth {
        &self.auth
    }
    pub fn state(&self) -> &State {
        &self.state
    }
    pub fn state_mut(&mut self) -> &mut State {
        &mut self.state
    }
    /// Moves finished replies and stream chunks to `to`, where the UI polls for them.
    pub fn hand_off(&mut self, to: &mut Net) {
        to.replies.extend(self.replies.drain());
        for (id, values) in self.chunks.drain() {
            to.chunks.entry(id).or_default().extend(values);
        }
    }
    pub fn watch_thread(&mut self, thread_id: &str) {
        self.kept_only.remove(thread_id);
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
    /// Follows a thread for a background reader (the running-thread keep-alive) without claiming
    /// it: [`Net::release_thread`] then drops it unless something watched it meanwhile.
    pub fn keep_thread(&mut self, thread_id: &str) {
        if self.state.thread_views.contains_key(thread_id) {
            return;
        }
        self.watch_thread(thread_id);
        self.kept_only.insert(thread_id.to_owned());
    }
    pub fn release_thread(&mut self, thread_id: &str) {
        if self.kept_only.remove(thread_id) {
            self.unwatch_thread(thread_id);
        }
    }
    pub fn unwatch_thread(&mut self, thread_id: &str) {
        self.state.thread_views.remove(thread_id);
        let _ = self
            .tx
            .send(Command::Unwatch(Route::Thread(thread_id.to_owned())));
    }
    pub fn watch_vcs(&mut self, cwd: &str) {
        if self.state.vcs.contains_key(cwd) {
            return;
        }
        self.state.vcs.insert(cwd.to_owned(), json!({"local":null,"remote":null}));
        let _ = self.tx.send(Command::Watch(Route::Vcs(cwd.to_owned())));
    }
    /// Resubscribes a status stream that ended in an error, keeping the error until a snapshot lands.
    pub fn retry_vcs(&mut self, cwd: &str) {
        if self.state.vcs.get(cwd).is_some_and(|v| v.get("error").is_some()) {
            let _ = self.tx.send(Command::Watch(Route::Vcs(cwd.to_owned())));
        }
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
    /// Whether a [`Net::watch_resource`] subscription is open, answered or not.
    pub fn watching(&self, method: &str, payload: Value) -> bool {
        self.resources.contains_key(&Route::Resource(method.into(), payload.to_string()))
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
        let id = next_id();
        let base = self.base_url.clone();
        let auth = self.auth.clone();
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
                let agent = ureq::AgentBuilder::new().redirects(0).timeout(std::time::Duration::from_secs(30)).build();
                let response = auth::send(&auth, &agent, &method, url.as_str(), |req| if method == "GET" { req.call() } else { req.send_json(&payload) });
                response.map_err(auth::Failed::json)?.into_json::<Value>().map_err(|e| json!({"message":e.to_string()}))
            })();
            let _ = tx.send(Incoming::Reply(id, result));
            wake();
        });
        id
    }
    /// Fetches the history page before a bounded thread's window; `merge_history` applies the reply.
    pub fn load_earlier(&mut self, thread_id: &str) -> Option<(u64, String)> {
        let cursor = self.state.thread_views.get(thread_id)?.history_cursor.clone()?;
        let id = self.history_page(thread_id, &[("cursor", cursor.as_str()), ("deferWork", "1")])?;
        Some((id, cursor))
    }
    /// Fetches the next page of the work deferred behind `anchor`; `merge_work` applies the reply.
    pub fn load_work(&mut self, thread_id: &str, anchor: &str) -> Option<(u64, String)> {
        let cursor = self.state.work_cursor(thread_id, anchor)?.to_owned();
        let id = self.history_page(thread_id, &[("cursor", cursor.as_str()), ("view", "work"), ("throughEntryId", anchor)])?;
        Some((id, cursor))
    }
    fn history_page(&self, thread_id: &str, query: &[(&str, &str)]) -> Option<u64> {
        let id = next_id();
        let mut url = url::Url::parse(&self.base_url).ok()?;
        url.path_segments_mut().ok()?.clear().extend(["api", "orchestration", "threads", thread_id, "history"]);
        url.query_pairs_mut().clear().extend_pairs(query.iter().copied());
        url.set_fragment(None);
        let auth = self.auth.clone();
        let tx = self.incoming_tx.clone();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let agent = ureq::AgentBuilder::new().redirects(0).timeout(std::time::Duration::from_secs(30)).build();
            let result = auth::send(&auth, &agent, "GET", url.as_str(), |req| req.set("x-t3-orchestration-protocol", "2").call())
                .map_err(auth::Failed::json)
                .and_then(|response| response.into_json::<Value>().map_err(|e| json!({"message":e.to_string()})));
            let _ = tx.send(Incoming::Reply(id, result));
            wake();
        });
        Some(id)
    }
    /// POSTs a file's bytes to an `attachments.createUploadUrl` URL, on the RPC reply channel.
    pub fn upload_file(&mut self, relative_url: &str, path: &std::path::Path, mime: &str) -> u64 {
        let id = next_id();
        let url = upload_url(&self.base_url, relative_url);
        let auth = self.auth.clone();
        let path = path.to_owned();
        let mime = mime.to_owned();
        let tx = self.incoming_tx.clone();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let url = url?;
                let bytes = std::fs::read(&path).map_err(|e| json!({"message":e.to_string()}))?;
                let agent = ureq::AgentBuilder::new().redirects(0).timeout(std::time::Duration::from_secs(600)).build();
                auth::send(&auth, &agent, "POST", &url, |req| req.set("Content-Type", &mime).send_bytes(&bytes)).map(|_| Value::Null).map_err(|failed| json!({"message": failed.text()}))
            })();
            let _ = tx.send(Incoming::Reply(id, result));
            wake();
        });
        id
    }
    pub fn request(&mut self, tag: &str, payload: Value) -> u64 {
        let id = next_id();
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
    /// An id no server will answer, for a reply this client is handed instead (see [`Net::put_reply`]).
    pub fn reserve_id(&self) -> u64 {
        next_id()
    }
    /// Answers `id` as the server would, for a request that other clients carried.
    pub fn put_reply(&mut self, id: u64, reply: Reply) {
        self.replies.insert(id, reply);
    }
    pub fn take_reply(&mut self, id: u64) -> Option<Reply> {
        self.replies.remove(&id)
    }
    /// Stream items a pending [`Net::request`] has received so far; its reply arrives on `Exit`.
    pub fn take_chunks(&mut self, id: u64) -> Vec<Value> {
        self.chunks.remove(&id).unwrap_or_default()
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
/// A WebSocket to one of the server's `/api` routes as this client, such as the preview frame
/// stream. Reads time out after `read_timeout`, so the caller's loop can also write.
/// What an [`Net::in_memory_sent`] connection was sent.
#[cfg(feature = "test-support")]
pub struct Sent(Receiver<Command>);

#[cfg(feature = "test-support")]
impl Sent {
    /// Requests sent since the last call, in order: id, method, payload.
    pub fn requests(&self) -> Vec<(u64, String, Value)> {
        self.0
            .try_iter()
            .filter_map(|command| match command {
                Command::Request(id, tag, payload) => Some((id, tag, payload)),
                _ => None,
            })
            .collect()
    }
}

pub fn api_socket(
    base: &str,
    auth: &Auth,
    path: &str,
    query: &str,
    read_timeout: std::time::Duration,
) -> Result<tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>, String> {
    use tungstenite::client::IntoClientRequest;
    use tungstenite::stream::MaybeTlsStream;
    if !path.starts_with("/api/") {
        return Err("Unsupported API path".into());
    }
    let mut url = url::Url::parse(base).map_err(|e| e.to_string())?;
    let (scheme, http) = match url.scheme() {
        "https" | "wss" => ("wss", "https"),
        "http" | "ws" => ("ws", "http"),
        _ => return Err("unsupported server URL scheme".into()),
    };
    url.set_path(path);
    url.set_query(Some(query).filter(|q| !q.is_empty()));
    url.set_fragment(None);
    // As `auth::send`: a refused DPoP token is renewed and the upgrade tried once more.
    let mut rejected: Option<String> = None;
    let mut socket = loop {
        // The server checks an upgrade's proof against the request as HTTP.
        url.set_scheme(http).map_err(|_| "invalid server URL")?;
        let (headers, token) = auth.headers("GET", url.as_str(), rejected.as_deref())?;
        url.set_scheme(scheme).map_err(|_| "invalid server URL")?;
        let mut req = url.as_str().into_client_request().map_err(|e| e.to_string())?;
        for (name, value) in headers {
            req.headers_mut().insert(name, value.parse().map_err(|_| "invalid authorization header")?);
        }
        match tungstenite::connect(req) {
            Ok((socket, _)) => break socket,
            Err(tungstenite::Error::Http(response)) if response.status() == 401 && token.is_some() && rejected.is_none() => rejected = token,
            Err(error) => return Err(error.to_string()),
        }
    };
    let tcp = match socket.get_mut() {
        MaybeTlsStream::Plain(tcp) => tcp,
        MaybeTlsStream::Rustls(tls) => &mut tls.sock,
        _ => return Err("unsupported TLS stream".into()),
    };
    tcp.set_read_timeout(Some(read_timeout)).map_err(|e| e.to_string())?;
    tcp.set_write_timeout(Some(std::time::Duration::from_secs(5))).map_err(|e| e.to_string())?;
    Ok(socket)
}
fn api_url(base: &str, path: &str, query: &str) -> Result<url::Url, String> {
    if !path.starts_with("/api/") {
        return Err("Unsupported API path".into());
    }
    let mut url = url::Url::parse(base).map_err(|e| e.to_string())?;
    url.set_path(path);
    url.set_query(Some(query).filter(|q| !q.is_empty()));
    url.set_fragment(None);
    Ok(url)
}
fn api_body(response: ureq::Response, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(response.into_reader(), limit), &mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}
/// The body of a GET on one of the server's `/api` routes as this client, at most `limit` bytes.
pub fn api_get(base: &str, auth: &Auth, path: &str, query: &str, limit: u64) -> Result<Vec<u8>, String> {
    let url = api_url(base, path, query)?;
    let agent = ureq::AgentBuilder::new().redirects(0).build();
    api_body(auth::send(auth, &agent, "GET", url.as_str(), |req| req.call()).map_err(auth::Failed::text)?, limit)
}
/// A POST of `body` to one of the server's `/api` routes as this client.
pub fn api_post(base: &str, auth: &Auth, path: &str, query: &str, content_type: &str, body: &[u8]) -> Result<(), String> {
    let url = api_url(base, path, query)?;
    let agent = ureq::AgentBuilder::new().redirects(0).timeout(std::time::Duration::from_secs(120)).build();
    auth::send(auth, &agent, "POST", url.as_str(), |req| req.set("Content-Type", content_type).send_bytes(body)).map(|_| ()).map_err(auth::Failed::text)
}
/// The body of an empty POST to one of the server's `/api` routes as this client, at most `limit` bytes.
pub fn api_post_read(base: &str, auth: &Auth, path: &str, query: &str, limit: u64) -> Result<Vec<u8>, String> {
    let url = api_url(base, path, query)?;
    let agent = ureq::AgentBuilder::new().redirects(0).timeout(std::time::Duration::from_secs(20)).build();
    api_body(auth::send(auth, &agent, "POST", url.as_str(), |req| req.call()).map_err(auth::Failed::text)?, limit)
}
/// A JSON POST on one of the server's `/api` routes, its response body at most `limit` bytes.
pub fn api_post_json(base: &str, auth: &Auth, path: &str, query: &str, body: &Value, timeout: std::time::Duration, limit: u64) -> Result<Vec<u8>, String> {
    let url = api_url(base, path, query)?;
    let agent = ureq::AgentBuilder::new().redirects(0).timeout(timeout).build();
    let body = body.to_string();
    api_body(auth::send(auth, &agent, "POST", url.as_str(), |req| req.set("Content-Type", "application/json").send_string(&body)).map_err(auth::Failed::text)?, limit)
}
/// The absolute URL of a signed upload path; nothing outside the server's upload route.
pub fn upload_url(base: &str, relative_url: &str) -> Result<String, Value> {
    if !relative_url.starts_with("/api/attachments/upload/") {
        return Err(json!({"message":"Unsupported upload path"}));
    }
    url::Url::parse(base)
        .and_then(|base| base.join(relative_url))
        .map(|url| url.to_string())
        .map_err(|e| json!({"message":e.to_string()}))
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
    /// Prepends an `OrchestrationV2ThreadHistoryPage` fetched with `cursor`. A page for a cursor the
    /// view has moved past (a newer snapshot or page) is dropped.
    pub fn merge_history(&mut self, thread_id: &str, cursor: &str, page: &Value) {
        let Some(view) = self.thread_views.get_mut(thread_id) else { return };
        if view.history_cursor.as_deref() != Some(cursor) {
            return;
        }
        projection::merge_older(&mut view.projection, projection::rows(page, "items"));
        view.history_cursor = page["nextCursor"].as_str().map(str::to_owned);
        view.has_more_history = page["hasMoreHistory"] == true;
    }
    /// The cursor of the work the server deferred behind `anchor`, while some is left.
    pub fn work_cursor(&self, thread_id: &str, anchor: &str) -> Option<&str> {
        let view = self.thread_views.get(thread_id)?;
        let row = projection::rows(&view.projection, "visibleTurnItems").iter().find(|r| r["sourceItemId"] == anchor)?;
        row["item"]["deferredWork"]["cursor"].as_str()
    }
    /// Applies a page of `anchor`'s deferred work fetched with `cursor` (`loadEarlierHistory` with
    /// a `workAnchorId`). `Ok(true)` means more is deferred and the next page should be asked for.
    /// A reply the anchor moved past fails while its work is still deferred.
    pub fn merge_work(&mut self, thread_id: &str, anchor: &str, cursor: &str, page: &Value) -> Result<bool, String> {
        match self.work_cursor(thread_id, anchor) {
            None => return Ok(false),
            Some(current) if current != cursor => return Err("Work history changed. Retry loading it.".into()),
            Some(_) => {}
        }
        let next = page["nextCursor"].as_str().filter(|c| !c.is_empty());
        if next == Some(cursor) {
            return Err("Work history did not advance.".into());
        }
        let Some(view) = self.thread_views.get_mut(thread_id) else { return Ok(false) };
        projection::merge_work(&mut view.projection, anchor, projection::rows(page, "items"), next);
        Ok(next.is_some())
    }
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
                self.shell_revision += 1;
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
                        let mut fresh = item["projection"].clone();
                        let kept = projection::kept_before(&view.projection, &fresh);
                        view.sequence = item["snapshotSequence"].as_u64().unwrap_or(0);
                        view.synchronized = false;
                        if kept.is_empty() {
                            view.history_cursor = item["historyCursor"].as_str().map(str::to_owned);
                            view.has_more_history = item["hasMoreHistory"] == true;
                        } else {
                            // A reconnect keeps the turns already shown; paging resumes before them.
                            projection::merge_older(&mut fresh, &kept);
                        }
                        view.projection = fresh;
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
                if let Some(error) = item.get("error") {
                    value["error"] = error.clone();
                } else if !value["local"].is_null() {
                    if let Some(map) = value.as_object_mut() {
                        map.remove("error");
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
