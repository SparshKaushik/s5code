//! Web `scheduledTasksSettings.logic.ts` and client-runtime `scheduledTaskWebhook.ts`: the
//! editor's draft, its checks, and the schedule it saves.
use crate::data::Zone;
use rune::TextEdit;
use serde_json::{Value, json};

pub const MIN_INTERVAL_MS: i64 = 60_000;
pub const MAX_DELIVERY_AGE_MINUTES: i64 = 24 * 60;
pub const MAX_FILTERS: usize = 16;
pub const DEFAULT_WEBHOOK_PROMPT: &str = "Handle this webhook:\n{{body}}";

/// Web `WEBHOOK_FILTER_OPS`.
pub const FILTER_OPS: [(&str, &str); 5] =
    [("equals", "equals"), ("notEquals", "does not equal"), ("in", "is one of"), ("startsWith", "starts with"), ("exists", "exists")];

pub const WEEKDAY_LABELS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Fixed,
    Once,
    Interval,
    Webhook,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Workspace {
    Worktree,
    Root,
    Existing,
}

impl Workspace {
    pub const ALL: [Workspace; 3] = [Workspace::Worktree, Workspace::Root, Workspace::Existing];
    pub fn label(self) -> &'static str {
        match self {
            Workspace::Worktree => "Create a new worktree",
            Workspace::Root => "Use the project checkout",
            Workspace::Existing => "Use a specific checkout",
        }
    }
    pub fn value(self) -> &'static str {
        match self {
            Workspace::Worktree => "worktree",
            Workspace::Root => "root",
            Workspace::Existing => "existing_worktree",
        }
    }
}

/// One editable filter row; `in` takes a comma-separated list, `exists` ignores the value.
#[derive(Clone)]
pub struct Filter {
    pub path: TextEdit,
    pub op: &'static str,
    pub value: TextEdit,
}

impl Filter {
    pub fn empty() -> Self {
        Filter { path: TextEdit::new(""), op: "equals", value: TextEdit::new("") }
    }
    fn parts(&self) -> (String, &'static str, String) {
        (self.path.text.clone(), self.op, self.value.text.clone())
    }
}

/// Web `DraftState`; text fields are the inputs' own edit state.
#[derive(Clone)]
pub struct Draft {
    pub editing_id: Option<String>,
    pub title: TextEdit,
    pub prompt: TextEdit,
    pub enabled: bool,
    pub mode: Mode,
    pub interval_minutes: TextEdit,
    pub time_of_day: TextEdit,
    /// Local date and time as `YYYY-MM-DDTHH:MM`, a `datetime-local` value.
    pub run_at: TextEdit,
    /// The stored ISO run time, sent back unchanged when the field was not edited.
    pub saved_run_at: String,
    pub weekdays: Vec<u8>,
    pub project_id: String,
    pub thread_id: Option<String>,
    pub workspace: Workspace,
    pub base_ref: TextEdit,
    pub start_from_origin: bool,
    pub checkout: TextEdit,
    /// `instanceId:model`, empty for the default.
    pub model_key: String,
    pub runtime_mode: String,
    pub interaction_mode: String,
    /// The source selection, kept with its options while the picker leaves it unchanged.
    pub base_model: Option<Value>,
    pub signature_enabled: bool,
    pub signature_header: TextEdit,
    pub signature_encoding: &'static str,
    pub signature_prefix: TextEdit,
    /// Write-only: empty keeps the secret already stored on the server.
    pub signature_secret: TextEdit,
    pub max_age: TextEdit,
    pub replay_key: TextEdit,
    pub coalesce_key: TextEdit,
    pub coalesce_window: TextEdit,
    pub filters: Vec<Filter>,
    saved_filters: Vec<(String, &'static str, String)>,
}

const SIGNATURE_HEADER: &str = "x-hub-signature-256";
const SIGNATURE_PREFIX: &str = "sha256=";

impl Draft {
    /// Web `EMPTY_DRAFT`.
    pub fn new(project_id: String) -> Self {
        Draft {
            editing_id: None,
            title: TextEdit::new(""),
            prompt: TextEdit::new(""),
            enabled: true,
            mode: Mode::Fixed,
            interval_minutes: TextEdit::new("15"),
            time_of_day: TextEdit::new("09:00"),
            run_at: TextEdit::new(""),
            saved_run_at: String::new(),
            weekdays: vec![1, 2, 3, 4, 5],
            project_id,
            thread_id: None,
            workspace: Workspace::Worktree,
            base_ref: TextEdit::new("main"),
            start_from_origin: true,
            checkout: TextEdit::new(""),
            model_key: String::new(),
            runtime_mode: "full-access".into(),
            interaction_mode: "default".into(),
            base_model: None,
            signature_enabled: false,
            signature_header: TextEdit::new(SIGNATURE_HEADER),
            signature_encoding: "hex",
            signature_prefix: TextEdit::new(SIGNATURE_PREFIX),
            signature_secret: TextEdit::new("").secure(),
            max_age: TextEdit::new(""),
            replay_key: TextEdit::new(""),
            coalesce_key: TextEdit::new(""),
            coalesce_window: TextEdit::new(""),
            filters: Vec::new(),
            saved_filters: Vec::new(),
        }
    }

