//! A `/pull-requests` row's own controls (PullRequestRow): the quick actions Shift reveals, the
//! stack menu, the checks popover, the change breakdown on hover and the description-match badge.

use std::collections::HashMap;
use std::time::Duration;

use rune::{Color, Cursor, El, Key, KeyEvent, ScrollSpec, Tx, col, el, icon, row, spacer, text};
use serde_json::{Value, json};

use super::pr_detail::{action_payload, reference};
use crate::data::s as field;
use crate::icons as i;
use crate::kit::{self, Menu, Placement, Popover, Tooltip};
use crate::theme::{self, AlphaMul, T, radius, tw};
use crate::{App, Route};

/// Answers older than this are read again when shown (the queries' `staleTimeMs`).
const STALE_MS: i64 = 60_000;
/// readableFailure's FAILURE_DETAIL_MAX_LENGTH.
const FAILURE_MAX: usize = 320;
const BREAKDOWN_KINDS: [(&str, &str); 5] = [("code", "Code"), ("tests", "Tests"), ("mocks", "Mocks"), ("generated", "Generated"), ("docs", "Docs")];

/// One row's lazily read answer: `pullRequests.stack`, `.detail` (for its checks) or `.changeBreakdown`.
#[derive(Default)]
pub struct Read {
    pub data: Option<Value>,
    pub error: Option<String>,
    pending: Option<u64>,
    at: i64,
}

#[derive(Default)]
pub struct Ui {
    /// Shift alone is held, so every row shows its quick actions (PullRequestSpeedModeProvider).
    pub speed: bool,
    acting: Vec<Acting>,
    pub stacks: HashMap<String, Read>,
    pub checks: HashMap<String, Read>,
    pub breakdowns: HashMap<String, Read>,
    /// The row whose stack menu, checks popover or change breakdown is showing, by row key.
    pub stack_open: Option<String>,
    pub checks_open: Option<String>,
    pub checks_all: bool,
    pub hovered: Option<String>,
}

/// A quick action on its way: merging reads the detail (and stack) before it runs.
struct Acting {
    key: String,
    entry: Value,
    action: &'static str,
    step: Step,
    id: u64,
}

enum Step {
    Detail,
    Stack(Value),
    Run,
}

#[derive(Clone, Copy)]
enum Kind {
    Stack,
    Checks,
    Breakdown,
}

fn ui(s: &mut App) -> &mut Ui {
    &mut s.ui.pages.pull_requests.row
}

/// `host:repository#number`, as pullRequestEntryKey.
pub fn row_key(e: &Value) -> String {
    format!("{}:{}#{}", field(e, "host"), field(e, "repository"), e["number"])
}

fn reads(u: &mut Ui, kind: Kind) -> &mut HashMap<String, Read> {
    match kind {
        Kind::Stack => &mut u.stacks,
        Kind::Checks => &mut u.checks,
        Kind::Breakdown => &mut u.breakdowns,
    }
}

/// Reads one row's answer unless a fresh one is held or already on its way.
fn want(s: &mut App, kind: Kind, e: &Value) {
    let key = row_key(e);
    let now = s.now_ms;
    let fresh = reads(ui(s), kind).get(&key).is_some_and(|r| r.pending.is_some() || (r.data.is_some() && r.error.is_none() && now - r.at < STALE_MS));
    if fresh {
        return;
    }
    let method = match kind {
        Kind::Stack => "pullRequests.stack",
        Kind::Checks => "pullRequests.detail",
        Kind::Breakdown => "pullRequests.changeBreakdown",
    };
    match s.src.net() {
        Some(net) => {
            let id = net.request(method, reference(e));
            reads(ui(s), kind).entry(key).or_default().pending = Some(id);
        }
        None if !crate::host::live() => {
            let r = reads(ui(s), kind).entry(key).or_default();
            r.data = Some(recorded(kind));
            r.at = now;
        }
        None => {}
    }
}

/// The bundled answers the fixture shell shows for any row.
fn recorded(kind: Kind) -> Value {
    match kind {
        Kind::Checks => super::fixture("pull-request-detail.json").map(|r| r["detail"].clone()),
        Kind::Stack => super::fixture("pull-request-row.json").map(|r| r["stack"].clone()),
        Kind::Breakdown => super::fixture("pull-request-row.json").map(|r| r["changeBreakdown"].clone()),
    }
    .unwrap_or(Value::Null)
}

pub fn poll(s: &mut App) -> bool {
    if !matches!(s.frame_route(), Route::PullRequests) {
        return false;
    }
    let now = s.now_ms;
    let mut changed = false;
    let mut answered = Vec::new();
    if let Some(net) = s.src.net() {
        let u = &mut s.ui.pages.pull_requests.row;
        for map in [&mut u.stacks, &mut u.checks, &mut u.breakdowns] {
            for r in map.values_mut() {
                if let Some(reply) = r.pending.and_then(|id| net.take_reply(id)) {
                    r.pending = None;
                    match reply {
                        Ok(v) => {
                            r.data = Some(v);
                            r.error = None;
                            r.at = now;
                        }
                        Err(e) => r.error = Some(super::pull_requests::error_text(&e, "The environment request failed.")),
                    }
                    changed = true;
                }
            }
        }
        let mut n = 0;
        while n < u.acting.len() {
            match net.take_reply(u.acting[n].id) {
                Some(reply) => answered.push((u.acting.remove(n), reply)),
                None => n += 1,
            }
        }
    }
    for (a, reply) in answered {
        advance(s, a, reply);
        changed = true;
    }
    changed
}

