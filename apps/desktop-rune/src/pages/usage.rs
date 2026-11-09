//! `/usage` (UsagePage): a route modal over the frame with subscription limits per provider
//! account, and local cost or token usage from `server.getUsageSummary` with its chart, totals,
//! share bars and breakdown tables.

use std::collections::HashMap;

use rune::{Color, El, LineCap, Path, ScrollSpec, Shape, Tx, canvas, col, el, icon, row, spacer, text};
use serde_json::{Value, json};

use crate::chat::composer::provider_icon;
use crate::data::{Zone, iso_ms, s as field};
use crate::icons as i;
use crate::kit::{self, Menu, MenuItem, Placement, ToggleGroup};
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route};

pub struct Ui {
    pub metric: &'static str,
    /// Days in the window; 1 is the hourly "Past 24h".
    pub window_days: i64,
    pub summary: Option<Value>,
    pub error: Option<String>,
    pub pending: Option<u64>,
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
    /// `server.refreshUsageRates` (true) or `server.refreshProviders` (false) in flight.
    refreshing: Option<(u64, bool)>,
    /// Rates refreshed; the rescanned summary is still on its way.
    reloading: bool,
    /// The request the shown summary answers, kept on screen while another window loads.
    pub answered: Option<Value>,
    /// Price, mapping and filter settings the summary was scanned under (usageScanSettingsAtom).
    scan: Option<Value>,
    pub preferences_read: bool,
}

