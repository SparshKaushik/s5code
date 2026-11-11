//! UsageLimitsPooled: subscription limits pooled per provider across every signed-in account,
//! one segment per account in each window, the account's details in a popover on the segment,
//! and its banked reset credits redeemable from there.

use std::collections::HashMap;
use std::time::Duration;

use rune::{Color, El, LineCap, Shape, canvas, col, el, icon, row, spacer, text};
use serde_json::{Value, json};

use crate::App;
use crate::chat::composer::provider_icon;
use crate::data::{Zone, iso_ms, s as field};
use crate::icons as i;
use crate::kit::{self, Placement};
use crate::theme::{self, AlphaMul, T, radius};

const MINUTE: i64 = 60_000;
const DAY: i64 = 86_400_000;

/// Where a reset credit is redeemed: the environment's key and `provider.consumeResetCredit`'s input.
type Redeem = (String, Value);

#[derive(Default)]
pub struct Ui {
    /// The segment whose popover is open, and whether the pointer rests on that popup.
    pub segment: Option<String>,
    over_popup: bool,
    email_shown: bool,
    /// The segment and redemption awaiting "Use a reset credit?".
    pub confirm: Option<(String, Redeem)>,
    /// The request, its segment and its environment.
    redeeming: Option<(u64, String, String)>,
    /// What each segment's last redemption said (useResetCredit's status).
    status: HashMap<String, String>,
    /// `server.refreshProviders` in flight per environment, and whether the header asked for it.
    refreshing: Vec<(String, u64)>,
    manual: bool,
    /// When each environment may refresh on its own again (refreshUsageLimits' five minutes).
    refresh_after: HashMap<String, i64>,
    /// The connected environments the last automatic refresh covered.
    connected: String,
}

fn ui(s: &mut App) -> &mut Ui {
    &mut s.ui.pages.usage.limits
}

fn env_net<'a>(s: &'a mut App, environment: &str) -> Option<&'a mut s5_net::Net> {
    crate::env::source_mut(s, environment).and_then(|src| src.net())
}

pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    if let Some((id, key, environment)) = s.ui.pages.usage.limits.redeeming.clone()
        && let Some(reply) = env_net(s, &environment).and_then(|n| n.take_reply(id))
    {
        let u = ui(s);
        u.redeeming = None;
        u.status.insert(key, redeemed(&reply));
        changed = true;
    }
    let now = s.now_ms;
    for (environment, id) in std::mem::take(&mut ui(s).refreshing) {
        if env_net(s, &environment).is_none_or(|n| n.take_reply(id).is_some()) {
            ui(s).refresh_after.insert(environment, now + 5 * 60_000);
            changed = true;
        } else {
            ui(s).refreshing.push((environment, id));
        }
    }
    if ui(s).refreshing.is_empty() {
        ui(s).manual = false;
    }
    // Like the page's effect: entering Limits, or the connected set changing, refreshes them.
    let set = if s.ui.pages.usage.metric == "limits" { connected(s).join(",") } else { String::new() };
    if set != s.ui.pages.usage.limits.connected {
        ui(s).connected = set.clone();
        if !set.is_empty() {
            refresh(s, true);
            changed = true;
        }
    }
    changed
}

/// The selected environments that are connected and have sent their config.
fn connected(s: &App) -> Vec<String> {
    let mut keys: Vec<String> = super::usage::selected_environments(s)
        .into_iter()
        .filter(|k| crate::env::status(s, k) == "Connected" && crate::env::source(s, k).is_some_and(|src| src.state().config.is_some()))
        .collect();
    keys.sort();
    keys
}

/// refreshLimits: `server.refreshProviders` on each selected connected environment that has none
/// in flight; an automatic refresh skips one refreshed in the last five minutes.
pub(super) fn refresh(s: &mut App, automatic: bool) {
    let now = s.now_ms;
    for environment in connected(s) {
        let u = &s.ui.pages.usage.limits;
        if u.refreshing.iter().any(|(k, _)| *k == environment) || (automatic && u.refresh_after.get(&environment).is_some_and(|at| now < *at)) {
            continue;
        }
        let Some(net) = env_net(s, &environment) else { continue };
        let id = net.request("server.refreshProviders", json!({}));
        ui(s).refreshing.push((environment, id));
    }
    if !automatic && !s.ui.pages.usage.limits.refreshing.is_empty() {
        ui(s).manual = true;
    }
}

/// Leaving the page: coming back to Limits refreshes them again, as a remount does.
pub(super) fn closed(u: &mut Ui) {
    u.connected.clear();
}

/// The header's spinner: a refresh the user asked for is still out.
pub(super) fn busy(u: &Ui) -> bool {
    u.manual
}

/// Escape closes the reset confirmation, then an open segment popover.
pub fn on_key(s: &mut App, e: &rune::KeyEvent) -> bool {
    if !matches!(e.key, rune::Key::Escape) {
        return false;
    }
    let u = ui(s);
    u.confirm.take().is_some() || u.segment.take().is_some()
}

/// useResetCredit's status line for a `provider.consumeResetCredit` reply.
fn redeemed(reply: &Result<Value, Value>) -> String {
    match reply {
        Ok(v) => v["warning"].as_str().map(str::to_string).unwrap_or_else(|| {
            match field(v, "outcome") {
                "reset" => "Reset applied. Your windows have cleared.",
                "nothingToReset" => "Nothing to reset right now.",
                "noCredit" => "No reset credit left.",
                _ => "That credit was already redeemed.",
            }
            .to_string()
        }),
        Err(e) => e["message"].as_str().map(str::to_string).unwrap_or_else(|| "Could not use the reset credit.".into()),
    }
}

// ---- accounts ---------------------------------------------------------------------------

/// PROVIDER_DISPLAY_NAMES, which match the provider clients' labels.
fn driver_label(driver: &str) -> Option<&'static str> {
    Some(match driver {
        "antigravity" => "Antigravity",
        "codex" => "Codex",
        "claudeAgent" => "Claude",
        "cursor" => "Cursor",
        "grok" => "Grok",
        "fx" => "fx",
        "kiro" => "Kiro",
        "muse" => "Muse Code",
        "acpRegistry" => "ACP Registry",
        "pi" => "Pi",
        "opencode" => "OpenCode",
        _ => return None,
    })
}

const CURSOR_WINDOWS: [(&str, &str, &str); 3] = [
    ("totalPercentUsed", "Overall", "Combined usage across both allowances, not a third quota."),
    ("autoPercentUsed", "Cursor Models", "Grok and Composer use this first. Auto can use either pool."),
    ("apiPercentUsed", "Other Models", "Claude, GPT, and Gemini use this pool. Grok and Composer fall back here."),
];

fn cursor_rank(id: &str) -> usize {
    CURSOR_WINDOWS.iter().position(|w| w.0 == id).unwrap_or(CURSOR_WINDOWS.len())
}

#[derive(Clone, Debug, PartialEq)]
struct Account {
    key: String,
    driver: String,
    display_name: Option<String>,
    email: Option<String>,
    plan: Option<String>,
    accent: Option<String>,
    /// Where the login is signed in, as (environment key, label).
    environments: Vec<(String, String)>,
    source_label: Option<String>,
    redeem: Option<Redeem>,
    limits: Value,
}

/// One selected environment's limits presentation: its key, label and visible config.
struct Env {
    key: String,
    label: String,
    config: Value,
}

