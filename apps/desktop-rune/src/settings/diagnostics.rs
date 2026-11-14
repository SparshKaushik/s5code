//! Web `DiagnosticsSettings`: Live Processes (with SIGINT / SIGKILL), Resource History, Trace
//! Diagnostics and its span tables. Results are one-shot RPC queries keyed by the focused
//! environment, so switching environments starts fresh as web keys the body by environment.

use super::controls as c;
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{Color, Cx, El, col, el, row, text};
use serde_json::{Value, json};
use std::collections::HashSet;

const TRACE: &str = "server.getTraceDiagnostics";
const PROCESSES: &str = "server.getProcessDiagnostics";
const HISTORY: &str = "server.getProcessResourceHistory";
const SIGNAL: &str = "server.signalProcess";
const DIAGNOSTICS_READ: &str = "diagnostics:read";
const ENVIRONMENT_MAINTAIN: &str = "environment:maintain";
const ORCHESTRATION_OPERATE: &str = "orchestration:operate";
/// `RESOURCE_HISTORY_WINDOWS`: label, window and bucket in ms.
const WINDOWS: [(&str, i64, i64); 4] = [
    ("5m", 5 * 60_000, 30_000),
    ("15m", 15 * 60_000, 60_000),
    ("30m", 30 * 60_000, 2 * 60_000),
    ("1h", 60 * 60_000, 5 * 60_000),
];
/// Web queries go stale after 30 s and refetch when the page mounts again.
const STALE_MS: i64 = 30_000;
/// A tick after this long without one means the page was left and opened again.
const REENTRY_GAP_MS: i64 = 2_000;

/// One query's last result, its failure, and the request in flight.
struct Query<D> {
    data: Option<D>,
    error: Option<String>,
    pending: Option<u64>,
    /// When the last reply settled; 0 before any.
    fetched_ms: i64,
}

impl<D> Default for Query<D> {
    fn default() -> Self {
        Self { data: None, error: None, pending: None, fetched_ms: 0 }
    }
}

impl<D> Query<D> {
    fn due(&self, now: i64, reentered: bool) -> bool {
        self.pending.is_none() && (self.fetched_ms == 0 || (reentered && now - self.fetched_ms > STALE_MS))
    }

    /// Web `isPending && data === null`, including the frame before the first request.
    fn initial_loading(&self) -> bool {
        self.data.is_none() && (self.pending.is_some() || self.fetched_ms == 0)
    }
}

/// A signal from the moment its button is pressed until the server answers; a SIGKILL waits
/// for its confirmation with no request yet.
struct Signal {
    pid: i64,
    start_time_ms: i64,
    signal: &'static str,
    request: Option<u64>,
}

#[derive(Default)]
pub struct Ui {
    /// The environment these results belong to.
    env: Option<String>,
    trace: Query<Trace>,
    processes: Query<Processes>,
    history: Query<History>,
    /// Index into `WINDOWS`; unset is web's 15 minutes.
    window: Option<usize>,
    collapsed: HashSet<i64>,
    signal: Option<Signal>,
    open_logs: Option<u64>,
    open_logs_error: Option<String>,
    /// Expanded `ExpandableText` rows.
    expanded: HashSet<String>,
    /// The trace ID copy button showing "Copied", until when.
    copied: Option<(String, i64)>,
    /// Replies to drop: resource history requests for a window no longer selected.
    discard: Vec<u64>,
    last_tick: i64,
}

impl Ui {
    fn window_index(&self) -> usize {
        self.window.unwrap_or(1).min(WINDOWS.len() - 1)
    }
}

// --- Results (contracts `server.ts`) -------------------------------------------------------

struct Trace {
    read_at: Option<i64>,
    record_count: i64,
    parse_error_count: i64,
    failure_count: i64,
    slow_span_threshold_ms: f64,
    slow_span_count: i64,
    top_spans: Vec<SpanSummary>,
    slowest: Vec<SpanOccurrence>,
    common_failures: Vec<FailureSummary>,
    latest_failures: Vec<RecentFailure>,
    logs: Vec<LogEvent>,
    partial_failure: bool,
    error: Option<String>,
}

struct SpanSummary {
    name: String,
    count: i64,
    failure_count: i64,
    average_ms: f64,
    max_ms: f64,
}

struct SpanOccurrence {
    name: String,
    duration_ms: f64,
    ended_at: Option<i64>,
    trace_id: String,
    span_id: String,
}

struct FailureSummary {
    name: String,
    cause: String,
    count: i64,
    last_seen_at: Option<i64>,
}

struct RecentFailure {
    name: String,
    cause: String,
    duration_ms: f64,
    ended_at: Option<i64>,
    trace_id: String,
    span_id: String,
}

struct LogEvent {
    span_name: String,
    level: String,
    message: String,
    seen_at: Option<i64>,
    trace_id: String,
    span_id: String,
}

struct Processes {
    server_pid: i64,
    read_at: Option<i64>,
    process_count: i64,
    total_rss_bytes: f64,
    total_cpu_percent: f64,
    processes: Vec<Process>,
    error: Option<String>,
}

struct Process {
    pid: i64,
    start_time_ms: i64,
    cpu_percent: f64,
    rss_bytes: f64,
    command: String,
    depth: i64,
    child_pids: Vec<i64>,
}

struct History {
    read_at: Option<i64>,
    sample_interval_ms: f64,
    retained_sample_count: i64,
    total_cpu_seconds: f64,
    buckets: Vec<Bucket>,
    top_processes: Vec<HistoryProcess>,
    error: Option<String>,
}

struct Bucket {
    avg_cpu_percent: f64,
    max_cpu_percent: f64,
}

struct HistoryProcess {
    process_key: String,
    pid: i64,
    command: String,
    depth: i64,
    is_server_root: bool,
    current_cpu_percent: f64,
    avg_cpu_percent: f64,
    max_cpu_percent: f64,
    cpu_seconds: f64,
    max_rss_bytes: f64,
}

struct SignalResult {
    signaled: bool,
    message: Option<String>,
}

/// `Schema.Option` on the wire: absent, `null`, `{_tag: "None"}`, `{_tag: "Some", value}`, or
/// the bare value.
pub(super) fn option(v: &Value) -> Option<&Value> {
    match v {
        Value::Null => None,
        Value::Object(map) => match map.get("_tag").and_then(Value::as_str) {
            Some("None") => None,
            Some("Some") => map.get("value").filter(|v| !v.is_null()),
            _ => Some(v),
        },
        other => Some(other),
    }
}

pub(super) fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

pub(super) fn int(v: &Value) -> i64 {
    v.as_f64().unwrap_or(0.0) as i64
}

pub(super) fn string(v: &Value) -> String {
    v.as_str().unwrap_or("").to_owned()
}

pub(super) fn at(v: &Value) -> Option<i64> {
    v.as_str().and_then(crate::data::iso_ms)
}

pub(super) fn list(v: &Value) -> impl Iterator<Item = &Value> {
    v.as_array().into_iter().flatten()
}

fn option_message(v: &Value) -> Option<String> {
    option(v).and_then(|e| e["message"].as_str()).map(str::to_owned)
}

fn decode_trace(v: &Value) -> Trace {
    Trace {
        read_at: at(&v["readAt"]),
        record_count: int(&v["recordCount"]),
        parse_error_count: int(&v["parseErrorCount"]),
        failure_count: int(&v["failureCount"]),
        slow_span_threshold_ms: num(&v["slowSpanThresholdMs"]),
        slow_span_count: int(&v["slowSpanCount"]),
        top_spans: list(&v["topSpansByCount"])
            .map(|s| SpanSummary {
                name: string(&s["name"]),
                count: int(&s["count"]),
                failure_count: int(&s["failureCount"]),
                average_ms: num(&s["averageDurationMs"]),
                max_ms: num(&s["maxDurationMs"]),
            })
            .collect(),
        slowest: list(&v["slowestSpans"])
            .map(|s| SpanOccurrence {
                name: string(&s["name"]),
                duration_ms: num(&s["durationMs"]),
                ended_at: at(&s["endedAt"]),
                trace_id: string(&s["traceId"]),
                span_id: string(&s["spanId"]),
            })
            .collect(),
        common_failures: list(&v["commonFailures"])
            .map(|f| FailureSummary {
                name: string(&f["name"]),
                cause: string(&f["cause"]),
                count: int(&f["count"]),
                last_seen_at: at(&f["lastSeenAt"]),
            })
            .collect(),
        latest_failures: list(&v["latestFailures"])
            .map(|f| RecentFailure {
                name: string(&f["name"]),
                cause: string(&f["cause"]),
                duration_ms: num(&f["durationMs"]),
                ended_at: at(&f["endedAt"]),
                trace_id: string(&f["traceId"]),
                span_id: string(&f["spanId"]),
            })
            .collect(),
        logs: list(&v["latestWarningAndErrorLogs"])
            .map(|e| LogEvent {
                span_name: string(&e["spanName"]),
                level: string(&e["level"]),
                message: string(&e["message"]),
                seen_at: at(&e["seenAt"]),
                trace_id: string(&e["traceId"]),
                span_id: string(&e["spanId"]),
            })
            .collect(),
        partial_failure: option(&v["partialFailure"]).and_then(Value::as_bool).unwrap_or(false),
        error: option_message(&v["error"]),
    }
}

fn decode_processes(v: &Value) -> Processes {
    Processes {
        server_pid: int(&v["serverPid"]),
        read_at: at(&v["readAt"]),
        process_count: int(&v["processCount"]),
        total_rss_bytes: num(&v["totalRssBytes"]),
        total_cpu_percent: num(&v["totalCpuPercent"]),
        processes: list(&v["processes"])
            .map(|p| Process {
                pid: int(&p["pid"]),
                start_time_ms: int(&p["startTimeMs"]),
                cpu_percent: num(&p["cpuPercent"]),
                rss_bytes: num(&p["rssBytes"]),
                command: string(&p["command"]),
                depth: int(&p["depth"]),
                child_pids: list(&p["childPids"]).map(int).collect(),
            })
            .collect(),
        error: option_message(&v["error"]),
    }
}

