//! McpAppFrame: an MCP app's page live in its timeline row. Rune embeds no web view, so the
//! Default profile's Chrome runs the page (`browser::Live`) and the row shows and drives it. The
//! app posts to `window.parent`, which at the top level is the page itself, so `BRIDGE` hands
//! its JSON-RPC to `__s5Frame` and `__s5Deliver` brings the host's messages in. `Host` is web's
//! `makeMcpAppHost` as a state machine: it takes the page's messages and returns what the frame
//! must do, so the protocol is tested apart from Chrome. A row runs its app only while on screen;
//! scrolled away, the app is asked to tear down and its page closes.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use base64::Engine;
use rune::{El, Tx, col, el, row, text};
use serde_json::{Map, Value, json};

use super::browser::{Cmd, LIVE_LOAD, Live};
use crate::chat::details::html_render;
use crate::data::s;
use crate::theme::{self, T};
use crate::{App, Route, icons, kit};

pub const PROTOCOL_VERSION: &str = "2026-01-26";
const MAX_MESSAGE_BYTES: usize = 256 * 1024;
const MAX_PENDING: usize = 16;
const TEARDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_HEIGHT: f32 = 320.;
const MAX_HEIGHT: f32 = 2000.;
const MAX_DOWNLOAD_BYTES: usize = 25 * 1024 * 1024;
/// How long a click in the app lets it open a link or go full screen, as Chrome's transient
/// user activation does for an iframe.
const ACTIVATION: Duration = Duration::from_secs(5);

/// Runs before the app's scripts: its JSON-RPC posts go to the host, and the host's arrive as
/// messages from `window.parent`, which the app checks.
const BRIDGE: &str = r#"(() => {
  const post = window.postMessage.bind(window);
  window.postMessage = function (message, ...rest) {
    if (message && typeof message === "object" && message.jsonrpc === "2.0") {
      try { __s5Frame(JSON.stringify(message)); } catch (e) {}
      return;
    }
    return post(message, ...rest);
  };
  Object.defineProperty(window, "__s5Deliver", {
    value: (data) => window.dispatchEvent(new MessageEvent("message", { data, source: window, origin: "null" })),
  });
})();"#;

// ---------------------------------------------------------------------------------------------
// The tool call's app (shared/mcpApp.ts).

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reference {
    pub attachment_id: String,
    pub server: String,
    pub tool: String,
    csp: Option<Value>,
    permissions: Option<Value>,
    pub prefers_border: bool,
}

fn bounded_name(value: &Value) -> Option<String> {
    value.as_str().filter(|v| !v.trim().is_empty() && v.encode_utf16().count() <= 256).map(str::to_owned)
}

/// readMcpAppReference.
fn read_reference(value: &Value) -> Option<Reference> {
    value["resourceUri"].as_str().filter(|u| u.encode_utf16().count() <= 4096 && u.starts_with("ui://"))?;
    Some(Reference {
        attachment_id: bounded_name(&value["attachmentId"])?,
        server: bounded_name(&value["server"])?,
        tool: bounded_name(&value["tool"])?,
        csp: read_csp(&value["csp"]),
        permissions: read_permissions(&value["permissions"]),
        prefers_border: value["prefersBorder"] == true,
    })
}

/// DOMAIN_PATTERN: a scheme-qualified origin, its host optionally under a `*.` wildcard.
fn domain_allowed(domain: &str) -> bool {
    if domain.encode_utf16().count() > 256 {
        return false;
    }
    let lower = domain.to_ascii_lowercase();
    let Some(rest) = ["https://", "http://", "wss://", "ws://"].iter().find_map(|p| lower.strip_prefix(p)) else { return false };
    let rest = rest.strip_prefix("*.").unwrap_or(rest);
    let (host, port) = match rest.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (rest, None),
    };
    port.is_none_or(|p| (1..=5).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))
        && host.split('.').all(|label| !label.is_empty() && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
}

/// readMcpAppCsp.
fn read_csp(value: &Value) -> Option<Value> {
    let fields = value.as_object()?;
    let csp: Map<String, Value> = ["connectDomains", "resourceDomains", "frameDomains", "baseUriDomains"]
        .iter()
        .filter_map(|key| {
            let domains: Vec<Value> = fields.get(*key)?.as_array()?.iter().filter(|d| d.as_str().is_some_and(domain_allowed)).take(32).cloned().collect();
            (!domains.is_empty()).then(|| ((*key).to_owned(), Value::Array(domains)))
        })
        .collect();
    (!csp.is_empty()).then_some(Value::Object(csp))
}

/// readMcpAppPermissions.
fn read_permissions(value: &Value) -> Option<Value> {
    let fields = value.as_object()?;
    let granted: Map<String, Value> =
        ["camera", "microphone", "geolocation", "clipboardWrite"].iter().filter(|k| fields.get(**k).is_some_and(Value::is_object)).map(|k| ((*k).to_owned(), json!({}))).collect();
    (!granted.is_empty()).then_some(Value::Object(granted))
}

/// mcpAppFromToolItem: an app only when its server and tool name the item's own tool, so one
/// tool cannot show another's app.
pub fn reference(item: &Value) -> Option<Reference> {
    if s(item, "type") != "dynamic_tool" || !s(item, "toolName").contains('.') {
        return None;
    }
    let app = read_reference(&html_render::read_field(&item["output"], "t3McpApp", 0).0?)?;
    (s(item, "toolName") == format!("{}.{}", app.server, app.tool)).then_some(app)
}

/// mcpAppFileName.
fn file_name(app: &Reference) -> String {
    let (mut name, mut run) = (String::new(), false);
    for c in app.tool.chars() {
        let kept = c.is_ascii_alphanumeric() || "_.-".contains(c);
        if kept || !run {
            name.push(if kept { c } else { '-' });
        }
        run = !kept;
    }
    let name: String = name.chars().take(80).collect();
    format!("{}.html", if name.is_empty() { "app" } else { &name })
}

// ---------------------------------------------------------------------------------------------
// The host protocol (client-runtime/mcpApps/host.ts).

#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub arguments: Value,
    pub result: Option<Value>,
}

#[derive(Debug, PartialEq)]
pub enum Download {
    Embedded { name: String, bytes: Vec<u8> },
    Link { name: String, uri: String },
}

impl Download {
    fn name(&self) -> &str {
        match self {
            Download::Embedded { name, .. } | Download::Link { name, .. } => name,
        }
    }
}

/// What the frame does for its host. A request carrying an `id` is answered with `Host::settle`.
#[derive(Debug, PartialEq)]
pub enum Effect {
    /// A message for the app.
    Post(Value),
    CallTool { id: Value, name: String, arguments: Value },
    ReadResource { id: Value, uri: String },
    OpenLink { id: Value, url: String },
    SendMessage { id: Value, text: String },
    /// `ui/update-model-context`'s `content` and `structuredContent`, as given.
    UpdateModelContext { id: Value, context: Value },
    DisplayMode { id: Value, mode: String },
    Download { id: Value, files: Vec<Download> },
    RequestTeardown,
    /// `ui/notifications/size-changed`'s height, which is all the frame follows.
    Size { height: Option<f64> },
    /// The app answered `ui/resource-teardown`, or the wait ran out: its page can close.
    TornDown,
}

#[derive(Default)]
pub struct Host {
    sandbox: Value,
    initialized: bool,
    disposed: bool,
    tool_call: Option<ToolCall>,
    tool_call_sent: bool,
    pending: usize,
    sent_context: Option<Value>,
    app_modes: Option<Vec<String>>,
    next_request: u64,
    teardown: Option<(String, Instant)>,
}

/// normalizeMcpAppToolResult: providers report absent fields as null, which the SDK rejects.
fn normalize_result(result: &Value) -> Value {
    let mut out = json!({ "content": if result["content"].is_array() { result["content"].clone() } else { json!([]) } });
    if result["structuredContent"].is_object() {
        out["structuredContent"] = result["structuredContent"].clone();
    }
    if result["isError"] == true {
        out["isError"] = json!(true);
    }
    if result["_meta"].is_object() {
        out["_meta"] = result["_meta"].clone();
    }
    out
}

fn is_id(value: &Value) -> bool {
    value.is_string() || value.is_number()
}

fn resource_bytes(content: &Value) -> Option<Vec<u8>> {
    if let Some(text) = content["text"].as_str() {
        return Some(text.as_bytes().to_vec());
    }
    base64::engine::general_purpose::STANDARD.decode(content["blob"].as_str()?).ok()
}

/// downloadName: the given name, else the URI's last segment, made safe for a file name.
fn download_name(uri: &Value, name: &Value) -> String {
    let raw = match name.as_str().filter(|n| !n.trim().is_empty()) {
        Some(name) => name,
        None => uri.as_str().map_or("download", |u| u.split(['/', '?', '#']).filter(|p| !p.is_empty()).last().unwrap_or("download")),
    };
    let safe: String = raw.chars().map(|c| if "\\/:*?\"<>|".contains(c) || (c as u32) < 32 { '-' } else { c }).take(200).collect();
    if safe.is_empty() { "download".to_owned() } else { safe }
}

/// readDownloads: every entry an embedded resource with contents or a resource link.
fn read_downloads(contents: &Value) -> Option<Vec<Download>> {
    let entries = contents.as_array().filter(|a| !a.is_empty())?;
    entries
        .iter()
        .map(|entry| match s(entry, "type") {
            "resource" if entry["resource"].is_object() => {
                let resource = &entry["resource"];
                Some(Download::Embedded { name: download_name(&resource["uri"], &resource["name"]), bytes: resource_bytes(resource)? })
            }
            "resource_link" => {
                let uri = entry["uri"].as_str()?;
                Some(Download::Link { name: download_name(&entry["uri"], &entry["name"]), uri: uri.to_owned() })
            }
            _ => None,
        })
        .collect()
}

impl Host {
    pub fn new(app: &Reference) -> Host {
        let mut sandbox = Map::new();
        if let Some(csp) = &app.csp {
            sandbox.insert("csp".into(), csp.clone());
        }
        if let Some(permissions) = &app.permissions {
            sandbox.insert("permissions".into(), permissions.clone());
        }
        Host { sandbox: Value::Object(sandbox), ..Default::default() }
    }

    fn post(&self, out: &mut Vec<Effect>, message: Value) {
        if !self.disposed {
            out.push(Effect::Post(message));
        }
    }

