//! `/usage` (UsagePage): a route modal over the frame with subscription limits per provider
//! account, and cost or token usage from each environment's `server.getUsageSummary`, merged
//! across the selected environments, with its chart, totals, share bars and breakdown tables.

use std::collections::HashMap;

use rune::{Color, El, LineCap, Path, ScrollSpec, Shape, Tx, canvas, col, el, icon, row, spacer, text};
use serde_json::{Value, json};

use super::usage_merge::{self, Mismatch};
use crate::chat::composer::provider_icon;
use crate::data::{Zone, iso_ms, s as field};
use crate::icons as i;
use crate::kit::{self, Menu, MenuItem, Placement, ToggleGroup};
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route};

/// One environment's `server.getUsageSummary` (usageByWindowAtom's query for it).
#[derive(Default)]
pub struct Scan {
    key: String,
    /// The input and scan settings last asked for; cleared to ask again.
    sent: Option<(Value, Value)>,
    /// The request in flight and the window key it asks for.
    pending: Option<(u64, Value)>,
    /// The request in flight waits for sources that were still refreshing (`awaitRefresh`).
    awaiting: bool,
    summary: Option<Value>,
    /// The window key `summary` answers.
    answered: Option<Value>,
    error: Option<String>,
    /// `server.refreshUsageRates` in flight before a rescan.
    rates: Option<u64>,
    /// The header's Refresh is still running here (refreshUsage).
    refreshing: bool,
    /// Bumped whenever this scan's answer changes.
    generation: u64,
    /// When `summary` answered, for its staleness.
    at: i64,
    /// Other windows' answers, held while unseen (the usageSummary atoms' cache).
    cached: Vec<Cached>,
}

/// Another window's answer and the scan settings it was asked with, answered at `at` and last on
/// screen at `left`.
struct Cached {
    window: Value,
    settings: Value,
    summary: Value,
    at: i64,
    left: i64,
}

/// The usageSummary atom's `staleTimeMs`: an older cached answer shows and is asked again.
const STALE_MS: i64 = 60_000;
/// The query atoms' idle TTL: a window unseen this long is dropped.
const IDLE_MS: i64 = 5 * 60_000;

impl Scan {
    /// Moves to `window` (Atom.swr on mount): the answer on screen is cached under its own window
    /// and `window`'s cached answer shows at once. True when that answer is fresh enough to keep.
    fn show(&mut self, window: &Value, settings: &Value, now: i64) -> bool {
        if self.answered.as_ref() == Some(window) {
            return false;
        }
        self.cached.retain(|c| now - c.left < IDLE_MS);
        if let (Some(summary), Some(answered)) = (self.summary.take(), self.answered.take()) {
            let settings = self.sent.as_ref().map_or(Value::Null, |(_, settings)| settings.clone());
            self.cached.push(Cached { window: answered, settings, summary, at: self.at, left: now });
        }
        self.pending = None;
        self.awaiting = false;
        self.error = None;
        self.generation += 1;
        // The model filter is part of the query's input, so another filter's answer is another query.
        let Some(hit) = self.cached.iter().position(|c| c.window == *window && c.settings[3] == settings[3]).map(|at| self.cached.remove(at)) else {
            return false;
        };
        let fresh = hit.settings == *settings && now - hit.at < STALE_MS;
        self.summary = Some(hit.summary);
        self.answered = Some(hit.window);
        self.at = hit.at;
        if fresh {
            self.refreshing = false;
        }
        fresh
    }
}

pub struct Ui {
    pub metric: &'static str,
    /// Days in the window; 1 is the hourly "Past 24h".
    pub window_days: i64,
    /// The selected environments' merged usage on screen (useUsage's `shown`), as one summary.
    pub summary: Option<Value>,
    pub requested: Option<Value>,
    pub hidden: Vec<&'static str>,
    pub menu: Option<&'static str>,
    pub by_period: bool,
    /// The chart column under the pointer and the pointer in the plot (`hoverIndex`).
    pub hover: Option<(usize, f32, f32)>,
    plot_w: f32,
    /// The IANA zone sent as `timeZone`, and the zone its labels are read in.
    pub zone_name: String,
    pub zone: Zone,
    /// The breakdown row open in the model dialog (`kind\0apiProvider\0model`).
    pub model: Option<String>,
    pub model_hover: Option<(usize, f32, f32)>,
    model_plot_w: f32,
    pub prices: super::usage_prices::Ui,
    pub limits: super::usage_limits::Ui,
    pub tags: super::usage_tags::Ui,
    /// The caption's "Unpriced usage details" popover.
    pub unpriced_tip: bool,
    pub scans: Vec<Scan>,
    /// The window the shown summary answers, kept on screen while another window loads.
    pub answered: Option<Value>,
    /// What `summary` was last merged from, so it merges again only when that changes.
    merged_from: String,
    /// The last merge any selected environment answered, with its selection and hidden providers
    /// and its window (useUsage's `lastAnswered`).
    kept: Option<(String, Value, Value)>,
    /// `summary` is `kept` while the current window still scans (`showingKept`).
    showing_kept: bool,
    pub preferences_read: bool,
    /// The environment filter's checked environments; `None` is "All environments".
    pub environments: Option<Vec<String>>,
    /// `server.updateSettings` opting an environment into Cursor's keychain login, in flight.
    pub cursor_enabling: Vec<(String, u64)>,
}

impl Default for Ui {
    fn default() -> Self {
        let (zone_name, zone) = local_zone();
        Ui {
            metric: "limits",
            window_days: 30,
            summary: None,
            requested: None,
            hidden: Vec::new(),
            menu: None,
            by_period: false,
            hover: None,
            plot_w: 0.0,
            zone_name,
            zone,
            model: None,
            model_hover: None,
            model_plot_w: 0.0,
            prices: Default::default(),
            limits: Default::default(),
            tags: Default::default(),
            unpriced_tip: false,
            scans: Vec::new(),
            answered: None,
            merged_from: String::new(),
            kept: None,
            showing_kept: false,
            preferences_read: false,
            environments: None,
            cursor_enabling: Vec::new(),
        }
    }
}

/// The machine's IANA zone, as the web's `Intl` resolves it. A zone without a name falls back to
/// UTC for both the request and the labels, so the two always agree.
fn local_zone() -> (String, Zone) {
    let link = std::fs::read_link("/etc/localtime").ok().map(|p| p.to_string_lossy().into_owned());
    let file = std::fs::read_to_string("/etc/timezone").ok();
    match zone_name(std::env::var("TZ").ok(), link, file) {
        Some(name) => (name, Zone::Local),
        None => ("UTC".into(), Zone::Fixed(0)),
    }
}

fn zone_name(tz: Option<String>, link: Option<String>, file: Option<String>) -> Option<String> {
    let from_path = |p: &str| p.split_once("zoneinfo/").map(|(_, name)| name.to_string()).filter(|n| !n.is_empty());
    let named = |n: &str| n.contains('/') || matches!(n, "UTC" | "GMT" | "UCT" | "Zulu");
    if let Some(tz) = tz.as_deref().map(|t| t.trim_start_matches(':')).filter(|t| !t.is_empty()) {
        // localtime reads TZ first, so a POSIX rule there wins over the system zone too.
        return if tz.starts_with('/') { from_path(tz) } else { named(tz).then(|| tz.to_string()) };
    }
    link.as_deref().and_then(from_path).or_else(|| file.map(|f| f.trim().to_string()).filter(|f| named(f)))
}

/// How hour labels read: in which zone, on a 12- or 24-hour clock, relative to when.
#[derive(Clone, Copy, Default)]
struct Clock {
    zone: Zone,
    hour12: bool,
    now: i64,
}

impl Clock {
    fn hour(self, at: i64) -> String {
        let h = self.zone.wall(at).rem_euclid(DAY) / HOUR;
        match (self.hour12, h) {
            (false, h) => format!("{h:02}:00"),
            (true, 0) => "12 AM".into(),
            (true, 12) => "12 PM".into(),
            (true, h) => format!("{} {}", h % 12, if h < 12 { "AM" } else { "PM" }),
        }
    }

    /// formatDateTimeShort: "Nov 7, 3 PM".
    fn date_hour(self, at: i64) -> String {
        format!("{}, {}", day_short(&day_of(self.zone.wall(at))), self.hour(at))
    }

    /// formatRelativeHourShort: "3 PM today", "3 PM yesterday", else the date.
    fn relative(self, at: i64) -> String {
        let day = |ms: i64| self.zone.wall(ms).div_euclid(DAY);
        match day(self.now) - day(at) {
            0 => format!("{} today", self.hour(at)),
            1 => format!("{} yesterday", self.hour(at)),
            _ => self.date_hour(at),
        }
    }
}

fn clock(s: &App) -> Clock {
    let u = &s.ui.pages.usage;
    let format = s.client_settings.values.get("timestampFormat").and_then(Value::as_str).unwrap_or("locale");
    let until = u.answered.as_ref().and_then(|r| r["untilTime"].as_str()).and_then(iso_ms).unwrap_or(s.now_ms);
    Clock { zone: u.zone, hour12: crate::desktop::locale::hour12(Some(format)), now: until }
}

fn model_aliases(s: &App) -> Value {
    s.client_settings.values.get("usageModelTags").cloned().filter(Value::is_array).unwrap_or_else(|| json!([]))
}

fn ui(s: &mut App) -> &mut Ui {
    &mut s.ui.pages.usage
}

/// Usage kinds in the web's order: (kind, label, provider driver for its mark).
const PROVIDERS: &[(&str, &str, &str)] = &[
    ("codex", "Codex", "codex"),
    ("claude", "Claude Code", "claudeAgent"),
    ("grok", "Grok Build", "grok"),
    ("cursor", "Cursor", "cursor"),
    ("opencode", "OpenCode", "opencode"),
    ("antigravity", "Antigravity", "antigravity"),
    ("pi", "Pi", "pi"),
    ("muse", "Muse Code", "muse"),
];

pub(super) fn kind_of_driver(driver: &str) -> &str {
    if driver == "claudeAgent" { "claude" } else { driver }
}

fn presentation(kind: &str) -> (&'static str, &'static str) {
    PROVIDERS.iter().find(|p| p.0 == kind).map(|p| (p.1, p.2)).unwrap_or(("Other", "other"))
}

fn hex(s: &str) -> Color {
    rune::color::hex(s).unwrap_or(Color::WHITE)
}

fn mix(a: Color, b: Color, k: f32) -> Color {
    Color { r: a.r * k + b.r * (1.0 - k), g: a.g * k + b.g * (1.0 - k), b: a.b * k + b.b * (1.0 - k), a: a.a * k + b.a * (1.0 - k) }
}

pub(super) fn color(t: &T, kind: &str) -> Color {
    match kind {
        "codex" => t.c.contrast_foreground,
        "claude" | "claudeAgent" => hex("#d97757"),
        "grok" => mix(t.c.contrast_foreground, t.c.background, 0.72),
        "cursor" => hex("#8b8b8b"),
        "opencode" => hex("#5b9bbd"),
        "antigravity" => hex("#8c7bd1"),
        "pi" => hex("#4d9f8a"),
        "muse" => hex("#0866ff"),
        _ => t.c.muted_foreground,
    }
}

// ---- dates ------------------------------------------------------------------------------

/// `text-4xl` (36/40), which the app's type scale stops short of.
const TEXT_4XL: rune::tok::TextStyle = rune::tok::TextStyle::new(36.0, 400.0).lh(40.0);

const DAY: i64 = 86_400_000;
const HOUR: i64 = 3_600_000;
const MINUTE: i64 = 60_000;

pub(super) fn civil(days: i64) -> (i64, i64, i64) {
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

fn day_of(ms: i64) -> String {
    let (y, m, d) = civil(ms.div_euclid(DAY));
    format!("{y:04}-{m:02}-{d:02}")
}

pub(super) fn iso_of(ms: i64) -> String {
    let rem = ms.rem_euclid(DAY);
    format!("{}T{:02}:{:02}:{:02}.000Z", day_of(ms), rem / HOUR, rem % HOUR / MINUTE, rem % MINUTE / 1000)
}

fn day_ms(day: &str) -> Option<i64> {
    iso_ms(&format!("{day}T00:00:00Z"))
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

fn day_short(day: &str) -> String {
    let mut parts = day.split('-').map(|p| p.parse::<usize>().unwrap_or(0));
    let (_, m, d) = (parts.next(), parts.next().unwrap_or(1), parts.next().unwrap_or(1));
    format!("{} {d}", MONTHS[m.clamp(1, 12) - 1])
}

// ---- formats ----------------------------------------------------------------------------

fn grouped(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (k, c) in digits.chars().enumerate() {
        if k > 0 && (digits.len() - k) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

pub(super) fn usd(v: f64) -> String {
    let cents = (v * 100.0).round() as i64;
    format!("${}.{:02}", grouped(cents / 100), (cents % 100).abs())
}

/// `toFixed` then `/\.0+$/`: only an all-zero fraction drops, so 1.50 stays 1.50.
fn trim(v: f64) -> String {
    let digits = if v.abs() >= 100.0 { 0 } else if v.abs() >= 10.0 { 1 } else { 2 };
    let s = format!("{v:.digits$}");
    match s.split_once('.') {
        Some((whole, fraction)) if fraction.bytes().all(|b| b == b'0') => whole.to_string(),
        _ => s,
    }
}

fn tokens(v: f64) -> String {
    let a = v.abs();
    if a >= 1e12 {
        format!("{}T", trim(v / 1e12))
    } else if a >= 1e9 {
        format!("{}B", trim(v / 1e9))
    } else if a >= 1e6 {
        format!("{}M", trim(v / 1e6))
    } else if a >= 1e3 {
        format!("{}K", trim(v / 1e3))
    } else {
        grouped(v.round() as i64)
    }
}

fn percent(share: f64) -> String {
    let p = share * 100.0;
    if p > 0.0 && p < 0.1 { "<0.1%".into() } else { format!("{p:.1}%") }
}

// ---- data -------------------------------------------------------------------------------

/// `server.getUsageSummary`'s input for the chosen window (makeWindow): calendar days in the
/// viewer's zone, or the rolling 24 hours to the current minute.
pub(super) fn input(u: &Ui, now: i64, aliases: &Value) -> Value {
    let day = |at: i64| day_of(u.zone.wall(at));
    if u.window_days == 1 {
        let until = now - now.rem_euclid(MINUTE);
        let since = until - DAY;
        return json!({"sinceDay": day(since), "untilDay": day(until), "timeZone": u.zone_name, "modelAliases": aliases, "resolution": "hour", "sinceTime": iso_of(since), "untilTime": iso_of(until)});
    }
    json!({"sinceDay": day_of(u.zone.wall(now) - (u.window_days - 1) * DAY), "untilDay": day(now), "timeZone": u.zone_name, "modelAliases": aliases, "resolution": "day"})
}

/// usageScanSettingsAtom: prices, mappings and the Cursor keychain opt-in rescan an environment
/// when they change, and `usageModelFilter` rides along in its request.
fn scan_settings(config: Option<&Value>) -> Value {
    let settings = config.map(|c| &c["settings"]).unwrap_or(&Value::Null);
    json!([settings["usagePriceOverrides"], settings["usageModelAliases"], settings["cursorKeychainUsageEnabled"], settings["usageModelFilter"]])
}

/// usageSummaryInput: each environment is asked with its own model filter.
fn scan_input(requested: &Value, settings: &Value) -> Value {
    let mut input = requested.clone();
    if !settings[3].is_null() {
        input["modelFilter"] = settings[3].clone();
    }
    input
}

/// What a fetched summary answers; the rolling hour bounds alone don't refetch.
fn window_key(input: &Value) -> Value {
    json!([input["sinceDay"], input["resolution"], input["timeZone"], input["modelAliases"]])
}

fn env_net<'a>(s: &'a mut App, key: &str) -> Option<&'a mut s5_net::Net> {
    crate::env::source_mut(s, key).and_then(|src| src.net())
}

fn config<'a>(s: &'a App, key: &str) -> Option<&'a Value> {
    crate::env::source(s, key).and_then(|src| src.state().config.as_ref())
}