    /// Web `taskToDraft`.
    pub fn from_task(task: &Value, zone: Zone) -> Self {
        let schedule = &task["schedule"];
        let kind = schedule["type"].as_str().unwrap_or("interval");
        let webhook = kind == "webhook";
        let text = |v: &Value| TextEdit::new(v.as_str().unwrap_or_default());
        let filters: Vec<Filter> = if webhook {
            schedule["filters"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|f| {
                    let op = FILTER_OPS.iter().map(|(op, _)| *op).find(|op| f["op"] == *op).unwrap_or("equals");
                    let value = match op {
                        "exists" => String::new(),
                        "in" => f["value"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", "),
                        _ => f["value"].as_str().unwrap_or_default().to_owned(),
                    };
                    Filter { path: text(&f["path"]), op, value: TextEdit::new(&value) }
                })
                .collect()
        } else {
            Vec::new()
        };
        let weekdays: Vec<u8> = schedule["weekdays"].as_array().into_iter().flatten().filter_map(|d| d.as_u64().map(|d| d as u8)).collect();
        let strategy = &task["workspaceStrategy"];
        let signature = &schedule["signature"];
        let signed = webhook && signature.is_object();
        let dedupe = &schedule["dedupe"];
        let selection = &task["modelSelection"];
        let mut draft = Draft::new(task["projectId"].as_str().unwrap_or_default().to_owned());
        draft.editing_id = task["id"].as_str().map(str::to_owned);
        draft.title = text(&task["title"]);
        draft.prompt = text(&task["prompt"]);
        draft.enabled = task["enabled"] == true;
        draft.mode = match kind {
            "fixed_time" => Mode::Fixed,
            "once" => Mode::Once,
            "webhook" => Mode::Webhook,
            _ => Mode::Interval,
        };
        draft.interval_minutes = TextEdit::new(&if kind == "interval" {
            number((schedule["everyMs"].as_f64().unwrap_or(0.0) / 60_000.0).max(1.0))
        } else {
            "15".into()
        });
        if kind == "fixed_time" {
            draft.time_of_day = text(&schedule["timeOfDay"]);
        }
        if kind == "once" {
            let saved = schedule["runAt"].as_str().unwrap_or_default();
            draft.run_at = TextEdit::new(&crate::data::iso_ms(saved).map(|at| format_local(zone.wall(at))).unwrap_or_default());
            draft.saved_run_at = saved.to_owned();
        }
        draft.weekdays = if kind == "fixed_time" && !weekdays.is_empty() { weekdays } else { (0..7).collect() };
        draft.thread_id = task["threadId"].as_str().map(str::to_owned);
        draft.workspace = match strategy["type"].as_str() {
            Some("root") => Workspace::Root,
            Some("existing_worktree") => Workspace::Existing,
            _ => Workspace::Worktree,
        };
        if draft.workspace == Workspace::Worktree {
            draft.base_ref = text(&strategy["baseRef"]);
            draft.start_from_origin = strategy["startFromOrigin"] == true;
        }
        if draft.workspace == Workspace::Existing {
            draft.checkout = text(&strategy["worktreePath"]);
        }
        draft.model_key = format!("{}:{}", selection["instanceId"].as_str().unwrap_or_default(), selection["model"].as_str().unwrap_or_default());
        draft.runtime_mode = task["runtimeMode"].as_str().unwrap_or("full-access").to_owned();
        draft.interaction_mode = task["interactionMode"].as_str().unwrap_or("default").to_owned();
        draft.base_model = selection.is_object().then(|| selection.clone());
        if signed {
            draft.signature_enabled = true;
            draft.signature_header = text(&signature["header"]);
            draft.signature_encoding = if signature["encoding"] == "base64" { "base64" } else { "hex" };
            draft.signature_prefix = text(&signature["prefix"]);
        }
        if webhook {
            draft.replay_key = text(&dedupe["replayKey"]);
            draft.coalesce_key = text(&dedupe["coalesceKey"]);
            if let Some(window) = dedupe["coalesceWindowSeconds"].as_i64() {
                draft.coalesce_window = TextEdit::new(&window.to_string());
            }
            if let Some(age) = schedule["maxDeliveryAgeMinutes"].as_i64() {
                draft.max_age = TextEdit::new(&age.to_string());
            }
        }
        draft.saved_filters = filters.iter().map(Filter::parts).collect();
        draft.filters = filters;
        draft
    }