    fn notify(&self, out: &mut Vec<Effect>, method: &str, params: Value) {
        self.post(out, json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn respond(&self, out: &mut Vec<Effect>, id: &Value, result: Value) {
        self.post(out, json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    fn fail(&self, out: &mut Vec<Effect>, id: &Value, code: i64, message: &str) {
        self.post(out, json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}));
    }

    fn send_tool_call(&mut self, out: &mut Vec<Effect>) {
        let Some(call) = self.tool_call.clone().filter(|_| self.initialized && !self.tool_call_sent) else { return };
        self.tool_call_sent = true;
        let arguments = if call.arguments.is_object() { call.arguments } else { json!({}) };
        self.notify(out, "ui/notifications/tool-input", json!({ "arguments": arguments }));
        if let Some(result) = call.result {
            self.notify(out, "ui/notifications/tool-result", normalize_result(&result));
        }
    }

    /// `answer`: hands a request to the frame, unless too many are already in flight.
    fn answer(&mut self, out: &mut Vec<Effect>, id: &Value, effect: Effect) {
        if self.pending >= MAX_PENDING {
            self.fail(out, id, -32000, "Too many requests in flight.");
            return;
        }
        self.pending += 1;
        out.push(effect);
    }

    /// Answers a request the frame was handed: its result, or why it failed.
    pub fn settle(&mut self, id: &Value, result: Result<Value, String>) -> Vec<Effect> {
        let mut out = vec![];
        self.pending = self.pending.saturating_sub(1);
        match result {
            Ok(value) => self.respond(&mut out, id, value),
            Err(message) => self.fail(&mut out, id, -32000, if message.trim().is_empty() { "Request failed." } else { &message }),
        }
        out
    }

    /// One message the app posted, as JSON text; `context` is the host context now.
    pub fn receive(&mut self, raw: &str, context: &Value) -> Vec<Effect> {
        let mut out = vec![];
        let Some(data) = serde_json::from_str::<Value>(raw).ok().filter(|d| !self.disposed && d.is_object() && d["jsonrpc"] == "2.0") else { return out };
        if raw.len() > MAX_MESSAGE_BYTES {
            if is_id(&data["id"]) && data["method"].is_string() {
                self.fail(&mut out, &data["id"], -32600, "Message too large.");
            }
            return out;
        }
        let Some(method) = data["method"].as_str() else {
            let answered = data.get("result").is_some() || data.get("error").is_some();
            let id = data["id"].as_str().filter(|_| answered);
            if id.is_some() && self.teardown.as_ref().map(|(t, _)| t.as_str()) == id {
                self.teardown = None;
                self.disposed = true;
                out.push(Effect::TornDown);
            }
            return out;
        };
        let empty = json!({});
        let params = if data["params"].is_object() { &data["params"] } else { &empty };
        if is_id(&data["id"]) {
            self.request(&mut out, &data["id"], method, params, context);
        } else {
            self.notification(&mut out, method, params, context);
        }
        out
    }

    fn request(&mut self, out: &mut Vec<Effect>, id: &Value, method: &str, params: &Value, context: &Value) {
        match method {
            "ui/initialize" => {
                self.app_modes = params["appCapabilities"]["availableDisplayModes"].as_array().map(|modes| modes.iter().filter_map(|m| m.as_str().map(str::to_owned)).collect());
                self.sent_context = Some(context.clone());
                let result = json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "hostInfo": {"name": "t3-code", "version": crate::desktop::updates::version()},
                    "hostCapabilities": {
                        "openLinks": {}, "serverTools": {}, "serverResources": {}, "logging": {},
                        "message": {"text": {}}, "updateModelContext": {"text": {}, "structuredContent": {}},
                        "downloadFile": {}, "sandbox": self.sandbox,
                    },
                    "hostContext": context,
                });
                self.respond(out, id, result);
            }
            "ping" => self.respond(out, id, json!({})),
            "tools/call" => {
                let arguments = if params.get("arguments").is_none_or(Value::is_null) { json!({}) } else { params["arguments"].clone() };
                match params["name"].as_str().filter(|n| !n.trim().is_empty() && arguments.is_object()) {
                    Some(name) => self.answer(out, id, Effect::CallTool { id: id.clone(), name: name.to_owned(), arguments }),
                    None => self.fail(out, id, -32602, "tools/call needs a tool name and object arguments."),
                }
            }
            "resources/read" => match params["uri"].as_str().filter(|u| !u.trim().is_empty()) {
                Some(uri) => self.answer(out, id, Effect::ReadResource { id: id.clone(), uri: uri.to_owned() }),
                None => self.fail(out, id, -32602, "resources/read needs a uri."),
            },
            "ui/open-link" => {
                let web = |u: &&str| u.get(..7).is_some_and(|h| h.eq_ignore_ascii_case("http://")) || u.get(..8).is_some_and(|h| h.eq_ignore_ascii_case("https://"));
                match params["url"].as_str().filter(web) {
                    Some(url) => self.answer(out, id, Effect::OpenLink { id: id.clone(), url: url.to_owned() }),
                    None => self.fail(out, id, -32602, "Only http(s) links can be opened."),
                }
            }
            "ui/message" => {
                // The SDK sends an array of content blocks; the spec text shows one block.
                let blocks: Vec<&Value> = match &params["content"] {
                    Value::Array(blocks) => blocks.iter().collect(),
                    block => vec![block],
                };
                let texts: Option<Vec<&str>> = blocks.iter().map(|b| b["text"].as_str().filter(|_| b["type"] == "text")).collect();
                let Some(texts) = texts.filter(|t| params["role"] == "user" && !t.is_empty()) else {
                    return self.fail(out, id, -32602, "Only text messages from the user role are supported.");
                };
                let text = texts.join("\n").trim().to_owned();
                if text.is_empty() {
                    return self.fail(out, id, -32602, "ui/message needs non-empty text.");
                }
                self.answer(out, id, Effect::SendMessage { id: id.clone(), text });
            }
            "ui/request-display-mode" => {
                let current = s(context, "displayMode");
                let offered = |m: &str| context["availableDisplayModes"].as_array().is_some_and(|a| a.iter().any(|v| v == m));
                // A mode the host lacks, or the app did not declare, leaves the current one.
                match params["mode"].as_str().filter(|m| offered(m) && self.app_modes.as_ref().is_none_or(|a| a.iter().any(|x| x.as_str() == *m)) && *m != current) {
                    Some(mode) => self.answer(out, id, Effect::DisplayMode { id: id.clone(), mode: mode.to_owned() }),
                    None => self.respond(out, id, json!({ "mode": current })),
                }
            }
            "ui/update-model-context" => {
                let (content, structured) = (params.get("content"), params.get("structuredContent"));
                if content.is_some_and(|c| !c.is_array()) || structured.is_some_and(|c| !c.is_object()) {
                    return self.fail(out, id, -32602, "Model context needs content blocks or an object of structured content.");
                }
                let mut context = Map::new();
                if let Some(content) = content {
                    context.insert("content".into(), content.clone());
                }
                if let Some(structured) = structured {
                    context.insert("structuredContent".into(), structured.clone());
                }
                self.answer(out, id, Effect::UpdateModelContext { id: id.clone(), context: Value::Object(context) });
            }
            "ui/download-file" => match read_downloads(&params["contents"]) {
                Some(files) => self.answer(out, id, Effect::Download { id: id.clone(), files }),
                None => self.fail(out, id, -32602, "ui/download-file needs embedded resources or resource links."),
            },
            _ => self.fail(out, id, -32601, &format!("Method not found: {method}")),
        }
    }

    fn notification(&mut self, out: &mut Vec<Effect>, method: &str, params: &Value, context: &Value) {
        match method {
            "ui/notifications/initialized" if !self.initialized => {
                self.initialized = true;
                self.send_tool_call(out);
                out.extend(self.update_context(context));
            }
            "ui/notifications/size-changed" => {
                let height = params["height"].as_f64();
                if params["width"].is_number() || height.is_some() {
                    out.push(Effect::Size { height });
                }
            }
            "ui/notifications/request-teardown" => out.push(Effect::RequestTeardown),
            _ => {}
        }
    }

    /// setToolCall: a running call has no result yet, so only its completion replaces it.
    pub fn set_tool_call(&mut self, call: ToolCall) -> Vec<Effect> {
        let mut out = vec![];
        let Some(current) = self.tool_call.as_mut().filter(|_| self.tool_call_sent) else {
            if self.tool_call.as_ref().is_none_or(|c| c.result.is_none()) {
                self.tool_call = Some(call);
            }
            self.send_tool_call(&mut out);
            return out;
        };
        let Some(result) = call.result.filter(|_| current.result.is_none()) else { return out };
        // The app already has this call's input; it only needs the result.
        current.result = Some(result.clone());
        self.notify(&mut out, "ui/notifications/tool-result", normalize_result(&result));
        out
    }

    /// updateHostContext: the top-level keys that changed since the app last heard, once it
    /// finished initializing.
    pub fn update_context(&mut self, next: &Value) -> Vec<Effect> {
        let mut out = vec![];
        let (Some(sent), Some(fields)) = (self.sent_context.as_ref().filter(|_| self.initialized), next.as_object()) else { return out };
        let changes: Map<String, Value> = fields.iter().filter(|(key, value)| sent.get(key.as_str()) != Some(*value)).map(|(k, v)| (k.clone(), v.clone())).collect();
        self.sent_context = Some(next.clone());
        if !changes.is_empty() {
            self.notify(&mut out, "ui/notifications/host-context-changed", Value::Object(changes));
        }
        out
    }

    /// Asks the app to wrap up before its page goes (`ui/resource-teardown`); `TornDown` once it
    /// answers or `tick` finds the wait over. An app that never initialized goes at once.
    pub fn teardown(&mut self, now: Instant) -> Vec<Effect> {
        if self.disposed || !self.initialized {
            self.disposed = true;
            return vec![Effect::TornDown];
        }
        let mut out = vec![];
        let id = format!("t3-host-{}", self.next_request);
        self.next_request += 1;
        self.post(&mut out, json!({"jsonrpc": "2.0", "id": id, "method": "ui/resource-teardown", "params": {}}));
        self.teardown = Some((id, now));
        out
    }

    /// When an unanswered `ui/resource-teardown` stops waiting.
    pub fn teardown_due(&self) -> Option<Instant> {
        self.teardown.as_ref().map(|(_, at)| *at + TEARDOWN_TIMEOUT)
    }

    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        if self.teardown.as_ref().is_some_and(|(_, at)| now.duration_since(*at) >= TEARDOWN_TIMEOUT) {
            self.teardown = None;
            self.disposed = true;
            return vec![Effect::TornDown];
        }
        vec![]
    }

    pub fn dispose(&mut self) {
        self.disposed = true;
    }
}