fn decode_history(v: &Value) -> History {
    History {
        read_at: at(&v["readAt"]),
        sample_interval_ms: num(&v["sampleIntervalMs"]),
        retained_sample_count: int(&v["retainedSampleCount"]),
        total_cpu_seconds: num(&v["totalCpuSecondsApprox"]),
        buckets: list(&v["buckets"])
            .map(|b| Bucket { avg_cpu_percent: num(&b["avgCpuPercent"]), max_cpu_percent: num(&b["maxCpuPercent"]) })
            .collect(),
        top_processes: list(&v["topProcesses"])
            .map(|p| HistoryProcess {
                process_key: string(&p["processKey"]),
                pid: int(&p["pid"]),
                command: string(&p["command"]),
                depth: int(&p["depth"]),
                is_server_root: p["isServerRoot"] == true,
                current_cpu_percent: num(&p["currentCpuPercent"]),
                avg_cpu_percent: num(&p["avgCpuPercent"]),
                max_cpu_percent: num(&p["maxCpuPercent"]),
                cpu_seconds: num(&p["cpuSecondsApprox"]),
                max_rss_bytes: num(&p["maxRssBytes"]),
            })
            .collect(),
        error: option_message(&v["error"]),
    }
}

fn decode_signal(v: &Value) -> SignalResult {
    SignalResult {
        signaled: v["signaled"] == true,
        message: option(&v["message"]).and_then(Value::as_str).map(str::to_owned),
    }
}

// --- Formatting -----------------------------------------------------------------------------

/// `Math.round`.
pub(super) fn js_round(v: f64) -> i64 {
    (v + 0.5).floor() as i64
}

/// `.replace(/\.0+$/, "")`.
pub(super) fn trim_zeros(s: String) -> String {
    let whole = match s.split_once('.') {
        Some((whole, fraction)) if !fraction.is_empty() && fraction.chars().all(|c| c == '0') => Some(whole.to_owned()),
        _ => None,
    };
    whole.unwrap_or(s)
}

/// `Intl.NumberFormat` in en-US.
pub(super) fn format_count(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

fn format_duration(ms: f64) -> String {
    if ms < 1_000.0 {
        return format!("{} ms", js_round(ms));
    }
    let digits: usize = if ms >= 10_000.0 { 1 } else { 2 };
    format!("{} s", trim_zeros(format!("{:.*}", digits, ms / 1_000.0)))
}

pub(super) fn format_cpu_time(seconds: f64) -> String {
    let fixed = |v: f64| trim_zeros(format!("{:.*}", if v >= 10.0 { 1 } else { 2 }, v));
    if seconds < 60.0 {
        return format!("{}s", fixed(seconds));
    }
    let minutes = seconds / 60.0;
    if minutes < 60.0 {
        return format!("{}m", fixed(minutes));
    }
    format!("{}h", trim_zeros(format!("{:.2}", minutes / 60.0)))
}

/// `formatBytes`.
pub(super) fn format_bytes(value: f64) -> String {
    if value.round() < 1024.0 {
        return format!("{} B", value.round() as i64);
    }
    let scaled = |v: f64| format!("{:.*}", if v >= 100.0 { 0 } else if v >= 10.0 { 1 } else { 2 }, v);
    let units = ["KB", "MB", "GB", "TB"];
    let (mut next, mut unit) = (value / 1024.0, 0);
    while scaled(next).parse::<f64>().unwrap_or(0.0) >= 1024.0 && unit < units.len() - 1 {
        next /= 1024.0;
        unit += 1;
    }
    format!("{} {}", scaled(next), units[unit])
}

pub(super) fn format_percent(v: f64) -> String {
    format!("{v:.1}%")
}

/// `formatRelativeElapsed`: the value and its suffix.
pub(super) fn relative(at: i64, now: i64) -> (String, Option<&'static str>) {
    let minutes = (now - at).max(0) / 60_000;
    match minutes {
        0 => ("just now".into(), None),
        m if m < 60 => (format!("{m}m"), Some("ago")),
        m if m < 24 * 60 => (format!("{}h", m / 60), Some("ago")),
        m => (format!("{}d", m / (24 * 60)), Some("ago")),
    }
}

/// `RelativeTimeNoWrap`.
fn relative_no_wrap(at: Option<i64>, now: i64) -> String {
    match at.map(|at| relative(at, now)) {
        Some((value, Some(suffix))) => format!("{value}\u{a0}{suffix}"),
        Some((value, None)) => value,
        None => String::new(),
    }
}

fn shorten_trace_id(trace_id: &str) -> String {
    let chars: Vec<char> = trace_id.chars().collect();
    if chars.len() <= 32 {
        return trace_id.to_owned();
    }
    let head: String = chars[..18].iter().collect();
    let tail: String = chars[chars.len() - 10..].iter().collect();
    format!("{head}...{tail}")
}

fn is_stale_signal_message(message: Option<&str>) -> bool {
    message.is_some_and(|m| m.contains("not a live descendant"))
}

/// `formatProcessName`: the executable's file name.
pub(super) fn format_process_name(command: &str) -> String {
    let Some(first) = command.split_whitespace().next() else { return command.to_owned() };
    let first = first.strip_prefix(['\'', '"']).unwrap_or(first);
    let normalized = first.strip_suffix(['\'', '"']).unwrap_or(first);
    normalized
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .last()
        .unwrap_or(normalized)
        .to_owned()
}

fn format_short_process_name(command: &str) -> String {
    let name = format_process_name(command);
    if name.chars().count() > 42 {
        format!("{}...", name.chars().take(39).collect::<String>())
    } else {
        name
    }
}

/// `/\b(codex|claude|opencode|cursor)\b/i`.
fn names_agent(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    ["codex", "claude", "opencode", "cursor"].iter().any(|agent| {
        lower.match_indices(agent).any(|(start, _)| {
            let end = start + agent.len();
            !word(lower[..start].chars().next_back()) && !word(lower[end..].chars().next())
        })
    })
}

fn format_process_type(process: &Process) -> &'static str {
    if process.depth > 0 {
        "Subprocess"
    } else if names_agent(&process.command) {
        "Agent"
    } else {
        "Process"
    }
}

/// `visibleProcessTree`: rows in server order, minus the descendants of collapsed rows.
fn visible_process_tree<'a>(processes: &'a [Process], collapsed: &HashSet<i64>) -> Vec<&'a Process> {
    let mut visible = Vec::new();
    let mut hidden_below: Option<i64> = None;
    for process in processes {
        if let Some(depth) = hidden_below {
            if process.depth > depth {
                continue;
            }
            hidden_below = None;
        }
        visible.push(process);
        if collapsed.contains(&process.pid) {
            hidden_below = Some(process.depth);
        }
    }
    visible
}

/// Resource history indents children from the shallowest child, under the server root.
fn history_depths(processes: &[HistoryProcess]) -> Vec<i64> {
    let shallowest = processes.iter().filter(|p| !p.is_server_root).map(|p| p.depth).min();
    processes
        .iter()
        .map(|p| match shallowest {
            Some(min) if !p.is_server_root => (p.depth - min + 1).max(1),
            _ => 0,
        })
        .collect()
}

/// A chart bar's height as a percent of the tallest peak, never under 2.
fn bar_percent(value: f64, max: f64) -> f64 {
    (value / max * 100.0).max(2.0)
}

// --- Access ---------------------------------------------------------------------------------

#[derive(PartialEq)]
enum Access {
    Granted,
    Pending,
    Denied(&'static str),
}

/// `resolveUsageAccess`; fixtures have no server and read everything.
fn access(s: &App) -> Access {
    if !matches!(s.src, crate::data::Source::Live(_)) {
        return Access::Granted;
    }
    match super::permissions::reply(s) {
        Some(Err(_)) => Access::Denied("Could not check this connection's access to diagnostics and usage."),
        Some(Ok(session)) if super::permissions::grants(session, DIAGNOSTICS_READ) => Access::Granted,
        Some(Ok(_)) => Access::Denied("This connection does not have access to diagnostics and usage."),
        None if matches!(s.state().conn, s5_net::Conn::Closed(_)) => Access::Denied("This environment is not connected."),
        None => Access::Pending,
    }
}

// --- Requests -------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Which {
    Trace,
    Processes,
    History,
}

fn history_input(ui: &Ui) -> Value {
    let (_, window_ms, bucket_ms) = WINDOWS[ui.window_index()];
    json!({"windowMs": window_ms, "bucketMs": bucket_ms})
}

fn refresh(s: &mut App, which: Which) {
    let Some(net) = s.src.net() else { return };
    let ui = &mut s.ui.settings.diagnostics;
    match which {
        Which::Trace => {
            if ui.trace.pending.is_none() {
                ui.trace.pending = Some(net.request(TRACE, json!({})));
            }
        }
        Which::Processes => {
            if ui.processes.pending.is_none() {
                ui.processes.pending = Some(net.request(PROCESSES, json!({})));
            }
        }
        Which::History => {
            if ui.history.pending.is_none() {
                let input = history_input(ui);
                ui.history.pending = Some(net.request(HISTORY, input));
            }
        }
    }
}

fn settle<D>(query: &mut Query<D>, net: &mut s5_net::Net, decode: fn(&Value) -> D, now: i64) -> bool {
    let Some(id) = query.pending else { return false };
    let Some(reply) = net.take_reply(id) else { return false };
    query.pending = None;
    query.fetched_ms = now.max(1);
    match reply {
        Ok(v) => {
            query.data = Some(decode(&v));
            query.error = None;
        }
        Err(e) => query.error = Some(crate::shell::palette::error_message(&e, "The request failed.")),
    }
    true
}

