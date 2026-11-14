//! Web `ResourceTelemetryDiagnostics`: Resource monitor, Host & collection, Resource timeline,
//! Live process tree and Instrumented application I/O, at the top of Diagnostics. The snapshot
//! is the `subscribeResourceTelemetry` stream; history is a one-shot query for the selected
//! window. Signals and retries belong to one environment, so a switch starts fresh.

use super::controls as c;
use super::diagnostics::{
    Column, alert_line, at, column, dot, empty_rows, format_bytes, format_count, format_cpu_time, format_percent,
    format_process_name, inline_button, int, js_round, list, mono_cell, num, option, relative, stats, string, table,
    trim_zeros,
};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{Color, El, col, el, row, text};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

const SUBSCRIBE: &str = "subscribeResourceTelemetry";
const HISTORY: &str = "server.getResourceTelemetryHistory";
const RETRY: &str = "server.retryResourceTelemetry";
const SIGNAL: &str = "server.signalProcess";
const DIAGNOSTICS_READ: &str = "diagnostics:read";
const ENVIRONMENT_MAINTAIN: &str = "environment:maintain";
/// `formatEnvironmentQueryError` when the cause carries no message.
const QUERY_FALLBACK: &str = "The environment request failed.";
/// `HISTORY_WINDOWS`: label, window and bucket in ms.
const HISTORY_WINDOWS: [(&str, i64, i64); 4] = [
    ("5m", 5 * 60_000, 15_000),
    ("15m", 15 * 60_000, 30_000),
    ("30m", 30 * 60_000, 60_000),
    ("1h", 60 * 60_000, 2 * 60_000),
];
/// The history query's `staleTimeMs`: a page opened again refetches after this.
const HISTORY_STALE_MS: i64 = 5_000;
/// A tick after this long without one means the page was left and opened again.
const REENTRY_GAP_MS: i64 = 2_000;

/// A signal from its button press until the server answers; a SIGKILL waits for its
/// confirmation with no request yet.
struct Signal {
    key: String,
    pid: i64,
    start_time_ms: i64,
    signal: &'static str,
    request: Option<u64>,
}

#[derive(Default)]
pub struct Ui {
    /// The environment this state belongs to.
    env: Option<String>,
    snapshot: Option<Snapshot>,
    /// The `readAt` of the decoded snapshot, so an unchanged stream value is not decoded again.
    stamp: Option<String>,
    /// The subscription's failure; the last snapshot stays shown under it.
    error: Option<String>,
    history: Option<History>,
    history_error: Option<String>,
    history_pending: Option<u64>,
    /// When the last history reply settled; 0 before any.
    history_fetched_ms: i64,
    /// Index into `HISTORY_WINDOWS`; unset is web's 15 minutes.
    window: Option<usize>,
    /// Collapsed rows of the live tree, by `pid:startTimeMs`.
    collapsed: HashSet<String>,
    signals: Vec<Signal>,
    retry: Option<u64>,
    /// History replies to drop: requests for a window no longer selected.
    discard: Vec<u64>,
    last_tick: i64,
}

impl Ui {
    fn window_index(&self) -> usize {
        self.window.unwrap_or(1).min(HISTORY_WINDOWS.len() - 1)
    }

    fn signaling(&self, key: &str) -> bool {
        self.signals.iter().any(|sig| sig.key == key)
    }
}

// --- Results (contracts `resourceTelemetry.ts`) ---------------------------------------------

struct Snapshot {
    read_at: Option<i64>,
    sample_interval_ms: f64,
    processes: Vec<Proc>,
    backend: Aggregate,
    electron: Aggregate,
    monitor: Aggregate,
    all_t3: Aggregate,
    power: Power,
    speed_limit: Option<f64>,
    attribution: Vec<Attribution>,
    health: Health,
}

struct Proc {
    pid: i64,
    start_time_ms: i64,
    ppid: i64,
    child_pids: Vec<i64>,
    depth: i64,
    name: String,
    command: String,
    category: String,
    cpu_percent: f64,
    cpu_time_ms: f64,
    resident_bytes: f64,
    read_bytes: f64,
    write_bytes: f64,
    read_rate: f64,
    write_rate: f64,
    io_semantics: String,
}

impl Proc {
    /// `processIdentityKey`.
    fn key(&self) -> String {
        format!("{}:{}", self.pid, self.start_time_ms)
    }
}

#[derive(Default)]
struct Aggregate {
    process_count: i64,
    cpu_percent: f64,
    cpu_time_ms: f64,
    rss_bytes: f64,
    peak_rss_bytes: f64,
    read_bytes: f64,
    write_bytes: f64,
    read_rate: f64,
    write_rate: f64,
    starts: i64,
    exits: i64,
}

/// `HostPowerSnapshot`; the boolean states are `"true"`, `"false"` or `"unknown"`.
struct Power {
    idle: String,
    idle_seconds: Option<f64>,
    locked: String,
    suspended: bool,
    on_battery: String,
    low_power_mode: String,
    thermal_state: String,
}

struct SourceHealth {
    status: String,
    last_error: Option<String>,
}

struct Health {
    native: SourceHealth,
    desktop: SourceHealth,
    sidecar_version: Option<String>,
    sidecar_pid: Option<i64>,
    restart_count: i64,
    collection_micros: f64,
    scanned: i64,
    retained: i64,
    inaccessible: i64,
}

struct Attribution {
    component: String,
    operation: String,
    read_bytes: f64,
    write_bytes: f64,
    count: i64,
    duration_ms: f64,
}

struct History {
    buckets: Vec<Bucket>,
    top_processes: Vec<Summary>,
}

struct Bucket {
    avg_cpu: f64,
    max_cpu: f64,
    read_bytes: f64,
    write_bytes: f64,
}

struct Summary {
    pid: i64,
    name: String,
    command: String,
    category: String,
    cpu_time_ms: f64,
    max_cpu: f64,
    peak_rss_bytes: f64,
    read_bytes: f64,
    write_bytes: f64,
    sample_count: i64,
}

fn opt_string(v: &Value) -> Option<String> {
    option(v).and_then(Value::as_str).map(str::to_owned)
}

fn decode_aggregate(v: &Value) -> Aggregate {
    Aggregate {
        process_count: int(&v["processCount"]),
        cpu_percent: num(&v["currentCpuPercent"]),
        cpu_time_ms: num(&v["cpuTimeMs"]),
        rss_bytes: num(&v["currentRssBytes"]),
        peak_rss_bytes: num(&v["peakRssBytes"]),
        read_bytes: num(&v["ioReadBytes"]),
        write_bytes: num(&v["ioWriteBytes"]),
        read_rate: num(&v["ioReadBytesPerSecond"]),
        write_rate: num(&v["ioWriteBytesPerSecond"]),
        starts: int(&v["processStarts"]),
        exits: int(&v["processExits"]),
    }
}

fn decode_source(v: &Value) -> SourceHealth {
    SourceHealth { status: string(&v["status"]), last_error: opt_string(&v["lastError"]) }
}

fn decode_health(v: &Value) -> Health {
    Health {
        native: decode_source(&v["native"]),
        desktop: decode_source(&v["desktop"]),
        sidecar_version: opt_string(&v["sidecarVersion"]),
        sidecar_pid: option(&v["sidecarPid"]).map(int),
        restart_count: int(&v["restartCount"]),
        collection_micros: num(&v["collectionDurationMicros"]),
        scanned: int(&v["scannedProcessCount"]),
        retained: int(&v["retainedProcessCount"]),
        inaccessible: int(&v["inaccessibleProcessCount"]),
    }
}

fn decode_process(p: &Value) -> Proc {
    Proc {
        pid: int(&p["identity"]["pid"]),
        start_time_ms: int(&p["identity"]["startTimeMs"]),
        ppid: int(&p["ppid"]),
        child_pids: list(&p["childPids"]).map(int).collect(),
        depth: int(&p["depth"]),
        name: string(&p["name"]),
        command: string(&p["command"]),
        category: string(&p["category"]),
        cpu_percent: num(&p["cpuPercent"]),
        cpu_time_ms: num(&p["cpuTimeMs"]),
        resident_bytes: num(&p["residentBytes"]),
        read_bytes: num(&p["ioReadBytes"]),
        write_bytes: num(&p["ioWriteBytes"]),
        read_rate: num(&p["ioReadBytesPerSecond"]),
        write_rate: num(&p["ioWriteBytesPerSecond"]),
        io_semantics: string(&p["ioSemantics"]),
    }
}

fn decode_snapshot(v: &Value) -> Snapshot {
    let groups = &v["groups"];
    let power = &v["power"];
    Snapshot {
        read_at: at(&v["readAt"]),
        sample_interval_ms: num(&v["sampleIntervalMs"]),
        processes: list(&v["processes"]).map(decode_process).collect(),
        backend: decode_aggregate(&groups["backend"]),
        electron: decode_aggregate(&groups["electron"]),
        monitor: decode_aggregate(&groups["monitor"]),
        all_t3: decode_aggregate(&groups["allT3"]),
        power: Power {
            idle: string(&power["idle"]),
            idle_seconds: option(&power["idleSeconds"]).and_then(Value::as_f64),
            locked: string(&power["locked"]),
            suspended: power["suspended"] == true,
            on_battery: string(&power["onBattery"]),
            low_power_mode: string(&power["lowPowerMode"]),
            thermal_state: string(&power["thermalState"]),
        },
        speed_limit: option(&v["speedLimitPercent"]).and_then(Value::as_f64),
        attribution: list(&v["attribution"]["entries"])
            .map(|e| Attribution {
                component: string(&e["component"]),
                operation: string(&e["operation"]),
                read_bytes: num(&e["logicalReadBytes"]),
                write_bytes: num(&e["logicalWriteBytes"]),
                count: int(&e["count"]),
                duration_ms: num(&e["durationMs"]),
            })
            .collect(),
        health: decode_health(&v["health"]),
    }
}