fn scan_at(s: &mut App, key: &str) -> usize {
    let scans = &mut ui(s).scans;
    match scans.iter().position(|x| x.key == key) {
        Some(at) => at,
        None => {
            scans.push(Scan { key: key.to_string(), ..Scan::default() });
            scans.len() - 1
        }
    }
}

/// Every environment is scanned, selected or not, like usageByWindowAtom; the selection only
/// picks which answers merge.
pub fn poll(s: &mut App) -> bool {
    if !matches!(s.route, Route::Usage) {
        return false;
    }
    let mut changed = super::usage_limits::poll(s) | super::usage_tags::poll(s);
    if !s.ui.pages.usage.preferences_read {
        let saved = s.client_settings.values.get(PREFERENCES).cloned().unwrap_or(Value::Null);
        apply_preferences(ui(s), &saved);
        changed = true;
    }
    let wanted = input(&s.ui.pages.usage, s.now_ms, &model_aliases(s));
    if s.ui.pages.usage.requested.as_ref().map(window_key) != Some(window_key(&wanted)) {
        ui(s).requested = Some(wanted);
        changed = true;
    }
    for key in crate::env::keys(s) {
        changed |= poll_scan(s, &key);
    }
    for (key, id) in std::mem::take(&mut ui(s).cursor_enabling) {
        match env_net(s, &key).map(|n| n.take_reply(id)) {
            Some(None) => ui(s).cursor_enabling.push((key, id)),
            reply => {
                if matches!(reply, Some(Some(Ok(_)))) {
                    refresh_usage(s, false);
                    super::usage_limits::refresh(s, false);
                }
                changed = true;
            }
        }
    }
    changed | remerge(s)
}

/// One environment's `server.getUsageSummary` (the usageSummary atom): an answer with sources
/// still refreshing shows and is asked again with `awaitRefresh`, and a failed second ask keeps it.
fn poll_scan(s: &mut App, key: &str) -> bool {
    let at = scan_at(s, key);
    let now = s.now_ms;
    let mut changed = false;
    if let Some(id) = s.ui.pages.usage.scans[at].rates
        && env_net(s, key).is_none_or(|n| n.take_reply(id).is_some())
    {
        let scan = &mut ui(s).scans[at];
        scan.rates = None;
        scan.sent = None;
        changed = true;
    }
    if let Some((id, window)) = s.ui.pages.usage.scans[at].pending.clone()
        && let Some(reply) = env_net(s, key).and_then(|n| n.take_reply(id))
    {
        changed = true;
        let scan = &mut ui(s).scans[at];
        scan.pending = None;
        let awaited = std::mem::take(&mut scan.awaiting);
        let mut again = None;
        match reply {
            Ok(summary) => {
                let refreshing = summary["sources"].as_array().into_iter().flatten().any(|src| src["refreshing"] == true);
                if refreshing && !awaited {
                    again = scan.sent.as_ref().map(|(input, _)| {
                        let mut input = input.clone();
                        input["awaitRefresh"] = json!(true);
                        input
                    });
                } else {
                    scan.refreshing = false;
                }
                scan.summary = Some(summary);
                scan.answered = Some(window.clone());
                scan.at = now;
                scan.error = None;
            }
            Err(_) if awaited => scan.refreshing = false,
            Err(_) => {
                scan.summary = None;
                scan.answered = None;
                scan.error = Some("This environment could not report usage.".into());
                scan.refreshing = false;
            }
        }
        scan.generation += 1;
        if let Some(input) = again
            && let Some(net) = env_net(s, key)
        {
            let id = net.request("server.getUsageSummary", input);
            let scan = &mut ui(s).scans[at];
            scan.pending = Some((id, window));
            scan.awaiting = true;
        }
    }
    let Some(requested) = s.ui.pages.usage.requested.clone() else { return changed };
    let settings = scan_settings(config(s, key));
    let want = (scan_input(&requested, &settings), settings);
    let connected = crate::env::status(s, key) == "Connected";
    let scan = &s.ui.pages.usage.scans[at];
    if !connected && scan.refreshing {
        ui(s).scans[at].refreshing = false;
        changed = true;
    }
    let scan = &s.ui.pages.usage.scans[at];
    if !connected || scan.rates.is_some() || scan.sent.as_ref() == Some(&want) {
        return changed;
    }
    let window = window_key(&want.0);
    if ui(s).scans[at].show(&window, &want.1, now) {
        ui(s).scans[at].sent = Some(want);
        return true;
    }
    match env_net(s, key) {
        Some(net) => {
            let id = net.request("server.getUsageSummary", want.0.clone());
            let scan = &mut ui(s).scans[at];
            scan.pending = Some((id, window));
            scan.awaiting = false;
            scan.error = None;
        }
        None if key == crate::env::LOCAL && !crate::host::live() => {
            let recorded = super::fixture("usage.json").map(|f| f["summary"].clone());
            let scan = &mut ui(s).scans[at];
            if scan.summary.is_none() {
                scan.summary = recorded;
            }
            scan.answered = Some(window);
            scan.generation += 1;
        }
        None => {}
    }
    ui(s).scans[at].sent = Some(want);
    true
}

/// The metric, window and hidden providers survive a restart (usagePagePreferences).
const PREFERENCES: &str = "usagePagePreferences";

fn apply_preferences(u: &mut Ui, saved: &Value) {
    u.preferences_read = true;
    if let Some(metric) = ["cost", "tokens", "limits"].into_iter().find(|m| saved["metric"] == *m) {
        u.metric = metric;
    }
    if let Some(days) = saved["windowDays"].as_i64().filter(|d| [1, 7, 30, 90].contains(d)) {
        u.window_days = days;
    }
    if let Some(hidden) = saved["hiddenProviders"].as_array() {
        u.hidden = PROVIDERS.iter().map(|p| p.0).filter(|k| hidden.iter().any(|h| h == k)).collect();
    }
}

fn preferences(u: &Ui) -> Value {
    json!({"metric": u.metric, "windowDays": u.window_days, "hiddenProviders": u.hidden})
}

fn save_preferences(s: &mut App) {
    let value = preferences(&s.ui.pages.usage);
    if s.client_settings.values.get(PREFERENCES) != Some(&value) {
        s.client_settings.values.insert(PREFERENCES.into(), value);
        s.client_settings.save();
    }
}

/// The header's Refresh: limits re-read provider status on every selected environment; usage
/// refreshes model rates and then rescans the window from now, keeping the current figures up.
fn refresh(s: &mut App) {
    if refreshing(&s.ui.pages.usage) {
        return;
    }
    if s.ui.pages.usage.metric == "limits" {
        super::usage_limits::refresh(s, false);
        return;
    }
    let now = input(&s.ui.pages.usage, s.now_ms, &model_aliases(s));
    ui(s).requested = Some(now);
    refresh_usage(s, true);
}

/// refreshUsage: each selected connected environment refreshes its model rates, then rescans.
fn refresh_usage(s: &mut App, from_header: bool) {
    for key in selected_environments(s) {
        if crate::env::status(s, &key) != "Connected" {
            continue;
        }
        let Some(net) = env_net(s, &key) else { continue };
        let id = net.request("server.refreshUsageRates", json!({}));
        let at = scan_at(s, &key);
        let scan = &mut ui(s).scans[at];
        scan.rates = Some(id);
        scan.refreshing |= from_header;
    }
}

/// A saved setting changed on `key`: ask it again (the usageSummary atom's refreshTrigger).
pub(super) fn rescan(s: &mut App, key: &str) {
    let at = scan_at(s, key);
    ui(s).scans[at].sent = None;
}

fn refreshing(u: &Ui) -> bool {
    u.scans.iter().any(|x| x.refreshing) || super::usage_limits::busy(&u.limits)
}

/// UsagePage's `refreshingUsage`: the header's usage refresh, which reads every figure as stale.
fn refreshing_usage(u: &Ui) -> bool {
    u.metric != "limits" && u.scans.iter().any(|x| x.refreshing)
}

fn is_selected(u: &Ui, key: &str) -> bool {
    u.environments.as_ref().is_none_or(|c| c.iter().any(|k| k == key))
}

/// The environments the filter leaves in, in the app's order (`selectedEnvironments`).
pub(super) fn selected_environments(s: &App) -> Vec<String> {
    let chosen = &s.ui.pages.usage.environments;
    crate::env::keys(s).into_iter().filter(|k| chosen.as_ref().is_none_or(|c| c.contains(k))).collect()
}

/// UsageEnvironmentFilter's checkbox: checking every environment again reads as "All".
pub(super) fn toggle_environment(all: &[String], selected: &[String], key: &str, on: bool) -> Option<Vec<String>> {
    let mut next: Vec<String> = selected.iter().filter(|k| *k != key).cloned().collect();
    if on {
        next.push(key.to_string());
    }
    (next.len() != all.len()).then_some(next)
}

const CHATGPT_USAGE_URL: &str = "https://chatgpt.com/#settings/Usage";

/// usesChatGptSharing: a ChatGPT login that shares its plan with other apps.
fn shares_chatgpt(provider: &Value) -> bool {
    provider["auth"]["status"] == "authenticated" && provider["auth"]["subscriptionSharing"] == true
}

pub(super) const CURSOR_KEYCHAIN_COPY: &str = "Requires access to your Cursor login in macOS Keychain.";

/// needsCursorKeychainAccess: the scan asks for the keychain and Cursor is ready.
fn needs_cursor_keychain(summary: &Value, providers: &Value) -> bool {
    summary["sources"].as_array().into_iter().flatten().any(|src| src["action"] == "enableCursorKeychain")
        && providers.as_array().into_iter().flatten().any(|p| p["driver"] == "cursor" && p["status"] == "ready")
}

fn reads_cursor_account(summary: &Value) -> bool {
    summary["sources"].as_array().into_iter().flatten().any(|src| src["fingerprint"]["provider"] == "cursor" && src["fingerprint"]["hostId"] == "cursor.com")
}

/// EnvironmentUsageStatus: one environment's connection and its answer for the requested window.
struct EnvUsage<'a> {
    key: String,
    label: String,
    connected: bool,
    pending: bool,
    error: Option<String>,
    summary: Option<&'a Value>,
    needs_cursor: bool,
}

/// usageByWindowAtom: every environment; one still connecting is pending and a lost one reports
/// an error (resolveUsageAccess).
fn environments(s: &App) -> Vec<EnvUsage<'_>> {
    let u = &s.ui.pages.usage;
    let window = u.requested.as_ref().map(window_key);
    crate::env::keys(s)
        .into_iter()
        .map(move |key| {
            let status = crate::env::status(s, &key);
            let label = crate::env::label(s, &key);
            if status != "Connected" {
                let pending = status != "Not connected";
                let error = (!pending).then(|| "This environment is not connected.".to_string());
                return EnvUsage { key, label, connected: false, pending, error, summary: None, needs_cursor: false };
            }
            let scan = u.scans.iter().find(|x| x.key == key);
            let summary = scan.filter(|x| x.answered.is_some() && x.answered == window).and_then(|x| x.summary.as_ref());
            let error = scan.and_then(|x| x.error.clone());
            let pending = scan.is_none_or(|x| x.pending.is_some() || (summary.is_none() && x.error.is_none()));
            let providers = config(s, &key).map(|c| &c["providers"]).unwrap_or(&Value::Null);
            let needs_cursor = summary.is_some_and(|v| needs_cursor_keychain(v, providers));
            EnvUsage { key, label, connected: true, pending, error, summary, needs_cursor }
        })
        .collect()
}

/// UsagePage's `environments.length === 0`: no environment has reported its config yet.
fn no_environments(s: &App) -> bool {
    crate::env::keys(s).iter().all(|k| config(s, k).is_none())
}

fn selected_usage(s: &App) -> Vec<EnvUsage<'_>> {
    let u = &s.ui.pages.usage;
    environments(s).into_iter().filter(|e| is_selected(u, &e.key)).collect()
}

#[derive(Debug, PartialEq)]
enum Phase {
    Inactive,
    Loading,
    Stale,
    Partway(Vec<String>),
    Ready,
}

/// usageEnvironmentProgress
fn progress(e: &EnvUsage<'_>, refreshing: bool) -> Phase {
    if !e.connected || e.error.is_some() {
        return Phase::Inactive;
    }
    let Some(summary) = e.summary else { return Phase::Loading };
    if refreshing {
        return Phase::Stale;
    }
    if !e.pending {
        return Phase::Ready;
    }
    let mut providers: Vec<String> = Vec::new();
    for src in summary["sources"].as_array().into_iter().flatten().filter(|src| src["refreshing"] == true) {
        let provider = field(&src["fingerprint"], "provider").to_string();
        if !providers.contains(&provider) {
            providers.push(provider);
        }
    }
    if providers.is_empty() { Phase::Stale } else { Phase::Partway(providers) }
}

/// updatingProvidersLabel
fn updating(providers: &[String]) -> String {
    match providers {
        [one] => format!("Updating {}…", presentation(one).0),
        many => format!("Updating {} providers…", many.len()),
    }
}

/// UsageEnvironmentFilter's status beside each environment.
fn status_label(e: &EnvUsage<'_>, refreshing: bool) -> String {
    let outdated = e.summary.is_some_and(|v| !usage_merge::compatible(v["contractVersion"].as_i64().unwrap_or(0), usage_merge::CONTRACT_VERSION));
    if e.error.is_some() {
        return "Unavailable".into();
    }
    if outdated {
        return "Update required".into();
    }
    if !e.connected {
        return "Connecting…".into();
    }
    match progress(e, refreshing) {
        Phase::Loading => "Scanning…".into(),
        Phase::Stale => "Refreshing…".into(),
        Phase::Partway(providers) => updating(&providers),
        Phase::Inactive | Phase::Ready => "Ready".into(),
    }
}

