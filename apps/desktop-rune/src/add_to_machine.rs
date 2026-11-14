//! AddProjectToMachineDialog with lib/addProjectToMachine.ts: clones a project onto another
//! connected environment, then relays the chosen git-ignored files from the source. The client
//! carries the files because the two servers never talk to each other.
//!
//! Also the composer's clone banner (useProjectCloneBanner): any project still cloning or
//! copying files into place, with Cancel, Retry and Remove project, and why Send waits.

use std::collections::{BTreeSet, HashMap};

use rune::{El, TextEdit, col, el, icon, row, text};
use serde_json::{Value, json};

use crate::shell::palette::add_project::{clone_destination, clone_directory_name, ensure_directory, iso_now};
use crate::theme::{self, T};
use crate::{App, Route, data, env, icons, kit};

/// Matches the server's read cap: a larger file would arrive truncated.
const COPYABLE_MAX: u64 = 1024 * 1024;
const CLONES: &str = "subscribeProjectClones";

#[derive(Default)]
pub struct Ui {
    dialog: Option<Dialog>,
    jobs: Vec<Job>,
    /// Environments whose clone list is subscribed: the composer project's and every job's.
    watched: BTreeSet<String>,
    /// Copies that failed, by project id, until the banner is dismissed: title and failures.
    copy_failed: HashMap<String, (String, Vec<String>)>,
    /// Banner actions in flight: the reply, its failure toast title, and a removed project.
    actions: Vec<(u64, &'static str, Option<String>)>,
}

struct Dialog {
    project: String,
    source: String,
    target: String,
    picking: bool,
    protocol: Option<Protocol>,
    path: TextEdit,
    listing: Option<u64>,
    files: Option<Result<Listing, String>>,
    /// None until the user changes it: the env files are picked by default.
    selected: Option<BTreeSet<String>>,
    submit: Option<Submit>,
    error: Option<String>,
}

struct Listing {
    files: Vec<(String, u64)>,
    truncated: bool,
}

struct Submit {
    reads: Vec<(String, u64)>,
    read: Vec<(String, Result<String, String>)>,
    start: Option<(u64, String)>,
}

/// Ignored-file copies still owed to a cloned project.
struct Job {
    target: String,
    label: String,
    project_id: String,
    cwd: String,
    subdirectory: String,
    read: Vec<(String, Result<String, String>)>,
    seen: bool,
    writes: Option<Vec<(String, u64)>>,
    failures: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Protocol {
    Ssh,
    Https,
}

// Pure helpers, as the web computes them.

/// `isDefaultCopiedFile`: env files are the ignored files people almost always want elsewhere.
fn is_default_copied(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == ".envrc" || name == ".dev.vars" || name == ".env" || name.strip_prefix(".env.").is_some_and(|rest| !rest.is_empty())
}

fn default_selection(files: &[(String, u64)]) -> BTreeSet<String> {
    files.iter().filter(|(path, bytes)| *bytes <= COPYABLE_MAX && is_default_copied(path)).map(|(path, _)| path.clone()).collect()
}

/// `projectSubdirectory`: a project rooted below its repository's top keeps that path in the clone.
fn subdirectory(repository_root: Option<&str>, workspace_root: &str) -> String {
    let Some(root) = repository_root.filter(|r| !r.is_empty()) else { return String::new() };
    let prefix = root.trim_end_matches(['/', '\\']);
    match workspace_root.strip_prefix(prefix) {
        Some(rest) if rest.starts_with(['/', '\\']) => rest[1..].replace('\\', "/"),
        _ => String::new(),
    }
}

fn copy_path(subdirectory: &str, relative: &str) -> String {
    if subdirectory.is_empty() { relative.to_owned() } else { format!("{subdirectory}/{relative}") }
}

fn copy_failure_title(count: usize, label: &str) -> String {
    format!("Could not copy {count} {} to {label}", if count == 1 { "file" } else { "files" })
}

/// `parseCloneUrl` from shared/cloneUrl.ts: protocol, host, path and whether a port is set.
fn parse_clone_url(url: &str) -> Option<(Protocol, String, String, bool)> {
    let url = url.trim();
    let ws = |v: &str| v.chars().any(char::is_whitespace);
    let authority = |rest: &str| -> Option<(String, String, bool)> {
        let (auth, path) = rest.split_once('/')?;
        if path.is_empty() || ws(auth) {
            return None;
        }
        let host_port = auth.rsplit_once('@').map_or(auth, |(_, h)| h);
        let (host, port) = match host_port.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (host_port, None),
        };
        if host.is_empty() || port.is_some_and(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit())) {
            return None;
        }
        Some((host.to_owned(), path.to_owned(), port.is_some()))
    };
    if let Some(rest) = url.strip_prefix("ssh://") {
        return authority(rest).map(|(h, p, port)| (Protocol::Ssh, h, p, port));
    }
    if let Some(rest) = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")) {
        return authority(rest).map(|(h, p, port)| (Protocol::Https, h, p, port));
    }
    // scp-like `[user@]host:path`, where the path never starts with a slash.
    let scp = |rest: &str| -> Option<(String, String)> {
        let (host, path) = rest.split_once(':')?;
        let bad_host = host.is_empty() || host.contains('/') || ws(host);
        (!bad_host && !path.is_empty() && !path.starts_with('/')).then(|| (host.to_owned(), path.to_owned()))
    };
    let with_user = url.split_once('@').filter(|(user, _)| !user.is_empty() && !user.contains('/') && !ws(user)).and_then(|(_, rest)| scp(rest));
    with_user.or_else(|| scp(url)).map(|(h, p)| (Protocol::Ssh, h, p, false))
}

pub fn clone_url_protocol(url: &str) -> Option<Protocol> {
    parse_clone_url(url).map(|p| p.0)
}