fn decode_history(v: &Value) -> History {
    History {
        buckets: list(&v["buckets"])
            .map(|b| Bucket {
                avg_cpu: num(&b["avgCpuPercent"]),
                max_cpu: num(&b["maxCpuPercent"]),
                read_bytes: num(&b["ioReadBytes"]),
                write_bytes: num(&b["ioWriteBytes"]),
            })
            .collect(),
        top_processes: list(&v["topProcesses"])
            .map(|p| Summary {
                pid: int(&p["identity"]["pid"]),
                name: string(&p["name"]),
                command: string(&p["command"]),
                category: string(&p["category"]),
                cpu_time_ms: num(&p["cpuTimeMs"]),
                max_cpu: num(&p["maxCpuPercent"]),
                peak_rss_bytes: num(&p["peakRssBytes"]),
                read_bytes: num(&p["ioReadBytes"]),
                write_bytes: num(&p["ioWriteBytes"]),
                sample_count: int(&p["sampleCount"]),
            })
            .collect(),
    }
}

/// What the stream's latest value changes: the net crate stores a failed subscription as
/// `{"error": exit}` in place of the snapshot.
enum Change {
    Failed(String),
    Sampled(String, Snapshot),
}

fn latest_change(v: &Value, stamp: Option<&str>, error: Option<&str>) -> Option<Change> {
    if let Some(cause) = v.get("error") {
        let message = crate::shell::palette::error_message(cause, QUERY_FALLBACK);
        if error == Some(message.as_str()) {
            return None;
        }
        return Some(Change::Failed(message));
    }
    let read_at = v["readAt"].as_str().unwrap_or("");
    if stamp == Some(read_at) && error.is_none() {
        return None;
    }
    Some(Change::Sampled(read_at.to_owned(), decode_snapshot(v)))
}

/// The failed signal's toast description; `None` when the process was signaled.
fn signal_failure(reply: &Result<Value, Value>, signal: &str, pid: i64) -> Option<String> {
    match reply {
        Err(error) => Some(crate::shell::palette::error_message(error, &format!("Failed to send {signal}."))),
        Ok(result) if result["signaled"] == true => None,
        Ok(result) => {
            Some(opt_string(&result["message"]).unwrap_or_else(|| format!("Failed to send {signal} to process {pid}.")))
        }
    }
}

// --- Derived values -------------------------------------------------------------------------

fn format_rate(value: f64) -> String {
    format!("{}/s", format_bytes(value))
}

fn format_duration_micros(value: f64) -> String {
    if value < 1_000.0 {
        return format!("{} µs", js_round(value));
    }
    if value < 1_000_000.0 {
        return format!("{} ms", trim_zeros(format!("{:.2}", value / 1_000.0)));
    }
    format!("{} s", trim_zeros(format!("{:.2}", value / 1_000_000.0)))
}

/// `toLocaleString(undefined, { maximumFractionDigits: 1 })` in en-US, for values ≥ 0.
fn format_locale_tenths(value: f64) -> String {
    let tenths = js_round(value * 10.0);
    match tenths % 10 {
        0 => format_count(tenths / 10),
        fraction => format!("{}.{fraction}", format_count(tenths / 10)),
    }
}

fn format_sample_interval(ms: f64) -> String {
    if ms < 1_000.0 {
        return format!("{} ms", js_round(ms).max(0));
    }
    let seconds = ms / 1_000.0;
    format!("{} {}", format_locale_tenths(seconds), if seconds == 1.0 { "second" } else { "seconds" })
}

/// `formatProcessName`: the reported name, else the executable's file name.
fn process_name(p: &Proc) -> String {
    if p.name.trim().is_empty() { format_process_name(&p.command) } else { p.name.clone() }
}

fn category_label(category: &str) -> &str {
    match category {
        "server" => "Server",
        "server-child" => "Backend child",
        "provider-root" => "Provider",
        "terminal-root" => "Terminal",
        "electron-main" => "Electron main",
        "electron-renderer" => "Renderer",
        "electron-gpu" => "GPU",
        "electron-utility" => "Electron utility",
        "resource-monitor" => "Monitor",
        "unknown-t3" => "T3 process",
        other => other,
    }
}

fn category_color(category: &str, t: &T) -> Color {
    if category == "resource-monitor" {
        t.c.warning
    } else if category.starts_with("electron-") {
        t.c.info
    } else if category == "server" {
        theme::tw::VIOLET_500
    } else {
        t.c.success
    }
}

fn io_semantics_label(semantics: &str) -> &'static str {
    match semantics {
        "storage" => "Storage bytes",
        "logical" => "Logical bytes",
        "all-io" => "All I/O bytes",
        _ => "Unavailable",
    }
}

fn boolean_label(value: &str, yes: &'static str, no: &'static str) -> &'static str {
    match value {
        "true" => yes,
        "false" => no,
        _ => "Unknown",
    }
}

fn can_signal(category: &str) -> bool {
    matches!(category, "server-child" | "provider-root" | "terminal-root")
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tone {
    Neutral,
    Default,
    Warning,
    Danger,
}

fn source_tone(status: &str) -> Tone {
    match status {
        "healthy" => Tone::Default,
        "starting" | "degraded" => Tone::Warning,
        _ => Tone::Danger,
    }
}

/// `shouldShowResourceMonitorRetry`.
fn needs_retry(native_status: Option<&str>, has_error: bool) -> bool {
    match native_status {
        None => has_error,
        Some(status) => matches!(status, "degraded" | "unavailable" | "stopped"),
    }
}

/// The browser build's monitor reports itself unavailable in web mode; that is expected there.
fn desktop_only(health: &SourceHealth) -> bool {
    health.status == "unavailable" && health.last_error.as_deref().is_some_and(|e| e.contains("'web' mode"))
}

fn has_host_power_signal(power: &Power) -> bool {
    [&power.on_battery, &power.low_power_mode, &power.idle, &power.locked, &power.thermal_state]
        .iter()
        .any(|v| v.as_str() != "unknown")
}

/// `resourceHistoryBarHeight`, in percent of the chart.
fn bar_height(value: f64, max: f64, minimum: f64) -> f64 {
    if value <= 0.0 {
        return 0.0;
    }
    (value / max.max(1.0) * 100.0).max(minimum)
}

/// `resourceHistoryCpuScaleMax`.
fn cpu_scale_max(buckets: &[Bucket]) -> f64 {
    buckets.iter().map(|b| b.avg_cpu).fold(1.0, f64::max)
}

fn io_scale_max(buckets: &[Bucket]) -> f64 {
    buckets.iter().map(|b| b.read_bytes + b.write_bytes).fold(1.0, f64::max)
}

/// `visibleResourceTelemetryProcesses`: descendants of collapsed rows hide by parentage, so
/// rows need not arrive depth-first.
fn visible_processes<'a>(processes: &'a [Proc], collapsed: &HashSet<String>) -> Vec<&'a Proc> {
    let mut children: HashMap<i64, Vec<&'a Proc>> = HashMap::new();
    for p in processes {
        children.entry(p.ppid).or_default().push(p);
    }
    let mut hidden: HashSet<String> = HashSet::new();
    let mut stack: Vec<i64> = processes.iter().filter(|p| collapsed.contains(&p.key())).map(|p| p.pid).collect();
    while let Some(pid) = stack.pop() {
        for child in children.get(&pid).into_iter().flatten() {
            if hidden.insert(child.key()) {
                stack.push(child.pid);
            }
        }
    }
    processes.iter().filter(|p| !hidden.contains(&p.key())).collect()
}

fn history_input(window: usize) -> Value {
    let (_, window_ms, bucket_ms) = HISTORY_WINDOWS[window.min(HISTORY_WINDOWS.len() - 1)];
    json!({"windowMs": window_ms, "bucketMs": bucket_ms})
}

// --- Requests -------------------------------------------------------------------------------

/// The telemetry stream while Diagnostics is open and this connection may read diagnostics.
pub fn wanted(s: &App) -> Vec<(String, Value)> {
    let open = matches!(&s.route, crate::Route::Settings(p) if p == "diagnostics");
    if open && super::permissions::can(s, DIAGNOSTICS_READ) {
        vec![(SUBSCRIBE.to_owned(), json!({}))]
    } else {
        Vec::new()
    }
}

/// Folds the stream's latest snapshot, requests history when missing or stale, and settles
/// replies; true when anything shown changed.
pub fn tick(s: &mut App) -> bool {
    let now = s.now_ms;
    let env = s.ui.env.active.clone();
    let mut changed = false;
    if s.ui.settings.resource_telemetry.env.as_deref() != Some(env.as_str()) {
        s.ui.settings.resource_telemetry = Ui { env: Some(env), ..Ui::default() };
        // Idempotent; covers a switch to an environment the subscription diff has not reached.
        if let Some(net) = s.src.net() {
            net.watch_resource(SUBSCRIBE, json!({}));
        }
        changed = true;
    }
    let latest = {
        let ui = &s.ui.settings.resource_telemetry;
        super::resource(s, SUBSCRIBE, json!({})).and_then(|v| latest_change(v, ui.stamp.as_deref(), ui.error.as_deref()))
    };
    match latest {
        Some(Change::Failed(message)) => {
            s.ui.settings.resource_telemetry.error = Some(message);
            changed = true;
        }
        Some(Change::Sampled(stamp, snapshot)) => {
            let ui = &mut s.ui.settings.resource_telemetry;
            ui.stamp = Some(stamp);
            ui.snapshot = Some(snapshot);
            ui.error = None;
            changed = true;
        }
        None => {}
    }
    let Some(net) = s.src.net() else { return changed };
    let ui = &mut s.ui.settings.resource_telemetry;
    let reentered = now - ui.last_tick > REENTRY_GAP_MS;
    ui.last_tick = now;
    for id in std::mem::take(&mut ui.discard) {
        if net.take_reply(id).is_none() {
            ui.discard.push(id);
        }
    }
    let stale = reentered && now - ui.history_fetched_ms > HISTORY_STALE_MS;
    if ui.history_pending.is_none() && (ui.history_fetched_ms == 0 || stale) {
        ui.history_pending = Some(net.request(HISTORY, history_input(ui.window_index())));
        changed = true;
    }
    if let Some(id) = ui.history_pending
        && let Some(reply) = net.take_reply(id)
    {
        ui.history_pending = None;
        ui.history_fetched_ms = now.max(1);
        match reply {
            Ok(v) => {
                ui.history = Some(decode_history(&v));
                ui.history_error = None;
            }
            Err(e) => ui.history_error = Some(crate::shell::palette::error_message(&e, QUERY_FALLBACK)),
        }
        changed = true;
    }
    let mut finished: Vec<(Signal, Result<Value, Value>)> = Vec::new();
    let mut i = 0;
    while i < ui.signals.len() {
        match ui.signals[i].request.and_then(|id| net.take_reply(id)) {
            Some(reply) => finished.push((ui.signals.remove(i), reply)),
            None => i += 1,
        }
    }
    let retry_reply = ui.retry.and_then(|id| net.take_reply(id));
    if retry_reply.is_some() {
        ui.retry = None;
        changed = true;
    }
    if let Some(Ok(_)) = &retry_reply {
        net.unwatch_resource(SUBSCRIBE, json!({}));
        net.watch_resource(SUBSCRIBE, json!({}));
        if ui.history_pending.is_none() {
            ui.history_pending = Some(net.request(HISTORY, history_input(ui.window_index())));
        }
    }
    changed |= !finished.is_empty();
    for (sig, reply) in finished {
        if let Some(detail) = signal_failure(&reply, sig.signal, sig.pid) {
            crate::shell::palette::toast_error_brief(s, &format!("Could not send {}", sig.signal), Some(&detail));
        }
    }
    if let Some(Err(error)) = retry_reply {
        let detail = crate::shell::palette::error_message(&error, "The resource monitor retry failed.");
        crate::shell::palette::toast_error_brief(s, "Could not restart resource monitor", Some(&detail));
    }
    changed
}