/// mcpAppStyleVariables: the app's standard variables from the theme's.
fn style_variables(theme: &Map<String, Value>) -> Map<String, Value> {
    const PAIRS: [(&str, &str); 22] = [
        ("--color-background-primary", "--background"),
        ("--color-background-secondary", "--card"),
        ("--color-background-tertiary", "--muted"),
        ("--color-background-inverse", "--foreground"),
        ("--color-background-info", "--info"),
        ("--color-background-danger", "--destructive-surface"),
        ("--color-background-warning", "--warning-surface"),
        ("--color-text-primary", "--foreground"),
        ("--color-text-secondary", "--muted-foreground"),
        ("--color-text-tertiary", "--muted-foreground"),
        ("--color-text-inverse", "--background"),
        ("--color-text-info", "--info-foreground"),
        ("--color-text-danger", "--destructive-foreground"),
        ("--color-text-success", "--success-foreground"),
        ("--color-text-warning", "--warning-foreground"),
        ("--color-border-primary", "--border"),
        ("--color-border-secondary", "--input"),
        ("--color-border-danger", "--destructive"),
        ("--color-ring-primary", "--ring"),
        ("--font-sans", "--font-sans"),
        ("--font-mono", "--font-mono"),
        ("--border-radius-md", "--radius"),
    ];
    PAIRS.iter().filter_map(|(name, from)| theme.get(*from).filter(|v| v.as_str().is_some_and(|v| !v.is_empty())).map(|v| ((*name).to_owned(), v.clone()))).collect()
}

/// McpAppRequestError's message, which may arrive as just its reason.
fn failure_message(cause: &Value) -> String {
    let error = cause.as_array().and_then(|c| c.iter().find(|r| s(r, "_tag") == "Fail")).map_or(cause, |r| &r["error"]);
    let message = s(error, "message").trim();
    if !message.is_empty() {
        return message.to_owned();
    }
    match s(error, "reason") {
        "not-an-app" => "This tool call has no MCP app.",
        "provider-unsupported" => "This thread's provider cannot run MCP app requests.",
        "session-stopped" => "The app's thread is not running. Send a message in the thread that created it to use the app again.",
        "tool-not-callable" => "This app cannot call that tool.",
        "unsupported-content" => "Only text and structured content are supported.",
        "request-failed" => "The app's MCP server request failed.",
        _ => "Request failed.",
    }
    .to_owned()
}

// ---------------------------------------------------------------------------------------------
// The frame.

#[derive(Default)]
pub struct Apps {
    frames: Vec<Frame>,
    /// Pages let go while their apps wrap up; each closes on its answer or the timeout.
    tearing: Vec<(Live, Host)>,
}

/// A row's app, as the timeline gives it.
#[derive(Clone)]
struct Seed {
    id: String,
    app: Reference,
    /// The thread and item that made the call, which the app's requests act for.
    thread: String,
    item: String,
    /// The thread on screen, which the app's model context and messages go to.
    conversation: String,
    revision: String,
}

#[derive(Default)]
struct Frame {
    id: String,
    app: Reference,
    thread: String,
    item: String,
    conversation: String,
    revision: String,
    width: f32,
    visible: bool,
    /// Its row left the timeline's virtual list, which unmounts it without a last layout.
    unlisted: bool,
    height: f32,
    dark: bool,
    styles: Value,
    /// The `assets.createUrl` in flight.
    url: Option<u64>,
    failed: bool,
    /// The `getTurnItem` in flight, and the revision it asks for.
    detail: Option<(u64, String)>,
    fetched: Option<String>,
    tool_call: Option<ToolCall>,
    /// The `toolInfo` asked once for the host context, and its tool.
    info: Option<u64>,
    asked_info: bool,
    tool: Option<Value>,
    live: Option<Live>,
    host: Host,
    loads: u32,
    closed: bool,
    navigated: bool,
    fullscreen: bool,
    /// The full-screen page's size, from its layout.
    full: [f32; 2],
    activated: Option<Instant>,
    /// Requests in flight for the app, by RPC id.
    calls: Vec<(u64, Call)>,
    /// A scene's picture, never loaded.
    still: bool,
}

enum Call {
    /// `tools/call`'s look at the tool before running it.
    Info { id: Value, name: String, arguments: Value },
    Tool(Value),
    Resource(Value),
    Context(Value),
    Message(Value),
    /// A linked file being read, and the files after it.
    Download { id: Value, name: String, rest: VecDeque<Download> },
}

impl Frame {
    fn new(seed: &Seed) -> Frame {
        Frame {
            id: seed.id.clone(),
            app: seed.app.clone(),
            thread: seed.thread.clone(),
            item: seed.item.clone(),
            conversation: seed.conversation.clone(),
            height: DEFAULT_HEIGHT,
            ..Default::default()
        }
    }

    fn live_id(&self) -> Option<u64> {
        self.live.as_ref().map(|l| l.id)
    }

    fn recently_activated(&self) -> bool {
        self.activated.is_some_and(|at| at.elapsed() < ACTIVATION)
    }

    /// McpAppFrame's hostContext.
    fn context(&self) -> Value {
        let mut context = json!({
            "theme": if self.dark { "dark" } else { "light" },
            "styles": {"variables": self.styles},
            "displayMode": if self.fullscreen { "fullscreen" } else { "inline" },
            "availableDisplayModes": ["inline", "fullscreen"],
            "containerDimensions": if self.fullscreen {
                json!({"width": self.full[0].round() as i64, "height": self.full[1].round() as i64})
            } else {
                json!({"width": self.width.round() as i64, "maxHeight": MAX_HEIGHT as i64})
            },
            "platform": "desktop",
            "locale": locale(),
            "timeZone": time_zone(),
            "userAgent": format!("t3-code/{}", crate::desktop::updates::version()),
            "deviceCapabilities": {"touch": false, "hover": true},
            "safeAreaInsets": {"top": 0, "right": 0, "bottom": 0, "left": 0},
        });
        if let Some(tool) = &self.tool {
            context["toolInfo"] = json!({ "tool": tool });
        }
        context
    }
}

/// The user's locale as a BCP 47 tag, from the environment.
fn locale() -> &'static str {
    static LOCALE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    LOCALE.get_or_init(read_locale)
}

fn read_locale() -> String {
    let raw = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.is_empty() && v != "C" && v != "POSIX");
    raw.and_then(|v| v.split(['.', '@']).next().map(|tag| tag.replace('_', "-"))).filter(|t| !t.is_empty()).unwrap_or_else(|| "en-US".to_owned())
}

/// The IANA time zone, from `TZ` or the `/etc/localtime` link.
fn time_zone() -> &'static str {
    static ZONE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ZONE.get_or_init(read_time_zone)
}

fn read_time_zone() -> String {
    let from_link = || std::fs::read_link("/etc/localtime").ok().and_then(|p| p.to_str().and_then(|p| p.split_once("zoneinfo/")).map(|(_, zone)| zone.to_owned()));
    std::env::var("TZ").ok().map(|z| z.trim_start_matches(':').to_owned()).filter(|z| !z.is_empty()).or_else(from_link).unwrap_or_else(|| "UTC".to_owned())
}

fn frame_mut<'a>(s: &'a mut App, id: &str) -> Option<&'a mut Frame> {
    s.ui.panel.mcp_apps.frames.iter_mut().find(|f| f.id == id)
}

/// The frame, while `live` is still its page.
fn live_frame<'a>(s: &'a mut App, id: &str, live: u64) -> Option<&'a mut Frame> {
    frame_mut(s, id).filter(|f| f.live_id() == Some(live))
}

fn cmd(s: &mut App, cmd: Cmd) -> bool {
    super::preview::inline_frame_cmd(s, cmd)
}

/// The inline page's size: the row's width at the app's height.
fn fit_inline(s: &mut App, id: &str) {
    let Some(frame) = frame_mut(s, id).filter(|f| !f.still) else { return };
    let (Some(live), false) = (frame.live_id(), frame.fullscreen) else { return };
    let (w, h) = (frame.width.max(1.) as u32, frame.height as u32);
    cmd(s, Cmd::LiveResize(live, w, h));
}

fn update_context(s: &mut App, id: &str) {
    let Some(frame) = frame_mut(s, id) else { return };
    let Some(live) = frame.live_id() else { return };
    let context = frame.context();
    let effects = frame.host.update_context(&context);
    run(s, id, live, effects);
}

/// The timeline's rows in view (`on_visible_rows`), with the row each app is in: an app whose
/// row left lets its page go, and one whose row came back may load again on its next layout.
pub fn in_view(s: &mut App, apps: &[(usize, String)], rows: std::ops::Range<usize>) -> bool {
    let mut changed = false;
    for (row, id) in apps {
        let Some(frame) = frame_mut(s, id) else { continue };
        let unlisted = !rows.contains(row);
        if frame.unlisted == unlisted {
            continue;
        }
        frame.unlisted = unlisted;
        changed = true;
        if unlisted && frame.visible {
            frame.visible = false;
            release(s, id);
        }
    }
    changed
}

/// Whether the thread waits on the user (an approval or a question): its apps stay inline so the
/// request shows, as web's step-aside.
fn awaiting_user(projection: &Value) -> bool {
    projection["runtimeRequests"].as_array().is_some_and(|requests| requests.iter().any(|r| s(r, "status") == "pending"))
}

fn thread_awaiting(app: &App, thread: &str) -> bool {
    app.state().thread_views.get(thread).is_some_and(|view| awaiting_user(&view.projection))
}

fn layout(s: &mut App, seed: &Seed, styles: &Value, dark: bool, [_, y, w, h]: [f32; 4]) -> bool {
    // Half a window of slack either side, so a short scroll back finds the app still running.
    let near = s.ui.desktop.window_size().is_none_or(|[_, height]| y + h > -height / 2. && y < height * 1.5);
    let apps = &mut s.ui.panel.mcp_apps;
    if !apps.frames.iter().any(|f| f.id == seed.id) {
        apps.frames.push(Frame::new(seed));
    }
    let Some(frame) = frame_mut(s, &seed.id) else { return false };
    let visible = near && !frame.unlisted;
    let resized = (frame.width - w.round()).abs() >= 1.;
    let themed = frame.dark != dark || frame.styles != *styles;
    let shown = frame.visible != visible;
    frame.width = w.round();
    frame.visible = visible;
    frame.dark = dark;
    if themed {
        frame.styles = styles.clone();
    }
    frame.revision.clone_from(&seed.revision);
    frame.conversation.clone_from(&seed.conversation);
    if !visible {
        if shown {
            release(s, &seed.id);
        }
        return shown;
    }
    if resized {
        fit_inline(s, &seed.id);
    }
    if resized || themed {
        update_context(s, &seed.id);
    }
    sync(s, &seed.id) | resized | themed | shown
}