/// `cloneUrlForProtocol`: the other machine may only have SSH keys or only an HTTPS helper.
pub fn clone_url_for_protocol(url: &str, protocol: Protocol) -> Option<String> {
    let (from, host, path, custom_port) = parse_clone_url(url)?;
    if from == protocol {
        return Some(url.trim().to_owned());
    }
    if custom_port {
        return None;
    }
    let parts = |v: &str, n: usize| -> Option<Vec<String>> {
        let parts: Vec<&str> = v.split('/').collect();
        (parts.len() == n && parts.iter().all(|p| !p.is_empty() && !p.contains(['?', '#']))).then(|| parts.into_iter().map(str::to_owned).collect())
    };
    // Azure's transports use different hosts and put the repository marker elsewhere.
    if from == Protocol::Ssh && host.eq_ignore_ascii_case("ssh.dev.azure.com") {
        return parts(path.strip_prefix("v3/")?, 3).map(|p| format!("https://dev.azure.com/{}/{}/_git/{}", p[0], p[1], p[2]));
    }
    if from == Protocol::Https && host.eq_ignore_ascii_case("dev.azure.com") {
        return parts(&path, 4).filter(|p| p[2] == "_git").map(|p| format!("git@ssh.dev.azure.com:v3/{}/{}/{}", p[0], p[1], p[3]));
    }
    Some(match protocol {
        Protocol::Ssh => format!("git@{host}:{path}"),
        Protocol::Https => format!("https://{host}/{path}"),
    })
}

#[derive(Debug, PartialEq)]
enum CloneState {
    Waiting { seen: bool },
    Done,
    Gone,
}

/// `waitForClone`: done, removed after it was listed, or still waiting (a failed clone can retry).
fn clone_state(clones: &Value, project_id: &str, seen: bool) -> CloneState {
    let clone = clones.as_array().and_then(|c| c.iter().find(|c| c["projectId"] == project_id));
    match clone {
        Some(c) if c["phase"] == "done" => CloneState::Done,
        Some(_) => CloneState::Waiting { seen: true },
        None if seen => CloneState::Gone,
        None => CloneState::Waiting { seen },
    }
}

// Reading the app.

fn project<'a>(s: &'a App, id: &str) -> Option<&'a Value> {
    s.state().projects.iter().find(|p| data::s(p, "id") == id)
}

fn config<'a>(s: &'a App, key: &str) -> Option<&'a Value> {
    env::source(s, key).and_then(|src| src.state().config.as_ref())
}

fn source_url(project: &Value) -> String {
    let identity = &project["repositoryIdentity"];
    identity["origin"]["remoteUrl"].as_str().or(identity["locator"]["remoteUrl"].as_str()).unwrap_or_default().to_owned()
}

fn canonical(project: &Value) -> Option<String> {
    let identity = &project["repositoryIdentity"];
    identity["origin"]["canonicalKey"].as_str().or(identity["canonicalKey"].as_str()).map(str::to_lowercase)
}