fn list(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn windows(limits: &Value) -> &[Value] {
    list(&limits["windows"])
}

fn used(w: &Value) -> f64 {
    w["usedPercent"].as_f64().unwrap_or(0.0)
}

fn text_of(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

fn checked(limits: &Value) -> Option<i64> {
    limits["checkedAt"].as_str().and_then(iso_ms)
}

/// `Date.parse(a.checkedAt) > Date.parse(b.checkedAt)`: an unreadable clock is never newer.
fn newer(a: &Value, b: &Value) -> bool {
    matches!((checked(a), checked(b)), (Some(a), Some(b)) if a > b)
}

/// providersWithLimits.
fn reports_limits(p: &Value) -> bool {
    p["enabled"] == true && p["installed"] == true && p["availability"] != "unavailable" && p["usageLimits"].is_object()
}

/// limitsNotice.
fn limits_notice(limits: &Value) -> Option<String> {
    let message = limits["unavailable"]["message"].as_str().map(str::to_string);
    match limits["unavailable"]["reason"].as_str() {
        Some("unsupported") => Some(message.unwrap_or_else(|| "This account has no subscription limits.".into())),
        Some("probeFailed") => Some(message.unwrap_or_else(|| "Could not read limits.".into())),
        _ if windows(limits).is_empty() => Some("No limits reported.".into()),
        _ => None,
    }
}

fn account_key(driver: &str, email: Option<&str>, limits: &Value) -> Option<String> {
    let email = email.map(|e| e.trim().to_lowercase()).filter(|e| !e.is_empty());
    match (email, limits["credentialFingerprint"].as_str().filter(|f| !f.is_empty())) {
        (Some(email), _) => Some(format!("{driver}:{email}")),
        (None, Some(fingerprint)) => Some(format!("{driver}:credential:{fingerprint}")),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct Report<'a> {
    driver: &'a str,
    email: Option<&'a str>,
    workspace: Option<&'a str>,
    limits: &'a Value,
}

impl<'a> Report<'a> {
    fn native(p: &'a Value) -> Self {
        Report { driver: field(p, "driver"), email: p["auth"]["email"].as_str(), workspace: p["auth"]["workspaceId"].as_str().filter(|w| !w.is_empty()), limits: &p["usageLimits"] }
    }

    fn hub(a: &'a Value) -> Self {
        Report { driver: field(a, "driver"), email: a["email"].as_str(), workspace: a["workspaceId"].as_str().filter(|w| !w.is_empty()), limits: &a["usageLimits"] }
    }

    fn key(&self) -> Option<String> {
        account_key(self.driver, self.email, self.limits)
    }
}

/// accountKeyResolver: one login is one account, split by workspace when any report names one.
struct Keys(HashMap<String, Vec<String>>);

impl Keys {
    fn new<'a>(reports: impl IntoIterator<Item = Report<'a>>) -> Self {
        let mut workspaces: HashMap<String, Vec<String>> = HashMap::new();
        for r in reports {
            if let (Some(key), Some(workspace)) = (r.key(), r.workspace) {
                let known = workspaces.entry(key).or_default();
                if !known.iter().any(|w| w == workspace) {
                    known.push(workspace.to_string());
                }
            }
        }
        Keys(workspaces)
    }

    fn resolve(&self, r: &Report) -> Option<String> {
        let key = r.key()?;
        if let Some(workspace) = r.workspace {
            return Some(format!("{key}:{workspace}"));
        }
        match self.0.get(&key).map(Vec::as_slice) {
            None => Some(key),
            Some([one]) => Some(format!("{key}:{one}")),
            Some(_) => None,
        }
    }
}

fn hub_name(id: &str) -> String {
    match id.len().checked_sub(5) {
        Some(at) if id.is_char_boundary(at) && id[at..].eq_ignore_ascii_case(".json") => id[..at].to_string(),
        _ => id.to_string(),
    }
}

/// collectLimitAccounts' merge: the freshest read wins, while plan, reset credits and hub
/// redemption each follow their own freshest source.
#[derive(Default)]
struct Merger {
    accounts: Vec<(String, Account)>,
    credits: HashMap<String, Account>,
    hubs: HashMap<String, Account>,
    plans: HashMap<String, Account>,
}

impl Merger {
    fn merge(&mut self, key: String, next: Account) {
        let fresher_than = |seen: Option<&Account>| seen.is_none_or(|p| newer(&next.limits, &p.limits));
        if next.redeem.as_ref().is_some_and(|r| r.1.get("sourceId").is_some()) && fresher_than(self.hubs.get(&key)) {
            self.hubs.insert(key.clone(), next.clone());
        }
        if next.limits["resetCredits"].is_object() && fresher_than(self.credits.get(&key)) {
            self.credits.insert(key.clone(), next.clone());
        }
        if next.plan.is_some() && fresher_than(self.plans.get(&key)) {
            self.plans.insert(key.clone(), next.clone());
        }
        let Some(at) = self.accounts.iter().position(|(k, _)| *k == key) else {
            self.accounts.push((key, next));
            return;
        };
        let previous = &self.accounts[at].1;
        let mut environments = previous.environments.clone();
        environments.extend(next.environments.iter().filter(|e| !previous.environments.iter().any(|seen| seen.0 == e.0)).cloned());
        let winner = if newer(&next.limits, &previous.limits) { &next } else { previous };
        let credit = self.credits.get(&key);
        // A hub e-mail names the account better than the driver's default instance name.
        let hub_email = [previous, &next].iter().any(|a| a.environments.is_empty() && a.email.is_some());
        let default_name = driver_label(&previous.driver);
        let display_name = previous
            .display_name
            .clone()
            .or_else(|| next.display_name.clone())
            .filter(|n| !(hub_email && default_name.is_some_and(|d| d.eq_ignore_ascii_case(n))));
        let redeem = match (self.hubs.get(&key), credit) {
            (Some(hub), _) => hub.redeem.clone(),
            (None, Some(c)) => c.redeem.clone(),
            (None, None) => winner.redeem.clone().or_else(|| previous.redeem.clone()).or_else(|| next.redeem.clone()),
        };
        let mut limits = winner.limits.clone();
        match credit.map(|c| &c.limits["resetCredits"]).filter(|c| c.is_object()) {
            Some(credits) => limits["resetCredits"] = credits.clone(),
            None => {
                if let Some(o) = limits.as_object_mut() {
                    o.remove("resetCredits");
                }
            }
        }
        let merged = Account {
            display_name,
            plan: self.plans.get(&key).and_then(|p| p.plan.clone()).or_else(|| previous.plan.clone()).or_else(|| next.plan.clone()),
            accent: previous.accent.clone().or_else(|| next.accent.clone()),
            source_label: if environments.is_empty() { previous.source_label.clone().or_else(|| next.source_label.clone()) } else { None },
            environments,
            redeem,
            limits,
            ..previous.clone()
        };
        self.accounts[at].1 = merged;
    }
}

/// collectLimitAccounts over the selected environments' providers and limit sources.
fn collect_accounts(envs: &[Env]) -> Vec<Account> {
    let keys = Keys::new(envs.iter().flat_map(|e| {
        let sources = list(&e.config["usageLimitSources"]);
        list(&e.config["providers"]).iter().filter(|p| p["enabled"] == true).map(Report::native).chain(sources.iter().flat_map(|src| list(&src["accounts"])).map(Report::hub))
    }));
    let mut merged = Merger::default();
    for e in envs {
        let id = if e.key == crate::env::LOCAL { "local" } else { e.key.as_str() };
        for p in list(&e.config["providers"]).iter().filter(|p| reports_limits(p)) {
            if limits_notice(&p["usageLimits"]).is_some() {
                continue;
            }
            let instance = field(p, "instanceId");
            let own = format!("{id}:{instance}");
            let key = keys.resolve(&Report::native(p)).unwrap_or_else(|| own.clone());
            merged.merge(
                key,
                Account {
                    key: own,
                    driver: field(p, "driver").to_string(),
                    display_name: p["displayName"].as_str().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string),
                    email: text_of(&p["auth"]["email"]),
                    plan: text_of(&p["auth"]["label"]),
                    accent: text_of(&p["accentColor"]),
                    environments: vec![(e.key.clone(), e.label.clone())],
                    source_label: None,
                    redeem: Some((e.key.clone(), json!({"instanceId": instance}))),
                    limits: p["usageLimits"].clone(),
                },
            );
        }
    }
    let labelled = envs.len() > 1;
    for e in envs {
        for src in list(&e.config["usageLimitSources"]) {
            let source_label = if labelled { format!("{} · {}", e.label, field(src, "label")) } else { field(src, "label").to_string() };
            for a in list(&src["accounts"]) {
                let limits = &a["usageLimits"];
                if limits_notice(limits).is_some() {
                    continue;
                }
                let id = field(a, "id");
                let key = match a["workspaceId"].as_str().filter(|w| !w.is_empty()) {
                    Some(workspace) => format!("{}:{id}:{workspace}", field(src, "id")),
                    None => format!("{}:{id}", field(src, "id")),
                };
                let email = text_of(&a["email"]);
                let redeem = limits["resetCredits"]["nextCreditId"]
                    .as_str()
                    .filter(|c| !c.is_empty())
                    .map(|credit| (e.key.clone(), json!({"sourceId": src["id"], "accountId": id, "creditId": credit})));
                let resolved = keys.resolve(&Report::hub(a)).unwrap_or_else(|| key.clone());
                merged.merge(
                    resolved,
                    Account {
                        key,
                        driver: field(a, "driver").to_string(),
                        display_name: email.is_none().then(|| hub_name(id)),
                        email,
                        plan: text_of(&a["plan"]),
                        accent: None,
                        environments: Vec::new(),
                        source_label: Some(source_label.clone()),
                        redeem,
                        limits: limits.clone(),
                    },
                );
            }
        }
    }
    merged.accounts.into_iter().map(|(_, a)| a).collect()
}

/// collectLimitNotices: limits a provider or source could not read, named by environment when
/// more than one is selected.
fn collect_notices(envs: &[Env]) -> Vec<String> {
    let named = |env: &Env, subject: &str| if envs.len() > 1 { format!("{} · {subject}", env.label) } else { subject.to_string() };
    let mut out = Vec::new();
    for e in envs {
        for p in list(&e.config["providers"]).iter().filter(|p| reports_limits(p)) {
            let limits = &p["usageLimits"];
            if limits["unavailable"]["reason"] == "unsupported" {
                continue;
            }
            if let Some(notice) = limits_notice(limits) {
                let name = p["displayName"].as_str().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(field(p, "driver"));
                out.push(format!("{}: {notice}", named(e, name)));
            }
        }
        for src in list(&e.config["usageLimitSources"]) {
            let label = named(e, field(src, "label"));
            if let Some(error) = src["error"].as_str() {
                out.push(format!("{label}: {error}"));
            } else if list(&src["accounts"]).is_empty() {
                out.push(format!("{label}: No accounts reported."));
            }
        }
    }
    out
}

struct Link {
    label: String,
    url: String,
    message: Option<String>,
    accounts: Vec<String>,
}

/// collectExternalUsageLinks: one card per usage page, naming every login it covers.
fn collect_links(envs: &[Env]) -> Vec<Link> {
    let mut links: Vec<Link> = Vec::new();
    for e in envs {
        for p in list(&e.config["providers"]).iter().filter(|p| reports_limits(p)) {
            let external = &p["usageLimits"]["externalUsage"];
            if !external.is_object() || p["auth"]["status"] != "authenticated" {
                continue;
            }
            let account = format!("{} on {}", p["displayName"].as_str().unwrap_or(field(p, "instanceId")), e.label);
            let (label, url) = (field(external, "label").to_string(), field(external, "url").to_string());
            let message = text_of(&p["usageLimits"]["unavailable"]["message"]);
            match links.iter_mut().find(|l| l.url == url) {
                Some(link) => {
                    link.label = label;
                    link.message = message;
                    if !link.accounts.contains(&account) {
                        link.accounts.push(account);
                    }
                }
                None => links.push(Link { label, url, message, accounts: vec![account] }),
            }
        }
    }
    links
}

/// UsageLimitsSection: hidden providers drop out, and a limit source whose accounts are all
/// hidden drops with them.
fn presentation(config: &Value, hidden: &[&str]) -> Value {
    let visible = |driver: &str| !hidden.contains(&super::usage::kind_of_driver(driver));
    let providers: Vec<Value> = list(&config["providers"]).iter().filter(|p| visible(field(p, "driver"))).cloned().collect();
    let sources: Vec<Value> = list(&config["usageLimitSources"])
        .iter()
        .filter_map(|src| {
            let all = list(&src["accounts"]);
            let accounts: Vec<Value> = all.iter().filter(|a| visible(field(a, "driver"))).cloned().collect();
            if accounts.is_empty() && !all.is_empty() {
                return None;
            }
            let mut src = src.clone();
            src["accounts"] = Value::Array(accounts);
            Some(src)
        })
        .collect();
    json!({"providers": providers, "usageLimitSources": sources})
}

// ---- pools ------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pace {
    Ahead,
    On,
    Under,
}

