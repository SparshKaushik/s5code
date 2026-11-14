//! Web `TelemetryExportSettings`: the OpenTelemetry export section of Settings › Diagnostics.
use super::controls as c;
use crate::kit;
use crate::shell::palette::toast_error;
use crate::theme::{self, AlphaMul, T};
use crate::App;
use rune::{El, TextEdit, col, row, text};
use serde_json::{Value, json};

/// Web `SIGNALS`: (setting key, running source key, running variable key, label, signal).
const SIGNALS: [(&str, &str, &str, &str, &str); 3] = [
    ("otlpTracesUrl", "otlpTracesSource", "otlpTracesVariable", "Traces", "traces"),
    ("otlpMetricsUrl", "otlpMetricsSource", "otlpMetricsVariable", "Metrics", "metrics"),
    ("otlpLogsUrl", "otlpLogsSource", "otlpLogsVariable", "Logs", "logs"),
];

#[derive(Default)]
pub struct Ui {
    /// The environment the form was filled from; another one refills it.
    env: Option<String>,
    inputs: [TextEdit; 3],
    checks: [Option<Check>; 3],
    saving: Option<u64>,
    message: Option<String>,
}

/// An endpoint test: its URL, the request in flight, and `server.checkOtlpEndpoint`'s result.
#[derive(Clone)]
struct Check {
    url: String,
    pending: Option<u64>,
    result: Option<Value>,
}

fn saved(s: &App, key: &str) -> String {
    super::server_settings(s)["observability"][key].as_str().unwrap_or_default().to_owned()
}

fn running(s: &App) -> Option<&Value> {
    s.state().config.as_ref().map(|c| &c["observability"])
}

/// Web `runningOverride`: the URL an environment variable or the desktop set, its label and variable.
fn running_override(running: Option<&Value>, i: usize) -> Option<(String, &'static str, Option<String>)> {
    let (key, source, variable, ..) = SIGNALS[i];
    let running = running?;
    let url = running[key].as_str()?;
    let label = match running[source].as_str()? {
        "environment" => "Set by environment",
        "bootstrap" => "Set by desktop",
        _ => return None,
    };
    Some((url.to_owned(), label, running[variable].as_str().map(str::to_owned)))
}

fn is_http(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["http://", "https://"].iter().any(|p| lower.strip_prefix(p).is_some_and(|rest| !rest.is_empty()))
}

#[derive(Debug, PartialEq)]
enum Tone {
    Idle,
    Busy,
    Good,
    Bad,
}

/// Web `endpointStatus`.
fn endpoint_status(url: &str, check: Option<&Check>) -> (String, Tone) {
    if url.is_empty() {
        return ("Off".into(), Tone::Idle);
    }
    let Some(check) = check.filter(|c| c.url == url) else { return ("Not checked".into(), Tone::Idle) };
    let Some(result) = &check.result else { return ("Checking…".into(), Tone::Busy) };
    match result["_tag"].as_str() {
        Some("Accepted") => (format!("Connected · {} ms", result["latencyMs"].as_f64().unwrap_or(0.0).round()), Tone::Good),
        Some("Rejected") => (format!("Rejected · HTTP {}", result["status"]), Tone::Bad),
        _ if result["timedOut"] == true => ("Timed out".into(), Tone::Bad),
        _ => ("Unreachable".into(), Tone::Bad),
    }
}

fn can_check(s: &App) -> bool {
    super::permissions::allowed(s, "server.checkOtlpEndpoint")
        && s.state().config.as_ref().is_some_and(|c| c["environment"]["capabilities"]["otlpEndpointCheck"] == true)
}

fn check(s: &mut App, i: usize, url: String) {
    if !is_http(&url) {
        return;
    }
    let id = s.src.net().map(|net| net.request("server.checkOtlpEndpoint", json!({"signal": SIGNALS[i].4, "url": url})));
    s.ui.settings.telemetry.checks[i] = Some(Check { url, pending: id, result: None });
}

/// Fills the form from the environment's saved URLs and tests them, once per environment.
fn fill(s: &mut App) {
    let running = running(s).cloned();
    let urls: Vec<String> = (0..3)
        .map(|i| running_override(running.as_ref(), i).map_or_else(|| saved(s, SIGNALS[i].0), |(url, ..)| url))
        .collect();
    for (i, (key, ..)) in SIGNALS.iter().enumerate() {
        s.ui.settings.telemetry.inputs[i] = TextEdit::new(saved(s, key));
    }
    s.ui.settings.telemetry.checks = Default::default();
    s.ui.settings.telemetry.message = None;
    if can_check(s) {
        for (i, url) in urls.into_iter().enumerate() {
            check(s, i, url);
        }
    }
}