/// `useAddProjectTargets`: connected environments that don't have this repository yet.
pub fn targets(s: &App, project_id: &str) -> Vec<(String, String)> {
    let Some(p) = project(s, project_id) else { return vec![] };
    if p["repositoryIdentity"]["locator"]["remoteUrl"].as_str().is_none_or(str::is_empty) {
        return vec![];
    }
    let key = canonical(p);
    let mut have: BTreeSet<String> = BTreeSet::from([env::owner(s, project_id)]);
    if key.is_some() {
        have.extend(s.state().projects.iter().filter(|o| canonical(o) == key).map(|o| env::owner(s, data::s(o, "id"))));
    }
    let mut out: Vec<(String, String)> = env::keys(s)
        .into_iter()
        .filter(|k| !have.contains(k) && env::connected(s, k) && config(s, k).is_some())
        .map(|k| {
            let label = env::label(s, &k);
            (k, label)
        })
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

fn default_path(s: &App, target: &str, url: &str) -> String {
    let base = config(s, target).and_then(|c| c["settings"]["addProjectBaseDirectory"].as_str()).unwrap_or("").trim();
    let initial = if base.is_empty() { "~/".to_owned() } else { ensure_directory(base) };
    clone_destination(&initial, &clone_directory_name(url))
}

fn take(s: &mut App, id: u64) -> Option<s5_net::Reply> {
    s.src.net().and_then(|n| n.take_reply(id))
}

fn message(e: &Value) -> String {
    crate::shell::palette::error_message(e, "An error occurred.")
}

/// Opens the dialog for `project_id`, on `target` when given and still a target.
pub fn open(s: &mut App, project_id: &str, target: Option<String>) {
    let targets = targets(s, project_id);
    let Some(target) = target.filter(|t| targets.iter().any(|(k, _)| k == t)).or_else(|| targets.first().map(|t| t.0.clone())) else { return };
    let Some(p) = project(s, project_id) else { return };
    let url = source_url(p);
    let cwd = data::s(p, "workspaceRoot").to_owned();
    let source = env::owner(s, project_id);
    let can_list = config(s, &source).is_some_and(|c| c["environment"]["capabilities"]["ignoredFileListing"] == true);
    let path = TextEdit::new(default_path(s, &target, &url));
    let listing = if can_list { env::net_of(s, &source).map(|n| n.request("projects.listIgnoredFiles", json!({"cwd": cwd}))) } else { None };
    s.ui.add_to_machine.dialog = Some(Dialog {
        project: project_id.to_owned(),
        source,
        target,
        picking: false,
        protocol: clone_url_protocol(&url),
        path,
        listing,
        files: None,
        selected: None,
        submit: None,
        error: None,
    });
}

fn close(s: &mut App) {
    s.ui.add_to_machine.dialog = None;
}

fn pick_target(s: &mut App, key: String) {
    let Some(d) = &s.ui.add_to_machine.dialog else { return };
    let url = project(s, &d.project).map(source_url).unwrap_or_default();
    let (old, new) = (default_path(s, &d.target, &url), default_path(s, &key, &url));
    let Some(d) = &mut s.ui.add_to_machine.dialog else { return };
    if d.path.text == old {
        d.path = TextEdit::new(new);
    }
    d.target = key;
    d.picking = false;
}

fn selected(d: &Dialog) -> BTreeSet<String> {
    match (&d.selected, &d.files) {
        (Some(set), _) => set.clone(),
        (None, Some(Ok(l))) => default_selection(&l.files),
        (None, _) => BTreeSet::new(),
    }
}

fn toggle(s: &mut App, path: &str) {
    let Some(d) = &mut s.ui.add_to_machine.dialog else { return };
    let mut set = selected(d);
    if !set.remove(path) {
        set.insert(path.to_owned());
    }
    d.selected = Some(set);
}

fn toggle_all(s: &mut App) {
    let Some(d) = &mut s.ui.add_to_machine.dialog else { return };
    let Some(Ok(l)) = &d.files else { return };
    let copyable: BTreeSet<String> = l.files.iter().filter(|f| f.1 <= COPYABLE_MAX).map(|f| f.0.clone()).collect();
    d.selected = Some(if selected(d).len() == copyable.len() { BTreeSet::new() } else { copyable });
}

/// Still loading: submitting before the list lands would skip the default env files.
fn unavailable(d: &Dialog) -> bool {
    d.listing.is_some() || d.submit.is_some()
}

fn submit(s: &mut App) {
    let Some(d) = &s.ui.add_to_machine.dialog else { return };
    if unavailable(d) {
        return;
    }
    if d.path.text.trim().is_empty() {
        if let Some(d) = &mut s.ui.add_to_machine.dialog {
            d.error = Some("Choose a folder on the other machine.".into());
        }
        return;
    }
    let files: Vec<String> = selected(d).into_iter().collect();
    let source = d.source.clone();
    let cwd = project(s, &d.project).map(|p| data::s(p, "workspaceRoot").to_owned()).unwrap_or_default();
    let mut reads = Vec::new();
    if let Some(net) = env::net_of(s, &source) {
        for path in files {
            let id = net.request("projects.readFile", json!({"cwd": cwd, "relativePath": path}));
            reads.push((path, id));
        }
    }
    if let Some(d) = &mut s.ui.add_to_machine.dialog {
        d.error = None;
        d.submit = Some(Submit { reads, read: Vec::new(), start: None });
    }
}

pub fn poll(s: &mut App) -> bool {
    poll_dialog(s) | poll_jobs(s)
}

fn poll_dialog(s: &mut App) -> bool {
    let Some(d) = &s.ui.add_to_machine.dialog else { return false };
    // The request ends with its project, so one that comes back cannot reopen it.
    if project(s, &d.project).is_none() {
        close(s);
        return true;
    }
    let mut changed = false;
    if let Some(id) = d.listing
        && let Some(reply) = take(s, id)
    {
        let files = reply
            .map(|v| Listing {
                files: v["files"].as_array().into_iter().flatten().map(|f| (data::s(f, "path").to_owned(), f["byteLength"].as_u64().unwrap_or(0))).collect(),
                truncated: v["truncated"] == true,
            })
            .map_err(|e| message(&e));
        let Some(d) = &mut s.ui.add_to_machine.dialog else { return true };
        d.listing = None;
        d.files = Some(files);
        changed = true;
    }
    let Some(sub) = s.ui.add_to_machine.dialog.as_ref().and_then(|d| d.submit.as_ref()) else { return changed };
    let pending: Vec<(String, u64)> = sub.reads.clone();
    let mut read = Vec::new();
    let mut waiting = Vec::new();
    for (path, id) in pending {
        match take(s, id) {
            Some(Ok(v)) if v["truncated"] == true => read.push((path, Err("Too large to copy.".to_owned()))),
            Some(Ok(v)) => read.push((path, Ok(data::s(&v, "contents").to_owned()))),
            Some(Err(e)) => read.push((path, Err(message(&e)))),
            None => waiting.push((path, id)),
        }
    }
    let Some(d) = s.ui.add_to_machine.dialog.as_mut() else { return changed };
    let Some(sub) = d.submit.as_mut() else { return changed };
    changed |= !read.is_empty();
    sub.reads = waiting;
    sub.read.extend(read);
    if !sub.reads.is_empty() {
        return changed;
    }
    match sub.start.clone() {
        None => start(s),
        Some((id, project_id)) => match take(s, id) {
            None => changed,
            Some(Err(e)) => {
                if let Some(d) = &mut s.ui.add_to_machine.dialog {
                    d.submit = None;
                    d.error = Some(message(&e));
                }
                true
            }
            Some(Ok(v)) => {
                started(s, project_id, data::s(&v, "cwd").to_owned());
                true
            }
        },
    }
}

/// Starts the clone on the target once every file is read, so the copy reflects the files as
/// they were when the user asked.
fn start(s: &mut App) -> bool {
    let Some(d) = &s.ui.add_to_machine.dialog else { return false };
    let Some(p) = project(s, &d.project) else { return false };
    let url = source_url(p);
    let remote_url = d.protocol.and_then(|proto| clone_url_for_protocol(&url, proto)).unwrap_or(url);
    let project_id = s5_net::uuid();
    let payload = json!({
        "projectId": project_id,
        "title": data::s(p, "title"),
        "createdAt": iso_now(),
        "remoteUrl": remote_url,
        "destinationPath": d.path.text.trim(),
    });
    let target = d.target.clone();
    let offline = format!("{} is not connected.", env::label(s, &target));
    let id = env::net_of(s, &target).map(|n| n.request("projectClone.start", payload));
    let Some(d) = &mut s.ui.add_to_machine.dialog else { return false };
    match (id, d.submit.as_mut()) {
        (Some(id), Some(sub)) => sub.start = Some((id, project_id)),
        _ => {
            d.submit = None;
            d.error = Some(offline);
        }
    }
    true
}

/// Lands on the new project right away; the copy follows once the clone finishes.
fn started(s: &mut App, project_id: String, cwd: String) {
    let Some(d) = s.ui.add_to_machine.dialog.take() else { return };
    let Some(p) = project(s, &d.project) else { return };
    let sub = d.submit.map(|sub| sub.read).unwrap_or_default();
    let subdirectory = subdirectory(p["repositoryIdentity"]["rootPath"].as_str(), data::s(p, "workspaceRoot"));
    if !sub.is_empty() {
        let label = env::label(s, &d.target);
        s.ui.add_to_machine.jobs.push(Job {
            target: d.target.clone(),
            label,
            project_id: project_id.clone(),
            cwd,
            subdirectory,
            read: sub,
            seen: false,
            writes: None,
            failures: Vec::new(),
        });
    }
    env::adopt(s, &project_id, &d.target);
    crate::shell::sidebar::go(s, Route::Home { project_id: Some(project_id) });
}

fn poll_jobs(s: &mut App) -> bool {
    sync_watch(s);
    let mut changed = poll_actions(s);
    if s.ui.add_to_machine.jobs.is_empty() {
        return changed;
    }
    let mut jobs = std::mem::take(&mut s.ui.add_to_machine.jobs);
    jobs.retain_mut(|job| {
        if let Some(writes) = &mut job.writes {
            writes.retain(|(path, id)| match take(s, *id) {
                None => true,
                Some(Ok(_)) => false,
                Some(Err(e)) => {
                    job.failures.push(format!("{path}: {}", message(&e)));
                    false
                }
            });
            if !writes.is_empty() {
                return true;
            }
            if !job.failures.is_empty() {
                let failed = (copy_failure_title(job.failures.len(), &job.label), std::mem::take(&mut job.failures));
                s.ui.add_to_machine.copy_failed.insert(job.project_id.clone(), failed);
            }
            changed = true;
            return false;
        }
        let clones = env::net_of(s, &job.target).and_then(|n| n.resource(CLONES, json!({}))).cloned().unwrap_or(Value::Null);
        match clone_state(&clones, &job.project_id, job.seen) {
            CloneState::Waiting { seen } => {
                job.seen = seen;
                true
            }
            CloneState::Gone => false,
            CloneState::Done => {
                let Some(net) = env::net_of(s, &job.target) else { return true };
                let mut writes = Vec::new();
                for (path, contents) in &job.read {
                    match contents {
                        Ok(contents) => writes.push((path.clone(), net.request("projects.writeFile", json!({"cwd": job.cwd, "relativePath": copy_path(&job.subdirectory, path), "contents": contents})))),
                        Err(e) => job.failures.push(format!("{path}: {e}")),
                    }
                }
                job.writes = Some(writes);
                changed = true;
                true
            }
        }
    });
    s.ui.add_to_machine.jobs = jobs;
    changed
}

/// The composer's project: its id and environment.
fn composer_project(s: &App) -> Option<(String, String)> {
    let id = crate::chat::composer::ctx(s).project.map(|p| data::s(p, "id").to_owned())?;
    let key = env::owner(s, &id);
    Some((id, key))
}

/// Subscribes to the clone lists the banner and the jobs read, and drops the rest.
fn sync_watch(s: &mut App) {
    let mut wanted: BTreeSet<String> = s.ui.add_to_machine.jobs.iter().map(|j| j.target.clone()).collect();
    wanted.extend(composer_project(s).map(|(_, key)| key));
    let watched = std::mem::take(&mut s.ui.add_to_machine.watched);
    for key in watched.difference(&wanted) {
        if let Some(net) = env::net_of(s, key) {
            net.unwatch_resource(CLONES, json!({}));
        }
    }
    let mut now = BTreeSet::new();
    for key in wanted {
        if let Some(net) = env::net_of(s, &key) {
            net.watch_resource(CLONES, json!({}));
            now.insert(key);
        }
    }
    s.ui.add_to_machine.watched = now;
}

fn poll_actions(s: &mut App) -> bool {
    let mut changed = false;
    for (id, title, removed) in std::mem::take(&mut s.ui.add_to_machine.actions) {
        match take(s, id) {
            None => s.ui.add_to_machine.actions.push((id, title, removed)),
            Some(Err(e)) => {
                crate::shell::palette::toast_error(s, title, Some(&message(&e)));
                changed = true;
            }
            Some(Ok(_)) => {
                if let Some(project) = removed
                    && s.route == (Route::Home { project_id: Some(project) })
                {
                    s.navigate(Route::Home { project_id: None });
                }
                changed = true;
            }
        }
    }
    changed
}

// The composer's clone banner.

fn clone_of<'a>(s: &'a App, project: &str, key: &str) -> Option<&'a Value> {
    let clones = env::source(s, key)?.net_ref()?.resource(CLONES, json!({}))?;
    clones.as_array()?.iter().find(|c| c["projectId"] == project)
}