/// Tracks Shift held alone; true when the rows' quick actions appear or go.
pub fn on_key(s: &mut App, e: &KeyEvent) -> bool {
    if !matches!(e.key, Key::Modifier { .. }) || !matches!(s.route, Route::PullRequests) {
        return false;
    }
    let speed = e.mods.shift && !e.mods.cmd && !e.mods.ctrl && !e.mods.alt;
    let u = ui(s);
    if u.speed == speed {
        return false;
    }
    u.speed = speed;
    true
}

// ---- quick actions ----------------------------------------------------------------------

/// The host's word for a change request (ChangeRequestTerminology.singular).
fn term(e: &Value) -> &'static str {
    match field(e, "provider") {
        "gitlab" => "merge request",
        "github" | "gitcafe" | "forgejo" | "bitbucket" | "azure-devops" => "pull request",
        _ => "change request",
    }
}

/// capitalizeTerm.
fn capitalized(term: &str) -> String {
    let mut chars = term.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// pullRequestHostActions: what the host reports, else what its kind allows when unreported.
fn host_actions(result: &Value, e: &Value) -> Vec<String> {
    let host = field(e, "host").to_lowercase();
    let reported = result["providers"].as_array().into_iter().flatten().filter(|p| p["actions"].is_array()).filter(|p| field(p, "host").to_lowercase() == host).last();
    match reported {
        Some(p) => p["actions"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect(),
        None if field(e, "provider") == "github" => ["close", "merge", "ready", "reopen"].map(String::from).to_vec(),
        None => Vec::new(),
    }
}

/// pullRequestQuickActions: what the row's state offers, among what its host allows.
fn quick_actions(e: &Value, host: &[String]) -> Vec<&'static str> {
    let for_state: &[&'static str] = match field(e, "state") {
        "merged" => &[],
        "closed" => &["reopen"],
        _ if e["isDraft"] == true => &["close", "ready"],
        _ => &["close", "merge"],
    };
    for_state.iter().copied().filter(|a| host.iter().any(|h| h == a)).collect()
}

/// PullRequestSpeedActions: the row's quick actions while Shift is held or one is running.
pub fn speed_actions(s: &App, t: &T, result: &Value, e: &Value) -> Option<El> {
    let u = &s.ui.pages.pull_requests.row;
    let key = row_key(e);
    let busy = u.acting.iter().any(|a| a.key == key);
    let actions = quick_actions(e, &host_actions(result, e));
    if actions.is_empty() || !(u.speed || busy) {
        return None;
    }
    let stacked = e["stack"].is_object();
    let mut group = row().key(format!("pr-speed-{key}")).shrink().gap(4.0).pr(12.0).center_y().role("group").label(format!("Quick actions for {} #{}", term(e), e["number"]));
    for action in actions {
        let (label, glyph): (&str, &'static rune::Icon) = match action {
            "close" => ("Close", &i::GIT_PULL_REQUEST_CLOSED),
            "merge" => ("Merge", &i::GIT_MERGE),
            "ready" => ("Ready for review", &i::GIT_PULL_REQUEST_ARROW),
            _ => ("Reopen", &i::GIT_PULL_REQUEST_ARROW),
        };
        let entry = e.clone();
        group = group.child(
            kit::Button::new(label)
                .variant(if action == "close" { "destructive-outline" } else { "outline" })
                .size("xs")
                .icon(if busy { &i::LOADER_CIRCLE } else { glyph })
                .disabled(busy || (action == "merge" && stacked))
                .on_press(move |s: &mut App| perform(s, &entry, action))
                .build(t),
        );
    }
    Some(group)
}

/// usePullRequestActionRunner: one action per row at a time; a merge first reads the detail to
/// check it can merge and to choose the method.
fn perform(s: &mut App, e: &Value, action: &'static str) {
    let key = row_key(e);
    if ui(s).acting.iter().any(|a| a.key == key) {
        return;
    }
    let a = Acting { key, entry: e.clone(), action, step: Step::Run, id: 0 };
    if action == "merge" {
        let mut fresh = reference(e);
        fresh["allowStale"] = json!(false);
        send(s, a, Step::Detail, "pullRequests.detail", fresh);
    } else {
        let payload = action_payload(e, action, None);
        send(s, a, Step::Run, "pullRequests.runAction", payload);
    }
}

fn send(s: &mut App, mut a: Acting, step: Step, method: &str, payload: Value) {
    match s.src.net() {
        Some(net) => {
            a.id = net.request(method, payload);
            a.step = step;
            ui(s).acting.push(a);
        }
        None => failed(s, &a, "Actions need a connected environment."),
    }
}

fn advance(s: &mut App, mut a: Acting, reply: Result<Value, Value>) {
    let answer = match reply {
        Ok(v) => v,
        Err(e) => return failed(s, &a, &super::pull_requests::error_text(&e, "")),
    };
    match std::mem::replace(&mut a.step, Step::Run) {
        Step::Detail => {
            let can = |list: &Value| list.as_array().is_some_and(|l| l.iter().any(|v| v == "merge"));
            if field(&answer, "state") != "open" || answer["isDraft"] == true || !can(&answer["capabilities"]["actions"]) || !can(&answer["viewerPermissions"]["actions"]) {
                return failed(s, &a, "This pull request cannot be merged.");
            }
            if answer["capabilities"]["stackActions"] == true {
                let mut fresh = reference(&a.entry);
                fresh["allowStale"] = json!(false);
                return send(s, a, Step::Stack(answer), "pullRequests.stack", fresh);
            }
            run_merge(s, a, &answer);
        }
        Step::Stack(detail) if answer.is_null() => run_merge(s, a, &detail),
        Step::Stack(_) => failed(s, &a, "Open this pull request to merge its stack."),
        Step::Run => succeeded(s, &a),
    }
}

fn run_merge(s: &mut App, a: Acting, detail: &Value) {
    let settings = crate::settings::server_settings(s);
    let project = settings["projectSettingsOverrides"][field(&a.entry, "projectId")]["pullRequestMergeMethod"].as_str().or_else(|| settings["pullRequestMergeMethod"].as_str());
    let method = merge_method(detail, project, s.ui.pages.detail.merge_method.as_deref());
    match method {
        Some(method) => {
            let payload = action_payload(&a.entry, "merge", Some(&method));
            send(s, a, Step::Run, "pullRequests.runAction", payload);
        }
        None => failed(s, &a, "No merge method is available for this repository."),
    }
}

/// resolvePullRequestMergeMethod over the methods the repository allows; None when it allows none.
fn merge_method(detail: &Value, project: Option<&str>, last: Option<&str>) -> Option<String> {
    let caps = &detail["mergeCapabilities"];
    let allowed: Vec<&str> = detail["capabilities"]["mergeMethods"].as_array().into_iter().flatten().filter_map(Value::as_str).filter(|m| caps[*m] == true).collect();
    [project, last].into_iter().flatten().find(|m| allowed.contains(m)).or(allowed.first().copied()).map(str::to_string)
}

fn succeeded(s: &mut App, a: &Acting) {
    let title = match a.action {
        "merge" => "Merge requested".to_string(),
        "ready" => "Marked ready for review".to_string(),
        "close" => format!("{} closed", capitalized(term(&a.entry))),
        _ => format!("{} reopened", capitalized(term(&a.entry))),
    };
    crate::shell::palette::toast(s, &title, None);
    let list = &mut s.ui.pages.pull_requests;
    // Hosts may accept a merge before it lands; the next read says whether it merged.
    if a.action == "merge" {
        list.requested = None;
        return;
    }
    let held = list.result.as_mut().and_then(|r| r["entries"].as_array_mut()).into_iter().flatten().find(|e| row_key(e) == a.key);
    if let Some(e) = held {
        match a.action {
            "close" => e["state"] = json!("closed"),
            "reopen" => e["state"] = json!("open"),
            _ => e["isDraft"] = json!(false),
        }
    }
}

fn failed(s: &mut App, a: &Acting, raw: &str) {
    let term = term(&a.entry);
    let (title, hint) = match a.action {
        "merge" => (
            format!("Could not merge this {term}"),
            "The host refused the merge. Check that you have write access, that the checks it requires have passed, and that the branch is not conflicting.",
        ),
        "ready" => ("Could not mark this ready for review".to_string(), "The host refused it. Check that you have write access to this repository."),
        "close" => (format!("Could not close this {term}"), "The host refused it. Check that you have write access, or that you opened it."),
        _ => (format!("Could not reopen this {term}"), "The host refused it. Check that you have write access, and that the branch still exists."),
    };
    crate::shell::palette::toast(s, &title, Some(&readable(raw, hint)));
}

/// readableFailure: the host's own words without the operation prefix, or the hint when they
/// are only tool noise.
fn readable(raw: &str, hint: &str) -> String {
    let detail = without_operation(raw).trim();
    if detail.is_empty() || noise(detail) {
        return hint.to_string();
    }
    if detail.chars().count() <= FAILURE_MAX { detail.to_string() } else { format!("{}…", detail.chars().take(FAILURE_MAX - 1).collect::<String>()) }
}

/// Drops `^Pull request operation \w+ failed:\s*` (any case).
fn without_operation(raw: &str) -> &str {
    const HEAD: &str = "pull request operation ";
    let lower = raw.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix(HEAD) else { return raw };
    let Some(end) = rest.find(" failed:") else { return raw };
    let word = &rest[..end];
    if word.is_empty() || !word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return raw;
    }
    raw[HEAD.len() + end + " failed:".len()..].trim_start()
}