/// Requests what is missing or stale and settles replies while the page is open; true when
/// anything the page shows changed.
pub fn tick(s: &mut App) -> bool {
    let now = s.now_ms;
    let granted = access(s) == Access::Granted;
    let env = s.ui.env.active.clone();
    let mut changed = false;
    if s.ui.settings.diagnostics.env.as_deref() != Some(env.as_str()) {
        s.ui.settings.diagnostics = Ui { env: Some(env), ..Ui::default() };
        changed = true;
    }
    let Some(net) = s.src.net() else { return changed };
    let ui = &mut s.ui.settings.diagnostics;
    let reentered = now - ui.last_tick > REENTRY_GAP_MS;
    ui.last_tick = now;
    for id in std::mem::take(&mut ui.discard) {
        if net.take_reply(id).is_none() {
            ui.discard.push(id);
        }
    }
    if granted {
        if ui.trace.due(now, reentered) {
            ui.trace.pending = Some(net.request(TRACE, json!({})));
            changed = true;
        }
        if ui.processes.due(now, reentered) {
            ui.processes.pending = Some(net.request(PROCESSES, json!({})));
            changed = true;
        }
        if ui.history.due(now, reentered) {
            let input = history_input(ui);
            ui.history.pending = Some(net.request(HISTORY, input));
            changed = true;
        }
    }
    changed |= settle(&mut ui.trace, net, decode_trace, now);
    changed |= settle(&mut ui.processes, net, decode_processes, now);
    changed |= settle(&mut ui.history, net, decode_history, now);
    if let Some(id) = ui.open_logs
        && let Some(reply) = net.take_reply(id)
    {
        changed = true;
        ui.open_logs = None;
        if let Err(e) = reply {
            ui.open_logs_error = Some(crate::shell::palette::error_message(&e, "Unable to open logs folder."));
        }
    }
    let signal_reply = ui.signal.as_ref().and_then(|sig| sig.request).and_then(|id| net.take_reply(id));
    if let Some(reply) = signal_reply {
        finish_signal(s, reply);
        changed = true;
    }
    changed
}

fn pick_window(s: &mut App, index: usize) {
    let ui = &mut s.ui.settings.diagnostics;
    if ui.window_index() == index {
        return;
    }
    ui.window = Some(index);
    if let Some(id) = ui.history.pending.take() {
        ui.discard.push(id);
    }
    ui.history = Query::default();
    refresh(s, Which::History);
}

fn toggle_collapsed(s: &mut App, pid: i64) {
    let collapsed = &mut s.ui.settings.diagnostics.collapsed;
    if !collapsed.remove(&pid) {
        collapsed.insert(pid);
    }
}

fn toggle_expanded(s: &mut App, key: &str) {
    let expanded = &mut s.ui.settings.diagnostics.expanded;
    if !expanded.remove(key) {
        expanded.insert(key.to_owned());
    }
}

/// `signalProcess`: a SIGKILL first opens the confirmation (`overlays`); SIGINT goes at once.
fn begin_signal(s: &mut App, pid: i64, signal: &'static str) {
    if !super::permissions::can(s, ENVIRONMENT_MAINTAIN) {
        return;
    }
    let ui = &mut s.ui.settings.diagnostics;
    if ui.signal.is_some() {
        return;
    }
    let start = ui.processes.data.as_ref().and_then(|d| d.processes.iter().find(|p| p.pid == pid)).map(|p| p.start_time_ms);
    let Some(start_time_ms) = start else { return };
    ui.signal = Some(Signal { pid, start_time_ms, signal, request: None });
    if signal != "SIGKILL" {
        send_signal(s);
    }
}

/// The server rejects a pid whose start time no longer matches, so a process that exited
/// during confirmation is never signaled by mistake.
fn send_signal(s: &mut App) {
    let allowed = super::permissions::can(s, ENVIRONMENT_MAINTAIN);
    let payload = s.ui.settings.diagnostics.signal.as_ref().filter(|_| allowed).map(|sig| {
        json!({"pid": sig.pid, "startTimeMs": sig.start_time_ms, "signal": sig.signal})
    });
    let id = match payload {
        Some(payload) => s.src.net().map(|n| n.request(SIGNAL, payload)),
        None => None,
    };
    let ui = &mut s.ui.settings.diagnostics;
    match id {
        Some(id) => {
            if let Some(sig) = ui.signal.as_mut() {
                sig.request = Some(id);
            }
        }
        None => ui.signal = None,
    }
}

fn cancel_signal(s: &mut App) {
    s.ui.settings.diagnostics.signal = None;
}

fn finish_signal(s: &mut App, reply: Result<Value, Value>) {
    let Some(sig) = s.ui.settings.diagnostics.signal.take() else { return };
    let signal = sig.signal;
    let fallback = format!("Failed to send {signal}.");
    let title = format!("Could not send {signal}");
    match reply {
        Err(error) => {
            let detail = crate::shell::palette::error_message(&error, &fallback);
            crate::shell::palette::toast_error_brief(s, &title, Some(&detail));
        }
        Ok(result) => {
            let result = decode_signal(&result);
            refresh(s, Which::Processes);
            if result.signaled {
                return;
            }
            if is_stale_signal_message(result.message.as_deref()) {
                crate::shell::palette::toast(
                    s,
                    "Process already exited",
                    Some("The process is not a child of the T3 Server. It might already have exited."),
                );
                return;
            }
            let detail = result.message.unwrap_or(fallback);
            crate::shell::palette::toast_error_brief(s, &title, Some(&detail));
        }
    }
}

fn logs_directory(s: &App) -> Option<String> {
    s.state()
        .config
        .as_ref()
        .and_then(|c| c["observability"]["logsDirectoryPath"].as_str())
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
}

/// The editor `useOpenInPreferredEditor` falls back to: the first one the host found.
fn first_editor(s: &App) -> Option<String> {
    if let Some(remote) = crate::env::ssh::editors(s) {
        return remote.first().map(|e| e.0.clone());
    }
    s.state()
        .config
        .as_ref()
        .and_then(|c| c["availableEditors"].as_array())
        .and_then(|v| v.first())
        .and_then(|e| e.as_str().or_else(|| e["id"].as_str()))
        .map(str::to_owned)
}

fn open_logs(s: &mut App) {
    let Some(path) = logs_directory(s) else { return };
    if !super::permissions::can(s, ORCHESTRATION_OPERATE) || s.ui.settings.diagnostics.open_logs.is_some() {
        return;
    }
    s.ui.settings.diagnostics.open_logs_error = None;
    let Some(editor) = first_editor(s) else {
        let environment = s
            .state()
            .config
            .as_ref()
            .and_then(|c| c["environment"]["environmentId"].as_str())
            .unwrap_or("")
            .to_owned();
        s.ui.settings.diagnostics.open_logs_error =
            Some(format!("No available editor can open {path} in environment {environment}."));
        return;
    };
    if crate::env::ssh::open_in_editor(s, &editor, &path) {
        return;
    }
    let id = s.src.net().map(|n| n.request("shell.openInEditor", json!({"cwd": path, "editor": editor})));
    s.ui.settings.diagnostics.open_logs = id;
}

// --- View -----------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Tone {
    Default,
    Warning,
    Danger,
}

fn label_font() -> rune::TextStyle {
    // `text-2xs font-semibold uppercase tracking-widest`.
    rune::TextStyle { tracking: 1.1, ..theme::weight(theme::TEXT_2XS, theme::SEMIBOLD) }
}