impl Pace {
    fn of(used: f64, elapsed: f64) -> Self {
        let gap = used - elapsed * 100.0;
        if gap > 5.0 {
            Pace::Ahead
        } else if gap < -5.0 {
            Pace::Under
        } else {
            Pace::On
        }
    }

    fn icon(self) -> &'static rune::Icon {
        match self {
            Pace::Ahead => &i::TRENDING_UP,
            Pace::On => &i::GAUGE,
            Pace::Under => &i::TRENDING_DOWN,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Pace::Ahead => "Ahead of pace: spending faster than the window elapses",
            Pace::On => "On pace with the window",
            Pace::Under => "Under pace: headroom left for the rest of the window",
        }
    }
}

struct Member {
    /// The account's column in its pool.
    account: usize,
    window: Value,
    refill: f64,
}

struct Reset {
    member: usize,
    at: i64,
    restores: i64,
}

struct PoolWindow {
    id: String,
    kind: String,
    label: String,
    members: Vec<Member>,
    /// Each account column's member, if the account reports this window.
    columns: Vec<Option<usize>>,
    remaining: i64,
    pace: Option<Pace>,
    resets: Vec<Reset>,
}

struct Pool {
    driver: String,
    accounts: Vec<Account>,
    windows: Vec<PoolWindow>,
}

fn kind_rank(kind: &str) -> u8 {
    match kind {
        "session" => 0,
        "weekly" => 1,
        "monthly" => 2,
        _ => 3,
    }
}

/// Codex ids name slots, so pool by kind; other drivers' ids identify distinct allowances.
fn pool_key(driver: &str, w: &Value) -> String {
    if driver == "codex" { field(w, "kind").to_string() } else { format!("{}:{}", field(w, "kind"), field(w, "id")) }
}

fn reset_millis(w: &Value) -> Option<i64> {
    w["resetsAt"].as_str().and_then(iso_ms)
}

/// poolMember: a session blocked by a spent weekly window counts as spent, refilling only when
/// the blocking window clears.
fn pool_member(a: &Account, reported: &Value, now: i64) -> (Value, f64) {
    let blocks = |w: &&Value| used(w) >= 100.0 && if a.driver == "claudeAgent" { w["id"] == "seven_day" } else { a.driver == "codex" && w["kind"] == "weekly" };
    let spent = if reported["kind"] == "session" { windows(&a.limits).iter().find(blocks) } else { None };
    let Some(spent) = spent else { return (reported.clone(), used(reported)) };
    let cutoff = checked(&a.limits).map(|c| c.max(now));
    let mut resets_at = None;
    let mut refill = 0.0;
    // A stale or missing clock cannot tell us when blocked quota becomes usable.
    if let (Some(weekly), Some(session), Some(cutoff)) = (reset_millis(spent), reset_millis(reported), cutoff)
        && weekly > cutoff
        && session > cutoff
    {
        (resets_at, refill) = if session <= weekly {
            (spent.get("resetsAt"), 100.0)
        } else if used(reported) >= 100.0 {
            (reported.get("resetsAt"), 100.0)
        } else {
            (spent.get("resetsAt"), 100.0 - used(reported).clamp(0.0, 100.0))
        };
    }
    let mut window = reported.clone();
    window["usedPercent"] = json!(100);
    if let Some(o) = window.as_object_mut() {
        o.remove("resetsAt");
        o.remove("windowDurationMins");
    }
    if let Some(at) = resets_at {
        window["resetsAt"] = at.clone();
    }
    (window, refill)
}

/// collectLimitPools: accounts grouped by driver, ordered by when their first window resets.
fn collect_pools(accounts: Vec<Account>, now: i64) -> Vec<Pool> {
    let mut by_driver: Vec<(String, Vec<Account>)> = Vec::new();
    for a in accounts {
        match by_driver.iter_mut().find(|(d, _)| *d == a.driver) {
            Some((_, members)) => members.push(a),
            None => by_driver.push((a.driver.clone(), vec![a])),
        }
    }
    by_driver
        .into_iter()
        .map(|(driver, mut members)| {
            let mut first: Option<&Value> = None;
            for w in members.iter().flat_map(|a| windows(&a.limits)) {
                if first.is_none_or(|f| kind_rank(field(w, "kind")) < kind_rank(field(f, "kind"))) {
                    first = Some(w);
                }
            }
            let order = first.map(|w| pool_key(&driver, w));
            let reset_of = |a: &Account| {
                order
                    .as_ref()
                    .and_then(|key| windows(&a.limits).iter().find(|w| pool_key(&driver, w) == *key))
                    .and_then(|w| reset_millis(&pool_member(a, w, now).0))
                    .unwrap_or(i64::MAX)
            };
            let name = |a: &Account| a.display_name.clone().or_else(|| a.email.clone()).unwrap_or_else(|| a.key.clone()).to_lowercase();
            members.sort_by(|a, b| reset_of(a).cmp(&reset_of(b)).then_with(|| name(a).cmp(&name(b))).then_with(|| a.key.cmp(&b.key)));
            let pooled = pool_windows(&members, now);
            Pool { driver, accounts: members, windows: pooled }
        })
        .collect()
}

fn pool_windows(accounts: &[Account], now: i64) -> Vec<PoolWindow> {
    let mut by_key: Vec<(String, Vec<Member>)> = Vec::new();
    for (n, a) in accounts.iter().enumerate() {
        for reported in windows(&a.limits) {
            let (window, refill) = pool_member(a, reported, now);
            let pooled = pool_key(&a.driver, &window);
            let taken = by_key.iter().any(|(k, members)| *k == pooled && members.iter().any(|m| m.account == n));
            let key = if taken { format!("{}:{}", field(&window, "kind"), field(&window, "id")) } else { pooled };
            let member = Member { account: n, window, refill };
            match by_key.iter_mut().find(|(k, _)| *k == key) {
                Some((_, members)) => members.push(member),
                None => by_key.push((key, vec![member])),
            }
        }
    }
    let mut pools: Vec<PoolWindow> = by_key
        .into_iter()
        .map(|(key, members)| {
            let first = &members[0].window;
            let id = if accounts[members[0].account].driver == "codex" { key } else { field(first, "id").to_string() };
            let n = members.len() as f64;
            let mean = members.iter().map(|m| used(&m.window)).sum::<f64>() / n;
            // Judge pace only for windows with reset clocks; others add spend without elapsed time.
            let timed: Vec<(f64, f64)> = members.iter().filter_map(|m| Some((used(&m.window), elapsed_share(&m.window, now)?))).collect();
            let pace = (!timed.is_empty()).then(|| {
                let t = timed.len() as f64;
                Pace::of(timed.iter().map(|x| x.0).sum::<f64>() / t, timed.iter().map(|x| x.1).sum::<f64>() / t)
            });
            let mut resets: Vec<Reset> = members
                .iter()
                .enumerate()
                .filter_map(|(k, m)| Some(Reset { member: k, at: reset_millis(&m.window)?, restores: (m.refill / n).round() as i64 }))
                .collect();
            resets.sort_by_key(|r| r.at);
            let columns = (0..accounts.len()).map(|a| members.iter().rposition(|m| m.account == a)).collect();
            PoolWindow {
                id,
                kind: field(first, "kind").to_string(),
                label: field(first, "label").to_string(),
                remaining: (100.0 - mean).round() as i64,
                pace,
                resets,
                columns,
                members,
            }
        })
        .collect();
    pools.sort_by_key(|p| kind_rank(&p.kind));
    pools
}