/// Asks for what an on-screen frame lacks: its tool call at this revision, and its page.
fn sync(s: &mut App, id: &str) -> bool {
    let Some(frame) = s.ui.panel.mcp_apps.frames.iter_mut().find(|f| f.id == id) else { return false };
    if !frame.visible || frame.closed || frame.navigated || frame.failed || frame.still {
        return false;
    }
    let mut asked = false;
    let stale = frame.fetched.as_deref() != Some(frame.revision.as_str()) && frame.detail.as_ref().is_none_or(|(_, r)| *r != frame.revision);
    if let (true, Some(net)) = (stale, s.src.net()) {
        frame.detail = Some((net.request("orchestration.getTurnItem", json!({"threadId": frame.thread, "itemId": frame.item})), frame.revision.clone()));
        asked = true;
    }
    if frame.live.is_none() && frame.url.is_none() && frame.width >= 1. {
        let resource = json!({"resource": {"_tag": "attachment", "attachmentId": frame.app.attachment_id, "fileName": file_name(&frame.app), "mimeType": "text/html", "disposition": "inline", "renderIntent": "mcp-app"}});
        match s.src.net() {
            Some(net) => frame.url = Some(net.request("assets.createUrl", resource)),
            None => frame.failed = true,
        }
        asked = true;
    }
    asked
}

/// Lets a frame's page go: the app is asked to tear down and the page closes after.
fn release(s: &mut App, id: &str) {
    let Some(frame) = frame_mut(s, id).filter(|f| !f.still) else { return };
    frame.fullscreen = false;
    frame.calls.clear();
    let Some(live) = frame.live.take() else { return };
    let mut host = std::mem::take(&mut frame.host);
    let effects = host.teardown(Instant::now());
    if deliver(s, live.id, effects) {
        cmd(s, Cmd::LiveClose(live.id));
    } else {
        s.ui.panel.mcp_apps.tearing.push((live, host));
    }
}

/// Posts a let-go page's messages; whether its app is done.
fn deliver(s: &mut App, live: u64, effects: Vec<Effect>) -> bool {
    let mut done = false;
    for effect in effects {
        match effect {
            Effect::Post(message) => {
                cmd(s, Cmd::LiveEval(live, format!("window.__s5Deliver && window.__s5Deliver({message})")));
            }
            Effect::TornDown => done = true,
            _ => {}
        }
    }
    done
}

fn start(s: &mut App, id: &str, url: String) {
    let Some(frame) = frame_mut(s, id) else { return };
    let mut live = Live::new(url, frame.width.max(1.) as u32, frame.height as u32, frame.dark);
    live.script = Some(BRIDGE.to_owned());
    frame.host = Host::new(&frame.app);
    if let Some(call) = frame.tool_call.clone() {
        frame.host.set_tool_call(call);
    }
    frame.loads = 0;
    let ask_info = !frame.asked_info;
    frame.asked_info = true;
    let (thread, item, tool) = (frame.thread.clone(), frame.item.clone(), frame.app.tool.clone());
    if !cmd(s, Cmd::LiveOpen(live.clone())) {
        if let Some(frame) = frame_mut(s, id) {
            frame.failed = true;
        }
        return;
    }
    let info = ask_info.then(|| s.src.net().map(|net| net.request("mcpApps.toolInfo", json!({"threadId": thread, "itemId": item, "name": tool})))).flatten();
    if let Some(frame) = frame_mut(s, id) {
        frame.live = Some(live);
        frame.info = frame.info.or(info);
    }
}

/// What a frame heard since the last poll.
enum Heard {
    Url(Option<String>),
    Detail(Option<Value>),
    Info(Value),
    Reply(Call, Result<Value, Value>),
    Messages(u64, Vec<String>),
    Gone,
}

pub fn poll(s: &mut App, tx: &mut Tx) -> bool {
    let apps = &s.ui.panel.mcp_apps;
    if apps.frames.is_empty() && apps.tearing.is_empty() {
        return false;
    }
    let now = Instant::now();
    let mut changed = false;
    // Pages wrapping up: their answers, then a close once done.
    let mut posts = vec![];
    let mut done = vec![];
    for (live, host) in &mut s.ui.panel.mcp_apps.tearing {
        let mut effects: Vec<Effect> = live.messages().iter().filter(|m| *m != LIVE_LOAD).flat_map(|m| host.receive(m, &Value::Null)).collect();
        effects.extend(host.tick(now));
        if effects.contains(&Effect::TornDown) || live.gone() {
            done.push(live.id);
        }
        posts.push((live.id, effects));
    }
    for (live, effects) in posts {
        deliver(s, live, effects);
    }
    s.ui.panel.mcp_apps.tearing.retain(|(live, _)| !done.contains(&live.id));
    for live in done {
        cmd(s, Cmd::LiveClose(live));
    }
    // A silent app is let go on time, not at the next wake that happens by.
    if let Some(due) = s.ui.panel.mcp_apps.tearing.iter().filter_map(|(_, host)| host.teardown_due()).min() {
        tx.wake_after(due.saturating_duration_since(now));
    }
    let stepping: Vec<String> = s.ui.panel.mcp_apps.frames.iter().filter(|f| f.fullscreen && thread_awaiting(s, &f.conversation)).map(|f| f.id.clone()).collect();
    for id in &stepping {
        exit_fullscreen(s, id);
    }
    changed |= !stepping.is_empty();
    // Apps of a thread no longer on screen go.
    let route = match s.frame_route() {
        Route::Thread(thread) => Some(thread.clone()),
        _ => None,
    };
    let left: Vec<String> = s.ui.panel.mcp_apps.frames.iter().filter(|f| route.as_deref() != Some(f.conversation.as_str()) && !f.still).map(|f| f.id.clone()).collect();
    for id in &left {
        release(s, id);
    }
    s.ui.panel.mcp_apps.frames.retain(|f| !left.contains(&f.id));
    changed |= !left.is_empty();

    let base = s.src.net().map(|net| net.base_url().trim_end_matches('/').to_owned()).unwrap_or_default();
    let mut heard: Vec<(String, Heard)> = vec![];
    for frame in &mut s.ui.panel.mcp_apps.frames {
        if let Some(net) = s.src.net() {
            if let Some(reply) = frame.url.and_then(|id| net.take_reply(id)) {
                frame.url = None;
                let path = reply.ok().and_then(|v| v["relativeUrl"].as_str().map(str::to_owned));
                heard.push((frame.id.clone(), Heard::Url(path.map(|p| format!("{base}{p}")))));
            }
            if let Some(reply) = frame.detail.as_ref().and_then(|(id, _)| net.take_reply(*id)) {
                if let Some((_, revision)) = frame.detail.take() {
                    frame.fetched = Some(revision);
                }
                heard.push((frame.id.clone(), Heard::Detail(reply.ok().map(|v| v["item"].clone()).filter(Value::is_object))));
            }
            if let Some(reply) = frame.info.and_then(|id| net.take_reply(id)) {
                frame.info = None;
                heard.push((frame.id.clone(), Heard::Info(reply.map(|v| v["tool"].clone()).unwrap_or_default())));
            }
            let mut index = 0;
            while index < frame.calls.len() {
                match net.take_reply(frame.calls[index].0) {
                    Some(reply) => heard.push((frame.id.clone(), Heard::Reply(frame.calls.remove(index).1, reply))),
                    None => index += 1,
                }
            }
        }
        if let Some(live) = &frame.live {
            if live.gone() && !frame.still {
                heard.push((frame.id.clone(), Heard::Gone));
            } else {
                let messages = live.messages();
                if !messages.is_empty() {
                    heard.push((frame.id.clone(), Heard::Messages(live.id, messages)));
                }
            }
        }
    }
    for (id, event) in heard {
        changed = true;
        hear(s, &id, event);
    }
    changed
}

fn hear(s: &mut App, id: &str, event: Heard) {
    match event {
        Heard::Url(Some(url)) => {
            if frame_mut(s, id).is_some_and(|f| f.visible && f.live.is_none() && !f.closed && !f.navigated) {
                start(s, id, url);
            }
        }
        Heard::Url(None) => {
            if let Some(frame) = frame_mut(s, id) {
                frame.failed = true;
            }
        }
        Heard::Detail(item) => {
            let Some(frame) = frame_mut(s, id) else { return };
            let Some(item) = item.filter(|i| i["type"] == "dynamic_tool") else { return };
            let result = Some(item["output"]["result"].clone()).filter(|r| !r.is_null());
            let call = ToolCall { arguments: item["input"].clone(), result };
            frame.tool_call = Some(call.clone());
            let Some(live) = frame.live_id() else { return };
            let effects = frame.host.set_tool_call(call);
            run(s, id, live, effects);
        }
        Heard::Info(tool) => {
            if let Some(frame) = frame_mut(s, id) {
                frame.tool = Some(tool).filter(|t| !t.is_null());
            }
            update_context(s, id);
        }
        Heard::Reply(call, reply) => answer(s, id, call, reply),
        Heard::Messages(live, messages) => {
            for message in messages {
                let Some(frame) = live_frame(s, id, live) else { return };
                if message == LIVE_LOAD {
                    // A second load is the app navigating away, which could impersonate another.
                    frame.loads += 1;
                    if frame.loads > 1 {
                        frame.host.dispose();
                        frame.navigated = true;
                        frame.fullscreen = false;
                        frame.live = None;
                        cmd(s, Cmd::LiveClose(live));
                        return;
                    }
                    continue;
                }
                let context = frame.context();
                let effects = frame.host.receive(&message, &context);
                run(s, id, live, effects);
            }
        }
        Heard::Gone => {
            if let Some(frame) = frame_mut(s, id) {
                frame.live = None;
                frame.fullscreen = false;
                frame.calls.clear();
                frame.host = Host::default();
            }
        }
    }
}