    /// Web: switching to Once suggests the next whole hour; switching to webhook fills an empty prompt.
    pub fn set_mode(&mut self, mode: Mode, now: i64, zone: Zone) {
        if mode == Mode::Once && self.run_at.text.is_empty() {
            let suggested = (now + 3_599_999).div_euclid(3_600_000) * 3_600_000;
            self.run_at = TextEdit::new(&format_local(zone.wall(suggested)));
            self.saved_run_at = iso(suggested);
        }
        if mode == Mode::Webhook && self.prompt.text.trim().is_empty() {
            self.prompt = TextEdit::new(DEFAULT_WEBHOOK_PROMPT);
        }
        self.mode = mode;
    }

    /// The interval in milliseconds, `None` when it is not a number.
    pub fn interval_ms(&self) -> Option<i64> {
        let minutes: f64 = self.interval_minutes.text.trim().parse().ok()?;
        let ms = (minutes * 60_000.0).round();
        (ms.is_finite() && ms.abs() < 9_007_199_254_740_991.0).then_some(ms as i64)
    }

    fn filters_changed(&self) -> bool {
        self.saved_filters.len() != self.filters.len()
            || self.saved_filters.iter().zip(&self.filters).any(|((path, op, value), f)| {
                *path != f.path.text || *op != f.op || (*op != "exists" && *value != f.value.text)
            })
    }
}

/// `15` for whole numbers, as JavaScript's `String(15)`.
fn number(value: f64) -> String {
    if value.fract() == 0.0 { format!("{}", value as i64) } else { format!("{value}") }
}

/// Web `scheduleDraftErrors`, plus a check the browser's time input does for it.
pub fn errors(draft: &Draft, zone: Zone) -> Vec<(String, String)> {
    let one = |title: &str, description: String| vec![(title.to_owned(), description)];
    match draft.mode {
        Mode::Once => {
            let Some(wall) = parse_local(&draft.run_at.text) else { return one("Invalid run time", "Pick a date and time.".into()) };
            // A time skipped by a daylight saving change reads back as a different clock time.
            if format_local(zone.wall(zone.instant(wall))) != draft.run_at.text {
                return one("Invalid run time", "That time does not exist in your time zone.".into());
            }
            vec![]
        }
        Mode::Fixed => {
            let time = draft.time_of_day.text.trim();
            if !time.is_empty() && !is_time_of_day(time) {
                return one("Invalid run time", "Enter a time as HH:MM.".into());
            }
            vec![]
        }
        Mode::Interval => vec![],
        Mode::Webhook => {
            if parse_max_age(&draft.max_age.text).is_none() {
                return one("Invalid age limit", format!("Enter whole minutes from 1 to {MAX_DELIVERY_AGE_MINUTES}, or leave it blank."));
            }
            let mut errors = Vec::new();
            for (label, value) in [("Duplicate request key", &draft.replay_key.text), ("Coalesce key", &draft.coalesce_key.text)] {
                if !value.trim().is_empty() && !is_dedupe_template(value.trim()) {
                    errors.push(("Invalid webhook settings".into(), format!("{label} must contain a valid request placeholder.")));
                }
            }
            let window = draft.coalesce_window.text.trim();
            if !window.is_empty() && !window.parse::<i64>().is_ok_and(|w| (10..=3600).contains(&w)) {
                errors.push(("Invalid webhook settings".into(), "Coalesce window must be whole seconds from 10 to 3600.".into()));
            }
            errors
        }
    }
}

/// Web `scheduleFromDraft`: the upsert's `schedule`, or `None` while the draft has errors.
pub fn schedule(draft: &Draft, zone: Zone) -> Option<Value> {
    if !errors(draft, zone).is_empty() {
        return None;
    }
    match draft.mode {
        Mode::Once => {
            let wall = parse_local(&draft.run_at.text)?;
            let saved = crate::data::iso_ms(&draft.saved_run_at);
            if saved.is_some_and(|at| format_local(zone.wall(at)) == draft.run_at.text) {
                return Some(json!({"type": "once", "runAt": draft.saved_run_at}));
            }
            // In an hour that repeats when clocks go back, keep the saved time's offset.
            let with_saved_offset = saved.map(|at| wall - (zone.wall(at) - at)).filter(|at| format_local(zone.wall(*at)) == draft.run_at.text);
            Some(json!({"type": "once", "runAt": iso(with_saved_offset.unwrap_or_else(|| zone.instant(wall)))}))
        }
        Mode::Webhook => {
            let max_age = parse_max_age(&draft.max_age.text)?;
            let filters = parse_filters(&draft.filters).ok()?;
            let (replay, coalesce) = (draft.replay_key.text.trim(), draft.coalesce_key.text.trim());
            let window = draft.coalesce_window.text.trim();
            let mut dedupe = json!({});
            if !replay.is_empty() {
                dedupe["replayKey"] = json!(replay);
            }
            if !coalesce.is_empty() {
                dedupe["coalesceKey"] = json!(coalesce);
            }
            if !window.is_empty() {
                dedupe["coalesceWindowSeconds"] = json!(window.parse::<i64>().ok()?);
            }
            let signature = draft.signature_enabled.then(|| {
                let mut signature = json!({
                    "header": draft.signature_header.text.trim(),
                    "encoding": draft.signature_encoding,
                    "prefix": draft.signature_prefix.text,
                });
                let secret = draft.signature_secret.text.trim();
                if !secret.is_empty() {
                    signature["secret"] = json!(secret);
                }
                signature
            });
            let mut out = json!({"type": "webhook", "signature": signature, "maxDeliveryAgeMinutes": max_age, "dedupe": dedupe});
            // Only edited filters are sent; the server keeps the stored ones otherwise.
            if draft.filters_changed() {
                out["filters"] = Value::Array(filters);
            }
            Some(out)
        }
        Mode::Interval => Some(json!({"type": "interval", "everyMs": draft.interval_ms().unwrap_or(0)})),
        Mode::Fixed => {
            let time = draft.time_of_day.text.trim();
            let mut out = json!({"type": "fixed_time", "timeOfDay": if time.is_empty() { "09:00" } else { time }});
            if !matches!(draft.weekdays.len(), 0 | 7) {
                let mut days = draft.weekdays.clone();
                days.sort_unstable();
                out["weekdays"] = json!(days);
            }
            Some(out)
        }
    }
}

/// Client-runtime `parseMaxDeliveryAge`: blank is no limit (`Some(None)`), `None` blocks saving.
pub fn parse_max_age(value: &str) -> Option<Option<i64>> {
    let value = value.trim();
    if value.is_empty() {
        return Some(None);
    }
    value.parse::<i64>().ok().filter(|m| (1..=MAX_DELIVERY_AGE_MINUTES).contains(m)).map(Some)
}

/// Client-runtime `parseWebhookFilterDrafts`, checked as the server's filter schema is.
pub fn parse_filters(drafts: &[Filter]) -> Result<Vec<Value>, String> {
    if drafts.len() > MAX_FILTERS {
        return Err(format!("Use at most {MAX_FILTERS} filters."));
    }
    let path_ok = |path: &str| ["body", "headers", "query"].iter().any(|root| path == *root || path.starts_with(&format!("{root}.")));
    drafts
        .iter()
        .enumerate()
        .map(|(index, draft)| {
            let path = draft.path.text.trim();
            let filter = match draft.op {
                "exists" => path_ok(path).then(|| json!({"path": path, "op": "exists"})),
                "in" => {
                    let values: Vec<&str> = draft.value.text.split(',').map(str::trim).filter(|v| !v.is_empty()).collect();
                    (path_ok(path) && !values.is_empty()).then(|| json!({"path": path, "op": "in", "value": values}))
                }
                op => path_ok(path).then(|| json!({"path": path, "op": op, "value": draft.value.text})),
            };
            filter.ok_or_else(|| {
                let needs = match draft.op {
                    "exists" => ".",
                    "in" => ", and at least one value.",
                    _ => ", and a value.",
                };
                format!("Filter {} needs a path under body, headers or query{needs}", index + 1)
            })
        })
        .collect()
}

/// Contracts `isWebhookDedupeTemplate`: at least one request placeholder and no stray braces.
pub fn is_dedupe_template(template: &str) -> bool {
    let placeholder = |inner: &str| {
        let inner = inner.trim();
        if inner == "request" {
            return true;
        }
        let mut parts = inner.split('.');
        matches!(parts.next(), Some("body" | "headers" | "query"))
            && parts.all(|p| !p.is_empty() && !p.chars().any(|c| c == '{' || c == '}' || c.is_whitespace()))
    };
    let (mut rest, mut matches, mut remainder) = (template, 0, String::new());
    while let Some(at) = rest.find("{{") {
        remainder.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        match after.find("}}") {
            Some(end) if placeholder(&after[..end]) => {
                matches += 1;
                rest = &after[end + 2..];
            }
            // Not a placeholder here: keep one brace and look again from the next.
            _ => {
                remainder.push('{');
                rest = &rest[at + 1..];
            }
        }
    }
    remainder.push_str(rest);
    matches > 0 && !remainder.contains("{{") && !remainder.contains("}}")
}

/// Contracts `TimeOfDay`: `H:MM` or `HH:MM`, 00:00 to 23:59.
pub fn is_time_of_day(time: &str) -> bool {
    let Some((h, m)) = time.split_once(':') else { return false };
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    digits(h) && h.len() <= 2 && digits(m) && m.len() == 2 && h.parse::<u32>().is_ok_and(|h| h < 24) && m.parse::<u32>().is_ok_and(|m| m < 60)
}

/// Web `webhookDeliveryLabel`.
pub fn delivery_label(delivery: &Value) -> String {
    let outcome = delivery["outcome"].as_str().unwrap_or_default();
    match outcome {
        "accepted" if delivery["pendingWindow"] == true => "Opened window".into(),
        "accepted" => "Ran".into(),
        "dispatch_failed" => "Run failed".into(),
        "duplicate" => "Duplicate".into(),
        "coalesced" => "Coalesced".into(),
        "filtered" => "Filtered".into(),
        "rejected_signature" => "Bad signature".into(),
        "disabled" => "Task paused".into(),
        "rate_limited" => "Rate limited".into(),
        "expired" => "Too old".into(),
        other => other.replace('_', " "),
    }
}

/// Web `deliveryOutcomeVariant`, for a badge.
pub fn delivery_variant(delivery: &Value) -> &'static str {
    let outcome = if delivery["pendingWindow"] == true { "coalesced" } else { delivery["outcome"].as_str().unwrap_or_default() };
    match outcome {
        "accepted" => "success",
        "duplicate" | "coalesced" | "disabled" | "rate_limited" | "expired" => "warning",
        "dispatch_failed" | "rejected_signature" => "error",
        _ => "outline",
    }
}