/// displayLimitWindows: Antigravity's Gemini pools first by model, Cursor's split pools in place
/// of their total.
fn display_windows(pool: &Pool) -> Vec<&PoolWindow> {
    let mut shown: Vec<&PoolWindow> = pool.windows.iter().collect();
    match pool.driver.as_str() {
        "antigravity" => {
            let base = |id: &str| ["-5h", "-weekly", "-quota"].iter().find_map(|suffix| id.strip_suffix(suffix)).unwrap_or(id).to_string();
            shown.sort_by(|a, b| {
                (!a.id.starts_with("gemini-"))
                    .cmp(&!b.id.starts_with("gemini-"))
                    .then_with(|| base(&a.id).cmp(&base(&b.id)))
                    .then_with(|| kind_rank(&a.kind).cmp(&kind_rank(&b.kind)))
            });
        }
        "cursor" => {
            let both = ["autoPercentUsed", "apiPercentUsed"].iter().all(|id| shown.iter().any(|w| w.id == *id));
            shown.retain(|w| !both || w.id != "totalPercentUsed");
            shown.sort_by_key(|w| cursor_rank(&w.id));
        }
        _ => {}
    }
    shown
}

fn remaining_percent(w: &Value) -> i64 {
    (100.0 - used(w).clamp(0.0, 100.0)).round() as i64
}

fn elapsed_share(w: &Value, now: i64) -> Option<f64> {
    let at = reset_millis(w)?;
    let len = w["windowDurationMins"].as_f64()? * MINUTE as f64;
    (len > 0.0).then(|| ((len - (at - now) as f64) / len).clamp(0.0, 1.0))
}

/// paceRemainingPercent: bars show quota remaining, so even pace tracks time remaining.
fn pace_remaining(w: &Value, now: i64) -> Option<i64> {
    elapsed_share(w, now).map(|e| ((1.0 - e) * 100.0).round() as i64)
}

/// formatDuration.
fn duration(ms: i64) -> String {
    let ms = ms.max(0);
    let (d, h, m) = (ms / DAY, ms % DAY / 3_600_000, ms % 3_600_000 / MINUTE);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

/// formatResetsIn.
fn resets_in(w: &Value, now: i64) -> Option<String> {
    let at = reset_millis(w)?;
    Some(if at <= now {
        "resets now".into()
    } else if at - now < MINUTE {
        "Resets momentarily".into()
    } else {
        format!("resets in {}", duration(at - now))
    })
}

fn credit_count(a: &Account) -> i64 {
    a.limits["resetCredits"]["availableCount"].as_i64().unwrap_or(0)
}

/// resetCreditsSummary.
fn credits_summary(credits: &Value, now: i64, compact: bool) -> String {
    let count = credits["availableCount"].as_i64().unwrap_or(0);
    let expires = credits["nextExpiresAt"].as_str().and_then(iso_ms).map(|at| duration(at - now));
    if count == 0 {
        return "No reset credits banked".into();
    }
    if compact {
        return format!("{count} banked{}", expires.map(|e| format!(" · expires in {e}")).unwrap_or_default());
    }
    format!("{count} {} banked{}", if count == 1 { "reset credit" } else { "reset credits" }, expires.map(|e| format!(" · next expires in {e}")).unwrap_or_default())
}

/// formatUpcomingTimestamp: "2:16 PM", "tomorrow at 2:16 PM" or "11/12 2:16 PM".
fn upcoming(at: i64, now: i64, zone: Zone, hour12: bool) -> String {
    let wall = zone.wall(at);
    let mins = wall.rem_euclid(DAY) / MINUTE;
    let (h, m) = (mins / 60, mins % 60);
    let time = if hour12 { format!("{}:{m:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" }) } else { format!("{h:02}:{m:02}") };
    let (day, today) = (wall.div_euclid(DAY), zone.wall(now).div_euclid(DAY));
    let date = || {
        let ((y, mo, d), (this_year, _, _)) = (super::usage::civil(day), super::usage::civil(today));
        if y == this_year { format!("{mo}/{d} {time}") } else { format!("{mo}/{d}/{y} {time}") }
    };
    match day - today {
        0 => time.clone(),
        1 => format!("tomorrow at {time}"),
        -1 => format!("yesterday at {time}"),
        _ => date(),
    }
}

/// redactedPlaceholder: a stable scramble keeping the address's shape.
fn redacted(value: &str) -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut state = 0x811c9dc5u32 as i32;
    for unit in value.encode_utf16() {
        state ^= i32::from(unit);
        state = state.wrapping_mul(0x01000193);
    }
    value
        .chars()
        .map(|c| {
            if matches!(c, '@' | '.' | '-' | '_') {
                return c;
            }
            state = (state ^ ((state as u32) >> 13) as i32).wrapping_mul(0x85ebca6bu32 as i32);
            state = (state ^ ((state as u32) >> 16) as i32).wrapping_mul(0xc2b2ae35u32 as i32);
            ALPHABET[(i64::from(state).abs() % ALPHABET.len() as i64) as usize] as char
        })
        .collect()
}

fn initials(email: &str) -> String {
    let (local, domain) = email.split_once('@').unwrap_or((email, ""));
    let out: String = local.chars().take(1).chain(domain.chars().take(1)).collect::<String>().to_uppercase();
    if out.is_empty() { "?".into() } else { out }
}

fn hue(email: &str) -> f32 {
    let mut hash: i32 = 0;
    for unit in email.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    (i64::from(hash).abs() % 360) as f32
}

/// An `oklch()` colour in the linear sRGB the theme's colours use.
fn oklch(l: f32, c: f32, h: f32) -> Color {
    let (a, b) = (c * h.to_radians().cos(), c * h.to_radians().sin());
    let l_ = (l + 0.396_337_8 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_35 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    Color {
        r: (4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_).clamp(0.0, 1.0),
        g: (-1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_4 * s_).clamp(0.0, 1.0),
        b: (-0.004_196_086 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_).clamp(0.0, 1.0),
        a: 1.0,
    }
}

/// normalizeProviderAccentColor: `#rrggbb` or nothing.
fn accent(a: &Account) -> Option<Color> {
    let digits = a.accent.as_deref()?.trim().strip_prefix('#')?;
    (digits.len() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit())).then(|| rune::color::hex(&format!("#{digits}"))).flatten()
}

// ---- view -------------------------------------------------------------------------------

/// The selected environments with their visible providers, in the app's order. Every connected
/// environment keeps its own config, so parked ones count as on the web.
fn presentations(s: &App) -> Vec<Env> {
    let hidden = &s.ui.pages.usage.hidden;
    super::usage::selected_environments(s)
        .into_iter()
        .map(|key| {
            let config = crate::env::source(s, &key).and_then(|src| src.state().config.as_ref()).map_or(Value::Null, |c| presentation(c, hidden));
            Env { label: crate::env::label(s, &key), key, config }
        })
        .collect()
}

/// UsageLimitsPooled for the selected environments' visible providers, with the Cursor keychain
/// prompt after the Codex and Claude pools.
pub fn view(s: &App, cx: &mut rune::Cx<App>, t: &T, overlays: &mut Vec<El>) -> El {
    let now = s.now_ms;
    let envs = presentations(s);
    let pools = collect_pools(collect_accounts(&envs), now);
    let notices = collect_notices(&envs);
    let links = collect_links(&envs);
    let mut cursor = super::usage::cursor_access(s).map(|label| cursor_prompt(s, t, &label));
    let mut out = col().w_full().gap(32.0);
    if pools.is_empty() && notices.is_empty() && cursor.is_none() && links.is_empty() {
        out = out.child(super::usage::muted_line(t, "No provider on the selected environments reports subscription limits."));
    }
    let cursor_at = ["codex", "claudeAgent"].iter().map(|d| pools.iter().position(|p| p.driver == *d).map_or(0, |at| at + 1)).max().unwrap_or(0);
    for (n, pool) in pools.iter().enumerate() {
        if n == cursor_at
            && let Some(prompt) = cursor.take()
        {
            out = out.child(prompt);
        }
        out = out.child(pool_section(s, cx, t, overlays, pool, now));
    }
    if let Some(prompt) = cursor {
        out = out.child(prompt);
    }
    for link in links {
        out = out.child(link_card(t, link));
    }
    if !notices.is_empty() {
        let mut lines = col().gap(2.0).grow(1.0).shrink();
        for n in notices {
            lines = lines.child(text(n).font(theme::SM_MEDIUM).color(t.c.warning_foreground).wrap());
        }
        out = out.child(
            row()
                .w_full()
                .gap(8.0)
                .radius(radius::XL)
                .border(1.0, t.c.warning.alpha_mul(0.32))
                .fill(t.c.warning_surface)
                .px(14.0)
                .py(12.0)
                .child(icon(&i::TRIANGLE_ALERT, 16.0).color(t.c.warning).mt(2.0).self_start())
                .child(lines),
        );
    }
    confirm_dialog(s, t, overlays);
    out
}