fn copying(s: &App, project: &str) -> bool {
    s.ui.add_to_machine.jobs.iter().any(|j| j.project_id == project)
}

/// `projectCloneSendBlockReason`.
fn block_reason(clone: Option<&Value>, copying: bool) -> Option<&'static str> {
    match clone {
        Some(c) if c["phase"] == "running" => Some("Cloning repository"),
        Some(c) if c["phase"] != "done" => Some("Repository not cloned"),
        _ => copying.then_some("Copying files"),
    }
}

/// Why Send waits for the composer's project, if it does.
pub fn send_block(s: &App) -> Option<&'static str> {
    let (project, key) = composer_project(s)?;
    block_reason(clone_of(s, &project, &key), copying(s, &project))
}

/// `projectCloneDisplayName`.
fn display_name(clone: &Value) -> String {
    if let Some(name) = clone["repository"]["nameWithOwner"].as_str() {
        return name.to_owned();
    }
    let path = data::s(clone, "destinationPath");
    path.split(['/', '\\']).filter(|p| !p.is_empty()).last().unwrap_or(path).to_owned()
}

/// `projectCloneProgressSummary`: stage, percent, then git's detail unless the clone stalled.
fn progress_summary(clone: &Value) -> String {
    let stage = match data::s(clone, "stage") {
        "connecting" => "Connecting",
        "counting" => "Counting objects",
        "receiving" => "Receiving objects",
        "resolving" => "Resolving deltas",
        _ => "Checking out files",
    };
    let mut parts = vec![stage.to_owned()];
    if let Some(percent) = clone["percent"].as_i64() {
        parts.push(format!("{percent}%"));
    }
    if clone["stalled"] == true {
        parts.push("Stalled".into());
    } else if let Some(detail) = clone["detail"].as_str().filter(|d| !d.is_empty()) {
        parts.push(detail.to_owned());
    }
    parts.join(" · ")
}

#[derive(Debug, PartialEq)]
enum Tone {
    Info,
    Warning,
    Error,
}

