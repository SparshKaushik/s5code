//! Git actions in the details card (GitActionsControl): the quick action and its menu, the
//! commit, default-branch and publish dialogs, and live progress from `git.runStackedAction`.
use std::collections::BTreeSet;

use rune::{Color, Cursor, El, Icon, ScrollSpec, TextEdit, col, el, icon, row, text};
use serde_json::{Value, json};

use super::{ROW, cwd};
use crate::chat::composer;
use crate::shell::palette::{self, add_project};
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, data, icons, kit};

const SUCCESS_MS: i64 = 10_000;

#[derive(Default)]
pub struct State {
    running: Option<Running>,
    success: Option<Success>,
    commit: Option<Commit>,
    confirm: Option<Confirm>,
    publish: Option<Publish>,
    init: Option<u64>,
    discovery: Option<Value>,
    discovery_pending: Option<u64>,
}

struct Running {
    id: u64,
    cwd: String,
    pull: bool,
    label: Option<String>,
    phase_label: Option<String>,
    output: Option<String>,
    phase_at: i64,
    hook_at: Option<i64>,
    result: Option<Value>,
    failure: Option<String>,
}

struct Success {
    cwd: String,
    title: String,
    description: Option<String>,
    until: i64,
}

#[derive(Default)]
struct Commit {
    message: TextEdit,
    excluded: BTreeSet<String>,
    editing: bool,
}

#[derive(Clone)]
struct Confirm {
    action: &'static str,
    branch: String,
    includes_commit: bool,
    message: Option<String>,
    files: Option<Vec<String>>,
}

struct Publish {
    step: usize,
    provider: Option<usize>,
    repository: Option<TextEdit>,
    public: bool,
    remote: TextEdit,
    https: bool,
    advanced: bool,
    error: Option<String>,
    pending: Option<u64>,
    result: Option<Value>,
}

/// The `{local, remote}` pair merged the way `VcsStatusResult` reads, remote defaults included.
pub fn status(s: &App) -> Option<Value> {
    let v = super::vcs(s)?;
    let mut merged = v["local"].clone();
    let remote = if v["remote"].is_object() { v["remote"].clone() } else { json!({"hasUpstream": false, "aheadCount": 0, "behindCount": 0, "pr": null}) };
    for (key, value) in remote.as_object()? {
        merged[key] = value.clone();
    }
    Some(merged)
}

pub struct Terms {
    pub short: &'static str,
    pub singular: &'static str,
}

