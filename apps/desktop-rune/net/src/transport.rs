//! One socket owner: acknowledges before delivering chunks, retains resume cursors across loss.
use crate::{Conn, Reply};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::{Receiver, RecvTimeoutError, Sender},
    time::{Duration, Instant},
};
use tungstenite::{Message, client::IntoClientRequest, stream::MaybeTlsStream};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) enum Route {
    Shell,
    Config,
    Thread(String),
    Vcs(String),
    Resource(String, String),
}
pub(crate) enum Command {
    Watch(Route),
    Unwatch(Route),
    Request(u64, String, Value),
    Reconnect,
    Stop,
}
pub(crate) enum Incoming {
    Connection(Conn),
    /// The trace ID of the failure behind the `Conn::Closed` that follows, when one was sent.
    Trace(Option<String>),
    Item(Route, Value),
    Reply(u64, Reply),
    /// Items a streaming request (`git.runStackedAction`) emits before its `Exit`.
    Chunk(u64, Vec<Value>),
}
struct Subscription {
    id: u64,
    sequence: Option<u64>,
}
fn request(id: u64, tag: &str, payload: Value) -> Value {
    json!({"_tag":"Request","id":id,"tag":tag,"payload":payload,"headers":[]})
}
fn subscribe(route: &Route, sub: &Subscription) -> Value {
    let (tag, mut payload) = match route {
        Route::Shell => (
            "orchestration.subscribeShell",
            json!({"requestCompletionMarker":true}),
        ),
        // Opt in to `environmentThemesUpdated`, which the palette lists as published themes.
        Route::Config => ("subscribeServerConfig", json!({"environmentThemes": true})),
        Route::Thread(id) => (
            "orchestration.subscribeThread",
            json!({"threadId":id,"requestCompletionMarker":true,"acceptBoundedSnapshot":true,"acceptDeferredWork":true}),
        ),
        Route::Vcs(cwd) => ("subscribeVcsStatus", json!({"cwd":cwd})),
        Route::Resource(method, payload) => (
            method.as_str(),
            serde_json::from_str(payload).unwrap_or(json!({})),
        ),
    };
    if matches!(route, Route::Shell | Route::Thread(_)) {
        if let Some(seq) = sub.sequence {
            payload["afterSequence"] = json!(seq);
        }
    }
    request(sub.id, tag, payload)
}
fn frames(text: &str) -> Result<Vec<Value>, serde_json::Error> {
    let v: Value = serde_json::from_str(text)?;
    Ok(match v {
        Value::Array(v) => v,
        v => vec![v],
    })
}
fn socket_url(base: &str) -> Result<String, String> {
    let mut url = url::Url::parse(base).map_err(|e| e.to_string())?;
    let scheme = match url.scheme() {
        "https" | "wss" => "wss",
        "http" | "ws" => "ws",
        _ => return Err("unsupported server URL scheme".into()),
    };
    url.set_scheme(scheme).map_err(|_| "invalid server URL")?;
    url.set_path("/ws");
    url.set_fragment(None);
    url.set_query(Some(
        "orchestrationProtocol=2&clientCapabilities=settingsSecretReferences",
    ));
    Ok(url.to_string())
}
/// A minted socket URL with the query this client always sends.
fn with_protocol(socket: &str) -> Result<String, String> {
    let mut url = url::Url::parse(socket).map_err(|e| e.to_string())?;
    url.query_pairs_mut().append_pair("orchestrationProtocol", "2").append_pair("clientCapabilities", "settingsSecretReferences");
    Ok(url.to_string())
}
fn handshake_trace(error: &tungstenite::Error) -> Option<String> {
    match error {
        tungstenite::Error::Http(response) => response.headers().get("traceparent").and_then(|v| v.to_str().ok()).and_then(crate::cloud::trace_id),
        _ => None,
    }
}
fn failure(message: &str) -> Reply {
    Err(json!({"_tag":"TransportError","message":message}))
}