fn settle(s: &mut App, id: &str, live: u64, request: &Value, result: Result<Value, String>) {
    let Some(frame) = live_frame(s, id, live) else { return };
    let effects = frame.host.settle(request, result);
    run(s, id, live, effects);
}

fn rpc(s: &mut App, id: &str, live: u64, method: &str, params: Value, call: Call) {
    let Some(request) = s.src.net().map(|net| net.request(method, params)) else {
        let request = match &call {
            Call::Info { id, .. } | Call::Tool(id) | Call::Resource(id) | Call::Context(id) | Call::Message(id) | Call::Download { id, .. } => id.clone(),
        };
        return settle(s, id, live, &request, Err("Reconnect to the environment and try again.".to_owned()));
    };
    if let Some(frame) = live_frame(s, id, live) {
        frame.calls.push((request, call));
    }
}

/// The frame's request target, as `{threadId, itemId}` with `extra`'s fields.
fn target(frame: &Frame, extra: Value) -> Value {
    let mut params = json!({"threadId": frame.thread, "itemId": frame.item});
    if let (Some(params), Value::Object(extra)) = (params.as_object_mut(), extra) {
        params.extend(extra);
    }
    params
}

/// Asks the user, inline so the question shows above the app.
fn ask(s: &mut App, id: &str, live: u64, message: String, request: Value, on_confirm: impl Fn(&mut App) + 'static, declined: Result<Value, String>) {
    exit_fullscreen(s, id);
    let id = id.to_owned();
    crate::shell::confirm::request_with(s, message, false, move |s: &mut App, _: &mut rune::Tx| on_confirm(s), move |s: &mut App| settle(s, &id, live, &request, declined.clone()));
}

/// Carries out what the host handed the frame.
fn run(s: &mut App, id: &str, live: u64, effects: Vec<Effect>) {
    for effect in effects {
        let Some(frame) = live_frame(s, id, live) else { return };
        let (server, conversation) = (frame.app.server.clone(), frame.conversation.clone());
        match effect {
            Effect::Post(message) => {
                cmd(s, Cmd::LiveEval(live, format!("window.__s5Deliver && window.__s5Deliver({message})")));
            }
            Effect::CallTool { id: request, name, arguments } => {
                let params = target(frame, json!({ "name": name }));
                rpc(s, id, live, "mcpApps.toolInfo", params, Call::Info { id: request, name, arguments });
            }
            Effect::ReadResource { id: request, uri } => {
                let params = target(frame, json!({ "uri": uri }));
                rpc(s, id, live, "mcpApps.readResource", params, Call::Resource(request));
            }
            Effect::OpenLink { id: request, url } => {
                if !frame.recently_activated() {
                    settle(s, id, live, &request, Err("Links open only from a click in the app.".to_owned()));
                    continue;
                }
                super::open_preview(s, &url);
                settle(s, id, live, &request, Ok(json!({})));
            }
            Effect::SendMessage { id: request, text } => {
                let (frame_id, thread) = (id.to_owned(), conversation.clone());
                let (reply_to, body) = (request.clone(), text.clone());
                let send = move |s: &mut App| {
                    let command = json!({
                        "type": "message.dispatch", "commandId": s5_net::uuid(), "createdBy": "user", "creationSource": "web",
                        "threadId": thread, "messageId": s5_net::uuid(), "text": body, "attachments": [],
                        "dispatchMode": {"type": "queue_after_active"},
                    });
                    match s.src.net().map(|net| net.dispatch(command)) {
                        Some(sent) => {
                            if let Some(frame) = live_frame(s, &frame_id, live) {
                                frame.calls.push((sent, Call::Message(reply_to.clone())));
                            }
                        }
                        None => settle(s, &frame_id, live, &reply_to, Err("Reconnect to the environment and try again.".to_owned())),
                    }
                };
                ask(s, id, live, format!("Send this message from {server}?\n{text}"), request, send, Err("Declined by the user.".to_owned()));
            }
            Effect::UpdateModelContext { id: request, context } => {
                let mut params = target(frame, context);
                params["conversationThreadId"] = json!(conversation);
                rpc(s, id, live, "mcpApps.updateModelContext", params, Call::Context(request));
            }
            Effect::DisplayMode { id: request, mode } => {
                let others = s.ui.panel.mcp_apps.frames.iter().any(|f| f.fullscreen && f.id != id);
                let waiting = thread_awaiting(s, &conversation);
                let allowed = mode == "fullscreen" && !others && !waiting && crate::shell::confirm::current(s).is_none() && live_frame(s, id, live).is_some_and(|f| f.recently_activated());
                if mode == "fullscreen" && allowed {
                    if let Some(frame) = live_frame(s, id, live) {
                        frame.fullscreen = true;
                    }
                    fit_fullscreen(s, id);
                    update_context(s, id);
                } else if mode == "inline" {
                    exit_fullscreen(s, id);
                }
                let current = if live_frame(s, id, live).is_some_and(|f| f.fullscreen) { "fullscreen" } else { "inline" };
                settle(s, id, live, &request, Ok(json!({ "mode": current })));
            }
            Effect::Download { id: request, files } => {
                let names = files.iter().map(Download::name).collect::<Vec<_>>().join(", ");
                let files = std::cell::RefCell::new(Some(VecDeque::from(files)));
                let (frame_id, reply_to) = (id.to_owned(), request.clone());
                let save = move |s: &mut App| {
                    if let Some(files) = files.borrow_mut().take() {
                        download(s, &frame_id, live, reply_to.clone(), files);
                    }
                };
                ask(s, id, live, format!("Save {names} from {server}?"), request, save, Ok(json!({"isError": true})));
            }
            Effect::RequestTeardown => {
                if frame.fullscreen {
                    exit_fullscreen(s, id);
                } else {
                    frame.closed = true;
                    release(s, id);
                }
            }
            Effect::Size { height } => {
                if let (Some(height), false) = (height, frame.fullscreen) {
                    frame.height = clamp_height(height);
                    fit_inline(s, id);
                }
            }
            Effect::TornDown => {}
        }
    }
}

/// clampMcpAppHeight.
fn clamp_height(height: f64) -> f32 {
    if !height.is_finite() {
        return DEFAULT_HEIGHT;
    }
    (height.round() as f32).clamp(80., MAX_HEIGHT)
}

/// Saves the files in order, reading a linked one first; a refusal answers `{isError: true}`.
fn download(s: &mut App, id: &str, live: u64, request: Value, mut files: VecDeque<Download>) {
    while let Some(file) = files.pop_front() {
        match file {
            Download::Embedded { name, bytes } => {
                if let Err(refusal) = save(&name, Some(bytes)) {
                    return settle(s, id, live, &request, refusal);
                }
            }
            Download::Link { name, uri } => {
                let Some(frame) = live_frame(s, id, live) else { return };
                let params = target(frame, json!({ "uri": uri }));
                return rpc(s, id, live, "mcpApps.readResource", params, Call::Download { id: request, name, rest: files });
            }
        }
    }
    settle(s, id, live, &request, Ok(json!({})));
}

/// A refusal is the app's `{isError: true}`; a failure to write is the request's.
fn save(name: &str, bytes: Option<Vec<u8>>) -> Result<(), Result<Value, String>> {
    let refused = Ok(json!({"isError": true}));
    let Some(bytes) = bytes else { return Err(refused) };
    if bytes.len() > MAX_DOWNLOAD_BYTES {
        return Err(refused);
    }
    super::browser::save_bytes(name, &bytes).map(|_| ()).map_err(Err)
}

/// A reply to one of the app's requests.
fn answer(s: &mut App, id: &str, call: Call, reply: Result<Value, Value>) {
    let Some(live) = frame_mut(s, id).and_then(|f| f.live_id()) else { return };
    let failed = |e: &Value| Err(failure_message(e));
    match call {
        Call::Info { id: request, name, arguments } => {
            let info = match reply {
                Ok(info) => info,
                Err(e) => return settle(s, id, live, &request, failed(&e)),
            };
            if info["callable"] != true {
                return settle(s, id, live, &request, Err("This app cannot call that tool.".to_owned()));
            }
            let Some(frame) = live_frame(s, id, live) else { return };
            let params = target(frame, json!({"name": name, "arguments": arguments}));
            if info["readOnly"] == true {
                return rpc(s, id, live, "mcpApps.callTool", params, Call::Tool(request));
            }
            let title = info["title"].as_str().unwrap_or(&name);
            let pretty = serde_json::to_string_pretty(&arguments).unwrap_or_default();
            let message = format!("Allow {} to run {title}?\n{pretty}", frame.app.server);
            let (frame_id, reply_to) = (id.to_owned(), request.clone());
            let run_tool = move |s: &mut App| rpc(s, &frame_id, live, "mcpApps.callTool", params.clone(), Call::Tool(reply_to.clone()));
            ask(s, id, live, message, request, run_tool, Err("Declined by the user.".to_owned()));
        }
        Call::Tool(request) => settle(s, id, live, &request, reply.map(|v| normalize_result(&v)).map_err(|e| failure_message(&e))),
        Call::Resource(request) => settle(s, id, live, &request, reply.map_err(|e| failure_message(&e))),
        Call::Context(request) | Call::Message(request) => settle(s, id, live, &request, reply.map(|_| json!({})).map_err(|e| failure_message(&e))),
        Call::Download { id: request, name, rest } => {
            let content = match reply {
                Ok(v) => v["contents"][0].clone(),
                Err(e) => return settle(s, id, live, &request, failed(&e)),
            };
            if let Err(refusal) = save(&name, resource_bytes(&content)) {
                return settle(s, id, live, &request, refusal);
            }
            download(s, id, live, request, rest);
        }
    }
}

fn fit_fullscreen(s: &mut App, id: &str) {
    let Some(frame) = frame_mut(s, id).filter(|f| !f.still) else { return };
    let (Some(live), true) = (frame.live_id(), frame.fullscreen && frame.full[0] >= 1. && frame.full[1] >= 1.) else { return };
    let (w, h) = (frame.full[0] as u32, frame.full[1] as u32);
    cmd(s, Cmd::LiveResize(live, w, h));
}

fn exit_fullscreen(s: &mut App, id: &str) {
    let Some(frame) = frame_mut(s, id).filter(|f| f.fullscreen) else { return };
    frame.fullscreen = false;
    fit_inline(s, id);
    update_context(s, id);
}