pub fn poll(s: &mut App) -> bool {
    if !matches!(&s.route, crate::Route::Settings(p) if p == "diagnostics") || s.state().config.is_none() {
        return false;
    }
    let mut changed = false;
    let env = super::permissions::target(s);
    if s.ui.settings.telemetry.env.as_ref() != Some(&env) {
        s.ui.settings.telemetry.env = Some(env);
        fill(s);
        changed = true;
    }
    for i in 0..3 {
        let Some(id) = s.ui.settings.telemetry.checks[i].as_ref().and_then(|c| c.pending) else { continue };
        let Some(reply) = s.src.net().and_then(|net| net.take_reply(id)) else { continue };
        let slot = &mut s.ui.settings.telemetry.checks[i];
        match reply {
            Ok(result) => {
                if let Some(check) = slot {
                    check.pending = None;
                    check.result = Some(result);
                }
            }
            // Web drops a check whose request failed.
            Err(_) => *slot = None,
        }
        changed = true;
    }
    let saving = s.ui.settings.telemetry.saving;
    if let Some(reply) = saving.and_then(|id| s.src.net().and_then(|net| net.take_reply(id))) {
        s.ui.settings.telemetry.saving = None;
        match reply {
            Ok(_) => s.ui.settings.telemetry.message = Some("Saved. Restart the server to apply.".into()),
            Err(error) => {
                let detail = error["message"].as_str().map_or_else(|| error.to_string(), str::to_owned);
                toast_error(s, "Setting not saved", Some(&detail));
            }
        }
        changed = true;
    }
    changed
}

/// The signals whose field differs from the saved URL.
fn changed(s: &App) -> Vec<usize> {
    let ui = &s.ui.settings.telemetry;
    (0..3).filter(|&i| ui.inputs[i].text.trim() != saved(s, SIGNALS[i].0)).collect()
}

fn save(s: &mut App) {
    let changed = changed(s);
    if changed.is_empty() || s.ui.settings.telemetry.saving.is_some() {
        return;
    }
    let patch: serde_json::Map<String, Value> =
        changed.iter().map(|&i| (SIGNALS[i].0.to_owned(), json!(s.ui.settings.telemetry.inputs[i].text.trim()))).collect();
    let id = s.src.net().map(|net| net.request("server.updateSettings", json!({"patch": {"observability": patch}})));
    let ui = &mut s.ui.settings.telemetry;
    ui.saving = id;
    ui.message = None;
}

fn discard(s: &mut App) {
    for (i, (key, ..)) in SIGNALS.iter().enumerate() {
        s.ui.settings.telemetry.inputs[i] = TextEdit::new(saved(s, key));
    }
    s.ui.settings.telemetry.message = None;
    if !can_check(s) {
        return;
    }
    let running = running(s).cloned();
    for i in 0..3 {
        let url = saved(s, SIGNALS[i].0);
        let tested = s.ui.settings.telemetry.checks[i].as_ref().is_some_and(|c| c.url == url);
        if running_override(running.as_ref(), i).is_none() && !tested {
            check(s, i, url);
        }
    }
}