/// `projectCloneBannerItem`: a failed copy first, then a clone that hasn't finished.
fn banner_item(clone: Option<&Value>, copy_failed: Option<&(String, Vec<String>)>) -> Option<(Tone, String, String)> {
    if let Some((title, failures)) = copy_failed {
        return Some((Tone::Error, title.clone(), failures.join("\n")));
    }
    let clone = clone.filter(|c| c["phase"] != "done")?;
    let name = display_name(clone);
    Some(match data::s(clone, "phase") {
        "running" => (Tone::Info, format!("Cloning {name}"), progress_summary(clone)),
        "cancelled" => (Tone::Warning, format!("Cancelled cloning {name}"), "Retry to bring in the repository.".into()),
        _ => (Tone::Error, format!("Failed to clone {name}"), data::s(clone, "error").to_owned()),
    })
}

fn clone_action(s: &mut App, key: &str, method: &str, project: &str, title: &'static str) {
    if let Some(net) = env::net_of(s, key) {
        let id = net.request(method, json!({"projectId": project}));
        s.ui.add_to_machine.actions.push((id, title, None));
    }
}

/// `useRemoveClonedProject`: unforced, so a thread made since the failed clone is never deleted.
fn remove_project(s: &mut App, key: &str, project: &str) {
    if let Some(net) = env::net_of(s, key) {
        let id = net.dispatch(json!({"type": "project.delete", "commandId": s5_net::uuid(), "projectId": project}));
        s.ui.add_to_machine.actions.push((id, "Failed to remove project", Some(project.to_owned())));
    }
}

/// The banner docked on the composer's shoulder, as `mismatch_banner` is.
pub fn banner(s: &App, t: &T) -> Option<El> {
    use crate::theme::radius;
    let (project, key) = composer_project(s)?;
    let clone = clone_of(s, &project, &key);
    let (tone, title, description) = banner_item(clone, s.ui.add_to_machine.copy_failed.get(&project))?;
    let ink = match tone {
        Tone::Info => t.c.info_foreground,
        Tone::Warning => t.c.warning_foreground,
        Tone::Error => t.c.destructive_foreground,
    };
    let button = |label: &str| kit::Button::new(label).variant("ghost").size("xs");
    let mut actions: Vec<El> = Vec::new();
    let failed_copy = s.ui.add_to_machine.copy_failed.contains_key(&project);
    let (p, k) = (project.clone(), key.clone());
    if failed_copy {
        actions.push(
            kit::icon_button(&icons::X, "Dismiss")
                .size("icon-xs")
                .on_press(move |s: &mut App| {
                    s.ui.add_to_machine.copy_failed.remove(&p);
                })
                .build(t),
        );
    } else if tone == Tone::Info {
        actions.push(button("Cancel").on_press(move |s: &mut App| clone_action(s, &k, "projectClone.cancel", &p, "Failed to cancel clone")).build(t));
    } else {
        let (p2, k2) = (p.clone(), k.clone());
        actions.push(button("Remove project").on_press(move |s: &mut App| remove_project(s, &k2, &p2)).build(t));
        actions.push(button("Retry").on_press(move |s: &mut App| clone_action(s, &k, "projectClone.retry", &p, "Failed to retry clone")).build(t));
    }
    let muted = t.c.muted_foreground;
    Some(
        row()
            .key("project-clone-banner")
            .w_full()
            .corners([radius::XL2, radius::XL2, 0.0, 0.0])
            .fill(if t.dark { t.c.surface_raised } else { t.c.card })
            .border(1.0, if t.dark { rune::Color::WHITE.alpha(0.05) } else { rune::Color::BLACK.alpha(0.08) })
            .pl(16.0)
            .pr(8.0)
            .pt(4.0)
            .pb(17.0 + 4.0)
            .gap(8.0)
            .center_y()
            .role("alert")
            .child(icon(&icons::DOWNLOAD, 14.0).color(ink))
            .child(text(title).font(theme::XS_MEDIUM).color(t.c.foreground).single_line().truncate().shrink().min_w(0.0))
            .child(
                text(description.replace('\n', "; "))
                    .font(theme::TEXT_XS)
                    .color(muted)
                    .single_line()
                    .truncate()
                    .grow(1.0)
                    .shrink()
                    .min_w(0.0)
                    .help(description),
            )
            .children(actions),
    )
}

// The dialog.