/// TOOL_NOISE: "GitHub CLI failed", "command failed", "exited with code 1", "Unknown error".
fn noise(detail: &str) -> bool {
    let lower = detail.to_lowercase();
    let d = lower.strip_suffix('.').unwrap_or(&lower);
    let words: Vec<&str> = d.split_whitespace().collect();
    match words.as_slice() {
        ["unknown", "error"] => true,
        ["exite" | "exited", "with", "code" | "status", n] => n.bytes().all(|b| b.is_ascii_digit()),
        [lead @ .., "failed"] => {
            let mut rest = lead.concat();
            if let Some(r) = ["github", "gitlab", "bitbucket", "azuredevops", "gitcafe", "forgejo"].iter().find_map(|l| rest.strip_prefix(l)) {
                rest = r.to_string();
            }
            if let Some(r) = ["cli", "api"].iter().find_map(|l| rest.strip_prefix(l)) {
                rest = r.to_string();
            }
            rest.is_empty() || rest == "command"
        }
        _ => false,
    }
}

// ---- checks -----------------------------------------------------------------------------

fn amber(t: &T) -> Color {
    if t.dark { tw::AMBER_400.alpha_mul(0.9) } else { tw::AMBER_600 }
}

fn success(t: &T) -> Color {
    if t.dark { tw::EMERALD_300.alpha_mul(0.9) } else { tw::EMERALD_600 }
}