/// useUsage's counts over the selected environments: (answered, still reporting).
fn reporting(selected: &[EnvUsage<'_>]) -> (usize, usize) {
    let answered = selected.iter().filter(|e| e.summary.is_some()).count();
    let still = selected.iter().filter(|e| e.summary.is_none() && e.error.is_none()).count();
    (answered, still)
}

/// usageLoading: whether every provider's figures wait, and which visible providers still update.
fn loading(selected: &[EnvUsage<'_>], refreshing: bool, showing_kept: bool, hidden: &[&str]) -> (bool, Vec<String>) {
    if showing_kept {
        return (true, Vec::new());
    }
    let mut every = false;
    let mut providers: Vec<String> = Vec::new();
    for e in selected {
        match progress(e, refreshing) {
            Phase::Loading | Phase::Stale => every = true,
            Phase::Partway(kinds) => {
                for kind in kinds {
                    if !hidden.contains(&kind.as_str()) && !providers.contains(&kind) {
                        providers.push(kind);
                    }
                }
            }
            Phase::Inactive | Phase::Ready => {}
        }
    }
    (every, providers)
}

/// UsageCoverageNotice: failed environments, excluded contracts and directories counted once.
fn coverage_lines(selected: &[EnvUsage<'_>], merged: &Value) -> Vec<String> {
    let mut lines: Vec<String> = selected.iter().filter_map(|e| Some(format!("{}: {}", e.label, e.error.as_ref()?))).collect();
    for e in selected {
        if let Some(m) = merged["mismatches"].as_array().into_iter().flatten().find(|m| m["environment"] == e.key.as_str()) {
            lines.push(usage_merge::mismatch_line(&e.label, &Mismatch { environment: e.key.clone(), server_behind: m["serverBehind"] == true }));
        }
    }
    let duplicates: Vec<&str> = merged["duplicates"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !duplicates.is_empty() {
        lines.push(format!("Counted once across environments sharing a transcript directory: {}", duplicates.join(", ")));
    }
    lines
}

/// mergeAnsweredUsage after withoutProviders, as one summary for `window`: the merged buckets,
/// every answer's visible sources, sessions per provider, and what the merge left out.
fn merged_summary(selected: &[EnvUsage<'_>], hidden: &[&str], window: &Value) -> Value {
    let shown = |provider: &Value| !hidden.contains(&provider.as_str().unwrap_or(""));
    let trimmed: Vec<_> = selected
        .iter()
        .filter_map(|e| {
            let mut summary = e.summary?.clone();
            if let Some(buckets) = summary["buckets"].as_array_mut() {
                buckets.retain(|b| shown(&b["provider"]));
            }
            if let Some(sources) = summary["sources"].as_array_mut() {
                sources.retain(|src| shown(&src["fingerprint"]["provider"]));
            }
            Some((e, summary))
        })
        .collect();
    let usages: Vec<_> = trimmed.iter().map(|(e, summary)| usage_merge::Usage { id: &e.key, label: &e.label, summary }).collect();
    let merged = usage_merge::merge_usage(&usages, usage_merge::CONTRACT_VERSION);
    let sources: Vec<Value> = trimmed.iter().flat_map(|(_, v)| v["sources"].as_array().cloned().unwrap_or_default()).collect();
    let sessions: serde_json::Map<String, Value> = merged.sessions.iter().map(|(p, n)| (p.clone(), json!(n))).collect();
    let mismatches: Vec<Value> = merged.mismatches.iter().map(|m| json!({"environment": m.environment, "serverBehind": m.server_behind})).collect();
    json!({
        "sinceDay": window["sinceDay"], "untilDay": window["untilDay"], "buckets": merged.buckets, "sources": sources,
        "sessions": sessions, "duplicates": merged.duplicates, "mismatches": mismatches,
    })
}

/// useUsage's `shown`: the selected environments' merged answers; while none has answered, the
/// last merge for this selection (`kept`), or nothing while they still scan.
fn remerge(s: &mut App) -> bool {
    let u = &s.ui.pages.usage;
    let Some(window) = u.requested.clone() else { return false };
    let selected = selected_usage(s);
    let answers: Vec<Value> = selected
        .iter()
        .map(|e| json!([e.key, u.scans.iter().find(|x| x.key == e.key).map_or(0, |x| x.generation), e.summary.is_some(), e.error, e.pending]))
        .collect();
    let from = json!([u.environments, u.hidden, window_key(&window), answers]).to_string();
    if from == u.merged_from {
        return false;
    }
    let selection = json!([u.environments, u.hidden]).to_string();
    let (answered, still) = reporting(&selected);
    let merged = merged_summary(&selected, &u.hidden, &window);
    let u = ui(s);
    u.merged_from = from;
    if answered > 0 {
        u.kept = Some((selection, merged.clone(), window.clone()));
        u.summary = Some(merged);
        u.answered = Some(window);
        u.showing_kept = false;
    } else if let Some((_, summary, kept_window)) = u.kept.as_ref().filter(|k| k.0 == selection) {
        u.summary = Some(summary.clone());
        u.answered = Some(kept_window.clone());
        u.showing_kept = true;
    } else {
        u.summary = (still == 0).then_some(merged);
        u.answered = Some(window);
        u.showing_kept = false;
    }
    true
}

/// cursorKeychainAccessEnvironments as (key, label): none while Cursor is hidden, or once any
/// selected environment reads a Cursor account, which already reports every machine's history.
fn cursor_access_environments(selected: &[EnvUsage<'_>], hidden: &[&str]) -> Vec<(String, String)> {
    if hidden.contains(&"cursor") || selected.iter().any(|e| e.summary.is_some_and(reads_cursor_account)) {
        return Vec::new();
    }
    selected.iter().filter(|e| e.needs_cursor).map(|e| (e.key.clone(), e.label.clone())).collect()
}

/// The selected environments that offer the Cursor keychain opt-in, as (key, label).
pub(super) fn cursor_access(s: &App) -> Vec<(String, String)> {
    cursor_access_environments(&selected_usage(s), &s.ui.pages.usage.hidden)
}

pub(super) fn cursor_enabling(s: &App, key: &str) -> bool {
    s.ui.pages.usage.cursor_enabling.iter().any(|e| e.0 == key)
}

/// CursorEnableButton: `server.updateSettings` turns `cursorKeychainUsageEnabled` on for `key`.
pub(super) fn enable_cursor(s: &mut App, key: &str) {
    if cursor_enabling(s, key) {
        return;
    }
    if let Some(net) = env_net(s, key) {
        let id = net.request("server.updateSettings", json!({"patch": {"cursorKeychainUsageEnabled": true}}));
        ui(s).cursor_enabling.push((key.to_string(), id));
    }
}

/// Scenes: `key` asked for the requested window, answered with `summary` or still scanning.
pub(super) fn scene_scan(s: &mut App, key: &str, summary: Option<Value>) {
    let Some(requested) = s.ui.pages.usage.requested.clone() else { return };
    let settings = scan_settings(config(s, key));
    let input = scan_input(&requested, &settings);
    let at = scan_at(s, key);
    let scan = &mut ui(s).scans[at];
    scan.answered = summary.is_some().then(|| window_key(&input));
    scan.summary = summary;
    scan.sent = Some((input, settings));
    scan.generation += 1;
    remerge(s);
}

fn scene_summary(s: &mut App, key: &str) -> Option<Value> {
    let at = scan_at(s, key);
    ui(s).scans[at].summary.clone()
}

/// The `usage-error` scene: this machine's scan failed with nothing kept, so the overview is
/// empty and the filter warns.
pub(super) fn error_scene(s: &mut App) {
    let at = scan_at(s, crate::env::LOCAL);
    let scan = &mut ui(s).scans[at];
    scan.summary = None;
    scan.answered = None;
    scan.error = Some("This environment could not report usage.".into());
    scan.generation += 1;
    ui(s).kept = None;
    remerge(s);
}

/// The `usage-cursor-keychain` scenes: Cursor is ready and this machine's scan asks for the keychain.
pub(super) fn cursor_scene(s: &mut App) {
    if let crate::data::Source::Fixture(state) = &mut s.src
        && let Some(providers) = state.config.as_mut().and_then(|c| c["providers"].as_array_mut())
    {
        for p in providers {
            if p["driver"] == "cursor" {
                p["status"] = json!("ready");
            }
        }
    }
    let mut summary = scene_summary(s, crate::env::LOCAL);
    if let Some(sources) = summary.as_mut().and_then(|v| v["sources"].as_array_mut()) {
        sources.push(json!({
            "fingerprint": {"provider": "cursor", "hostId": "devbox"}, "status": "missing", "scannedFiles": 0, "skippedFiles": 0,
            "malformedRecords": 0, "distinctSessions": 0, "message": null, "action": "enableCursorKeychain",
        }));
    }
    scene_scan(s, crate::env::LOCAL, summary);
}

/// A second environment for the multi-environment scenes: the `usage-limits-environments`
/// "Studio Mac", answering with this machine's summary changed by `edit`, or still scanning.
pub(super) fn studio_scene(s: &mut App, edit: Option<fn(&mut Value)>) {
    super::usage_limits::environments_scene(s);
    let summary = edit.and_then(|edit| {
        let mut summary = scene_summary(s, crate::env::LOCAL)?;
        edit(&mut summary);
        Some(summary)
    });
    scene_scan(s, "studio", summary);
}

/// The `usage-cursor-keychain-environments` scene: both machines ask for Cursor's keychain;
/// Studio Mac copies this machine's config after Cursor turned ready.
pub(super) fn cursor_environments_scene(s: &mut App) {
    cursor_scene(s);
    studio_scene(s, Some(|summary| {
        for src in summary["sources"].as_array_mut().into_iter().flatten() {
            src["fingerprint"]["hostId"] = json!("studio");
        }
    }));
}

/// The `usage-provider-loading` scenes: this machine answered with Claude Code still refreshing
/// and Grok Build not read yet, and is asked again with `awaitRefresh`.
pub(super) fn updating_scene(s: &mut App) {
    let mut summary = scene_summary(s, crate::env::LOCAL);
    if let Some(sources) = summary.as_mut().and_then(|v| v["sources"].as_array_mut()) {
        for src in sources.iter_mut().filter(|src| src["fingerprint"]["provider"] == "claude") {
            src["refreshing"] = json!(true);
        }
        sources.push(json!({
            "fingerprint": {"provider": "grok", "hostId": "devbox", "resolvedHomePath": "/home/ubuntu/.grok", "volumeId": "66306:1"}, "status": "ok",
            "scannedFiles": 0, "skippedFiles": 0, "malformedRecords": 0, "distinctSessions": 0, "message": null, "refreshing": true,
        }));
    }
    scene_scan(s, crate::env::LOCAL, summary);
    let at = scan_at(s, crate::env::LOCAL);
    let scan = &mut ui(s).scans[at];
    scan.pending = scan.answered.clone().map(|window| (0, window));
    scan.awaiting = true;
    scan.generation += 1;
    remerge(s);
}

/// The `usage-chart-tooltip-partial` scene: the provider loading scene's chart hovered on its
/// last day with figures, where updating providers' values and the total read as partial.
pub(super) fn tooltip_scene(s: &mut App) {
    updating_scene(s);
    let u = ui(s);
    let Some(summary) = &u.summary else { return };
    let m = merge(summary, &u.hidden, u.answered.as_ref(), None);
    let Some(index) = m.periods.iter().rposition(|p| m.by_period.contains_key(p)) else { return };
    let w = 640.0;
    u.plot_w = w;
    u.hover = Some((index, index as f32 / (m.periods.len().max(2) - 1) as f32 * w, 48.0));
}

/// The `usage-no-environments` scene: this machine is still connecting and has no config yet.
pub(super) fn no_environments_scene(s: &mut App) {
    if let crate::data::Source::Fixture(state) = &mut s.src {
        state.config = None;
        state.conn = s5_net::Conn::Connecting;
    }
    ui(s).menu = Some("environments");
    remerge(s);
}

/// The `usage-offline` scene: only a saved environment that is not connected is selected.
pub(super) fn offline_scene(s: &mut App) {
    s.ui.env.saved.push(crate::env::Saved { id: "lab".into(), label: "Lab server".into(), base: "https://lab.example".into(), bearer: "scene".into() });
    ui(s).environments = Some(vec!["lab".into()]);
    remerge(s);
}

/// CursorEnableRow: the overview's Cursor line, with an inline "Enable" in place of figures.
fn cursor_enable_row(s: &App, t: &T, key: &str, label: &str, show_environment: bool) -> El {
    let mut enable = row()
        .role("button")
        .label(format!("Enable Cursor usage from {label}"))
        .help(CURSOR_KEYCHAIN_COPY)
        .child(text("Enable").font(theme::SM_MEDIUM).color(t.c.foreground));
    let key = key.to_string();
    enable = if cursor_enabling(s, &key) { enable.opacity(0.64) } else { enable.cursor(rune::Cursor::Pointer).focusable(true).on_press(move |s: &mut App| enable_cursor(s, &key)) };
    let name = if show_environment { format!("Cursor · {label}") } else { "Cursor".to_string() };
    row()
        .w_full()
        .gap(8.0)
        .min_h(20.0)
        .center_y()
        .child(el().size(8.0, 8.0).radius(radius::FULL).fill(color(t, "cursor")))
        .child(provider_icon(t, "cursor", 16.0))
        .child(text(name).font(theme::TEXT_SM).color(t.c.foreground).truncate().shrink())
        .child(spacer())
        .child(enable)
}

/// UsagePage's metric and period shortcuts: `c`/`t`/`l` and Mod+Shift+1..4 by default.
fn shortcut(s: &App, e: &rune::KeyEvent) -> Option<&'static str> {
    const COMMANDS: &[&str] = &["usage.cost", "usage.tokens", "usage.limits", "usage.period.day", "usage.period.week", "usage.period.month", "usage.period.quarter"];
    let find = |e: &rune::KeyEvent| COMMANDS.iter().copied().find(|c| crate::panel::shortcut(s, e, c));
    find(e).or_else(|| {
        // Shift turns the digit into its symbol on most layouts; bindings name the digit.
        let symbol = match &e.key {
            rune::Key::Text(t) => t.chars().next()?,
            rune::Key::Command(c) => *c,
            _ => return None,
        };
        let digit = unshifted(symbol)?;
        let key = if matches!(e.key, rune::Key::Command(_)) { rune::Key::Command(digit) } else { rune::Key::Text(digit.to_string()) };
        find(&rune::KeyEvent { key, mods: e.mods.clone() })
    })
}

fn unshifted(c: char) -> Option<char> {
    let at = "!@#$%^&*()".find(c)?;
    "1234567890".chars().nth(at)
}

pub fn close(s: &mut App) {
    ui(s).menu = None;
    super::usage_limits::closed(&mut ui(s).limits);
    let route = s.ui.settings.return_route.take().unwrap_or(Route::Home { project_id: None });
    s.navigate(route);
}

pub fn on_key(s: &mut App, e: &rune::KeyEvent, _: &mut Tx) -> bool {
    if !matches!(s.route, Route::Usage) {
        return false;
    }
    if super::usage_prices::on_key(s, e) || super::usage_tags::on_key(s, e) || super::usage_limits::on_key(s, e) {
        return true;
    }
    if !matches!(e.key, rune::Key::Escape) {
        let u = &s.ui.pages.usage;
        // Like resolveUsageShortcut: not while a dialog, menu or the palette has the keys.
        if u.prices.open || u.model.is_some() || u.menu.is_some() || u.tags.target.is_some() || u.limits.confirm.is_some() || s.ui.shell.palette.open {
            return false;
        }
        let Some(command) = shortcut(s, e) else { return false };
        let u = ui(s);
        match command {
            "usage.cost" => u.metric = "cost",
            "usage.tokens" => u.metric = "tokens",
            "usage.limits" => u.metric = "limits",
            _ if u.metric == "limits" => {}
            "usage.period.day" => u.window_days = 1,
            "usage.period.week" => u.window_days = 7,
            "usage.period.month" => u.window_days = 30,
            _ => u.window_days = 90,
        }
        save_preferences(s);
        return true;
    }
    let u = ui(s);
    if u.model.take().is_none() && u.menu.take().is_none() {
        close(s);
    }
    true
}

#[derive(Default, Clone, Copy)]
struct Totals {
    cost: f64,
    tokens: f64,
}

/// One breakdown row (ModelTotals): a transcript name, or every name tagged as one catalog entry.
#[derive(Default)]
struct ModelRow {
    /// `kind\0apiProvider\0model`, or `kind\0tag\0pricedAs` for a tagged row.
    key: String,
    kind: &'static str,
    model: String,
    apis: Vec<String>,
    priced_as: Option<String>,
    unpriced_only: bool,
    tagged: bool,
    totals: Totals,
    records: f64,
    unpriced_records: f64,
}

impl ModelRow {
    /// Only meaningful when exactly one gateway fed the row.
    fn api(&self) -> &str {
        match self.apis.as_slice() {
            [one] => one,
            _ => "",
        }
    }

    /// isModelCostUnknown: every record lacks rates, so the cost is unknown rather than zero.
    fn cost_unknown(&self) -> bool {
        self.records > 0.0 && self.unpriced_records >= self.records
    }
}

#[derive(Default)]
struct Merged {
    cost: f64,
    tokens: f64,
    uncached: f64,
    cached: f64,
    creation: f64,
    output: f64,
    savings: f64,
    records: f64,
    unpriced: f64,
    /// Input, cache read, cache write, output, unsplit.
    category: [f64; 5],
    fast: f64,
    ultrafast: f64,
    premium: f64,
    unpriced_tokens: f64,
    /// Distinct sessions over the visible providers (merged.sessions).
    sessions: i64,
    hourly: bool,
    clock: Clock,
    providers: Vec<(&'static str, Totals, i64)>,
    models: Vec<ModelRow>,
    periods: Vec<String>,
    by_period: HashMap<String, HashMap<&'static str, Totals>>,
}

/// Totals over `summary`'s visible providers, or over one breakdown row's buckets when `model`
/// names one. `window` is the request, whose rolling hours the hourly periods follow.
fn merge(summary: &Value, hidden: &[&str], window: Option<&Value>, model: Option<&str>) -> Merged {
    let mut m = Merged::default();
    let mut models: HashMap<String, ModelRow> = HashMap::new();
    let mut providers: HashMap<&'static str, Totals> = HashMap::new();
    let hourly = summary["buckets"].as_array().into_iter().flatten().any(|b| b["hourStart"].is_string());
    for b in summary["buckets"].as_array().into_iter().flatten() {
        let Some(&(kind, _, _)) = PROVIDERS.iter().find(|p| p.0 == field(b, "provider")) else { continue };
        if hidden.contains(&kind) {
            continue;
        }
        // A tagged bucket keys by what the user said it is, so one model under several gateway
        // names collapses into one row.
        let tagged_as = b["pricedAs"].as_str().filter(|_| b["costSource"] == "userTagged");
        let model_key = match tagged_as {
            Some(priced_as) => format!("{kind}\0tag\0{priced_as}"),
            None => format!("{kind}\0{}\0{}", field(b, "apiProvider"), field(b, "model")),
        };
        if model.is_some_and(|k| k != model_key) {
            continue;
        }
        let n = |k: &str| b["totals"][k].as_f64().unwrap_or(0.0);
        let (uncached, cached, creation, output) = (n("uncachedInputTokens"), n("cachedInputTokens"), n("cacheCreationTokens"), n("outputTokens"));
        let total = Totals { cost: b["costUsd"].as_f64().unwrap_or(0.0), tokens: uncached + cached + creation + output };
        m.cost += total.cost;
        m.tokens += total.tokens;
        m.uncached += uncached;
        m.cached += cached;
        m.creation += creation;
        m.output += output;
        m.savings += b["cacheSavingsUsd"].as_f64().unwrap_or(0.0);
        m.records += b["records"].as_f64().unwrap_or(0.0);
        let (records, unpriced) = (b["records"].as_f64().unwrap_or(0.0), b["unpricedRecords"].as_f64().unwrap_or(0.0));
        m.unpriced += unpriced;
        if records > 0.0 {
            m.unpriced_tokens += total.tokens * unpriced / records;
        }
        let cat = &b["categoryCostUsd"];
        let split: Vec<f64> = ["input", "cacheRead", "cacheWrite", "output"].iter().map(|k| cat[*k].as_f64().unwrap_or(0.0)).collect();
        for (slot, v) in split.iter().enumerate() {
            m.category[slot] += v;
        }
        m.category[4] += (total.cost - split.iter().sum::<f64>()).max(0.0);
        m.fast += b["fastCostUsd"].as_f64().unwrap_or(0.0);
        m.ultrafast += b["ultrafastCostUsd"].as_f64().unwrap_or(0.0);
        m.premium += b["speedPremiumUsd"].as_f64().unwrap_or(0.0);
        let add = |t: &mut Totals| {
            t.cost += total.cost;
            t.tokens += total.tokens;
        };
        add(providers.entry(kind).or_default());
        let entry = models.entry(model_key.clone()).or_insert_with(|| ModelRow {
            key: model_key,
            kind,
            model: tagged_as.unwrap_or(field(b, "model")).to_string(),
            priced_as: b["pricedAs"].as_str().map(str::to_string),
            unpriced_only: true,
            ..ModelRow::default()
        });
        let api = field(b, "apiProvider");
        if !api.is_empty() && !entry.apis.iter().any(|a| a == api) {
            entry.apis.push(api.to_string());
        }
        entry.unpriced_only &= b["costSource"] == "unpriced";
        entry.tagged |= b["costSource"] == "userTagged";
        entry.records += records;
        entry.unpriced_records += unpriced;
        add(&mut entry.totals);
        let period = if hourly { iso_ms(field(b, "hourStart")).map(iso_of).unwrap_or_default() } else { field(b, "day").to_string() };
        add(m.by_period.entry(period).or_default().entry(kind).or_default());
    }
    let mut sessions: HashMap<&str, i64> = HashMap::new();
    match summary["sessions"].as_object() {
        Some(merged) => {
            for (provider, n) in merged {
                sessions.insert(provider, n.as_i64().unwrap_or(0));
            }
        }
        None => {
            for src in summary["sources"].as_array().into_iter().flatten() {
                *sessions.entry(field(&src["fingerprint"], "provider")).or_default() += src["distinctSessions"].as_i64().unwrap_or(0);
            }
        }
    }
    m.sessions = sessions.iter().filter(|(p, _)| !hidden.contains(p)).map(|(_, n)| n).sum();
    m.providers = PROVIDERS.iter().filter_map(|p| providers.get(p.0).map(|t| (p.0, *t, sessions.get(p.0).copied().unwrap_or(0)))).collect();
    m.models = models.into_values().collect();
    m.models.sort_by(|a, b| b.totals.cost.total_cmp(&a.totals.cost).then(b.totals.tokens.total_cmp(&a.totals.tokens)));
    let bounds = window.filter(|w| w["resolution"] == "hour").and_then(|w| Some((iso_ms(w["sinceTime"].as_str()?)?, iso_ms(w["untilTime"].as_str()?)?)));
    m.hourly = hourly || bounds.is_some();
    if let Some((since, until)) = bounds {
        // enumerateHourStarts: rolling buckets start on the window's minute, not the hour.
        m.periods = (0..).map(|k| since + k * HOUR).take_while(|at| *at < until).map(iso_of).collect();
    } else if hourly {
        let until = summary["buckets"].as_array().into_iter().flatten().filter_map(|b| iso_ms(field(b, "hourStart"))).max().unwrap_or(0);
        m.periods = (0..24).rev().map(|k| iso_of(until - k * HOUR)).collect();
    } else if let (Some(a), Some(b)) = (day_ms(field(summary, "sinceDay")), day_ms(field(summary, "untilDay"))) {
        m.periods = (0..=((b - a) / DAY)).map(|k| day_of(a + k * DAY)).collect();
    }
    m
}

/// A period's axis and table label: its local hour, or its day.
fn period_label(period: &str, clock: Clock) -> String {
    match iso_ms(period).filter(|_| period.contains('T')) {
        Some(at) => clock.hour(at),
        None => day_short(period),
    }
}

// ---- view -------------------------------------------------------------------------------

pub fn view(s: &App, cx: &mut rune::Cx<App>, t: &T) -> El {
    let u = &s.ui.pages.usage;
    let mut overlays = Vec::new();
    let header = header(s, t, &mut overlays);
    let mut page = col().w_full().max_w(1024.0).px(24.0).pt(24.0).pb(48.0).gap(24.0);
    let selected = selected_usage(s);
    let noun = if u.metric == "limits" { "limits" } else { "usage" };
    page = if selected.is_empty() {
        page.child(muted_line(t, &if no_environments(s) { format!("Connect an environment to see {noun}.") } else { format!("Select an environment to see {noun}.") }))
    } else if u.metric == "limits" {
        page.child(super::usage_limits::view(s, cx, t, &mut overlays))
    } else if let Some(summary) = &u.summary {
        if selected.iter().any(|e| e.connected) {
            overview(s, t, summary, page, &mut overlays)
        } else {
            let mut lines = col().w_full().py(48.0).gap(8.0).items_center();
            for e in &selected {
                let prefix = if selected.len() > 1 { format!("{}: ", e.label) } else { String::new() };
                let line = format!("{prefix}{}", e.error.as_deref().unwrap_or(""));
                lines = lines.child(text(line).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap().align(rune::Align::Center));
            }
            page.child(lines)
        }
    } else {
        skeleton(t, page)
    };
    if let (Some(key), Some(summary), false) = (&u.model, &u.summary, u.metric == "limits") {
        model_dialog(s, t, summary, key, &mut overlays);
    }
    super::usage_prices::view(s, t, &mut overlays);
    super::usage_tags::view(s, t, &mut overlays);
    let body = kit::scroll_area(
        "usage-scroll",
        col().key("usage-scroll").grow(1.0).min_h(0.0).w_full().items_center().scroll(ScrollSpec { stick_bottom: false }).child(page),
        t,
    );
    let modal = col()
        .key("usage-modal")
        .padding(1.0)
        .modal(true, close)
        .w_full()
        .max_w(1280.0)
        .h_full()
        .max_h(960.0)
        .radius(16.0)
        .clip_rounded(16.0)
        .border(1.0, t.c.border)
        .shadow(25.0, 50.0, Color::BLACK.alpha(0.4))
        .focusable(true)
        .role("dialog")
        .label("Usage")
        .fill(t.c.background)
        .child(header)
        .child(body);
    rune_kit::root(
        crate::kit::motion::scrim(t).padding(40.0).center().child(crate::kit::motion::popup(modal)),
        overlays,
    )
    .transition(rune::Transition::none())
    .absolute(0.0, 0.0)
}

pub(super) fn muted_line(t: &T, line: &str) -> El {
    text(line.to_string()).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap()
}

fn header(s: &App, t: &T, overlays: &mut Vec<El>) -> El {
    let u = &s.ui.pages.usage;
    let k = kit::theme(t);
    let sep = || text("/").font(theme::SM_MEDIUM).color(t.c.icon_muted);
    let crumb = |label: String| {
        row()
            .gap(4.0)
            .center_y()
            .cursor(rune::Cursor::Pointer)
            .child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground))
            .child(icon(&i::CHEVRON_DOWN, 14.0).color(t.c.muted_foreground).opacity(0.6))
    };
    let visible: Vec<&str> = PROVIDERS.iter().map(|p| p.0).filter(|k| !u.hidden.contains(k)).collect();
    let provider_label = match visible.as_slice() {
        [] => "No providers".into(),
        [one] => presentation(one).0.to_string(),
        v if v.len() == PROVIDERS.len() => "All providers".into(),
        v => format!("{} providers", v.len()),
    };
    let toggle = |name: &'static str| {
        move |s: &mut App| {
            let u = ui(s);
            u.menu = if u.menu == Some(name) { None } else { Some(name) };
        }
    };
    let mut providers = Menu::new("usage-providers")
        .trigger(crumb(provider_label).on_press(toggle("providers")))
        .placement(Placement::BottomStart)
        .min_width(200.0)
        .open(u.menu == Some("providers"))
        .on_close(|s: &mut App| ui(s).menu = None)
        .item(MenuItem::new("All providers").checked(u.hidden.is_empty()).on_select(|s: &mut App| {
            let u = ui(s);
            u.hidden = if u.hidden.is_empty() { PROVIDERS.iter().map(|p| p.0).collect() } else { Vec::new() };
            u.menu = Some("providers");
            save_preferences(s);
        }))
        .separator();
    for &(kind, label, _) in PROVIDERS {
        providers = providers.item(MenuItem::new(label).checked(!u.hidden.contains(&kind)).on_select(move |s: &mut App| {
            let u = ui(s);
            if let Some(at) = u.hidden.iter().position(|h| *h == kind) {
                u.hidden.remove(at);
            } else {
                u.hidden.push(kind);
            }
            // A checkbox item keeps the menu open (closeOnClick={false}).
            u.menu = Some("providers");
            save_preferences(s);
        }));
    }
    let show_status = u.metric != "limits";
    let all = environments(s);
    let selected = selected_usage(s);
    let env_label = match (&u.environments, selected.as_slice()) {
        (None, _) => "All environments".to_string(),
        (Some(_), [one]) => one.label.clone(),
        (Some(_), many) => format!("{} environments", many.len()),
    };
    let merged = u.summary.as_ref().unwrap_or(&Value::Null);
    let issue = show_status && (selected.iter().any(|e| e.error.is_some()) || merged["mismatches"].as_array().is_some_and(|m| !m.is_empty()));
    let env_trigger = row()
        .gap(4.0)
        .center_y()
        .cursor(rune::Cursor::Pointer)
        .child(text(env_label).font(theme::SM_MEDIUM).color(t.c.muted_foreground))
        .child(if issue {
            icon(&i::CIRCLE_ALERT, 14.0).color(t.c.warning_foreground).role("img").label("Some environments could not report usage")
        } else {
            icon(&i::CHEVRON_DOWN, 14.0).color(t.c.muted_foreground).opacity(0.6)
        });
    let mut envs = Menu::new("usage-environments")
        .trigger(env_trigger.on_press(toggle("environments")))
        .placement(Placement::BottomStart)
        .min_width(220.0)
        .open(u.menu == Some("environments"))
        .on_close(|s: &mut App| ui(s).menu = None)
        .item(MenuItem::new("All environments").checked(u.environments.is_none()).on_select(|s: &mut App| {
            let u = ui(s);
            u.environments = if u.environments.is_none() { Some(Vec::new()) } else { None };
            u.menu = Some("environments");
        }))
        .separator();
    let keys: Vec<String> = all.iter().map(|e| e.key.clone()).collect();
    let checked_keys: Vec<String> = selected.iter().map(|e| e.key.clone()).collect();
    let rescanning = refreshing_usage(u);
    for e in &all {
        let checked = checked_keys.contains(&e.key);
        let (keys, checked_keys, key) = (keys.clone(), checked_keys.clone(), e.key.clone());
        let mut item = MenuItem::new(e.label.clone()).checked(checked).on_select(move |s: &mut App| {
            let u = ui(s);
            u.environments = toggle_environment(&keys, &checked_keys, &key, !checked);
            u.menu = Some("environments");
        });
        if show_status {
            let ink = if e.error.is_some() { t.c.destructive } else { t.c.muted_foreground };
            item = item.trailing(text(status_label(e, rescanning)).font(theme::TEXT_XS).color(ink).ml(12.0));
        }
        envs = envs.item(item);
    }
    let note = |line: String| text(line).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap();
    if no_environments(s) {
        envs = envs.row(col().max_w(280.0).px(8.0).py(8.0).child(note("No environments connected.".into())));
    }
    let can_read = selected.iter().any(|e| e.connected);
    let (answered, still) = reporting(&selected);
    if show_status && answered > 0 && still > 0 {
        envs = envs.row(col().max_w(280.0).px(8.0).py(8.0).child(note("Totals are partial while selected environments scan.".into())));
    }
    let coverage = if show_status { coverage_lines(&selected, merged) } else { Vec::new() };
    if !coverage.is_empty() {
        let mut notice = col().max_w(280.0).gap(4.0).px(8.0).py(8.0);
        for line in coverage {
            notice = notice.child(note(line));
        }
        envs = envs.separator().row(notice);
    }
    envs = envs.separator().item(MenuItem::new("Model prices").icon(&i::SLIDERS_HORIZONTAL).on_select(|s: &mut App| {
        ui(s).menu = None;
        super::usage_prices::open(s, None);
    }));
    let mut bar = row()
        .key("usage-header")
        .w_full()
        .min_h(48.0)
        .shrink()
        .pl(20.0)
        .pr(8.0)
        .gap(4.0)
        .center_y()
        .child(
            row()
                .gap(12.0)
                .center_y()
                .shrink()
                .child(text("Usage").font(theme::SM_MEDIUM).color(t.c.muted_foreground))
                .child(sep())
                .child(envs.build(&k, overlays))
                .child(sep())
                .child(providers.build(&k, overlays)),
        );
    let limits = u.metric == "limits";
    if !limits && let Some(summary) = &u.summary {
        let bounds = u.answered.as_ref().filter(|r| r["resolution"] == "hour").and_then(|r| Some((iso_ms(r["sinceTime"].as_str()?)?, iso_ms(r["untilTime"].as_str()?)?)));
        let label = match bounds {
            Some((since, until)) => format!("{} to {}", clock(s).date_hour(since), clock(s).date_hour(until)),
            None if u.window_days == 1 => "Past 24 hours".to_string(),
            None => format!("{} to {}", day_short(field(summary, "sinceDay")), day_short(field(summary, "untilDay"))),
        };
        bar = bar.child(text(label).font(theme::TEXT_XS).color(t.c.muted_foreground).ml(8.0));
    }
    bar = bar.child(spacer());
    let mut right = row().gap(8.0).center_y();
    if !limits {
        let windows = [1, 7, 30, 90];
        let selected = windows.iter().position(|w| *w == u.window_days).unwrap_or(2);
        right = right.child(
            ToggleGroup::new(&["Past 24h", "7 days", "30 days", "90 days"], &[selected])
                .on_pick(move |s: &mut App, n: usize, _: &mut Tx| {
                    ui(s).window_days = windows[n];
                    save_preferences(s);
                })
                .build(&k),
        );
    }
    let metrics = ["cost", "tokens", "limits"];
    let selected = metrics.iter().position(|m| *m == u.metric).unwrap_or(2);
    right = right
        .child(
            ToggleGroup::new(&["Cost", "Tokens", "Limits"], &[selected])
                .on_pick(move |s: &mut App, n: usize, _: &mut Tx| {
                    ui(s).metric = metrics[n];
                    save_preferences(s);
                })
                .build(&k),
        )
        .child(
            kit::Button::new(if limits { "Refresh limits" } else { "Refresh usage" })
                .variant("ghost")
                .size("icon-sm")
                .icon(if refreshing(u) { &i::LOADER_CIRCLE } else { &i::REFRESH_CW })
                .disabled(refreshing(u) || (!limits && !can_read))
                .on_press(refresh)
                .build(t),
        )
        .child(kit::Button::new("Close").variant("ghost").size("icon-sm").icon(&i::X).on_press(close).build(t));
    col().w_full().child(bar.child(right)).child(el().w_full().h(1.0).fill(t.c.border))
}

// ---- cost and tokens --------------------------------------------------------------------

fn overview(s: &App, t: &T, summary: &Value, page: El, overlays: &mut Vec<El>) -> El {
    let u = &s.ui.pages.usage;
    let tokens_view = u.metric == "tokens";
    let mut m = merge(summary, &u.hidden, u.answered.as_ref(), None);
    m.clock = clock(s);
    let value = |x: Totals| if tokens_view { x.tokens } else { x.cost };
    let fmt = |v: f64| if tokens_view { tokens(v) } else { usd(v) };
    let tnum = |style: rune::tok::TextStyle| style.features("'tnum' 1");
    let selected = selected_usage(s);
    let (every, partway) = loading(&selected, refreshing_usage(u), u.showing_kept, &u.hidden);
    let dim = if every || !partway.is_empty() { 0.4 } else { 1.0 };
    let sessions = m.sessions;
    let mut caption = row().gap(4.0).center_y().child(
        text(format!("{} sessions{}", grouped(sessions), if tokens_view { "" } else { " · API estimate" })).font(theme::TEXT_XS).color(t.c.muted_foreground),
    );
    if !tokens_view && m.records > 0.0 && m.unpriced > 0.0 {
        let trigger = row()
            .key("usage-unpriced-info")
            .size(16.0, 16.0)
            .center()
            .radius(radius::SM)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label("Unpriced usage details")
            .child(icon(&i::INFO, 12.0).color(t.c.muted_foreground))
            .hover_delays(std::time::Duration::from_millis(300), std::time::Duration::ZERO)
            .on_hover_change(|s: &mut App, on, _| ui(s).unpriced_tip = on)
            .on_press(|s: &mut App| ui(s).unpriced_tip = true);
        caption = caption.child(
            kit::Tooltip::new("usage-unpriced-tip")
                .trigger(trigger)
                .label(format!("API estimate excludes {} unpriced records.", percent(m.unpriced / m.records)))
                .open(u.unpriced_tip)
                .build(&kit::theme(t), overlays),
        );
    }
    let mut left = col()
        .w(288.0)
        .shrink()
        .gap(20.0)
        .child(
            col()
                .gap(4.0)
                .child(text(fmt(if tokens_view { m.tokens } else { m.cost })).font(tnum(theme::weight(TEXT_4XL, theme::SEMIBOLD))).color(t.c.foreground).opacity(dim))
                .child(caption.opacity(dim)),
        );
    let chatgpt = selected.iter().any(|e| config(s, &e.key).is_some_and(|c| c["providers"].as_array().into_iter().flatten().any(shares_chatgpt)));
    if !u.hidden.contains(&"codex") && chatgpt {
        left = left.child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(text("ChatGPT shared usage").font(theme::TEXT_XS).color(t.c.muted_foreground))
                .child(spacer())
                .child(kit::Button::new("Manage usage").variant("ghost-muted").size("xs").icon(&i::EXTERNAL_LINK).build(t).on_press_tx(|_: &mut App, tx: &mut Tx| {
                    let _ = tx.open_url(CHATGPT_USAGE_URL);
                })),
        );
    }
    let with_data = |kind: &str| m.providers.iter().any(|p| p.0 == kind && (p.1.tokens > 0.0 || p.1.cost > 0.0));
    let active = chart_providers(&m, &partway);
    let loading_kinds: Vec<&str> = active.iter().copied().filter(|k| every || partway.iter().any(|x| x == k)).collect();
    let cursor_at = ["codex", "claude"].iter().map(|k| active.iter().position(|p| p == k).map_or(0, |at| at + 1)).max().unwrap_or(0);
    let show_environment = selected.len() > 1;
    let cursor: Vec<El> = cursor_access_environments(&selected, &u.hidden).iter().map(|(key, label)| cursor_enable_row(s, t, key, label, show_environment)).collect();
    let mut cursor = Some(cursor);
    let mut rows = col().gap(16.0);
    for (n, &kind) in active.iter().enumerate() {
        if n == cursor_at
            && let Some(lines) = cursor.take()
        {
            rows = rows.children(lines);
        }
        let (totals, sessions) = m.providers.iter().find(|p| p.0 == kind).map_or((Totals::default(), 0), |p| (p.1, p.2));
        let figure = if loading_kinds.contains(&kind) { 0.4 } else { 1.0 };
        let awaiting = loading_kinds.contains(&kind) && !with_data(kind);
        let (label, driver) = presentation(kind);
        let share = if tokens_view { totals.tokens / m.tokens.max(1.0) } else { totals.cost / m.cost.max(f64::MIN_POSITIVE) };
        let sub = if tokens_view { format!("{} of tokens · {}", percent(share), usd(totals.cost)) } else { format!("{} of cost · {} tokens", percent(share), tokens(totals.tokens)) };
        let detail = if awaiting { 0.0 } else { figure };
        rows = rows.child(
            col()
                .gap(4.0)
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .min_h(20.0)
                        .center_y()
                        .child(el().size(8.0, 8.0).radius(radius::FULL).fill(color(t, kind)))
                        .child(provider_icon(t, driver, 16.0))
                        .child(text(label).font(theme::TEXT_SM).color(t.c.foreground))
                        .child(text(sessions_label(sessions)).font(theme::TEXT_2XS).color(t.c.muted_foreground).opacity(detail))
                        .child(spacer())
                        .child(text(if awaiting { "-".to_string() } else { fmt(value(totals)) }).font(tnum(theme::SM_MEDIUM)).color(t.c.foreground).opacity(figure)),
                )
                .child(text(sub).font(theme::TEXT_XS).color(t.c.muted_foreground).opacity(detail)),
        );
    }
    left = left.child(rows.children(cursor.into_iter().flatten()));
    let hourly = m.hourly;
    let chart_title = match (hourly, tokens_view) {
        (false, false) => "Daily cost",
        (false, true) => "Daily processed tokens",
        (true, false) => "Hourly cost",
        (true, true) => "Hourly processed tokens",
    };
    let right = col().grow(1.0).shrink().min_w(0.0).gap(12.0).child(h2(t, chart_title)).child(chart(t, &m, tokens_view, (u.hover, u.plot_w), false, &active, &loading_kinds));
    let top = row().w_full().gap(24.0).child(left).child(right);

    let metric = |label: &str, v: String| {
        col().grow(1.0).w(0.0).gap(2.0).child(text(label.to_string()).font(theme::TEXT_XS).color(t.c.muted_foreground)).child(text(v).font(tnum(theme::weight(theme::TEXT_BASE, theme::MEDIUM))).color(t.c.foreground))
    };
    let totals = col().w_full().gap(8.0).child(h2(t, "Totals")).child(
        row()
            .w_full()
            .gap(24.0)
            .py(4.0)
            .opacity(dim)
            .child(metric("Processed tokens", tokens(m.tokens)))
            .child(metric("Cached input", tokens(m.cached)))
            .child(metric("Uncached input", tokens(m.uncached)))
            .child(metric("Output", tokens(m.output)))
            .child(metric("Cache savings", usd(m.savings))),
    );
    let mut page = page;
    for message in source_messages(summary, &u.hidden) {
        page = page.child(muted_line(t, &message));
    }
    let mut page = page.child(top).child(totals);
    if m.tokens > 0.0 {
        // The metric picks the splits: tokens by type, or cost by type and by speed.
        let mut shares = row().w_full().gap(48.0).opacity(dim);
        if tokens_view {
            shares = shares.child(share_bar(t, "Tokens by type", &token_parts(t, &m), tokens, None)).child(el().grow(1.0).w(0.0));
        } else {
            shares = shares.child(share_bar(t, "Cost by type", &cost_parts(t, &m), usd, None));
            shares = match speed_parts(t, &m) {
                Some(parts) => shares.child(share_bar(t, "Cost by speed", &parts, usd, Some(m.premium))),
                None => shares.child(el().grow(1.0).w(0.0)),
            };
        }
        page = page.child(shares);
    }
    page.child(breakdown(s, t, &m, dim))
}

/// UsageModelDialog: one breakdown row's totals, trend and splits.
fn model_dialog(s: &App, t: &T, summary: &Value, key: &str, overlays: &mut Vec<El>) {
    let u = &s.ui.pages.usage;
    let all = merge(summary, &u.hidden, u.answered.as_ref(), None);
    let mut m = merge(summary, &u.hidden, u.answered.as_ref(), Some(key));
    m.clock = clock(s);
    let mut parts = key.split('\0');
    let (kind, _api, model) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let (label, _) = presentation(kind);
    let unknown = m.records > 0.0 && m.unpriced >= m.records;
    let description = if unknown { label.to_string() } else { format!("{label} · {} of cost", percent(m.cost / all.cost.max(f64::MIN_POSITIVE))) };
    let priced = m.tokens - m.unpriced_tokens;
    let input = m.uncached + m.cached + m.creation;
    let mut stats = vec![("Cost", if unknown { "Unpriced".to_string() } else { usd(m.cost) }), ("Tokens", tokens(m.tokens))];
    if priced > 0.0 && !unknown {
        stats.push(("Per 1M tokens", usd(m.cost / priced * 1_000_000.0)));
    }
    if input > 0.0 {
        stats.push(("Cache hit", percent(m.cached / input)));
    }
    let mut grid = row().w_full().gap(24.0);
    for (name, value) in stats {
        grid = grid.child(
            col()
                .grow(1.0)
                .w(0.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text(name).font(theme::TEXT_XS).color(t.c.muted_foreground))
                .child(text(value).font(theme::weight(theme::TEXT_2XL, theme::SEMIBOLD).features("'tnum' 1")).color(t.c.foreground)),
        );
    }
    // Unpriced cost is unknown, not zero, so its trend shows tokens.
    let tokens_view = unknown || u.metric == "tokens";
    let mut shares = row().w_full().gap(40.0);
    if !unknown {
        shares = shares.child(share_bar(t, "Cost by type", &cost_parts(t, &m), usd, None));
    }
    shares = shares.child(share_bar(t, "Tokens by type", &token_parts(t, &m), tokens, None));
    let providers: Vec<&'static str> = PROVIDERS.iter().map(|p| p.0).filter(|k| *k == kind).collect();
    let mut body = col().w_full().gap(32.0).child(grid).child(chart(t, &m, tokens_view, (u.model_hover, u.model_plot_w), true, &providers, &[])).child(shares);
    if let Some(parts) = speed_parts(t, &m) {
        body = body.child(row().w_full().child(share_bar(t, "Cost by speed", &parts, usd, Some(m.premium))));
    }
    if m.unpriced_tokens > 0.0 {
        let set_price = model.to_string();
        body = body.child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(text(format!("{} tokens have no known price", tokens(m.unpriced_tokens))).font(theme::TEXT_XS).color(t.c.muted_foreground))
                .child(spacer())
                .child(
                    kit::Button::new("Set price")
                        .on_press(move |s: &mut App| {
                            let u = ui(s);
                            u.model = None;
                            u.model_hover = None;
                            super::usage_prices::open(s, Some(&set_price));
                        })
                        .build(t),
                ),
        );
    }
    kit::Dialog::new("usage-model", model)
        .description(description)
        .child(body)
        .width(768.0)
        .open(true)
        .on_close(|s: &mut App| {
            let u = ui(s);
            u.model = None;
            u.model_hover = None;
        })
        .build(t, overlays);
}