/// Web `scheduleLabel`.
pub fn schedule_label(schedule: &Value, zone: Zone) -> String {
    match schedule["type"].as_str().unwrap_or_default() {
        "webhook" => "On webhook".into(),
        "once" => {
            let at = schedule["runAt"].as_str().and_then(crate::data::iso_ms).unwrap_or(0);
            format!("Once, {}", medium_short(zone.wall(at)))
        }
        "interval" => {
            let every = schedule["everyMs"].as_f64().unwrap_or(0.0);
            if (every / 60_000.0).fract() == 0.0 {
                format!("Every {} min", number(every / 60_000.0))
            } else {
                format!("Every {} sec", (every / 1000.0).round() as i64)
            }
        }
        _ => {
            let days: Vec<i64> = schedule["weekdays"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect();
            let days = if days.is_empty() {
                "Daily".into()
            } else if days.len() == 5 && days.iter().all(|d| (1..=5).contains(d)) {
                "Weekdays".into()
            } else {
                days.iter().map(|d| WEEKDAY_LABELS[(*d).clamp(0, 6) as usize]).collect::<Vec<_>>().join(", ")
            };
            format!("{days} at {}", schedule["timeOfDay"].as_str().unwrap_or_default())
        }
    }
}

/// Web `relativeLabel`: past times as `formatRelativeTime`, upcoming ones counted down.
pub fn relative_label(value: Option<&str>, now: i64) -> String {
    let Some(at) = value.and_then(crate::data::iso_ms) else { return "Not scheduled".into() };
    let diff = at - now;
    if diff <= 0 {
        return crate::settings::storage::relative(value.unwrap_or_default(), now);
    }
    let minutes = (diff + 59_999) / 60_000;
    if minutes < 2 {
        return "in under a minute".into();
    }
    if minutes < 60 {
        return format!("in {minutes}m");
    }
    let hours = (minutes as f64 / 60.0).round() as i64;
    if hours < 24 {
        return format!("in {hours}h");
    }
    format!("in {}d", (hours as f64 / 24.0).round() as i64)
}

const DAY: i64 = 86_400_000;

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// A `datetime-local` value as wall-clock milliseconds; impossible dates are `None`.
pub fn parse_local(text: &str) -> Option<i64> {
    let (date, time) = text.trim().split_once('T')?;
    let mut parts = date.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 || time.len() != 5 || !is_time_of_day(time) {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&m) || d < 1 || d > days[(m - 1) as usize] {
        return None;
    }
    let (h, min) = time.split_once(':')?;
    Some(days_from_civil(y, m, d) * DAY + (h.parse::<i64>().ok()? * 60 + min.parse::<i64>().ok()?) * 60_000)
}

/// Web `toDateTimeLocalValue` of a wall-clock time.
pub fn format_local(wall: i64) -> String {
    let (y, m, d) = civil_from_days(wall.div_euclid(DAY));
    let minutes = wall.rem_euclid(DAY) / 60_000;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}", minutes / 60, minutes % 60)
}