/// pullRequestChecksStatePresentation.
fn checks_state(t: &T, state: &str) -> Option<(&'static rune::Icon, Color, &'static str)> {
    match state {
        "passing" => Some((&i::CIRCLE_CHECK, success(t), "All checks have passed")),
        "failing" => Some((&i::CIRCLE_X, t.c.destructive, "Some checks were not successful")),
        "pending" => Some((&i::CIRCLE_DOT, amber(t), "Some checks haven't completed yet")),
        _ => None,
    }
}

/// An `action-required` check whose link is a workflow run waiting for approval.
fn workflow_approval(c: &Value) -> bool {
    const RUNS: &str = "/actions/runs/";
    field(c, "status") == "action-required"
        && c["url"].as_str().is_some_and(|url| {
            url.match_indices(RUNS).any(|(at, _)| {
                let rest = &url[at + RUNS.len()..];
                let digits = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
                digits > 0 && (digits == rest.len() || rest.as_bytes()[digits] == b'/')
            })
        })
}

/// CHECK_STATUS_PRESENTATION and pullRequestCheckStatusLabel.
fn check_status(t: &T, c: &Value) -> (&'static rune::Icon, Color, &'static str) {
    let faint = t.c.muted_foreground.alpha_mul(0.7);
    match field(c, "status") {
        "pending" => (&i::LOADER_CIRCLE, tw::AMBER_500, "Running"),
        "action-required" => (&i::CIRCLE_DOT, amber(t), if workflow_approval(c) { "Awaiting approval" } else { "Awaiting action" }),
        "success" => (&i::CIRCLE_CHECK, success(t), "Passed"),
        "failure" => (&i::CIRCLE_X, t.c.destructive, "Failed"),
        "cancelled" => (&i::CIRCLE_X, t.c.destructive, "Cancelled"),
        "skipped" => (&i::CIRCLE_SLASH, faint, "Skipped"),
        _ => (&i::CIRCLE_DASHED, faint, "Neutral"),
    }
}

/// groupPullRequestChecks, then the rows shown: finished checks fold behind "Show all" while any
/// need attention or are still running. Also says whether they fold.
fn visible_checks(checks: &[Value], all: bool) -> (Vec<&Value>, bool) {
    let of = |statuses: &[&str]| checks.iter().filter(|c| statuses.contains(&field(c, "status"))).collect::<Vec<_>>();
    let (attention, running, completed) = (of(&["failure", "cancelled", "action-required"]), of(&["pending"]), of(&["success", "skipped", "neutral"]));
    let folds = attention.len() + running.len() > 0 && !completed.is_empty();
    let mut shown = attention;
    shown.extend(running);
    if all || !folds {
        shown.extend(completed);
    }
    (shown, folds)
}

fn toggle_checks(s: &mut App, e: &Value) {
    let key = row_key(e);
    let u = ui(s);
    if u.checks_open.take().as_deref() == Some(key.as_str()) {
        return;
    }
    u.checks_open = Some(key);
    u.checks_all = false;
    want(s, Kind::Checks, e);
}

/// PullRequestChecksPopover: the checks glyph, opening the checks the detail reports.
pub fn checks(s: &App, t: &T, e: &Value, overlays: &mut Vec<El>) -> Option<El> {
    let (glyph, ink, label) = checks_state(t, e["checksState"].as_str()?)?;
    let u = &s.ui.pages.pull_requests.row;
    let key = row_key(e);
    let open = u.checks_open.as_deref() == Some(key.as_str());
    let entry = e.clone();
    let trigger = el().cursor(Cursor::Pointer).role("button").label(format!("Checks: {label}")).child(icon(glyph, 14.0).color(ink)).on_press(move |s: &mut App| toggle_checks(s, &entry));
    let mut popover = Popover::new(format!("pr-checks-{key}"))
        .trigger(trigger)
        .open(open)
        .placement(Placement::BottomStart)
        .width("md")
        .label(label)
        .on_close(|s: &mut App| ui(s).checks_open = None);
    if open {
        let muted = |m: &str| text(m.to_string()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap();
        let read = u.checks.get(&key);
        let body = match (read.and_then(|r| r.data.as_ref()), read.and_then(|r| r.error.as_deref())) {
            (_, Some(error)) => muted(error),
            (None, _) => muted(if read.is_some_and(|r| r.pending.is_some()) { "Loading checks…" } else { "No checks reported" }),
            (Some(detail), None) => checks_body(t, detail["checks"].as_array().map(Vec::as_slice).unwrap_or(&[]), u.checks_all),
        };
        popover = popover.child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground)).child(body);
    }
    Some(popover.build(&kit::theme(t), overlays))
}