/// Web refreshes the subscription atom: the stream restarts and sends a fresh snapshot.
fn refresh_snapshot(s: &mut App) {
    if let Some(net) = s.src.net() {
        net.unwatch_resource(SUBSCRIBE, json!({}));
        net.watch_resource(SUBSCRIBE, json!({}));
    }
}

fn refresh_history(s: &mut App) {
    let Some(net) = s.src.net() else { return };
    let ui = &mut s.ui.settings.resource_telemetry;
    if ui.history_pending.is_none() {
        ui.history_pending = Some(net.request(HISTORY, history_input(ui.window_index())));
    }
}

fn pick_window(s: &mut App, index: usize) {
    let ui = &mut s.ui.settings.resource_telemetry;
    if ui.window_index() == index {
        return;
    }
    ui.window = Some(index);
    if let Some(id) = ui.history_pending.take() {
        ui.discard.push(id);
    }
    ui.history = None;
    ui.history_error = None;
    ui.history_fetched_ms = 0;
    refresh_history(s);
}

fn retry_collector(s: &mut App) {
    let allowed = super::permissions::can(s, ENVIRONMENT_MAINTAIN) && super::permissions::can(s, DIAGNOSTICS_READ);
    if !allowed || s.ui.settings.resource_telemetry.retry.is_some() {
        return;
    }
    let id = s.src.net().map(|net| net.request(RETRY, json!({})));
    s.ui.settings.resource_telemetry.retry = id;
}

fn toggle_collapsed(s: &mut App, key: &str) {
    let collapsed = &mut s.ui.settings.resource_telemetry.collapsed;
    if !collapsed.remove(key) {
        collapsed.insert(key.to_owned());
    }
}

/// `signalProcess`: a SIGKILL first opens the confirmation (`overlays`); SIGINT goes at once.
fn begin_signal(s: &mut App, pid: i64, start_time_ms: i64, signal: &'static str) {
    if !super::permissions::can(s, ENVIRONMENT_MAINTAIN) {
        return;
    }
    let key = format!("{pid}:{start_time_ms}");
    let ui = &mut s.ui.settings.resource_telemetry;
    if ui.signaling(&key) {
        return;
    }
    ui.signals.push(Signal { key: key.clone(), pid, start_time_ms, signal, request: None });
    if signal != "SIGKILL" {
        send_signal(s, &key);
    }
}

/// The server rejects a pid whose start time no longer matches, so a process that exited
/// during confirmation is never signaled by mistake.
fn send_signal(s: &mut App, key: &str) {
    let allowed = super::permissions::can(s, ENVIRONMENT_MAINTAIN);
    let payload = s
        .ui
        .settings
        .telemetry
        .signals
        .iter()
        .find(|sig| sig.key == key && sig.request.is_none())
        .filter(|_| allowed)
        .map(|sig| json!({"pid": sig.pid, "startTimeMs": sig.start_time_ms, "signal": sig.signal}));
    let id = match payload {
        Some(payload) => s.src.net().map(|net| net.request(SIGNAL, payload)),
        None => None,
    };
    let signals = &mut s.ui.settings.resource_telemetry.signals;
    match id {
        Some(id) => {
            if let Some(sig) = signals.iter_mut().find(|sig| sig.key == key) {
                sig.request = Some(id);
            }
        }
        None => signals.retain(|sig| sig.key != key || sig.request.is_some()),
    }
}

fn cancel_confirm(s: &mut App) {
    s.ui.settings.resource_telemetry.signals.retain(|sig| sig.request.is_some());
}

// --- View -----------------------------------------------------------------------------------

/// `text-3xs font-semibold uppercase tracking-widest`.
fn label_3xs() -> rune::TextStyle {
    rune::TextStyle { tracking: 1.0, ..theme::weight(theme::TEXT_3XS, theme::SEMIBOLD) }
}

fn caps(label: &str, alpha: f32, t: &T) -> El {
    text(label.to_uppercase()).font(label_3xs()).color(t.c.muted_foreground.alpha_mul(alpha))
}

fn hrule(t: &T) -> El {
    el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.6))
}

fn vrule(t: &T) -> El {
    el().w(1.0).self_stretch().fill(t.c.border.alpha_mul(0.6))
}

/// `SettingsSection` with its title icon; the body is one card child with its own rules.
fn section(title: &str, glyph: &'static rune::Icon, actions: Vec<El>, body: El, t: &T) -> El {
    col()
        .w_full()
        .gap(10.0)
        .child(
            row()
                .w_full()
                .min_h(28.0)
                .px(16.0)
                .gap(16.0)
                .center_y()
                .child(
                    row()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(8.0)
                        .center_y()
                        .child(rune::icon(glyph, 16.0).color(t.c.muted_foreground))
                        .child(
                            text(title.to_owned())
                                .font(theme::weight(theme::TEXT_BASE, theme::SEMIBOLD))
                                .color(t.c.foreground)
                                .truncate()
                                .shrink()
                                .min_w(0.0),
                        ),
                )
                .child(row().gap(8.0).center_y().children(actions)),
        )
        .child(c::card(vec![body], t))
}

/// `SourceStatusBadge`; `desktop_only` is the neutral "Desktop only" presentation.
fn status_badge(prefix: &str, status: &str, desktop_only: bool, t: &T) -> El {
    let tone = if desktop_only { Tone::Neutral } else { source_tone(status) };
    let shown = if desktop_only { "Desktop only" } else { status };
    let (border, fill, fg, mark) = match tone {
        Tone::Neutral => (
            t.c.border.alpha_mul(0.7),
            t.c.muted.alpha_mul(0.45),
            t.c.muted_foreground,
            t.c.muted_foreground.alpha_mul(0.55),
        ),
        Tone::Default => (t.c.success.alpha_mul(0.25), t.c.success.alpha_mul(0.1), t.c.success_foreground, t.c.success),
        Tone::Warning => (t.c.warning.alpha_mul(0.3), t.c.warning.alpha_mul(0.1), t.c.warning_foreground, t.c.warning),
        Tone::Danger => {
            (t.c.destructive.alpha_mul(0.3), t.c.destructive.alpha_mul(0.1), t.c.destructive, t.c.destructive)
        }
    };
    let label = format!("{prefix} {shown}").trim().to_uppercase();
    row()
        .gap(6.0)
        .center_y()
        .px(10.0)
        .py(4.0)
        .radius(theme::radius::FULL)
        .border(1.0, border)
        .fill(fill)
        .child(dot(mark))
        .child(text(label).font(label_3xs()).color(fg))
}

/// `LastSampleLabel`.
fn last_sample(read_at: Option<i64>, now: i64, t: &T) -> El {
    let Some(at) = read_at else {
        return text("Waiting for sample").font(theme::TEXT_2XS).color(t.c.muted_foreground.alpha_mul(0.55));
    };
    let color = t.c.muted_foreground.alpha_mul(0.6);
    let (value, suffix) = relative(at, now);
    let mut label = row()
        .gap(4.0)
        .center_y()
        .child(text("Updated").font(theme::TEXT_2XS).color(color))
        .child(text(value).font(theme::mono(theme::TEXT_2XS)).color(color));
    if let Some(suffix) = suffix {
        label = label.child(text(suffix).font(theme::TEXT_2XS).color(color));
    }
    label
}

/// A `RefreshIcon` button: the glyph turns into the busy spinner while pending.
fn refresh_button(label: &'static str, pending: bool, t: &T, f: fn(&mut App)) -> El {
    crate::kit::Button::new(label)
        .variant("ghost")
        .size("icon-micro")
        .icon(if pending { &icons::LOADER_CIRCLE } else { &icons::REFRESH_CW })
        .disabled(pending)
        .on_press(f)
        .build(t)
}

/// `IconStat`.
fn icon_stat(glyph: &'static rune::Icon, label: &str, value: String, detail: Option<String>, tone: Tone, t: &T) -> El {
    let color = match tone {
        Tone::Warning => t.c.warning_foreground,
        Tone::Danger => t.c.destructive,
        Tone::Neutral | Tone::Default => t.c.foreground,
    };
    let mut block = col()
        .grow(1.0)
        .w(0.0)
        .min_w(0.0)
        .px(20.0)
        .py(16.0)
        .child(
            row()
                .min_w(0.0)
                .gap(8.0)
                .center_y()
                .child(rune::icon(glyph, 14.0).color(t.c.muted_foreground.alpha_mul(0.55)))
                .child(caps(label, 0.7, t).truncate().shrink().min_w(0.0)),
        )
        .child(
            text(value)
                .font(theme::weight(theme::mono(theme::TEXT_2XL), theme::SEMIBOLD))
                .color(color)
                .truncate()
                .mt(10.0),
        );
    if let Some(detail) = detail {
        block = block.child(
            text(detail).font(theme::TEXT_3XS).color(t.c.muted_foreground.alpha_mul(0.6)).truncate().mt(6.0),
        );
    }
    block
}

