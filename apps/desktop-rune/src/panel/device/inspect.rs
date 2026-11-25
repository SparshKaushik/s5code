//! Device tools' readouts from the device hub (web `deviceHubApi.ts`): the accessibility
//! elements drawn over the screen and the iOS event log, each fed by a thread while it shows.

use std::io::BufRead;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

/// Web's `AX_POLL_INTERVAL_MS`.
const AX_POLL: Duration = Duration::from_secs(2);
/// Web's `AX_ELEMENT_LIMIT`.
const AX_LIMIT: usize = 500;
/// Web's `DEVICE_EVENT_LOG_LIMIT`.
pub const LOG_LIMIT: usize = 100;

/// One accessibility element, normalized to the displayed screen: 0..1 on both axes.
#[derive(Clone, Debug, PartialEq)]
pub struct AxElement {
    pub id: String,
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogEntry {
    pub id: i64,
    pub timestamp: String,
    pub summary: String,
}

/// Data a thread keeps current while this lives; dropping it stops the thread.
pub struct Feed<T> {
    pub data: Arc<Mutex<T>>,
    pub rev: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

impl<T> Drop for Feed<T> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl<T> Feed<T> {
    /// Data that never changes, for scenes.
    pub fn fixed(data: T) -> Self {
        Feed { data: Arc::new(Mutex::new(data)), rev: Arc::default(), stop: Arc::new(AtomicBool::new(true)) }
    }