fn note(message: &str, t: &T) -> El {
    text(message.to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap()
}

/// `StatBlock`.
fn stat(label: &str, value: String, tip: Option<String>, tone: Tone, t: &T) -> El {
    let mut head = row().min_w(0.0).gap(6.0).center_y().child(
        text(label.to_uppercase())
            .font(label_font())
            .color(t.c.muted_foreground.alpha_mul(0.7))
            .truncate()
            .shrink()
            .min_w(0.0),
    );
    if let Some(tip) = tip {
        head = head.child(
            rune::icon(&icons::INFO, 12.0)
                .color(t.c.muted_foreground.alpha_mul(0.6))
                .focusable(true)
                .label(format!("{label} details"))
                .help(tip),
        );
    }
    let color = match tone {
        Tone::Default => t.c.foreground,
        Tone::Warning => t.c.warning_foreground,
        Tone::Danger => t.c.destructive,
    };
    col()
        .grow(1.0)
        .min_w(0.0)
        .px(20.0)
        .py(12.0)
        .child(head)
        .child(
            text(value)
                .font(theme::weight(theme::mono(theme::TEXT_LG), theme::SEMIBOLD))
                .color(color)
                .truncate()
                .mt(4.0),
        )
}

/// `StatsGrid`: four blocks with rules between them.
pub(super) fn stats(blocks: Vec<El>, t: &T) -> El {
    let mut grid = row().w_full().items_stretch();
    for (i, block) in blocks.into_iter().enumerate() {
        if i > 0 {
            grid = grid.child(el().w(1.0).self_stretch().fill(t.c.border.alpha_mul(0.6)));
        }
        grid = grid.child(block);
    }
    grid
}

pub(super) fn alert_line(message: String, color: Color) -> El {
    row()
        .w_full()
        .gap(8.0)
        .items_start()
        .child(rune::icon(&icons::TRIANGLE_ALERT, 14.0).color(color).mt(1.0))
        .child(text(message).font(theme::TEXT_XS).color(color).wrap().selectable(true).grow(1.0).min_w(0.0))
}

/// The red (or amber) lines under a section's stats; `None` when there are none.
fn alerts(lines: Vec<El>) -> Option<El> {
    (!lines.is_empty()).then(|| col().w_full().px(20.0).py(12.0).gap(8.0).children(lines))
}

pub(super) fn empty_rows(label: &str, t: &T) -> El {
    text(label.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).px(20.0).py(16.0)
}

/// `DiagnosticsLastChecked`.
fn last_checked(read_at: Option<Option<i64>>, now: i64, t: &T) -> El {
    let (label, alpha) = match read_at {
        None => ("Checking".to_owned(), 0.5),
        Some(None) => ("Checked unavailable".to_owned(), 0.5),
        Some(Some(at)) => match relative(at, now) {
            (value, Some(suffix)) => (format!("Checked {value} {suffix}"), 0.6),
            (value, None) => (format!("Checked {value}"), 0.6),
        },
    };
    text(label).font(theme::TEXT_2XS).color(t.c.muted_foreground.alpha_mul(alpha))
}

/// `DiagnosticsRefreshButton`; the refresh glyph turns into the busy spinner while loading.
fn refresh_button(pending: bool, label: &'static str, which: Which, t: &T) -> El {
    crate::kit::Button::new(label)
        .variant("ghost-muted")
        .size("icon-xs")
        .icon(if pending { &icons::LOADER_CIRCLE } else { &icons::REFRESH_CW })
        .disabled(pending)
        .on_press(move |s: &mut App| refresh(s, which))
        .build(t)
        .help(label)
}

/// `InlineButton`: medium xs text, dimmed and unreachable when disabled.
pub(super) fn inline_button(label: &str, color: Color, enabled: bool, t: &T, f: impl Fn(&mut App) + 'static) -> El {
    let control = text(label.to_owned())
        .font(theme::XS_MEDIUM)
        .color(color)
        .focusable(true)
        .role("button")
        .label(label.to_owned())
        .cursor(rune::Cursor::Pointer)
        .focus_ring(2.0, -2.0, t.c.ring);
    c::press_if(control, enabled, f)
}

/// `ExpandableText`: clamped to `lines` until expanded, when long or multi-line.
fn expandable(ui: &Ui, t: &T, key: String, body: &str, lines: usize, expand_label: &'static str) -> El {
    let can_expand = body.chars().count() > 180 || body.contains('\n');
    let open = ui.expanded.contains(&key);
    let mut copy = text(body.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().selectable(true);
    if can_expand && !open {
        copy = copy.max_lines(lines);
    }
    let mut out = col().w_full().min_w(0.0).child(copy);
    if can_expand {
        out = out.child(
            inline_button(if open { "Show less" } else { expand_label }, t.c.muted_foreground, true, t, move |s: &mut App| {
                toggle_expanded(s, &key)
            })
            .expanded(open)
            .mt(4.0)
            .self_start(),
        );
    }
    out
}

/// `TraceIdCell`: the shortened ID, its full text on hover, and a copy button.
fn trace_cell(ui: &Ui, now: i64, t: &T, trace_id: &str, key: String) -> El {
    let copied = ui.copied.as_ref().is_some_and(|(k, until)| *k == key && now < *until);
    let id = trace_id.to_owned();
    let copy = crate::kit::Button::new(if copied { "Copied trace ID" } else { "Copy trace ID" })
        .variant("ghost-muted")
        .size("icon-micro")
        .icon(&icons::COPY)
        .build(t)
        .help(if copied { "Copied" } else { "Copy full trace ID" })
        .on_press_tx(move |s: &mut App, tx: &mut rune::Tx| {
            tx.copy(id.clone());
            s.ui.settings.diagnostics.copied = Some((key.clone(), crate::data::now_ms() + 1_200));
            tx.wake_after(std::time::Duration::from_millis(1_200));
        });
    row()
        .w_full()
        .min_w(0.0)
        .gap(8.0)
        .center_y()
        .child(
            text(shorten_trace_id(trace_id))
                .font(theme::mono(theme::TEXT_2XS))
                .color(t.c.muted_foreground)
                .truncate()
                .grow(1.0)
                .min_w(0.0)
                .help(trace_id.to_owned()),
        )
        .child(copy)
}

pub(super) struct Column {
    pub(super) header: &'static str,
    pub(super) weight: f32,
    pub(super) end: bool,
    /// This column's horizontal padding when it differs from the table's (web `p-2`).
    pub(super) pad: Option<f32>,
}

pub(super) const fn column(header: &'static str, weight: f32, end: bool) -> Column {
    Column { header, weight, end, pad: None }
}

/// Web's `<table>`, at least `min_w` wide and scrolling sideways when the card is narrower.
/// `fixed` is `table-fixed`: every column takes its percentage of the width, whatever it holds;
/// otherwise columns start from their content and share what is left. `top` is `align-top`.
pub(super) fn table(t: &T, min_w: f32, pad: f32, top: bool, fixed: bool, columns: &[Column], rows: Vec<Vec<El>>) -> El {
    let cell = |c: &Column, content: El, py: f32| {
        let mut cell = row().grow(c.weight).min_w(0.0).px(c.pad.unwrap_or(pad)).py(py);
        if fixed {
            cell = cell.w(0.0);
        }
        cell = if top { cell.items_start() } else { cell.center_y() };
        if c.end {
            cell = cell.justify_end();
        }
        cell.child(content)
    };
    let header = row().w_full().px(20.0 - pad).children(columns.iter().map(|c| {
        cell(c, text(c.header.to_uppercase()).font(label_font()).color(t.c.muted_foreground.alpha_mul(0.7)).truncate(), 10.0)
    }));
    let rule = || el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.5));
    let mut body = col().w_full().min_w(min_w).child(header).child(rule());
    let py = if top { 12.0 } else { 8.0 };
    for (i, cells) in rows.into_iter().enumerate() {
        if i > 0 {
            body = body.child(rule());
        }
        let line = row()
            .w_full()
            .px(20.0 - pad)
            .hover(t.c.muted.alpha_mul(0.2))
            .children(columns.iter().zip(cells).map(|(c, content)| cell(c, content, py)));
        body = body.child(line);
    }
    col().w_full().scroll_x().child(body)
}

pub(super) fn mono_cell(value: String, color: Color) -> El {
    text(value).font(theme::MONO_XS).color(color)
}

fn name_cell(name: &str, t: &T) -> El {
    text(name.to_owned()).font(theme::XS_MEDIUM).color(t.c.foreground).wrap().selectable(true).min_w(0.0)
}

pub(super) fn dot(color: Color) -> El {
    el().size(6.0, 6.0).radius(3.0).fill(color)
}

pub fn view(s: &App, _cx: &mut Cx<App>, t: &T) -> El {
    match access(s) {
        Access::Granted => {}
        Access::Pending => return note("Checking diagnostics access…", t),
        Access::Denied(message) => return note(message, t),
    }
    let blank;
    let ui = if s.ui.settings.diagnostics.env.as_deref() == Some(s.ui.env.active.as_str()) {
        &s.ui.settings.diagnostics
    } else {
        blank = Ui::default();
        &blank
    };
    col()
        .w_full()
        .gap(40.0)
        .child(super::resource_telemetry::view(s, t))
        .child(processes_section(s, ui, t))
        .child(history_section(ui, s.now_ms, t))
        .child(trace_section(s, ui, t))
        .children(trace_tables(ui, s.now_ms, t))
        .child(super::telemetry::view(s, t))
}

fn processes_section(s: &App, ui: &Ui, t: &T) -> El {
    let q = &ui.processes;
    let data = q.data.as_ref();
    let dots = || "...".to_owned();
    let blocks = vec![
        stat("Child Processes", data.map_or_else(dots, |d| format_count(d.process_count)), None, Tone::Default, t),
        stat(
            "CPU",
            data.map_or_else(dots, |d| format_percent(d.total_cpu_percent)),
            Some("Total CPU across live child processes of the current server process. The desktop shell and other parent processes are not included.".into()),
            Tone::Default,
            t,
        ),
        stat(
            "Memory",
            data.map_or_else(dots, |d| format_bytes(d.total_rss_bytes)),
            Some("Total resident memory across live child processes of the current server process. The desktop shell and other parent processes are not included.".into()),
            Tone::Default,
            t,
        ),
        stat("Server PID", data.map_or_else(dots, |d| d.server_pid.to_string()), None, Tone::Default, t),
    ];
    let mut children = vec![stats(blocks, t)];
    let mut lines = Vec::new();
    if let Some(message) = data.and_then(|d| d.error.clone()) {
        lines.push(alert_line(message, t.c.destructive));
    }
    if let Some(error) = q.error.clone() {
        lines.push(alert_line(error, t.c.destructive));
    }
    children.extend(alerts(lines));
    children.push(process_table(s, ui, t));
    c::section_with_actions(
        "Live Processes",
        vec![
            last_checked(data.map(|d| d.read_at), s.now_ms, t),
            refresh_button(q.pending.is_some(), "Refresh process diagnostics", Which::Processes, t),
        ],
        children,
        t,
    )
}

const PROCESS_COLUMNS: [Column; 7] = [
    column("Name", 24.0, false),
    column("CPU", 8.0, true),
    column("Memory", 10.0, true),
    column("Command", 33.0, false),
    column("PID", 8.0, true),
    column("Type", 11.0, false),
    Column { header: "Kill", weight: 6.0, end: true, pad: Some(8.0) },
];