fn metric_pair(label: &str, value: String, t: &T) -> El {
    col()
        .grow(1.0)
        .w(0.0)
        .min_w(0.0)
        .child(caps(label, 0.45, t))
        .child(text(value).font(theme::weight(theme::MONO_XS, theme::MEDIUM)).color(t.c.foreground.alpha_mul(0.9)).truncate())
}

/// `AggregateCard`.
fn aggregate_card(label: &str, accent: Color, a: &Aggregate, t: &T) -> El {
    let count = format!("{} {}", a.process_count, if a.process_count == 1 { "process" } else { "processes" });
    col()
        .grow(1.0)
        .w(0.0)
        .min_w(0.0)
        .px(20.0)
        .pb(16.0)
        .child(el().h(2.0).w_full().radius(1.0).fill(accent.alpha_mul(0.75)))
        .child(
            row()
                .w_full()
                .mt(14.0)
                .gap(12.0)
                .center_y()
                .justify_between()
                .child(caps(label, 0.75, t))
                .child(
                    text(count)
                        .font(theme::mono(theme::TEXT_3XS))
                        .color(t.c.muted_foreground.alpha_mul(0.7))
                        .px(6.0)
                        .py(2.0)
                        .radius(6.0)
                        .fill(t.c.muted.alpha_mul(0.55)),
                ),
        )
        .child(
            row()
                .w_full()
                .mt(14.0)
                .gap(16.0)
                .child(metric_pair("CPU", format_percent(a.cpu_percent), t))
                .child(metric_pair("Memory", format_bytes(a.rss_bytes), t)),
        )
        .child(
            row()
                .w_full()
                .mt(10.0)
                .gap(16.0)
                .child(metric_pair("Read", format_rate(a.read_rate), t))
                .child(metric_pair("Write", format_rate(a.write_rate), t)),
        )
}

fn error_strip(message: String, t: &T) -> El {
    row().w_full().px(20.0).py(12.0).fill(t.c.destructive.alpha_mul(0.05)).child(alert_line(message, t.c.destructive))
}

/// Rows with web's `border-t ... first:border-t-0` between them.
fn ruled(items: Vec<El>, t: &T) -> El {
    let mut out = col().w_full();
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            out = out.child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.5)));
        }
        out = out.child(item);
    }
    out
}

/// `DetailRow`.
fn detail_row(label: &str, value: String, color: Option<Color>, t: &T) -> El {
    row()
        .w_full()
        .py(10.0)
        .gap(16.0)
        .center_y()
        .justify_between()
        .child(text(label.to_owned()).font(theme::TEXT_2XS).color(t.c.muted_foreground.alpha_mul(0.75)))
        .child(
            text(value)
                .font(theme::mono(theme::TEXT_2XS))
                .color(color.unwrap_or(t.c.foreground.alpha_mul(0.85)))
                .truncate()
                .shrink()
                .min_w(0.0),
        )
}

/// `HealthSource`.
fn health_source(label: &str, health: &SourceHealth, t: &T) -> El {
    let desktop_only = desktop_only(health);
    let note = if desktop_only {
        "Available when this page runs inside the desktop app.".to_owned()
    } else {
        health.last_error.clone().unwrap_or_else(|| "No reported errors".to_owned())
    };
    row()
        .w_full()
        .py(12.0)
        .gap(16.0)
        .items_start()
        .justify_between()
        .child(
            col().grow(1.0).min_w(0.0).child(text(label.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground)).child(
                text(note)
                    .font(theme::TEXT_2XS)
                    .color(t.c.muted_foreground.alpha_mul(0.65))
                    .wrap()
                    .selectable(true)
                    .mt(4.0),
            ),
        )
        .child(status_badge("", &health.status, desktop_only, t))
}

fn half_header(glyph: &'static rune::Icon, label: &str, t: &T) -> El {
    row()
        .gap(8.0)
        .center_y()
        .mb(12.0)
        .child(
            el().size(24.0, 24.0)
                .center()
                .radius(6.0)
                .fill(t.c.muted.alpha_mul(0.6))
                .child(rune::icon(glyph, 14.0).color(t.c.muted_foreground.alpha_mul(0.7))),
        )
        .child(caps(label, 0.7, t))
}

pub fn view(s: &App, t: &T) -> El {
    let blank;
    let ui = if s.ui.settings.resource_telemetry.env.as_deref() == Some(s.ui.env.active.as_str()) {
        &s.ui.settings.resource_telemetry
    } else {
        blank = Ui::default();
        &blank
    };
    // Web `isPending`: the stream has not delivered since it (re)started. Fixtures have none.
    let pending =
        matches!(s.src, crate::data::Source::Live(_)) && super::resource(s, SUBSCRIBE, json!({})).is_none();
    col()
        .w_full()
        .gap(40.0)
        .child(monitor_section(s, ui, pending, t))
        .child(host_section(ui, t))
        .child(timeline_section(ui, t))
        .child(tree_section(s, ui, t))
        .child(attribution_section(ui, t))
}

fn monitor_section(s: &App, ui: &Ui, pending: bool, t: &T) -> El {
    let snapshot = ui.snapshot.as_ref();
    let all = snapshot.map(|d| &d.all_t3);
    let dots = || "...".to_owned();
    let write_tone = match all.map(|a| a.write_rate) {
        Some(rate) if rate >= 10.0 * 1_024.0 * 1_024.0 => Tone::Danger,
        Some(rate) if rate >= 1_024.0 * 1_024.0 => Tone::Warning,
        _ => Tone::Default,
    };
    let speed_limit = snapshot.and_then(|d| d.speed_limit);
    let strip = row()
        .w_full()
        .px(20.0)
        .py(16.0)
        .gap(12.0)
        .center_y()
        .justify_between()
        .fill(t.c.muted.alpha_mul(0.3))
        .child(
            col().grow(1.0).min_w(0.0).max_w(576.0).child(caps("T3 system footprint", 0.7, t)).child(
                text("Live native counters for the server, providers, terminals, desktop processes, and the monitor itself.")
                    .font(theme::TEXT_XS)
                    .color(t.c.muted_foreground)
                    .wrap()
                    .mt(4.0),
            ),
        )
        .child(row().gap(8.0).center_y().child(dot(t.c.success)).child(
            text(format!("Sampling every {}", snapshot.map_or_else(dots, |d| format_sample_interval(d.sample_interval_ms))))
                .font(theme::TEXT_3XS)
                .color(t.c.muted_foreground.alpha_mul(0.65)),
        ));
    let first = vec![
        icon_stat(
            &icons::CPU,
            "Current CPU",
            all.map_or_else(dots, |a| format_percent(a.cpu_percent)),
            all.map(|a| format!("{} observed CPU time", format_cpu_time(a.cpu_time_ms / 1_000.0))),
            Tone::Default,
            t,
        ),
        icon_stat(
            &icons::MEMORY_STICK,
            "Resident memory",
            all.map_or_else(dots, |a| format_bytes(a.rss_bytes)),
            all.map(|a| format!("{} combined process peaks", format_bytes(a.peak_rss_bytes))),
            Tone::Default,
            t,
        ),
        icon_stat(
            &icons::ACTIVITY,
            "Process count",
            all.map_or_else(dots, |a| a.process_count.to_string()),
            all.map(|a| format!("{} starts · {} exits", a.starts, a.exits)),
            Tone::Default,
            t,
        ),
    ];
    let second = vec![
        icon_stat(
            &icons::HARD_DRIVE,
            "Read throughput",
            all.map_or_else(dots, |a| format_rate(a.read_rate)),
            all.map(|a| format!("{} observed", format_bytes(a.read_bytes))),
            Tone::Default,
            t,
        ),
        icon_stat(
            &icons::DATABASE,
            "Write throughput",
            all.map_or_else(dots, |a| format_rate(a.write_rate)),
            all.map(|a| format!("{} observed", format_bytes(a.write_bytes))),
            write_tone,
            t,
        ),
        icon_stat(
            &icons::GAUGE,
            "CPU speed limit",
            match (snapshot, speed_limit) {
                (None, _) => dots(),
                (Some(_), None) => "Unknown".to_owned(),
                (Some(_), Some(limit)) => format!("{limit:.0}%"),
            },
            snapshot.map(|d| format!("{} thermal state", d.power.thermal_state)),
            if speed_limit.is_some_and(|limit| limit < 80.0) { Tone::Warning } else { Tone::Default },
            t,
        ),
    ];
    let mut body = col().w_full().child(strip).child(hrule(t)).child(stats(first, t)).child(hrule(t)).child(stats(second, t));
    if let Some(error) = ui.error.clone() {
        body = body.child(el().h(1.0).w_full().fill(t.c.destructive.alpha_mul(0.2))).child(error_strip(error, t));
    }
    if let Some(d) = snapshot {
        let cards = row()
            .w_full()
            .items_stretch()
            .fill(t.c.muted.alpha_mul(0.1))
            .child(aggregate_card("Backend + agents", t.c.success.alpha_mul(0.8), &d.backend, t))
            .child(vrule(t))
            .child(aggregate_card("Desktop", t.c.info.alpha_mul(0.8), &d.electron, t))
            .child(vrule(t))
            .child(aggregate_card("Monitor overhead", t.c.warning.alpha_mul(0.8), &d.monitor, t));
        body = body.child(hrule(t)).child(cards);
    }
    let mut actions = Vec::new();
    if let Some(d) = snapshot {
        actions.push(status_badge("Native", &d.health.native.status, false, t));
    }
    actions.push(last_sample(snapshot.and_then(|d| d.read_at), s.now_ms, t));
    actions.push(refresh_button("Refresh resource telemetry", pending, t, refresh_snapshot).help("Refresh telemetry snapshot"));
    section("Resource monitor", &icons::ACTIVITY, actions, body, t)
}