/// JavaScript `Date.prototype.toISOString`.
pub fn iso(ms: i64) -> String {
    let (y, m, d) = civil_from_days(ms.div_euclid(DAY));
    let rem = ms.rem_euclid(DAY);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000)
}

/// `Intl.DateTimeFormat` with `dateStyle: "medium", timeStyle: "short"` in English.
fn medium_short(wall: i64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (y, m, d) = civil_from_days(wall.div_euclid(DAY));
    let minutes = wall.rem_euclid(DAY) / 60_000;
    let (h, min) = (minutes / 60, minutes % 60);
    let hour = if h % 12 == 0 { 12 } else { h % 12 };
    format!("{} {d}, {y}, {hour}:{min:02} {}", MONTHS[(m - 1) as usize], if h < 12 { "AM" } else { "PM" })
}

#[cfg(test)]
mod tests {
    use super::*;

    const UTC: Zone = Zone::Fixed(0);

    fn task(schedule: Value) -> Value {
        json!({
            "id": "t1", "title": "Check", "prompt": "Look", "enabled": true, "projectId": "p1", "threadId": null,
            "schedule": schedule, "workspaceStrategy": {"type": "worktree", "baseRef": "dev", "startFromOrigin": false},
            "modelSelection": {"instanceId": "codex", "model": "gpt", "options": {"effort": "high"}},
            "runtimeMode": "auto", "interactionMode": "plan",
        })
    }