/// `ProcessDiagnosticsTable`.
fn process_table(s: &App, ui: &Ui, t: &T) -> El {
    let processes = ui.processes.data.as_ref().map(|d| d.processes.as_slice()).unwrap_or_default();
    let visible = visible_process_tree(processes, &ui.collapsed);
    let can_maintain = super::permissions::can(s, ENVIRONMENT_MAINTAIN);
    let signaling = ui.signal.as_ref().map(|sig| sig.pid);
    let rows: Vec<Vec<El>> = visible
        .iter()
        .map(|p| {
            let name = format_process_name(&p.command);
            let expanded = !ui.collapsed.contains(&p.pid);
            let pid = p.pid;
            let toggle = if p.child_pids.is_empty() {
                el().size(20.0, 20.0)
            } else {
                crate::kit::Button::new(if expanded { format!("Collapse {name}") } else { format!("Expand {name}") })
                    .variant("ghost-muted")
                    .size("icon-micro")
                    .icon(if expanded { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT })
                    .on_press(move |s: &mut App| toggle_collapsed(s, pid))
                    .build(t)
            };
            let name_el = row()
                .w_full()
                .min_w(0.0)
                .pl(p.depth.min(6) as f32 * 10.0)
                .gap(8.0)
                .center_y()
                .child(toggle)
                .child(dot(t.c.success.alpha_mul(0.8)))
                .child(
                    text(name)
                        .font(theme::XS_MEDIUM)
                        .color(t.c.foreground)
                        .truncate()
                        .shrink()
                        .min_w(0.0)
                        .help(p.command.clone()),
                );
            let enabled = can_maintain && signaling != Some(pid);
            let kill = row()
                .gap(6.0)
                .center_y()
                .child(
                    inline_button("INT", t.c.muted_foreground, enabled, t, move |s: &mut App| begin_signal(s, pid, "SIGINT"))
                        .label("Send SIGINT")
                        .help("Send SIGINT"),
                )
                .child(
                    inline_button("KILL", t.c.destructive.alpha_mul(0.8), enabled, t, move |s: &mut App| begin_signal(s, pid, "SIGKILL"))
                        .label("Send SIGKILL")
                        .help("Send SIGKILL"),
                );
            vec![
                name_el,
                mono_cell(format_percent(p.cpu_percent), t.c.foreground),
                mono_cell(format_bytes(p.rss_bytes), t.c.foreground),
                text(p.command.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().min_w(0.0).help(p.command.clone()),
                mono_cell(p.pid.to_string(), t.c.muted_foreground),
                text(format_process_type(p)).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().min_w(0.0),
                kill,
            ]
        })
        .collect();
    let empty = rows.is_empty();
    let table = table(t, 1040.0, 12.0, false, true, &PROCESS_COLUMNS, rows);
    if !empty {
        return table;
    }
    let label = if ui.processes.initial_loading() { "Loading live processes..." } else { "No live descendant processes found." };
    col().w_full().child(table).child(empty_rows(label, t))
}

fn history_section(ui: &Ui, now: i64, t: &T) -> El {
    let q = &ui.history;
    let data = q.data.as_ref();
    let dots = || "...".to_owned();
    let blocks = vec![
        stat(
            "CPU Time",
            data.map_or_else(dots, |d| format_cpu_time(d.total_cpu_seconds)),
            Some("Approximate active CPU time for the T3 server root process and its descendants during the selected window. It grows only while sampled processes use CPU and older samples leave as the window moves.".into()),
            Tone::Default,
            t,
        ),
        stat(
            "Samples",
            data.map_or_else(dots, |d| format_count(d.retained_sample_count)),
            Some("In-memory process samples retained by the server. The server samples only while diagnostics is open, and this resets when the server restarts.".into()),
            Tone::Default,
            t,
        ),
        stat(
            "Interval",
            data.map_or_else(dots, |d| {
                if d.sample_interval_ms == 0.0 { "On demand".to_owned() } else { format_duration(d.sample_interval_ms) }
            }),
            None,
            Tone::Default,
            t,
        ),
        stat("Processes", data.map_or_else(dots, |d| format_count(d.top_processes.len() as i64)), None, Tone::Default, t),
    ];
    let mut children = vec![stats(blocks, t)];
    let mut lines = Vec::new();
    if let Some(message) = data.and_then(|d| d.error.clone()) {
        lines.push(alert_line(message, t.c.destructive));
    }
    if let Some(error) = q.error.clone() {
        lines.push(alert_line(error, t.c.destructive));
    }
    children.extend(alerts(lines));
    children.push(chart(data.map(|d| d.buckets.as_slice()).unwrap_or_default(), t));
    children.push(history_table(ui, t));
    let window = crate::kit::ToggleGroup::new(&WINDOWS.map(|w| w.0), &[ui.window_index()])
        .on_pick(|s: &mut App, i: usize, _: &mut rune::Tx| pick_window(s, i))
        .build(&crate::kit::theme(t));
    c::section_with_actions(
        "Resource History",
        vec![
            row().role("group").label("Process history period").child(window),
            last_checked(data.map(|d| d.read_at), now, t),
            refresh_button(q.pending.is_some(), "Refresh resource history", Which::History, t),
        ],
        children,
        t,
    )
}

/// `ProcessResourceHistoryChart`: per bucket, the peak bar with the average over it.
fn chart(buckets: &[Bucket], t: &T) -> El {
    const INNER: f64 = 96.0;
    let max = buckets.iter().map(|b| b.max_cpu_percent).fold(1.0_f64, f64::max);
    let mut bars = row()
        .w_full()
        .h(112.0)
        .padding(8.0)
        .gap(4.0)
        .items_end()
        .radius(4.0)
        .fill(t.c.muted.alpha_mul(0.1));
    for bucket in buckets {
        let peak = (bar_percent(bucket.max_cpu_percent, max) / 100.0 * INNER) as f32;
        let average = (bar_percent(bucket.avg_cpu_percent, max) / 100.0 * INNER) as f32;
        bars = bars.child(
            col()
                .grow(1.0)
                .min_w(4.0)
                .h_full()
                .justify_end()
                .label(format!(
                    "Average CPU {:.1}%, peak CPU {:.1}%",
                    bucket.avg_cpu_percent, bucket.max_cpu_percent
                ))
                .help(format!("Avg {:.1}%, peak {:.1}%", bucket.avg_cpu_percent, bucket.max_cpu_percent))
                .child(el().w_full().h((peak - average).max(0.0)).radius(2.0).fill(t.c.foreground.alpha_mul(0.15)))
                .child(el().w_full().h(average).radius(2.0).fill(t.c.foreground.alpha_mul(0.6))),
        );
    }
    col().w_full().px(20.0).py(12.0).child(bars)
}

const HISTORY_COLUMNS: [Column; 8] = [
    column("Process", 24.0, false),
    column("CPU Time", 10.0, true),
    column("Current", 10.0, true),
    column("Average", 10.0, true),
    column("Peak", 10.0, true),
    column("Max Mem", 10.0, true),
    column("Command", 16.0, false),
    column("PID", 10.0, true),
];

/// `ProcessResourceHistoryTable`.
fn history_table(ui: &Ui, t: &T) -> El {
    let processes = ui.history.data.as_ref().map(|d| d.top_processes.as_slice()).unwrap_or_default();
    let depths = history_depths(processes);
    let rows: Vec<Vec<El>> = processes
        .iter()
        .zip(depths)
        .map(|(p, depth)| {
            let name = format_short_process_name(&p.command);
            let role = if p.is_server_root { "Root" } else { "Child" };
            let name_el = row()
                .key(format!("diagnostics-history-{}", p.process_key))
                .w_full()
                .min_w(0.0)
                .pl(depth.min(6) as f32 * 10.0)
                .gap(8.0)
                .center_y()
                .label(format!("{role} process {name}"))
                .child(el().size(20.0, 20.0))
                .child(dot(if p.is_server_root { t.c.warning.alpha_mul(0.9) } else { t.c.success.alpha_mul(0.8) }))
                .child(
                    text(name)
                        .font(theme::XS_MEDIUM)
                        .color(t.c.foreground)
                        .truncate()
                        .shrink()
                        .min_w(0.0)
                        .help(p.command.clone()),
                );
            vec![
                name_el,
                mono_cell(format_cpu_time(p.cpu_seconds), t.c.foreground),
                mono_cell(format_percent(p.current_cpu_percent), t.c.foreground),
                mono_cell(format_percent(p.avg_cpu_percent), t.c.foreground),
                mono_cell(format_percent(p.max_cpu_percent), t.c.foreground),
                mono_cell(format_bytes(p.max_rss_bytes), t.c.foreground),
                text(p.command.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().min_w(0.0).help(p.command.clone()),
                mono_cell(p.pid.to_string(), t.c.muted_foreground),
            ]
        })
        .collect();
    let empty = rows.is_empty();
    let table = table(t, 980.0, 12.0, false, true, &HISTORY_COLUMNS, rows);
    if !empty {
        return table;
    }
    let label = if ui.history.initial_loading() {
        "Collecting process resource samples..."
    } else {
        "No process resource samples found for this window."
    };
    col().w_full().child(table).child(empty_rows(label, t))
}

fn trace_section(s: &App, ui: &Ui, t: &T) -> El {
    let q = &ui.trace;
    let data = q.data.as_ref();
    let dots = || "...".to_owned();
    let tone = |n: Option<i64>, tone: Tone| if n.is_some_and(|n| n > 0) { tone } else { Tone::Default };
    let blocks = vec![
        stat("Spans", data.map_or_else(dots, |d| format_count(d.record_count)), None, Tone::Default, t),
        stat(
            "Failures",
            data.map_or_else(dots, |d| format_count(d.failure_count)),
            None,
            tone(data.map(|d| d.failure_count), Tone::Danger),
            t,
        ),
        stat(
            "Slow Spans",
            data.map_or_else(dots, |d| format_count(d.slow_span_count)),
            Some(match data {
                Some(d) => format!("Spans with a duration of {} or longer.", format_duration(d.slow_span_threshold_ms)),
                None => "Spans at or above the configured slow-span threshold.".to_owned(),
            }),
            tone(data.map(|d| d.slow_span_count), Tone::Warning),
            t,
        ),
        stat(
            "Parse Errors",
            data.map_or_else(dots, |d| format_count(d.parse_error_count)),
            None,
            tone(data.map(|d| d.parse_error_count), Tone::Warning),
            t,
        ),
    ];
    let mut children = vec![stats(blocks, t)];
    let mut lines = Vec::new();
    if let Some(error) = ui.open_logs_error.clone() {
        lines.push(alert_line(error, t.c.destructive));
    }
    if let Some(d) = data
        && let Some(message) = &d.error
    {
        lines.push(if d.partial_failure {
            alert_line(
                format!("Some trace files could not be read, so diagnostics may be incomplete. {message}"),
                t.c.warning_foreground,
            )
        } else {
            alert_line(message.clone(), t.c.destructive)
        });
    }
    if let Some(error) = q.error.clone() {
        lines.push(alert_line(error, t.c.destructive));
    }
    children.extend(alerts(lines));
    let can_open = super::permissions::can(s, ORCHESTRATION_OPERATE) && logs_directory(s).is_some() && ui.open_logs.is_none();
    let folder = crate::kit::Button::new("Open logs folder")
        .variant("ghost-muted")
        .size("icon-xs")
        .icon(&icons::FOLDER_OPEN)
        .disabled(!can_open)
        .on_press(open_logs)
        .build(t)
        .help("Open logs folder");
    c::section_with_actions(
        "Trace Diagnostics",
        vec![
            last_checked(data.map(|d| d.read_at), s.now_ms, t),
            folder,
            refresh_button(q.pending.is_some(), "Refresh trace diagnostics", Which::Trace, t),
        ],
        children,
        t,
    )
}

const LATEST_COLUMNS: [Column; 4] =
    [column("Span", 28.0, false), column("Cause", 48.0, false), column("Duration", 12.0, false), column("Ended", 12.0, false)];
const COMMON_COLUMNS: [Column; 4] =
    [column("Span", 28.0, false), column("Count", 10.0, false), column("Cause", 48.0, false), column("Last Seen", 14.0, false)];
const SLOWEST_COLUMNS: [Column; 4] =
    [column("Span", 44.0, false), column("Duration", 14.0, false), column("Ended", 12.0, false), column("Trace", 30.0, false)];
const LOG_COLUMNS: [Column; 5] = [
    column("Time", 11.0, false),
    column("Level", 9.0, false),
    column("Span", 24.0, false),
    column("Message", 26.0, false),
    column("Trace", 30.0, false),
];
const TOP_COLUMNS: [Column; 5] = [
    column("Span", 48.0, false),
    column("Count", 13.0, false),
    column("Failures", 13.0, false),
    column("Average", 13.0, false),
    column("Max", 13.0, false),
];

/// Latest Failures, Most Common Failures, Slowest Spans, Span Logs and Top Span Names.
fn trace_tables(ui: &Ui, now: i64, t: &T) -> Vec<El> {
    let data = ui.trace.data.as_ref();
    let loading = ui.trace.initial_loading();
    let section = |title: &str, body: Option<El>, loading_label: &str, empty_label: &str| {
        let body = body.unwrap_or_else(|| empty_rows(if loading { loading_label } else { empty_label }, t));
        c::section(title, vec![body], t)
    };
    let muted = t.c.muted_foreground;

    let latest = data.filter(|d| !d.latest_failures.is_empty()).map(|d| {
        let rows = d
            .latest_failures
            .iter()
            .map(|f| {
                vec![
                    name_cell(&f.name, t),
                    expandable(ui, t, format!("latest:{}:{}", f.trace_id, f.span_id), &f.cause, 3, "Show full error"),
                    mono_cell(format_duration(f.duration_ms), t.c.foreground),
                    mono_cell(relative_no_wrap(f.ended_at, now), muted),
                ]
            })
            .collect();
        table(t, 640.0, 16.0, true, false, &LATEST_COLUMNS, rows)
    });
    let common = data.filter(|d| !d.common_failures.is_empty()).map(|d| {
        let rows = d
            .common_failures
            .iter()
            .map(|f| {
                vec![
                    name_cell(&f.name, t),
                    mono_cell(format_count(f.count), t.c.foreground),
                    expandable(ui, t, format!("common:{}:{}", f.name, f.cause), &f.cause, 3, "Show full error"),
                    mono_cell(relative_no_wrap(f.last_seen_at, now), muted),
                ]
            })
            .collect();
        table(t, 760.0, 16.0, true, false, &COMMON_COLUMNS, rows)
    });
    let slowest = data.filter(|d| !d.slowest.is_empty()).map(|d| {
        let rows = d
            .slowest
            .iter()
            .map(|span| {
                vec![
                    name_cell(&span.name, t),
                    mono_cell(format_duration(span.duration_ms), t.c.foreground),
                    mono_cell(relative_no_wrap(span.ended_at, now), muted),
                    trace_cell(ui, now, t, &span.trace_id, format!("slowest:{}:{}", span.trace_id, span.span_id)),
                ]
            })
            .collect();
        table(t, 900.0, 16.0, true, true, &SLOWEST_COLUMNS, rows)
    });
    let logs = data.filter(|d| !d.logs.is_empty()).map(|d| {
        let rows = d
            .logs
            .iter()
            .map(|event| {
                let key = format!("log:{}:{}:{}:{}", event.trace_id, event.span_id, event.seen_at.unwrap_or(0), event.message);
                let level = row().self_start().px(6.0).py(2.0).radius(4.0).fill(t.c.muted).child(
                    text(event.level.to_uppercase())
                        .font(theme::weight(theme::mono(theme::TEXT_2XS), theme::MEDIUM))
                        .color(t.c.foreground.alpha_mul(0.8)),
                );
                vec![
                    mono_cell(relative_no_wrap(event.seen_at, now), muted),
                    level,
                    text(event.span_name.clone()).font(theme::XS_MEDIUM).color(t.c.foreground).truncate().min_w(0.0).help(event.span_name.clone()),
                    expandable(ui, t, key.clone(), &event.message, 2, "Show full message"),
                    trace_cell(ui, now, t, &event.trace_id, key),
                ]
            })
            .collect();
        table(t, 920.0, 16.0, true, true, &LOG_COLUMNS, rows)
    });
    let top = data.filter(|d| !d.top_spans.is_empty()).map(|d| {
        let rows = d
            .top_spans
            .iter()
            .map(|span| {
                vec![
                    name_cell(&span.name, t),
                    mono_cell(format_count(span.count), t.c.foreground),
                    mono_cell(format_count(span.failure_count), t.c.foreground),
                    mono_cell(format_duration(span.average_ms), t.c.foreground),
                    mono_cell(format_duration(span.max_ms), t.c.foreground),
                ]
            })
            .collect();
        table(t, 760.0, 16.0, true, true, &TOP_COLUMNS, rows)
    });
    vec![
        section("Latest Failures", latest, "Loading failures...", "No failed spans found."),
        section("Most Common Failures", common, "Loading failure groups...", "No repeated failures found."),
        section("Slowest Spans", slowest, "Loading slow spans...", "No spans found."),
        section("Span Logs", logs, "Loading recent logs...", "No warnings or errors found."),
        section("Top Span Names", top, "Loading span names...", "No spans found."),
    ]
}

/// The SIGKILL confirmation, as web's destructive `dialogs.confirm` splits its message.
pub fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(sig) = &s.ui.settings.diagnostics.signal else { return };
    if sig.signal != "SIGKILL" || sig.request.is_some() {
        return;
    }
    let cancel = crate::kit::Button::new("Cancel").variant("outline").on_press(cancel_signal).build(t);
    let confirm = crate::kit::Button::new("Confirm").variant("destructive").on_press(send_signal).build(t);
    crate::kit::Dialog::new("diagnostics-sigkill", format!("Send SIGKILL to process {}?", sig.pid))
        .alert()
        .width(448.0)
        .open(true)
        .description("This cannot be handled by the process.")
        .child(row().w_full().gap(8.0).justify_end().child(cancel).child(confirm))
        .on_close(cancel_signal)
        .build(t, layers);
}

// --- Fixture --------------------------------------------------------------------------------

/// Populated results for headless scenes: a provider tree, an hour of CPU buckets, and span
/// tables with a failure, a slow span and warning logs.
pub fn fixture(s: &mut App) {
    let now = s.now_ms;
    let ago = |ms: i64| Some(now - ms);
    let process = |pid: i64, depth: i64, cpu: f64, mb: f64, command: &str, children: &[i64]| Process {
        pid,
        start_time_ms: now - 3_600_000 + pid,
        cpu_percent: cpu,
        rss_bytes: mb * 1024.0 * 1024.0,
        command: command.to_owned(),
        depth,
        child_pids: children.to_vec(),
    };
    let processes = vec![
        process(48302, 0, 12.4, 412.0, "/opt/homebrew/bin/codex app-server --listen stdio://", &[48310, 48311]),
        process(48310, 1, 38.2, 96.5, "/bin/zsh -lc vp run test", &[48390]),
        process(48390, 2, 61.7, 288.0, "node /Users/dev/t3code/node_modules/.bin/vitest run --reporter=dot", &[]),
        process(48311, 1, 0.4, 12.2, "rg --files --hidden --glob !.git", &[]),
        process(48420, 0, 3.1, 233.0, "claude --output-format stream-json --verbose --input-format stream-json", &[]),
        process(48501, 0, 0.0, 6.4, "/bin/zsh -l", &[48502]),
        process(48502, 1, 1.8, 141.0, "node /Users/dev/t3code/node_modules/.bin/vp run dev", &[]),
    ];
    let total_cpu: f64 = processes.iter().map(|p| p.cpu_percent).sum();
    let total_rss: f64 = processes.iter().map(|p| p.rss_bytes).sum();
    let buckets = (0..30)
        .map(|i| {
            let avg = 6.0 + 5.0 * (i as f64 / 3.0).sin().abs() + (i % 7) as f64 * 0.8;
            Bucket { avg_cpu_percent: avg, max_cpu_percent: avg * 1.6 + 3.0 }
        })
        .collect();
    let history_process = |pid: i64, depth: i64, root: bool, cpu: f64, seconds: f64, mb: f64, command: &str| HistoryProcess {
        process_key: format!("{pid}:{}", now - 3_600_000),
        pid,
        command: command.to_owned(),
        depth,
        is_server_root: root,
        current_cpu_percent: cpu,
        avg_cpu_percent: cpu * 0.7,
        max_cpu_percent: cpu * 1.9,
        cpu_seconds: seconds,
        max_rss_bytes: mb * 1024.0 * 1024.0,
    };
    let top_processes = vec![
        history_process(48211, 0, true, 4.2, 96.4, 318.0, "/Applications/T3 Code.app/Contents/Resources/node dist/bin.mjs serve"),
        history_process(48390, 3, false, 61.7, 241.0, 402.0, "node /Users/dev/t3code/node_modules/.bin/vitest run --reporter=dot"),
        history_process(48310, 2, false, 38.2, 74.5, 112.0, "/bin/zsh -lc vp run test"),
        history_process(48302, 1, false, 12.4, 61.2, 455.0, "/opt/homebrew/bin/codex app-server --listen stdio://"),
        history_process(48420, 1, false, 3.1, 8.75, 240.0, "claude --output-format stream-json --verbose"),
        history_process(48502, 2, false, 1.8, 3.4, 150.0, "node /Users/dev/t3code/node_modules/.bin/vp run dev"),
    ];
    let span = |name: &str, count: i64, failures: i64, average: f64, max: f64| SpanSummary {
        name: name.to_owned(),
        count,
        failure_count: failures,
        average_ms: average,
        max_ms: max,
    };
    let trace_id = |n: u32| format!("{:032x}", 0x4bf92f3577b34da6a3ce929d0e0e4736_u128 + n as u128);
    let trace = Trace {
        read_at: ago(40_000),
        record_count: 18_432,
        parse_error_count: 0,
        failure_count: 7,
        slow_span_threshold_ms: 1_000.0,
        slow_span_count: 23,
        top_spans: vec![
            span("orchestration.dispatchCommand", 4_812, 2, 18.0, 942.0),
            span("projection.apply", 3_904, 0, 2.4, 61.0),
            span("provider.codex.turn", 214, 3, 28_410.0, 192_300.0),
            span("checkpoint.capture", 211, 1, 640.0, 4_820.0),
            span("vcs.status", 1_288, 1, 46.0, 1_210.0),
            span("ws.rpc", 6_003, 0, 4.1, 380.0),
        ],
        slowest: vec![
            SpanOccurrence { name: "provider.codex.turn".into(), duration_ms: 192_300.0, ended_at: ago(14 * 60_000), trace_id: trace_id(1), span_id: "a3ce929d0e0e4736".into() },
            SpanOccurrence { name: "provider.claude.turn".into(), duration_ms: 88_120.0, ended_at: ago(42 * 60_000), trace_id: trace_id(2), span_id: "b7ad6b7169203331".into() },
            SpanOccurrence { name: "checkpoint.capture".into(), duration_ms: 4_820.0, ended_at: ago(3 * 3_600_000), trace_id: trace_id(3), span_id: "c0ffee0123456789".into() },
        ],
        common_failures: vec![
            FailureSummary { name: "provider.codex.turn".into(), cause: "ProviderProcessExitedError: codex exited with code 1".into(), count: 3, last_seen_at: ago(14 * 60_000) },
            FailureSummary { name: "orchestration.dispatchCommand".into(), cause: "OrchestrationCommandRejected: thread is archived".into(), count: 2, last_seen_at: ago(2 * 3_600_000) },
        ],
        latest_failures: vec![
            RecentFailure {
                name: "provider.codex.turn".into(),
                cause: "ProviderProcessExitedError: codex exited with code 1\n    at CodexAdapter.awaitExit (apps/server/src/provider/codex/CodexAdapter.ts:412:11)\n    at runTurn (apps/server/src/orchestration-v2/effects/runTurn.ts:88:5)".into(),
                duration_ms: 31_204.0,
                ended_at: ago(14 * 60_000),
                trace_id: trace_id(1),
                span_id: "a3ce929d0e0e4736".into(),
            },
            RecentFailure {
                name: "vcs.status".into(),
                cause: "GitCommandError: fatal: not a git repository (or any of the parent directories): .git".into(),
                duration_ms: 48.0,
                ended_at: ago(55 * 60_000),
                trace_id: trace_id(4),
                span_id: "d4e5f60718293a4b".into(),
            },
        ],
        logs: vec![
            LogEvent { span_name: "provider.codex.turn".into(), level: "Warning".into(), message: "Provider stream stalled for 30s; waiting for the next event.".into(), seen_at: ago(16 * 60_000), trace_id: trace_id(1), span_id: "a3ce929d0e0e4736".into() },
            LogEvent {
                span_name: "checkpoint.capture".into(),
                level: "Error".into(),
                message: "Checkpoint capture skipped: the working tree has 2,418 untracked files over the size budget. Add large build outputs to .gitignore or raise the checkpoint size limit in settings so turns can be restored.".into(),
                seen_at: ago(3 * 3_600_000),
                trace_id: trace_id(3),
                span_id: "c0ffee0123456789".into(),
            },
        ],
        partial_failure: false,
        error: None,
    };
    let fetched = now - 40_000;
    let ui = &mut s.ui.settings.diagnostics;
    *ui = Ui { env: Some(s.ui.env.active.clone()), last_tick: now, ..Ui::default() };
    ui.processes = Query {
        data: Some(Processes {
            server_pid: 48211,
            read_at: ago(40_000),
            process_count: processes.len() as i64,
            total_rss_bytes: total_rss,
            total_cpu_percent: total_cpu,
            processes,
            error: None,
        }),
        error: None,
        pending: None,
        fetched_ms: fetched,
    };
    ui.history = Query {
        data: Some(History {
            read_at: ago(40_000),
            sample_interval_ms: 2_000.0,
            retained_sample_count: 1_350,
            total_cpu_seconds: 485.25,
            buckets,
            top_processes,
            error: None,
        }),
        error: None,
        pending: None,
        fetched_ms: fetched,
    };
    ui.trace = Query { data: Some(trace), error: None, pending: None, fetched_ms: fetched };
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: &str = "2026-11-06T09:00:00.000Z";

    #[test]
    fn options_decode_in_every_wire_shape() {
        assert_eq!(option(&Value::Null), None);
        assert_eq!(option(&json!({"_tag": "None"})), None);
        assert_eq!(option(&json!({"_tag": "Some", "value": 3})), Some(&json!(3)));
        assert_eq!(option(&json!(true)), Some(&json!(true)));
        assert_eq!(option(&json!({"message": "m"})), Some(&json!({"message": "m"})));
        assert_eq!(option(&json!({})["missing"]), None);
    }

    #[test]
    fn trace_diagnostics_decode() {
        let v = json!({
            "traceFilePath": "/t/trace.ndjson", "scannedFilePaths": [], "readAt": AT,
            "recordCount": 12, "parseErrorCount": 1, "failureCount": 2, "interruptionCount": 0,
            "slowSpanThresholdMs": 1000, "slowSpanCount": 3, "logLevelCounts": {},
            "topSpansByCount": [{"name": "a", "count": 5, "failureCount": 1, "totalDurationMs": 50,
                "averageDurationMs": 10, "maxDurationMs": 20}],
            "slowestSpans": [{"name": "b", "durationMs": 1500, "endedAt": AT, "traceId": "t1", "spanId": "s1"}],
            "commonFailures": [{"name": "c", "cause": "boom", "count": 2, "lastSeenAt": AT, "traceId": "t2", "spanId": "s2"}],
            "latestFailures": [{"name": "d", "cause": "bang", "durationMs": 9, "endedAt": AT, "traceId": "t3", "spanId": "s3"}],
            "latestWarningAndErrorLogs": [{"spanName": "e", "level": "Warning", "message": "slow", "seenAt": AT,
                "traceId": "t4", "spanId": "s4"}],
            "partialFailure": {"_tag": "Some", "value": true},
            "error": {"_tag": "Some", "value": {"kind": "trace-file-read-failed", "message": "EACCES"}}
        });
        let trace = decode_trace(&v);
        let at = crate::data::iso_ms(AT);
        assert_eq!((trace.read_at, trace.record_count, trace.parse_error_count), (at, 12, 1));
        assert_eq!((trace.failure_count, trace.slow_span_count, trace.slow_span_threshold_ms), (2, 3, 1000.0));
        assert_eq!((trace.top_spans[0].name.as_str(), trace.top_spans[0].average_ms), ("a", 10.0));
        assert_eq!((trace.slowest[0].trace_id.as_str(), trace.slowest[0].ended_at), ("t1", at));
        assert_eq!((trace.common_failures[0].cause.as_str(), trace.common_failures[0].count), ("boom", 2));
        assert_eq!(trace.latest_failures[0].span_id, "s3");
        assert_eq!((trace.logs[0].level.as_str(), trace.logs[0].seen_at), ("Warning", at));
        assert!(trace.partial_failure);
        assert_eq!(trace.error.as_deref(), Some("EACCES"));

        let bare = decode_trace(&json!({"readAt": AT, "partialFailure": null, "error": {"_tag": "None"}}));
        assert!(!bare.partial_failure);
        assert_eq!(bare.error, None);
        assert!(bare.top_spans.is_empty() && bare.logs.is_empty());
        let unwrapped = decode_trace(&json!({"error": {"kind": "trace-file-not-found", "message": "missing"}}));
        assert_eq!(unwrapped.error.as_deref(), Some("missing"));
        assert!(!unwrapped.partial_failure, "an absent partialFailure is false");
    }

    #[test]
    fn process_diagnostics_decode() {
        let v = json!({
            "serverPid": 10, "readAt": AT, "processCount": 2, "totalRssBytes": 2048, "totalCpuPercent": 12.5,
            "processes": [
                {"pid": 11, "startTimeMs": 99, "ppid": 10, "pgid": {"_tag": "None"}, "status": "S", "cpuPercent": 10.0,
                 "rssBytes": 1024, "elapsed": "01:00", "command": "codex app-server", "depth": 0, "childPids": [12]},
                {"pid": 12, "startTimeMs": 100, "ppid": 11, "status": "R", "cpuPercent": 2.5,
                 "rssBytes": 1024, "elapsed": "00:10", "command": "rg x", "depth": 1, "childPids": []}
            ],
            "error": null
        });
        let d = decode_processes(&v);
        assert_eq!((d.server_pid, d.process_count, d.total_rss_bytes, d.total_cpu_percent), (10, 2, 2048.0, 12.5));
        assert_eq!(d.processes.len(), 2);
        assert_eq!((d.processes[0].pid, d.processes[0].start_time_ms, &d.processes[0].child_pids), (11, 99, &vec![12]));
        assert_eq!(d.processes[1].depth, 1);
        assert_eq!(d.error, None);
        let failed = decode_processes(&json!({"processes": [], "error": {"message": "ps timed out"}}));
        assert_eq!(failed.error.as_deref(), Some("ps timed out"));
    }

    #[test]
    fn resource_history_decode() {
        let v = json!({
            "readAt": AT, "windowMs": 900000, "bucketMs": 60000, "sampleIntervalMs": 0, "retainedSampleCount": 4,
            "totalCpuSecondsApprox": 1.5,
            "buckets": [{"startedAt": AT, "endedAt": AT, "avgCpuPercent": 1.0, "maxCpuPercent": 4.0, "maxRssBytes": 1,
                "maxProcessCount": 2}],
            "topProcesses": [{"processKey": "10:1", "pid": 10, "ppid": 1, "command": "node server", "depth": 0,
                "isServerRoot": true, "firstSeenAt": AT, "lastSeenAt": AT, "currentCpuPercent": 1.0,
                "avgCpuPercent": 0.5, "maxCpuPercent": 3.0, "cpuSecondsApprox": 0.25, "currentRssBytes": 5,
                "maxRssBytes": 6, "sampleCount": 4}],
            "error": {"_tag": "Some", "value": {"failureTag": "ProcessDiagnosticsQueryTimeoutError", "message": "timed out"}}
        });
        let h = decode_history(&v);
        assert_eq!((h.sample_interval_ms, h.retained_sample_count, h.total_cpu_seconds), (0.0, 4, 1.5));
        assert_eq!((h.buckets[0].avg_cpu_percent, h.buckets[0].max_cpu_percent), (1.0, 4.0));
        let p = &h.top_processes[0];
        assert_eq!((p.process_key.as_str(), p.is_server_root, p.cpu_seconds, p.max_rss_bytes), ("10:1", true, 0.25, 6.0));
        assert_eq!(h.error.as_deref(), Some("timed out"));
        assert_eq!(decode_history(&json!({})).error, None);
    }

    #[test]
    fn signal_results_decode_and_recognize_exited_processes() {
        let sent = decode_signal(&json!({"pid": 5, "signal": "SIGINT", "signaled": true}));
        assert!(sent.signaled && sent.message.is_none());
        let stale = decode_signal(&json!({"pid": 5, "signal": "SIGKILL", "signaled": false,
            "message": {"_tag": "Some", "value": "Process 5 is not a live descendant of the server."}}));
        assert!(!stale.signaled);
        assert!(is_stale_signal_message(stale.message.as_deref()));
        let bare = decode_signal(&json!({"signaled": false, "message": "Permission denied"}));
        assert_eq!(bare.message.as_deref(), Some("Permission denied"));
        assert!(!is_stale_signal_message(bare.message.as_deref()));
        assert!(!is_stale_signal_message(None));
    }

    #[test]
    fn numbers_read_like_web() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(18_432), "18,432");
        assert_eq!(format_count(1_234_567), "1,234,567");
        assert_eq!(format_duration(999.4), "999 ms");
        assert_eq!(format_duration(1_500.0), "1.50 s");
        assert_eq!(format_duration(2_000.0), "2 s");
        assert_eq!(format_duration(12_000.0), "12 s");
        assert_eq!(format_duration(12_340.0), "12.3 s");
        assert_eq!(format_cpu_time(5.0), "5s");
        assert_eq!(format_cpu_time(5.5), "5.50s");
        assert_eq!(format_cpu_time(12.34), "12.3s");
        assert_eq!(format_cpu_time(90.0), "1.50m");
        assert_eq!(format_cpu_time(7_200.0), "2h");
        assert_eq!(format_bytes(1023.4), "1023 B");
        assert_eq!(format_bytes(1536.0), "1.50 KB");
        assert_eq!(format_bytes(1024.0 * 1023.9), "1.00 MB");
        assert_eq!(format_bytes(412.0 * 1024.0 * 1024.0), "412 MB");
        assert_eq!(format_percent(61.66), "61.7%");
    }

    #[test]
    fn relative_times_floor_to_their_unit() {
        let at = crate::data::iso_ms(AT).unwrap();
        assert_eq!(relative(at, at + 59_000), ("just now".to_owned(), None));
        assert_eq!(relative(at, at + 5 * 60_000 + 59_000), ("5m".to_owned(), Some("ago")));
        assert_eq!(relative(at, at + 50 * 3_600_000), ("2d".to_owned(), Some("ago")));
        assert_eq!(relative_no_wrap(Some(at), at + 3 * 3_600_000), "3h\u{a0}ago");
        assert_eq!(relative_no_wrap(None, at), "");
    }

    #[test]
    fn process_names_and_types() {
        assert_eq!(format_process_name("/opt/homebrew/bin/codex app-server"), "codex");
        assert_eq!(format_process_name("\"C:\\Program Files\\node.exe\" x"), "Program");
        assert_eq!(format_process_name("'/usr/bin/node' server.js"), "node");
        assert_eq!(format_process_name("   "), "   ");
        let long = format!("/bin/{}", "a".repeat(50));
        assert_eq!(format_short_process_name(&long), format!("{}...", "a".repeat(39)));
        assert_eq!(shorten_trace_id("abc"), "abc");
        assert_eq!(shorten_trace_id(&"0123456789".repeat(4)), "012345678901234567...0123456789");
        let process = |depth: i64, command: &str| Process {
            pid: 1,
            start_time_ms: 0,
            cpu_percent: 0.0,
            rss_bytes: 0.0,
            command: command.to_owned(),
            depth,
            child_pids: vec![],
        };
        assert_eq!(format_process_type(&process(1, "codex")), "Subprocess");
        assert_eq!(format_process_type(&process(0, "/usr/local/bin/Claude --print")), "Agent");
        assert_eq!(format_process_type(&process(0, "opencode-server")), "Agent");
        assert_eq!(format_process_type(&process(0, "cursors")), "Process");
        assert_eq!(format_process_type(&process(0, "my_codex")), "Process");
    }

    #[test]
    fn collapsed_rows_hide_only_their_descendants() {
        let p = |pid: i64, depth: i64| Process {
            pid,
            start_time_ms: 0,
            cpu_percent: 0.0,
            rss_bytes: 0.0,
            command: String::new(),
            depth,
            child_pids: vec![],
        };
        let tree = vec![p(1, 0), p(2, 1), p(3, 2), p(4, 1), p(5, 0), p(6, 1)];
        let pids = |collapsed: &[i64]| {
            let set: HashSet<i64> = collapsed.iter().copied().collect();
            visible_process_tree(&tree, &set).iter().map(|p| p.pid).collect::<Vec<_>>()
        };
        assert_eq!(pids(&[]), vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(pids(&[2]), vec![1, 2, 4, 5, 6]);
        assert_eq!(pids(&[1]), vec![1, 5, 6]);
        assert_eq!(pids(&[1, 5]), vec![1, 5]);
    }

    #[test]
    fn history_children_indent_from_the_shallowest_child() {
        let p = |depth: i64, root: bool| HistoryProcess {
            process_key: String::new(),
            pid: 1,
            command: String::new(),
            depth,
            is_server_root: root,
            current_cpu_percent: 0.0,
            avg_cpu_percent: 0.0,
            max_cpu_percent: 0.0,
            cpu_seconds: 0.0,
            max_rss_bytes: 0.0,
        };
        assert_eq!(history_depths(&[p(0, true), p(3, false), p(4, false), p(3, false)]), vec![0, 1, 2, 1]);
        assert_eq!(history_depths(&[p(2, true)]), vec![0]);
        assert_eq!(bar_percent(0.0, 10.0), 2.0);
        assert_eq!(bar_percent(5.0, 10.0), 50.0);
    }

    #[test]
    fn windows_default_to_fifteen_minutes() {
        let ui = Ui::default();
        assert_eq!(history_input(&ui), json!({"windowMs": 900_000, "bucketMs": 60_000}));
        let ui = Ui { window: Some(3), ..Ui::default() };
        assert_eq!(history_input(&ui), json!({"windowMs": 3_600_000, "bucketMs": 300_000}));
    }

    #[test]
    fn queries_request_once_then_again_when_stale_on_return() {
        let mut q: Query<()> = Query::default();
        assert!(q.due(1_000, false) && q.initial_loading());
        q.pending = Some(1);
        assert!(!q.due(1_000, true) && q.initial_loading());
        q.pending = None;
        q.fetched_ms = 1_000;
        assert!(!q.due(1_000 + STALE_MS + 1, false));
        assert!(q.due(1_000 + STALE_MS + 1, true));
        assert!(!q.due(1_000 + STALE_MS, true));
        assert!(!q.initial_loading(), "a settled failure without data is no longer loading");
    }
}