fn host_section(ui: &Ui, t: &T) -> El {
    let snapshot = ui.snapshot.as_ref();
    let mut host = col().grow(1.0).w(0.0).min_w(0.0).px(20.0).py(16.0).child(half_header(&icons::BATTERY, "Host state", t));
    host = match snapshot.filter(|d| has_host_power_signal(&d.power)) {
        Some(d) => {
            let p = &d.power;
            let idle = match p.idle_seconds {
                Some(seconds) => format!("{} · {}s", boolean_label(&p.idle, "Idle", "Active"), js_round(seconds)),
                None => boolean_label(&p.idle, "Idle", "Active").to_owned(),
            };
            let session = if p.suspended { "Suspended" } else { boolean_label(&p.locked, "Locked", "Unlocked") };
            let thermal = matches!(p.thermal_state.as_str(), "serious" | "critical").then_some(t.c.destructive);
            host.child(ruled(
                vec![
                    detail_row("Power source", boolean_label(&p.on_battery, "Battery", "External power").to_owned(), None, t),
                    detail_row("Low power mode", boolean_label(&p.low_power_mode, "Enabled", "Disabled").to_owned(), None, t),
                    detail_row("Idle", idle, None, t),
                    detail_row("Session", session.to_owned(), None, t),
                    detail_row("Thermal", p.thermal_state.clone(), thermal, t),
                ],
                t,
            ))
        }
        None => host.child(
            col()
                .w_full()
                .px(16.0)
                .py(20.0)
                .radius(12.0)
                .border(1.0, t.c.border.alpha_mul(0.7))
                .fill(t.c.muted.alpha_mul(0.2))
                .child(text("Desktop host signals not connected").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(
                    text("Power, idle, lock, and thermal state are supplied by the desktop host. Process telemetry remains fully active in this browser session.")
                        .font(theme::TEXT_2XS)
                        .color(t.c.muted_foreground.alpha_mul(0.7))
                        .wrap()
                        .max_w(384.0)
                        .mt(6.0),
                ),
        ),
    };
    let mut collection =
        col().grow(1.0).w(0.0).min_w(0.0).px(20.0).py(16.0).child(half_header(&icons::GAUGE, "Collection health", t));
    collection = match snapshot {
        Some(d) => {
            let h = &d.health;
            let sidecar = match &h.sidecar_version {
                None => "Unavailable".to_owned(),
                Some(version) => match h.sidecar_pid {
                    Some(pid) => format!("{version} · PID {pid}"),
                    None => version.clone(),
                },
            };
            collection.child(ruled(
                vec![
                    health_source("Native process monitor", &h.native, t),
                    health_source("Electron main process", &h.desktop, t),
                    detail_row("Collection time", format_duration_micros(h.collection_micros), None, t),
                    detail_row("Process scan", format!("{}/{} retained", h.retained, h.scanned), None, t),
                    detail_row(
                        "Inaccessible",
                        h.inaccessible.to_string(),
                        (h.inaccessible > 0).then_some(t.c.warning_foreground),
                        t,
                    ),
                    detail_row("Sidecar", sidecar, None, t),
                    detail_row("Restarts", h.restart_count.to_string(), None, t),
                ],
                t,
            ))
        }
        None => collection.child(
            text("Waiting for collector health.").font(theme::TEXT_XS).color(t.c.muted_foreground).py(16.0),
        ),
    };
    let body = row().w_full().items_stretch().child(host).child(vrule(t)).child(collection);
    let mut actions = Vec::new();
    if needs_retry(snapshot.map(|d| d.health.native.status.as_str()), ui.error.is_some()) {
        let retrying = ui.retry.is_some();
        actions.push(
            crate::kit::Button::new("Retry monitor")
                .variant("outline")
                .size("xs")
                .icon(if retrying { &icons::LOADER_CIRCLE } else { &icons::REFRESH_CW })
                .disabled(retrying)
                .on_press(retry_collector)
                .build(t),
        );
    }
    section("Host & collection", &icons::GAUGE, actions, body, t)
}

fn timeline_section(ui: &Ui, t: &T) -> El {
    let history = ui.history.as_ref();
    let mut body = col().w_full();
    if let Some(error) = ui.history_error.clone() {
        body = body.child(error_strip(error, t)).child(el().h(1.0).w_full().fill(t.c.destructive.alpha_mul(0.2)));
    }
    body = body
        .child(chart(history.map(|h| h.buckets.as_slice()).unwrap_or_default(), t))
        .child(hrule(t))
        .child(history_table(history.map(|h| h.top_processes.as_slice()).unwrap_or_default(), t));
    let window = crate::kit::ToggleGroup::new(&HISTORY_WINDOWS.map(|w| w.0), &[ui.window_index()])
        .on_pick(|s: &mut App, i: usize, _: &mut rune::Tx| pick_window(s, i))
        .build(&crate::kit::theme(t));
    let actions = vec![
        row().role("group").label("Resource history period").child(window),
        refresh_button("Refresh resource history", ui.history_pending.is_some(), t, refresh_history),
    ];
    section("Resource timeline", &icons::HARD_DRIVE, actions, body, t)
}

/// `ResourceHistoryChart`: per bucket, average CPU beside I/O reads and writes.
fn chart(buckets: &[Bucket], t: &T) -> El {
    // `h-32` less its border and `pt-3 pb-2`.
    const INNER: f64 = 106.0;
    let cpu_max = cpu_scale_max(buckets);
    let io_max = io_scale_max(buckets);
    let swatch = |color: Color, label: &'static str| {
        row()
            .gap(6.0)
            .center_y()
            .child(el().size(12.0, 6.0).radius(3.0).fill(color))
            .child(text(label).font(theme::TEXT_3XS).color(t.c.muted_foreground.alpha_mul(0.65)))
    };
    let legend = row()
        .w_full()
        .flex_wrap()
        .gap(16.0)
        .center_y()
        .mb(12.0)
        .child(swatch(t.c.foreground.alpha_mul(0.7), "CPU average"))
        .child(swatch(t.c.info.alpha_mul(0.7), "I/O reads"))
        .child(swatch(t.c.warning.alpha_mul(0.8), "I/O writes"));
    let mut bars = row()
        .w_full()
        .h(128.0)
        .px(8.0)
        .pt(12.0)
        .pb(8.0)
        .gap(4.0)
        .items_end()
        .radius(8.0)
        .border(1.0, t.c.border.alpha_mul(0.4))
        .fill(t.c.muted.alpha_mul(0.08));
    let bar = |percent: f64, color: Color| {
        el().grow(1.0).w(0.0).h((percent / 100.0 * INNER) as f32).corners([2.0, 2.0, 0.0, 0.0]).fill(color)
    };
    for b in buckets {
        bars = bars.child(
            row()
                .grow(1.0)
                .w(0.0)
                .min_w(4.0)
                .h_full()
                .gap(1.0)
                .items_end()
                .help(format!(
                    "CPU avg {:.1}%\nCPU peak {:.1}%\nRead {}\nWrite {}",
                    b.avg_cpu,
                    b.max_cpu,
                    format_bytes(b.read_bytes),
                    format_bytes(b.write_bytes)
                ))
                .child(bar(bar_height(b.avg_cpu, cpu_max, 2.0), t.c.foreground.alpha_mul(0.65)))
                .child(bar(bar_height(b.read_bytes, io_max, 1.0), t.c.info.alpha_mul(0.7)))
                .child(bar(bar_height(b.write_bytes, io_max, 1.0), t.c.warning.alpha_mul(0.8))),
        );
    }
    col().w_full().px(20.0).py(16.0).child(legend).child(bars)
}

const HISTORY_COLUMNS: [Column; 9] = [
    column("Process", 24.0, false),
    column("Category", 11.0, false),
    column("CPU Time", 10.0, true),
    column("Peak CPU", 10.0, true),
    column("Peak Mem", 11.0, true),
    column("Read", 11.0, true),
    column("Write", 11.0, true),
    column("Samples", 7.0, true),
    column("PID", 5.0, true),
];

/// `HistoryProcessTable`.
fn history_table(processes: &[Summary], t: &T) -> El {
    let muted = t.c.muted_foreground;
    let rows: Vec<Vec<El>> = processes
        .iter()
        .map(|p| {
            let name = if p.name.is_empty() { p.command.clone() } else { p.name.clone() };
            let tip = if p.command.is_empty() { p.name.clone() } else { p.command.clone() };
            vec![
                text(name).font(theme::XS_MEDIUM).color(t.c.foreground).truncate().min_w(0.0).help(tip),
                text(category_label(&p.category).to_owned()).font(theme::TEXT_2XS).color(muted).truncate().min_w(0.0),
                mono_cell(format_cpu_time(p.cpu_time_ms / 1_000.0), t.c.foreground),
                mono_cell(format_percent(p.max_cpu), t.c.foreground),
                mono_cell(format_bytes(p.peak_rss_bytes), t.c.foreground),
                mono_cell(format_bytes(p.read_bytes), t.c.info_foreground),
                mono_cell(format_bytes(p.write_bytes), t.c.warning_foreground),
                mono_cell(p.sample_count.to_string(), muted),
                mono_cell(p.pid.to_string(), muted),
            ]
        })
        .collect();
    let empty = rows.is_empty();
    let table = table(t, 1020.0, 12.0, false, true, &HISTORY_COLUMNS, rows);
    if !empty {
        return table;
    }
    col().w_full().child(table).child(empty_rows("No retained process samples in this window.", t))
}

const PROCESS_COLUMNS: [Column; 11] = [
    column("Process", 20.0, false),
    column("Category", 10.0, false),
    column("CPU", 7.0, true),
    column("CPU Time", 8.0, true),
    column("Memory", 9.0, true),
    column("Read/s", 9.0, true),
    column("Write/s", 9.0, true),
    column("Read Total", 10.0, true),
    column("Write Total", 8.0, true),
    column("PID", 6.0, true),
    Column { header: "Kill", weight: 4.0, end: true, pad: Some(8.0) },
];