/// `changeRequestTerminologyFor`: no provider reads as GitHub; an unknown host as "change request".
pub fn terms(status: Option<&Value>) -> Terms {
    match status.and_then(|v| v["sourceControlProvider"]["kind"].as_str()) {
        Some("gitlab") => Terms { short: "MR", singular: "merge request" },
        None | Some("github" | "gitcafe" | "forgejo" | "bitbucket" | "azure-devops") => Terms { short: "PR", singular: "pull request" },
        Some(_) => Terms { short: "change request", singular: "change request" },
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Run(&'static str),
    Pull,
    Publish,
    Hint,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Quick {
    pub label: String,
    pub disabled: bool,
    pub kind: Kind,
    pub hint: Option<String>,
}

fn quick(label: impl Into<String>, kind: Kind) -> Quick {
    Quick { label: label.into(), disabled: false, kind, hint: None }
}
fn hint(label: &str, hint: impl Into<String>) -> Quick {
    Quick { label: label.into(), disabled: true, kind: Kind::Hint, hint: Some(hint.into()) }
}

fn counts(v: &Value) -> (bool, bool, bool, u64, u64, u64) {
    let ahead = v["aheadCount"].as_u64().unwrap_or(0);
    let behind = v["behindCount"].as_u64().unwrap_or(0);
    let ahead_of_default = v["aheadOfDefaultCount"].as_u64().unwrap_or(ahead);
    (!v["refName"].is_null(), v["hasWorkingTreeChanges"] == true, v["pr"]["state"] == "open", ahead, behind, ahead_of_default)
}

/// `resolveQuickAction`.
pub fn resolve_quick_action(status: Option<&Value>, busy: bool, is_default: bool, primary_remote: bool) -> Quick {
    if busy {
        return hint("Commit", "Git action in progress.");
    }
    let Some(v) = status else { return hint("Commit", "Git status is unavailable.") };
    let (branch, changes, open_pr, ahead, behind, ahead_of_default) = counts(v);
    let upstream = v["hasUpstream"] == true;
    let t = terms(status);
    if !branch {
        return hint("Commit", format!("Create and checkout a ref before pushing or opening a {}.", t.singular));
    }
    if changes {
        if behind > 0 || (!upstream && !primary_remote) {
            return quick("Commit", Kind::Run("commit"));
        }
        if open_pr || is_default {
            return quick("Commit & push", Kind::Run("commit_push"));
        }
        return quick(format!("Commit, push & {}", t.short), Kind::Run("commit_push_pr"));
    }
    let push = || {
        if open_pr || is_default {
            quick("Push", Kind::Run(if is_default { "commit_push" } else { "push" }))
        } else {
            quick(format!("Push & create {}", t.short), Kind::Run("create_pr"))
        }
    };
    if !upstream {
        if !primary_remote {
            return quick("Publish repository", Kind::Publish);
        }
        if ahead == 0 {
            return if open_pr { hint("Commit", "Branch is up to date. No action needed.") } else { hint("Push", "No local commits to push.") };
        }
        return push();
    }
    if ahead > 0 && behind > 0 {
        return hint("Sync ref", "Branch has diverged from upstream. Rebase/merge first.");
    }
    if behind > 0 {
        return quick("Pull", Kind::Pull);
    }
    if ahead > 0 {
        return push();
    }
    if !open_pr && ahead_of_default > 0 && !is_default {
        return quick(format!("Create {}", t.short), Kind::Run("create_pr"));
    }
    hint("Commit", "Branch is up to date. No action needed.")
}

#[derive(Clone, PartialEq, Debug)]
pub struct Item {
    pub id: &'static str,
    pub label: String,
    pub disabled: bool,
}

/// `buildMenuItems`.
pub fn build_menu_items(status: Option<&Value>, busy: bool, primary_remote: bool) -> Vec<Item> {
    let Some(v) = status else { return vec![] };
    let (branch, changes, open_pr, ahead, behind, ahead_of_default) = counts(v);
    let upstream = v["hasUpstream"] == true;
    let reachable = upstream || primary_remote;
    let mut items = vec![Item { id: "commit", label: "Commit".into(), disabled: busy || !changes }];
    if !primary_remote {
        return items;
    }
    items.push(Item { id: "push", label: "Push".into(), disabled: !(!busy && branch && behind == 0 && ahead > 0 && reachable) });
    if !open_pr {
        let can = !busy && branch && ahead_of_default > 0 && behind == 0 && reachable;
        items.push(Item { id: "pr", label: format!("Create {}", terms(status).short), disabled: !can });
    }
    items
}

/// `getMenuActionDisabledReason`.
pub fn disabled_reason(item: &Item, status: Option<&Value>, busy: bool, primary_remote: bool) -> Option<String> {
    if !item.disabled {
        return None;
    }
    if busy {
        return Some("Git action in progress.".into());
    }
    let Some(v) = status else { return Some("Git status is unavailable.".into()) };
    let (branch, changes, _, ahead, behind, ahead_of_default) = counts(v);
    let upstream = v["hasUpstream"] == true;
    let t = terms(status);
    Some(match item.id {
        "commit" if !changes => "Worktree is clean. Make changes before committing.".into(),
        "commit" => "Commit is currently unavailable.".into(),
        "push" if !branch => "Detached HEAD: check out a branch before pushing.".into(),
        "push" if changes => "Commit or stash local changes before pushing.".into(),
        "push" if behind > 0 => "Branch is behind upstream. Pull/rebase before pushing.".into(),
        "push" if !upstream && !primary_remote => "Add an \"origin\" remote before pushing.".into(),
        "push" if ahead == 0 => "No local commits to push.".into(),
        "push" => "Push is currently unavailable.".into(),
        _ if !branch => format!("Detached HEAD: check out a branch before creating a {}.", t.singular),
        _ if !upstream && !primary_remote => format!("Add an \"origin\" remote before creating a {}.", t.singular),
        _ if ahead_of_default == 0 => format!("No local commits to include in a {}.", t.singular),
        _ if behind > 0 => format!("Branch is behind upstream. Pull/rebase before creating a {}.", t.singular),
        _ => format!("Create {} is currently unavailable.", t.singular),
    })
}

/// `resolveDefaultBranchActionDialogCopy`: title, description, continue label.
pub fn default_branch_copy(action: &str, branch: &str, includes_commit: bool, t: &Terms) -> (String, String, String) {
    let suffix = format!(" on \"{branch}\". You can continue on this ref or create a feature ref and run the same action there.");
    let short = t.short;
    match (action == "push" || action == "commit_push", includes_commit) {
        (true, true) => ("Commit & push to default ref?".into(), format!("This action will commit and push changes{suffix}"), format!("Commit & push to {branch}")),
        (true, false) => ("Push to default ref?".into(), format!("This action will push local commits{suffix}"), format!("Push to {branch}")),
        (false, true) => (
            format!("Commit, push & create {short} from default ref?"),
            format!("This action will commit, push, and create a {}{suffix}", t.singular),
            format!("Commit, push & create {short}"),
        ),
        (false, false) => (
            format!("Push & create {short} from default ref?"),
            format!("This action will push local commits and create a {}{suffix}", t.singular),
            format!("Push & create {short}"),
        ),
    }
}

/// `formatGitActionElapsed`.
pub fn elapsed(started: i64, now: i64) -> String {
    let seconds = ((now - started) / 1000).max(0);
    if seconds < 60 { format!("{seconds}s") } else { format!("{}m {}s", seconds / 60, seconds % 60) }
}

/// Folds one `GitActionProgressEvent` into the running state (`applyVcsActionProgressEvent`).
fn apply(r: &mut Running, event: &Value, now: i64) {
    match data::s(event, "kind") {
        "action_started" => (r.phase_at, r.hook_at, r.output) = (now, None, None),
        "phase_started" => {
            let label = Some(data::s(event, "label").to_string());
            (r.label, r.phase_label, r.phase_at, r.hook_at, r.output) = (label.clone(), label, now, None, None);
        }
        "hook_started" => (r.label, r.hook_at, r.output) = (Some(format!("Running {}...", data::s(event, "hookName"))), Some(now), None),
        "hook_output" => r.output = Some(data::s(event, "text").to_string()),
        "hook_finished" => (r.label, r.hook_at, r.output) = (r.phase_label.clone(), None, None),
        "action_finished" => r.result = Some(event["result"].clone()),
        "action_failed" => r.failure = Some(data::s(event, "message").to_string()),
        _ => {}
    }
}

/// Busy per checkout, as web keys action state by `{environmentId, cwd}`.
fn is_busy(s: &App) -> bool {
    let g = &s.ui.chat.details.git;
    let here = cwd(s);
    g.running.as_ref().is_some_and(|r| Some(&r.cwd) == here.as_ref()) || g.publish.as_ref().is_some_and(|p| p.pending.is_some())
}

/// `resolveChangesTotals`: the branch's totals when it has work, else the working tree's, so a
/// fresh commit doesn't read +0 while the diff still shows it.
pub fn changes_totals(local: &Value) -> (u64, u64) {
    let pair = |v: &Value| (v["insertions"].as_u64().unwrap_or(0), v["deletions"].as_u64().unwrap_or(0));
    let tree = pair(&local["workingTree"]);
    let branch = local["branchChanges"].is_object().then(|| pair(&local["branchChanges"]));
    match branch {
        Some(b) if b != (0, 0) => b,
        _ if tree != (0, 0) => tree,
        b => b.unwrap_or(tree),
    }
}

fn quick_action(s: &App, status: Option<&Value>) -> Quick {
    let is_default = status.is_some_and(|v| v["isDefaultRef"] == true);
    let primary = status.is_some_and(|v| v["hasPrimaryRemote"] == true);
    resolve_quick_action(status, is_busy(s), is_default, primary)
}

/// Runs a stacked action, first asking on the default branch (`runGitActionWithToast`).
fn run(s: &mut App, action: &'static str, message: Option<String>, files: Option<Vec<String>>, feature_branch: bool, skip_prompt: bool) {
    let Some(cwd) = cwd(s) else { return };
    let status = status(s);
    let is_default = !feature_branch && status.as_ref().is_some_and(|v| v["isDefaultRef"] == true);
    let branch = status.as_ref().and_then(|v| v["refName"].as_str()).map(str::to_string);
    let commits = matches!(action, "commit" | "commit_push" | "commit_push_pr");
    let includes_commit = commits && (action == "commit" || status.as_ref().is_some_and(|v| v["hasWorkingTreeChanges"] == true) || feature_branch);
    if !skip_prompt && is_default && action != "commit"
        && let Some(branch) = branch
    {
        s.ui.chat.details.git.confirm = Some(Confirm { action, branch, includes_commit, message, files });
        return;
    }
    let c = composer::ctx(s);
    let mut payload = json!({"actionId": s5_net::uuid(), "cwd": cwd, "action": action});
    match (c.thread, c.project) {
        (Some(thread), _) => payload["threadId"] = json!(data::s(thread, "id")),
        (None, Some(project)) => payload["projectId"] = json!(data::s(project, "id")),
        _ => {}
    }
    if let Some(message) = message {
        payload["commitMessage"] = json!(message);
    }
    if feature_branch {
        payload["featureBranch"] = json!(true);
    }
    if let Some(files) = files {
        payload["filePaths"] = json!(files);
    }
    let Some(net) = s.src.net() else { return };
    let id = net.request("git.runStackedAction", payload);
    start(s, id, cwd, false);
}

fn start(s: &mut App, id: u64, cwd: String, pull: bool) {
    let git = &mut s.ui.chat.details.git;
    git.success = None;
    git.running = Some(Running { id, cwd, pull, label: None, phase_label: None, output: None, phase_at: data::now_ms(), hook_at: None, result: None, failure: None });
}

fn run_quick(s: &mut App) {
    let status = status(s);
    let q = quick_action(s, status.as_ref());
    match q.kind {
        Kind::Publish => open_publish(s),
        Kind::Pull => {
            let Some(cwd) = cwd(s) else { return };
            let Some(net) = s.src.net() else { return };
            let id = net.request("vcs.pull", json!({"cwd": cwd}));
            start(s, id, cwd, true);
        }
        Kind::Hint => palette::toast(s, &q.label, q.hint.as_deref()),
        Kind::Run(action) => run(s, action, None, None, false, false),
    }
}

fn pick(s: &mut App, id: &str) {
    s.ui.chat.details.menu = None;
    match id {
        "push" => run(s, "push", None, None, false, false),
        "pr" => run(s, "create_pr", None, None, false, false),
        "publish" => open_publish(s),
        _ => s.ui.chat.details.git.commit = Some(Commit::default()),
    }
}

/// The menu opens on fresh status (`requestVcsStatusRefresh`); a failed stream resubscribes.
pub fn refresh(s: &mut App) {
    if let (Some(cwd), Some(net)) = (cwd(s), s.src.net()) {
        net.retry_vcs(&cwd);
        net.request("vcs.refreshStatus", json!({"cwd": cwd}));
    }
}

/// Why the status stream failed, as the menu shows it (`gitStatusError`).
pub fn status_error(s: &App) -> Option<String> {
    let error = s.state().vcs.get(&cwd(s)?)?.get("error")?;
    Some(palette::error_message(error, "The environment request failed."))
}

fn init(s: &mut App) {
    let (Some(cwd), Some(net)) = (cwd(s), s.src.net()) else { return };
    s.ui.chat.details.git.init = Some(net.request("vcs.init", json!({"cwd": cwd})));
}

fn open_publish(s: &mut App) {
    s.ui.chat.details.menu = None;
    s.ui.chat.details.git.publish = Some(Publish {
        step: 0,
        provider: None,
        repository: None,
        public: false,
        remote: TextEdit::new("origin"),
        https: false,
        advanced: false,
        error: None,
        pending: None,
        result: None,
    });
    let git = &mut s.ui.chat.details.git;
    if git.discovery_pending.is_none() {
        git.discovery_pending = s.src.net().map(|net| net.request("server.discoverSourceControl", json!({})));
    }
}

pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    let now = data::now_ms();
    if s.ui.chat.details.git.success.as_ref().is_some_and(|v| now >= v.until) {
        s.ui.chat.details.git.success = None;
        changed = true;
    }
    if let Some(id) = s.ui.chat.details.git.running.as_ref().map(|r| r.id)
        && let Some(net) = s.src.net()
    {
        let chunks = net.take_chunks(id);
        let reply = net.take_reply(id);
        if let Some(r) = s.ui.chat.details.git.running.as_mut() {
            for event in &chunks {
                apply(r, event, now);
                changed = true;
            }
        }
        if let Some(reply) = reply {
            finish(s, reply);
            changed = true;
        }
    }
    if let Some(id) = s.ui.chat.details.git.init
        && let Some(reply) = s.src.net().and_then(|net| net.take_reply(id))
    {
        s.ui.chat.details.git.init = None;
        if let Err(e) = reply {
            palette::toast_error_brief(s, "Git initialization failed", Some(&palette::error_message(&e, "An error occurred.")));
        }
        changed = true;
    }
    if let Some(id) = s.ui.chat.details.git.discovery_pending
        && let Some(reply) = s.src.net().and_then(|net| net.take_reply(id))
    {
        let git = &mut s.ui.chat.details.git;
        git.discovery_pending = None;
        git.discovery = Some(reply.unwrap_or(Value::Null));
        changed = true;
    }
    if let Some(id) = s.ui.chat.details.git.publish.as_ref().and_then(|p| p.pending)
        && let Some(reply) = s.src.net().and_then(|net| net.take_reply(id))
        && let Some(p) = s.ui.chat.details.git.publish.as_mut()
    {
        p.pending = None;
        match reply {
            Ok(v) => (p.result, p.step) = (Some(v), 2),
            Err(e) => p.error = Some(palette::error_message(&e, "An error occurred.")),
        }
        changed = true;
    }
    changed
}

fn finish(s: &mut App, reply: s5_net::Reply) {
    let Some(r) = s.ui.chat.details.git.running.take() else { return };
    let until = data::now_ms() + SUCCESS_MS;
    if r.pull {
        match reply {
            Ok(v) => {
                let refname = data::s(&v, "refName");
                let (title, description) = if v["status"] == "pulled" {
                    ("Pulled", format!("Updated {refname} from {}", v["upstreamRef"].as_str().unwrap_or("upstream")))
                } else {
                    ("Already up to date", format!("{refname} is already synchronized."))
                };
                s.ui.chat.details.git.success = Some(Success { cwd: r.cwd, title: title.into(), description: Some(description), until });
            }
            Err(e) => palette::toast_error(s, "Pull failed", Some(&palette::error_message(&e, "An error occurred."))),
        }
        return;
    }
    let result = match (reply, r.result) {
        (Ok(_), Some(result)) => result,
        (Ok(v), None) => v,
        (Err(e), _) => {
            let message = r.failure.unwrap_or_else(|| palette::error_message(&e, "An error occurred."));
            return palette::toast_error(s, "Action failed", Some(&message));
        }
    };
    sync_branch(s, &result);
    let created = result["pr"]["status"] == "created";
    let title = if created { format!("Created {}", terms(status(s).as_ref()).short) } else { data::s(&result["toast"], "title").to_string() };
    let description = if created { None } else { result["toast"]["description"].as_str().map(str::to_string) };
    s.ui.chat.details.git.success = Some(Success { cwd: r.cwd, title, description, until });
}

/// `syncThreadBranchAfterGitAction`: a feature branch the action made becomes the thread's, or
/// the draft's, so a new worktree then starts from the commit.
pub(super) fn sync_branch(s: &mut App, result: &Value) {
    let Some(branch) = result["branch"]["name"].as_str().filter(|_| result["branch"]["status"] == "created") else { return };
    let Some(thread) = composer::ctx(s).thread else {
        let run = super::draft_run_mut(s);
        (run.base, run.from_origin) = (Some(branch.to_string()), false);
        return;
    };
    if thread["branch"] == branch {
        return;
    }
    let (id, worktree) = (data::s(thread, "id").to_string(), thread["worktreePath"].clone());
    if let Some(net) = s.src.net() {
        net.dispatch(json!({"type": "thread.metadata.update", "threadId": id, "branch": branch, "worktreePath": worktree}));
    }
}

fn quick_icon(q: &Quick) -> &'static Icon {
    match q.kind {
        Kind::Publish => &icons::CLOUD_UPLOAD,
        Kind::Pull => &icons::CLOUD_DOWNLOAD,
        Kind::Run("commit") => &icons::GIT_COMMIT_HORIZONTAL,
        Kind::Run("push" | "commit_push") => &icons::CLOUD_UPLOAD,
        Kind::Run(_) => &icons::GIT_PULL_REQUEST_ARROW,
        Kind::Hint if q.label == "Commit" => &icons::GIT_COMMIT_HORIZONTAL,
        Kind::Hint if q.label == "Push" => &icons::CLOUD_UPLOAD,
        Kind::Hint => &icons::INFO,
    }
}

/// The source control mark for change request rows, like the web's `SourceControlIcon`.
fn source_icon(status: Option<&Value>, size: f32, color: Color) -> El {
    let kind = status.and_then(|v| v["sourceControlProvider"]["kind"].as_str()).unwrap_or("github");
    add_project::PROVIDERS
        .iter()
        .position(|p| p.kind == kind)
        .and_then(|i| add_project::brand(i, size, color))
        .unwrap_or_else(|| icon(&icons::GIT_PULL_REQUEST_ARROW, size).color(color))
}

/// A two-line status row: a leading glyph, a title, then a muted detail line when present.
fn status_lines(t: &T, glyph: El, title: &str, detail: Option<&str>) -> El {
    let mut lines = col().grow(1.0).shrink().min_w(0.0).child(row().h(24.0).center_y().child(text(title.to_string()).font(ROW).color(t.c.foreground.alpha_mul(0.8)).truncate()));
    if let Some(detail) = detail {
        lines = lines.child(text(detail.to_string()).font(theme::TEXT_2XS).color(t.c.muted_foreground).truncate().help(detail.to_string()).pb(2.0));
    }
    row().min_h(32.0).py(4.0).pl(10.0).pr(10.0).gap(12.0).items_start().grow(1.0).shrink().min_w(0.0).child(row().h(24.0).center_y().child(glyph)).child(lines)
}

/// The Git actions row (or "Initialize Git"), and whether a run or status shows on it.
pub fn view(s: &App, t: &T, local: &Value, draft: bool) -> Option<El> {
    let muted = t.c.muted_foreground;
    let fg = t.c.foreground.alpha_mul(0.8);
    if local["isRepo"] != true {
        let busy = s.ui.chat.details.git.init.is_some();
        let label = if busy { "Initializing..." } else { "Initialize Git" };
        let row = super::action(t, "details-git-init", icon(&icons::GIT_BRANCH_PLUS, 14.0).color(fg), label, fg).w_full().gap(12.0);
        return Some(if busy { row.opacity(0.64) } else { row.on_press(init) });
    }
    let status = status(s);
    let q = quick_action(s, status.as_ref());
    let git = &s.ui.chat.details.git;
    let here = cwd(s);
    let running = git.running.as_ref().filter(|r| Some(&r.cwd) == here.as_ref());
    let success = git.success.as_ref().filter(|v| Some(&v.cwd) == here.as_ref());
    if quiet(s, draft) && running.is_none() && q.disabled && changes_totals(local) == (0, 0) && status_error(s).is_none() {
        return None;
    }
    let group = row().key("details-git").w_full().radius(theme::layout::CONTROL_RADIUS);
    if let Some(r) = running {
        let status = r.label.as_deref().filter(|l| *l != "Running source control action").unwrap_or(if r.pull { "Pulling latest changes..." } else { "Starting source control action..." });
        let output = if r.pull { None } else { r.output.as_deref() };
        let started = if r.pull { r.phase_at } else { r.hook_at.unwrap_or(r.phase_at) };
        let spinner = kit::Spinner::new().size("sm").build(&kit::theme(t));
        return Some(
            group
                .fill(if t.dark { Color::WHITE.alpha(0.055) } else { Color::BLACK.alpha(0.035) })
                .role("status")
                .label(output.map_or_else(|| status.to_string(), |o| format!("{status} {o}")))
                .child(status_lines(t, spinner, status, output))
                .child(text(elapsed(started, s.now_ms.max(data::now_ms()))).font(theme::TEXT_2XS).color(muted).h(32.0).center_y().pr(10.0)),
        );
    }
    let main = match success {
        Some(v) => status_lines(t, icon(&icons::CHECK, 14.0).color(t.c.success), &v.title, v.description.as_deref()).role("status"),
        None => {
            let glyph = match q.kind {
                Kind::Run("create_pr" | "commit_push_pr") => source_icon(status.as_ref(), 16.0, muted),
                _ => icon(quick_icon(&q), 16.0).color(muted),
            };
            let main = super::action(t, "details-git-quick", glyph, &q.label, fg);
            match &q.hint {
                Some(hint) if q.disabled => main.opacity(0.64).help(hint.clone()),
                _ => main.on_press(|s: &mut App| run_quick(s)),
            }
        }
    };
    Some(super::split(t, "details-git-menu", main.grow(1.0).shrink().min_w(0.0), "Git action options", super::Menu::Git))
}

fn quiet(s: &App, draft: bool) -> bool {
    draft || composer::ctx(s).thread.is_some_and(|t| t["latestRunId"].is_null())
}

/// The Git menu's rows (`gitItems`).
pub fn menu_rows(s: &App, t: &T) -> Vec<El> {
    let status = status(s);
    let busy = is_busy(s);
    let primary = status.as_ref().is_some_and(|v| v["hasPrimaryRemote"] == true);
    let mut rows = vec![];
    for item in build_menu_items(status.as_ref(), busy, primary) {
        let glyph = match item.id {
            "commit" => icon(&icons::GIT_COMMIT_HORIZONTAL, 16.0).color(t.c.muted_foreground),
            "push" => icon(&icons::CLOUD_UPLOAD, 16.0).color(t.c.muted_foreground),
            _ => source_icon(status.as_ref(), 16.0, t.c.muted_foreground),
        };
        let reason = disabled_reason(&item, status.as_ref(), busy, primary);
        let id = item.id;
        rows.push(super::menu_row(t, Some(glyph), &item.label, (!item.disabled).then_some(move |s: &mut App| pick(s, id)), reason));
    }
    if status.as_ref().is_some_and(|v| v["isRepo"] == true && v["hasPrimaryRemote"] != true) {
        let glyph = icon(&icons::CLOUD_UPLOAD, 16.0).color(t.c.muted_foreground);
        rows.push(super::menu_row(t, Some(glyph), "Publish repository...", (!busy).then_some(|s: &mut App| pick(s, "publish")), None));
    }
    if let Some(v) = &status {
        let note = if v["refName"].is_null() {
            Some(format!("Detached HEAD: create and check out a branch to enable push and {} actions.", terms(status.as_ref()).singular))
        } else if v["hasWorkingTreeChanges"] != true && v["behindCount"].as_u64().unwrap_or(0) > 0 && v["aheadCount"].as_u64().unwrap_or(0) == 0 {
            Some("Behind upstream. Pull/rebase first.".into())
        } else {
            None
        };
        if let Some(note) = note {
            rows.push(text(note).font(theme::TEXT_XS).color(t.c.warning).wrap().px(8.0).py(6.0));
        }
    }
    if let Some(error) = status_error(s) {
        rows.push(text(error).font(theme::TEXT_XS).color(t.c.destructive).wrap().px(8.0).py(6.0));
    }
    rows
}

/// A commit dialog file row opens in the editor, against the worktree root (`openChangedFileInEditor`).
fn open_file(s: &mut App, path: &str) {
    let root = status(s).and_then(|v| v["workingTree"]["rootPath"].as_str().map(str::to_string));
    let (Some(root), Some((editor, _))) = (root, super::preferred_editor(s)) else {
        return palette::toast_error(s, "Editor opening is unavailable.", None);
    };
    let target = if path.starts_with('/') { path.to_string() } else { format!("{}/{path}", root.trim_end_matches('/')) };
    if let Some(net) = s.src.net() {
        net.request("shell.openInEditor", json!({"cwd": target, "editor": editor}));
    }
}

pub fn overlays(s: &App, t: &T, out: &mut Vec<El>) {
    commit_dialog(s, t, out);
    confirm_dialog(s, t, out);
    publish_dialog(s, t, out);
}

fn close_commit(s: &mut App) {
    s.ui.chat.details.git.commit = None;
}

fn submit_commit(s: &mut App, feature_branch: bool) {
    let Some(commit) = s.ui.chat.details.git.commit.take() else { return };
    let files = status(s).map(|v| files(&v)).unwrap_or_default();
    let selected: Vec<String> = files.iter().filter(|f| !commit.excluded.contains(&f.0)).map(|f| f.0.clone()).collect();
    let message = Some(commit.message.text.trim().to_string()).filter(|m| !m.is_empty());
    let paths = (selected.len() != files.len()).then_some(selected);
    run(s, "commit", message, paths, feature_branch, feature_branch);
}

fn files(status: &Value) -> Vec<(String, u64, u64)> {
    status["workingTree"]["files"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| (data::s(f, "path").to_string(), f["insertions"].as_u64().unwrap_or(0), f["deletions"].as_u64().unwrap_or(0)))
        .collect()
}

fn diff_count(t: &T, add: u64, del: u64) -> El {
    let mono = theme::mono(theme::TEXT_SM);
    row()
        .child(text(format!("+{}", super::thousands(add))).font(mono).color(t.c.diff_addition))
        .child(text(" / ").font(mono).color(t.c.muted_foreground))
        .child(text(format!("-{}", super::thousands(del))).font(mono).color(t.c.diff_deletion))
}

fn commit_dialog(s: &App, t: &T, out: &mut Vec<El>) {
    let Some(commit) = &s.ui.chat.details.git.commit else { return };
    let k = kit::theme(t);
    let status = status(s);
    let all = status.as_ref().map(files).unwrap_or_default();
    let selected: Vec<_> = all.iter().filter(|f| !commit.excluded.contains(&f.0)).collect();
    let (all_selected, none_selected) = (selected.len() == all.len(), selected.is_empty());
    let muted = t.c.muted_foreground;
    let is_default = status.as_ref().is_some_and(|v| v["isDefaultRef"] == true);
    let mut branch = row().w_full().gap(8.0).center_y().child(text("Branch").font(theme::TEXT_SM).color(muted)).child(
        text(status.as_ref().and_then(|v| v["refName"].as_str()).unwrap_or("(detached HEAD)").to_string()).font(theme::SM_MEDIUM).color(t.c.foreground).grow(1.0).truncate(),
    );
    if is_default {
        branch = branch.child(text("Default branch").font(theme::TEXT_SM).color(t.c.warning));
    }
    let mut heading = row().gap(8.0).center_y();
    if commit.editing && !all.is_empty() {
        heading = heading.child(
            kit::Checkbox::new(all_selected)
                .indeterminate(!all_selected && !none_selected)
                .hide_label()
                .label("Select all files")
                .on_toggle(move |s: &mut App| {
                    let paths: BTreeSet<String> = status_files(s);
                    if let Some(c) = s.ui.chat.details.git.commit.as_mut() {
                        c.excluded = if c.excluded.is_empty() { paths } else { BTreeSet::new() };
                    }
                })
                .build(&k),
        );
    }
    heading = heading.child(text("Files").font(theme::TEXT_SM).color(muted));
    if !all_selected && !commit.editing {
        heading = heading.child(text(format!("({} of {})", selected.len(), all.len())).font(theme::TEXT_SM).color(muted));
    }
    let mut files_head = row().w_full().center_y().child(heading).child(el().grow(1.0));
    if !all.is_empty() {
        files_head = files_head.child(
            kit::Button::new(if commit.editing { "Done" } else { "Edit" })
                .variant("ghost")
                .size("xs")
                .on_press(|s: &mut App| {
                    if let Some(c) = s.ui.chat.details.git.commit.as_mut() {
                        c.editing = !c.editing;
                    }
                })
                .build(t),
        );
    }
    let mut section = col().w_full().gap(4.0).child(files_head);
    if all.is_empty() {
        section = section.child(text("none").font(theme::SM_MEDIUM).color(t.c.foreground));
    } else {
        let mut list = col().w_full().gap(4.0).padding(4.0);
        for (path, add, del) in &all {
            let excluded = commit.excluded.contains(path);
            let mut line = row().w_full().gap(8.0).center_y().px(8.0).py(4.0).radius(radius::MD).hover(t.c.accent.alpha_mul(0.5));
            if commit.editing {
                let owned = path.clone();
                line = line.child(
                    kit::Checkbox::new(!excluded)
                        .hide_label()
                        .label(path.clone())
                        .on_toggle(move |s: &mut App| {
                            if let Some(c) = s.ui.chat.details.git.commit.as_mut()
                                && !c.excluded.remove(&owned)
                            {
                                c.excluded.insert(owned.clone());
                            }
                        })
                        .build(&k),
                );
            }
            let target = path.clone();
            let name = text(path.clone())
                .font(theme::mono(theme::TEXT_SM))
                .color(if excluded { muted } else { t.c.foreground })
                .truncate_head()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .cursor(Cursor::Pointer)
                .role("button")
                .label(format!("Open {path} in editor"))
                .on_press(move |s: &mut App| open_file(s, &target));
            line = line.child(name);
            line = line.child(if excluded { text("Excluded").font(theme::mono(theme::TEXT_SM)).color(muted) } else { diff_count(t, *add, *del) });
            list = list.child(line);
        }
        let (add, del) = selected.iter().fold((0, 0), |acc, f| (acc.0 + f.1, acc.1 + f.2));
        section = section
            .child(el().w_full().h(176.0).key("git-commit-files").radius(radius::LG).fill(if t.dark { Color::WHITE.alpha(0.025) } else { t.c.card }).border(1.0, if t.dark { Color::WHITE.alpha(0.05) } else { Color::BLACK.alpha(0.05) }).scroll(ScrollSpec { stick_bottom: false }).child(list))
            .child(row().w_full().justify_end().child(diff_count(t, add, del)));
    }
    let summary = col()
        .w_full()
        .gap(12.0)
        .padding(12.0)
        .radius(radius::XL)
        .fill(if t.dark { Color::WHITE.alpha(0.035) } else { t.c.muted.alpha_mul(0.5) })
        .border(1.0, if t.dark { Color::WHITE.alpha(0.05) } else { Color::BLACK.alpha(0.05) })
        .child(branch)
        .child(section);
    let message = col().w_full().gap(4.0).child(text("Commit message (optional)").font(theme::SM_MEDIUM).color(t.c.foreground)).child(
        kit::TextArea::new(&commit.message)
            .placeholder("Leave empty to auto-generate")
            .rows(3)
            .on_edit(|s: &mut App| &mut s.ui.chat.details.git.commit.get_or_insert_with(Commit::default).message)
            .build(&k)
            .w_full(),
    );
    let footer = row()
        .w_full()
        .gap(8.0)
        .justify_end()
        .child(kit::Button::new("Cancel").variant("outline").size("sm").on_press(close_commit).build(t))
        .child(kit::Button::new("Commit on new branch").variant("outline").size("sm").disabled(none_selected).on_press(|s: &mut App| submit_commit(s, true)).build(t))
        .child(kit::Button::new("Commit").size("sm").disabled(none_selected).on_press(|s: &mut App| submit_commit(s, false)).build(t));
    kit::Dialog::new("git-commit", "Commit changes")
        .open(true)
        .description("Review and confirm your commit. Leave the message blank to auto-generate one.")
        .child(col().w_full().gap(16.0).child(summary).child(message))
        .child(footer)
        .on_close(close_commit)
        .build(t, out);
}

fn status_files(s: &App) -> BTreeSet<String> {
    status(s).map(|v| files(&v).into_iter().map(|f| f.0).collect()).unwrap_or_default()
}

fn confirm_dialog(s: &App, t: &T, out: &mut Vec<El>) {
    let Some(c) = &s.ui.chat.details.git.confirm else { return };
    let (title, description, continue_label) = default_branch_copy(c.action, &c.branch, c.includes_commit, &terms(status(s).as_ref()));
    let changes = status(s).is_none_or(|v| v["hasWorkingTreeChanges"] == true);
    let unavailable = (!changes).then(|| {
        format!("There are no changes to commit, so a feature branch can't be named. Create a branch yourself or continue on {}.", c.branch)
    });
    let resume = |feature: bool| {
        move |s: &mut App| {
            if let Some(c) = s.ui.chat.details.git.confirm.take() {
                run(s, c.action, c.message, c.files, feature, true);
            }
        }
    };
    let mut body = col().w_full().gap(16.0);
    if let Some(reason) = &unavailable {
        body = body.child(text(reason.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap());
    }
    let footer = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(kit::Button::new("Abort").variant("outline").size("sm").on_press(|s: &mut App| s.ui.chat.details.git.confirm = None).build(t))
        .child(el().grow(1.0))
        .child(kit::Button::new(continue_label).variant("outline").size("sm-multiline").on_press(resume(false)).build(t))
        .child(kit::Button::new("Check out feature branch & continue").size("sm-multiline").disabled(unavailable.is_some()).on_press(resume(true)).build(t));
    kit::Dialog::new("git-default-branch", title)
        .open(true)
        .width(576.0)
        .description(description)
        .child(body.child(footer))
        .on_close(|s: &mut App| s.ui.chat.details.git.confirm = None)
        .build(t, out);
}

/// `getPublishProviderReadiness`.
fn readiness(discovery: Option<&Value>, kind: &str) -> (bool, Option<String>) {
    let entry = discovery.and_then(|d| d["sourceControlProviders"].as_array()).and_then(|v| v.iter().find(|p| p["kind"] == kind));
    let Some(entry) = entry else { return (false, Some("Provider status unavailable. Open Settings -> Source control and rescan.".into())) };
    if entry["status"] != "available" {
        return (false, Some(data::s(entry, "installHint").to_string()));
    }
    if entry["auth"]["status"] == "unauthenticated" {
        let detail = option(&entry["auth"]["detail"]);
        return (false, Some(detail.unwrap_or_else(|| format!("{} is not authenticated. Open Settings -> Source control for setup guidance.", data::s(entry, "label")))));
    }
    (true, None)
}

/// An Effect `Option` as it crosses the wire.
fn option(v: &Value) -> Option<String> {
    v["value"].as_str().or(v.as_str()).map(str::to_string)
}

fn auth(discovery: Option<&Value>, kind: &str, field: &str) -> Option<String> {
    discovery.and_then(|d| d["sourceControlProviders"].as_array())?.iter().find(|p| p["kind"] == kind).and_then(|p| option(&p["auth"][field]))
}

/// The provider cards in the web's order: ready first, then by label.
fn provider_order(discovery: Option<&Value>) -> Vec<usize> {
    let mut order: Vec<usize> = (0..add_project::PROVIDERS.len()).collect();
    order.sort_by_key(|i| (!readiness(discovery, add_project::PROVIDERS[*i].kind).0, add_project::PROVIDERS[*i].label));
    order
}

/// The chosen provider when ready, else the first ready one, else the choice or GitHub.
fn publish_provider(p: &Publish, discovery: Option<&Value>) -> usize {
    let ready = |i: usize| readiness(discovery, add_project::PROVIDERS[i].kind).0;
    p.provider.filter(|i| ready(*i)).or_else(|| provider_order(discovery).into_iter().find(|i| ready(*i))).or(p.provider).unwrap_or(0)
}

fn publish_host(kind: &str, signed_in: Option<String>) -> String {
    match kind {
        "github" => "github.com".into(),
        "gitlab" => "gitlab.com".into(),
        "gitcafe" => "git.cafe".into(),
        "bitbucket" => "bitbucket.org".into(),
        "azure-devops" => "dev.azure.com".into(),
        _ => signed_in.unwrap_or_else(|| "your server".into()),
    }
}

fn publish_mut(s: &mut App) -> Option<&mut Publish> {
    s.ui.chat.details.git.publish.as_mut()
}

fn publish_repository(s: &App, p: &Publish) -> String {
    let discovery = s.ui.chat.details.git.discovery.as_ref();
    match &p.repository {
        Some(edit) => edit.text.clone(),
        None => auth(discovery, add_project::PROVIDERS[publish_provider(p, discovery)].kind, "account").map(|a| format!("{a}/")).unwrap_or_default(),
    }
}

fn can_publish(s: &App, p: &Publish) -> bool {
    let discovery = s.ui.chat.details.git.discovery.as_ref();
    let repository = publish_repository(s, p);
    let mut parts = repository.trim().splitn(2, '/');
    let owner = parts.next().unwrap_or("").trim();
    let name = parts.next().unwrap_or("").trim();
    readiness(discovery, add_project::PROVIDERS[publish_provider(p, discovery)].kind).0 && p.pending.is_none() && !owner.is_empty() && !name.is_empty()
}

fn submit_publish(s: &mut App) {
    let Some(p) = s.ui.chat.details.git.publish.as_ref() else { return };
    if !can_publish(s, p) {
        return;
    }
    let provider = add_project::PROVIDERS[publish_provider(p, s.ui.chat.details.git.discovery.as_ref())].kind;
    let remote = Some(p.remote.text.trim()).filter(|r| !r.is_empty()).unwrap_or("origin").to_string();
    let payload = json!({
        "cwd": cwd(s),
        "provider": provider,
        "repository": publish_repository(s, p).trim(),
        "visibility": if p.public { "public" } else { "private" },
        "remoteName": remote,
        "protocol": if p.https { "https" } else { "ssh" },
    });
    let id = s.src.net().map(|net| net.request("sourceControl.publishRepository", payload));
    if let Some(p) = publish_mut(s) {
        p.error = None;
        p.pending = id;
    }
}

fn open_settings(s: &mut App) {
    s.ui.chat.details.git.publish = None;
    crate::shell::sidebar::go(s, crate::Route::Settings("source-control".into()));
}

fn card(t: &T, selected: bool) -> El {
    let fill = if selected && t.dark { t.c.primary.alpha_mul(0.1) } else if t.dark { Color::WHITE.alpha(0.035) } else { t.c.background };
    let border = if selected { t.c.primary } else if t.dark { Color::TRANSPARENT } else { t.c.border };
    row().gap(12.0).px(12.0).py(12.0).center_y().radius(radius::LG).fill(fill).border(if selected { 2.0 } else { 1.0 }, border).w(0.0).grow(1.0).min_w(0.0)
}

fn publish_dialog(s: &App, t: &T, out: &mut Vec<El>) {
    let Some(p) = &s.ui.chat.details.git.publish else { return };
    let k = kit::theme(t);
    let discovery = s.ui.chat.details.git.discovery.as_ref();
    let provider = publish_provider(p, discovery);
    let info = &add_project::PROVIDERS[provider];
    let label = |text_: String| text(text_).font(theme::XS_MEDIUM).color(t.c.foreground);
    let steps = ["Provider", "Repository", "Summary"];
    let summaries = [Some(info.label.to_string()), p.result.as_ref().map(|r| data::s(&r["repository"], "nameWithOwner").to_string()), None];
    let mut header = row().w_full().gap(8.0).center_y();
    for (i, step) in steps.iter().enumerate() {
        let current = i == p.step;
        let enabled = p.step != 2 && i < steps.len() - 1 && i <= p.step && !current;
        let mut chip = col().gap(2.0).w(0.0).grow(1.0).min_w(0.0).child(
            row().gap(6.0).center_y()
                .child(el().size(18.0, 18.0).radius(9.0).center().fill(if i <= p.step { t.c.primary } else { t.c.muted }).child(text(format!("{}", i + 1)).font(theme::TEXT_2XS).color(if i <= p.step { t.c.primary_foreground } else { t.c.muted_foreground })))
                .child(text(step.to_string()).font(theme::XS_MEDIUM).color(if current { t.c.foreground } else { t.c.muted_foreground })),
        );
        if let Some(summary) = summaries[i].as_ref().filter(|_| i < p.step) {
            chip = chip.child(text(summary.clone()).font(theme::TEXT_2XS).color(t.c.muted_foreground).truncate());
        }
        if enabled {
            chip = chip.cursor(Cursor::Pointer).on_press(move |s: &mut App| {
                if let Some(p) = publish_mut(s) {
                    p.step = i;
                }
            });
        }
        header = header.child(chip);
    }
    let mut body = col().w_full().gap(20.0);
    match p.step {
        0 => {
            let mut grid = col().w_full().gap(8.0);
            let order = provider_order(discovery);
            for pair in order.chunks(2) {
                let mut line = row().w_full().gap(8.0);
                for &i in pair {
                    let option = &add_project::PROVIDERS[i];
                    let (ready, hint) = readiness(discovery, option.kind);
                    let mark = add_project::brand(i, 20.0, t.c.foreground).unwrap_or_else(|| icon(&icons::GIT_PULL_REQUEST_ARROW, 20.0).color(t.c.muted_foreground));
                    let name = text(option.label).font(theme::SM_MEDIUM).color(t.c.foreground).truncate().grow(1.0).shrink().min_w(0.0);
                    let card = if ready {
                        card(t, provider == i).cursor(Cursor::Pointer).role("radio").label(option.label).child(mark).child(name).on_press(move |s: &mut App| {
                            if let Some(p) = publish_mut(s) {
                                (p.provider, p.repository) = (Some(i), None);
                            }
                        })
                    } else {
                        let setup = kit::Button::new("Setup Required").variant("warning-outline").size("micro").on_press(open_settings).build(t).help(hint.unwrap_or_else(|| "Open Settings -> Source control to configure this provider.".into()));
                        card(t, false).opacity(0.64).child(mark).child(name).child(setup)
                    };
                    line = line.child(card);
                }
                grid = grid.child(line);
            }
            body = body.child(col().w_full().gap(8.0).child(label("Provider".into())).child(grid));
        }
        1 => {
            let host = publish_host(info.kind, auth(discovery, info.kind, "host"));
            let edit = p.repository.clone().unwrap_or_default();
            let input = kit::Input::new(&edit)
                .placeholder(info.hint)
                .font("mono")
                .disabled(p.pending.is_some())
                .on_edit(|s: &mut App| publish_mut(s).expect("publish dialog is open").repository.get_or_insert_with(TextEdit::default))
                .on_submit(|s: &mut App, _| submit_publish(s))
                .build(t)
                .single_line()
                .grow(1.0)
                .shrink()
                .min_w(0.0);
            let prefix = row()
                .gap(6.0)
                .px(10.0)
                .center_y()
                .fill(t.c.muted.alpha_mul(0.5))
                .child(add_project::brand(provider, 14.0, t.c.muted_foreground).unwrap_or_else(|| el()))
                .child(text(format!("{host}/")).font(theme::MONO_XS).color(t.c.muted_foreground));
            let field = row().w_full().radius(radius::MD).border(1.0, t.c.input).clip_rounded(radius::MD).child(prefix).child(input);
            body = body.child(col().w_full().gap(8.0).child(label("Repository".into())).child(field));
            let mut visibility = row().w_full().gap(8.0);
            for (public, name, detail, glyph) in [(false, "Private", "Only invited people", &icons::LOCK), (true, "Public", "Anyone on the web", &icons::GLOBE)] {
                visibility = visibility.child(
                    card(t, p.public == public)
                        .py(10.0)
                        .cursor(Cursor::Pointer)
                        .role("radio")
                        .label(name)
                        .child(icon(glyph, 16.0).color(t.c.muted_foreground))
                        .child(col().child(text(name).font(theme::SM_MEDIUM).color(t.c.foreground)).child(text(detail).font(theme::TEXT_XS).color(t.c.muted_foreground)))
                        .on_press(move |s: &mut App| {
                            if let Some(p) = publish_mut(s).filter(|p| p.pending.is_none()) {
                                p.public = public;
                            }
                        }),
                );
            }
            body = body.child(col().w_full().gap(8.0).child(label("Visibility".into())).child(visibility));
            let mut advanced = col().w_full().child(
                row()
                    .gap(4.0)
                    .center_y()
                    .cursor(Cursor::Pointer)
                    .child(icon(if p.advanced { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 14.0).color(t.c.muted_foreground))
                    .child(text("Advanced").font(theme::XS_MEDIUM).color(t.c.muted_foreground))
                    .on_press(|s: &mut App| {
                        if let Some(p) = publish_mut(s) {
                            p.advanced = !p.advanced;
                        }
                    }),
            );
            if p.advanced {
                let remote = col().gap(6.0).w(0.0).grow(1.0).min_w(0.0).child(label("Remote".into())).child(
                    kit::Input::new(&p.remote).placeholder("origin").disabled(p.pending.is_some()).on_edit(|s: &mut App| &mut publish_mut(s).expect("publish dialog is open").remote).build(t).single_line(),
                );
                let protocol = col().gap(6.0).w(0.0).grow(1.0).min_w(0.0).child(label("Protocol".into())).child(
                    kit::ToggleGroup::new(&["SSH", "HTTPS"], &[usize::from(p.https)])
                        .on_pick(|s: &mut App, index, _| {
                            if let Some(p) = publish_mut(s).filter(|p| p.pending.is_none()) {
                                p.https = index == 1;
                            }
                        })
                        .build(&k),
                );
                advanced = advanced.child(row().w_full().gap(12.0).mt(12.0).child(remote).child(protocol));
            }
            body = body.child(advanced);
            if p.pending.is_some() {
                body = body.child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .center_y()
                        .px(12.0)
                        .py(8.0)
                        .radius(radius::MD)
                        .fill(if t.dark { Color::WHITE.alpha(0.035) } else { t.c.muted.alpha_mul(0.4) })
                        .role("status")
                        .child(kit::Spinner::new().size("sm").build(&k))
                        .child(text(format!("Publishing repository to {}...", info.label)).font(theme::TEXT_XS).color(t.c.muted_foreground)),
                );
            } else if let Some(error) = &p.error {
                body = body.child(
                    col()
                        .w_full()
                        .gap(2.0)
                        .px(12.0)
                        .py(8.0)
                        .radius(radius::MD)
                        .fill(t.c.destructive.alpha_mul(0.1))
                        .border(1.0, t.c.destructive.alpha_mul(0.4))
                        .role("alert")
                        .child(text("Publish failed").font(theme::XS_MEDIUM).color(t.c.destructive))
                        .child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive.alpha_mul(0.9)).wrap()),
                );
            }
        }
        _ => {
            if let Some(r) = &p.result {
                let pushed = r["status"] == "pushed";
                let url = data::s(&r["repository"], "url").to_string();
                body = body
                    .child(
                        col()
                            .w_full()
                            .gap(8.0)
                            .items_center()
                            .child(el().size(32.0, 32.0).radius(16.0).center().fill(t.c.success.alpha_mul(0.15)).child(icon(&icons::CHECK, 16.0).color(t.c.success)))
                            .child(text(if pushed { "Repository published" } else { "Repository created" }).font(theme::weight(theme::TEXT_SM, theme::SEMIBOLD)).color(t.c.foreground))
                            .child(
                                text(if pushed {
                                    format!("{} is now live on {}.", data::s(r, "branch"), info.label)
                                } else {
                                    format!("Remote \"{}\" is set up. Make a commit and push it to share your code.", data::s(r, "remoteName"))
                                })
                                .font(theme::TEXT_XS)
                                .color(t.c.muted_foreground)
                                .wrap()
                                .max_w(320.0),
                            ),
                    )
                    .child(
                        row()
                            .w_full()
                            .gap(8.0)
                            .center_y()
                            .px(12.0)
                            .py(8.0)
                            .radius(radius::LG)
                            .fill(if t.dark { Color::WHITE.alpha(0.035) } else { t.c.muted.alpha_mul(0.4) })
                            .child(add_project::brand(provider, 14.0, t.c.muted_foreground).unwrap_or_else(|| el()))
                            .child(text(data::s(&r["repository"], "nameWithOwner").to_string()).font(theme::MONO_XS).color(t.c.foreground).truncate()),
                    )
                    .child(kit::Button::new(format!("Open on {}", info.label)).variant("outline").size("sm").on_press(move |s: &mut App| crate::settings::auth::open_url(s, &url)).build(t).w_full());
            } else {
                body = body.child(text("Publish result unavailable.").font(theme::TEXT_XS).color(t.c.muted_foreground));
            }
        }
    }
    let close = |s: &mut App| s.ui.chat.details.git.publish = None;
    let pending = p.pending.is_some();
    let mut footer = row().w_full().gap(8.0).justify_end();
    if p.step == 2 {
        footer = footer.child(kit::Button::new("Done").on_press(close).build(t));
    } else {
        footer = footer.child(
            kit::Button::new(if p.step == 0 { "Cancel" } else { "Back" })
                .variant("outline")
                .disabled(pending)
                .on_press(|s: &mut App| {
                    let Some(p) = publish_mut(s) else { return };
                    if p.step > 0 {
                        p.step -= 1;
                    } else {
                        s.ui.chat.details.git.publish = None;
                    }
                })
                .build(t),
        );
        footer = footer.child(if p.step == 0 {
            kit::Button::new("Next")
                .disabled(!readiness(discovery, info.kind).0)
                .on_press(|s: &mut App| {
                    // The field starts as `{account}/` for the chosen provider (`publishRepositoryPrefill`).
                    let prefill = s.ui.chat.details.git.publish.as_ref().map(|p| publish_repository(s, p)).unwrap_or_default();
                    if let Some(p) = publish_mut(s) {
                        let mut edit = TextEdit::new(prefill);
                        edit.focused = true;
                        p.repository.get_or_insert(edit);
                        p.step = 1;
                    }
                })
                .build(t)
        } else {
            kit::Button::new(if pending { "Publishing..." } else { "Publish" }).disabled(!can_publish(s, p)).on_press(submit_publish).build(t)
        });
    }
    kit::Dialog::new("git-publish", "Publish repository")
        .open(true)
        .description("Pick where to host it, then point us at a repo to push to.")
        .child(header)
        .child(body)
        .child(footer)
        .on_close(close)
        .build(t, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(fields: Value) -> Value {
        let mut v = json!({"isRepo": true, "hasPrimaryRemote": true, "isDefaultRef": false, "refName": "feature", "hasWorkingTreeChanges": false, "hasUpstream": true, "aheadCount": 0, "behindCount": 0, "pr": null});
        for (key, value) in fields.as_object().unwrap() {
            v[key] = value.clone();
        }
        v
    }

    #[test]
    fn changes_totals_prefer_branch_work_then_the_tree() {
        let local = |branch: Value| json!({"workingTree": {"insertions": 3, "deletions": 1}, "branchChanges": branch});
        assert_eq!(changes_totals(&local(json!({"insertions": 9, "deletions": 0}))), (9, 0));
        assert_eq!(changes_totals(&local(json!({"insertions": 0, "deletions": 0}))), (3, 1), "a zero branch total doesn't hide a dirty tree");
        assert_eq!(changes_totals(&local(Value::Null)), (3, 1));
    }

    #[test]
    fn quick_action_follows_web_precedence() {
        let q = |fields: Value, default: bool, remote: bool| resolve_quick_action(Some(&status(fields)), false, default, remote);
        assert_eq!(resolve_quick_action(None, true, false, true).hint.as_deref(), Some("Git action in progress."));
        assert_eq!(q(json!({"hasWorkingTreeChanges": true}), false, true).kind, Kind::Run("commit_push_pr"));
        assert_eq!(q(json!({"hasWorkingTreeChanges": true}), false, true).label, "Commit, push & PR");
        assert_eq!(q(json!({"hasWorkingTreeChanges": true}), true, true).kind, Kind::Run("commit_push"));
        // No remote at all: commits stay local, and a clean tree offers publishing.
        assert_eq!(q(json!({"hasWorkingTreeChanges": true, "hasUpstream": false}), false, false).kind, Kind::Run("commit"));
        assert_eq!(q(json!({"hasUpstream": false}), false, false).kind, Kind::Publish);
        assert_eq!(q(json!({"hasUpstream": false}), false, true).hint.as_deref(), Some("No local commits to push."));
        assert_eq!(q(json!({"hasUpstream": false, "aheadCount": 2}), true, true).kind, Kind::Run("commit_push"));
        assert_eq!(q(json!({"aheadCount": 1, "behindCount": 1}), false, true).label, "Sync ref");
        assert_eq!(q(json!({"behindCount": 1}), false, true).kind, Kind::Pull);
        assert_eq!(q(json!({"aheadCount": 1, "sourceControlProvider": {"kind": "gitlab"}}), false, true).label, "Push & create MR");
        assert_eq!(q(json!({"aheadOfDefaultCount": 3}), false, true).label, "Create PR");
        assert_eq!(q(json!({"refName": null}), false, true).hint.as_deref(), Some("Create and checkout a ref before pushing or opening a pull request."));
        assert!(q(json!({}), false, true).disabled);
    }

    #[test]
    fn menu_items_and_reasons_match_web() {
        let v = status(json!({"hasWorkingTreeChanges": true}));
        let items = build_menu_items(Some(&v), false, true);
        assert_eq!(items.iter().map(|i| (i.id, i.disabled)).collect::<Vec<_>>(), vec![("commit", false), ("push", true), ("pr", true)]);
        assert_eq!(disabled_reason(&items[1], Some(&v), false, true).as_deref(), Some("Commit or stash local changes before pushing."));
        assert_eq!(disabled_reason(&items[2], Some(&v), false, true).as_deref(), Some("No local commits to include in a pull request."));
        let clean = status(json!({}));
        let items = build_menu_items(Some(&clean), false, false);
        assert_eq!(items.len(), 1);
        assert_eq!(disabled_reason(&items[0], Some(&clean), false, false).as_deref(), Some("Worktree is clean. Make changes before committing."));
        let open_pr = status(json!({"aheadCount": 1, "pr": {"state": "open"}}));
        assert_eq!(build_menu_items(Some(&open_pr), false, true).iter().map(|i| (i.id, i.disabled)).collect::<Vec<_>>(), vec![("commit", true), ("push", false)]);
    }

    #[test]
    fn default_branch_copy_and_progress() {
        let t = terms(None);
        assert_eq!(default_branch_copy("commit_push", "main", true, &t).2, "Commit & push to main");
        assert_eq!(default_branch_copy("create_pr", "main", false, &t).0, "Push & create PR from default ref?");
        assert_eq!(elapsed(0, 59_900), "59s");
        assert_eq!(elapsed(0, 61_000), "1m 1s");
        let mut r = Running { id: 1, cwd: String::new(), pull: false, label: None, phase_label: None, output: None, phase_at: 0, hook_at: None, result: None, failure: None };
        apply(&mut r, &json!({"kind": "phase_started", "label": "Committing..."}), 10);
        apply(&mut r, &json!({"kind": "hook_started", "hookName": "pre-commit"}), 20);
        apply(&mut r, &json!({"kind": "hook_output", "text": "lint ok"}), 30);
        assert_eq!((r.label.as_deref(), r.output.as_deref(), r.hook_at), (Some("Running pre-commit..."), Some("lint ok"), Some(20)));
        apply(&mut r, &json!({"kind": "hook_finished"}), 40);
        assert_eq!((r.label.as_deref(), r.output, r.phase_at), (Some("Committing..."), None, 10));
    }
}