fn h2(t: &T, label: &str) -> El {
    text(label.to_string()).font(theme::SM_MEDIUM).color(t.c.foreground)
}

fn shade(t: &T, k: f32) -> Color {
    mix(t.c.foreground, t.c.background, k)
}

fn token_parts(t: &T, m: &Merged) -> Vec<(&'static str, f64, Color)> {
    vec![("Input", m.uncached, shade(t, 0.6)), ("Cache read", m.cached, shade(t, 0.3)), ("Cache write", m.creation, shade(t, 0.72)), ("Output", m.output, shade(t, 1.0))]
}

/// costTypeSegments: unsplit cost under a cent rounds away rather than showing as "Other".
fn cost_parts(t: &T, m: &Merged) -> Vec<(&'static str, f64, Color)> {
    let other = if m.category[4] >= 0.005 { m.category[4] } else { 0.0 };
    vec![
        ("Input", m.category[0], shade(t, 0.6)),
        ("Cache read", m.category[1], shade(t, 0.3)),
        ("Cache write", m.category[2], shade(t, 0.72)),
        ("Output", m.category[3], shade(t, 1.0)),
        ("Other", other, shade(t, 0.44)),
    ]
}

fn speed_parts(t: &T, m: &Merged) -> Option<Vec<(&'static str, f64, Color)>> {
    (m.fast + m.ultrafast > 0.0).then(|| {
        let standard = (m.cost - m.fast - m.ultrafast).max(0.0);
        vec![("Standard", standard, shade(t, 0.34)), ("Fast", m.fast, shade(t, 0.66)), ("Ultrafast", m.ultrafast, shade(t, 1.0))]
    })
}