pub(crate) fn run(
    base: String,
    auth: crate::Auth,
    subscribe_shell: bool,
    commands: Receiver<Command>,
    incoming: Sender<Incoming>,
    wake: impl Fn(),
) {
    let emit = |event| {
        let _ = incoming.send(event);
        wake();
    };
    let mut subs: HashMap<Route, Subscription> = HashMap::new();
    // Stream ids occupy a separate range that JavaScript can represent exactly.
    let mut next = 1u64 << 32;
    for route in [Route::Shell, Route::Config].into_iter().filter(|_| subscribe_shell) {
        subs.insert(
            route,
            Subscription {
                id: next,
                sequence: None,
            },
        );
        next += 1;
    }
    let mut backoff = Duration::from_millis(250);
    loop {
        emit(Incoming::Connection(Conn::Connecting));
        let mut trace = None;
        let result = (|| -> Result<(), String> {
            let req = match &auth {
                crate::Auth::Bearer(token) => {
                    let mut req = socket_url(&base)?
                        .into_client_request()
                        .map_err(|e| e.to_string())?;
                    req.headers_mut().insert(
                        "Authorization",
                        format!("Bearer {token}")
                            .parse()
                            .map_err(|_| "invalid bearer token")?,
                    );
                    req
                }
                crate::Auth::Dpop(dpop) => {
                    let url = dpop.socket().map_err(|refused| {
                        trace = refused.trace.clone();
                        refused.message
                    })?;
                    with_protocol(&url)?.into_client_request().map_err(|e| e.to_string())?
                }
            };
            let (mut socket, _) = tungstenite::connect(req).map_err(|e| {
                trace = handshake_trace(&e);
                e.to_string()
            })?;
            let tcp = match socket.get_mut() {
                MaybeTlsStream::Plain(tcp) => tcp,
                MaybeTlsStream::Rustls(tls) => &mut tls.sock,
                _ => return Err("unsupported TLS stream".into()),
            };
            tcp.set_read_timeout(Some(Duration::from_millis(50)))
                .map_err(|e| e.to_string())?;
            tcp.set_write_timeout(Some(Duration::from_secs(5)))
                .map_err(|e| e.to_string())?;
            for sub in subs.values_mut() {
                sub.id = next;
                next += 1;
            }
            for (route, sub) in &subs {
                socket
                    .send(Message::Text(subscribe(route, sub).to_string()))
                    .map_err(|e| e.to_string())?;
            }
            emit(Incoming::Connection(Conn::Open));
            let _renewal = match &auth {
                crate::Auth::Dpop(dpop) => Some(crate::auth::renew_while_open(dpop.clone())),
                crate::Auth::Bearer(_) => None,
            };
            let connected_at = Instant::now();
            let mut pending = HashSet::new();
            let mut inbound = Instant::now();
            let mut ping = Instant::now();
            let outcome = (|| -> Result<(), String> {
                loop {
                    while let Ok(command) = commands.try_recv() {
                        match command {
                            Command::Stop => return Ok(()),
                            Command::Reconnect => return Err("reconnect requested".into()),
                            Command::Watch(route) => {
                                if !subs.contains_key(&route) {
                                    let sub = Subscription {
                                        id: next,
                                        sequence: None,
                                    };
                                    next += 1;
                                    socket
                                        .send(Message::Text(subscribe(&route, &sub).to_string()))
                                        .map_err(|e| e.to_string())?;
                                    subs.insert(route, sub);
                                }
                            }
                            Command::Unwatch(route) => {
                                if let Some(sub) = subs.remove(&route) {
                                    socket
                                        .send(Message::Text(
                                            json!({"_tag":"Interrupt","requestId":sub.id})
                                                .to_string(),
                                        ))
                                        .map_err(|e| e.to_string())?;
                                }
                            }
                            Command::Request(id, tag, payload) => {
                                pending.insert(id);
                                socket
                                    .send(Message::Text(request(id, &tag, payload).to_string()))
                                    .map_err(|e| e.to_string())?;
                            }
                        }
                    }
                    if ping.elapsed() >= Duration::from_secs(5) {
                        socket
                            .send(Message::Text(json!({"_tag":"Ping"}).to_string()))
                            .map_err(|e| e.to_string())?;
                        ping = Instant::now();
                    }
                    if inbound.elapsed() >= Duration::from_secs(15) {
                        return Err("no inbound frame for 15 seconds".into());
                    }
                    let message = match socket.read() {
                        Ok(message) => {
                            inbound = Instant::now();
                            message
                        }
                        Err(tungstenite::Error::Io(e))
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) =>
                        {
                            continue;
                        }
                        Err(e) => return Err(e.to_string()),
                    };
                    let text = match message {
                        Message::Text(text) => text,
                        Message::Close(_) => return Err("server closed connection".into()),
                        _ => {
                            socket.flush().map_err(|e| e.to_string())?;
                            continue;
                        }
                    };
                    for frame in frames(&text).map_err(|e| e.to_string())? {
                        let id = frame["requestId"]
                            .as_u64()
                            .or_else(|| frame["requestId"].as_str().and_then(|s| s.parse().ok()))
                            .unwrap_or(u64::MAX);
                        match frame["_tag"].as_str().unwrap_or("") {
                            "Chunk" => {
                                socket
                                    .send(Message::Text(
                                        json!({"_tag":"Ack","requestId":frame["requestId"]})
                                            .to_string(),
                                    ))
                                    .map_err(|e| e.to_string())?;
                                if pending.contains(&id) {
                                    if let Some(values) = frame["values"].as_array() {
                                        emit(Incoming::Chunk(id, values.clone()));
                                    }
                                } else if let Some((route, sub)) =
                                    subs.iter_mut().find(|(_, s)| s.id == id)
                                {
                                    if let Some(values) = frame["values"].as_array() {
                                        for item in values {
                                            if item["kind"] == "snapshot"
                                                && item
                                                    .get("resolvedRepositoryIdentityRoots")
                                                    .is_none()
                                            {
                                                sub.sequence = item["snapshotSequence"]
                                                    .as_u64()
                                                    .or_else(|| {
                                                        item["snapshot"]["snapshotSequence"]
                                                            .as_u64()
                                                    });
                                            } else if let Some(seq) = item["sequence"].as_u64() {
                                                sub.sequence =
                                                    Some(sub.sequence.unwrap_or(0).max(seq));
                                            }
                                            emit(Incoming::Item(route.clone(), item.clone()));
                                        }
                                    }
                                }
                            }
                            "Exit" => {
                                if pending.remove(&id) {
                                    let exit = &frame["exit"];
                                    emit(Incoming::Reply(
                                        id,
                                        if exit["_tag"] == "Success" {
                                            Ok(exit["value"].clone())
                                        } else {
                                            Err(exit["cause"].clone())
                                        },
                                    ));
                                } else if let Some(route) = subs
                                    .iter()
                                    .find(|(route, s)| {
                                        matches!(route, Route::Resource(..) | Route::Vcs(_)) && s.id == id
                                    })
                                    .map(|(r, _)| r.clone())
                                {
                                    subs.remove(&route);
                                    emit(Incoming::Item(route, json!({"error":frame["exit"]})));
                                } else if let Some((_, sub)) =
                                    subs.iter_mut().find(|(_, s)| s.id == id)
                                {
                                    sub.sequence = None;
                                    return Err(format!("subscription ended: {}", frame["exit"]));
                                }
                            }
                            "Defect" => return Err(format!("server defect: {}", frame["defect"])),
                            _ => {}
                        }
                    }
                }
            })();
            for id in pending {
                emit(Incoming::Reply(
                    id,
                    failure("connection lost; command outcome may be unknown"),
                ));
            }
            let _ = socket.close(None);
            if connected_at.elapsed() >= Duration::from_secs(15) {
                backoff = Duration::from_millis(250);
            }
            outcome
        })();
        match result {
            Ok(()) => return,
            Err(reason) => {
                emit(Incoming::Trace(trace));
                emit(Incoming::Connection(Conn::Closed(reason)));
            }
        }
        let deadline = Instant::now() + backoff;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match commands.recv_timeout(remaining) {
                Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(Command::Watch(route)) => {
                    subs.entry(route).or_insert_with(|| {
                        let id = next;
                        next += 1;
                        Subscription { id, sequence: None }
                    });
                }
                Ok(Command::Unwatch(route)) => {
                    subs.remove(&route);
                }
                Ok(Command::Request(id, _, _)) => {
                    emit(Incoming::Reply(id, failure("not connected")))
                }
                Ok(Command::Reconnect) => break,
                Err(RecvTimeoutError::Timeout) => break,
            }
        }
        backoff = (backoff * 2).min(Duration::from_secs(8));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codec() {
        let req = request(3, "test", json!({"x":1}));
        assert_eq!(
            req,
            json!({"_tag":"Request","id":3,"tag":"test","payload":{"x":1},"headers":[]})
        );
        assert_eq!(frames(&req.to_string()).unwrap(), vec![req.clone()]);
        assert_eq!(
            frames(&json!([req.clone(),{"_tag":"Pong"}]).to_string())
                .unwrap()
                .len(),
            2
        );
        assert!(frames("bad").is_err());
    }
    #[test]
    fn resume() {
        let sub = Subscription {
            id: 5,
            sequence: Some(42),
        };
        assert_eq!(
            subscribe(&Route::Thread("t".into()), &sub)["payload"],
            json!({"threadId":"t","requestCompletionMarker":true,"acceptBoundedSnapshot":true,"acceptDeferredWork":true,"afterSequence":42})
        );
        assert_eq!(
            subscribe(
                &Route::Resource(
                    "provider.auth.subscribe".into(),
                    json!({"instanceId":"custom"}).to_string()
                ),
                &sub
            ),
            json!({"_tag":"Request","id":5,"tag":"provider.auth.subscribe","payload":{"instanceId":"custom"},"headers":[]})
        );
        assert!(
            socket_url("https://example.com/pair#secret")
                .unwrap()
                .starts_with("wss://example.com/ws?")
        );
    }
}

