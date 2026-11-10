//! A `/pull-requests` row's own controls (PullRequestRow): the quick actions Shift reveals and the
//! close sweep across rows, the stack menu, the checks popover, the change breakdown on hover, the
//! description-match badge and the provider mark.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::time::Duration;

use rune::{Color, Cursor, Drag, El, Key, KeyEvent, Phase, ScrollSpec, Tx, col, el, icon, row, spacer, text};
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
/// The quick actions' runAction grant (AuthSourceControlWriteScope).
const SOURCE_CONTROL_WRITE: &str = "source-control:write";
/// A close sweep starts once the pointer has moved this far (the sweep sensor's `distance`).
const SWEEP_DISTANCE: f32 = 6.0;
/// A list row (56px) and the 2px under it, and the xs Close button's top in the row: rows in a
/// group are all one height, so the row under the pointer follows from how far it moved.
const ROW_PITCH: f32 = 58.0;
const CLOSE_TOP: f32 = 16.0;

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
    /// The connection's session (`GET /api/auth/session`), read for its write grant.
    pub session: Read,
    sweep: Option<Sweep>,
    batches: Vec<Batch>,
}

/// A quick action on its way: merging reads the detail (and stack) before it runs. A close from a
/// sweep belongs to its batch.
struct Acting {
    key: String,
    entry: Value,
    action: &'static str,
    step: Step,
    id: u64,
    batch: Option<u64>,
}

/// A press on Close, and once it moved far enough, the rows it swept (startCloseSweep).
struct Sweep {
    origin: String,
    /// The origin's group as shown when pressed, top to bottom.
    rows: Vec<Value>,
    started: bool,
    target: Option<usize>,
    swept: Vec<String>,
}