fn tree_section(s: &App, ui: &Ui, t: &T) -> El {
    let mut actions = Vec::new();
    if ui.snapshot.is_some() {
        let color = t.c.muted_foreground.alpha_mul(0.55);
        actions.push(
            row()
                .gap(4.0)
                .center_y()
                .child(text("Identity:").font(theme::TEXT_3XS).color(color))
                .child(text("PID + start time").font(theme::mono(theme::TEXT_3XS)).color(color)),
        );
    }
    section("Live process tree", &icons::CPU, actions, process_table(s, ui, t), t)
}

/// `ProcessTable`.
fn process_table(s: &App, ui: &Ui, t: &T) -> El {
    let processes = ui.snapshot.as_ref().map(|d| d.processes.as_slice()).unwrap_or_default();
    let visible = visible_processes(processes, &ui.collapsed);
    let can_maintain = super::permissions::can(s, ENVIRONMENT_MAINTAIN);
    let muted = t.c.muted_foreground;
    let rows: Vec<Vec<El>> = visible
        .iter()
        .map(|p| {
            let key = p.key();
            let name = process_name(p);
            let collapsed = ui.collapsed.contains(&key);
            let toggle = if p.child_pids.is_empty() {
                el().size(20.0, 20.0)
            } else {
                let toggled = key.clone();
                crate::kit::Button::new(if collapsed { format!("Expand {name}") } else { format!("Collapse {name}") })
                    .variant("ghost-muted")
                    .size("icon-micro")
                    .icon(if collapsed { &icons::CHEVRON_RIGHT } else { &icons::CHEVRON_DOWN })
                    .on_press(move |s: &mut App| toggle_collapsed(s, &toggled))
                    .build(t)
            };
            let tip = if p.command.is_empty() { p.name.clone() } else { p.command.clone() };
            let name_el = row()
                .w_full()
                .min_w(0.0)
                .pl(p.depth.min(7) as f32 * 10.0)
                .gap(8.0)
                .center_y()
                .child(toggle)
                .child(dot(category_color(&p.category, t)))
                .child(text(name).font(theme::XS_MEDIUM).color(t.c.foreground).truncate().shrink().min_w(0.0).help(tip));
            let actions = if can_signal(&p.category) {
                let enabled = can_maintain && !ui.signaling(&key);
                let (pid, start) = (p.pid, p.start_time_ms);
                row()
                    .gap(6.0)
                    .center_y()
                    .child(
                        inline_button("INT", muted, enabled, t, move |s: &mut App| begin_signal(s, pid, start, "SIGINT"))
                            .label("Send SIGINT")
                            .help("Send SIGINT"),
                    )
                    .child(
                        inline_button("KILL", t.c.destructive.alpha_mul(0.8), enabled, t, move |s: &mut App| {
                            begin_signal(s, pid, start, "SIGKILL")
                        })
                        .label("Send SIGKILL")
                        .help("Send SIGKILL"),
                    )
            } else {
                text("-").font(theme::TEXT_3XS).color(muted.alpha_mul(0.35))
            };
            vec![
                name_el,
                text(category_label(&p.category).to_owned()).font(theme::TEXT_2XS).color(muted).truncate().min_w(0.0),
                mono_cell(format_percent(p.cpu_percent), t.c.foreground),
                mono_cell(format_cpu_time(p.cpu_time_ms / 1_000.0), t.c.foreground),
                mono_cell(format_bytes(p.resident_bytes), t.c.foreground),
                mono_cell(format_rate(p.read_rate), t.c.info_foreground),
                mono_cell(format_rate(p.write_rate), t.c.warning_foreground),
                mono_cell(format_bytes(p.read_bytes), muted),
                mono_cell(format_bytes(p.write_bytes), muted).help(io_semantics_label(&p.io_semantics)),
                mono_cell(p.pid.to_string(), muted),
                actions,
            ]
        })
        .collect();
    let empty = rows.is_empty();
    let table = table(t, 1320.0, 12.0, false, true, &PROCESS_COLUMNS, rows);
    if !empty {
        return table;
    }
    col().w_full().child(table).child(empty_rows("Waiting for the native process monitor.", t))
}

const ATTRIBUTION_COLUMNS: [Column; 6] = [
    column("Component", 22.0, false),
    column("Operation", 28.0, false),
    column("Logical Read", 14.0, true),
    column("Logical Write", 14.0, true),
    column("Count", 10.0, true),
    column("Time", 12.0, true),
];

fn attribution_section(ui: &Ui, t: &T) -> El {
    let entries = ui.snapshot.as_ref().map(|d| d.attribution.as_slice()).unwrap_or_default();
    let rows: Vec<Vec<El>> = entries
        .iter()
        .map(|e| {
            vec![
                text(e.component.clone()).font(theme::XS_MEDIUM).color(t.c.foreground).truncate().min_w(0.0),
                text(e.operation.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().min_w(0.0),
                mono_cell(format_bytes(e.read_bytes), t.c.info_foreground),
                mono_cell(format_bytes(e.write_bytes), t.c.warning_foreground),
                mono_cell(e.count.to_string(), t.c.foreground),
                mono_cell(format!("{}s", trim_zeros(format!("{:.2}", e.duration_ms / 1_000.0))), t.c.muted_foreground),
            ]
        })
        .collect();
    let empty = rows.is_empty();
    let mut table_el = table(t, 720.0, 12.0, false, true, &ATTRIBUTION_COLUMNS, rows);
    if empty {
        table_el = col()
            .w_full()
            .child(table_el)
            .child(empty_rows("No instrumented application I/O has been recorded yet.", t));
    }
    let body = col()
        .w_full()
        .child(
            text("Native counters identify which process is reading or writing. These application-level counters identify known T3 operations so process spikes can be correlated with specific persistence and logging paths.")
                .font(theme::TEXT_2XS)
                .color(t.c.muted_foreground)
                .wrap()
                .w_full()
                .px(20.0)
                .py(12.0)
                .fill(t.c.muted.alpha_mul(0.15)),
        )
        .child(hrule(t))
        .child(table_el);
    let actions = vec![
        text("Logical bytes by operation").font(theme::TEXT_3XS).color(t.c.muted_foreground.alpha_mul(0.55)),
    ];
    section("Instrumented application I/O", &icons::DATABASE, actions, body, t)
}

/// The SIGKILL confirmation, as web's destructive `dialogs.confirm` splits its message.
pub fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    let ui = &s.ui.settings.resource_telemetry;
    if ui.env.as_deref() != Some(s.ui.env.active.as_str()) {
        return;
    }
    let Some(sig) = ui.signals.iter().find(|sig| sig.signal == "SIGKILL" && sig.request.is_none()) else { return };
    let key = sig.key.clone();
    let cancel = crate::kit::Button::new("Cancel").variant("outline").on_press(cancel_confirm).build(t);
    let confirm = crate::kit::Button::new("Confirm")
        .variant("destructive")
        .on_press(move |s: &mut App| send_signal(s, &key))
        .build(t);
    crate::kit::Dialog::new("telemetry-sigkill", format!("Send SIGKILL to process {}?", sig.pid))
        .alert()
        .width(448.0)
        .open(true)
        .description("This cannot be handled by the process.")
        .child(row().w_full().gap(8.0).justify_end().child(cancel).child(confirm))
        .on_close(cancel_confirm)
        .build(t, layers);
}

// --- Fixture --------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn fixture_process(
    now: i64,
    pid: i64,
    ppid: i64,
    depth: i64,
    category: &str,
    name: &str,
    command: &str,
    children: &[i64],
    cpu: f64,
    mb: f64,
    read_kb_s: f64,
    write_kb_s: f64,
) -> Proc {
    Proc {
        pid,
        start_time_ms: now - 3_600_000 + pid,
        ppid,
        child_pids: children.to_vec(),
        depth,
        name: name.to_owned(),
        command: command.to_owned(),
        category: category.to_owned(),
        cpu_percent: cpu,
        cpu_time_ms: cpu * 36_000.0,
        resident_bytes: mb * 1024.0 * 1024.0,
        read_bytes: read_kb_s * 1024.0 * 900.0,
        write_bytes: write_kb_s * 1024.0 * 900.0,
        read_rate: read_kb_s * 1024.0,
        write_rate: write_kb_s * 1024.0,
        io_semantics: if category.starts_with("electron-") { "unavailable".into() } else { "storage".into() },
    }
}

fn fixture_aggregate(processes: &[Proc], include: impl Fn(&str) -> bool) -> Aggregate {
    let mut a = Aggregate::default();
    for p in processes.iter().filter(|p| include(p.category.as_str())) {
        a.process_count += 1;
        a.cpu_percent += p.cpu_percent;
        a.cpu_time_ms += p.cpu_time_ms;
        a.rss_bytes += p.resident_bytes;
        a.peak_rss_bytes += p.resident_bytes * 1.25;
        a.read_bytes += p.read_bytes;
        a.write_bytes += p.write_bytes;
        a.read_rate += p.read_rate;
        a.write_rate += p.write_rate;
    }
    a.starts = a.process_count + 4;
    a.exits = 4;
    a
}