/// "Show app": the closed app loads again.
fn show(s: &mut App, id: &str) {
    if let Some(frame) = frame_mut(s, id) {
        frame.closed = false;
        frame.navigated = false;
    }
    sync(s, id);
}

fn activate(s: &mut App, id: &str) {
    if let Some(frame) = frame_mut(s, id) {
        frame.activated = Some(Instant::now());
    }
}

fn surface(frame: &Frame, key: String) -> Option<El> {
    let live = frame.live.as_ref()?;
    let (down, escape) = (frame.id.clone(), frame.id.clone());
    Some(super::html_frame::surface(live, key, move |s: &mut App| activate(s, &down), move |s: &mut App| exit_fullscreen(s, &escape)))
}

/// McpAppTimelineRow, or none when the item shows no app.
pub fn row_view(app: &App, item: &Value, t: &T) -> Option<El> {
    let reference = reference(item)?;
    let conversation = match app.frame_route() {
        Route::Thread(thread) => thread.clone(),
        _ => String::new(),
    };
    let thread = Some(s(item, "sourceThreadId")).filter(|v| !v.is_empty()).map_or_else(|| conversation.clone(), str::to_owned);
    let revision = if matches!(s(item, "status"), "idle" | "pending" | "running" | "waiting") { "live".to_owned() } else { s(item, "updatedAt").to_owned() };
    let seed = Seed {
        id: s(item, "id").to_owned(),
        item: Some(s(item, "sourceItemId")).filter(|v| !v.is_empty()).unwrap_or(s(item, "id")).to_owned(),
        app: reference,
        thread,
        conversation,
        revision,
    };
    Some(view(app, seed, t))
}

fn view(app: &App, seed: Seed, t: &T) -> El {
    let frame = app.ui.panel.mcp_apps.frames.iter().find(|f| f.id == seed.id);
    let server = seed.app.server.clone();
    let wrap = |child: El| col().w_full().min_w(0.0).px(4.0).child(child);
    if frame.is_some_and(|f| f.closed) {
        let id = seed.id.clone();
        return wrap(
            row()
                .key(format!("mcp-app-{}", seed.id))
                .w_full()
                .gap(8.0)
                .px(12.0)
                .py(8.0)
                .center_y()
                .radius(8.0)
                .border(1.0, t.c.border)
                .child(text(format!("The {server} app was closed")).font(theme::TEXT_XS).color(t.c.muted_foreground).grow(1.0).shrink().min_w(0.0))
                .child(kit::Button::new("Show app").variant("ghost").size("xs").on_press(move |s: &mut App| show(s, &id)).build(t)),
        );
    }
    let fullscreen = frame.is_some_and(|f| f.fullscreen);
    let styles = Value::Object(style_variables(&super::html_frame::theme_variables(t)));
    let dark = t.dark;
    let (key, border) = (format!("mcp-app-{}", seed.id), seed.app.prefers_border && !fullscreen);
    let mut body = col()
        .key(key)
        .w_full()
        .h(frame.map_or(DEFAULT_HEIGHT, |f| f.height))
        .label(format!("{server} app"))
        .on_layout_tx(move |s: &mut App, r, tx| {
            if !layout(s, &seed, &styles, dark, r) {
                tx.live_only();
            }
        });
    if border {
        body = body.radius(8.0).border(1.0, t.c.border).clip_rounded(8.0);
    }
    let note = |body: El, message: String| body.center().child(text(message).font(theme::TEXT_XS).color(t.c.muted_foreground));
    let body = match frame {
        Some(f) if f.navigated => note(body, format!("The {server} app left its page and was stopped")),
        Some(f) if f.failed => note(body, format!("Unable to load the {server} app")),
        // Full screen keeps the row's height so the timeline holds still.
        Some(f) if !f.fullscreen => body.children(surface(f, format!("mcp-app-live-{}", f.id))),
        _ => body,
    };
    wrap(body)
}

/// The full-screen app over the window, under a header with its server and "Exit full screen".
pub fn overlay(app: &App, t: &T, out: &mut Vec<El>) {
    let Some(frame) = app.ui.panel.mcp_apps.frames.iter().find(|f| f.fullscreen) else { return };
    let Some(page) = surface(frame, format!("mcp-app-live-{}", frame.id)) else { return };
    let (exit, fit) = (frame.id.clone(), frame.id.clone());
    let header = row()
        .h(40.0)
        .w_full()
        .px(12.0)
        .gap(8.0)
        .center_y()
        .child(text(frame.app.server.clone()).font(theme::TEXT_SM).color(t.c.foreground).single_line().truncate().grow(1.0).shrink().min_w(0.0))
        .child(kit::Button::new("Exit full screen").icon(&icons::MINIMIZE_2).variant("ghost").size("icon-sm").on_press(move |s: &mut App| exit_fullscreen(s, &exit)).build(t).help("Exit full screen"));
    let area = el().w_full().grow(1.0).min_h(0.0).child(page).on_layout_tx(move |s: &mut App, [_, _, w, h], tx| {
        let Some(frame) = frame_mut(s, &fit) else { return };
        if (frame.full[0] - w.round()).abs() < 1. && (frame.full[1] - h.round()).abs() < 1. {
            tx.live_only();
            return;
        }
        frame.full = [w.round(), h.round()];
        fit_fullscreen(s, &fit);
        update_context(s, &fit);
    });
    out.push(col().key("mcp-app-fullscreen").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(t.c.background).child(header).child(el().h(1.0).w_full().fill(t.c.border)).child(area));
}