fn sessions_label(n: i64) -> String {
    format!("{} {}", grouped(n), if n == 1 { "session" } else { "sessions" })
}

/// usageSourceMessages: what a partial, failed or Cursor source said, once each, with the
/// malformed-record warnings folded into one total.
fn source_messages(summary: &Value, hidden: &[&str]) -> Vec<String> {
    let warning = |n: i64| format!("{n} malformed or inconsistent usage record(s) were skipped; usage may be incomplete.");
    let mut messages: Vec<String> = Vec::new();
    let mut malformed: HashMap<String, i64> = HashMap::new();
    for source in summary["sources"].as_array().into_iter().flatten() {
        let fp = &source["fingerprint"];
        let provider = fp["provider"].as_str().unwrap_or("");
        if hidden.contains(&provider) {
            continue;
        }
        let status = source["status"].as_str().unwrap_or("");
        let count = source["malformedRecords"].as_i64().unwrap_or(0);
        let action = source["action"].as_bool().unwrap_or(false) || source["action"].is_object() || source["action"].is_string();
        if !action && status != "partial" && status != "failed" && provider != "cursor" && !(status == "missing" && count > 0) {
            continue;
        }
        let mut message = source["message"].as_str().unwrap_or("").to_string();
        if count > 0 {
            let key = json!([fp["hostId"], fp["provider"], fp["resolvedHomePath"], fp["volumeId"]]).to_string();
            let seen = malformed.entry(key).or_default();
            *seen = (*seen).max(count);
            // Servers also append the warning to the message; keep any read failure before it.
            if let Some(rest) = message.strip_suffix(&warning(count)) {
                message = rest.trim().to_string();
            }
        }
        if !message.is_empty() && !messages.contains(&message) {
            messages.push(message);
        }
    }
    let total: i64 = malformed.values().sum();
    if total > 0 {
        let line = warning(total);
        if !messages.contains(&line) {
            messages.push(line);
        }
    }
    messages
}