/// CursorEnableLimits for the one environment whose scan asks for the keychain.
fn cursor_prompt(s: &App, t: &T, label: &str) -> El {
    col()
        .w_full()
        .gap(12.0)
        .child(row().gap(8.0).center_y().child(el().size(20.0, 20.0).center().child(provider_icon(t, "cursor", 16.0))).child(text("Cursor").font(theme::SM_MEDIUM).color(t.c.foreground)))
        .child(
            col()
                .w_full()
                .gap(12.0)
                .radius(radius::LG)
                .border(1.0, t.c.border.alpha_mul(0.6))
                .padding(16.0)
                .child(text(super::usage::CURSOR_KEYCHAIN_COPY).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap())
                .child(row().child(
                    kit::Button::new("Enable")
                        .variant("outline")
                        .size("sm")
                        .disabled(s.ui.pages.usage.cursor_enabling.is_some())
                        .on_press(super::usage::enable_cursor)
                        .build(t)
                        .label(format!("Enable Cursor usage from {label}")),
                )),
        )
}

fn link_card(t: &T, link: Link) -> El {
    let chatgpt = link.url == "https://chatgpt.com/#settings/Usage";
    let mut copy = col()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(4.0)
        .child(text(link.label).font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(text(link.accounts.join(", ")).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap());
    let note = if chatgpt { Some("View usage in ChatGPT with your connected account.".to_string()) } else { link.message };
    if let Some(note) = note {
        copy = copy.child(text(note).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().max_w(576.0));
    }
    let mut lead = row().grow(1.0).shrink().min_w(0.0).gap(12.0).center_y();
    if chatgpt {
        lead = lead.child(provider_icon(t, "codex", 20.0));
    }
    let url = link.url;
    row()
        .w_full()
        .gap(12.0)
        .radius(radius::XL)
        .border(1.0, t.c.border)
        .padding(16.0)
        .center_y()
        .child(lead.child(copy))
        .child(kit::Button::new("Manage usage").variant("ghost-muted").size("xs").icon(&i::EXTERNAL_LINK).build(t).on_press_tx(move |_: &mut App, tx: &mut rune::Tx| {
            let _ = tx.open_url(&url);
        }))
}

fn pool_section(s: &App, cx: &mut rune::Cx<App>, t: &T, overlays: &mut Vec<El>, pool: &Pool, now: i64) -> El {
    let label = driver_label(&pool.driver).unwrap_or(pool.driver.as_str()).to_string();
    let mut section = col().w_full().gap(12.0).child(
        row().gap(8.0).center_y().child(el().size(20.0, 20.0).center().child(provider_icon(t, &pool.driver, 16.0))).child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground)),
    );
    for w in display_windows(pool) {
        let details = if pool.driver == "cursor" { CURSOR_WINDOWS.iter().find(|c| c.0 == w.id) } else { None };
        section = section.child(window_card(s, cx, t, overlays, pool, w, details, now));
    }
    section
}

#[allow(clippy::too_many_arguments)]
fn window_card(s: &App, cx: &mut rune::Cx<App>, t: &T, overlays: &mut Vec<El>, pool: &Pool, w: &PoolWindow, details: Option<&(&str, &str, &str)>, now: i64) -> El {
    let label = details.map_or_else(|| w.label.clone(), |d| d.1.to_string());
    let mut figure = row()
        .gap(8.0)
        .center_y()
        .child(text(format!("{}%", w.remaining)).font(theme::weight(theme::TEXT_3XL, theme::SEMIBOLD).features("'tnum' 1")).color(t.c.foreground))
        .child(text("left").font(theme::TEXT_SM).color(t.c.muted_foreground).mt(8.0));
    if let Some(pace) = w.pace {
        figure = figure.child(icon(pace.icon(), 14.0).color(t.c.muted_foreground).mt(8.0).role("img").label(pace.label()).help(pace.label()));
    }
    let mut left = col().w(176.0).shrink().gap(4.0).child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground)).child(figure);
    if let Some(next) = w.resets.iter().find(|r| r.restores > 0).filter(|_| w.columns.len() > 1) {
        left = left.child(text(format!("↻ +{}%", next.restores)).font(theme::XS_MEDIUM.features("'tnum' 1")).color(t.c.foreground));
    }
    let mut card = col()
        .w_full()
        .gap(12.0)
        .radius(radius::LG)
        .border(1.0, t.c.border.alpha_mul(0.6))
        .padding(16.0)
        .child(row().w_full().gap(24.0).center_y().child(left).child(pool_bar(s, cx, t, overlays, pool, w, now)));
    if let Some(d) = details {
        card = card.child(text(d.2).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap());
    }
    card
}

/// The popover, legend and status key of one account's segment in one window.
pub(super) fn segment_key(driver: &str, kind: &str, id: &str, account: &str) -> String {
    format!("usage-seg-{driver}-{kind}:{id}-{account}")
}

struct Seg<'a> {
    key: String,
    account: &'a Account,
    member: &'a Member,
    reset: Option<&'a Reset>,
    color: Color,
    index: usize,
    show_name: bool,
    wide: bool,
}

/// PoolBar: one column per account; below `@2xl/pool` segments show their number and a legend
/// row names each.
fn pool_bar(s: &App, cx: &mut rune::Cx<App>, t: &T, overlays: &mut Vec<El>, pool: &Pool, w: &PoolWindow, now: i64) -> El {
    let bar_key = format!("usage-pool-{}-{}:{}", pool.driver, w.kind, w.id);
    let wide = cx.layout_with(bar_key.as_str(), |r| r[2]).unwrap_or(744.0) >= 672.0;
    let fallback = super::usage::color(t, &pool.driver);
    let u = &s.ui.pages.usage.limits;
    let mut segments = row().w_full().gap(4.0);
    let mut legend = col().w_full().gap(4.0);
    let mut statuses = Vec::new();
    for (n, column) in w.columns.iter().enumerate() {
        let Some(member) = column.map(|k| &w.members[k]) else {
            segments = segments.child(el().grow(1.0).w(0.0));
            continue;
        };
        let account = &pool.accounts[member.account];
        let seg = Seg {
            key: segment_key(&pool.driver, &w.kind, &w.id, &account.key),
            account,
            member,
            reset: w.resets.iter().find(|r| w.members[r.member].account == member.account),
            color: accent(account).unwrap_or(fallback),
            index: n + 1,
            show_name: w.columns.len() > 1,
            wide,
        };
        segments = segments.child(segment(s, cx, t, overlays, &seg, now));
        if !wide {
            legend = legend.child(legend_row(t, &seg, now));
        }
        // The popover closed before the confirm, so progress and the outcome need a home outside it.
        let status = if u.redeeming.as_ref().is_some_and(|r| r.1 == seg.key) { Some("Using reset credit…".to_string()) } else { u.status.get(&seg.key).cloned() };
        if let Some(status) = status {
            statuses.push(row().w_full().gap(4.0).center_y().child(account_name(t, account, theme::XS_MEDIUM)).child(text(status).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().shrink()));
        }
    }
    let mut bar = col().key(bar_key).grow(1.0).shrink().min_w(0.0).gap(4.0).child(segments);
    if !wide {
        bar = bar.child(legend);
    }
    bar.children(statuses)
}

/// AccountName: the instance's name, else an initials chip for the e-mail, else the driver.
fn account_name(t: &T, a: &Account, font: rune::tok::TextStyle) -> El {
    match (&a.display_name, &a.email) {
        (Some(name), _) => text(name.clone()).font(font).color(t.c.foreground).truncate().shrink(),
        (None, Some(email)) => chip(email),
        (None, None) => text(driver_label(&a.driver).unwrap_or(a.driver.as_str()).to_string()).font(font).color(t.c.foreground).truncate().shrink(),
    }
}

/// AccountChip.
fn chip(email: &str) -> El {
    let hue = hue(email);
    row()
        .size(16.0, 16.0)
        .shrink()
        .center()
        .radius(radius::FULL)
        .fill(oklch(0.85, 0.08, hue))
        .role("img")
        .label(format!("Account {}", initials(email)))
        .child(text(initials(email)).font(theme::weight(theme::TEXT_3XS, theme::SEMIBOLD).lh(10.0)).color(oklch(0.35, 0.1, hue)))
}

fn hover_segment(s: &mut App, key: &str, inside: bool) {
    let u = ui(s);
    if inside {
        u.segment = Some(key.to_string());
        u.over_popup = false;
    } else if !u.over_popup && u.segment.as_deref() == Some(key) {
        u.segment = None;
    }
}