    #[test]
    fn a_task_round_trips_through_the_draft_like_web_task_to_draft() {
        let fixed = Draft::from_task(&task(json!({"type": "fixed_time", "timeOfDay": "07:30", "weekdays": [3, 1]})), UTC);
        assert_eq!((fixed.mode, fixed.time_of_day.text.as_str(), fixed.base_ref.text.as_str(), fixed.start_from_origin), (Mode::Fixed, "07:30", "dev", false));
        assert_eq!(fixed.model_key, "codex:gpt");
        assert_eq!(schedule(&fixed, UTC), Some(json!({"type": "fixed_time", "timeOfDay": "07:30", "weekdays": [1, 3]})));
        // No weekdays means every day, which saves without a list.
        let daily = Draft::from_task(&task(json!({"type": "fixed_time", "timeOfDay": "07:30"})), UTC);
        assert_eq!(daily.weekdays.len(), 7);
        assert_eq!(schedule(&daily, UTC), Some(json!({"type": "fixed_time", "timeOfDay": "07:30"})));
        let interval = Draft::from_task(&task(json!({"type": "interval", "everyMs": 90_000})), UTC);
        assert_eq!(interval.interval_minutes.text, "1.5");
        assert_eq!(interval.interval_ms(), Some(90_000));
        // A legacy sub-minute interval opens as one minute.
        assert_eq!(Draft::from_task(&task(json!({"type": "interval", "everyMs": 1000})), UTC).interval_minutes.text, "1");
        let mut bad = Draft::new("p".into());
        bad.mode = Mode::Interval;
        bad.interval_minutes = TextEdit::new("soon");
        assert_eq!(bad.interval_ms(), None);
    }