/// Scenes: the item's app already running, its page drawn here as a weather card on the dark
/// theme since the render host has no Chrome.
pub fn scene_seed(app: &mut App, item: &Value, height: f32) {
    let Some(reference) = reference(item) else { return };
    let (w, h) = (1456u32, (height * 2.) as u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for (index, pixel) in rgba.chunks_exact_mut(4).enumerate() {
        let (x, y) = (index as u32 % w, index as u32 / w);
        let card = x >= 48 && x < w - 48 && y >= 48 && y < h - 48;
        let sun = (x as i64 - 260).pow(2) + (y as i64 - 260).pow(2) < 120 * 120;
        let bar = (0..5).find(|b| x >= 520 + b * 180 && x < 520 + b * 180 + 120).filter(|b| y >= h - 120 - [220, 300, 180, 260, 340][*b as usize] && y < h - 120);
        let color: [u8; 3] = match (sun, bar, card) {
            (true, _, true) => [0xfb, 0xbf, 0x24],
            (_, Some(_), true) => [0x60, 0xa5, 0xfa],
            (_, _, true) => [0x17, 0x17, 0x1a],
            _ => [0, 0, 0],
        };
        pixel.copy_from_slice(&[color[0], color[1], color[2], 0xff]);
    }
    let live = Live::new(String::new(), 728, height as u32, true);
    live.pixels.set(w, h, rgba);
    let frame = Frame { id: s(item, "id").to_owned(), app: reference, height, dark: true, live: Some(live), still: true, visible: true, ..Default::default() };
    app.ui.panel.mcp_apps.frames.push(frame);
}

/// A minimal MCP app for the live scene: it initializes with the host, themes itself from the
/// host context, shows the call's input and result, and reports its height.
const SCENE_APP: &str = r#"<!doctype html><html><head><meta charset="utf-8"><style>
html { color-scheme: dark; } body { margin: 0; font: 14px/1.45 var(--font-sans, system-ui, sans-serif); color: var(--color-text-primary, #eee); background: var(--color-background-secondary, #111); }
.card { padding: 20px 24px; } h1 { font-size: 18px; margin: 0 0 4px; } .muted { color: var(--color-text-secondary, #999); font-size: 12px; }
.days { display: flex; gap: 12px; align-items: flex-end; height: 120px; margin-top: 16px; } .day { flex: 1; text-align: center; font-size: 12px; }
.bar { background: #60a5fa; border-radius: 6px 6px 0 0; margin-bottom: 4px; }
</style></head><body><div class="card" id="card"><h1 id="title">Waiting for the host…</h1><div class="muted" id="host"></div><div class="days" id="days"></div></div>
<script>
let next = 1; const waiting = {};
const send = (m) => window.parent.postMessage(Object.assign({ jsonrpc: "2.0" }, m), "*");
const request = (method, params) => new Promise((done) => { const id = next++; waiting[id] = done; send({ id, method, params }); });
const theme = (context) => { for (const [k, v] of Object.entries(context?.styles?.variables ?? {})) document.documentElement.style.setProperty(k, v); };
addEventListener("message", (event) => {
  const m = event.data;
  if (event.source !== window.parent || !m || m.jsonrpc !== "2.0") return;
  if (m.id !== undefined && waiting[m.id]) { waiting[m.id](m.result ?? m.error); delete waiting[m.id]; return; }
  if (m.method === "ui/notifications/tool-input") document.getElementById("title").textContent = "Forecast for " + m.params.arguments.city;
  if (m.method === "ui/notifications/host-context-changed") theme(m.params);
  if (m.method === "ui/notifications/tool-result") {
    const days = m.params.structuredContent?.days ?? [];
    document.getElementById("days").innerHTML = days.map((d) => `<div class="day"><div class="bar" style="height:${d.high * 8}px"></div>${d.day} ${d.high}°</div>`).join("");
    send({ method: "ui/notifications/size-changed", params: { height: document.getElementById("card").offsetHeight } });
  }
});
request("ui/initialize", { protocolVersion: "2026-01-26", appInfo: { name: "forecast", version: "1.0.0" }, appCapabilities: { availableDisplayModes: ["inline"] } }).then((r) => {
  theme(r.hostContext);
  document.getElementById("host").textContent = `${r.hostInfo.name} · protocol ${r.protocolVersion} · ${r.hostContext.displayMode} · ${r.hostContext.theme} · ${r.hostContext.containerDimensions.width}px`;
  send({ method: "ui/notifications/initialized", params: {} });
});
</script></body></html>"#;

/// Scenes with Chrome (`S5_CHROME`): the row's app run for real through the bridge and host,
/// pumping the frame's own poll here since a headless run's clock does not wait on Chrome. It
/// ends once the app sized itself after the call's result, then keeps the last painted frame.
pub fn scene_live(app: &mut App, id: &str) {
    super::html_frame::scene_waker(app);
    let conversation = match app.frame_route() {
        Route::Thread(thread) => thread.clone(),
        _ => String::new(),
    };
    let styles = Value::Object(style_variables(&super::html_frame::theme_variables(&crate::theme::theme(true))));
    let Some(frame) = frame_mut(app, id) else { return };
    let call = ToolCall {
        arguments: json!({"city": "Oslo"}),
        result: Some(json!({"content": [{"type": "text", "text": "Five days"}], "structuredContent": {"days": [
            {"day": "Mon", "high": 4}, {"day": "Tue", "high": 6}, {"day": "Wed", "high": 3}, {"day": "Thu", "high": 8}, {"day": "Fri", "high": 11}]}, "_meta": null})),
    };
    // From the default height, so the app's own size report is what changes it.
    *frame = Frame { live: None, still: false, width: 728., height: DEFAULT_HEIGHT, visible: true, dark: true, styles, conversation, tool_call: Some(call), ..std::mem::take(frame) };
    start(app, id, super::html_frame::data_url(SCENE_APP));
    let mut tx = Tx::default();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut sized = None;
    while Instant::now() < deadline {
        poll(app, &mut tx);
        let Some(frame) = frame_mut(app, id) else { return };
        if frame.live.is_none() {
            return eprintln!("scene: the app's page went away (failed {}, navigated {})", frame.failed, frame.navigated);
        }
        // Sized after its result: let Chrome paint at the new height, then stop.
        if sized.is_none() && (frame.height - DEFAULT_HEIGHT).abs() >= 1. {
            eprintln!("scene: the app sized itself to {} px after its result", frame.height);
            sized = Some(Instant::now());
        }
        if sized.is_some_and(|at| at.elapsed() > Duration::from_millis(1500)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if sized.is_none() {
        eprintln!("scene: the app never reported its size");
    }
    if let Some(frame) = frame_mut(app, id) {
        frame.still = true;
    }
    super::browser::shutdown();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> Reference {
        read_reference(&json!({"attachmentId": "a1", "server": "weather", "tool": "forecast", "resourceUri": "ui://weather/forecast.html",
            "csp": {"connectDomains": ["https://api.weather.test", "javascript:alert(1)"], "frameDomains": ["nope"]},
            "permissions": {"geolocation": {}, "camera": true}, "prefersBorder": true}))
        .unwrap()
    }

    fn context(mode: &str) -> Value {
        json!({"theme": "dark", "displayMode": mode, "availableDisplayModes": ["inline", "fullscreen"], "containerDimensions": {"width": 700, "maxHeight": 2000}})
    }

    /// Feeds recorded messages, returning everything the host did.
    fn replay(host: &mut Host, messages: &[&str], context: &Value) -> Vec<Effect> {
        messages.iter().flat_map(|m| host.receive(m, context)).collect()
    }

    fn posted(effects: &[Effect]) -> Vec<&Value> {
        effects.iter().filter_map(|e| if let Effect::Post(v) = e { Some(v) } else { None }).collect()
    }

    const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"ui/initialize","params":{"protocolVersion":"2025-06-18","appInfo":{"name":"forecast","version":"1.0.0"},"appCapabilities":{"availableDisplayModes":["inline","fullscreen"]}}}"#;
    const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"ui/notifications/initialized","params":{}}"#;

    #[test]
    fn an_app_shows_only_for_its_own_tool_call() {
        let output = json!({"content": [{"type": "text", "text": "{\"t3McpApp\":{\"attachmentId\":\"a1\",\"server\":\"weather\",\"tool\":\"forecast\",\"resourceUri\":\"ui://weather/forecast.html\"}}"}]});
        let item = |tool: &str| json!({"type": "dynamic_tool", "status": "completed", "toolName": tool, "output": output});
        let found = reference(&item("weather.forecast")).unwrap();
        assert_eq!((found.attachment_id.as_str(), found.server.as_str(), found.prefers_border), ("a1", "weather", false));
        assert_eq!(reference(&item("other.forecast")), None, "another tool cannot show weather's app");
        // The resource must be a ui:// one, and names stay bounded.
        assert_eq!(read_reference(&json!({"attachmentId": "a", "server": "s", "tool": "t", "resourceUri": "https://x"})), None);
        assert_eq!(read_reference(&json!({"attachmentId": "a", "server": " ", "tool": "t", "resourceUri": "ui://x"})), None);
        // Only well-formed origins and the known permissions pass.
        let app = app();
        assert_eq!(app.csp, Some(json!({"connectDomains": ["https://api.weather.test"]})));
        assert_eq!(app.permissions, Some(json!({"geolocation": {}})));
        assert!(domain_allowed("wss://*.example.com:8443") && !domain_allowed("https://*.") && !domain_allowed("https://a.b:123456"));
        assert_eq!(file_name(&Reference { tool: "get weather/now!".into(), ..app.clone() }), "get-weather-now-.html");
        assert_eq!(file_name(&Reference { tool: "***".into(), ..app }), "-.html");
    }

    #[test]
    fn the_handshake_sends_the_call_once_the_app_is_ready() {
        let mut host = Host::new(&app());
        // The call arrives before the app initializes: nothing goes out yet.
        assert_eq!(host.set_tool_call(ToolCall { arguments: json!({"city": "Oslo"}), result: None }), vec![]);
        let effects = replay(&mut host, &[INITIALIZE], &context("inline"));
        let reply = posted(&effects)[0];
        assert_eq!(reply["id"], 1);
        assert_eq!(reply["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(reply["result"]["hostInfo"]["name"], "t3-code");
        assert_eq!(reply["result"]["hostCapabilities"]["sandbox"], json!({"csp": {"connectDomains": ["https://api.weather.test"]}, "permissions": {"geolocation": {}}}));
        assert_eq!(reply["result"]["hostContext"]["displayMode"], "inline");
        // Initialized: the input goes, then the result once the call finishes.
        let effects = replay(&mut host, &[INITIALIZED, INITIALIZED], &context("inline"));
        assert_eq!(posted(&effects).len(), 1, "a repeated initialized does nothing");
        assert_eq!(posted(&effects)[0]["method"], "ui/notifications/tool-input");
        assert_eq!(posted(&effects)[0]["params"], json!({"arguments": {"city": "Oslo"}}));
        let done = ToolCall { arguments: json!({"city": "Oslo"}), result: Some(json!({"content": null, "structuredContent": {"temp": 4}, "isError": false, "_meta": null})) };
        let effects = host.set_tool_call(done.clone());
        assert_eq!(posted(&effects)[0]["method"], "ui/notifications/tool-result");
        assert_eq!(posted(&effects)[0]["params"], json!({"content": [], "structuredContent": {"temp": 4}}), "nulls are dropped for the SDK");
        assert_eq!(host.set_tool_call(done), vec![], "a result goes once");
        // A context change sends only the keys that changed.
        let mut next = context("inline");
        next["theme"] = json!("light");
        let effects = host.update_context(&next);
        assert_eq!(posted(&effects)[0], &json!({"jsonrpc": "2.0", "method": "ui/notifications/host-context-changed", "params": {"theme": "light"}}));
        assert_eq!(host.update_context(&next), vec![]);
    }

    #[test]
    fn requests_route_to_the_frame_or_fail_with_web_errors() {
        let mut host = Host::new(&app());
        let ctx = context("inline");
        let effects = replay(&mut host, &[
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"refresh","arguments":{"days":3}}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"refresh"}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"","arguments":{}}}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"x","arguments":[1]}}"#,
            r#"{"jsonrpc":"2.0","id":"r","method":"resources/read","params":{"uri":"ui://weather/data.json"}}"#,
            r#"{"jsonrpc":"2.0","id":6,"method":"resources/read","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":7,"method":"ui/open-link","params":{"url":"HTTPS://example.com/a"}}"#,
            r#"{"jsonrpc":"2.0","id":8,"method":"ui/open-link","params":{"url":"file:///etc/passwd"}}"#,
            r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","id":10,"method":"sampling/createMessage","params":{}}"#,
            r#"{"jsonrpc":"1.0","id":11,"method":"ping"}"#,
            r#"[1,2]"#,
            r#"{"jsonrpc":"2.0","id":12,"method":"ui/update-model-context","params":{"content":[{"type":"text","text":"Oslo picked"}]}}"#,
            r#"{"jsonrpc":"2.0","id":13,"method":"ui/update-model-context","params":{"structuredContent":[1]}}"#,
        ], &ctx);
        let mut handed = effects.iter().filter(|e| !matches!(e, Effect::Post(_)));
        assert_eq!(handed.next(), Some(&Effect::CallTool { id: json!(2), name: "refresh".into(), arguments: json!({"days": 3}) }));
        assert_eq!(handed.next(), Some(&Effect::CallTool { id: json!(3), name: "refresh".into(), arguments: json!({}) }), "arguments default to an empty object");
        assert_eq!(handed.next(), Some(&Effect::ReadResource { id: json!("r"), uri: "ui://weather/data.json".into() }));
        assert_eq!(handed.next(), Some(&Effect::OpenLink { id: json!(7), url: "HTTPS://example.com/a".into() }));
        assert_eq!(handed.next(), Some(&Effect::UpdateModelContext { id: json!(12), context: json!({"content": [{"type": "text", "text": "Oslo picked"}]}) }));
        assert_eq!(handed.next(), None);
        let errors: Vec<(Value, Value, Value)> = posted(&effects).iter().map(|p| (p["id"].clone(), p["error"]["code"].clone(), p["error"]["message"].clone())).collect();
        assert_eq!(errors, vec![
            (json!(4), json!(-32602), json!("tools/call needs a tool name and object arguments.")),
            (json!(5), json!(-32602), json!("tools/call needs a tool name and object arguments.")),
            (json!(6), json!(-32602), json!("resources/read needs a uri.")),
            (json!(8), json!(-32602), json!("Only http(s) links can be opened.")),
            (json!(9), Value::Null, Value::Null),
            (json!(10), json!(-32601), json!("Method not found: sampling/createMessage")),
            (json!(13), json!(-32602), json!("Model context needs content blocks or an object of structured content.")),
        ]);
        // Answers: a result, a failure's message, and the default for an empty one.
        assert_eq!(posted(&host.settle(&json!(2), Ok(json!({"content": []}))))[0], &json!({"jsonrpc": "2.0", "id": 2, "result": {"content": []}}));
        assert_eq!(posted(&host.settle(&json!(3), Err("Declined by the user.".into())))[0]["error"], json!({"code": -32000, "message": "Declined by the user."}));
        assert_eq!(posted(&host.settle(&json!("r"), Err(" ".into())))[0]["error"]["message"], "Request failed.");
    }

    #[test]
    fn too_many_or_too_large_requests_are_refused() {
        let mut host = Host::new(&app());
        let ping = |id: usize| format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"resources/read","params":{{"uri":"ui://a"}}}}"#);
        for id in 0..MAX_PENDING {
            assert!(matches!(host.receive(&ping(id), &Value::Null)[..], [Effect::ReadResource { .. }]));
        }
        let refused = host.receive(&ping(99), &Value::Null);
        assert_eq!(posted(&refused)[0]["error"], json!({"code": -32000, "message": "Too many requests in flight."}));
        host.settle(&json!(0), Ok(json!({})));
        assert!(matches!(host.receive(&ping(100), &Value::Null)[..], [Effect::ReadResource { .. }]), "a settled request frees a slot");
        let big = format!(r#"{{"jsonrpc":"2.0","id":7,"method":"ping","params":{{"pad":"{}"}}}}"#, "x".repeat(MAX_MESSAGE_BYTES));
        assert_eq!(posted(&host.receive(&big, &Value::Null))[0]["error"], json!({"code": -32600, "message": "Message too large."}));
        let big_note = format!(r#"{{"jsonrpc":"2.0","method":"ui/notifications/size-changed","params":{{"pad":"{}"}}}}"#, "x".repeat(MAX_MESSAGE_BYTES));
        assert_eq!(host.receive(&big_note, &Value::Null), vec![], "a large notification is dropped unanswered");
    }

    #[test]
    fn messages_must_be_user_text() {
        let mut host = Host::new(&app());
        let effects = replay(&mut host, &[
            r#"{"jsonrpc":"2.0","id":1,"method":"ui/message","params":{"role":"user","content":[{"type":"text","text":" Book it "},{"type":"text","text":"for two "}]}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"ui/message","params":{"role":"user","content":{"type":"text","text":"One block"}}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"ui/message","params":{"role":"assistant","content":[{"type":"text","text":"x"}]}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"ui/message","params":{"role":"user","content":[{"type":"image","data":"…"}]}}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"ui/message","params":{"role":"user","content":[]}}"#,
            r#"{"jsonrpc":"2.0","id":6,"method":"ui/message","params":{"role":"user","content":[{"type":"text","text":"  "}]}}"#,
        ], &Value::Null);
        let sent: Vec<&Effect> = effects.iter().filter(|e| matches!(e, Effect::SendMessage { .. })).collect();
        assert_eq!(sent, [&Effect::SendMessage { id: json!(1), text: "Book it \nfor two".into() }, &Effect::SendMessage { id: json!(2), text: "One block".into() }]);
        let errors: Vec<&Value> = posted(&effects).iter().map(|p| &p["error"]["message"]).collect();
        let only_text = json!("Only text messages from the user role are supported.");
        assert_eq!(errors, [&only_text, &only_text, &only_text, &json!("ui/message needs non-empty text.")]);
    }

    #[test]
    fn display_modes_change_only_to_ones_both_sides_offer() {
        let mut host = Host::new(&app());
        replay(&mut host, &[r#"{"jsonrpc":"2.0","id":1,"method":"ui/initialize","params":{"appCapabilities":{"availableDisplayModes":["inline","fullscreen","pip"]}}}"#], &context("inline"));
        let ask = |mode: &str, id: u32| format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"ui/request-display-mode","params":{{"mode":"{mode}"}}}}"#);
        assert_eq!(host.receive(&ask("fullscreen", 2), &context("inline")), vec![Effect::DisplayMode { id: json!(2), mode: "fullscreen".into() }]);
        // The host offers no pip, and asking for the current mode answers it at once.
        assert_eq!(posted(&host.receive(&ask("pip", 3), &context("inline")))[0]["result"], json!({"mode": "inline"}));
        assert_eq!(posted(&host.receive(&ask("inline", 4), &context("inline")))[0]["result"], json!({"mode": "inline"}));
        // An app that declared only inline cannot go full screen.
        let mut inline_only = Host::new(&app());
        replay(&mut inline_only, &[r#"{"jsonrpc":"2.0","id":1,"method":"ui/initialize","params":{"appCapabilities":{"availableDisplayModes":["inline"]}}}"#], &context("inline"));
        assert_eq!(posted(&inline_only.receive(&ask("fullscreen", 5), &context("inline")))[0]["result"], json!({"mode": "inline"}));
    }

    #[test]
    fn downloads_name_and_decode_their_files() {
        let mut host = Host::new(&app());
        let effects = replay(&mut host, &[
            r#"{"jsonrpc":"2.0","id":1,"method":"ui/download-file","params":{"contents":[
                {"type":"resource","resource":{"uri":"file:///tmp/report.csv","text":"a,b"}},
                {"type":"resource","resource":{"uri":"ui://x","name":"my:file?.bin","blob":"AAEC"}},
                {"type":"resource_link","uri":"ui://weather/exports/","name":""}]}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"ui/download-file","params":{"contents":[{"type":"resource","resource":{"uri":"ui://x"}}]}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"ui/download-file","params":{"contents":[]}}"#,
        ], &Value::Null);
        assert_eq!(effects[0], Effect::Download { id: json!(1), files: vec![
            Download::Embedded { name: "report.csv".into(), bytes: b"a,b".to_vec() },
            Download::Embedded { name: "my-file-.bin".into(), bytes: vec![0, 1, 2] },
            Download::Link { name: "exports".into(), uri: "ui://weather/exports/".into() },
        ] });
        let message = json!("ui/download-file needs embedded resources or resource links.");
        assert_eq!(posted(&effects).iter().map(|p| &p["error"]["message"]).collect::<Vec<_>>(), [&message, &message]);
        assert_eq!(download_name(&Value::Null, &json!("")), "download");
        // As web splits it, a query or fragment is the last segment.
        assert_eq!(download_name(&json!("ui://a/b#frag"), &Value::Null), "frag");
        assert_eq!(download_name(&Value::Null, &json!("x".repeat(300))).len(), 200);
    }

    #[test]
    fn notifications_size_the_frame_and_teardown_waits_for_the_app() {
        let mut host = Host::new(&app());
        let effects = replay(&mut host, &[
            r#"{"jsonrpc":"2.0","method":"ui/notifications/size-changed","params":{"height":512.4}}"#,
            r#"{"jsonrpc":"2.0","method":"ui/notifications/size-changed","params":{"height":"tall"}}"#,
            r#"{"jsonrpc":"2.0","method":"ui/notifications/request-teardown"}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info","data":"hi"}}"#,
        ], &Value::Null);
        assert_eq!(effects, vec![Effect::Size { height: Some(512.4) }, Effect::RequestTeardown]);
        assert_eq!((clamp_height(512.4), clamp_height(5.), clamp_height(9000.)), (512., 80., 2000.));
        // Before it initializes, an app goes at once.
        assert_eq!(Host::new(&app()).teardown(Instant::now()), vec![Effect::TornDown]);
        // After, it is asked and closes on its answer, and then hears nothing more.
        replay(&mut host, &[INITIALIZE, INITIALIZED], &context("inline"));
        let start = Instant::now();
        let asked = host.teardown(start);
        assert_eq!(posted(&asked)[0], &json!({"jsonrpc": "2.0", "id": "t3-host-0", "method": "ui/resource-teardown", "params": {}}));
        assert_eq!(host.receive(r#"{"jsonrpc":"2.0","id":"t3-host-9","result":{}}"#, &Value::Null), vec![], "another id is not the answer");
        assert_eq!(host.tick(start + Duration::from_millis(1999)), vec![]);
        assert_eq!(host.receive(r#"{"jsonrpc":"2.0","id":"t3-host-0","result":{}}"#, &Value::Null), vec![Effect::TornDown]);
        assert_eq!(host.receive(r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#, &Value::Null), vec![]);
        // An app that never answers is let go after two seconds, which the frame wakes for.
        let mut silent = Host::new(&app());
        replay(&mut silent, &[INITIALIZE, INITIALIZED], &context("inline"));
        assert_eq!(silent.teardown_due(), None);
        silent.teardown(start);
        assert_eq!(silent.teardown_due(), Some(start + TEARDOWN_TIMEOUT));
        assert_eq!(silent.tick(start + TEARDOWN_TIMEOUT), vec![Effect::TornDown]);
        assert_eq!(silent.teardown_due(), None);
    }

    #[test]
    fn a_pending_request_keeps_apps_inline() {
        assert!(awaiting_user(&json!({"runtimeRequests": [{"kind": "approval", "status": "resolved"}, {"kind": "user_input", "status": "pending"}]})));
        assert!(!awaiting_user(&json!({"runtimeRequests": [{"kind": "approval", "status": "resolved"}]})));
        assert!(!awaiting_user(&json!({})));
    }

    #[test]
    fn rows_leaving_the_list_let_their_apps_go_and_come_back() {
        let mut s = App::new();
        let seed = Seed { id: "a".into(), app: app(), thread: "t".into(), item: "a".into(), conversation: "t".into(), revision: "r".into() };
        s.ui.panel.mcp_apps.frames.push(Frame { visible: true, ..Frame::new(&seed) });
        let apps = [(3, "a".to_owned()), (9, "missing".to_owned())];
        // In view: nothing changes.
        assert!(!in_view(&mut s, &apps, 0..5));
        // Scrolled out of the list: unlisted and no longer on screen.
        assert!(in_view(&mut s, &apps, 5..12));
        let frame = &s.ui.panel.mcp_apps.frames[0];
        assert!(frame.unlisted && !frame.visible);
        assert!(!in_view(&mut s, &apps, 6..12), "reported once");
        // Back in view: listed again, for its next layout to load.
        assert!(in_view(&mut s, &apps, 2..8));
        assert!(!s.ui.panel.mcp_apps.frames[0].unlisted);
    }

    #[test]
    fn the_theme_maps_to_the_apps_variables() {
        let theme = super::super::html_frame::theme_variables(&crate::theme::theme(true));
        let variables = style_variables(&theme);
        assert_eq!(variables["--color-background-primary"], theme["--background"]);
        assert_eq!(variables["--color-text-danger"], theme["--destructive-foreground"]);
        assert_eq!(variables["--border-radius-md"], "0.625rem");
        assert_eq!(variables.len(), 22);
        assert_eq!(failure_message(&json!([{"_tag": "Fail", "error": {"_tag": "McpAppRequestError", "reason": "session-stopped"}}])), "The app's thread is not running. Send a message in the thread that created it to use the app again.");
        assert_eq!(failure_message(&json!({"message": "Boom"})), "Boom");
    }
}