fn hover_popup(s: &mut App, inside: bool) {
    let u = ui(s);
    u.over_popup = inside;
    if !inside {
        u.segment = None;
    }
}

fn segment(s: &App, cx: &mut rune::Cx<App>, t: &T, overlays: &mut Vec<El>, seg: &Seg, now: i64) -> El {
    let w = &seg.member.window;
    let width = cx.layout_with(seg.key.as_str(), |r| r[2]).unwrap_or(744.0);
    let h = if seg.wide { 32.0 } else { 20.0 };
    let remaining = remaining_percent(w);
    // Below 6px there is no room for the gap and tick.
    let time_left = pace_remaining(w, now).filter(|_| width >= 6.0);
    let resets = resets_in(w, now);
    let credits = credit_count(seg.account);
    let mut bar = el().key(seg.key.clone()).grow(1.0).w(0.0).min_w(0.0).h(h).radius(radius::MD).fill(t.c.muted).clip_rounded(radius::MD);
    bar = bar.child(el().absolute(0.0, 0.0).w(width * remaining as f32 / 100.0).h(h).radius(radius::MD).fill(seg.color.alpha_mul(0.35)));
    if remaining < 100 && seg.reset.is_some() && seg.member.refill > 0.0 {
        // What the next reset refills, hatched in the bar's colour from the right.
        let hatch_w = width * ((100 - remaining) as f32).min(seg.member.refill as f32) / 100.0;
        let mut shape = Shape::new();
        let mut x = -h;
        while x < hatch_w {
            shape = shape.line([x, h], [x + h, 0.0], 1.0, LineCap::Butt, seg.color.alpha_mul(0.2));
            x += 5.0 * std::f32::consts::SQRT_2;
        }
        bar = bar.child(canvas(shape.view_box(hatch_w.max(1.0), h)).absolute(width - hatch_w, 0.0).w(hatch_w).h(h));
    }
    if let Some(left) = time_left {
        let x = (width * left as f32 / 100.0).clamp(3.0, width - 3.0);
        bar = bar
            .child(el().absolute(x - 3.0, 0.0).w(6.0).h(h).fill(t.c.background))
            .child(el().absolute(x - 1.0, 2.0).w(2.0).h(h - 4.0).radius(radius::FULL).fill(t.c.foreground));
    }
    let content = if seg.wide {
        let mut inner = row().absolute(0.0, 0.0).w(width).h(h).px(8.0).gap(6.0).center_y();
        // A single-account bar repeats the headline percentage.
        if seg.show_name {
            inner = inner.child(account_name(t, seg.account, theme::XS_MEDIUM)).child(text(format!("{remaining}%")).font(theme::weight(theme::TEXT_XS, theme::SEMIBOLD).features("'tnum' 1")).color(t.c.foreground));
        }
        if resets.is_some() || credits > 0 {
            inner = inner.child(spacer()).child(reset_pill(t, resets.as_deref(), credits, true));
        }
        inner
    } else {
        row().absolute(0.0, 0.0).w(width).h(h).center().child(text(seg.index.to_string()).font(theme::weight(theme::TEXT_3XS, theme::SEMIBOLD).features("'tnum' 1")).color(t.c.foreground.alpha_mul(0.8)))
    };
    let name = seg.account.display_name.clone().or_else(|| seg.account.email.as_deref().map(initials)).unwrap_or_else(|| seg.account.driver.clone());
    let mut label = format!("{name}: {remaining}% left");
    if let Some(r) = &resets {
        label.push_str(&format!(", {r}"));
    }
    if let Some(left) = pace_remaining(w, now) {
        label.push_str(&format!(", {left}% of the window left"));
    }
    if credits > 0 {
        label.push_str(&format!(", {credits} reset {} banked", if credits == 1 { "credit" } else { "credits" }));
    }
    let (hover_key, press_key) = (seg.key.clone(), seg.key.clone());
    let open = s.ui.pages.usage.limits.segment.as_deref() == Some(seg.key.as_str());
    let trigger = bar
        .child(content)
        .role("button")
        .label(label)
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .hover_delays(Duration::from_millis(100), Duration::from_millis(150))
        .on_hover_change(move |s: &mut App, inside, _| hover_segment(s, &hover_key, inside))
        .on_press(move |s: &mut App| ui(s).segment = Some(press_key.clone()));
    let body = popover_body(s, t, seg, now).on_hover_change(|s: &mut App, inside, _| hover_popup(s, inside));
    kit::Popover::new(format!("{}-popover", seg.key))
        .trigger(trigger)
        .open(open)
        .placement(Placement::Top)
        .gutter(6.0)
        .padding(12.0)
        .child(body)
        .build(&kit::theme(t), overlays)
        .grow(1.0)
        .w(0.0)
        .min_w(0.0)
}

/// The "↻ 2h 5m · 🎟 2" chip on a segment or legend row.
fn reset_pill(t: &T, resets: Option<&str>, credits: i64, boxed: bool) -> El {
    let ink = if boxed { t.c.foreground } else { t.c.muted_foreground };
    let mut pill = row().gap(6.0).center_y();
    if boxed {
        pill = pill.px(6.0).py(2.0).radius(radius::SM).fill(t.c.background.alpha_mul(0.85));
    }
    if let Some(r) = resets {
        pill = pill.child(text(r.replace("resets in ", "↻ ")).font(theme::TEXT_2XS.features("'tnum' 1")).color(ink));
    }
    if credits > 0 {
        if resets.is_some() {
            pill = pill.child(text("·").font(theme::TEXT_2XS).color(t.c.muted_foreground));
        }
        pill = pill.child(
            row()
                .gap(2.0)
                .center_y()
                .child(icon(&i::TICKET, 12.0).color(t.c.foreground))
                .child(text(credits.to_string()).font(theme::weight(theme::TEXT_2XS, theme::SEMIBOLD).features("'tnum' 1")).color(t.c.foreground)),
        );
    }
    pill
}

/// LegendRow: the narrow bar's key, opening the same popover as its segment.
fn legend_row(t: &T, seg: &Seg, now: i64) -> El {
    let w = &seg.member.window;
    let swatch = row()
        .size(16.0, 16.0)
        .center()
        .radius(radius::SM)
        .fill(seg.color.alpha_mul(0.35))
        .child(text(seg.index.to_string()).font(theme::weight(theme::TEXT_3XS, theme::SEMIBOLD).features("'tnum' 1")).color(t.c.foreground.alpha_mul(0.8)));
    let key = seg.key.clone();
    let credits = credit_count(seg.account);
    let resets = resets_in(w, now);
    let mut line = row()
        .key(format!("{}-legend", seg.key))
        .w_full()
        .h(28.0)
        .px(6.0)
        .gap(6.0)
        .center_y()
        .radius(radius::MD)
        .hover(t.c.accent)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(format!("Segment {}", seg.index))
        .on_press(move |s: &mut App| ui(s).segment = Some(key.clone()))
        .child(swatch)
        .child(account_name(t, seg.account, theme::XS_MEDIUM))
        .child(text(format!("{}%", remaining_percent(w))).font(theme::weight(theme::TEXT_XS, theme::SEMIBOLD).features("'tnum' 1")).color(t.c.foreground));
    if resets.is_some() || credits > 0 {
        line = line.child(spacer()).child(reset_pill(t, resets.as_deref(), credits, false));
    }
    line
}

fn info_row(t: &T, label: &str, value: String) -> El {
    row()
        .w_full()
        .gap(12.0)
        .child(text(label.to_string()).font(theme::TEXT_XS).color(t.c.muted_foreground).w(72.0).shrink())
        .child(text(value).font(theme::TEXT_XS.features("'tnum' 1")).color(t.c.foreground).wrap().grow(1.0).min_w(0.0))
}

fn divided(t: &T, rows: Vec<El>) -> El {
    col().w_full().gap(4.0).child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.6)).mb(6.0)).children(rows)
}