/// A populated snapshot and 15 minutes of history for headless scenes: the server with a
/// provider tree, a terminal, the monitor sidecar and the desktop processes.
pub fn fixture(s: &mut App) {
    let now = s.now_ms;
    let server = "/Applications/T3 Code.app/Contents/Resources/node dist/bin.mjs serve";
    let processes = vec![
        fixture_process(now, 48211, 48000, 0, "server", "node", server, &[48302, 48420, 48501, 48600], 4.2, 318.0, 12.0, 96.0),
        fixture_process(now, 48302, 48211, 1, "provider-root", "codex", "/opt/homebrew/bin/codex app-server --listen stdio://", &[48310], 12.4, 412.0, 3.0, 22.0),
        fixture_process(now, 48310, 48302, 2, "server-child", "zsh", "/bin/zsh -lc vp run test", &[48390], 0.6, 9.5, 0.0, 0.0),
        fixture_process(now, 48390, 48310, 3, "server-child", "node", "node /Users/dev/t3code/node_modules/.bin/vitest run --reporter=dot", &[], 61.7, 288.0, 840.0, 1_460.0),
        fixture_process(now, 48420, 48211, 1, "provider-root", "claude", "claude --output-format stream-json --verbose --input-format stream-json", &[], 3.1, 233.0, 1.5, 6.0),
        fixture_process(now, 48501, 48211, 1, "terminal-root", "zsh", "/bin/zsh -l", &[], 0.0, 6.4, 0.0, 0.0),
        fixture_process(now, 48600, 48211, 1, "resource-monitor", "t3-resource-monitor", "t3-resource-monitor --stdio", &[], 0.4, 7.8, 0.0, 0.2),
        fixture_process(now, 48000, 1, 0, "electron-main", "T3 Code", "/Applications/T3 Code.app/Contents/MacOS/T3 Code", &[48010, 48020], 1.8, 182.0, 0.0, 0.0),
        fixture_process(now, 48010, 48000, 1, "electron-renderer", "T3 Code Helper (Renderer)", "T3 Code Helper (Renderer) --type=renderer", &[], 6.9, 356.0, 0.0, 0.0),
        fixture_process(now, 48020, 48000, 1, "electron-gpu", "T3 Code Helper (GPU)", "T3 Code Helper (GPU) --type=gpu-process", &[], 2.3, 141.0, 0.0, 0.0),
    ];
    let backend = fixture_aggregate(&processes, |c| matches!(c, "server" | "server-child" | "provider-root" | "terminal-root"));
    let electron = fixture_aggregate(&processes, |c| c.starts_with("electron-"));
    let monitor = fixture_aggregate(&processes, |c| c == "resource-monitor");
    let all_t3 = fixture_aggregate(&processes, |_| true);
    let attribution = [
        ("orchestration", "event-sink.commit", 0.0, 18_400_000.0, 4_812, 2_310.0),
        ("projection", "thread-snapshot.write", 2_200_000.0, 9_850_000.0, 1_204, 860.0),
        ("checkpoint", "git.write-tree", 41_000_000.0, 6_300_000.0, 211, 14_820.0),
        ("logging", "trace.append", 0.0, 3_900_000.0, 18_432, 412.0),
    ]
    .into_iter()
    .map(|(component, operation, read, write, count, ms)| Attribution {
        component: component.to_owned(),
        operation: operation.to_owned(),
        read_bytes: read,
        write_bytes: write,
        count,
        duration_ms: ms,
    })
    .collect();
    let snapshot = Snapshot {
        read_at: Some(now - 2_000),
        sample_interval_ms: 2_000.0,
        processes,
        backend,
        electron,
        monitor,
        all_t3,
        power: Power {
            idle: "false".into(),
            idle_seconds: Some(4.0),
            locked: "false".into(),
            suspended: false,
            on_battery: "false".into(),
            low_power_mode: "false".into(),
            thermal_state: "nominal".into(),
        },
        speed_limit: Some(100.0),
        attribution,
        health: Health {
            native: SourceHealth { status: "healthy".into(), last_error: None },
            desktop: SourceHealth { status: "healthy".into(), last_error: None },
            sidecar_version: Some("0.4.2".into()),
            sidecar_pid: Some(48600),
            restart_count: 0,
            collection_micros: 1_840.0,
            scanned: 612,
            retained: 10,
            inaccessible: 3,
        },
    };
    let buckets = (0..30)
        .map(|i| {
            let avg = 18.0 + 14.0 * (i as f64 / 3.0).sin().abs() + (i % 7) as f64 * 2.0;
            Bucket {
                avg_cpu: avg,
                max_cpu: avg * 1.7,
                read_bytes: if i % 4 == 0 { 0.0 } else { 2_400_000.0 * (1.0 + (i % 5) as f64) },
                write_bytes: 6_100_000.0 * (1.0 + (i as f64 / 4.0).cos().abs() * 3.0),
            }
        })
        .collect();
    let summary = |pid: i64, name: &str, command: &str, category: &str, seconds: f64, peak: f64, mb: f64, read_mb: f64, write_mb: f64| Summary {
        pid,
        name: name.to_owned(),
        command: command.to_owned(),
        category: category.to_owned(),
        cpu_time_ms: seconds * 1_000.0,
        max_cpu: peak,
        peak_rss_bytes: mb * 1024.0 * 1024.0,
        read_bytes: read_mb * 1024.0 * 1024.0,
        write_bytes: write_mb * 1024.0 * 1024.0,
        sample_count: 450,
    };
    let top_processes = vec![
        summary(48390, "node", "node /Users/dev/t3code/node_modules/.bin/vitest run --reporter=dot", "server-child", 241.0, 118.4, 402.0, 612.0, 1_180.0),
        summary(48302, "codex", "/opt/homebrew/bin/codex app-server --listen stdio://", "provider-root", 61.2, 38.9, 455.0, 4.2, 19.6),
        summary(48010, "T3 Code Helper (Renderer)", "T3 Code Helper (Renderer) --type=renderer", "electron-renderer", 52.7, 24.1, 371.0, 0.0, 0.0),
        summary(48211, "node", server, "server", 38.4, 11.2, 340.0, 9.8, 74.3),
        summary(48420, "claude", "claude --output-format stream-json --verbose", "provider-root", 8.75, 9.6, 240.0, 1.1, 5.2),
        summary(48600, "t3-resource-monitor", "t3-resource-monitor --stdio", "resource-monitor", 3.6, 1.2, 8.1, 0.0, 0.2),
    ];
    let ui = &mut s.ui.settings.resource_telemetry;
    *ui = Ui { env: Some(s.ui.env.active.clone()), last_tick: now, ..Ui::default() };
    ui.snapshot = Some(snapshot);
    ui.history = Some(History { buckets, top_processes });
    ui.history_fetched_ms = now;
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: &str = "2026-11-06T09:00:00.000Z";

    fn aggregate(n: i64) -> Value {
        json!({"processCount": n, "currentCpuPercent": 12.5, "cpuTimeMs": 90_000, "currentRssBytes": 2048,
            "peakRssBytes": 4096, "ioReadBytes": 10, "ioWriteBytes": 20, "ioReadBytesPerSecond": 1.5,
            "ioWriteBytesPerSecond": 2.5, "processStarts": 7, "processExits": 3})
    }

    fn snapshot_json(speed_limit: Value, last_error: Value, sidecar_pid: Value, idle_seconds: Value) -> Value {
        json!({
            "readAt": AT,
            "sampleIntervalMs": 2000,
            "processes": [{
                "identity": {"pid": 11, "startTimeMs": 99}, "ppid": 10, "childPids": [12, 13], "depth": 1,
                "name": "codex", "command": "/bin/codex app-server", "status": "S", "category": "provider-root",
                "cpuPercent": 3.25, "cpuTimeMs": 1500, "residentBytes": 1024, "peakResidentBytes": 2048,
                "virtualBytes": 1, "ioReadBytes": 100, "ioWriteBytes": 200, "ioReadBytesPerSecond": 10.0,
                "ioWriteBytesPerSecond": 20.0, "ioSemantics": "all-io", "runTimeMs": 5, "firstSeenAt": AT, "lastSeenAt": AT
            }],
            "groups": {"backend": aggregate(1), "electron": aggregate(2), "monitor": aggregate(3), "allT3": aggregate(6)},
            "power": {"source": "electron-main", "idle": "true", "idleSeconds": idle_seconds, "locked": "unknown",
                "suspended": true, "onBattery": "false", "lowPowerMode": "unknown", "thermalState": "serious",
                "stale": false, "updatedAt": AT},
            "speedLimitPercent": speed_limit,
            "attribution": {"readAt": AT, "entries": [{"component": "projection", "operation": "write",
                "logicalReadBytes": 5, "logicalWriteBytes": 6, "count": 7, "durationMs": 1234}]},
            "health": {
                "native": {"status": "degraded", "lastSampleAt": {"_tag": "None"}, "lastError": last_error},
                "desktop": {"status": "healthy", "lastSampleAt": AT, "lastError": null},
                "sidecarVersion": "0.4.2", "sidecarPid": sidecar_pid, "restartCount": 2,
                "collectionDurationMicros": 1840, "scannedProcessCount": 600, "retainedProcessCount": 10,
                "inaccessibleProcessCount": 1
            }
        })
    }

    #[test]
    fn snapshot_decodes_with_tagged_options() {
        let v = snapshot_json(
            json!({"_tag": "Some", "value": 72.0}),
            json!({"_tag": "Some", "value": "sidecar exited"}),
            json!({"_tag": "None"}),
            json!(12.4),
        );
        let d = decode_snapshot(&v);
        assert_eq!((d.read_at, d.sample_interval_ms), (crate::data::iso_ms(AT), 2000.0));
        let p = &d.processes[0];
        assert_eq!((p.pid, p.start_time_ms, p.ppid, p.depth, &p.child_pids), (11, 99, 10, 1, &vec![12, 13]));
        assert_eq!((p.category.as_str(), p.io_semantics.as_str(), p.key()), ("provider-root", "all-io", "11:99".to_owned()));
        assert_eq!((p.cpu_percent, p.cpu_time_ms, p.read_rate, p.write_bytes), (3.25, 1500.0, 10.0, 200.0));
        assert_eq!((d.backend.process_count, d.electron.process_count, d.monitor.process_count), (1, 2, 3));
        let a = &d.all_t3;
        assert_eq!((a.process_count, a.cpu_time_ms, a.peak_rss_bytes, a.starts, a.exits), (6, 90_000.0, 4096.0, 7, 3));
        assert_eq!((a.read_rate, a.write_rate), (1.5, 2.5));
        assert_eq!((d.power.idle.as_str(), d.power.idle_seconds, d.power.suspended), ("true", Some(12.4), true));
        assert_eq!(d.power.thermal_state, "serious");
        assert_eq!(d.speed_limit, Some(72.0));
        assert_eq!(d.health.native.last_error.as_deref(), Some("sidecar exited"));
        assert_eq!(d.health.desktop.last_error, None);
        assert_eq!((d.health.sidecar_version.as_deref(), d.health.sidecar_pid), (Some("0.4.2"), None));
        assert_eq!((d.health.restart_count, d.health.scanned, d.health.retained, d.health.inaccessible), (2, 600, 10, 1));
        let e = &d.attribution[0];
        assert_eq!((e.component.as_str(), e.count, e.duration_ms, e.write_bytes), ("projection", 7, 1234.0, 6.0));
    }

    #[test]
    fn snapshot_decodes_with_bare_and_absent_options() {
        let bare = decode_snapshot(&snapshot_json(json!(55.5), json!("boom"), json!(4321), Value::Null));
        assert_eq!(bare.speed_limit, Some(55.5));
        assert_eq!(bare.health.native.last_error.as_deref(), Some("boom"));
        assert_eq!(bare.health.sidecar_pid, Some(4321));
        assert_eq!(bare.power.idle_seconds, None);
        let none = decode_snapshot(&snapshot_json(Value::Null, json!({"_tag": "None"}), Value::Null, Value::Null));
        assert_eq!((none.speed_limit, none.health.native.last_error, none.health.sidecar_pid), (None, None, None));
        let empty = decode_snapshot(&json!({}));
        assert!(empty.processes.is_empty() && empty.attribution.is_empty() && empty.read_at.is_none());
        assert_eq!(empty.health.sidecar_version, None);
    }

    #[test]
    fn history_decodes() {
        let v = json!({
            "readAt": AT, "windowMs": 900000, "bucketMs": 30000, "sampleIntervalMs": 2000, "retainedSampleCount": 4,
            "buckets": [{"startedAt": AT, "endedAt": AT, "avgCpuPercent": 1.5, "maxCpuPercent": 4.0, "maxRssBytes": 1,
                "ioReadBytes": 10, "ioWriteBytes": 20, "maxProcessCount": 2}],
            "topProcesses": [{"identity": {"pid": 10, "startTimeMs": 1}, "ppid": 1, "depth": 0, "name": "",
                "command": "node server", "category": "server", "firstSeenAt": AT, "lastSeenAt": AT,
                "currentCpuPercent": 1.0, "avgCpuPercent": 0.5, "maxCpuPercent": 3.0, "cpuTimeMs": 250,
                "currentRssBytes": 5, "peakRssBytes": 6, "ioReadBytes": 7, "ioWriteBytes": 8,
                "ioSemantics": "storage", "sampleCount": 4}],
            "health": {}
        });
        let h = decode_history(&v);
        let b = &h.buckets[0];
        assert_eq!((b.avg_cpu, b.max_cpu, b.read_bytes, b.write_bytes), (1.5, 4.0, 10.0, 20.0));
        let p = &h.top_processes[0];
        assert_eq!((p.pid, p.name.as_str(), p.command.as_str(), p.category.as_str()), (10, "", "node server", "server"));
        assert_eq!((p.cpu_time_ms, p.max_cpu, p.peak_rss_bytes, p.sample_count), (250.0, 3.0, 6.0, 4));
        assert!(decode_history(&json!({})).buckets.is_empty());
    }

    #[test]
    fn stream_values_change_only_on_a_new_sample_or_failure() {
        let sample = json!({"readAt": AT});
        assert!(matches!(latest_change(&sample, None, None), Some(Change::Sampled(stamp, _)) if stamp == AT));
        assert!(latest_change(&sample, Some(AT), None).is_none());
        assert!(
            matches!(latest_change(&sample, Some(AT), Some("old")), Some(Change::Sampled(..))),
            "a sample after a failure clears it"
        );
        let failed = json!({"error": {"_tag": "Failure", "cause": [{"_tag": "Fail", "error": {"_tag": "EnvironmentAuthorizationError", "message": "Denied"}}]}});
        assert!(matches!(latest_change(&failed, Some(AT), None), Some(Change::Failed(m)) if m == "Denied"));
        assert!(latest_change(&failed, Some(AT), Some("Denied")).is_none());
        let bare = json!({"error": {"_tag": "Failure", "cause": []}});
        assert!(matches!(latest_change(&bare, None, None), Some(Change::Failed(m)) if m == QUERY_FALLBACK));
    }

    #[test]
    fn signal_failures_read_like_web() {
        assert_eq!(signal_failure(&Ok(json!({"pid": 5, "signal": "SIGINT", "signaled": true})), "SIGINT", 5), None);
        assert_eq!(
            signal_failure(&Ok(json!({"signaled": false, "message": {"_tag": "Some", "value": "Permission denied"}})), "SIGKILL", 5),
            Some("Permission denied".to_owned())
        );
        assert_eq!(
            signal_failure(&Ok(json!({"signaled": false, "message": {"_tag": "None"}})), "SIGKILL", 5),
            Some("Failed to send SIGKILL to process 5.".to_owned())
        );
        assert_eq!(signal_failure(&Err(json!([])), "SIGINT", 5), Some("Failed to send SIGINT.".to_owned()));
    }

    #[test]
    fn collapsed_rows_hide_descendants_by_parentage() {
        let p = |pid: i64, ppid: i64, depth: i64| Proc {
            pid,
            start_time_ms: pid * 1_000,
            ppid,
            child_pids: vec![],
            depth,
            name: String::new(),
            command: String::new(),
            category: "server-child".into(),
            cpu_percent: 0.0,
            cpu_time_ms: 0.0,
            resident_bytes: 0.0,
            read_bytes: 0.0,
            write_bytes: 0.0,
            read_rate: 0.0,
            write_rate: 0.0,
            io_semantics: String::new(),
        };
        // Not depth-first, as the web logic test.
        let tree = vec![p(1, 0, 0), p(10, 0, 0), p(2, 1, 1), p(11, 10, 1), p(3, 2, 2)];
        let pids = |collapsed: &[&str]| {
            let set: HashSet<String> = collapsed.iter().map(|k| k.to_string()).collect();
            visible_processes(&tree, &set).iter().map(|p| p.pid).collect::<Vec<_>>()
        };
        assert_eq!(pids(&[]), vec![1, 10, 2, 11, 3]);
        assert_eq!(pids(&["1:1000"]), vec![1, 10, 11]);
        assert_eq!(pids(&["2:2000"]), vec![1, 10, 2, 11]);
        assert_eq!(pids(&["1:1000", "10:10000"]), vec![1, 10]);
        assert_eq!(pids(&["1:999"]), vec![1, 10, 2, 11, 3], "a stale identity collapses nothing");
    }

    #[test]
    fn retry_shows_for_failed_collectors_only() {
        assert!(needs_retry(None, true));
        assert!(!needs_retry(None, false));
        assert!(needs_retry(Some("degraded"), false));
        assert!(needs_retry(Some("unavailable"), false));
        assert!(needs_retry(Some("stopped"), false));
        assert!(!needs_retry(Some("healthy"), true));
        assert!(!needs_retry(Some("starting"), false));
        assert_eq!(source_tone("healthy"), Tone::Default);
        assert_eq!(source_tone("starting"), Tone::Warning);
        assert_eq!(source_tone("stopped"), Tone::Danger);
        let health = |status: &str, error: Option<&str>| SourceHealth {
            status: status.into(),
            last_error: error.map(str::to_owned),
        };
        assert!(desktop_only(&health("unavailable", Some("Desktop telemetry is unavailable in 'web' mode"))));
        assert!(!desktop_only(&health("unavailable", Some("sidecar exited"))));
        assert!(!desktop_only(&health("degraded", Some("'web' mode"))));
        assert!(!desktop_only(&health("unavailable", None)));
    }

    #[test]
    fn chart_scales_and_bars() {
        let bucket = |avg: f64, read: f64, write: f64| Bucket { avg_cpu: avg, max_cpu: avg, read_bytes: read, write_bytes: write };
        assert_eq!(cpu_scale_max(&[]), 1.0);
        assert_eq!(cpu_scale_max(&[bucket(0.4, 0.0, 0.0), bucket(250.0, 0.0, 0.0)]), 250.0);
        assert_eq!(io_scale_max(&[bucket(0.0, 10.0, 30.0), bucket(0.0, 5.0, 5.0)]), 40.0);
        assert_eq!(bar_height(0.0, 10.0, 2.0), 0.0);
        assert_eq!(bar_height(0.01, 10.0, 2.0), 2.0);
        assert_eq!(bar_height(5.0, 10.0, 2.0), 50.0);
        assert_eq!(bar_height(0.5, 0.0, 1.0), 50.0, "the scale never drops under 1");
    }

    #[test]
    fn numbers_read_like_web() {
        assert_eq!(format_rate(1536.0), "1.50 KB/s");
        assert_eq!(format_rate(0.0), "0 B/s");
        assert_eq!(format_duration_micros(999.4), "999 µs");
        assert_eq!(format_duration_micros(1_840.0), "1.84 ms");
        assert_eq!(format_duration_micros(2_000.0), "2 ms");
        assert_eq!(format_duration_micros(1_500_000.0), "1.50 s");
        assert_eq!(format_sample_interval(250.4), "250 ms");
        assert_eq!(format_sample_interval(1_000.0), "1 second");
        assert_eq!(format_sample_interval(2_000.0), "2 seconds");
        assert_eq!(format_sample_interval(2_500.0), "2.5 seconds");
        assert_eq!(format_sample_interval(1_250_000.0), "1,250 seconds");
        assert_eq!(format_cpu_time(90_000.0 / 1_000.0), "1.50m");
        assert_eq!(boolean_label("true", "Battery", "External power"), "Battery");
        assert_eq!(boolean_label("false", "Battery", "External power"), "External power");
        assert_eq!(boolean_label("unknown", "Battery", "External power"), "Unknown");
        assert_eq!(category_label("server-child"), "Backend child");
        assert_eq!(category_label("future-kind"), "future-kind");
        assert_eq!(io_semantics_label("all-io"), "All I/O bytes");
    }

    #[test]
    fn history_windows_default_to_fifteen_minutes() {
        assert_eq!(history_input(Ui::default().window_index()), json!({"windowMs": 900_000, "bucketMs": 30_000}));
        assert_eq!(history_input(3), json!({"windowMs": 3_600_000, "bucketMs": 120_000}));
        assert_eq!(history_input(9), json!({"windowMs": 3_600_000, "bucketMs": 120_000}));
    }
}