/// Web's OpenTelemetry export section, for the Diagnostics page to mount.
#[allow(dead_code)]
pub(crate) fn view(s: &App, t: &T) -> El {
    let ui = &s.ui.settings.telemetry;
    let can_edit = super::permissions::allowed(s, "server.checkOtlpEndpoint");
    let checking = can_check(s);
    let saving = ui.saving.is_some();
    let running = running(s);
    let muted = |line: String| text(line).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap();
    let mut form = col()
        .w_full()
        .px(16.0)
        .py(16.0)
        .gap(16.0)
        .child(muted("Send traces, metrics, and logs to an OTLP HTTP receiver. Restart the server to apply. Environment variables override these settings.".into()));
    if !can_edit {
        let label = crate::env::label(s, &super::permissions::target(s));
        form = form.child(muted(format!("This connection lacks permission to change settings on {label}.")));
    }
    for (i, (key, _, _, label, signal)) in SIGNALS.iter().enumerate() {
        let over = running_override(running, i);
        let url = over.as_ref().map_or_else(|| ui.inputs[i].text.trim().to_owned(), |(url, ..)| url.clone());
        let probe = checking.then(|| endpoint_status(&url, ui.checks[i].as_ref()));
        let status = match (&over, probe) {
            (Some((_, label, _)), Some((check, tone))) => Some((format!("{label} · {check}"), tone)),
            (Some((_, label, _)), None) => Some(((*label).to_owned(), Tone::Idle)),
            (None, check) => check,
        };
        let mut heading = row().gap(6.0).center_y();
        if let Some((_, tone)) = &status {
            heading = heading.child(super::connections::dot(match tone {
                Tone::Idle => t.c.muted_foreground.alpha_mul(0.4),
                Tone::Busy => t.c.warning,
                Tone::Good => t.c.success,
                Tone::Bad => t.c.destructive,
            }));
        }
        heading = heading.child(text(format!("{label} endpoint")).font(theme::XS_MEDIUM).color(t.c.foreground));
        let mut top = row().w_full().gap(12.0).center_y().child(heading.grow(1.0));
        if let Some((line, _)) = &status {
            top = top.child(text(line.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground));
        }
        let mut field = row().w_full().gap(8.0).child(
            kit::Input::new(&ui.inputs[i])
                .placeholder(format!("http://localhost:4318/v1/{signal}"))
                .size("sm")
                .disabled(saving || !can_edit)
                .on_edit(move |s: &mut App| {
                    s.ui.settings.telemetry.message = None;
                    &mut s.ui.settings.telemetry.inputs[i]
                })
                .on_submit(|s: &mut App, _| save(s))
                .build(t)
                .single_line()
                .grow(1.0),
        );
        if checking {
            let in_flight = ui.checks[i].as_ref().is_some_and(|c| c.result.is_none());
            let target = url.clone();
            field = field.child(
                kit::Button::new("Test")
                    .variant("outline")
                    .size("sm")
                    .disabled(!is_http(&url) || in_flight)
                    .on_press(move |s: &mut App| check(s, i, target.clone()))
                    .build(t),
            );
        }
        let mut block = col().w_full().gap(6.0).child(top).child(field);
        let saved_url = saved(s, key);
        let running_url = running.and_then(|r| r[*key].as_str()).unwrap_or_default();
        let variable = over.as_ref().and_then(|(.., v)| v.clone());
        if saved_url != running_url || variable.is_some() {
            let shown = if running_url.is_empty() { "Disabled" } else { running_url };
            let mut line = row().w_full().gap(4.0).child(muted(format!("Running: {shown}")));
            if let Some(variable) = variable {
                line = line.child(muted("from".into())).child(text(variable).font(theme::mono(theme::TEXT_XS)).color(t.c.muted_foreground));
            }
            block = block.child(line);
        }
        form = form.child(block);
    }
    let dirty = !changed(s).is_empty();
    form = form.child(
        row()
            .w_full()
            .gap(12.0)
            .center_y()
            .child(text(ui.message.clone().unwrap_or_default()).role("status").font(theme::TEXT_XS).color(t.c.muted_foreground).grow(1.0))
            .child(
                kit::Button::new("Discard")
                    .variant("outline")
                    .size("xs")
                    .disabled(!dirty || saving || !can_edit)
                    .on_press(discard)
                    .build(t),
            )
            .child(
                kit::Button::new(if saving { "Saving…" } else { "Save" })
                    .size("xs")
                    .disabled(!dirty || saving || !can_edit)
                    .on_press(save)
                    .build(t),
            ),
    );
    c::section("OpenTelemetry export", vec![form], t)
}

/// Web Maintenance's Diagnostics section: the way to this page.
pub fn maintenance_section(s: &App, t: &T) -> El {
    let description = if s.ui.settings.environment_only {
        "Inspect processes, resource use, and logs on this environment."
    } else {
        "Inspect processes, resource use, and logs on one environment at a time."
    };
    let line = row()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(32.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(text("Diagnostics").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(text(description).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()),
        )
        .child(
            kit::Button::new("View diagnostics")
                .variant("outline")
                .size("sm")
                .on_press(|s: &mut App| s.navigate(crate::Route::Settings("diagnostics".into())))
                .build(t),
        );
    c::section("Diagnostics", vec![line], t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_status_and_overrides_read_like_web() {
        let check = |result: Option<Value>| Check { url: "http://h:4318/v1/traces".into(), pending: None, result };
        let url = "http://h:4318/v1/traces";
        assert_eq!(endpoint_status("", None), ("Off".into(), Tone::Idle));
        assert_eq!(endpoint_status(url, None).0, "Not checked");
        assert_eq!(endpoint_status("http://other", Some(&check(None))).0, "Not checked", "a check of another URL is stale");
        assert_eq!(endpoint_status(url, Some(&check(None))), ("Checking…".into(), Tone::Busy));
        assert_eq!(endpoint_status(url, Some(&check(Some(json!({"_tag": "Accepted", "latencyMs": 12.4}))))), ("Connected · 12 ms".into(), Tone::Good));
        assert_eq!(endpoint_status(url, Some(&check(Some(json!({"_tag": "Rejected", "status": 404}))))).0, "Rejected · HTTP 404");
        assert_eq!(endpoint_status(url, Some(&check(Some(json!({"_tag": "Unreachable", "timedOut": true}))))).0, "Timed out");
        assert_eq!(endpoint_status(url, Some(&check(Some(json!({"_tag": "Unreachable", "timedOut": false}))))).0, "Unreachable");
        let running = json!({"otlpTracesUrl": "http://env:4318", "otlpTracesSource": "environment", "otlpTracesVariable": "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT", "otlpLogsUrl": "", "otlpLogsSource": "settings"});
        assert_eq!(
            running_override(Some(&running), 0),
            Some(("http://env:4318".into(), "Set by environment", Some("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT".into())))
        );
        assert_eq!(running_override(Some(&running), 2), None, "a saved setting is no override");
        assert!(is_http("HTTPS://x") && !is_http("http://") && !is_http("ftp://x"));
    }
}