/// SegmentPopover: who the account is, where it is signed in, and this window's figures.
fn popover_body(s: &App, t: &T, seg: &Seg, now: i64) -> El {
    let a = seg.account;
    let w = &seg.member.window;
    let u = &s.ui.pages.usage;
    let hour12 = crate::desktop::locale::hour12(s.client_settings.values.get("timestampFormat").and_then(Value::as_str));
    let name = a.display_name.clone().unwrap_or_else(|| driver_label(&a.driver).unwrap_or(a.driver.as_str()).to_string());
    let avatar = if a.redeem.is_some() { Some(el().size(20.0, 20.0).center().child(provider_icon(t, &a.driver, 16.0))) } else { a.email.as_deref().map(chip) };
    let mut head = col()
        .w_full()
        .gap(2.0)
        .child(row().w_full().gap(8.0).center_y().children(avatar).child(text(name).font(theme::SM_MEDIUM).color(t.c.foreground).truncate().shrink()));
    if let Some(email) = &a.email {
        let shown = u.limits.email_shown;
        head = head.child(
            text(if shown { email.clone() } else { redacted(email) })
                .font(theme::mono(theme::TEXT_2XS))
                .color(t.c.muted_foreground)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label("Toggle account email visibility")
                .help(if shown { "Click to hide email" } else { "Click to reveal email" })
                .on_press(|s: &mut App| {
                    let u = ui(s);
                    u.email_shown = !u.email_shown;
                }),
        );
    }
    let mut body = col().w(288.0).gap(10.0).child(head);
    let mut who = Vec::new();
    if let Some(plan) = &a.plan {
        who.push(info_row(t, "Plan", plan.clone()));
    }
    let signed_in = !a.environments.is_empty();
    let places = a.environments.iter().map(|e| e.1.as_str()).collect::<Vec<_>>().join(", ");
    if let Some(place) = if signed_in { Some(places) } else { a.source_label.clone() } {
        who.push(info_row(t, if signed_in { "Signed in" } else { "Via" }, place));
    }
    body = body.child(divided(t, who));
    let mut figures = vec![info_row(t, "Left", format!("{}%", remaining_percent(w)))];
    if let Some(at) = reset_millis(w) {
        let suffix = resets_in(w, now).map(|r| format!(" · {}", r.replace("resets in ", "in "))).unwrap_or_default();
        figures.push(info_row(t, "Resets", format!("{}{suffix}", upcoming(at, now, u.zone, hour12))));
    }
    if let Some(left) = pace_remaining(w, now) {
        figures.push(info_row(t, "Even pace", format!("{left}% left by now")));
    }
    if let Some(reset) = seg.reset.filter(|r| r.restores > 0) {
        figures.push(info_row(t, "Restores", format!("+{}% of pool", reset.restores)));
    }
    body = body.child(divided(t, figures));
    if let Some(target) = a.redeem.clone().filter(|_| credit_count(a) > 0) {
        let busy = u.limits.redeeming.as_ref().is_some_and(|r| r.1 == seg.key);
        let key = seg.key.clone();
        let line = row()
            .w_full()
            .gap(12.0)
            .center_y()
            .child(text(credits_summary(&a.limits["resetCredits"], now, true)).font(theme::TEXT_XS.features("'tnum' 1")).color(t.c.muted_foreground))
            .child(spacer())
            .child(
                kit::Button::new(if busy { "Using…" } else { "Use reset" })
                    .size("xs")
                    .variant("outline")
                    .disabled(busy)
                    .on_press(move |s: &mut App| {
                        let u = ui(s);
                        u.segment = None;
                        u.confirm = Some((key.clone(), target.clone()));
                    })
                    .build(t),
            );
        body = body.child(divided(t, vec![line]));
    }
    body
}

/// ResetCreditDialog: redemption spends a credit, so it asks first.
fn confirm_dialog(s: &App, t: &T, overlays: &mut Vec<El>) {
    let Some((key, target)) = s.ui.pages.usage.limits.confirm.clone() else { return };
    let cancel = kit::Button::new("Cancel").variant("outline").on_press(|s: &mut App| ui(s).confirm = None).build(t);
    let confirm = kit::Button::new("Use credit").on_press(move |s: &mut App| redeem(s, &key, &target)).build(t);
    kit::Dialog::new("usage-reset-credit", "Use a reset credit?")
        .alert()
        .width(448.0)
        .open(true)
        .description("This redeems one credit on your account and clears the current rate-limit windows. It cannot be undone.")
        .child(row().w_full().gap(8.0).justify_end().child(cancel).child(confirm))
        .on_close(|s: &mut App| ui(s).confirm = None)
        .build(t, overlays);
}

/// Spends the credit on the environment that reported it.
fn redeem(s: &mut App, key: &str, (environment, input): &Redeem) {
    ui(s).confirm = None;
    let Some(net) = env_net(s, environment) else { return };
    let id = net.request("provider.consumeResetCredit", input.clone());
    let u = ui(s);
    u.status.remove(key);
    u.redeeming = Some((id, key.to_string(), environment.clone()));
}

/// The `usage-limits-pooled` scenes: a second Codex login, a CLI proxy account and a failing
/// proxy from `usage.json`'s `pooled` recording, beside the recorded limits.
pub(super) fn pooled_scene(s: &mut App) {
    let recording = super::fixture("usage.json").unwrap_or(Value::Null);
    let pooled = &recording["pooled"];
    if let crate::data::Source::Fixture(state) = &mut s.src
        && let Some(config) = state.config.as_mut()
    {
        if let Some(providers) = config["providers"].as_array_mut() {
            providers.extend(list(&pooled["providers"]).iter().cloned());
        }
        config["usageLimitSources"] = pooled["usageLimitSources"].clone();
    }
}