pub fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(d) = &s.ui.add_to_machine.dialog else { return };
    let Some(p) = project(s, &d.project) else { return };
    let k = kit::theme(t);
    let targets = targets(s, &d.project);
    let target_label = targets.iter().find(|(key, _)| *key == d.target).map_or_else(|| env::label(s, &d.target), |t| t.1.clone());
    let url = source_url(p);
    let remote_url = d.protocol.and_then(|proto| clone_url_for_protocol(&url, proto)).unwrap_or_else(|| url.clone());
    let chosen = selected(d);
    let muted = t.c.muted_foreground;
    let mono = |v: &str| text(v.to_owned()).font(theme::MONO_XS).color(muted).truncate().shrink().min_w(0.);
    let label = |v: &str| text(v.to_owned()).font(theme::weight(theme::TEXT_SM, 500.)).color(t.c.foreground);
    let machine = |name: &str| {
        row().gap(6.).center_y().min_w(0.).child(icon(&icons::SERVER, 14.).color(muted)).child(label(name).truncate().shrink().min_w(0.))
    };

    let mut popup = Vec::new();
    let target_cell = if targets.len() > 1 {
        let labels: Vec<String> = targets.iter().map(|t| t.1.clone()).collect();
        let keys = targets.clone();
        kit::Select::new("add-machine-target", &labels)
            .label("Machine to add the project to")
            .size("sm")
            .current(Some(target_label.as_str()))
            .open(d.picking)
            .on_toggle(|s: &mut App| {
                if let Some(d) = &mut s.ui.add_to_machine.dialog {
                    d.picking ^= true;
                }
            })
            .on_close(|s: &mut App| {
                if let Some(d) = &mut s.ui.add_to_machine.dialog {
                    d.picking = false;
                }
            })
            .on_choose(move |s: &mut App, chosen: &str, _: &mut rune::Tx| {
                if let Some((key, _)) = keys.iter().find(|t| t.1 == chosen) {
                    pick_target(s, key.clone());
                }
            })
            .build(&k, &mut popup)
    } else {
        machine(&target_label)
    };
    let count = chosen.len();
    let route = row()
        .w_full()
        .gap(12.)
        .center_y()
        .child(col().grow(1.).shrink().min_w(0.).gap(4.).child(machine(&env::label(s, &d.source))).child(mono(data::s(p, "workspaceRoot"))))
        .child(
            col()
                .w(112.)
                .gap(2.)
                .items_center()
                .child(text("git clone").font(theme::TEXT_XS).color(muted))
                .child(row().w_full().center_y().child(el().grow(1.).h(1.).fill(t.c.border)).child(icon(&icons::ARROW_RIGHT, 14.).color(muted)))
                .child(text(if count == 0 { "no files".to_owned() } else { format!("+ {count} {}", if count == 1 { "file" } else { "files" }) }).font(theme::TEXT_XS).color(muted)),
        )
        .child(col().grow(1.).shrink().min_w(0.).gap(4.).child(target_cell).child(mono(&d.path.text)));

    let mut protocol = col().w_full().gap(6.).child(label("Clone over"));
    let (ssh, https) = (clone_url_for_protocol(&url, Protocol::Ssh), clone_url_for_protocol(&url, Protocol::Https));
    if ssh.is_some() && https.is_some() {
        let current = d.protocol.map(|p| if p == Protocol::Ssh { 0 } else { 1 });
        protocol = protocol.child(
            kit::SegmentedControl::new(&["SSH", "HTTPS"], current)
                .label("Clone protocol")
                .fit()
                .on_pick(|s: &mut App, i: usize, _: &mut rune::Tx| {
                    if let Some(d) = &mut s.ui.add_to_machine.dialog {
                        d.protocol = Some(if i == 0 { Protocol::Ssh } else { Protocol::Https });
                    }
                })
                .build(&k),
        );
    }
    protocol = protocol.child(mono(&remote_url));

    let folder = col().w_full().gap(6.).child(label(&format!("Folder on {target_label}"))).child(
        kit::Input::new(&d.path)
            .font("mono")
            .on_edit(|s: &mut App| &mut s.ui.add_to_machine.dialog.as_mut().expect("the dialog is open").path)
            .on_submit(|s: &mut App, _| submit(s))
            .build(t)
            .single_line(),
    );

    let mut dialog = kit::Dialog::new("add-to-machine", "Add to another machine")
        .open(true)
        .width(576.)
        .scroll_all()
        .description(format!(
            "Clone {} and bring the local files git does not track.",
            p["repositoryIdentity"]["displayName"].as_str().unwrap_or(data::s(p, "title"))
        ))
        .on_close(close)
        .child(route)
        .child(protocol)
        .child(folder);
    if d.listing.is_some() || d.files.is_some() {
        dialog = dialog.child(files_section(d, &chosen, t, &k));
    }
    if let Some(error) = &d.error {
        dialog = dialog.child(text(error.clone()).font(theme::TEXT_SM).color(t.c.destructive_foreground).wrap());
    }
    let submitting = d.submit.is_some();
    dialog = dialog.child(
        row()
            .w_full()
            .gap(8.)
            .justify_end()
            .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
            .child(
                kit::Button::new(if submitting { "Starting…".to_owned() } else { format!("Clone to {target_label}") })
                    .disabled(unavailable(d))
                    .on_press(submit)
                    .build(t),
            ),
    );
    dialog.build(t, layers);
    layers.extend(popup);
}

fn files_section(d: &Dialog, chosen: &BTreeSet<String>, t: &T, k: &rune_kit::Kit) -> El {
    let muted = t.c.muted_foreground;
    let note = |v: String| text(v).font(theme::TEXT_XS).color(muted).wrap();
    let mut header = row().w_full().gap(8.).center_y().child(text("Copy ignored files").font(theme::weight(theme::TEXT_SM, 500.)).color(t.c.foreground).grow(1.));
    let mut section = col().w_full().gap(6.);
    match &d.files {
        None => section = section.child(header).child(note("Finding ignored files…".into())),
        Some(Err(e)) => section = section.child(header).child(text(e.clone()).font(theme::TEXT_XS).color(t.c.destructive_foreground).wrap()),
        Some(Ok(l)) => {
            let copyable = l.files.iter().filter(|f| f.1 <= COPYABLE_MAX).count();
            if copyable > 0 {
                let bytes: u64 = l.files.iter().filter(|f| chosen.contains(&f.0)).map(|f| f.1).sum();
                let size = if bytes > 0 { format!(" · {}", format_bytes(bytes as f64)) } else { String::new() };
                header = header
                    .child(text(format!("{} of {}{size}", chosen.len(), l.files.len())).font(theme::TEXT_XS).color(muted))
                    .child(kit::Button::new(if chosen.len() == copyable { "None" } else { "All" }).variant("ghost").size("xs").on_press(toggle_all).build(t));
            }
            section = section.child(header);
            if l.files.is_empty() {
                section = section.child(note("No ignored files to copy.".into()));
            } else {
                let mut list = col().key("add-machine-files").w_full().max_h(208.).scroll(rune::ScrollSpec { stick_bottom: false }).radius(6.).border(1., t.c.border);
                for (i, (path, bytes)) in l.files.iter().enumerate() {
                    let too_large = *bytes > COPYABLE_MAX;
                    let checked = chosen.contains(path);
                    let toggle_path = path.clone();
                    let mut line = row()
                        .key(format!("add-machine-file-{i}"))
                        .w_full()
                        .gap(8.)
                        .px(10.)
                        .py(6.)
                        .center_y()
                        .child(kit::Checkbox::new(checked).hide_label().label(path.clone()).disabled(too_large).build(k))
                        .child(text(path.clone()).font(theme::MONO_XS).color(t.c.foreground).truncate().grow(1.).shrink().min_w(0.))
                        .child(text(if too_large { "Over 1 MB".to_owned() } else { format_bytes(*bytes as f64) }).font(theme::TEXT_XS).color(muted));
                    line = if too_large { line.opacity(0.64) } else { line.on_press(move |s: &mut App| toggle(s, &toggle_path)) };
                    if i > 0 {
                        list = list.child(el().w_full().h(1.).fill(t.c.border));
                    }
                    list = list.child(line);
                }
                section = section.child(list);
            }
            if l.truncated {
                section = section.child(note(format!("Showing the first {} files.", l.files.len())));
            }
            section = section.child(note("Files are copied after the clone finishes. Folders git ignores whole, like node_modules, are left out.".into()));
        }
    }
    section
}