/// Closes sent together, reported in one toast once all answered (usePullRequestCloseBatch).
struct Batch {
    id: u64,
    size: usize,
    closed: usize,
    failures: Vec<String>,
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
        if let Some(reply) = u.session.pending.and_then(|id| net.take_reply(id)) {
            u.session.pending = None;
            u.session.at = now;
            match reply {
                Ok(v) => {
                    u.session.data = Some(v);
                    u.session.error = None;
                }
                Err(e) => u.session.error = Some(super::pull_requests::error_text(&e, "")),
            }
            changed = true;
        }
        if u.session.pending.is_none() && (u.session.at == 0 || now - u.session.at >= STALE_MS) {
            u.session.pending = Some(net.http_request("GET", "/api/auth/session", json!({})));
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

/// sessionGrantsScope: the session's permissions when it lists them, else its scopes, where a
/// legacy credential's `orchestration:operate` still grants source control writes.
fn grants(session: &Value, scope: &str) -> bool {
    let has = |list: &Value, scope: &str| list.as_array().is_some_and(|l| l.iter().any(|v| v == scope));
    if session["authenticated"] != true {
        return false;
    }
    if session.get("permissions").is_some_and(|p| !p.is_null()) {
        return has(&session["permissions"], scope);
    }
    if has(&session["scopes"], scope) {
        return true;
    }
    if session["auth"].get("serverUpdateScope").is_some() {
        return false;
    }
    scope == SOURCE_CONTROL_WRITE && has(&session["scopes"], "orchestration:operate")
}

/// runAction's permissionAtom: whether this connection may act on pull requests. Unknown until the
/// session answers; the recorded scenes may.
fn can_write(s: &App) -> bool {
    let session = &s.ui.pages.pull_requests.row.session;
    match (&session.data, &session.error) {
        (_, Some(_)) => false,
        (Some(v), None) => grants(v, SOURCE_CONTROL_WRITE),
        (None, None) => !matches!(s.src, crate::data::Source::Live(_)),
    }
}

/// PullRequestSpeedActions: the row's quick actions while Shift is held, one is running, or a close
/// sweep passes over the row. They stay disabled while the connection can't write.
pub fn speed_actions(s: &App, t: &T, result: &Value, e: &Value) -> Option<El> {
    let u = &s.ui.pages.pull_requests.row;
    let key = row_key(e);
    let sweeping = u.sweep.as_ref().is_some_and(|w| w.started && w.swept.contains(&key));
    let busy = sweeping || u.acting.iter().any(|a| a.key == key);
    let actions = quick_actions(e, &host_actions(result, e));
    if actions.is_empty() || !(u.speed || busy) {
        return None;
    }
    let writable = can_write(s);
    let stacked = e["stack"].is_object();
    let (term, number) = (term(e), &e["number"]);
    let mut group = row().key(format!("pr-speed-{key}")).shrink().gap(4.0).pr(12.0).center_y().role("group").label(format!("Quick actions for {term} #{number}"));
    for action in actions {
        let (label, glyph): (&str, &'static rune::Icon) = match action {
            "close" => ("Close", &i::GIT_PULL_REQUEST_CLOSED),
            "merge" => ("Merge", &i::GIT_MERGE),
            "ready" => ("Ready for review", &i::GIT_PULL_REQUEST_ARROW),
            _ => ("Reopen", &i::GIT_PULL_REQUEST_ARROW),
        };
        let tip = match action {
            "merge" if stacked => format!("Open this {term} to merge its stack"),
            "close" => "Close immediately, or drag across rows to close several".to_string(),
            _ => format!("{label} immediately"),
        };
        let disabled = !writable || busy || (action == "merge" && stacked);
        let mut button = kit::Button::new(label)
            .variant(if action == "close" { "destructive-outline" } else { "outline" })
            .size("xs")
            .icon(if busy { &i::LOADER_CIRCLE } else { glyph })
            .disabled(disabled);
        // Close acts on release instead, so a drag from it can sweep other rows.
        if action != "close" {
            let entry = e.clone();
            button = button.on_press(move |s: &mut App| perform(s, &entry, action));
        }
        let mut control = button.build(t).label(format!("{label} #{number}")).help(tip);
        if action == "close" && !disabled {
            let (dragged, keyed) = (e.clone(), e.clone());
            control = control.on_drag(move |s: &mut App, d: &mut Drag, _: &mut Tx| sweep(s, &dragged, d)).on_key(move |s: &mut App, k: &KeyEvent, _: &mut Tx| {
                match k.key {
                    Key::Enter => perform(s, &keyed, "close"),
                    Key::Text(ref c) if c == " " => perform(s, &keyed, "close"),
                    _ => return false,
                }
                true
            });
        }
        group = group.child(control);
    }
    Some(group)
}

// ---- close sweep ------------------------------------------------------------------------

/// Escape ends a sweep in progress without closing anything.
pub(super) fn cancel_sweep(u: &mut Ui) -> bool {
    u.sweep.take().is_some()
}

/// The rows of the group `key` is shown in, top to bottom.
fn group_of(s: &App, key: &str) -> Vec<Value> {
    let list = &s.ui.pages.pull_requests;
    let Some(result) = list.result.as_ref() else { return Vec::new() };
    super::pull_requests::groups(list, result).into_iter().map(|(_, rows)| rows).find(|rows| rows.iter().any(|r| row_key(r) == key)).unwrap_or_default()
}

/// The sweep's canClose: an open row its host can close, with no action of its own on the way.
fn closable(u: &Ui, result: &Value, e: &Value) -> bool {
    let key = row_key(e);
    field(e, "state") == "open" && host_actions(result, e).iter().any(|a| a == "close") && !u.acting.iter().any(|a| a.key == key)
}

/// The row under the pointer, `y` below the top of the origin row at `at`, within its group: past
/// either end the whole rest of the group is swept, as rows outside it never close.
fn sweep_target(len: usize, at: usize, y: f32) -> usize {
    (at as f32 + (y / ROW_PITCH).floor()).clamp(0.0, len.saturating_sub(1) as f32) as usize
}

/// startCloseSweep: a drag from a row's Close closes the open rows it passes in that row's group,
/// all at once on release; a press that doesn't move closes the row alone.
fn sweep(s: &mut App, e: &Value, d: &Drag) {
    let key = row_key(e);
    match d.phase {
        Phase::Down => {
            let rows = group_of(s, &key);
            ui(s).sweep = Some(Sweep { origin: key, rows, started: false, target: None, swept: Vec::new() });
        }
        Phase::Move => {
            let list = &s.ui.pages.pull_requests;
            let Some(w) = list.row.sweep.as_ref().filter(|w| w.origin == key) else { return };
            if !w.started && d.dx.hypot(d.dy) < SWEEP_DISTANCE {
                return;
            }
            let (Some(at), Some(result)) = (w.rows.iter().position(|r| row_key(r) == key), list.result.as_ref()) else { return };
            let target = sweep_target(w.rows.len(), at, CLOSE_TOP + d.y);
            if w.started && w.target == Some(target) {
                return;
            }
            let swept: Vec<String> = w.rows[at.min(target)..=at.max(target)].iter().filter(|r| closable(&list.row, result, r)).map(row_key).collect();
            if let Some(w) = ui(s).sweep.as_mut() {
                w.started = true;
                w.target = Some(target);
                w.swept = swept;
            }
        }
        Phase::Up => {
            let Some(w) = ui(s).sweep.take().filter(|w| w.origin == key) else { return };
            if d.cancelled() {
                return;
            }
            if !w.started {
                return perform(s, e, "close");
            }
            let rows = group_of(s, &key);
            let list = &s.ui.pages.pull_requests;
            let batch: Vec<Value> = match list.result.as_ref() {
                Some(result) => rows.into_iter().filter(|r| w.swept.contains(&row_key(r)) && closable(&list.row, result, r)).collect(),
                None => Vec::new(),
            };
            close_batch(s, batch);
        }
    }
}

/// usePullRequestCloseBatch: closes the open rows at once; each leaves the open list as it closes,
/// and one toast reports them all.
fn close_batch(s: &mut App, entries: Vec<Value>) {
    let acting = &s.ui.pages.pull_requests.row.acting;
    let batch: Vec<Value> = entries.into_iter().filter(|e| field(e, "state") == "open" && !acting.iter().any(|a| a.key == row_key(e))).collect();
    if batch.is_empty() {
        return;
    }
    let mut sent = Vec::new();
    if let Some(net) = s.src.net() {
        for e in batch {
            let id = net.request("pullRequests.runAction", action_payload(&e, "close", None));
            sent.push(Acting { key: row_key(&e), entry: e, action: "close", step: Step::Run, id, batch: None });
        }
    }
    let Some(id) = sent.first().map(|a| a.id) else {
        return crate::shell::palette::toast(s, "Actions need a connected environment.", None);
    };
    let u = ui(s);
    u.batches.push(Batch { id, size: sent.len(), closed: 0, failures: Vec::new() });
    u.acting.extend(sent.into_iter().map(|a| Acting { batch: Some(id), ..a }));
}

/// One close of a batch answered: a closed row leaves the open list at once, and the last answer
/// reports the batch.
fn settle(s: &mut App, batch: u64, a: &Acting, reply: Result<Value, Value>) {
    let failure = match reply {
        Ok(_) => {
            super::pull_requests::after_action(&mut s.ui.pages.pull_requests, &a.key, "close", crate::data::now_ms());
            None
        }
        Err(e) => Some(format!("#{}: {}", a.entry["number"], readable(&super::pull_requests::error_text(&e, ""), failure_text("close", term(&a.entry)).1))),
    };
    let u = ui(s);
    let Some(n) = u.batches.iter().position(|b| b.id == batch) else { return };
    let b = &mut u.batches[n];
    match failure {
        Some(f) => b.failures.push(f),
        None => b.closed += 1,
    }
    if b.closed + b.failures.len() < b.size {
        return;
    }
    let b = u.batches.remove(n);
    let (title, description) = batch_summary(b.size, b.closed, &b.failures);
    crate::shell::palette::toast(s, &title, description.as_deref());
}

/// The batch toast: how many closed, and the first three failures.
fn batch_summary(size: usize, closed: usize, failures: &[String]) -> (String, Option<String>) {
    if failures.is_empty() {
        return (format!("Closed {closed} pull request{}", if closed == 1 { "" } else { "s" }), None);
    }
    (format!("Closed {closed} of {size} pull requests"), Some(failures.iter().take(3).cloned().collect::<Vec<_>>().join("\n")))
}

/// usePullRequestActionRunner: one action per row at a time; a merge first reads the detail to
/// check it can merge and to choose the method.
fn perform(s: &mut App, e: &Value, action: &'static str) {
    let key = row_key(e);
    if ui(s).acting.iter().any(|a| a.key == key) {
        return;
    }
    let a = Acting { key, entry: e.clone(), action, step: Step::Run, id: 0, batch: None };
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
    if let Some(batch) = a.batch {
        return settle(s, batch, &a, reply);
    }
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
    super::pull_requests::after_action(list, &a.key, a.action, crate::data::now_ms());
}

fn failed(s: &mut App, a: &Acting, raw: &str) {
    let (title, hint) = failure_text(a.action, term(&a.entry));
    crate::shell::palette::toast(s, &title, Some(&readable(raw, hint)));
}

/// actionFailureLabels and actionFailureHints.
fn failure_text(action: &str, term: &str) -> (String, &'static str) {
    match action {
        "merge" => (
            format!("Could not merge this {term}"),
            "The host refused the merge. Check that you have write access, that the checks it requires have passed, and that the branch is not conflicting.",
        ),
        "ready" => ("Could not mark this ready for review".to_string(), "The host refused it. Check that you have write access to this repository."),
        "close" => (format!("Could not close this {term}"), "The host refused it. Check that you have write access, or that you opened it."),
        _ => (format!("Could not reopen this {term}"), "The host refused it. Check that you have write access, and that the branch still exists."),
    }
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
            .child(
                text(field(c, "name").to_string())
                    .font(theme::TEXT_XS)
                    .color(t.c.foreground)
                    .truncate()
                    .grow(1.0)
                    .shrink()
                    .min_w(0.0)
                    .help(c["description"].as_str().unwrap_or(field(c, "name")).to_string()),
            )
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

/// savedPullRequestStack: this row's stack as the newest thread link saved it (the row's own link,
/// else one whose stack holds the row), with each layer's saved title. Saved membership only
/// navigates; it never carries the heads an action needs.
fn saved_stack<'a>(links: impl Iterator<Item = &'a Value>, e: &Value) -> Option<Value> {
    let host = field(e, "host").to_lowercase();
    if host.is_empty() {
        return None;
    }
    let repository = field(e, "repository").to_lowercase();
    let number = &e["number"];
    let matching: Vec<&Value> = links.filter(|l| field(l, "host").to_lowercase() == host && field(l, "repository").to_lowercase() == repository).collect();
    let synced = |l: &&Value| Reverse(crate::data::iso_ms(l["snapshot"]["syncedAt"].as_str().unwrap_or(field(l, "linkedAt"))).unwrap_or(i64::MIN));
    let holds = |stack: &Value, n: &Value| stack["layers"].as_array().is_some_and(|layers| layers.iter().any(|layer| &layer["number"] == n));
    let exact: Vec<&Value> = matching.iter().copied().filter(|l| &l["number"] == number).collect();
    let candidates = if exact.is_empty() { matching.iter().copied().filter(|l| holds(&l["stack"], number)).collect() } else { exact };
    let stack = &candidates.into_iter().min_by_key(synced)?["stack"];
    if !holds(stack, number) {
        return None;
    }
    let layers: Vec<Value> = stack["layers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|layer| {
            let mut layer = layer.clone();
            let n = layer["number"].clone();
            if let Some(snapshot) = matching.iter().copied().filter(|l| l["number"] == n).min_by_key(synced).map(|l| &l["snapshot"]).filter(|v| v.is_object()) {
                layer["title"] = snapshot["title"].clone();
                layer["isDraft"] = snapshot["isDraft"].clone();
            }
            layer
        })
        .collect();
    Some(json!({"id": stack["id"], "number": stack["number"], "url": stack["url"], "base": stack["base"], "layers": layers}))
}

/// pullRequestStackView: the host's answer once it succeeded, else what it last held or the saved
/// stack; with its notice, whether that may be stale (true) or is refreshing (false).
fn stack_view(read: Option<&Read>, saved: Option<Value>) -> (Option<Value>, Option<bool>) {
    let success = read.is_some_and(|r| r.data.is_some() && r.error.is_none());
    let held = read.and_then(|r| r.data.clone()).filter(Value::is_object);
    let data = if success { held } else { held.or(saved) };
    let notice = match read {
        _ if data.is_none() => None,
        Some(Read { error: Some(_), .. }) => Some(true),
        _ if !success || read.is_some_and(|r| r.pending.is_some()) => Some(false),
        _ => None,
    };
    (data, notice)
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
        .help(format!("View stack #{number}, layer {position} of {size}"))
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
                .children(notice.map(|stale| {
                    let (label, tip) = if stale { ("May be stale", "Stack data may be stale. We couldn’t refresh it.") } else { ("Refreshing…", "Refreshing stack… Showing saved data.") };
                    text(label).font(theme::TEXT_XS).color(muted).role("status").help(tip)
                }))
        };
        let read = u.stacks.get(&key);
        let saved = saved_stack(s.state().threads.iter().flat_map(|th| th["pullRequests"].as_array().into_iter().flatten()), e);
        match stack_view(read, saved) {
            (Some(stack), notice) => {
                menu = menu.row(header(&stack["number"], notice));
                if notice == Some(true) {
                    let entry = e.clone();
                    menu = menu.item(kit::MenuItem::new("Retry stack refresh").on_select(move |s: &mut App| want(s, Kind::Stack, &entry)));
                }
                for layer in stack["layers"].as_array().into_iter().flatten().rev() {
                    menu = menu.row(layer_row(t, e, layer));
                }
                menu = menu.group_label(format!("↳ {}", field(&stack, "base")));
            }
            (None, _) => {
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
        .help("Matched in the description")
        .child(icon(&i::SEARCH, 12.0).color(t.c.muted_foreground))
        .child(text("matched in the description").font(theme::TEXT_3XS).color(t.c.muted_foreground).truncate().shrink())
}

/// PullRequestRow's provider mark, with its name as the tooltip, once the list spans more than one
/// host (showProvider).
pub fn provider_mark(s: &App, t: &T, result: &Value, e: &Value) -> Option<El> {
    let kept = &s.ui.pages.pull_requests.hosts;
    let hosts = if kept.is_empty() { result["providers"].as_array().map_or(0, Vec::len) } else { kept.len() };
    if hosts < 2 {
        return None;
    }
    let (kind, name) = (field(e, "provider"), provider_name(field(e, "provider")));
    Some(el().label(name).help(name).child(icon(host_icon(kind), 12.0).color(t.c.muted_foreground)))
}

/// The source control definition's label for a provider kind.
fn provider_name(kind: &str) -> &'static str {
    match kind {
        "github" => "GitHub",
        "gitlab" => "GitLab",
        "bitbucket" => "Bitbucket",
        "azure-devops" => "Azure DevOps",
        "gitcafe" => "GitCafe",
        "forgejo" => "Forgejo",
        _ => "source control",
    }
}

// One-colour source control marks (getSourceControlPresentationForKind's Icon): each host's own
// silhouette, without the gradients Bitbucket and Azure DevOps paint theirs with.
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

static BITBUCKET: rune::Icon = rune::Icon {
    name: "pr-host-bitbucket",
    view: 2481.29,
    body: "<path d=\"M80.52 125.05C36.62 124.48 0.57 159.61 0.01 203.51c-0.06 4.61 0.28 9.22 1.02 13.77l337.48 2048.72c8.68 51.75 53.26 89.8 105.74 90.24h1619.03c39.38 0.5 73.19 -27.9 79.49 -66.78l337.49 -2071.78c7.03 -43.34 -22.41 -84.17 -65.75 -91.2c-4.55 -0.74 -9.15 -1.08 -13.76 -1.02L80.52 125.05zM1501.59 1605.74H984.84l-139.92 -731h781.89L1501.59 1605.74z\" fill=\"currentColor\" fill-rule=\"evenodd\"/>",
};
static AZURE_DEVOPS: rune::Icon = rune::Icon {
    name: "pr-host-azure-devops",
    view: 512.0,
    body: "<path d=\"M116.1 73.4c-5.8 5.8-13.7 13.8-13.7 22.6v216.4c0 8.9-3.3 17.3-9 23.4s-13.3 9.2-21.1 8.5l-42.7-3.1c-7.9-.5-15-3.9-20.3-9.3-5.7-5.7-9.3-13.7-9.3-22.6V202.7c0-8.9 3.6-16.9 9.4-22.6l2-1.9z\" fill=\"currentColor\"/><path d=\"m500.5 366-128 106.7c-5.5 4.6-12.7 7.4-20.5 7.4q-2.4 0-4.8-.3l-112.5-16.9v27.8c0 11.7-9.6 21.3-21.3 21.3-7.3 0-13.5-3.6-17.5-9.1l-36.2-51.4-5.4-7.6-36.6-51.9-7-10c-2.6-3.5-3.9-7.8-3.9-12.3 0-6 2.5-11.6 6.8-15.7 3.8-3.7 9.1-5.8 14.5-5.8.4 0 1 .1 1.5.1l105.2 7.5L384 366.5l93.7 6.7c.7.1 1.6.1 2.3.1 7.8 0 14.9-2.7 20.5-7.3\" fill=\"currentColor\"/><path d=\"M512 170.7v170.7c0 9.9-4.5 18.8-11.5 24.6l-128 106.7c7-5.9 11.5-14.7 11.5-24.6V175.3c0-16.7 13-30.7 29.8-31.9l63.9-4.6c.7-.1 1.6-.1 2.3-.1 7.8 0 14.9 2.8 20.5 7.4 7 5.8 11.5 14.6 11.5 24.6\" fill=\"currentColor\"/><path d=\"M384 125.5c0 11.2-8.6 20.5-19.8 21.3-.5 0-1.1.1-1.6.1l-127.9 9.2-132.3 9.1-72.7 5.5c-6.9.4-13.2 3.1-18.2 7.4L116.1 73.4c4.7-4.7 10.9-8 17.8-9.1l100.8-15.1V21.3C234.7 9.6 244.3 0 256 0c5.8 0 11.1 2.3 14.9 6.1l33.4 32.6 16.9 16.5 40.3 39.3 16.1 15.8c4.1 3.9 6.4 9.4 6.4 15.2\" fill=\"currentColor\"/>",
};

/// The host's mark for a provider kind, as the host picker and rows show it.
pub fn host_icon(kind: &str) -> &'static rune::Icon {
    match kind {
        "github" => &GITHUB,
        "gitlab" => &GITLAB,
        "gitcafe" => &GITCAFE,
        "forgejo" => &FORGEJO,
        "bitbucket" => &BITBUCKET,
        "azure-devops" => &AZURE_DEVOPS,
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
        "stack-saved" => {
            // Dismissed links still save the stack, and leave the recorded sidebar as it was.
            let stack = recorded(Kind::Stack);
            let links: Vec<Value> = stack["layers"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|layer| {
                    json!({"host": e["host"], "repository": e["repository"], "number": layer["number"], "url": e["url"], "source": "stack-dismissed", "linkedAt": "2026-11-08T09:00:00.000Z",
                        "snapshot": {"state": "open", "title": layer["title"], "headBranch": layer["headBranch"], "baseBranch": stack["base"], "isDraft": false, "updatedAt": null, "syncedAt": "2026-11-08T09:00:00.000Z"},
                        "stack": {"kind": "native", "id": stack["id"], "number": stack["number"], "url": stack["url"], "base": stack["base"], "layers": stack["layers"]}})
                })
                .collect();
            if let crate::data::Source::Fixture(state) = &mut s.src
                && let Some(thread) = state.threads.first_mut()
            {
                thread["pullRequests"] = json!(links);
            }
            let u = ui(s);
            u.stacks.insert(key.clone(), Read { pending: Some(0), ..Read::default() });
            u.stack_open = Some(key);
        }
        "checks" => {
            want(s, Kind::Checks, &e);
            ui(s).checks_open = Some(key);
        }
        "sweep" => {
            let rows = group_of(s, &key);
            let Some(at) = rows.iter().position(|r| row_key(r) == key) else { return };
            let target = sweep_target(rows.len(), at, CLOSE_TOP + 2.0 * ROW_PITCH);
            let list = &s.ui.pages.pull_requests;
            let swept: Vec<String> = match list.result.as_ref() {
                Some(result) => rows[at..=target].iter().filter(|r| closable(&list.row, result, r)).map(row_key).collect(),
                None => return,
            };
            ui(s).sweep = Some(Sweep { origin: key, rows, started: true, target: Some(target), swept });
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
    fn shift_alone_shows_the_quick_actions_until_released() {
        let mut s = App::new();
        s.route = Route::PullRequests;
        let key = |pressed: bool, mods: rune::Mods| KeyEvent { key: Key::Modifier { modifier: rune::Modifier::ShiftLeft, pressed }, mods };
        let shift = || rune::Mods { shift: true, ..Default::default() };
        assert!(on_key(&mut s, &key(true, shift())));
        assert!(s.ui.pages.pull_requests.row.speed);
        assert!(!on_key(&mut s, &key(true, shift())));
        assert!(on_key(&mut s, &key(false, rune::Mods::default())));
        assert!(!s.ui.pages.pull_requests.row.speed);
        // Shift inside a chord is a shortcut, and other pages ignore it.
        assert!(!on_key(&mut s, &key(true, rune::Mods { shift: true, ctrl: true, cmd: true, ..Default::default() })));
        s.route = Route::Home { project_id: None };
        assert!(!on_key(&mut s, &key(true, shift())));
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
        assert_eq!(stack_view(Some(&held), None).1, Some(true));
        assert_eq!(row_key(&entry(json!({}))), "github.com:acme/app#7");
    }
    #[test]
    fn quick_actions_need_the_source_control_write_grant() {
        let w = SOURCE_CONTROL_WRITE;
        assert!(grants(&json!({"authenticated": true, "scopes": [w]}), w));
        assert!(grants(&json!({"authenticated": true, "scopes": ["orchestration:operate"], "auth": {}}), w));
        assert!(!grants(&json!({"authenticated": true, "scopes": ["orchestration:operate"], "auth": {"serverUpdateScope": "server:update"}}), w));
        assert!(!grants(&json!({"authenticated": true, "scopes": [w], "permissions": ["orchestration:read"]}), w));
        assert!(!grants(&json!({"authenticated": false, "scopes": [w]}), w));
        let mut s = App::new();
        assert!(can_write(&s), "the recorded scenes may act");
        s.ui.pages.pull_requests.row.session.data = Some(json!({"authenticated": true, "scopes": ["orchestration:read"]}));
        assert!(!can_write(&s));
        s.ui.pages.pull_requests.row.session.data = Some(json!({"authenticated": true, "permissions": [w]}));
        assert!(can_write(&s));
        s.ui.pages.pull_requests.row.session.error = Some("offline".into());
        assert!(!can_write(&s));
    }

    #[test]
    fn a_close_sweep_takes_the_closable_rows_from_the_origin_to_the_pointer() {
        assert_eq!(sweep_target(5, 2, CLOSE_TOP), 2);
        assert_eq!(sweep_target(5, 2, -1.0), 1);
        assert_eq!(sweep_target(5, 2, ROW_PITCH * 2.0 + 1.0), 4);
        assert_eq!(sweep_target(5, 2, ROW_PITCH * 9.0), 4);
        assert_eq!(sweep_target(5, 0, -500.0), 0);
        let mut u = Ui::default();
        let result = json!({"providers": [{"host": "github.com"}]});
        let open = entry(json!({}));
        assert!(closable(&u, &result, &open));
        assert!(closable(&u, &result, &entry(json!({"isDraft": true}))));
        assert!(!closable(&u, &result, &entry(json!({"state": "closed"}))));
        assert!(!closable(&u, &json!({"providers": [{"host": "github.com", "actions": ["merge"]}]}), &open));
        u.acting.push(Acting { key: row_key(&open), entry: open.clone(), action: "merge", step: Step::Detail, id: 1, batch: None });
        assert!(!closable(&u, &result, &open));
    }

    #[test]
    fn a_batch_closes_each_row_as_it_answers_and_reports_once() {
        let mut s = App::new();
        let rows = [entry(json!({})), entry(json!({"number": 8})), entry(json!({"number": 9}))];
        s.ui.pages.pull_requests.result = Some(json!({"entries": rows}));
        let acting = |e: &Value| Acting { key: row_key(e), entry: e.clone(), action: "close", step: Step::Run, id: 0, batch: Some(1) };
        ui(&mut s).batches.push(Batch { id: 1, size: 3, closed: 0, failures: Vec::new() });
        settle(&mut s, 1, &acting(&rows[0]), Ok(Value::Null));
        let refused = json!({"_tag": "PullRequestOperationError", "operation": "runAction", "detail": "Not allowed."});
        settle(&mut s, 1, &acting(&rows[1]), Err(refused));
        let states = |s: &App| s.ui.pages.pull_requests.result.as_ref().map(|r| r["entries"].as_array().into_iter().flatten().map(|e| field(e, "state").to_string()).collect::<Vec<_>>());
        assert_eq!(states(&s), Some(vec!["closed".into(), "open".into(), "open".into()]));
        let b = &s.ui.pages.pull_requests.row.batches[0];
        assert_eq!((b.closed, b.failures.as_slice()), (1, ["#8: Not allowed.".to_string()].as_slice()));
        settle(&mut s, 1, &acting(&rows[2]), Ok(Value::Null));
        assert!(s.ui.pages.pull_requests.row.batches.is_empty());
        assert_eq!(batch_summary(3, 2, &["#8: Not allowed.".into()]), ("Closed 2 of 3 pull requests".to_string(), Some("#8: Not allowed.".to_string())));
        assert_eq!(batch_summary(1, 1, &[]), ("Closed 1 pull request".to_string(), None));
        let failures: Vec<String> = (1..=5).map(|n| format!("#{n}: refused")).collect();
        assert_eq!(batch_summary(5, 0, &failures).1.map(|d| d.lines().count()), Some(3));
    }

    #[test]
    fn the_stack_menu_falls_back_to_the_newest_saved_link() {
        let layers = json!([{"number": 6, "headBranch": "base-fix", "state": "open"}, {"number": 7, "headBranch": "top", "state": "open"}]);
        let stack = |n: i64| json!({"kind": "native", "id": "s", "number": n, "url": "u", "base": "main", "layers": layers});
        let link = |number: i64, at: &str, stack: Value, title: &str| {
            json!({"host": "GitHub.com", "repository": "Acme/App", "number": number, "linkedAt": at, "snapshot": {"title": title, "isDraft": false, "syncedAt": at}, "stack": stack})
        };
        let links = [link(7, "2026-01-01T00:00:00Z", stack(1), "old"), link(7, "2026-02-01T00:00:00Z", stack(2), "seven"), link(6, "2026-01-01T00:00:00Z", Value::Null, "six")];
        let saved = saved_stack(links.iter(), &entry(json!({}))).expect("saved stack");
        assert_eq!((&saved["number"], &saved["layers"][0]["title"], &saved["layers"][1]["title"]), (&json!(2), &json!("six"), &json!("seven")));
        // The row's own link without a stack wins over another link's stack holding it.
        assert!(saved_stack(links.iter(), &entry(json!({"number": 6}))).is_none());
        assert_eq!(saved_stack(links[..2].iter(), &entry(json!({"number": 6}))).map(|s| s["number"].clone()), Some(json!(2)));
        assert!(saved_stack(links.iter(), &entry(json!({"number": 5}))).is_none());

        let read = |data: Option<Value>, error: Option<&str>, pending: Option<u64>| Read { data, error: error.map(str::to_string), pending, at: 0 };
        let saved = Some(json!({"number": 2}));
        assert_eq!(stack_view(Some(&read(None, None, Some(1))), saved.clone()), (saved.clone(), Some(false)));
        assert_eq!(stack_view(Some(&read(None, Some("offline"), None)), saved.clone()), (saved.clone(), Some(true)));
        assert_eq!(stack_view(Some(&read(Some(json!({"number": 3})), Some("offline"), None)), saved.clone()), (Some(json!({"number": 3})), Some(true)));
        assert_eq!(stack_view(Some(&read(Some(Value::Null), None, None)), saved.clone()), (None, None));
        assert_eq!(stack_view(Some(&read(Some(json!({"number": 3})), None, None)), None), (Some(json!({"number": 3})), None));
        assert_eq!(stack_view(None, None), (None, None));
    }
}