/// The `usage-limits-environments` scenes: a saved "Studio Mac" where the same Codex login is
/// signed in, with a Claude login of its own.
pub(super) fn environments_scene(s: &mut App) {
    let Some(mut config) = s.state().config.clone() else { return };
    config["environment"]["label"] = json!("Studio Mac");
    config["environment"]["environmentId"] = json!("studio-env");
    for p in config["providers"].as_array_mut().into_iter().flatten() {
        let driver = field(p, "driver").to_string();
        match driver.as_str() {
            "codex" => p["usageLimits"]["checkedAt"] = json!("2026-11-06T10:20:00.000Z"),
            "claudeAgent" => p["auth"]["email"] = json!("studio@example.com"),
            _ => {}
        }
    }
    let mut state = crate::data::empty_state();
    state.conn = s5_net::Conn::Open;
    state.shell_ready = true;
    state.config = Some(config);
    s.ui.env.saved.push(crate::env::Saved { id: "studio".into(), label: "Studio Mac".into(), base: "https://studio.example".into(), bearer: "scene".into() });
    crate::env::park(s, "studio", crate::data::Source::Fixture(state));
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-11-06T10:30:00.000Z";

    fn now() -> i64 {
        iso_ms(NOW).unwrap()
    }

    fn provider(instance: &str, driver: &str, email: &str, windows: Value) -> Value {
        json!({
            "instanceId": instance, "driver": driver, "displayName": driver_label(driver), "enabled": true, "installed": true,
            "auth": {"status": "authenticated", "email": email, "label": "Pro"},
            "usageLimits": {"checkedAt": "2026-11-06T10:00:00.000Z", "windows": windows},
        })
    }

    fn window(id: &str, kind: &str, used: f64, resets: &str) -> Value {
        json!({"id": id, "kind": kind, "label": id, "usedPercent": used, "resetsAt": resets, "windowDurationMins": 300})
    }

    fn env(key: &str, label: &str, config: Value) -> Env {
        Env { key: key.into(), label: label.into(), config }
    }

    #[test]
    fn logins_merge_across_instances_and_hub_accounts() {
        let mut second = provider("codex_2", "codex", " Me@Example.com ", json!([window("primary", "session", 50.0, "2026-11-06T12:00:00.000Z")]));
        second["usageLimits"]["checkedAt"] = json!("2026-11-06T10:10:00.000Z");
        second["usageLimits"]["resetCredits"] = json!({"availableCount": 1});
        let config = json!({
            "providers": [provider("codex", "codex", "me@example.com", json!([window("primary", "session", 10.0, "2026-11-06T12:00:00.000Z")])), second],
            "usageLimitSources": [{"id": "hub", "label": "CLI Proxy", "accounts": [
                {"id": "me.JSON", "driver": "codex", "email": "me@example.com", "plan": "Team", "usageLimits": {"checkedAt": "2026-11-06T09:00:00.000Z", "windows": [window("primary", "session", 90.0, "2026-11-06T12:00:00.000Z")], "resetCredits": {"availableCount": 3, "nextCreditId": "c1"}}},
                {"id": "other.json", "driver": "claudeAgent", "usageLimits": {"checkedAt": "2026-11-06T09:00:00.000Z", "credentialFingerprint": "fp", "windows": [window("five_hour", "session", 5.0, "2026-11-06T12:00:00.000Z")]}},
            ]}],
        });
        let accounts = collect_accounts(&[env("", "Laptop", config)]);
        assert_eq!(accounts.len(), 2);
        let me = &accounts[0];
        // The freshest read's windows, the hub's redemption, and its e-mail over "Codex".
        assert_eq!(used(&windows(&me.limits)[0]), 50.0);
        assert_eq!(me.redeem, Some((String::new(), json!({"sourceId": "hub", "accountId": "me.JSON", "creditId": "c1"}))));
        assert_eq!(me.limits["resetCredits"]["availableCount"], 1);
        assert_eq!((me.display_name.as_deref(), me.plan.as_deref(), me.source_label.as_deref()), (None, Some("Pro"), None));
        assert_eq!(me.environments, vec![(String::new(), "Laptop".to_string())]);
        assert_eq!((accounts[1].display_name.as_deref(), accounts[1].source_label.as_deref()), (Some("other"), Some("CLI Proxy")));
    }

    #[test]
    fn one_login_on_two_environments_is_signed_in_on_both() {
        let local = provider("codex", "codex", "me@example.com", json!([window("primary", "session", 10.0, NOW)]));
        let mut remote = provider("codex", "codex", "Me@example.com", json!([window("primary", "session", 30.0, NOW)]));
        remote["usageLimits"]["checkedAt"] = json!("2026-11-06T10:20:00.000Z");
        remote["usageLimits"]["resetCredits"] = json!({"availableCount": 1});
        let hub = json!([{"id": "hub", "label": "CLI Proxy", "accounts": [], "error": "Offline."}]);
        let envs = [env("", "Laptop", json!({"providers": [local]})), env("studio", "Studio Mac", json!({"providers": [remote], "usageLimitSources": hub}))];
        let accounts = collect_accounts(&envs);
        assert_eq!(accounts.len(), 1);
        let me = &accounts[0];
        assert_eq!(me.key, "local:codex");
        assert_eq!(me.environments.iter().map(|e| e.1.as_str()).collect::<Vec<_>>(), ["Laptop", "Studio Mac"]);
        // The credit is spent on the environment that reported it; the freshest read shows.
        assert_eq!(me.redeem, Some(("studio".to_string(), json!({"instanceId": "codex"}))));
        assert_eq!(used(&windows(&me.limits)[0]), 30.0);
        assert_eq!(collect_notices(&envs), ["Studio Mac · CLI Proxy: Offline."]);
        assert_eq!(collect_links(&envs).len(), 0);
    }

    #[test]
    fn workspaces_split_one_login_and_unknown_ones_stay_apart() {
        let mut a = provider("codex", "codex", "me@example.com", json!([window("primary", "session", 10.0, NOW)]));
        a["auth"]["workspaceId"] = json!("w1");
        let mut b = provider("codex_b", "codex", "me@example.com", json!([window("primary", "session", 20.0, NOW)]));
        b["auth"]["workspaceId"] = json!("w2");
        let c = provider("codex_c", "codex", "me@example.com", json!([window("primary", "session", 30.0, NOW)]));
        let keys = |config: Value| collect_accounts(&[env("", "L", config)]).into_iter().map(|a| a.key).collect::<Vec<_>>();
        assert_eq!(keys(json!({"providers": [a.clone(), b, c.clone()]})), ["local:codex", "local:codex_b", "local:codex_c"]);
        // One known workspace claims the login that names none.
        assert_eq!(keys(json!({"providers": [a, c]})), ["local:codex"]);
    }

    #[test]
    fn notices_and_links_follow_the_web_copy() {
        let mut failing = provider("claudeAgent", "claudeAgent", "me@example.com", json!([]));
        failing["usageLimits"]["unavailable"] = json!({"reason": "probeFailed"});
        let mut unsupported = provider("grok", "grok", "", json!([]));
        unsupported["usageLimits"]["unavailable"] = json!({"reason": "unsupported"});
        let mut linked = provider("codex", "codex", "me@example.com", json!([]));
        linked["usageLimits"]["externalUsage"] = json!({"label": "ChatGPT usage", "url": "https://chatgpt.com/#settings/Usage"});
        let config = json!({
            "providers": [failing, unsupported, linked],
            "usageLimitSources": [{"id": "a", "label": "Hub", "accounts": [], "error": "Offline."}, {"id": "b", "label": "Empty", "accounts": []}],
        });
        let hidden = presentation(&config, &["codex"]);
        let envs = [env("", "Laptop", config)];
        assert_eq!(collect_notices(&envs), ["Claude: Could not read limits.", "Codex: No limits reported.", "Hub: Offline.", "Empty: No accounts reported."]);
        let links = collect_links(&envs);
        assert_eq!((links.len(), links[0].accounts.clone()), (1, vec!["Codex on Laptop".to_string()]));
        assert_eq!(list(&hidden["providers"]).len(), 2);
        assert_eq!(list(&hidden["usageLimitSources"]).len(), 2);
    }

    fn account(key: &str, driver: &str, windows: Value) -> Account {
        Account {
            key: key.into(),
            driver: driver.into(),
            display_name: Some(key.into()),
            email: None,
            plan: None,
            accent: None,
            environments: vec![(String::new(), "L".into())],
            source_label: None,
            redeem: None,
            limits: json!({"checkedAt": "2026-11-06T10:00:00.000Z", "windows": windows}),
        }
    }

    #[test]
    fn pools_order_accounts_by_reset_and_refill_sessions_a_spent_week_blocks() {
        let spent = account(
            "a",
            "codex",
            json!([window("primary", "session", 40.0, "2026-11-06T11:00:00.000Z"), window("secondary", "weekly", 100.0, "2026-11-07T09:00:00.000Z")]),
        );
        let fresh = account("b", "codex", json!([window("primary", "session", 20.0, "2026-11-06T10:45:00.000Z"), window("secondary", "weekly", 50.0, "2026-11-09T00:00:00.000Z")]));
        let pools = collect_pools(vec![spent, fresh], now());
        let pool = &pools[0];
        // The blocked session resets with the week, so "b" resets first.
        assert_eq!(pool.accounts.iter().map(|a| a.key.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        let session = &pool.windows[0];
        assert_eq!((session.id.as_str(), session.remaining, session.columns.clone()), ("session", 40, vec![Some(0), Some(1)]));
        let blocked = &session.members[1];
        assert_eq!((used(&blocked.window), blocked.refill, field(&blocked.window, "resetsAt")), (100.0, 100.0, "2026-11-07T09:00:00.000Z"));
        assert!(blocked.window.get("windowDurationMins").is_none());
        assert_eq!(session.resets.iter().map(|r| (r.member, r.restores)).collect::<Vec<_>>(), [(0, 10), (1, 50)]);
        assert_eq!(session.pace, Some(Pace::Under));
        assert_eq!((pool.windows[1].id.as_str(), pool.windows[1].remaining), ("weekly", 25));
    }

    #[test]
    fn a_second_window_of_one_kind_gets_its_own_pool() {
        let a = account("a", "codex", json!([window("primary", "weekly", 10.0, NOW), window("spark", "weekly", 30.0, NOW)]));
        let pools = collect_pools(vec![a], now());
        assert_eq!(pools[0].windows.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), ["weekly", "weekly:spark"]);
    }

    #[test]
    fn cursor_shows_its_split_pools_and_antigravity_gemini_first() {
        let cursor = account("c", "cursor", json!([window("totalPercentUsed", "monthly", 1.0, NOW), window("apiPercentUsed", "monthly", 1.0, NOW), window("autoPercentUsed", "monthly", 1.0, NOW)]));
        let agy = account("g", "antigravity", json!([window("claude-weekly", "weekly", 1.0, NOW), window("gemini-pro-weekly", "weekly", 1.0, NOW), window("gemini-pro-5h", "session", 1.0, NOW)]));
        let pools = collect_pools(vec![cursor, agy], now());
        let ids = |p: &Pool| display_windows(p).iter().map(|w| w.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&pools[0]), ["autoPercentUsed", "apiPercentUsed"]);
        assert_eq!(ids(&pools[1]), ["gemini-pro-5h", "gemini-pro-weekly", "claude-weekly"]);
    }

    #[test]
    fn figures_read_like_the_web() {
        let w = window("p", "session", 38.4, "2026-11-06T12:00:00.000Z");
        assert_eq!(remaining_percent(&w), 62);
        assert_eq!(resets_in(&w, now()).as_deref(), Some("resets in 1h 30m"));
        assert_eq!(pace_remaining(&w, now()), Some(30));
        assert_eq!(resets_in(&window("p", "session", 0.0, "2026-11-06T10:30:30.000Z"), now()).as_deref(), Some("Resets momentarily"));
        let credits = json!({"availableCount": 2, "nextExpiresAt": "2026-11-08T10:30:00.000Z"});
        assert_eq!(credits_summary(&credits, now(), false), "2 reset credits banked · next expires in 2d 0h");
        assert_eq!(credits_summary(&credits, now(), true), "2 banked · expires in 2d 0h");
        assert_eq!(credits_summary(&json!({"availableCount": 0}), now(), true), "No reset credits banked");
        let utc = Zone::Fixed(0);
        assert_eq!(upcoming(iso_ms("2026-11-06T14:16:00Z").unwrap(), now(), utc, true), "2:16 PM");
        assert_eq!(upcoming(iso_ms("2026-11-07T09:05:00Z").unwrap(), now(), utc, false), "tomorrow at 09:05");
        assert_eq!(upcoming(iso_ms("2026-11-12T00:05:00Z").unwrap(), now(), utc, true), "11/12 12:05 AM");
        assert_eq!(redeemed(&Ok(json!({"outcome": "noCredit"}))), "No reset credit left.");
        assert_eq!(redeemed(&Err(json!({}))), "Could not use the reset credit.");
    }

    #[test]
    fn account_chips_scramble_and_colour_like_the_web() {
        assert_eq!(redacted("sparsh@example.com"), "5vaw63@q3cqdu8.37s");
        assert_eq!(redacted("work@acme.io"), "bj48@g6uw.de");
        assert_eq!((hue("sparsh@example.com"), hue("work@acme.io")), (288.0, 125.0));
        assert_eq!((initials("work@acme.io"), initials("")), ("WA".to_string(), "?".to_string()));
        assert_eq!(hub_name("Team.JSON"), "Team");
    }
}