fn checks_body(t: &T, checks: &[Value], all: bool) -> El {
    if checks.is_empty() {
        return text("No checks reported").font(theme::TEXT_XS).color(t.c.muted_foreground);
    }
    let (shown, folds) = visible_checks(checks, all);
    let mut list = col().key("pr-checks-list").w_full().max_h(256.0).gap(4.0).scroll(ScrollSpec { stick_bottom: false });
    for (n, c) in shown.into_iter().enumerate() {
        let (glyph, ink, status) = check_status(t, c);
        let mut line = row()
            .key(format!("pr-check-{n}"))
            .w_full()
            .gap(8.0)
            .center_y()
            .child(icon(glyph, 14.0).color(ink))
            .child(text(field(c, "name").to_string()).font(theme::TEXT_XS).color(t.c.foreground).truncate().grow(1.0).shrink().min_w(0.0))
            .child(text(status).font(theme::TEXT_XS).color(t.c.muted_foreground));
        if let Some(url) = c["url"].as_str().map(str::to_string) {
            line = line.child(text("Details").font(theme::TEXT_XS).color(t.c.primary).cursor(Cursor::Pointer).role("link").on_press_tx(move |_: &mut App, tx: &mut Tx| {
                let _ = tx.open_url(&url);
            }));
        }
        list = list.child(line);
    }
    let mut out = col().w_full().gap(4.0).child(list);
    if folds {
        out = out.child(kit::Button::new(if all { "Show less" } else { "Show all" }).variant("ghost").size("xs").on_press(|s: &mut App| ui(s).checks_all ^= true).build(t));
    }
    out
}

// ---- stack ------------------------------------------------------------------------------

fn toggle_stack(s: &mut App, e: &Value) {
    let key = row_key(e);
    let u = ui(s);
    if u.stack_open.take().as_deref() == Some(key.as_str()) {
        return;
    }
    u.stack_open = Some(key);
    want(s, Kind::Stack, e);
}

/// resolvePullRequestState for a stack layer: its glyph, tone and label.
fn layer_state(t: &T, layer: &Value) -> (&'static rune::Icon, Color, &'static str) {
    match field(layer, "state") {
        "merged" => (&i::GIT_MERGE, if t.dark { tw::VIOLET_300.alpha_mul(0.9) } else { tw::VIOLET_600 }, "Merged"),
        "closed" => (&i::GIT_PULL_REQUEST_CLOSED, if t.dark { tw::RED_300.alpha_mul(0.9) } else { tw::RED_600 }, "Closed"),
        _ if layer["isDraft"] == true => (&i::GIT_PULL_REQUEST_DRAFT, if t.dark { tw::ZINC_400.alpha_mul(0.8) } else { tw::ZINC_500 }, "Draft"),
        _ => (&i::GIT_PULL_REQUEST_ARROW, success(t), "Open"),
    }
}

/// The stack layer's page on its host, from this row's own link.
fn layer_url(url: &str, from: &Value, to: i64) -> Option<String> {
    url.strip_suffix(&format!("/{from}")).map(|base| format!("{base}/{to}"))
}

/// A layer shown in the list opens in the detail; one the list doesn't hold opens on its host.
fn select_layer(s: &mut App, tx: &mut Tx, e: &Value, number: i64) {
    ui(s).stack_open = None;
    let list = &mut s.ui.pages.pull_requests;
    let listed = list.result.as_ref().and_then(|r| {
        r["entries"].as_array()?.iter().find(|x| field(x, "host").eq_ignore_ascii_case(field(e, "host")) && field(x, "repository") == field(e, "repository") && x["number"] == number).map(|x| field(x, "url").to_string())
    });
    match listed {
        Some(url) => list.selected = Some(url),
        None => {
            if let Some(url) = layer_url(field(e, "url"), &e["number"], number) {
                let _ = tx.open_url(&url);
            }
        }
    }
}

/// pullRequestStackView's notice: whether the held stack is stale, or still refreshing.
fn stack_notice(read: &Read) -> Option<bool> {
    match (&read.error, read.pending) {
        (Some(_), _) => Some(true),
        (None, Some(_)) => Some(false),
        _ => None,
    }
}