fn share_bar(t: &T, title: &str, parts: &[(&str, f64, Color)], fmt: fn(f64) -> String, premium: Option<f64>) -> El {
    let shown: Vec<_> = parts.iter().filter(|p| p.1 > 0.0).collect();
    let mut bar = row().w_full().h(8.0).gap(2.0);
    for (n, (_, v, c)) in shown.iter().enumerate() {
        let r = if n == 0 || n + 1 == shown.len() { radius::FULL } else { radius::XS };
        bar = bar.child(el().grow(*v as f32).w(0.0).min_w(4.0).h(8.0).radius(r).fill(*c));
    }
    let mut legend = row().w_full().flex_wrap().gaps(4.0, 16.0);
    for (label, v, c) in &shown {
        legend = legend.child(
            row()
                .gap(6.0)
                .center_y()
                .child(el().size(8.0, 8.0).radius(radius::XS).fill(*c))
                .child(text(label.to_string()).font(theme::TEXT_XS).color(t.c.muted_foreground))
                .child(text(fmt(*v)).font(theme::TEXT_XS.features("'tnum' 1")).color(t.c.foreground)),
        );
    }
    let mut head = row().w_full().gap(12.0).center_y().child(h2(t, title)).child(spacer());
    if let Some(premium) = premium {
        head = head.child(text("Premium").font(theme::TEXT_XS).color(t.c.muted_foreground)).child(text(usd(premium)).font(theme::TEXT_XS.features("'tnum' 1")).color(t.c.foreground).ml(-8.0));
    }
    col().grow(1.0).w(0.0).min_w(0.0).gap(10.0).child(head).child(bar).child(legend)
}

const VIEW_W: f32 = 960.0;
const VIEW_H: f32 = 260.0;
const PLOT_TOP: f32 = 8.0;

fn nice_scale(peak: f64) -> (f64, Vec<f64>) {
    if peak <= 0.0 {
        return (0.0, vec![0.0]);
    }
    let raw = peak / 4.0;
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let step = (if norm > 5.0 { 10.0 } else if norm > 2.0 { 5.0 } else if norm > 1.0 { 2.0 } else { 1.0 }) * mag;
    let max = (peak / step).ceil() * step;
    let mut ticks = Vec::new();
    let mut v = 0.0;
    while v <= max + step * 1e-6 {
        ticks.push(v);
        v += step;
    }
    (max, ticks)
}

fn tangents(pts: &[[f32; 2]]) -> Vec<f32> {
    let n = pts.len();
    if n < 2 {
        return vec![0.0];
    }
    let slopes: Vec<f32> = pts.windows(2).map(|p| if p[1][0] == p[0][0] { 0.0 } else { (p[1][1] - p[0][1]) / (p[1][0] - p[0][0]) }).collect();
    let mut t = vec![0.0; n];
    t[0] = slopes[0];
    t[n - 1] = slopes[n - 2];
    for k in 1..n - 1 {
        let (a, b) = (slopes[k - 1], slopes[k]);
        t[k] = if a * b <= 0.0 { 0.0 } else { (a + b) / 2.0 };
    }
    for k in 0..n - 1 {
        let slope = slopes[k];
        if slope == 0.0 {
            t[k] = 0.0;
            t[k + 1] = 0.0;
            continue;
        }
        let (a, b) = (t[k] / slope, t[k + 1] / slope);
        let mag = a * a + b * b;
        if mag > 9.0 {
            let scale = 3.0 / mag.sqrt();
            t[k] = scale * a * slope;
            t[k + 1] = scale * b * slope;
        }
    }
    t
}

fn curve(pts: &[[f32; 2]]) -> Path {
    let tan = tangents(pts);
    let mut path = Path::new().move_to(pts[0]);
    for k in 0..pts.len() - 1 {
        let (a, b) = (pts[k], pts[k + 1]);
        let dx = b[0] - a[0];
        path = path.cubic_to([a[0] + dx / 3.0, a[1] + tan[k] * dx / 3.0], [b[0] - dx / 3.0, b[1] - tan[k + 1] * dx / 3.0], b);
    }
    path
}

/// The column a pointer `x` across a plot `w` wide hovers, of `n` evenly spaced periods.
fn hovered_column(x: f32, w: f32, n: usize) -> Option<usize> {
    (n > 0 && w > 0.0).then(|| ((x.clamp(0.0, w) / w * (n - 1) as f32).round() as usize).min(n - 1))
}

/// UsagePage's `activeProviders`, the chart's series: providers with figures, and providers still
/// updating before their first figures arrive.
fn chart_providers(m: &Merged, partway: &[String]) -> Vec<&'static str> {
    let with_data = |kind: &str| m.providers.iter().any(|p| p.0 == kind && (p.1.tokens > 0.0 || p.1.cost > 0.0));
    PROVIDERS.iter().map(|p| p.0).filter(|k| with_data(*k) || partway.iter().any(|x| x == k)).collect()
}

/// buildChart's paths: largest total first, so it draws beneath the others.
fn series(m: &Merged, providers: &[&'static str], value: &dyn Fn(&Totals) -> f64) -> Vec<&'static str> {
    let total = |kind: &str| m.providers.iter().find(|p| p.0 == kind).map_or(0.0, |p| value(&p.1));
    let mut out = providers.to_vec();
    out.sort_by(|a, b| total(*b).total_cmp(&total(*a)));
    out
}

/// UsageChartTooltip's figures for `period`: each chart provider's value and whether it still
/// loads, the period's total, and whether that total is partial.
fn tooltip_figures(m: &Merged, period: &str, providers: &[&'static str], loading: &[&str], value: &dyn Fn(&Totals) -> f64) -> (Vec<(&'static str, f64, bool)>, f64, bool) {
    let values = m.by_period.get(period);
    let rows = providers.iter().map(|&kind| (kind, values.and_then(|p| p.get(kind)).map(value).unwrap_or(0.0), loading.contains(&kind))).collect();
    let total = values.map_or(0.0, |p| p.values().map(value).sum());
    (rows, total, providers.iter().any(|kind| loading.contains(kind)))
}

/// UsageChartTooltip: the hovered period, each chart provider's value, and the total, beside the
/// pointer and flipped to stay inside the plot. Figures still loading read muted.
fn chart_tooltip(t: &T, m: &Merged, (index, x, y): (usize, f32, f32), plot: (f32, f32), tokens_view: bool, providers: &[&'static str], loading: &[&str]) -> El {
    let value = |v: &Totals| if tokens_view { v.tokens } else { v.cost };
    let format = |v: f64| if tokens_view { tokens(v) } else { usd(v) };
    let period = m.periods.get(index).cloned().unwrap_or_default();
    let (rows, total, partial) = tooltip_figures(m, &period, providers, loading, &value);
    let muted = t.c.muted_foreground;
    let figure = |still: bool| if still { muted } else { t.c.foreground };
    let num = theme::TEXT_XS.features("'tnum' 1");
    let title = match iso_ms(&period).filter(|_| m.hourly) {
        Some(at) => m.clock.relative(at),
        None => period_label(&period, m.clock),
    };
    let mut body = col().child(text(title).font(theme::TEXT_XS).color(muted).mb(4.0));
    for (kind, v, still) in rows {
        let (label, driver) = presentation(kind);
        body = body.child(
            row()
                .gap(12.0)
                .center_y()
                .child(row().gap(6.0).center_y().child(provider_icon(t, driver, 12.0)).child(text(label).font(theme::TEXT_XS).color(muted)))
                .child(spacer())
                .child(text(format(v)).font(num).color(figure(still))),
        );
    }
    body = body.child(
        row()
            .mt(4.0)
            .pt(4.0)
            .gap(12.0)
            .center_y()
            .child(el().absolute(0.0, 0.0).w_full().h(1.0).fill(t.c.border))
            .child(text("Total").font(theme::TEXT_XS).color(muted))
            .child(spacer())
            .child(text(format(total)).font(num).color(figure(partial))),
    );
    // The tooltip isn't measured before it's placed, so flip on its estimated size.
    let (w, h) = (176.0, 46.0 + 18.0 * providers.len() as f32);
    let gap = 12.0;
    let left = if x + gap + w <= plot.0 { x + gap } else { x - gap - w };
    let top = if y + gap + h <= plot.1 { y + gap } else { y - gap - h };
    body.absolute(left.clamp(0.0, (plot.0 - w).max(0.0)), top.clamp(0.0, (plot.1 - h).max(0.0)))
        .w(w)
        .px(10.0)
        .py(8.0)
        .radius(radius::XL)
        .fill(t.c.popover.alpha_mul(0.92))
        .border(1.0, t.c.border.alpha_mul(0.5))
        .shadow(4.0, 12.0, Color::BLACK.alpha(if t.dark { 0.32 } else { 0.1 }))
}

/// The hover slot a chart writes: the page's, or the model dialog's (`dialog`).
fn slot(u: &mut Ui, dialog: bool) -> (&mut Option<(usize, f32, f32)>, &mut f32) {
    if dialog { (&mut u.model_hover, &mut u.model_plot_w) } else { (&mut u.hover, &mut u.plot_w) }
}

/// UsageProviderChart: a series per chart provider, even one without figures yet; `loading`
/// providers' series dim, and with nothing to plot yet the axis keeps unlabeled placeholder
/// ticks (chartScale).
fn chart(t: &T, m: &Merged, tokens_view: bool, (hover, plot_w): (Option<(usize, f32, f32)>, f32), dialog: bool, providers: &[&'static str], loading: &[&str]) -> El {
    let value = |x: &Totals| if tokens_view { x.tokens } else { x.cost };
    let peak = m.by_period.values().flat_map(|p| p.values()).map(value).fold(0.0, f64::max);
    let labeled = peak > 0.0 || loading.is_empty();
    let (max, ticks) = if labeled { nice_scale(peak) } else { (4.0, vec![0.0, 1.0, 2.0, 3.0, 4.0]) };
    let to_y = |v: f64| if max == 0.0 { VIEW_H } else { VIEW_H - (v / max) as f32 * (VIEW_H - PLOT_TOP) };
    let plot_h = 224.0;
    let mut axis = el().w(56.0).h(plot_h).shrink();
    for tick in ticks.iter().filter(|_| labeled) {
        let label = if *tick == 0.0 { "0".to_string() } else if tokens_view { tokens(*tick) } else { usd(*tick) };
        let y = to_y(*tick) / VIEW_H * plot_h;
        axis = axis.child(row().absolute(0.0, y - 7.0).w(56.0).h(14.0).justify_end().child(text(label).font(theme::TEXT_3XS.features("'tnum' 1")).color(t.c.muted_foreground)));
    }
    let mut shape = Shape::new();
    for tick in &ticks {
        let y = to_y(*tick);
        shape = shape.line([0.0, y], [VIEW_W, y], 1.0, LineCap::Butt, t.c.border);
    }
    let step = if m.periods.len() < 2 { 0.0 } else { VIEW_W / (m.periods.len() - 1) as f32 };
    let mut lines = Vec::new();
    for kind in series(m, providers, &value) {
        let pts: Vec<[f32; 2]> = m
            .periods
            .iter()
            .enumerate()
            .map(|(n, p)| [n as f32 * step, to_y(m.by_period.get(p).and_then(|x| x.get(kind)).map(value).unwrap_or(0.0))])
            .collect();
        if pts.len() < 2 {
            continue;
        }
        let c = if loading.contains(&kind) { color(t, kind).alpha_mul(0.4) } else { color(t, kind) };
        let line = curve(&pts);
        let area = curve(&pts).line_to([VIEW_W, VIEW_H]).line_to([0.0, VIEW_H]).close();
        shape = shape.fill_path(&area, c.alpha_mul(0.12));
        lines.push((line, c));
    }
    for (line, c) in &lines {
        shape = shape.stroke_path(line, 2.0, LineCap::Round, *c);
    }
    let hover = hover.filter(|h| h.0 < m.periods.len());
    if let Some((index, ..)) = hover {
        let x = index as f32 * step;
        shape = shape.line([x, PLOT_TOP], [x, VIEW_H], 1.0, LineCap::Butt, t.c.muted_foreground);
    }
    let n = m.periods.len();
    let mut plot = el()
        .key(if dialog { "usage-model-plot" } else { "usage-plot" })
        .grow(1.0)
        .min_w(0.0)
        .h(plot_h)
        .child(canvas(shape.view_box(VIEW_W, VIEW_H)).w_full().h(plot_h))
        .on_layout(move |s: &mut App, r| {
            let (_, w) = slot(ui(s), dialog);
            if (*w - r[2]).abs() > 0.5 {
                *w = r[2];
            }
        })
        .on_pointer_move_local(move |s: &mut App, p, _| {
            let (hover, w) = slot(ui(s), dialog);
            let w = *w;
            *hover = hovered_column(p.x, w, n).map(|index| (index, p.x.clamp(0.0, w), p.y.clamp(0.0, plot_h)));
        })
        .on_hover_change(move |s: &mut App, on, _| {
            if !on {
                *slot(ui(s), dialog).0 = None;
            }
        });
    if let Some(hover) = hover {
        plot = plot.child(chart_tooltip(t, m, hover, (plot_w, plot_h), tokens_view, providers, loading));
    }
    let label = |n: usize| m.periods.get(n).map(|p| period_label(p, m.clock).to_uppercase()).unwrap_or_default();
    let x_labels = row()
        .w_full()
        .pl(64.0)
        .child(text(label(0)).font(theme::TEXT_3XS).color(t.c.muted_foreground))
        .child(spacer())
        .child(text(label(m.periods.len() / 2)).font(theme::TEXT_3XS).color(t.c.muted_foreground))
        .child(spacer())
        .child(text(label(m.periods.len().saturating_sub(1))).font(theme::TEXT_3XS).color(t.c.muted_foreground));
    col().w_full().gap(4.0).child(row().w_full().gap(8.0).child(axis).child(plot)).child(x_labels)
}

fn breakdown(s: &App, t: &T, m: &Merged, dim: f32) -> El {
    let u = &s.ui.pages.usage;
    let k = kit::theme(t);
    let period = if m.hourly || u.answered.as_ref().is_some_and(|r| r["resolution"] == "hour") { "Hour" } else { "Day" };
    let toggle = ToggleGroup::new(&["Model", period], &[usize::from(u.by_period)]).on_pick(|s: &mut App, n: usize, _: &mut Tx| ui(s).by_period = n == 1).build(&k);
    let header = row().w_full().center_y().child(h2(t, "Breakdown")).child(spacer()).child(toggle);
    let table = if u.by_period { period_table(t, m) } else { model_table(t, m, u.metric == "tokens") };
    col().w_full().gap(12.0).child(header).child(table.opacity(dim))
}

fn cell(label: String, w: f32, style: rune::tok::TextStyle, c: Color) -> El {
    row().w(w).shrink().justify_end().child(text(label).font(style).color(c))
}

fn model_table(t: &T, m: &Merged, tokens_view: bool) -> El {
    let head = theme::TEXT_XS;
    let muted = t.c.muted_foreground;
    let num = theme::TEXT_SM.features("'tnum' 1");
    let mut table = col().w_full().child(
        row()
            .w_full()
            .py(8.0)
            .center_y()
            .child(row().pl(8.0).pr(12.0).w(40.0).child(text("#").font(head).color(muted)))
            .child(row().grow(1.0).shrink().min_w(0.0).child(text("Model").font(head).color(muted)))
            .child(cell("Cost".into(), 120.0, head, muted))
            .child(cell("Share".into(), 96.0, head, muted))
            .child(cell("Tokens".into(), 112.0, head, muted).pr(8.0)),
    );
    table = table.child(el().w_full().h(1.0).fill(t.c.border));
    if m.models.is_empty() {
        return table.child(row().w_full().py(24.0).center().child(text("No activity in this window.").font(theme::TEXT_SM).color(muted)));
    }
    let peak = m.models.iter().map(|x| if tokens_view { x.totals.tokens } else { x.totals.cost }).fold(0.0, f64::max).max(f64::MIN_POSITIVE);
    let total = if tokens_view { m.tokens } else { m.cost }.max(f64::MIN_POSITIVE);
    for (n, line) in by_metric(&m.models, tokens_view).into_iter().enumerate() {
        let (kind, api, model, totals) = (line.kind, line.api(), line.model.as_str(), line.totals);
        let (_, driver) = presentation(kind);
        let v = if tokens_view { totals.tokens } else { totals.cost };
        let mut name = row().gap(6.0).center_y().child(provider_icon(t, driver, 14.0)).child(text(model.to_string()).font(theme::TEXT_SM).color(t.c.foreground).truncate().shrink());
        if !api.is_empty() {
            name = name.child(text(format!("via {api}")).font(theme::TEXT_XS).color(muted));
        }
        if line.unpriced_only || line.tagged {
            name = name.child(tag_button(t, line));
        }
        let unpriced = line.cost_unknown();
        let cost = if unpriced { "Unpriced".to_string() } else { usd(totals.cost) };
        // modelShare: an unknown cost has no cost share, only a token share.
        let share = if unpriced && !tokens_view { String::new() } else { percent(v / total) };
        let open = line.key.clone();
        table = table
            .child(
                row()
                    .key(format!("usage-model-{n}"))
                    .w_full()
                    .py(10.0)
                    .center_y()
                    .radius(radius::MD)
                    .hover(t.c.accent.alpha_mul(0.5))
                    .cursor(rune::Cursor::Pointer)
                    .role("button")
                    .label(format!("{model} details"))
                    .on_press(move |s: &mut App| ui(s).model = Some(open.clone()))
                    .child(row().pl(8.0).pr(12.0).w(40.0).child(text((n + 1).to_string()).font(theme::TEXT_XS.features("'tnum' 1")).color(muted)))
                    .child(
                        col()
                            .grow(1.0)
                            .shrink()
                            .min_w(0.0)
                            .child(name)
                            .child(if v > 0.0 { el().mt(6.0).h(2.0).w(((v / peak) as f32 * 192.0).max(8.0)).radius(radius::FULL).fill(color(t, kind)) } else { el().mt(6.0).h(2.0) }),
                    )
                    .child(cell(cost, 120.0, num, if unpriced { muted } else { t.c.foreground }))
                    .child(cell(share, 96.0, num, muted))
                    .child(cell(tokens(totals.tokens), 112.0, num, muted).pr(8.0)),
            )
            .child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.5)));
    }
    table
}

fn period_table(t: &T, m: &Merged) -> El {
    let head = theme::TEXT_XS;
    let muted = t.c.muted_foreground;
    let num = theme::TEXT_SM.features("'tnum' 1");
    let kinds: Vec<&str> = m.providers.iter().map(|p| p.0).collect();
    let mut header = row().w_full().py(8.0).center_y().child(row().grow(1.0).pl(8.0).child(text(if m.hourly { "Hour" } else { "Day" }).font(head).color(muted)));
    for kind in &kinds {
        header = header.child(cell(presentation(kind).0.to_string(), 120.0, head, muted));
    }
    header = header.child(cell("Total".into(), 112.0, head, muted)).child(cell("Tokens".into(), 112.0, head, muted).pr(8.0));
    let mut table = col().w_full().child(header).child(el().w_full().h(1.0).fill(t.c.border));
    let periods = active_periods(m);
    if periods.is_empty() {
        return table.child(row().w_full().py(24.0).center().child(text("No activity in this window.").font(theme::TEXT_SM).color(muted)));
    }
    for p in periods {
        let row_totals = m.by_period.get(p);
        let get = |k: &str| row_totals.and_then(|x| x.get(k)).copied().unwrap_or_default();
        let mut line = row().w_full().py(8.0).center_y().child(row().grow(1.0).pl(8.0).child(text(period_label(p, m.clock)).font(theme::TEXT_SM).color(t.c.foreground)));
        let mut sum = Totals::default();
        for kind in &kinds {
            let x = get(kind);
            sum.cost += x.cost;
            sum.tokens += x.tokens;
            line = line.child(cell(usd(x.cost), 120.0, num, muted));
        }
        line = line.child(cell(usd(sum.cost), 112.0, num, t.c.foreground)).child(cell(tokens(sum.tokens), 112.0, num, muted).pr(8.0));
        table = table.child(line).child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.5)));
    }
    table
}