    #[test]
    fn once_keeps_an_unedited_time_and_rejects_impossible_ones() {
        // The server stores UTC.
        let saved = "2026-11-02T14:00:00.000Z";
        let zone = Zone::Fixed(3_600_000);
        let mut draft = Draft::from_task(&task(json!({"type": "once", "runAt": saved})), zone);
        assert_eq!(draft.run_at.text, "2026-11-02T15:00");
        assert_eq!(schedule(&draft, zone), Some(json!({"type": "once", "runAt": saved})), "an untouched time is sent back as stored");
        draft.run_at = TextEdit::new("2026-11-02T16:30");
        assert_eq!(schedule(&draft, zone), Some(json!({"type": "once", "runAt": "2026-11-02T15:30:00.000Z"})));
        draft.run_at = TextEdit::new("2026-02-30T10:00");
        assert_eq!(errors(&draft, zone), vec![("Invalid run time".to_owned(), "Pick a date and time.".to_owned())]);
        assert_eq!(schedule(&draft, zone), None);
        let mut fresh = Draft::new("p".into());
        fresh.set_mode(Mode::Once, crate::data::iso_ms("2026-11-02T14:10:00Z").unwrap(), UTC);
        assert_eq!((fresh.run_at.text.as_str(), fresh.saved_run_at.as_str()), ("2026-11-02T15:00", "2026-11-02T15:00:00.000Z"));
    }