    pub fn rev(&self) -> u64 {
        self.rev.load(Ordering::Relaxed)
    }
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

/// serve-sim's `/helper/<device>/ax`: nested elements in points; the first root is the screen.
pub fn ios_ax(payload: &Value) -> Result<Vec<AxElement>, String> {
    let Some(roots) = payload.as_array() else {
        return Err(payload["error"].as_str().unwrap_or("Unexpected accessibility payload.").to_owned());
    };
    let frame = roots.first().map(|r| &r["frame"]);
    let screen_w = frame.and_then(|f| num(&f["width"])).unwrap_or(1.).max(1.);
    let screen_h = frame.and_then(|f| num(&f["height"])).unwrap_or(1.).max(1.);
    let mut out = vec![];
    fn visit(node: &Value, path: String, size: (f64, f64), out: &mut Vec<AxElement>) {
        if out.len() >= AX_LIMIT || !node["frame"].is_object() {
            return;
        }
        let frame = &node["frame"];
        let (w, h) = (num(&frame["width"]).unwrap_or(0.), num(&frame["height"]).unwrap_or(0.));
        let covers_screen = (w - size.0).abs() < 0.5 && (h - size.1).abs() < 0.5;
        if !covers_screen && w > 0. && h > 0. {
            out.push(AxElement {
                id: node["AXUniqueId"].as_str().map_or_else(|| path.clone(), str::to_owned),
                label: node["AXLabel"].as_str().unwrap_or_default().to_owned(),
                x: (num(&frame["x"]).unwrap_or(0.) / size.0) as f32,
                y: (num(&frame["y"]).unwrap_or(0.) / size.1) as f32,
                width: (w / size.0) as f32,
                height: (h / size.1) as f32,
            });
        }
        for (i, child) in node["children"].as_array().into_iter().flatten().enumerate() {
            visit(child, format!("{path}.{i}"), size, out);
        }
    }
    for (i, root) in roots.iter().enumerate() {
        visit(root, i.to_string(), (screen_w, screen_h), &mut out);
    }
    Ok(out)
}

/// serve-emu's `/api/accessibility`: a flat node list in pixels; the first node is the screen.
/// Screen-sized containers and unlabeled, unclickable nodes are left out.
pub fn android_ax(payload: &Value) -> Result<Vec<AxElement>, String> {
    let Some(nodes) = payload["nodes"].as_array() else {
        return Err(payload["error"].as_str().unwrap_or("Unexpected accessibility payload.").to_owned());
    };
    let nodes: Vec<&Value> = nodes.iter().filter(|n| n["bounds"].is_object()).collect();
    let root = nodes.first().map(|n| &n["bounds"]);
    let screen_w = root.and_then(|b| num(&b["right"])).unwrap_or(1.).max(1.);
    let screen_h = root.and_then(|b| num(&b["bottom"])).unwrap_or(1.).max(1.);
    Ok(nodes
        .iter()
        .skip(1)
        .filter_map(|node| {
            let b = &node["bounds"];
            let (left, top) = (num(&b["left"]).unwrap_or(0.), num(&b["top"]).unwrap_or(0.));
            let width = (num(&b["right"]).unwrap_or(left) - left) / screen_w;
            let height = (num(&b["bottom"]).unwrap_or(top) - top) / screen_h;
            let text = node["text"].as_str().unwrap_or_default();
            let label = if text.is_empty() { node["contentDescription"].as_str().unwrap_or_default() } else { text };
            if (width >= 0.95 && height >= 0.9) || (label.is_empty() && node["clickable"] != true) {
                return None;
            }
            let id = match &node["id"] {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            Some(AxElement {
                id,
                label: label.to_owned(),
                x: (left / screen_w) as f32,
                y: (top / screen_h) as f32,
                width: width as f32,
                height: height as f32,
            })
        })
        .collect())
}

fn log_entry(raw: &Value) -> Option<LogEntry> {
    let id = raw["id"].as_i64().or_else(|| raw["id"].as_f64().map(|f| f as i64))?;
    let summary = raw["summary"].as_str().or_else(|| raw["msg"].as_str()).unwrap_or_default();
    Some(LogEntry { id, timestamp: raw["timestamp"].as_str().unwrap_or_default().to_owned(), summary: summary.to_owned() })
}

/// One event-log message: `{events: [...]}` replaces the log, `{event: {...}}` adds to it.
/// Keeps the newest `LOG_LIMIT` entries (web `appendDeviceEventLog`).
pub fn apply_log_message(log: &mut Vec<LogEntry>, data: &Value) {
    if let Some(events) = data["events"].as_array() {
        *log = events.iter().filter_map(log_entry).collect();
    } else if let Some(entry) = log_entry(&data["event"]) {
        log.push(entry);
    } else {
        return;
    }
    if log.len() > LOG_LIMIT {
        log.drain(..log.len() - LOG_LIMIT);
    }
}

/// The time of day an event-log timestamp shows (web `timestamp.slice(11, 19)`).
pub fn log_time(timestamp: &str) -> &str {
    timestamp.get(11..19).unwrap_or_default()
}

/// Server-sent events: `data:` lines gathered until a blank line ends the event.
#[derive(Default)]
pub struct Sse {
    data: String,
}

impl Sse {
    pub fn line(&mut self, line: &str) -> Option<String> {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            return (!self.data.is_empty()).then(|| std::mem::take(&mut self.data));
        }
        if let Some(rest) = line.strip_prefix("data:") {
            if !self.data.is_empty() {
                self.data.push('\n');
            }
            self.data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
        }
        None
    }
}

fn sleep_unless(stop: &AtomicBool, total: Duration) {
    let step = Duration::from_millis(100);
    let mut slept = Duration::ZERO;
    while slept < total && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(step);
        slept += step;
    }
}

/// Polls the screen's accessibility elements every two seconds; a failed poll keeps the last.
pub fn spawn_ax(base: String, auth: s5_net::Auth, path: String, query: String, android: bool, wake: impl Fn() + Send + 'static) -> Feed<Option<Vec<AxElement>>> {
    let feed = Feed { data: Arc::new(Mutex::new(None)), rev: Arc::default(), stop: Arc::default() };
    let (data, rev, stop) = (feed.data.clone(), feed.rev.clone(), feed.stop.clone());
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            let parsed = s5_net::api_get(&base, &auth, &path, &query, 8 << 20)
                .ok()
                .and_then(|body| serde_json::from_slice::<Value>(&body).ok())
                .and_then(|v| (if android { android_ax(&v) } else { ios_ax(&v) }).ok());
            if let Some(elements) = parsed
                && !stop.load(Ordering::Relaxed)
            {
                *data.lock().unwrap_or_else(|e| e.into_inner()) = Some(elements);
                rev.fetch_add(1, Ordering::Relaxed);
                wake();
            }
            sleep_unless(&stop, AX_POLL);
        }
    });
    feed
}