/// `formatBytes`: the unit is picked after rounding, so 1023.6 KB reads "1.00 MB".
fn format_bytes(value: f64) -> String {
    if value.round() < 1024.0 {
        return format!("{} B", value.round() as i64);
    }
    let scaled = |v: f64| format!("{:.*}", if v >= 100.0 { 0 } else if v >= 10.0 { 1 } else { 2 }, v);
    let units = ["KB", "MB", "GB", "TB"];
    let (mut next, mut unit) = (value / 1024.0, 0);
    while scaled(next).parse::<f64>().unwrap_or(0.0) >= 1024.0 && unit < units.len() - 1 {
        next /= 1024.0;
        unit += 1;
    }
    format!("{} {}", scaled(next), units[unit])
}

/// A project with a GitHub remote, a connected "Build box" without it, and its ignored files.
fn scene_dialog(s: &mut App) {
    crate::scene_clock(s);
    let identity = json!({"canonicalKey": "github.com/acme/widgets", "locator": {"remoteUrl": "git@github.com:acme/widgets.git"}, "displayName": "acme/widgets"});
    let crate::data::Source::Fixture(local) = &mut s.src else { return };
    let Some(project) = local.projects.first_mut() else { return };
    project["repositoryIdentity"] = identity;
    let id = data::s(project, "id").to_owned();
    local.conn = s5_net::Conn::Open;
    local.shell_ready = true;
    let mut remote = crate::data::empty_state();
    remote.config = Some(json!({"environment": {"environmentId": "box-id", "label": "Build box"}, "settings": {"addProjectBaseDirectory": "~/code"}}));
    remote.conn = s5_net::Conn::Open;
    remote.shell_ready = true;
    s.ui.env.saved.push(env::Saved { id: "box".into(), label: "Build box".into(), base: "https://box.example.com".into(), bearer: String::new() });
    env::park(s, "box", crate::data::Source::Fixture(remote));
    s.route = Route::Home { project_id: Some(id.clone()) };
    open(s, &id, None);
    if let Some(d) = &mut s.ui.add_to_machine.dialog {
        let files = [(".env", 412), (".env.local", 1_288), ("apps/api/.dev.vars", 96), ("coverage/lcov.info", 2_400_000), ("notes.txt", 3_100)];
        d.files = Some(Ok(Listing { files: files.iter().map(|(p, b)| ((*p).to_owned(), *b)).collect(), truncated: false }));
    }
}

/// A project whose env files did not land on the new machine.
fn scene_copy_failed(s: &mut App) {
    crate::scene_clock(s);
    let Some(id) = s.state().projects.first().map(|p| data::s(p, "id").to_owned()) else { return };
    s.route = Route::Home { project_id: Some(id.clone()) };
    let failures = vec![".env: Permission denied (os error 13)".to_owned(), "apps/api/.dev.vars: Permission denied (os error 13)".to_owned()];
    s.ui.add_to_machine.copy_failed.insert(id, (copy_failure_title(failures.len(), "Build box"), failures));
}