/// The row's "Tag"/"Tagged" chip, opening UsageModelTagDialog on its transcript name or tag.
fn tag_button(t: &T, line: &ModelRow) -> El {
    let target = super::usage_tags::Target {
        provider: line.kind.to_string(),
        label: line.model.clone(),
        untagged: (!line.tagged).then(|| (line.api().to_string(), line.model.clone())),
        tagged_as: line.priced_as.clone().filter(|_| line.tagged),
    };
    row()
        .key(format!("usage-tag-{}", line.key))
        .gap(4.0)
        .px(6.0)
        .py(2.0)
        .center_y()
        .radius(radius::XS)
        .border(1.0, t.c.border)
        .hover(t.c.accent)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(format!("Tag {}", line.model))
        .on_press(move |s: &mut App| super::usage_tags::open(s, target.clone()))
        .child(icon(&i::TAG, 12.0).color(t.c.muted_foreground))
        .child(text(if line.tagged { "Tagged" } else { "Tag" }).font(theme::TEXT_XS).color(t.c.muted_foreground))
}

/// The breakdown's order: by cost, or by tokens in the tokens view (sortModelsByTokens).
fn by_metric(models: &[ModelRow], tokens_view: bool) -> Vec<&ModelRow> {
    let mut sorted: Vec<_> = models.iter().collect();
    if tokens_view {
        sorted.sort_by(|a, b| b.totals.tokens.total_cmp(&a.totals.tokens).then(b.totals.cost.total_cmp(&a.totals.cost)));
    }
    sorted
}

/// The time breakdown lists periods that had activity, newest first (merged.daily/hourly).
fn active_periods(m: &Merged) -> Vec<&String> {
    m.periods.iter().rev().filter(|p| m.by_period.get(*p).is_some_and(|row| !row.is_empty())).collect()
}