/// Streams serve-sim's event log while the section is open, reconnecting when it goes quiet.
pub fn spawn_log(base: String, auth: s5_net::Auth, path: String, query: String, wake: impl Fn() + Send + 'static) -> Feed<Vec<LogEntry>> {
    let feed = Feed { data: Arc::new(Mutex::new(vec![])), rev: Arc::default(), stop: Arc::default() };
    let (data, rev, stop) = (feed.data.clone(), feed.rev.clone(), feed.stop.clone());
    std::thread::spawn(move || {
        let url = format!("{}{path}?{query}", base.trim_end_matches('/'));
        let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout_read(Duration::from_secs(20)).build();
        while !stop.load(Ordering::Relaxed) {
            let response = auth.apply(agent.get(&url).set("Accept", "text/event-stream")).ok().and_then(|request| request.call().ok());
            let Some(response) = response else {
                sleep_unless(&stop, Duration::from_secs(3));
                continue;
            };
            let mut sse = Sse::default();
            for line in std::io::BufReader::new(response.into_reader()).lines() {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let Ok(line) = line else { break };
                let Some(message) = sse.line(&line) else { continue };
                let Ok(value) = serde_json::from_str::<Value>(&message) else { continue };
                apply_log_message(&mut data.lock().unwrap_or_else(|e| e.into_inner()), &value);
                rev.fetch_add(1, Ordering::Relaxed);
                wake();
            }
            sleep_unless(&stop, Duration::from_secs(1));
        }
    });
    feed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accessibility_trees_become_screen_fractions() {
        let ios = json!([{
            "frame": {"x": 0, "y": 0, "width": 400, "height": 800}, "type": "Application", "AXLabel": "App",
            "children": [
                {"frame": {"x": 40, "y": 80, "width": 200, "height": 40}, "type": "Button", "AXLabel": "Sign in", "AXUniqueId": "sign-in"},
                {"frame": {"x": 0, "y": 0, "width": 0, "height": 10}, "type": "Other"},
                {"frame": {"x": 100, "y": 400, "width": 100, "height": 80}, "type": "StaticText", "children": []}
            ]
        }]);
        let elements = ios_ax(&ios).unwrap();
        assert_eq!(elements.len(), 2, "the screen-sized root and empty frames are left out");
        assert_eq!(elements[0], AxElement { id: "sign-in".into(), label: "Sign in".into(), x: 0.1, y: 0.1, width: 0.5, height: 0.05 });
        assert_eq!((elements[1].id.as_str(), elements[1].label.as_str()), ("0.2", ""), "no id: its path");
        assert_eq!(ios_ax(&json!({"error": "Helper not running"})), Err("Helper not running".into()));

        let android = json!({"nodes": [
            {"bounds": {"left": 0, "top": 0, "right": 1080, "bottom": 2400}},
            {"bounds": {"left": 0, "top": 0, "right": 1080, "bottom": 2300}, "text": "Root", "className": "android.widget.FrameLayout"},
            {"id": 7, "bounds": {"left": 108, "top": 240, "right": 540, "bottom": 480}, "contentDescription": "Search", "className": "android.widget.ImageButton"},
            {"bounds": {"left": 0, "top": 600, "right": 100, "bottom": 700}, "className": "android.view.View"},
            {"bounds": {"left": 0, "top": 900, "right": 540, "bottom": 1020}, "clickable": true, "className": "android.widget.Button"}
        ]});
        let elements = android_ax(&android).unwrap();
        assert_eq!(elements.len(), 2, "screen-sized and unlabeled, unclickable nodes are left out");
        assert_eq!(elements[0], AxElement { id: "7".into(), label: "Search".into(), x: 0.1, y: 0.1, width: 0.4, height: 0.1 });
        assert_eq!((elements[1].id.as_str(), elements[1].label.as_str(), elements[1].y), ("", "", 0.375), "a clickable node without a label stays");
    }

    #[test]
    fn the_event_log_streams_resets_and_keeps_the_newest_hundred() {
        let mut sse = Sse::default();
        assert_eq!(sse.line("event: message"), None);
        assert_eq!(sse.line("data: {\"events\":"), None);
        assert_eq!(sse.line("data: []}\r"), None);
        assert_eq!(sse.line(""), Some("{\"events\":\n[]}".into()));
        assert_eq!(sse.line(""), None, "a blank line alone is no event");

        let mut log = vec![];
        apply_log_message(&mut log, &json!({"events": [{"id": 1, "timestamp": "2026-11-22T10:15:30.123Z", "kind": "tap", "summary": "Tapped Sign in"}, {"id": "x"}]}));
        assert_eq!(log, vec![LogEntry { id: 1, timestamp: "2026-11-22T10:15:30.123Z".into(), summary: "Tapped Sign in".into() }]);
        apply_log_message(&mut log, &json!({"event": {"id": 2, "msg": "App launched"}}));
        assert_eq!(log[1].summary, "App launched", "`msg` stands in for `summary`");
        apply_log_message(&mut log, &json!({"ping": true}));
        assert_eq!(log.len(), 2);
        for id in 3..=150 {
            apply_log_message(&mut log, &json!({"event": {"id": id}}));
        }
        assert_eq!((log.len(), log[0].id, log[LOG_LIMIT - 1].id), (LOG_LIMIT, 51, 150));
        apply_log_message(&mut log, &json!({"events": []}));
        assert!(log.is_empty(), "a reset batch replaces the log");
        assert_eq!(log_time("2026-11-22T10:15:30.123Z"), "10:15:30");
        assert_eq!(log_time("bad"), "");
    }
}