#[cfg(test)]
mod socket_tests {
    use super::*;
    use std::net::TcpListener;
    fn read(ws: &mut tungstenite::WebSocket<std::net::TcpStream>) -> Value {
        serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap()
    }
    #[test]
    fn acknowledgements_replies_interrupt_and_resume() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let (events, incoming) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            run(
                format!("http://{address}"),
                crate::Auth::Bearer("test-token".into()),
                true,
                rx,
                events,
                || {},
            )
        });
        let (tcp, _) = listener.accept().unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut ws = tungstenite::accept_hdr(
            tcp,
            |request: &tungstenite::handshake::server::Request, response| {
                assert_eq!(request.headers()["Authorization"], "Bearer test-token");
                assert!(
                    request
                        .uri()
                        .query()
                        .unwrap()
                        .contains("orchestrationProtocol=2")
                );
                Ok(response)
            },
        )
        .unwrap();
        let subscriptions = [read(&mut ws), read(&mut ws)];
        let shell = subscriptions
            .iter()
            .find(|r| r["tag"] == "orchestration.subscribeShell")
            .unwrap()["id"]
            .clone();
        let config = subscriptions
            .iter()
            .find(|r| r["tag"] == "subscribeServerConfig")
            .unwrap()["id"]
            .clone();
        ws.send(Message::Text(json!([
            {"_tag":"Chunk","requestId":shell,"values":[{"kind":"snapshot","snapshot":{"snapshotSequence":42}},{"kind":"synchronized"}]},
            {"_tag":"Chunk","requestId":config,"values":[{"type":"snapshot","config":{}}]}
        ]).to_string())).unwrap();
        assert_eq!(read(&mut ws), json!({"_tag":"Ack","requestId":shell}));
        assert_eq!(read(&mut ws), json!({"_tag":"Ack","requestId":config}));
        tx.send(Command::Watch(Route::Thread("t".into()))).unwrap();
        let watch = read(&mut ws);
        assert_eq!(watch["payload"]["threadId"], "t");
        tx.send(Command::Unwatch(Route::Thread("t".into())))
            .unwrap();
        assert_eq!(
            read(&mut ws),
            json!({"_tag":"Interrupt","requestId":watch["id"]})
        );
        tx.send(Command::Request(1, "test".into(), json!({})))
            .unwrap();
        assert_eq!(read(&mut ws)["id"], 1);
        ws.send(Message::Text(
            json!({"_tag":"Exit","requestId":1,"exit":{"_tag":"Success","value":{"ok":true}}})
                .to_string(),
        ))
        .unwrap();
        loop {
            if let Incoming::Reply(1, result) =
                incoming.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert_eq!(result.unwrap(), json!({"ok":true}));
                break;
            }
        }
        // A streaming request's chunks reach the caller before its Exit.
        tx.send(Command::Request(3, "git.runStackedAction".into(), json!({})))
            .unwrap();
        assert_eq!(read(&mut ws)["id"], 3);
        ws.send(Message::Text(
            json!({"_tag":"Chunk","requestId":"3","values":[{"kind":"action_started"}]}).to_string(),
        ))
        .unwrap();
        assert_eq!(read(&mut ws), json!({"_tag":"Ack","requestId":"3"}));
        loop {
            if let Incoming::Chunk(3, values) =
                incoming.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert_eq!(values, vec![json!({"kind":"action_started"})]);
                break;
            }
        }
        ws.send(Message::Text(
            json!({"_tag":"Exit","requestId":"3","exit":{"_tag":"Success","value":null}}).to_string(),
        ))
        .unwrap();
        loop {
            if let Incoming::Reply(3, result) =
                incoming.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert!(result.is_ok());
                break;
            }
        }
        tx.send(Command::Request(2, "failure".into(), json!({})))
            .unwrap();
        read(&mut ws);
        ws.send(Message::Text(json!({"_tag":"Exit","requestId":2,"exit":{"_tag":"Failure","cause":[{"_tag":"Fail","error":"denied"}]}}).to_string())).unwrap();
        loop {
            if let Incoming::Reply(2, result) =
                incoming.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert!(result.is_err());
                break;
            }
        }
        drop(ws);
        let (tcp, _) = listener.accept().unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut ws = tungstenite::accept(tcp).unwrap();
        let subscriptions = [read(&mut ws), read(&mut ws)];
        let shell = subscriptions
            .iter()
            .find(|r| r["tag"] == "orchestration.subscribeShell")
            .unwrap();
        assert_eq!(shell["payload"]["afterSequence"], 42);
        tx.send(Command::Stop).unwrap();
        worker.join().unwrap();
    }
    #[test]
    fn a_requests_only_connection_sends_nothing_but_its_requests() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let (events, _incoming) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            run(format!("http://{address}"), crate::Auth::Bearer("test-token".into()), false, rx, events, || {})
        });
        let (tcp, _) = listener.accept().unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut ws = tungstenite::accept(tcp).unwrap();
        tx.send(Command::Request(7, "server.updateServer".into(), json!({"targetVersion":"1.0.0"})))
            .unwrap();
        let first = read(&mut ws);
        assert_eq!(
            (first["id"].clone(), first["tag"].clone()),
            (json!(7), json!("server.updateServer")),
            "FAILED: an outdated server's socket must not subscribe to the shell or config"
        );
        tx.send(Command::Stop).unwrap();
        worker.join().unwrap();
    }
    #[test]
    fn a_minted_socket_asks_before_each_attempt_and_keeps_the_refusals_trace() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tungstenite::handshake::server::{ErrorResponse, Request, Response};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let (events, incoming) = std::sync::mpsc::channel();
        let attempts = std::sync::Arc::new(AtomicUsize::new(0));
        struct Mint(std::sync::Arc<AtomicUsize>, std::net::SocketAddr);
        impl crate::Dpop for Mint {
            fn socket(&self) -> Result<String, crate::cloud::Failure> {
                if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(crate::cloud::Failure { message: "Could not connect relay environment: HTTP 502".into(), trace: Some("0af7651916cd43dd8448eb211c80319c".into()), status: Some(502) });
                }
                Ok(format!("ws://{}/ws?wsTicket=t-1&dpop=proof", self.1))
            }
            fn token(&self, _: Option<&str>) -> Result<String, crate::cloud::Failure> {
                Ok("unused".into())
            }
            fn expires_at(&self) -> Option<u64> {
                None
            }
            fn proof(&self, _: &str, _: &str, _: &str) -> Result<String, crate::cloud::Failure> {
                Ok("unused".into())
            }
        }
        let mint = crate::Auth::Dpop(std::sync::Arc::new(Mint(attempts.clone(), address)));
        let worker = std::thread::spawn(move || run("http://unused.invalid".into(), mint, false, rx, events, || {}));
        let (tcp, _) = listener.accept().unwrap();
        let mut seen = None;
        let _ws = tungstenite::accept_hdr(tcp, |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
            seen = Some((request.uri().to_string(), request.headers().contains_key("authorization")));
            Ok(response)
        })
        .unwrap();
        let (uri, authorized) = seen.unwrap();
        assert_eq!(uri, "/ws?wsTicket=t-1&dpop=proof&orchestrationProtocol=2&clientCapabilities=settingsSecretReferences");
        assert!(!authorized, "a minted socket carries no bearer");
        assert_eq!(attempts.load(Ordering::SeqCst), 2, "each attempt mints a fresh ticket");
        let events: Vec<Incoming> = incoming.try_iter().collect();
        assert!(
            matches!(events.as_slice(), [Incoming::Connection(Conn::Connecting), Incoming::Trace(Some(trace)), Incoming::Connection(Conn::Closed(reason)), ..] if trace == "0af7651916cd43dd8448eb211c80319c" && reason.ends_with("HTTP 502")),
            "the refusal's trace arrives before its Closed"
        );
        tx.send(Command::Stop).unwrap();
        worker.join().unwrap();
    }
}