/// UsageSkeleton: static placeholders in the overview's shape.
fn skeleton(t: &T, page: El) -> El {
    let bar = t.c.muted_foreground.alpha_mul(0.15);
    let sk = |w: f32, h: f32| el().w(w).h(h).radius(radius::SM).fill(bar);
    let pill = |d: f32| el().size(d, d).radius(radius::FULL).fill(bar);
    let mut left = col().w(288.0).shrink().gap(20.0).child(col().gap(4.0).child(sk(144.0, 40.0)).child(sk(128.0, 16.0)));
    for _ in PROVIDERS {
        left = left.child(
            col()
                .gap(4.0)
                .child(row().w_full().min_h(20.0).center_y().child(row().gap(8.0).center_y().child(pill(8.0)).child(pill(16.0)).child(sk(80.0, 14.0))).child(spacer()).child(sk(56.0, 14.0)))
                .child(sk(144.0, 16.0)),
        );
    }
    let right = col()
        .grow(1.0)
        .shrink()
        .gap(12.0)
        .child(sk(96.0, 20.0))
        .child(col().gap(4.0).pl(64.0).child(el().w_full().h(224.0).radius(radius::SM).fill(bar)).child(el().w_full().h(16.0).radius(radius::SM).fill(bar)));
    let metric = |label: &str| col().grow(1.0).w(0.0).gap(2.0).child(text(label.to_string()).font(theme::TEXT_XS).color(t.c.muted_foreground)).child(sk(64.0, 24.0));
    page.child(row().w_full().gap(24.0).child(left).child(right))
        .child(
            col().w_full().gap(8.0).child(h2(t, "Totals")).child(
                row()
                    .w_full()
                    .gap(24.0)
                    .py(4.0)
                    .child(metric("Processed tokens"))
                    .child(metric("Cached input"))
                    .child(metric("Uncached input"))
                    .child(metric("Output"))
                    .child(metric("Cache savings")),
            ),
        )
        .child(col().w_full().gap(10.0).child(sk(112.0, 20.0)).child(el().w_full().h(8.0).radius(radius::SM).fill(bar)).child(sk(288.0, 16.0)))
        .child(
            col()
                .w_full()
                .gap(12.0)
                .child(row().w_full().center_y().child(h2(t, "Breakdown")).child(spacer()).child(el().w(112.0).h(28.0).radius(radius::LG).fill(bar)))
                .child(el().w_full().h(176.0).radius(radius::SM).fill(bar)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    const IST: Zone = Zone::Fixed(330 * MINUTE);

    fn ui_in(zone: Zone, window_days: i64) -> Ui {
        Ui { zone_name: "Asia/Kolkata".into(), zone, window_days, ..Ui::default() }
    }

    #[test]
    fn windows_are_calendar_days_and_rolling_hours_in_the_viewers_zone() {
        // 2026-11-07T20:00:30Z is 01:30 on Nov 8 in Kolkata.
        let now = iso_ms("2026-11-07T20:00:30.000Z").unwrap();
        let tags = json!([{"provider": "codex", "apiProvider": "", "model": "m", "pricedAs": "gpt-5"}]);
        let days = input(&ui_in(IST, 7), now, &tags);
        assert_eq!(days, json!({"sinceDay": "2026-11-02", "untilDay": "2026-11-08", "timeZone": "Asia/Kolkata", "modelAliases": tags, "resolution": "day"}));
        let hours = input(&ui_in(IST, 1), now, &json!([]));
        assert_eq!(
            hours,
            json!({"sinceDay": "2026-11-07", "untilDay": "2026-11-08", "timeZone": "Asia/Kolkata", "modelAliases": [], "resolution": "hour", "sinceTime": "2026-11-06T20:00:00.000Z", "untilTime": "2026-11-07T20:00:00.000Z"})
        );
        // The minute ticking over keeps the fetched window; a new tag refetches.
        let later = input(&ui_in(IST, 1), now + 5 * MINUTE, &json!([]));
        assert_eq!(window_key(&later), window_key(&hours));
        assert_ne!(window_key(&input(&ui_in(IST, 1), now, &tags)), window_key(&hours));
    }

    #[test]
    fn hour_labels_read_the_local_clock() {
        let at = iso_ms("2026-11-07T09:37:00.000Z").unwrap();
        let c = Clock { zone: IST, hour12: true, now: iso_ms("2026-11-07T20:00:00.000Z").unwrap() };
        assert_eq!(c.hour(at), "3 PM");
        assert_eq!(Clock { hour12: false, ..c }.hour(at), "15:00");
        assert_eq!(c.hour(iso_ms("2026-11-07T18:30:00.000Z").unwrap()), "12 AM");
        assert_eq!(c.date_hour(at), "Nov 7, 3 PM");
        // 20:00Z is already Nov 8 in Kolkata, so Nov 7's hours are yesterday's.
        assert_eq!(c.relative(at), "3 PM yesterday");
        assert_eq!(c.relative(iso_ms("2026-11-07T19:00:00.000Z").unwrap()), "12 AM today");
        assert_eq!(c.relative(iso_ms("2026-11-05T09:37:00.000Z").unwrap()), "Nov 5, 3 PM");
    }

    #[test]
    fn hourly_buckets_land_on_the_windows_rolling_hours() {
        let window = json!({"resolution": "hour", "sinceTime": "2026-11-06T20:07:00.000Z", "untilTime": "2026-11-07T20:07:00.000Z"});
        let bucket = |hour: &str, cost: f64| json!({"day": "2026-11-07", "hourStart": hour, "provider": "codex", "model": "m", "apiProvider": "", "totals": {"outputTokens": 10}, "costUsd": cost, "records": 2, "unpricedRecords": 1});
        let summary = json!({"buckets": [bucket("2026-11-07T19:07:00.000Z", 1.5), bucket("2026-11-06T20:07:00Z", 0.5)], "sources": []});
        let m = merge(&summary, &[], Some(&window), None);
        assert!(m.hourly);
        assert_eq!(m.periods.len(), 24);
        assert_eq!((m.periods[0].as_str(), m.periods[23].as_str()), ("2026-11-06T20:07:00.000Z", "2026-11-07T19:07:00.000Z"));
        assert_eq!(m.by_period[&m.periods[23]]["codex"].cost, 1.5);
        assert_eq!(m.by_period[&m.periods[0]]["codex"].cost, 0.5);
        assert_eq!(m.unpriced_tokens, 10.0);
        assert!(merge(&summary, &[], Some(&window), Some("codex\0\0other")).models.is_empty());
    }

    #[test]
    fn the_zone_name_comes_from_tz_then_the_localtime_link() {
        let link = Some("/usr/share/zoneinfo/Asia/Kolkata".to_string());
        assert_eq!(zone_name(None, link.clone(), None).as_deref(), Some("Asia/Kolkata"));
        assert_eq!(zone_name(Some(":America/New_York".into()), link.clone(), None).as_deref(), Some("America/New_York"));
        assert_eq!(zone_name(Some("/usr/share/zoneinfo/Europe/Berlin".into()), None, None).as_deref(), Some("Europe/Berlin"));
        assert_eq!(zone_name(Some("IST-5:30".into()), link, None), None);
        assert_eq!(zone_name(None, Some("/var/db/timezone/zoneinfo/Europe/Paris".into()), None).as_deref(), Some("Europe/Paris"));
        assert_eq!(zone_name(None, None, Some("Etc/UTC\n".into())).as_deref(), Some("Etc/UTC"));
        assert_eq!(zone_name(None, None, None), None);
    }

    #[test]
    fn the_pointer_picks_the_nearest_period_point() {
        assert_eq!(hovered_column(0.0, 300.0, 4), Some(0));
        assert_eq!(hovered_column(149.0, 300.0, 4), Some(1));
        assert_eq!(hovered_column(151.0, 300.0, 4), Some(2));
        assert_eq!(hovered_column(900.0, 300.0, 4), Some(3));
        assert_eq!(hovered_column(-5.0, 300.0, 1), Some(0));
        assert_eq!(hovered_column(10.0, 300.0, 0), None);
    }

    #[test]
    fn token_counts_drop_only_an_all_zero_fraction() {
        assert_eq!(tokens(1_500_000.0), "1.50M");
        assert_eq!(tokens(2_000_000.0), "2M");
        assert_eq!(tokens(12_300.0), "12.3K");
        assert_eq!(tokens(250_000_000_000.0), "250B");
        assert_eq!(tokens(999.0), "999");
    }

    #[test]
    fn source_messages_skip_ready_sources_and_fold_malformed_counts() {
        let warning = |n: i64| format!("{n} malformed or inconsistent usage record(s) were skipped; usage may be incomplete.");
        let fp = |provider: &str, home: &str| json!({"hostId": "h", "provider": provider, "resolvedHomePath": home, "volumeId": "v"});
        let summary = json!({"sources": [
            {"fingerprint": fp("codex", "/a"), "status": "ready", "malformedRecords": 0, "message": "fine"},
            {"fingerprint": fp("claude", "/b"), "status": "failed", "malformedRecords": 0, "message": "Could not read ~/.claude."},
            {"fingerprint": fp("claude", "/c"), "status": "partial", "malformedRecords": 2, "message": format!("Read failed. {}", warning(2))},
            {"fingerprint": fp("claude", "/c"), "status": "partial", "malformedRecords": 3, "message": warning(3)},
            {"fingerprint": fp("codex", "/d"), "status": "missing", "malformedRecords": 1},
            {"fingerprint": fp("grok", "/e"), "status": "failed", "malformedRecords": 0, "message": "Grok is hidden."},
        ]});
        assert_eq!(source_messages(&summary, &["grok"]), vec!["Could not read ~/.claude.".to_string(), "Read failed.".into(), warning(4)]);
    }

    #[test]
    fn the_breakdown_orders_models_by_the_metric_and_lists_active_periods_newest_first() {
        let line = |key: &str, cost: f64, tokens: f64| ModelRow { key: key.into(), totals: Totals { cost, tokens }, ..ModelRow::default() };
        let models = vec![line("a", 5.0, 10.0), line("b", 1.0, 90.0)];
        let names = |v: Vec<&ModelRow>| v.iter().map(|m| m.key.clone()).collect::<Vec<_>>();
        assert_eq!(names(by_metric(&models, false)), ["a", "b"]);
        assert_eq!(names(by_metric(&models, true)), ["b", "a"]);

        let mut m = Merged { periods: vec!["2026-11-01".into(), "2026-11-02".into(), "2026-11-03".into()], ..Merged::default() };
        for day in ["2026-11-01", "2026-11-03"] {
            m.by_period.entry(day.into()).or_default().insert("codex", Totals::default());
        }
        assert_eq!(active_periods(&m), ["2026-11-03", "2026-11-01"]);
        assert!(active_periods(&Merged::default()).is_empty());
    }

    #[test]
    fn tagged_buckets_share_one_row_and_unpriced_rows_stay_taggable() {
        let bucket = |api: &str, model: &str, source: &str, priced_as: Value, records: f64, unpriced: f64| {
            let cost = if unpriced >= records { 0.0 } else { 1.0 };
            json!({"provider": "pi", "apiProvider": api, "model": model, "costSource": source, "pricedAs": priced_as, "day": "2026-11-01", "records": records, "unpricedRecords": unpriced, "costUsd": cost, "totals": {"outputTokens": 10}})
        };
        let opus = json!("anthropic/claude-opus-5");
        let summary = json!({"buckets": [
            bucket("agentrouter", "opus-5", "userTagged", opus.clone(), 2.0, 0.0),
            bucket("tokenrouter", "opus-latest", "userTagged", opus, 3.0, 0.0),
            bucket("moonshot", "kimi-k3", "unpriced", Value::Null, 4.0, 4.0),
        ]});
        let m = merge(&summary, &[], None, None);
        let rows: Vec<_> = m.models.iter().map(|r| (r.key.as_str(), r.model.as_str(), r.api(), r.tagged, r.unpriced_only, r.cost_unknown())).collect();
        assert_eq!(
            rows,
            [("pi\0tag\0anthropic/claude-opus-5", "anthropic/claude-opus-5", "", true, false, false), ("pi\0moonshot\0kimi-k3", "kimi-k3", "moonshot", false, true, true)]
        );
        assert_eq!(merge(&summary, &[], None, Some("pi\0tag\0anthropic/claude-opus-5")).records, 5.0);
    }

    #[test]
    fn page_preferences_round_trip_and_ignore_unknown_values() {
        let mut u = Ui::default();
        apply_preferences(&mut u, &json!({"metric": "tokens", "windowDays": 7, "hiddenProviders": ["grok", "nope", "codex"]}));
        assert_eq!((u.metric, u.window_days, u.hidden.clone()), ("tokens", 7, vec!["codex", "grok"]));
        let mut back = Ui::default();
        apply_preferences(&mut back, &preferences(&u));
        assert_eq!((back.metric, back.window_days, back.hidden), ("tokens", 7, vec!["codex", "grok"]));

        let mut fresh = Ui::default();
        apply_preferences(&mut fresh, &json!({"metric": "watts", "windowDays": 3}));
        assert_eq!((fresh.metric, fresh.window_days, fresh.preferences_read), ("limits", 30, true));
    }

    #[test]
    fn shifted_digits_resolve_to_the_bound_digit() {
        assert_eq!(unshifted('!'), Some('1'));
        assert_eq!(unshifted('$'), Some('4'));
        assert_eq!(unshifted('1'), None);
    }

    #[test]
    fn the_environment_filter_reads_all_once_every_environment_is_checked() {
        let all = ["".to_string(), "studio".into(), "lab".into()];
        assert_eq!(toggle_environment(&all, &all, "studio", false), Some(vec!["".to_string(), "lab".into()]));
        assert_eq!(toggle_environment(&all, &["".to_string(), "lab".into()], "studio", true), None);
        assert_eq!(toggle_environment(&all, &[], "lab", true), Some(vec!["lab".to_string()]));
    }

    #[test]
    fn cursor_asks_for_the_keychain_only_while_no_scan_reads_its_account() {
        let asking = json!({"sources": [{"fingerprint": {"provider": "cursor", "hostId": "devbox"}, "status": "missing", "action": "enableCursorKeychain"}]});
        let ready = json!([{"driver": "cursor", "status": "ready"}]);
        assert!(needs_cursor_keychain(&asking, &ready));
        assert!(!needs_cursor_keychain(&asking, &json!([{"driver": "cursor", "status": "disabled"}])));
        let account = json!({"sources": [{"fingerprint": {"provider": "cursor", "hostId": "cursor.com"}, "status": "ok"}]});
        let studio = EnvUsage { needs_cursor: true, ..env("studio", Some(&asking)) };
        assert_eq!(cursor_access_environments(&[env("", None), studio], &[]), [("studio".to_string(), "STUDIO".to_string())]);
        let studio = EnvUsage { needs_cursor: true, ..env("studio", Some(&asking)) };
        assert!(cursor_access_environments(&[env("", Some(&account)), studio], &[]).is_empty());
        let studio = EnvUsage { needs_cursor: true, ..env("studio", Some(&asking)) };
        assert!(cursor_access_environments(&[studio], &["cursor"]).is_empty());
    }

    fn env<'a>(key: &str, summary: Option<&'a Value>) -> EnvUsage<'a> {
        EnvUsage { key: key.into(), label: key.to_uppercase(), connected: true, pending: false, error: None, summary, needs_cursor: false }
    }

    fn answering(providers: &[&str]) -> Value {
        let sources: Vec<Value> = providers.iter().map(|p| json!({"fingerprint": {"provider": p}, "refreshing": true})).collect();
        json!({"contractVersion": usage_merge::CONTRACT_VERSION, "sources": sources})
    }

    #[test]
    fn the_filter_status_follows_each_environments_scan() {
        let (none, one, two, old) = (answering(&[]), answering(&["claude"]), answering(&["claude", "codex"]), json!({"contractVersion": 3, "sources": []}));
        let failed = EnvUsage { error: Some("This environment could not report usage.".into()), ..env("a", None) };
        assert_eq!(status_label(&failed, false), "Unavailable");
        assert_eq!(progress(&failed, false), Phase::Inactive);
        assert_eq!(status_label(&env("a", Some(&old)), false), "Update required");
        assert_eq!(status_label(&EnvUsage { connected: false, pending: true, ..env("a", None) }, false), "Connecting…");
        assert_eq!(status_label(&EnvUsage { pending: true, ..env("a", None) }, false), "Scanning…");
        assert_eq!(status_label(&env("a", Some(&none)), true), "Refreshing…");
        assert_eq!(status_label(&EnvUsage { pending: true, ..env("a", Some(&none)) }, false), "Refreshing…");
        assert_eq!(status_label(&EnvUsage { pending: true, ..env("a", Some(&one)) }, false), "Updating Claude Code…");
        assert_eq!(progress(&EnvUsage { pending: true, ..env("a", Some(&two)) }, false), Phase::Partway(vec!["claude".into(), "codex".into()]));
        assert_eq!(status_label(&EnvUsage { pending: true, ..env("a", Some(&two)) }, false), "Updating 2 providers…");
        assert_eq!(status_label(&env("a", Some(&two)), false), "Ready");
    }

    #[test]
    fn totals_are_partial_while_a_selected_environment_scans_and_its_providers_wait() {
        let grok = answering(&["grok"]);
        let answered = || EnvUsage { pending: true, ..env("a", Some(&grok)) };
        let scanning = || EnvUsage { pending: true, ..env("b", None) };
        let failed = || EnvUsage { error: Some("This environment is not connected.".into()), connected: false, ..env("c", None) };
        assert_eq!(reporting(&[answered(), scanning(), failed()]), (1, 1));
        assert_eq!(reporting(&[failed()]), (0, 0));
        assert_eq!(loading(&[answered(), failed()], false, false, &[]), (false, vec!["grok".to_string()]));
        assert_eq!(loading(&[answered()], false, false, &["grok"]), (false, Vec::<String>::new()));
        assert_eq!(loading(&[answered(), scanning()], false, false, &[]), (true, vec!["grok".to_string()]));
        assert_eq!(loading(&[answered()], false, true, &[]), (true, Vec::<String>::new()));
    }

    #[test]
    fn the_coverage_notice_names_failures_excluded_contracts_and_shared_directories() {
        let failed = EnvUsage { error: Some("This environment could not report usage.".into()), ..env("a", None) };
        let merged = json!({"mismatches": [{"environment": "b", "serverBehind": true}], "duplicates": ["B: /home/.claude", "B: /home/.codex"]});
        assert_eq!(
            coverage_lines(&[failed, env("b", None)], &merged),
            [
                "A: This environment could not report usage.",
                "B runs an older server version and is excluded from totals.",
                "Counted once across environments sharing a transcript directory: B: /home/.claude, B: /home/.codex",
            ]
        );
        assert!(coverage_lines(&[env("a", None)], &Value::Null).is_empty());
    }

    #[test]
    fn selected_answers_merge_without_hidden_providers_counting_a_shared_directory_once() {
        let source = |provider: &str| json!({"fingerprint": {"hostId": "mac", "provider": provider, "resolvedHomePath": format!("/{provider}"), "volumeId": "v"}, "status": "ok", "distinctSessions": 3});
        let bucket = |provider: &str, cost: f64| json!({"day": "2026-11-01", "provider": provider, "model": "m", "apiProvider": "", "totals": {"outputTokens": 10}, "costUsd": cost, "records": 1, "unpricedRecords": 0});
        let summary = json!({"contractVersion": usage_merge::CONTRACT_VERSION, "readAt": "2026-11-02T00:00:00.000Z", "buckets": [bucket("codex", 1.0), bucket("claude", 2.0)], "sources": [source("codex"), source("claude")]});
        let window = json!({"sinceDay": "2026-11-01", "untilDay": "2026-11-02"});
        let merged = merged_summary(&[env("a", Some(&summary)), env("b", Some(&summary))], &["claude"], &window);
        assert_eq!(merged["buckets"].as_array().map(Vec::len), Some(1));
        assert_eq!(merged["sources"].as_array().map(Vec::len), Some(2));
        assert_eq!(merged["sessions"], json!({"codex": 3}));
        assert_eq!(merged["duplicates"], json!(["B: /codex"]));
        let m = merge(&merged, &[], None, None);
        assert_eq!((m.cost, m.sessions, m.periods.len()), (1.0, 3, 2));
        assert!(merged_summary(&[env("a", None)], &[], &window)["buckets"].as_array().is_some_and(Vec::is_empty));
    }

    #[test]
    fn a_window_switched_back_to_shows_its_cached_answer_and_asks_again_once_stale() {
        let settings = json!([null, null, null, null]);
        let (a, b) = (json!(["2026-10-08", "day"]), json!(["2026-11-05", "day"]));
        let answer = |n: i64| Some(json!({"buckets": [n]}));
        let mut scan = Scan { summary: answer(1), answered: Some(a.clone()), sent: Some((json!({}), settings.clone())), pending: Some((7, a.clone())), ..Scan::default() };
        assert!(!scan.show(&b, &settings, 1_000));
        assert_eq!((scan.summary.clone(), scan.pending.clone()), (None, None), "nothing held for b, and a's request is dropped");
        scan.summary = answer(2);
        scan.answered = Some(b.clone());
        scan.at = 2_000;

        assert!(scan.show(&a, &settings, 30_000), "a answered within the stale time, so it is not asked again");
        assert_eq!((scan.summary.clone(), scan.answered.clone()), (answer(1), Some(a.clone())));
        assert!(!scan.show(&b, &settings, 70_000), "b is shown at once but asked again");
        assert_eq!(scan.summary, answer(2));

        let filtered = json!([null, null, null, ["codex"]]);
        assert!(!scan.show(&a, &filtered, 80_000));
        assert_eq!(scan.summary, None, "another model filter is another query");
        let repriced = json!([{"m": 1}, null, null, null]);
        assert!(!scan.show(&b, &repriced, 80_000));
        assert_eq!(scan.summary, answer(2), "changed prices keep the answer on screen while it is asked again");
        assert!(!scan.show(&a, &settings, 70_000 + IDLE_MS));
        assert_eq!(scan.summary, None, "a window unseen past the idle TTL is dropped");
    }

    #[test]
    fn the_chart_draws_providers_with_figures_or_still_updating_and_marks_partial_figures() {
        let bucket = |provider: &str, day: &str, cost: f64, tokens: i64| json!({"provider": provider, "day": day, "costUsd": cost, "totals": {"outputTokens": tokens}});
        let summary = json!({"sinceDay": "2026-08-01", "untilDay": "2026-08-02", "buckets": [
            bucket("codex", "2026-08-01", 2.0, 100), bucket("claude", "2026-08-01", 0.0, 0), bucket("opencode", "2026-08-02", 0.0, 40),
        ]});
        let m = merge(&summary, &[], None, None);
        let active = chart_providers(&m, &["grok".to_string()]);
        assert_eq!(active, ["codex", "grok", "opencode"], "Claude Code has buckets but no figures");
        assert_eq!(series(&m, &active, &|x: &Totals| x.cost), ["codex", "grok", "opencode"]);
        assert_eq!(series(&m, &active, &|x: &Totals| x.tokens), ["codex", "opencode", "grok"]);

        let cost = |x: &Totals| x.cost;
        let (rows, total, partial) = tooltip_figures(&m, "2026-08-01", &active, &["grok"], &cost);
        assert_eq!(rows, [("codex", 2.0, false), ("grok", 0.0, true), ("opencode", 0.0, false)]);
        assert_eq!((total, partial), (2.0, true));
        assert!(!tooltip_figures(&m, "2026-08-02", &active, &[], &cost).2);
    }
}