/// PullRequestStackPopover: the row's layer in its stack, opening the stack's layers.
pub fn stack(s: &App, t: &T, e: &Value, overlays: &mut Vec<El>) -> Option<El> {
    let m = e.get("stack").filter(|m| m.is_object())?;
    let (number, position, size) = (&m["number"], &m["position"], &m["size"]);
    let u = &s.ui.pages.pull_requests.row;
    let key = row_key(e);
    let open = u.stack_open.as_deref() == Some(key.as_str());
    let muted = t.c.muted_foreground;
    let entry = e.clone();
    let trigger = row()
        .gap(4.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .role("button")
        .label(format!("Stack {number}, layer {position} of {size}"))
        .child(icon(&i::LAYERS, 12.0).color(muted))
        .child(text(format!("{position}/{size}")).font(theme::TEXT_XS).color(muted))
        .on_press(move |s: &mut App| toggle_stack(s, &entry));
    let mut menu = Menu::new(format!("pr-stack-{key}")).trigger(trigger).placement(Placement::BottomStart).width(320.0).open(open).on_close(|s: &mut App| ui(s).stack_open = None);
    if open {
        let header = |n: &Value, notice: Option<bool>| {
            row()
                .w_full()
                .px(8.0)
                .py(6.0)
                .gap(8.0)
                .center_y()
                .child(text(format!("Stack #{n}")).font(theme::XS_MEDIUM).color(muted))
                .child(spacer())
                .children(notice.map(|stale| text(if stale { "May be stale" } else { "Refreshing…" }).font(theme::TEXT_XS).color(muted)))
        };
        let read = u.stacks.get(&key);
        match read.and_then(|r| r.data.as_ref()).filter(|d| d.is_object()) {
            Some(stack) => {
                let notice = read.and_then(stack_notice);
                menu = menu.row(header(&stack["number"], notice));
                if notice == Some(true) {
                    let entry = e.clone();
                    menu = menu.item(kit::MenuItem::new("Retry stack refresh").on_select(move |s: &mut App| want(s, Kind::Stack, &entry)));
                }
                for layer in stack["layers"].as_array().into_iter().flatten().rev() {
                    menu = menu.row(layer_row(t, e, layer));
                }
                menu = menu.group_label(format!("↳ {}", field(stack, "base")));
            }
            None => {
                let status = match read {
                    Some(Read { error: Some(error), .. }) => error.clone(),
                    Some(Read { data: Some(_), .. }) => "This pull request is no longer in a stack.".to_string(),
                    _ => "Loading stack…".to_string(),
                };
                menu = menu.row(header(number, None)).group_label(status);
            }
        }
    }
    Some(menu.build(&kit::theme(t), overlays))
}

/// PullRequestStackLayerContent in a menu row, checked on the row's own layer.
fn layer_row(t: &T, e: &Value, layer: &Value) -> El {
    let (glyph, tone, label) = layer_state(t, layer);
    let number = layer["number"].as_i64().unwrap_or(0);
    let title = layer["title"].as_str().filter(|v| !v.is_empty()).unwrap_or(field(layer, "headBranch")).to_string();
    let entry = e.clone();
    row()
        .key(format!("pr-stack-layer-{number}"))
        .w_full()
        .px(8.0)
        .py(6.0)
        .gap(8.0)
        .radius(4.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .role("menuitem")
        .hover(t.c.accent)
        .child(icon(glyph, 16.0).color(tone))
        .child(
            col()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .child(text(title).font(theme::TEXT_SM).color(t.c.foreground).truncate())
                .child(text(format!("#{number} · {} · {label}", field(layer, "headBranch"))).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate()),
        )
        .children((layer["number"] == e["number"]).then(|| icon(&i::CHECK, 14.0).color(t.c.foreground)))
        .on_press_tx(move |s: &mut App, tx: &mut Tx| select_layer(s, tx, &entry, number))
}

// ---- change breakdown -------------------------------------------------------------------

/// formatDiffCount: 1234 is "1,234".
fn count(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (k, c) in digits.chars().enumerate() {
        if k > 0 && (digits.len() - k) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// The kinds with changed lines, in KIND_LABELS order.
fn breakdown_rows(b: &Value) -> Vec<(&'static str, i64, i64)> {
    BREAKDOWN_KINDS
        .iter()
        .map(|&(kind, label)| (label, b[kind]["additions"].as_i64().unwrap_or(0), b[kind]["deletions"].as_i64().unwrap_or(0)))
        .filter(|&(_, a, d)| a != 0 || d != 0)
        .collect()
}

/// PullRequestChangeBreakdownStat: the row's +N -M, splitting into code, tests, mocks, generated
/// and docs on hover; the split is read on the first hover, so unhovered rows cost nothing.
pub fn breakdown(s: &App, t: &T, e: &Value, stat: El, overlays: &mut Vec<El>) -> El {
    let key = row_key(e);
    let u = &s.ui.pages.pull_requests.row;
    let read = u.breakdowns.get(&key);
    let (hover_key, entry) = (key.clone(), e.clone());
    let trigger = stat.hover_delays(Duration::from_millis(600), Duration::ZERO).on_hover_change(move |s: &mut App, on, _: &mut Tx| {
        if on {
            ui(s).hovered = Some(hover_key.clone());
            want(s, Kind::Breakdown, &entry);
        } else if ui(s).hovered.as_deref() == Some(hover_key.as_str()) {
            ui(s).hovered = None;
        }
    });
    let content = match read {
        Some(r) if r.data.as_ref().is_some_and(Value::is_object) => r.data.as_ref().map(|b| breakdown_grid(t, b, (e["additions"].as_i64().unwrap_or(0), e["deletions"].as_i64().unwrap_or(0)))),
        Some(r) if r.pending.is_some() => Some(text("Loading breakdown").font(theme::TEXT_XS).color(t.c.foreground)),
        _ => None,
    };
    let open = content.is_some() && u.hovered.as_deref() == Some(key.as_str());
    let mut tip = Tooltip::new(format!("pr-stat-{key}")).placement(Placement::Bottom).trigger(trigger).open(open);
    if let Some(content) = content {
        tip = tip.child(content);
    }
    tip.build(&kit::theme(t), overlays)
}

fn breakdown_grid(t: &T, b: &Value, (additions, deletions): (i64, i64)) -> El {
    let mono = theme::mono(theme::TEXT_XS).features("'tnum' 1");
    let line = |label: &str, a: i64, d: i64| {
        row()
            .gap(16.0)
            .center_y()
            .child(row().w(72.0).child(text(label.to_string()).font(theme::TEXT_XS).color(t.c.foreground)))
            .child(row().w(56.0).justify_end().child(text(format!("+{}", count(a))).font(mono).color(t.c.diff_addition_foreground)))
            .child(row().w(56.0).justify_end().child(text(format!("-{}", count(d))).font(mono).color(t.c.diff_deletion)))
    };
    let mut grid = col().py(2.0).gap(4.0);
    for (label, a, d) in breakdown_rows(b) {
        grid = grid.child(line(label, a, d));
    }
    grid.child(el().w_full().h(1.0).fill(t.c.border)).child(line("Total", additions, deletions))
}

// ---- row meta ---------------------------------------------------------------------------

/// The badge on a row the hosts matched by its description, where no shown field holds the search.
pub fn matched_badge(t: &T) -> El {
    row()
        .h(16.0)
        .min_w(24.0)
        .px(4.0)
        .gap(4.0)
        .radius(radius::FULL)
        .border(1.0, t.c.border.alpha_mul(0.6))
        .center_y()
        .shrink()
        .label("matched in the description")
        .child(icon(&i::SEARCH, 12.0).color(t.c.muted_foreground))
        .child(text("matched in the description").font(theme::TEXT_3XS).color(t.c.muted_foreground).truncate().shrink())
}

// One-colour source control marks for menus (getSourceControlPresentationForKind's Icon). Bitbucket
// and Azure DevOps marks are gradients; they keep the generic glyph, as the palette's brands do.
static GITHUB: rune::Icon = rune::Icon {
    name: "pr-host-github",
    view: 16.0,
    body: "<path d=\"M8 0C3.58 0 0 3.58 0 8C0 11.54 2.29 14.53 5.47 15.59C5.87 15.66 6.02 15.42 6.02 15.21C6.02 15.02 6.01 14.39 6.01 13.72C4 14.09 3.48 13.23 3.32 12.78C3.23 12.55 2.84 11.84 2.5 11.65C2.22 11.5 1.82 11.13 2.49 11.12C3.12 11.11 3.57 11.7 3.72 11.94C4.44 13.15 5.59 12.81 6.05 12.6C6.12 12.08 6.33 11.73 6.56 11.53C4.78 11.33 2.92 10.64 2.92 7.58C2.92 6.71 3.23 5.99 3.74 5.43C3.66 5.23 3.38 4.41 3.82 3.31C3.82 3.31 4.49 3.1 6.02 4.13C6.66 3.95 7.34 3.86 8.02 3.86C8.7 3.86 9.38 3.95 10.02 4.13C11.55 3.09 12.22 3.31 12.22 3.31C12.66 4.41 12.38 5.23 12.3 5.43C12.81 5.99 13.12 6.7 13.12 7.58C13.12 10.65 11.25 11.33 9.47 11.53C9.76 11.78 10.01 12.26 10.01 13.01C10.01 14.08 10 14.94 10 15.21C10 15.42 10.15 15.67 10.55 15.59C13.71 14.53 16 11.53 16 8C16 3.58 12.42 0 8 0Z\" fill=\"currentColor\" fill-rule=\"evenodd\"/>",
};
static GITLAB: rune::Icon = rune::Icon {
    name: "pr-host-gitlab",
    view: 32.0,
    body: "<path d=\"m31.46 12.78-.04-.12-4.35-11.35A1.14 1.14 0 0 0 25.94.6c-.24 0-.47.1-.66.24-.19.15-.33.36-.39.6l-2.94 9h-11.9l-2.94-9A1.14 1.14 0 0 0 6.07.58a1.15 1.15 0 0 0-1.14.72L.58 12.68l-.05.11a8.1 8.1 0 0 0 2.68 9.34l.02.01.04.03 6.63 4.97 3.28 2.48 2 1.52a1.35 1.35 0 0 0 1.62 0l2-1.52 3.28-2.48 6.67-5h.02a8.09 8.09 0 0 0 2.7-9.36Z\" fill=\"currentColor\"/>",
};
static GITCAFE: rune::Icon = rune::Icon { name: "pr-host-gitcafe", view: 64.0, body: "<path d=\"M4.8 4.8h54.4v54.4H4.8z\" fill=\"currentColor\"/>" };
static FORGEJO: rune::Icon = rune::Icon {
    name: "pr-host-forgejo",
    view: 212.0,
    body: "<path d=\"M64 174 v-98 a50 50 0 0 1 50-50 h20\" stroke=\"currentColor\" stroke-width=\"25\" fill=\"none\"/><path d=\"M130 26 a18 18 0 1 0 36 0 a18 18 0 1 0 -36 0z\" stroke=\"currentColor\" stroke-width=\"15\" fill=\"none\"/><path d=\"M64 174 v-30 a50 50 0 0 1 50-50 h20\" stroke=\"currentColor\" stroke-width=\"25\" fill=\"none\"/><path d=\"M130 94 a18 18 0 1 0 36 0 a18 18 0 1 0 -36 0z\" stroke=\"currentColor\" stroke-width=\"15\" fill=\"none\"/><path d=\"M46 186 a18 18 0 1 0 36 0 a18 18 0 1 0 -36 0z\" stroke=\"currentColor\" stroke-width=\"15\" fill=\"none\"/>",
};

/// The host picker's mark for a provider kind.
pub fn host_icon(kind: &str) -> &'static rune::Icon {
    match kind {
        "github" => &GITHUB,
        "gitlab" => &GITLAB,
        "gitcafe" => &GITCAFE,
        "forgejo" => &FORGEJO,
        _ => &i::GIT_PULL_REQUEST_ARROW,
    }
}

// ---- scenes -----------------------------------------------------------------------------

/// Scene state: the fixture row numbered `number` with its stack menu, checks popover or change
/// breakdown showing.
pub fn scene(s: &mut App, number: i64, part: &str) {
    let Some(e) = s.ui.pages.pull_requests.result.as_ref().and_then(|r| r["entries"].as_array()?.iter().find(|e| e["number"] == number).cloned()) else { return };
    let key = row_key(&e);
    match part {
        "stack" => {
            want(s, Kind::Stack, &e);
            ui(s).stack_open = Some(key);
        }
        "checks" => {
            want(s, Kind::Checks, &e);
            ui(s).checks_open = Some(key);
        }
        _ => {
            want(s, Kind::Breakdown, &e);
            ui(s).hovered = Some(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(v: Value) -> Value {
        let mut e = json!({"provider": "github", "host": "github.com", "repository": "acme/app", "number": 7, "state": "open", "isDraft": false});
        if let (Some(e), Some(v)) = (e.as_object_mut(), v.as_object()) {
            e.extend(v.clone());
        }
        e
    }

    #[test]
    fn quick_actions_follow_the_state_and_what_the_host_allows() {
        let reported = json!({"providers": [{"host": "GitHub.com", "actions": ["close", "reopen"]}]});
        let open = entry(json!({}));
        assert_eq!(host_actions(&reported, &open), ["close", "reopen"]);
        assert_eq!(quick_actions(&open, &host_actions(&reported, &open)), ["close"]);
        let unreported = json!({"providers": [{"host": "github.com"}]});
        assert_eq!(quick_actions(&open, &host_actions(&unreported, &open)), ["close", "merge"]);
        assert_eq!(quick_actions(&entry(json!({"isDraft": true})), &host_actions(&unreported, &open)), ["close", "ready"]);
        assert_eq!(quick_actions(&entry(json!({"state": "closed"})), &host_actions(&unreported, &open)), ["reopen"]);
        assert!(quick_actions(&entry(json!({"state": "merged"})), &host_actions(&unreported, &open)).is_empty());
        let gitlab = entry(json!({"provider": "gitlab", "host": "gitlab.com"}));
        assert!(host_actions(&unreported, &gitlab).is_empty());
        assert_eq!((term(&gitlab), capitalized(term(&open)).as_str()), ("merge request", "Pull request"));
    }

    #[test]
    fn merging_picks_an_allowed_method_by_project_then_last_used() {
        let detail = json!({"capabilities": {"mergeMethods": ["merge", "squash", "rebase"]}, "mergeCapabilities": {"merge": false, "squash": true, "rebase": true}});
        assert_eq!(merge_method(&detail, Some("rebase"), Some("squash")).as_deref(), Some("rebase"));
        assert_eq!(merge_method(&detail, Some("merge"), Some("rebase")).as_deref(), Some("rebase"));
        assert_eq!(merge_method(&detail, None, None).as_deref(), Some("squash"));
        assert_eq!(merge_method(&json!({"capabilities": {"mergeMethods": ["merge"]}, "mergeCapabilities": {}}), None, None), None);
    }

    #[test]
    fn failures_keep_the_hosts_words_and_hide_tool_noise() {
        let hint = "The host refused it.";
        assert_eq!(readable("Pull request operation runAction failed: Base branch was modified.", hint), "Base branch was modified.");
        for noise in ["GitHub CLI failed.", "gh failed", "command failed", "exited with code 1", "Unknown error", "Pull request operation runAction failed: ", ""] {
            let expected = if noise == "gh failed" { noise } else { hint };
            assert_eq!(readable(noise, hint), expected, "{noise}");
        }
        assert_eq!(readable(&"x".repeat(400), hint).chars().count(), FAILURE_MAX);
    }

    #[test]
    fn finished_checks_fold_only_beside_ones_needing_attention() {
        let check = |name: &str, status: &str, url: Value| json!({"name": name, "status": status, "url": url});
        let checks = [check("lint", "success", Value::Null), check("test", "failure", Value::Null), check("e2e", "pending", Value::Null), check("ci", "action-required", json!("https://github.com/a/b/actions/runs/12/job"))];
        let names = |(shown, folds): (Vec<&Value>, bool)| (shown.iter().map(|c| field(c, "name").to_string()).collect::<Vec<_>>(), folds);
        assert_eq!(names(visible_checks(&checks, false)), (vec!["test".into(), "ci".into(), "e2e".into()], true));
        assert_eq!(names(visible_checks(&checks, true)).0.len(), 4);
        assert_eq!(names(visible_checks(&checks[..1], false)), (vec!["lint".into()], false));
        assert!(workflow_approval(&checks[3]));
        assert!(!workflow_approval(&check("ci", "action-required", json!("https://github.com/a/b/actions/runs/x"))));
    }

    #[test]
    fn the_breakdown_lists_changed_kinds_in_order_and_stacks_link_by_number() {
        let b = json!({"code": {"additions": 1290, "deletions": 30}, "tests": {"additions": 0, "deletions": 0}, "docs": {"additions": 4, "deletions": 0}});
        assert_eq!(breakdown_rows(&b), [("Code", 1290, 30), ("Docs", 4, 0)]);
        assert_eq!(count(1234567), "1,234,567");
        assert_eq!(layer_url("https://github.com/a/b/pull/7", &json!(7), 9).as_deref(), Some("https://github.com/a/b/pull/9"));
        let held = Read { data: Some(json!({})), error: Some("offline".into()), pending: None, at: 0 };
        assert_eq!(stack_notice(&held), Some(true));
        assert_eq!(row_key(&entry(json!({}))), "github.com:acme/app#7");
    }
}