pub const SCENES: &[(&str, fn(&mut App))] = &[("add-to-machine", scene_dialog), ("project-copy-failed", scene_copy_failed)];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_env_files_under_the_read_cap_by_default() {
        for path in [".env", ".env.local", "apps/web/.env.production", ".envrc", "worker/.dev.vars"] {
            assert!(is_default_copied(path), "{path}");
        }
        for path in [".env.", ".environment", "env", "notes/.envrc.bak", ".dev.vars.example"] {
            assert!(!is_default_copied(path), "{path}");
        }
        let files = vec![(".env".to_owned(), 10), ("big/.env".to_owned(), COPYABLE_MAX + 1), ("dist/app.js".to_owned(), 10)];
        assert_eq!(default_selection(&files), BTreeSet::from([".env".to_owned()]));
    }

    #[test]
    fn copies_keep_the_projects_place_below_its_repository() {
        assert_eq!(subdirectory(Some("/r/repo"), "/r/repo/apps/web"), "apps/web");
        assert_eq!(subdirectory(Some("/r/repo/"), "/r/repo/apps"), "apps");
        assert_eq!(subdirectory(Some("C:\\r\\repo"), "C:\\r\\repo\\apps\\web"), "apps/web");
        assert_eq!(subdirectory(Some("/r/repo"), "/r/repo"), "");
        assert_eq!(subdirectory(Some("/r/repo"), "/r/repository"), "");
        assert_eq!(subdirectory(None, "/r/repo/apps"), "");
        assert_eq!(copy_path("apps/web", ".env"), "apps/web/.env");
        assert_eq!(copy_path("", ".env"), ".env");
        assert_eq!(copy_failure_title(1, "Studio"), "Could not copy 1 file to Studio");
        assert_eq!(copy_failure_title(3, "Studio"), "Could not copy 3 files to Studio");
    }

    #[test]
    fn rewrites_clone_urls_as_shared_clone_url_does() {
        let gh_ssh = "git@github.com:acme/app.git";
        let gh_https = "https://github.com/acme/app.git";
        assert_eq!(clone_url_protocol(gh_ssh), Some(Protocol::Ssh));
        assert_eq!(clone_url_protocol(gh_https), Some(Protocol::Https));
        assert_eq!(clone_url_protocol("ssh://git@host:2222/a/b"), Some(Protocol::Ssh));
        assert_eq!(clone_url_protocol("/local/path"), None);
        assert_eq!(clone_url_protocol("not a url"), None);
        assert_eq!(clone_url_for_protocol(gh_ssh, Protocol::Https).as_deref(), Some(gh_https));
        assert_eq!(clone_url_for_protocol(gh_https, Protocol::Ssh).as_deref(), Some(gh_ssh));
        assert_eq!(clone_url_for_protocol(&format!(" {gh_ssh} "), Protocol::Ssh).as_deref(), Some(gh_ssh));
        assert_eq!(clone_url_for_protocol("https://user:tok@github.com/acme/app", Protocol::Ssh).as_deref(), Some("git@github.com:acme/app"));
        assert_eq!(clone_url_for_protocol("ssh://git@host:2222/a/b", Protocol::Https), None);
        assert_eq!(clone_url_for_protocol("https://host:8443/a/b", Protocol::Ssh), None);
        assert_eq!(
            clone_url_for_protocol("git@ssh.dev.azure.com:v3/org/proj/repo", Protocol::Https).as_deref(),
            Some("https://dev.azure.com/org/proj/_git/repo")
        );
        assert_eq!(
            clone_url_for_protocol("https://dev.azure.com/org/proj/_git/repo", Protocol::Ssh).as_deref(),
            Some("git@ssh.dev.azure.com:v3/org/proj/repo")
        );
        assert_eq!(clone_url_for_protocol("https://dev.azure.com/org/proj/repo", Protocol::Ssh), None);
    }

    #[test]
    fn waits_for_the_clone_to_finish_or_disappear() {
        let list = |phase: &str| json!([{"projectId": "p", "phase": phase, "error": "auth failed"}]);
        assert_eq!(clone_state(&Value::Null, "p", false), CloneState::Waiting { seen: false });
        assert_eq!(clone_state(&list("running"), "p", false), CloneState::Waiting { seen: true });
        assert_eq!(clone_state(&list("failed"), "p", true), CloneState::Waiting { seen: true });
        assert_eq!(clone_state(&list("done"), "p", true), CloneState::Done);
        assert_eq!(clone_state(&json!([]), "p", true), CloneState::Gone);
        assert_eq!(clone_state(&json!([]), "p", false), CloneState::Waiting { seen: false });
    }

    #[test]
    fn the_clone_banner_and_send_block_read_like_use_project_clone_banner() {
        let running = json!({"projectId": "p", "phase": "running", "stage": "receiving", "percent": 42, "detail": "1.2 MiB/s", "repository": {"nameWithOwner": "acme/app"}, "destinationPath": "/w/app"});
        assert_eq!(block_reason(Some(&running), true), Some("Cloning repository"));
        assert_eq!(banner_item(Some(&running), None), Some((Tone::Info, "Cloning acme/app".into(), "Receiving objects · 42% · 1.2 MiB/s".into())));
        let stalled = json!({"phase": "running", "stage": "counting", "percent": null, "detail": "old", "stalled": true, "repository": null, "destinationPath": "/w/widgets/"});
        assert_eq!(banner_item(Some(&stalled), None).map(|b| (b.1, b.2)), Some(("Cloning widgets".into(), "Counting objects · Stalled".into())));
        let failed = json!({"phase": "failed", "error": "auth failed", "repository": null, "destinationPath": "/w/app"});
        assert_eq!(block_reason(Some(&failed), false), Some("Repository not cloned"));
        assert_eq!(banner_item(Some(&failed), None), Some((Tone::Error, "Failed to clone app".into(), "auth failed".into())));
        let cancelled = json!({"phase": "cancelled", "repository": null, "destinationPath": "C:\\w\\app"});
        assert_eq!(banner_item(Some(&cancelled), None), Some((Tone::Warning, "Cancelled cloning app".into(), "Retry to bring in the repository.".into())));
        let done = json!({"phase": "done"});
        assert_eq!(block_reason(Some(&done), true), Some("Copying files"));
        assert_eq!(block_reason(Some(&done), false), None);
        assert_eq!(block_reason(None, false), None);
        assert_eq!(banner_item(Some(&done), None), None);
        let copy = ("Could not copy 2 files to Box".to_owned(), vec![".env: denied".to_owned(), ".envrc: denied".to_owned()]);
        assert_eq!(banner_item(Some(&done), Some(&copy)), Some((Tone::Error, copy.0.clone(), ".env: denied\n.envrc: denied".into())));
    }

    fn app_with_remote() -> App {
        let mut s = App::new();
        let identity = json!({"canonicalKey": "github.com/acme/app", "locator": {"remoteUrl": "git@github.com:acme/app.git"}, "rootPath": "/w/app", "displayName": "acme/app"});
        let mut local = crate::data::empty_state();
        local.config = Some(json!({"environment": {"environmentId": "local-id", "label": "Laptop", "capabilities": {"ignoredFileListing": true}}, "settings": {}}));
        local.projects = vec![json!({"id": "p1", "title": "app", "workspaceRoot": "/w/app", "repositoryIdentity": identity})];
        local.shell_ready = true;
        local.conn = s5_net::Conn::Open;
        s.src = crate::data::Source::Fixture(local);
        s.ui.env.active = env::LOCAL.into();
        let mut remote = crate::data::empty_state();
        remote.config = Some(json!({"environment": {"environmentId": "box-id", "label": "Box"}, "settings": {"addProjectBaseDirectory": "/srv/code"}}));
        remote.shell_ready = true;
        remote.conn = s5_net::Conn::Open;
        s.ui.env.saved.push(env::Saved { id: "box".into(), label: "Box".into(), base: "https://box.example".into(), bearer: "b".into() });
        env::park(&mut s, "box", crate::data::Source::Fixture(remote));
        s
    }

    #[test]
    fn offers_connected_machines_without_the_repository() {
        let mut s = app_with_remote();
        assert_eq!(targets(&s, "p1"), vec![("box".to_owned(), "Box".to_owned())]);
        assert_eq!(default_path(&s, "box", "git@github.com:acme/app.git"), "/srv/code/app");
        open(&mut s, "p1", None);
        let d = s.ui.add_to_machine.dialog.as_ref().expect("opens");
        assert_eq!((d.target.as_str(), d.path.text.as_str(), d.protocol), ("box", "/srv/code/app", Some(Protocol::Ssh)));
        close(&mut s);
        // The same repository already on Box: nowhere left to add it.
        if let Some(crate::data::Source::Fixture(st)) = env::source_mut(&mut s, "box") {
            st.projects.push(json!({"id": "p2", "repositoryIdentity": {"canonicalKey": "GitHub.com/acme/app"}}));
        }
        env::adopt(&mut s, "p2", "box");
        if let crate::data::Source::Fixture(st) = &mut s.src {
            st.projects.push(json!({"id": "p2", "repositoryIdentity": {"canonicalKey": "GitHub.com/acme/app"}}));
        }
        assert!(targets(&s, "p1").is_empty());
    }
}