impl Default for Ui {
    fn default() -> Self {
        let (zone_name, zone) = local_zone();
        Ui {
            metric: "limits",
            window_days: 30,
            summary: None,
            error: None,
            pending: None,
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
            refreshing: None,
            reloading: false,
            answered: None,
            scan: None,
            preferences_read: false,
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
    Clock { zone: u.zone, hour12: format != "24-hour", now: until }
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

fn iso_of(ms: i64) -> String {
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

/// The settings a scan depends on: prices, mappings and the Cursor keychain opt-in refetch the
/// summary when they change, and `usageModelFilter` rides along in the request (usageSummaryInput).
fn scan_settings(s: &App) -> Value {
    let settings = s.state().config.as_ref().map(|c| &c["settings"]).unwrap_or(&Value::Null);
    json!([settings["usagePriceOverrides"], settings["usageModelAliases"], settings["cursorKeychainUsageEnabled"], settings["usageModelFilter"]])
}

/// What a fetched summary answers; the rolling hour bounds alone don't refetch.
fn window_key(input: &Value) -> Value {
    json!([input["sinceDay"], input["resolution"], input["timeZone"], input["modelAliases"]])
}

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
    if let Some((id, rates)) = s.ui.pages.usage.refreshing
        && let Some(_) = s.src.net().and_then(|n| n.take_reply(id))
    {
        let u = ui(s);
        u.refreshing = None;
        // Rates landed (a failed refresh still rescans, like refreshUsage): fetch again.
        if rates {
            u.requested = None;
            u.reloading = true;
        }
        changed = true;
    }
    if s.ui.pages.usage.metric == "limits" {
        return changed;
    }
    if let Some(id) = s.ui.pages.usage.pending
        && let Some(reply) = s.src.net().and_then(|n| n.take_reply(id))
    {
        let u = ui(s);
        u.pending = None;
        u.reloading = false;
        match reply {
            Ok(summary) => {
                u.summary = Some(summary);
                u.answered = u.requested.clone();
                u.error = None;
            }
            Err(_) => {
                u.summary = None;
                u.error = Some("This environment could not report usage.".into());
            }
        }
        changed = true;
    }
    let scan = scan_settings(s);
    let mut wanted = input(&s.ui.pages.usage, s.now_ms, &model_aliases(s));
    if !scan[3].is_null() {
        wanted["modelFilter"] = scan[3].clone();
    }
    let current = s.ui.pages.usage.requested.as_ref().map(window_key);
    if current != Some(window_key(&wanted)) || s.ui.pages.usage.scan.as_ref() != Some(&scan) {
        s.ui.pages.usage.requested = Some(wanted.clone());
        s.ui.pages.usage.scan = Some(scan);
        match s.src.net() {
            // The last summary stays up, dimmed, until this one answers (useUsage's `shown`).
            Some(net) => {
                let id = net.request("server.getUsageSummary", wanted);
                ui(s).pending = Some(id);
            }
            None if !crate::host::live() => {
                if s.ui.pages.usage.summary.is_none() {
                    s.ui.pages.usage.summary = super::fixture("usage.json").map(|f| f["summary"].clone());
                }
                s.ui.pages.usage.answered = s.ui.pages.usage.requested.clone();
            }
            None => {}
        }
        changed = true;
    }
    changed
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

/// The header's Refresh: limits re-read provider status; usage refreshes model rates and then
/// rescans the window from now, keeping the current figures up meanwhile.
fn refresh(s: &mut App) {
    if refreshing(&s.ui.pages.usage) {
        return;
    }
    let rates = s.ui.pages.usage.metric != "limits";
    let (method, payload) = if rates { ("server.refreshUsageRates", json!({})) } else { ("server.refreshProviders", json!({})) };
    match s.src.net() {
        Some(net) => {
            let id = net.request(method, payload);
            ui(s).refreshing = Some((id, rates));
        }
        None => ui(s).requested = None,
    }
}

fn refreshing(u: &Ui) -> bool {
    u.refreshing.is_some() || u.reloading
}

/// Figures from another window, or mid-refresh, read as stale (`figureClass`).
fn stale(u: &Ui) -> bool {
    u.reloading || (u.pending.is_some() && u.answered.as_ref().map(window_key) != u.requested.as_ref().map(window_key))
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
    for src in summary["sources"].as_array().into_iter().flatten() {
        *sessions.entry(field(&src["fingerprint"], "provider")).or_default() += src["distinctSessions"].as_i64().unwrap_or(0);
    }
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
    let connected = s.state().config.is_some();
    page = if !connected {
        page.child(muted_line(t, if u.metric == "limits" { "Connect an environment to see limits." } else { "Connect an environment to see usage." }))
    } else if u.metric == "limits" {
        page.child(super::usage_limits::view(s, cx, t, &mut overlays))
    } else if let Some(error) = u.error.as_ref().filter(|_| u.summary.is_none()) {
        page.child(col().w_full().py(48.0).items_center().child(text(error.clone()).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap().align(rune::Align::Center)))
    } else if let Some(summary) = &u.summary {
        overview(s, t, summary, page, &mut overlays)
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
    let crumb = |label: String, current: bool| {
        row()
            .gap(4.0)
            .center_y()
            .cursor(rune::Cursor::Pointer)
            .child(text(label).font(theme::SM_MEDIUM).color(if current { t.c.foreground } else { t.c.muted_foreground }))
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
        .trigger(crumb(provider_label, true).on_press(toggle("providers")))
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
    let target = super::usage_prices::target(s);
    let mut envs = Menu::new("usage-environments")
        .trigger(crumb("All environments".into(), false).on_press(toggle("environments")))
        .placement(Placement::BottomStart)
        .min_width(220.0)
        .open(u.menu == Some("environments"))
        .on_close(|s: &mut App| ui(s).menu = None)
        .item(MenuItem::new("All environments").checked(true).on_select(|s: &mut App| ui(s).menu = Some("environments")))
        .separator();
    let status = if u.metric == "limits" {
        None
    } else if u.error.is_some() {
        Some("Unavailable")
    } else if u.pending.is_some() && u.summary.is_none() {
        Some("Scanning…")
    } else if u.pending.is_some() || refreshing(u) {
        Some("Refreshing…")
    } else {
        Some("Ready")
    };
    let mut env_item = MenuItem::new(target.label).checked(true).on_select(|s: &mut App| ui(s).menu = Some("environments"));
    if let Some(status) = status {
        env_item = env_item.shortcut(status);
    }
    envs = envs.item(env_item).separator().item(MenuItem::new("Model prices").icon(&i::SLIDERS_HORIZONTAL).on_select(|s: &mut App| {
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
                .disabled(refreshing(u))
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
    let dim = if stale(u) { 0.4 } else { 1.0 };

    let sessions: i64 = m.providers.iter().map(|p| p.2).sum();
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
    let mut rows = col().gap(16.0);
    for &(kind, totals, sessions) in &m.providers {
        let (label, driver) = presentation(kind);
        let share = if tokens_view { totals.tokens / m.tokens.max(1.0) } else { totals.cost / m.cost.max(f64::MIN_POSITIVE) };
        let sub = if tokens_view { format!("{} of tokens · {}", percent(share), usd(totals.cost)) } else { format!("{} of cost · {} tokens", percent(share), tokens(totals.tokens)) };
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
                        .child(text(sessions_label(sessions)).font(theme::TEXT_2XS).color(t.c.muted_foreground).opacity(dim))
                        .child(spacer())
                        .child(text(fmt(value(totals))).font(tnum(theme::SM_MEDIUM)).color(t.c.foreground).opacity(dim)),
                )
                .child(text(sub).font(theme::TEXT_XS).color(t.c.muted_foreground).opacity(dim)),
        );
    }
    left = left.child(rows);
    let hourly = m.hourly;
    let chart_title = match (hourly, tokens_view) {
        (false, false) => "Daily cost",
        (false, true) => "Daily processed tokens",
        (true, false) => "Hourly cost",
        (true, true) => "Hourly processed tokens",
    };
    let right = col().grow(1.0).shrink().min_w(0.0).gap(12.0).child(h2(t, chart_title)).child(chart(t, &m, tokens_view, u.hover, u.plot_w, false));
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
    let mut body = col().w_full().gap(32.0).child(grid).child(chart(t, &m, tokens_view, u.model_hover, u.model_plot_w, true)).child(shares);
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

/// UsageChartTooltip: the hovered period, each provider's value, and the total, beside the
/// pointer and flipped to stay inside the plot.
fn chart_tooltip(t: &T, m: &Merged, index: usize, (x, y): (f32, f32), plot: (f32, f32), fmt: &dyn Fn(&Totals) -> f64, tokens_view: bool) -> El {
    let format = |v: f64| if tokens_view { tokens(v) } else { usd(v) };
    let period = m.periods.get(index).cloned().unwrap_or_default();
    let values = m.by_period.get(&period);
    let muted = t.c.muted_foreground;
    let num = theme::TEXT_XS.features("'tnum' 1");
    let mut total = 0.0;
    let title = match iso_ms(&period).filter(|_| m.hourly) {
        Some(at) => m.clock.relative(at),
        None => period_label(&period, m.clock),
    };
    let mut body = col().child(text(title).font(theme::TEXT_XS).color(muted).mb(4.0));
    for (kind, _, _) in &m.providers {
        let v = values.and_then(|p| p.get(kind)).map(fmt).unwrap_or(0.0);
        total += v;
        let (label, driver) = presentation(kind);
        body = body.child(
            row()
                .gap(12.0)
                .center_y()
                .child(row().gap(6.0).center_y().child(provider_icon(t, driver, 12.0)).child(text(label).font(theme::TEXT_XS).color(muted)))
                .child(spacer())
                .child(text(format(v)).font(num).color(t.c.foreground)),
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
            .child(text(format(total)).font(num).color(t.c.foreground)),
    );
    // The tooltip isn't measured before it's placed, so flip on its estimated size.
    let (w, h) = (176.0, 46.0 + 18.0 * m.providers.len() as f32);
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

fn chart(t: &T, m: &Merged, tokens_view: bool, hover: Option<(usize, f32, f32)>, plot_w: f32, dialog: bool) -> El {
    let value = |x: &Totals| if tokens_view { x.tokens } else { x.cost };
    let peak = m.by_period.values().flat_map(|p| p.values()).map(value).fold(0.0, f64::max);
    let (max, ticks) = nice_scale(peak);
    let to_y = |v: f64| if max == 0.0 { VIEW_H } else { VIEW_H - (v / max) as f32 * (VIEW_H - PLOT_TOP) };
    let plot_h = 224.0;
    let mut axis = el().w(56.0).h(plot_h).shrink();
    for tick in &ticks {
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
    let mut series: Vec<(&str, f64)> = m.providers.iter().map(|p| (p.0, value(&p.1))).collect();
    series.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut lines = Vec::new();
    for (kind, _) in &series {
        let pts: Vec<[f32; 2]> = m
            .periods
            .iter()
            .enumerate()
            .map(|(n, p)| [n as f32 * step, to_y(m.by_period.get(p).and_then(|x| x.get(kind)).map(value).unwrap_or(0.0))])
            .collect();
        if pts.len() < 2 {
            continue;
        }
        let c = color(t, kind);
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
    if let Some((index, x, y)) = hover {
        plot = plot.child(chart_tooltip(t, m, index, (x, y), (plot_w, plot_h), &value, tokens_view));
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
}