    #[test]
    fn webhook_settings_validate_and_save_like_web_schedule_from_draft() {
        let mut draft = Draft::new("p".into());
        draft.set_mode(Mode::Webhook, 0, UTC);
        assert_eq!(draft.prompt.text, DEFAULT_WEBHOOK_PROMPT);
        draft.max_age = TextEdit::new("2000");
        assert_eq!(errors(&draft, UTC)[0].0, "Invalid age limit");
        draft.max_age = TextEdit::new("");
        draft.replay_key = TextEdit::new("delivery");
        draft.coalesce_window = TextEdit::new("5");
        assert_eq!(
            errors(&draft, UTC).iter().map(|e| e.1.as_str()).collect::<Vec<_>>(),
            ["Duplicate request key must contain a valid request placeholder.", "Coalesce window must be whole seconds from 10 to 3600."]
        );
        draft.replay_key = TextEdit::new("{{ headers.x-delivery }}");
        draft.coalesce_window = TextEdit::new("");
        draft.signature_enabled = true;
        assert_eq!(
            schedule(&draft, UTC),
            Some(json!({
                "type": "webhook",
                "signature": {"header": "x-hub-signature-256", "encoding": "hex", "prefix": "sha256="},
                "maxDeliveryAgeMinutes": null,
                "dedupe": {"replayKey": "{{ headers.x-delivery }}"},
            })),
            "unchanged filters are left out, so the server keeps its own"
        );
        draft.filters.push(Filter { path: TextEdit::new("body.action"), op: "in", value: TextEdit::new("opened, reopened,") });
        assert_eq!(schedule(&draft, UTC).unwrap()["filters"], json!([{"path": "body.action", "op": "in", "value": ["opened", "reopened"]}]));
        draft.filters.push(Filter { path: TextEdit::new("action"), op: "exists", value: TextEdit::new("") });
        assert_eq!(parse_filters(&draft.filters), Err("Filter 2 needs a path under body, headers or query.".into()));
    }

    #[test]
    fn dedupe_templates_match_contracts() {
        assert!(is_dedupe_template("{{body.id}}"));
        assert!(is_dedupe_template("repo-{{ body.repository.id }}-{{request}}"));
        assert!(is_dedupe_template("{{{body}}}"), "a stray single brace on each side is plain text");
        assert!(!is_dedupe_template("static"));
        assert!(!is_dedupe_template("{{body.}}"));
        assert!(!is_dedupe_template("{{env.HOME}}"));
        assert!(!is_dedupe_template("{{body}} {{"));
        assert!(is_time_of_day("9:05") && is_time_of_day("23:59"));
        assert!(!is_time_of_day("24:00") && !is_time_of_day("09:5") && !is_time_of_day("nine"));
    }

    #[test]
    fn labels_read_like_web() {
        let zone = UTC;
        assert_eq!(schedule_label(&json!({"type": "fixed_time", "timeOfDay": "09:00", "weekdays": [1, 2, 3, 4, 5]}), zone), "Weekdays at 09:00");
        assert_eq!(schedule_label(&json!({"type": "fixed_time", "timeOfDay": "09:00"}), zone), "Daily at 09:00");
        assert_eq!(schedule_label(&json!({"type": "fixed_time", "timeOfDay": "18:00", "weekdays": [0, 6]}), zone), "Sun, Sat at 18:00");
        assert_eq!(schedule_label(&json!({"type": "interval", "everyMs": 900_000}), zone), "Every 15 min");
        assert_eq!(schedule_label(&json!({"type": "interval", "everyMs": 1500}), zone), "Every 2 sec");
        assert_eq!(schedule_label(&json!({"type": "once", "runAt": "2026-11-02T15:00:00Z"}), zone), "Once, Nov 2, 2026, 3:00 PM");
        let now = crate::data::iso_ms("2026-11-02T12:00:00Z").unwrap();
        assert_eq!(relative_label(None, now), "Not scheduled");
        assert_eq!(relative_label(Some("2026-11-02T12:00:30Z"), now), "in under a minute");
        assert_eq!(relative_label(Some("2026-11-02T12:45:00Z"), now), "in 45m");
        assert_eq!(relative_label(Some("2026-11-02T15:20:00Z"), now), "in 3h");
        assert_eq!(relative_label(Some("2026-11-05T12:00:00Z"), now), "in 3d");
        assert_eq!(relative_label(Some("2026-11-02T11:00:00Z"), now), "1h ago");
        assert_eq!(delivery_label(&json!({"outcome": "accepted", "pendingWindow": true})), "Opened window");
        assert_eq!(delivery_label(&json!({"outcome": "new_thing"})), "new thing");
        assert_eq!(delivery_variant(&json!({"outcome": "accepted", "pendingWindow": true})), "warning");
    }
}
