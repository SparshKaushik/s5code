//! The composer's trigger drawer (ComposerCommandMenu): `@` threads, machines, and files and
//! folders from `projects.searchEntries`, `/` commands, `$` skills, `#` the project's pull
//! requests. It rises out of the top of the composer, tucked 17 points under it. Skills and
//! commands come from the cwd's workspace snapshot when the provider has one.

use rune::{Cursor, Cx, El, Tx, col, el, icon, row, text};
use serde_json::{Value, json};

use super::{ctx, draft, draft_mut, provider, selection, set_interaction_mode, supports_plan};
use crate::data;
use crate::icons;
use crate::theme::{self, T, radius};
use crate::App;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Path,
    Slash,
    Skill,
    PullRequest,
}

/// The trigger token under the caret.
pub struct Open {
    pub kind: Kind,
    pub token: String,
    pub start: usize,
    pub end: usize,
}

impl Open {
    fn query(&self) -> &str {
        &self.token[1..]
    }
}

#[derive(Default)]
pub struct Mention {
    query: Option<String>,
    request: Option<u64>,
    pub loading: bool,
    entries: Vec<(String, bool)>,
    prs: Prs,
    snapshot: Snapshot,
    links: Links,
}

/// useLinkedEnvironments: the composer's environment's `peerLinks.list`, asked for as an `@`
/// query starts and kept briefly, so its agents' linked machines' threads can be mentioned.
#[derive(Default)]
struct Links {
    /// The environment the list belongs to, and when it was asked for.
    env: Option<String>,
    asked_at: i64,
    request: Option<u64>,
    /// Each link that has not expired: its environment id and the label it was linked under.
    links: Vec<(String, String)>,
}

const LINKS_TTL_MS: i64 = 30_000;

/// useProviderWorkspaceSnapshotRefresh: one `server.refreshProviders` scan per environment,
/// provider and cwd (`key`), again once the TTL passes, retried after a cooldown up to three
/// times. The config stream then carries the snapshot.
#[derive(Default)]
struct Snapshot {
    key: Option<(String, String, String)>,
    /// The scan in flight and the key it asked about.
    request: Option<u64>,
    asked: Option<(String, String, String)>,
    requested_at: Option<i64>,
    retry_at: Option<i64>,
    failures: u32,
    had: bool,
}

/// PROVIDER_WORKSPACE_SNAPSHOT_TTL_MS, WORKSPACE_SNAPSHOT_RETRY_COOLDOWN_MS, WORKSPACE_SNAPSHOT_MAX_ATTEMPTS.
const SNAPSHOT_TTL_MS: i64 = 5 * 60_000;
const SNAPSHOT_RETRY_MS: i64 = 10_000;
const SNAPSHOT_ATTEMPTS: u32 = 3;

impl Snapshot {
    /// planProviderWorkspaceSnapshotRefresh: whether to scan now. A lost snapshot lifts the
    /// TTL and the cooldown.
    fn plan(&mut self, complete: bool, current: bool, now: i64) -> bool {
        if self.had && !complete {
            (self.requested_at, self.retry_at, self.failures) = (None, None, 0);
        }
        self.had = complete;
        if self.requested_at.is_some_and(|at| now - at >= SNAPSHOT_TTL_MS) {
            (self.requested_at, self.failures) = (None, 0);
        }
        if self.retry_at.is_some_and(|at| now >= at) {
            self.retry_at = None;
        }
        let due = self.request.is_none() && !current && self.requested_at.is_none() && self.retry_at.is_none();
        if due {
            self.requested_at = Some(now);
        }
        due
    }

    /// failProviderWorkspaceSnapshotRefresh: cool down before the next try; after the last, only
    /// the TTL lets one through.
    fn failed(&mut self, now: i64) {
        self.failures += 1;
        if self.failures < SNAPSHOT_ATTEMPTS {
            (self.requested_at, self.retry_at) = (None, Some(now + SNAPSHOT_RETRY_MS));
        }
    }

    /// isProviderWorkspaceSnapshotScanning.
    fn scanning(&self, complete: bool, pending: bool) -> bool {
        (self.requested_at.is_some() || pending) && !complete && self.failures < SNAPSHOT_ATTEMPTS
    }
}

fn workspace_snapshot<'a>(provider: &'a Value, cwd: &str) -> Option<&'a Value> {
    provider["workspaceSnapshots"].as_array()?.iter().find(|w| data::s(w, "cwd") == cwd)
}

/// The cwd's snapshot is (complete, current, pending): hasCompleteProviderWorkspaceSnapshot,
/// hasCurrentProviderWorkspaceSnapshot and `slashCommandsPending`.
fn snapshot_status(provider: Option<&Value>, cwd: &str, now: i64) -> (bool, bool, bool) {
    let Some(w) = provider.and_then(|p| workspace_snapshot(p, cwd)) else { return (false, false, false) };
    let pending = w["slashCommandsPending"] == true;
    let current = !pending && data::ts(w, "checkedAt").is_some_and(|at| now - at < SNAPSHOT_TTL_MS);
    (!pending, current, pending)
}

/// Keeps the cwd's snapshot fresh and takes its scan's reply. It scans when the provider or cwd
/// changes, the snapshot goes missing, a pending scan's cooldown ends, or (`typed`) the user
/// types, each only when the snapshot is stale and no scan ran within the TTL.
pub fn sync_snapshot(s: &mut App, typed: bool) -> bool {
    let request = s.ui.chat.composer.mention.snapshot.request;
    let reply = match (request, s.src.net()) {
        (_, None) => return false,
        (Some(r), Some(net)) => net.take_reply(r),
        (None, Some(_)) => None,
    };
    let now = data::now_ms();
    let target = selection(s).map(|(i, _)| i).zip(search_root(s)).map(|(i, cwd)| (s.ui.env.active.clone(), i, cwd));
    let (complete, current, pending) = target.as_ref().map_or((false, false, false), |(_, i, cwd)| snapshot_status(provider(s, i), cwd, now));
    let snap = &mut s.ui.chat.composer.mention.snapshot;
    let mut check = typed;
    if let Some(reply) = &reply {
        snap.request = None;
        check = true;
        // A scan counts once its reply holds the cwd's complete snapshot.
        if snap.asked.is_some() && snap.asked == snap.key {
            let done = snap.asked.as_ref().is_some_and(|(_, instance, cwd)| {
                let providers = reply.as_ref().ok().and_then(|v| v["providers"].as_array());
                let replied = providers.and_then(|list| list.iter().find(|p| data::s(p, "instanceId") == instance.as_str()));
                snapshot_status(replied, cwd, now).0
            });
            if !done {
                snap.failed(now);
            }
        }
    }
    let Some(key) = target else { return reply.is_some() };
    if snap.key.as_ref() != Some(&key) {
        let (request, asked) = (snap.request, snap.asked.take());
        *snap = Snapshot { key: Some(key), request, asked, ..Snapshot::default() };
        check = true;
    }
    check |= snap.had != complete || (pending && snap.retry_at.is_some_and(|at| now >= at));
    if !check || !snap.plan(complete, current, now) {
        return reply.is_some();
    }
    snap.asked = snap.key.clone();
    let Some((_, instance, cwd)) = snap.key.clone() else { return true };
    // The reply carries only this cwd's snapshot; the config stream carries it to everyone.
    let input = json!({"instanceId": instance, "cwd": cwd, "workspaceSnapshotsOnDemand": true});
    let id = s.src.net().map(|net| net.request("server.refreshProviders", input));
    s.ui.chat.composer.mention.snapshot.request = id;
    true
}

/// The cwd's skills are still being scanned, so more may arrive.
fn skills_scanning(s: &App) -> bool {
    let snap = &s.ui.chat.composer.mention.snapshot;
    let Some((_, instance, cwd)) = &snap.key else { return false };
    let (complete, _, pending) = snapshot_status(provider(s, instance), cwd, data::now_ms());
    snap.scanning(complete, pending)
}

/// The `#` list: one `pullRequests.list` read per project and text query (numbers filter
/// the unqueried list, as useComposerMenuItems does).
#[derive(Default)]
struct Prs {
    key: Option<(String, String)>,
    request: Option<u64>,
    loading: bool,
    failed: bool,
    entries: Vec<Value>,
}

const PR_RESULTS: usize = 12;

pub struct Item {
    pub id: String,
    pub label: String,
    pub description: String,
    pub folder: Option<bool>,
    pub pr: Option<Value>,
    /// A thread or machine's context record, which goes in as a chip.
    pub context: Option<Value>,
}

const THREAD_RESULTS: usize = 5;

const BUILTIN_SLASH: [(&str, &str); 1] = [("model", "Switch response model for this thread")];
/// Shown after `/model` when the provider has plan mode.
const PLAN_SLASH: [(&str, &str); 2] =
    [("plan", "Switch this thread into plan mode"), ("default", "Switch this thread back to normal build mode")];
/// Shown last among the built-ins, at the message start, on a thread that can start side chats.
const SIDE_SLASH: [(&str, &str); 2] = [("side", "Ask a side question in a new side chat"), ("btw", "Same as /side")];

pub fn open_menu(s: &App) -> Option<Open> {
    let d = draft(s)?;
    let text = &d.edit.text;
    let caret = d.edit.focus.min(text.len());
    if !text.is_char_boundary(caret) {
        return None;
    }
    let start = text[..caret].rfind(char::is_whitespace).map_or(0, |i| i + text[i..].chars().next().map_or(1, char::len_utf8));
    let token = &text[start..caret];
    let kind = match token.chars().next()? {
        '@' => Kind::Path,
        // A second slash makes the word a path (isPathWord).
        '/' if !text[start + 1..].split(char::is_whitespace).next().unwrap_or("").contains('/') => Kind::Slash,
        '$' => Kind::Skill,
        '#' if pr_query(&token[1..]) => Kind::PullRequest,
        _ => return None,
    };
    if s.ui.chat.composer.menu_dismissed.as_deref() == Some(token) {
        return None;
    }
    Some(Open { kind, token: token.to_string(), start, end: caret })
}

/// The selected provider's `skills` or `slashCommands`: the cwd's workspace snapshot's when it has
/// one, else the machine-wide list (resolveProviderSkillsForCwd, resolveProviderSlashCommandsForCwd).
fn provider_list<'a>(s: &'a App, key: &str) -> Vec<&'a Value> {
    let Some(p) = selection(s).and_then(|(i, _)| provider(s, &i)) else { return Vec::new() };
    resolved(p, search_root(s).as_deref(), key).iter().collect()
}

fn resolved<'a>(provider: &'a Value, cwd: Option<&str>, key: &str) -> &'a [Value] {
    // `snapshot?.skills ?? provider.skills`: a snapshot without the list keeps the global one.
    let snapshot = cwd.and_then(|cwd| workspace_snapshot(provider, cwd)).map(|w| &w[key]).filter(|v| !v.is_null());
    snapshot.unwrap_or(&provider[key]).as_array().map_or(&[], |v| v.as_slice())
}

/// The selected provider's skills for the cwd.
pub fn skills(s: &App) -> Vec<&Value> {
    provider_list(s, "skills")
}

/// The composer's own environment.
fn own_environment(s: &App) -> String {
    s.state().config.as_ref().map(|c| data::s(&c["environment"], "environmentId").to_owned()).unwrap_or_default()
}

fn context_item(record: Value, description: &str) -> Item {
    let label = data::s(&record, "label").to_owned();
    Item { id: super::chips::pill_id(&record), label, description: description.to_owned(), folder: None, pr: None, context: Some(record) }
}

/// offersLinkedEnvironments: this server links machines, and Settings has it on.
fn offers_links(s: &App) -> bool {
    s.state().config.as_ref().is_some_and(|c| c["environment"]["capabilities"]["peerLinks"] == true)
        && crate::settings::server_settings(s)["linkedEnvironmentsEnabled"] == true
}

/// linkedEnvironmentNames: the machines the composer's environment links to, by environment id,
/// named as this window knows them, else as they were linked.
fn linked_environments(s: &App) -> Vec<(String, String)> {
    let links = &s.ui.chat.composer.mention.links;
    if !offers_links(s) || links.env.as_deref() != Some(s.ui.env.active.as_str()) {
        return Vec::new();
    }
    let local = crate::env::local_id(s);
    links
        .links
        .iter()
        .map(|(id, label)| {
            let key = if local.as_deref() == Some(id.as_str()) { crate::env::LOCAL } else { id.as_str() };
            let known = crate::env::label(s, key);
            (id.clone(), if known.is_empty() { label.clone() } else { known })
        })
        .collect()
}

/// Asks for the composer's environment's links, unless a recent answer for it is in hand.
fn request_links(s: &mut App) {
    if !offers_links(s) {
        return;
    }
    let (env, now) = (s.ui.env.active.clone(), data::now_ms());
    let links = &s.ui.chat.composer.mention.links;
    if links.env.as_ref() == Some(&env) && (links.request.is_some() || now - links.asked_at < LINKS_TTL_MS) {
        return;
    }
    let Some(net) = s.src.net() else { return };
    let request = net.request("peerLinks.list", json!({}));
    let links = &mut s.ui.chat.composer.mention.links;
    if links.env.as_ref() != Some(&env) {
        links.links.clear();
    }
    (links.env, links.asked_at, links.request) = (Some(env), now, Some(request));
}

/// matchComposerThreadItems: listed threads other than the open one, on this environment or a
/// machine it links to ("Thread on {machine}"), best title match first. A bare `@` lists none.
fn thread_items(s: &App, query: &str) -> Vec<Item> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let open = ctx(s).thread.map(|t| data::s(t, "id"));
    let own = own_environment(s);
    let local = crate::env::local_id(s);
    let linked = linked_environments(s);
    // Each thread with its environment id, and its machine's name when that is a linked one.
    let candidates: Vec<(&Value, &str, Option<&str>)> = s
        .state()
        .threads
        .iter()
        .filter(|t| {
            Some(data::s(t, "id")) != open
                && t["archivedAt"].is_null()
                && t["lineage"]["relationshipToParent"] != "subagent"
                && t["lineage"]["sideChat"] != true
        })
        .filter_map(|t| match t.get(crate::env::TAG).and_then(Value::as_str) {
            None => Some((t, own.as_str(), None)),
            Some(key) => {
                let id = if key == crate::env::LOCAL { local.as_deref()? } else { key };
                linked.iter().find(|(l, _)| l == id).map(|(l, name)| (t, l.as_str(), Some(name.as_str())))
            }
        })
        .collect();
    let recency = |t: &Value| data::ts(t, "latestUserMessageAt").or_else(|| data::ts(t, "updatedAt")).unwrap_or(0);
    let titles: Vec<(&str, i64)> = candidates.iter().map(|&(t, ..)| (data::s(t, "title"), recency(t))).collect();
    super::search::rank_titles(&titles, query, THREAD_RESULTS)
        .into_iter()
        .map(|i| {
            let (thread, environment, machine) = candidates[i];
            let description = machine.map_or_else(|| "Thread".to_owned(), |m| format!("Thread on {m}"));
            context_item(super::chips::thread(environment, data::s(thread, "id"), titles[i].0), &description)
        })
        .collect()
}

/// matchComposerEnvironmentItems: the user's other machines whose name holds the query, when
/// this server links machines (offersLinkedEnvironments).
fn environment_items(s: &App, query: &str) -> Vec<Item> {
    let query = query.trim().to_lowercase();
    if query.is_empty() || !offers_links(s) {
        return Vec::new();
    }
    let own = own_environment(s);
    crate::env::keys(s)
        .into_iter()
        .filter_map(|key| {
            let id = if key == crate::env::LOCAL { crate::env::local_id(s)? } else { key.clone() };
            let label = crate::env::label(s, &key);
            (id != own && label.to_lowercase().contains(&query)).then(|| context_item(super::chips::environment(&id, &label), "Machine"))
        })
        .collect()
}

pub fn items(s: &App, m: &Open) -> Vec<Item> {
    let q = m.query().to_lowercase();
    let item = |id: String, label: String, description: String| Item { id, label, description, folder: None, pr: None, context: None };
    match m.kind {
        Kind::Slash => {
            let plan: &[(&str, &str)] = if supports_plan(s) { &PLAN_SLASH[..] } else { &[] };
            // Provider commands work only at the message start.
            let at_start = m.start == 0;
            let reason = crate::chat::side::disabled_reason(s);
            let side: Vec<(&str, &str)> =
                if at_start && crate::chat::side::available(s) { SIDE_SLASH.iter().map(|&(n, d)| (n, reason.unwrap_or(d))).collect() } else { Vec::new() };
            let builtins = BUILTIN_SLASH.iter().chain(plan).chain(&side).map(|(n, d)| item(format!("/{n}"), format!("/{n}"), d.to_string()));
            let skills = slash_menu_skills(s);
            let skill_names: Vec<String> = skills.iter().map(|k| data::s(k, "name").trim().to_lowercase()).collect();
            let compact = compact_available(s, m);
            let provided = provider_list(s, "slashCommands")
                .into_iter()
                .filter(|_| at_start)
                .filter(|c| !skill_names.contains(&data::s(c, "name").trim().to_lowercase()))
                .filter(|c| data::s(c, "name") != "compact" || compact)
                .map(|c| item(format!("/{}", data::s(c, "name")), format!("/{}", data::s(c, "name")), command_description(c)));
            // A skill runs anywhere in the prompt, as `$name`.
            let skill_items = skills.iter().map(|k| item(format!("${}", data::s(k, "name")), format!("/skill:{}", data::s(k, "name")), skill_description(k, "")));
            let builtin = BUILTIN_SLASH.len() + plan.len() + side.len();
            let all: Vec<Item> = builtins.chain(provided).chain(skill_items).collect();
            ranked_slash(all, builtin, q.trim_start_matches('/'))
        }
        Kind::Skill => provider_list(s, "skills")
            .into_iter()
            .map(|c| item(format!("${}", data::s(c, "name")), super::chips::skill_label(c), skill_description(c, "Run provider skill")))
            .filter(|i| i.label.to_lowercase().contains(&q) || i.id[1..].to_lowercase().contains(&q))
            .collect(),
        Kind::Path => {
            let mut out = thread_items(s, m.query());
            out.extend(environment_items(s, m.query()));
            let mention = &s.ui.chat.composer.mention;
            if mention.query.as_deref() == Some(m.query()) {
                out.extend(mention.entries.iter().map(|(path, folder)| {
                    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
                    Item { id: path.clone(), label: name.to_string(), description: dir.to_string(), folder: Some(*folder), pr: None, context: None }
                }));
            }
            out
        }
        Kind::PullRequest => pr_matches(s, m.query())
            .into_iter()
            .map(|e| Item { id: format!("#{}", e["number"]), label: format!("#{}", e["number"]), description: data::s(e, "title").to_string(), folder: None, pr: Some(e.clone()), context: None })
            .collect(),
    }
}

/// A provider command's description, its input hint, or "Run provider command".
fn command_description(c: &Value) -> String {
    [data::s(c, "description"), data::s(&c["input"], "hint")].into_iter().find(|d| !d.is_empty()).unwrap_or("Run provider command").to_owned()
}

/// A skill's short description, its description, its scope, else `fallback`.
fn skill_description(k: &Value, fallback: &str) -> String {
    let scope = data::s(k, "scope");
    let scoped = if scope.is_empty() { String::new() } else { format!("{scope} skill") };
    [data::s(k, "shortDescription").to_owned(), data::s(k, "description").to_owned(), scoped]
        .into_iter()
        .find(|d| !d.is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

/// getProviderSkillsForSlashMenu: invocable skills, one per name, when Settings lists them in `/`.
fn slash_menu_skills(s: &App) -> Vec<&Value> {
    if crate::settings::value(s, "showSkillsInSlashMenu") == false {
        return Vec::new();
    }
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for k in provider_list(s, "skills") {
        let name = data::s(k, "name").trim().to_lowercase();
        if k["enabled"] == true && k["userInvocable"] != false && !seen.contains(&name) {
            seen.push(name);
            out.push(k);
        }
    }
    out
}

/// `/compact` is offered only alone in the prompt, with nothing attached, while compacting can
/// run (compactSlashCommandAvailable).
fn compact_available(s: &App, m: &Open) -> bool {
    let Some(d) = draft(s) else { return false };
    let text = &d.edit.text;
    m.end <= text.len()
        && text[..m.start].trim().is_empty()
        && text[m.end..].trim().is_empty()
        && d.attachments.is_empty()
        && d.edit.pills().next().is_none()
        && matches!(super::context::compact_state(s), Some(None))
}

/// searchSlashCommandItems: by the best of the name's and the description's match, built-ins
/// before provider commands on a tie, then by name.
fn ranked_slash(items: Vec<Item>, builtin: usize, q: &str) -> Vec<Item> {
    let q = q.trim();
    if q.is_empty() {
        return items;
    }
    let mut ranked: Vec<(usize, usize, Item)> = items
        .into_iter()
        .enumerate()
        .filter_map(|(i, it)| {
            let name = score(&it.label[1..].to_lowercase(), q, [0, 2, 4, 6], Some(100), &["-", "_", "/"]);
            let desc = score(&it.description.to_lowercase(), q, [20, 22, 24, 26], None, &[" ", "-", "_", "/"]);
            let best = name.into_iter().chain(desc).min()?;
            Some((best, i, it))
        })
        .collect();
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| (a.1 >= builtin).cmp(&(b.1 >= builtin))).then_with(|| a.2.label.cmp(&b.2.label)));
    ranked.into_iter().map(|(_, _, it)| it).collect()
}

/// scoreQueryMatch over lowercased `value`: exact, prefix, word boundary, substring, then a
/// subsequence, each later tier scoring higher (worse) and longer values a little worse.
fn score(value: &str, q: &str, [exact, prefix, boundary, includes]: [usize; 4], fuzzy: Option<usize>, markers: &[&str]) -> Option<usize> {
    if value.is_empty() || q.is_empty() {
        return None;
    }
    let (v, qc): (Vec<char>, Vec<char>) = (value.chars().collect(), q.chars().collect());
    let penalty = v.len().saturating_sub(qc.len()).min(64);
    let char_index = |byte: usize| value[..byte].chars().count();
    if value == q {
        return Some(exact);
    }
    if value.starts_with(q) {
        return Some(prefix + penalty);
    }
    if let Some(at) = markers.iter().filter_map(|m| value.find(&format!("{m}{q}")).map(|i| char_index(i) + m.chars().count())).min() {
        return Some(boundary + at * 2 + penalty);
    }
    if let Some(i) = value.find(q) {
        return Some(includes + char_index(i) * 2 + penalty);
    }
    let base = fuzzy?;
    let (mut qi, mut first, mut prev, mut gaps) = (0, None, None, 0);
    for (vi, c) in v.iter().enumerate() {
        if *c != qc[qi] {
            continue;
        }
        first.get_or_insert(vi);
        if let Some(p) = prev {
            gaps += vi - p - 1;
        }
        prev = Some(vi);
        qi += 1;
        if qi == qc.len() {
            let first = first.unwrap_or(0);
            let span = vi - first + 1 - qc.len();
            return Some(base + first * 2 + gaps * 3 + span + penalty);
        }
    }
    None
}

/// `#` then nothing, or a letter or digit and then letters, digits, `_` or `-`.
fn pr_query(q: &str) -> bool {
    let mut chars = q.chars();
    match chars.next() {
        None => true,
        Some(c) => c.is_alphanumeric() && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-'),
    }
}

fn numeric(q: &str) -> bool {
    q.chars().all(|c| c.is_ascii_digit())
}

/// filterComposerPullRequestMatches for a number (exact first, then the newest), the server's
/// order for text.
fn pr_matches<'a>(s: &'a App, q: &str) -> Vec<&'a Value> {
    let prs = &s.ui.chat.composer.mention.prs;
    let project = ctx(s).project.map(|p| data::s(p, "id")).unwrap_or_default();
    match &prs.key {
        Some((p, k)) if p == project && *k == pr_key(q) => {}
        _ => return Vec::new(),
    }
    let mut list: Vec<&Value> = prs.entries.iter().filter(|e| !numeric(q) || e["number"].to_string().contains(q)).collect();
    if numeric(q) {
        list.retain(|e| data::s(e, "projectId").is_empty() || data::s(e, "projectId") == project);
        list.sort_by(|a, b| {
            let exact = |e: &Value| e["number"].to_string() == q;
            exact(b).cmp(&exact(a)).then_with(|| data::s(b, "updatedAt").cmp(data::s(a, "updatedAt")))
        });
    }
    list.truncate(PR_RESULTS);
    list
}

fn pr_key(q: &str) -> String {
    if numeric(q) { String::new() } else { q.to_owned() }
}

/// The working directory file searches run in: the thread's worktree, else the project root.
pub(super) fn search_root(s: &App) -> Option<String> {
    let c = ctx(s);
    let worktree = c.thread.map(|t| data::s(t, "worktreePath")).filter(|p| !p.is_empty());
    worktree.or_else(|| c.project.map(|p| data::s(p, "workspaceRoot"))).filter(|p| !p.is_empty()).map(str::to_string)
}

/// Starts a file search when the `@` query changed, or a pull request read for `#`; typing also
/// re-checks the workspace snapshot.
pub fn refresh(s: &mut App) {
    sync_snapshot(s, true);
    let Some(m) = open_menu(s) else { return };
    if m.kind == Kind::PullRequest {
        let Some(project) = ctx(s).project.map(|p| data::s(p, "id").to_string()) else { return };
        let key = (project.clone(), pr_key(m.query()));
        let prs = &mut s.ui.chat.composer.mention.prs;
        if prs.key.as_ref() == Some(&key) {
            return;
        }
        *prs = Prs { key: Some(key.clone()), ..Prs::default() };
        let mut input = json!({"state": "all", "projectId": project, "limit": 99});
        if !key.1.is_empty() {
            input["query"] = json!(key.1);
        }
        match s.src.net() {
            Some(net) => {
                let request = net.request("pullRequests.list", input);
                let prs = &mut s.ui.chat.composer.mention.prs;
                prs.request = Some(request);
                prs.loading = true;
            }
            None if !crate::host::live() => {
                let entries = crate::pages::fixture("pull-requests.json").and_then(|v| v["entries"].as_array().cloned()).unwrap_or_default();
                s.ui.chat.composer.mention.prs.entries = entries.into_iter().map(|mut e| {
                    e["projectId"] = json!(project);
                    e
                }).collect();
            }
            None => {}
        }
        return;
    }
    if m.kind == Kind::Path && !m.query().trim().is_empty() {
        request_links(s);
    }
    if m.kind != Kind::Path || s.ui.chat.composer.mention.query.as_deref() == Some(m.query()) {
        return;
    }
    let query = m.query().to_string();
    let root = search_root(s);
    let mention = &mut s.ui.chat.composer.mention;
    mention.query = Some(query.clone());
    mention.entries.clear();
    mention.request = None;
    mention.loading = false;
    let (Some(root), Some(net)) = (root, s.src.net()) else { return };
    // A bare `@` lists nothing until a character follows (useComposerMenuItems).
    if query.is_empty() {
        return;
    }
    let request = net.request("projects.searchEntries", json!({"cwd": root, "query": query, "limit": 80}));
    let mention = &mut s.ui.chat.composer.mention;
    mention.request = Some(request);
    mention.loading = true;
}

pub fn poll(mention: &mut Mention, net: &mut s5_net::Net) -> bool {
    let prs = &mut mention.prs;
    let pr_reply = prs.request.and_then(|r| net.take_reply(r));
    let got_prs = pr_reply.is_some();
    if let Some(reply) = pr_reply {
        prs.request = None;
        prs.loading = false;
        match reply {
            Ok(v) => prs.entries = v["entries"].as_array().cloned().unwrap_or_default(),
            Err(_) => prs.failed = true,
        }
    }
    let links = &mut mention.links;
    let links_reply = links.request.and_then(|r| net.take_reply(r));
    let got = got_prs || links_reply.is_some();
    if let Some(reply) = links_reply {
        links.request = None;
        let list = reply.ok().and_then(|v| v["links"].as_array().cloned()).unwrap_or_default();
        links.links = list.iter().filter(|l| data::s(l, "status") != "expired").map(|l| (data::s(l, "environmentId").to_owned(), data::s(l, "label").to_owned())).collect();
    }
    let Some(request) = mention.request else { return got };
    let Some(reply) = net.take_reply(request) else { return got };
    mention.request = None;
    mention.loading = false;
    mention.entries = reply
        .ok()
        .and_then(|v| v["entries"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|e| (data::s(e, "path").to_string(), data::s(e, "kind") == "directory"))
        .collect();
    true
}

pub fn choose(s: &mut App, m: &Open, index: usize, _tx: &mut Tx) {
    let Some(item) = items(s, m).into_iter().nth(index) else { return };
    s.ui.chat.composer.menu_active = 0;
    if item.id == "/model" {
        let d = draft_mut(s);
        d.edit.set_selection(m.start, m.end);
        d.edit.insert("");
        super::picker::toggle(s);
        return;
    }
    // `/usage-limits` opens the provider's limits rather than going to the agent.
    if item.id == "/usage-limits" && m.kind == Kind::Slash && super::usage_limits::offered(s) {
        let d = draft_mut(s);
        d.edit.set_selection(m.start, m.end);
        d.edit.insert("");
        super::usage_limits::open(s);
        return;
    }
    if let Some(mode) = item.id.strip_prefix('/').filter(|_| m.kind == Kind::Slash).filter(|id| PLAN_SLASH.iter().any(|(n, _)| n == id)) {
        let d = draft_mut(s);
        d.edit.set_selection(m.start, m.end);
        d.edit.insert("");
        set_interaction_mode(s, mode);
        return;
    }
    if matches!(item.id.as_str(), "/side" | "/btw") && m.kind == Kind::Slash && crate::chat::side::disabled_reason(s).is_some() {
        return;
    }
    if let Some(record) = item.context {
        let id = super::chips::pill_id(&record);
        let d = draft_mut(s);
        d.edit.replace_with_pill(m.start..m.end, id.clone(), data::s(&record, "label"));
        d.edit.insert(" ");
        if !d.contexts.iter().any(|c| super::chips::pill_id(c) == id) {
            d.contexts.push(record);
        }
        return;
    }
    // A skill goes in as a chip that sends as `$name` (the composer-skill node).
    if let Some(name) = item.id.strip_prefix('$').filter(|_| matches!(m.kind, Kind::Skill | Kind::Slash)) {
        let label = skills(s).into_iter().find(|k| data::s(k, "name") == name).map_or_else(|| name.to_owned(), super::chips::skill_label);
        let d = draft_mut(s);
        d.edit.replace_with_pill(m.start..m.end, super::chips::skill_pill(name), &label);
        d.edit.insert(" ");
        return;
    }
    if let Some(pr) = &item.pr {
        let record = super::chips::pull_request(pr, false, &[]);
        let d = draft_mut(s);
        d.edit.replace_with_pill(m.start..m.end, super::chips::pill_id(&record), data::s(&record, "label"));
        d.edit.insert(" ");
        d.contexts.push(record);
        return;
    }
    let d = draft_mut(s);
    d.edit.set_selection(m.start, m.end);
    match m.kind {
        Kind::Path => {
            d.edit.replace_with_pill(m.start..m.end, item.id.clone(), &item.label);
            d.edit.insert(" ");
        }
        _ => d.edit.insert(&format!("{} ", item.id)),
    }
}

fn surface(t: &T) -> El {
    col()
        .key("composer-menu")
        .corners([radius::XL2, radius::XL2, 0.0, 0.0])
        .fill(if t.dark { t.c.surface_raised } else { t.c.card })
        .border(1.0, if t.dark { rune::Color::WHITE.alpha(0.05) } else { rune::Color::BLACK.alpha(0.08) })
}

pub fn view(s: &App, _cx: &mut Cx<App>, t: &T) -> Option<El> {
    if s.ui.chat.composer.picker.open {
        return None;
    }
    let m = open_menu(s)?;
    let list = items(s, &m);
    if list.is_empty() {
        let prs = &s.ui.chat.composer.mention.prs;
        let message = match m.kind {
            Kind::Path if s.ui.chat.composer.mention.loading => "Searching workspace files...".to_string(),
            Kind::Path => "No matching files or folders.".to_string(),
            Kind::Skill | Kind::Slash if skills_scanning(s) => "Searching workspace skills...".to_string(),
            Kind::Skill => "No skills found. Try / to browse provider commands.".to_string(),
            Kind::Slash => "No matching command.".to_string(),
            Kind::PullRequest if ctx(s).project.is_none() => "Pull requests are not available for this project.".to_string(),
            Kind::PullRequest if prs.loading || prs.key.is_none() => "Finding pull request...".to_string(),
            Kind::PullRequest if prs.failed => "Pull requests could not be read for this project.".to_string(),
            Kind::PullRequest if !m.query().is_empty() => format!("No pull request matches {}.", m.query()),
            Kind::PullRequest => "No pull requests found in this repository.".to_string(),
        };
        return Some(surface(t).px(20.0).pt(14.0).pb(28.0 + 17.0).child(text(message).font(theme::TEXT_XS).color(t.c.secondary_label)));
    }
    let active = s.ui.chat.composer.menu_active.min(list.len() - 1);
    let mut rows = col().w_full();
    for (i, item) in list.into_iter().enumerate() {
        let mut r = row()
            .key(format!("composer-menu-{}", item.id))
            .h(28.0)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(theme::layout::CONTROL_RADIUS)
            .cursor(Cursor::Pointer)
            .on_hover(move |s: &mut App| s.ui.chat.composer.menu_active = i)
            .on_press_tx(super::act_tx(s, move |s: &mut App, tx: &mut Tx| {
                if let Some(m) = open_menu(s) {
                    choose(s, &m, i, tx);
                }
            }));
        if i == active {
            r = r.fill(t.c.accent);
        }
        if let Some(folder) = item.folder {
            r = r.child(if folder { icon(&icons::FOLDER, 16.0).color(t.c.secondary_label) } else { crate::panel::diff::file_icon(&item.id, 16.0, t.dark) });
        }
        if let Some(record) = &item.context
            && let Some((_, glyph)) = super::chips::look(data::s(record, "kind"), Some(record))
        {
            r = r.child(icon(glyph, 16.0).color(t.c.secondary_label));
        }
        if let Some(pr) = &item.pr
            && let Some((tone, glyph)) = super::chips::look("review-comment", Some(&json!({"pullRequest": pr})))
        {
            r = r.child(icon(glyph, 16.0).color(tone));
        }
        r = r
            .child(text(item.label).font(theme::XS_MEDIUM).color(t.c.foreground).truncate().shrink())
            .child(text(item.description).font(theme::TEXT_XS).color(t.c.secondary_label).truncate().grow(1.0).shrink().min_w(0.0));
        rows = rows.child(r);
    }
    let label = match m.kind {
        Kind::Path => "Files and folders",
        Kind::PullRequest => "Pull requests",
        Kind::Slash => "Commands",
        Kind::Skill => "Skills",
    };
    let rows = col().w_full().max_h(280.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(rows.role("listbox").label(label));
    Some(surface(t).px(8.0).pt(8.0).pb(8.0 + 17.0).child(rows).child(el()))
}

/// `@replay` on the showcase: the other replay threads, then a machine this server can link.
pub fn threads_scene(s: &mut App) {
    super::on_replay_showcase(s);
    if let data::Source::Fixture(state) = &mut s.src
        && let Some(config) = state.config.as_mut()
    {
        config["settings"]["linkedEnvironmentsEnabled"] = json!(true);
    }
    s.ui.env.saved.push(crate::env::Saved { id: "replay-box".into(), label: "Replay box".into(), base: "https://replay-box.example.com".into(), bearer: String::new() });
    super::set_text(s, "@replay");
    refresh(s);
}

/// A skill picked from the `$` menu sits in the prompt as a chip.
pub fn skill_pill_scene(s: &mut App) {
    super::on_replay_showcase(s);
    let slug = provider(s, "opencode").and_then(|p| p["models"][0]["slug"].as_str()).unwrap_or("").to_owned();
    draft_mut(s).model = Some(("opencode".into(), slug));
    // This project's workspace snapshot lists its own skills in place of the global ones.
    super::set_text(s, "$custom");
    if let Some(m) = open_menu(s) {
        choose(s, &m, 0, &mut Tx::default());
    }
    draft_mut(s).edit.insert("for a dark theme");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skill_pill_scene_holds_a_skill_chip() {
        let mut s = App::new();
        skill_pill_scene(&mut s);
        let d = draft(&s).unwrap();
        assert!(d.edit.pills().any(|(_, id)| id.starts_with(super::super::chips::SKILL)), "FAILED: {:?}", d.edit.text);
    }

    #[test]
    fn a_workspace_snapshot_replaces_the_machine_wide_lists() {
        let provider = json!({
            "skills": [{"name": "global"}], "slashCommands": [{"name": "init"}],
            "workspaceSnapshots": [{"cwd": "/repo", "checkedAt": "2026-01-01T00:00:00Z", "skills": [{"name": "repo-skill"}], "slashCommands": []}],
        });
        let names = |cwd: Option<&str>, key: &str| resolved(&provider, cwd, key).iter().map(|v| data::s(v, "name").to_owned()).collect::<Vec<_>>();
        assert_eq!(names(Some("/repo"), "skills"), ["repo-skill"]);
        let partial = json!({"skills": [{"name": "global"}], "workspaceSnapshots": [{"cwd": "/repo", "slashCommands": []}]});
        assert_eq!(resolved(&partial, Some("/repo"), "skills").len(), 1, "FAILED: a snapshot without skills keeps the global list");
        assert!(names(Some("/repo"), "slashCommands").is_empty(), "the snapshot's empty list wins");
        assert_eq!(names(Some("/other"), "skills"), ["global"]);
        assert_eq!(names(None, "slashCommands"), ["init"]);

        let at = data::ts(&json!({"t": "2026-01-01T00:00:00Z"}), "t").unwrap();
        assert_eq!(snapshot_status(Some(&provider), "/repo", at + 1000), (true, true, false));
        assert_eq!(snapshot_status(Some(&provider), "/repo", at + SNAPSHOT_TTL_MS), (true, false, false), "stale after the TTL");
        assert_eq!(snapshot_status(Some(&provider), "/other", at), (false, false, false));
        let pending = json!({"workspaceSnapshots": [{"cwd": "/repo", "checkedAt": "2026-01-01T00:00:00Z", "slashCommandsPending": true}]});
        assert_eq!(snapshot_status(Some(&pending), "/repo", at), (false, false, true));
    }

    #[test]
    fn a_cwd_scans_once_per_ttl_and_retries_a_failed_scan_after_a_cooldown() {
        let mut snap = Snapshot::default();
        assert!(snap.plan(false, false, 0), "FAILED: a cwd without a snapshot scans");
        assert!(snap.scanning(false, false));
        snap.request = Some(1);
        assert!(!snap.plan(false, false, 10), "FAILED: never twice while one is in flight");
        snap.request = None;
        assert!(!snap.plan(true, true, 20), "FAILED: a current snapshot needs no scan");
        assert!(!snap.scanning(true, false));
        assert!(!snap.plan(true, false, 1_000), "FAILED: a stale snapshot waits out the TTL from the last scan");
        assert!(snap.plan(true, false, SNAPSHOT_TTL_MS), "FAILED: and scans again after it");

        let mut snap = Snapshot::default();
        assert!(snap.plan(false, false, 0));
        snap.failed(100);
        assert!(!snap.scanning(false, false), "FAILED: a failed scan stops the scanning row");
        assert!(!snap.plan(false, false, 5_000), "FAILED: cools down");
        assert!(snap.plan(false, false, 100 + SNAPSHOT_RETRY_MS));
        snap.failed(20_000);
        assert!(snap.plan(false, false, 20_000 + SNAPSHOT_RETRY_MS));
        snap.failed(40_000);
        assert!(!snap.plan(false, false, 40_000 + SNAPSHOT_RETRY_MS), "FAILED: three failures wait for the TTL");
        assert!(snap.plan(false, false, 30_000 + SNAPSHOT_TTL_MS));

        let mut snap = Snapshot::default();
        assert!(snap.plan(false, false, 0));
        assert!(!snap.plan(true, true, 10));
        assert!(snap.plan(false, false, 20), "FAILED: a lost snapshot scans at once");
    }

    #[test]
    fn at_mentions_list_threads_and_insert_them_as_chips() {
        let mut s = App::new();
        let showcase = s.state().threads.iter().find(|t| t["title"] == "Replay showcase").map(|t| data::s(t, "id").to_owned()).unwrap();
        s.navigate(crate::Route::Thread(showcase.clone()));
        let d = draft_mut(&mut s);
        d.edit = rune::TextEdit::new("see @replay");
        d.edit.set_caret(11);
        let m = open_menu(&s).unwrap();
        let list = items(&s, &m);
        let labels: Vec<&str> = list.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"Approval flow replay") && labels.contains(&"Todo list replay"), "{labels:?}");
        assert!(!labels.contains(&"Replay showcase"), "FAILED: the open thread is left out");
        assert!(list.iter().all(|i| i.description == "Thread"), "FAILED: machines need linked environments on");

        let index = list.iter().position(|i| i.label == "Todo list replay").unwrap();
        choose(&mut s, &m, index, &mut Tx::default());
        let d = draft(&s).unwrap();
        let record = d.contexts.last().unwrap().clone();
        assert_eq!((data::s(&record, "kind"), data::s(&record, "label")), ("thread", "Todo list replay"));
        let (text, sent) = super::super::chips::outgoing(&d.edit, &d.contexts);
        assert_eq!(text, format!("see [Todo list replay](t3-context://v1/thread/{}) ", data::s(&record, "contextId")));
        assert_eq!(sent, [record]);
    }

    #[test]
    fn at_mentions_offer_other_machines_only_when_this_server_links_them() {
        let mut s = App::new();
        s.ui.env.saved.push(crate::env::Saved { id: "env-box".into(), label: "Build box".into(), base: "https://box".into(), bearer: String::new() });
        let labels = |s: &App| {
            let d = draft(s).unwrap();
            let m = open_menu(s).unwrap();
            assert_eq!(d.edit.text, "@box");
            items(s, &m).into_iter().filter(|i| i.description == "Machine").map(|i| i.label).collect::<Vec<_>>()
        };
        let d = draft_mut(&mut s);
        d.edit = rune::TextEdit::new("@box");
        d.edit.set_caret(4);
        assert!(labels(&s).is_empty());
        if let data::Source::Fixture(state) = &mut s.src {
            state.config.as_mut().unwrap()["settings"]["linkedEnvironmentsEnabled"] = json!(true);
        }
        assert_eq!(labels(&s), ["Build box"]);
        let m = open_menu(&s).unwrap();
        let index = items(&s, &m).iter().position(|i| i.label == "Build box").unwrap();
        choose(&mut s, &m, index, &mut Tx::default());
        let record = draft(&s).unwrap().contexts[0].clone();
        assert_eq!((data::s(&record, "kind"), data::s(&record, "environmentId")), ("environment", "env-box"));
    }

    #[test]
    fn at_mentions_list_threads_on_linked_machines_by_machine() {
        let mut s = App::new();
        if let data::Source::Fixture(state) = &mut s.src {
            let config = state.config.get_or_insert_with(|| json!({}));
            config["environment"]["capabilities"]["peerLinks"] = json!(true);
            config["settings"]["linkedEnvironmentsEnabled"] = json!(true);
        }
        s.ui.env.saved.push(crate::env::Saved { id: "env-box".into(), label: "Build box".into(), base: "https://box".into(), bearer: String::new() });
        for (key, id, title) in [("env-box", "box-thread", "Zebra rollout on the box"), ("env-lab", "lab-thread", "Zebra notes in the lab"), ("env-other", "other-thread", "Zebra elsewhere")] {
            let mut theirs = data::empty_state();
            theirs.threads.push(json!({"id": id, "title": title, "projectId": format!("{key}-project")}));
            crate::env::park(&mut s, key, data::Source::Fixture(theirs));
        }
        crate::env::drain(&mut s);
        let d = draft_mut(&mut s);
        d.edit = rune::TextEdit::new("@zebra");
        d.edit.set_caret(6);
        let listed = |s: &App| {
            let m = open_menu(s).unwrap();
            items(s, &m).into_iter().map(|i| (i.label, i.description)).collect::<Vec<_>>()
        };
        assert!(listed(&s).is_empty(), "FAILED: another machine's threads need a link to it");

        // `peerLinks.list` answers: an expired link counts for nothing.
        let mut net = s5_net::Net::in_memory("http://local.test");
        let request = net.reserve_id();
        net.put_reply(request, Ok(json!({"links": [
            {"environmentId": "env-box", "label": "box", "status": "reachable"},
            {"environmentId": "env-lab", "label": "Lab", "status": "unreachable"},
            {"environmentId": "env-other", "label": "Other", "status": "expired"},
        ]})));
        let links = &mut s.ui.chat.composer.mention.links;
        (links.env, links.request) = (Some(s.ui.env.active.clone()), Some(request));
        assert!(poll(&mut s.ui.chat.composer.mention, &mut net));
        let mut found = listed(&s);
        found.sort();
        assert_eq!(found, [("Zebra notes in the lab".to_owned(), "Thread on Lab".to_owned()), ("Zebra rollout on the box".to_owned(), "Thread on Build box".to_owned())], "a known machine goes by its own name");

        let m = open_menu(&s).unwrap();
        let index = items(&s, &m).iter().position(|i| i.label == "Zebra rollout on the box").unwrap();
        choose(&mut s, &m, index, &mut Tx::default());
        let record = draft(&s).unwrap().contexts[0].clone();
        assert_eq!((data::s(&record, "kind"), data::s(&record, "environmentId"), data::s(&record, "threadId")), ("thread", "env-box", "box-thread"));
    }

    #[test]
    fn a_picked_skill_is_a_chip_that_sends_as_its_token() {
        for (text, index) in [("$contr", 0), ("/skill:contr", usize::MAX)] {
            let mut s = App::new();
            let d = draft_mut(&mut s);
            d.model = Some(("opencode".into(), "m".into()));
            d.edit = rune::TextEdit::new(text);
            d.edit.set_caret(text.len());
            let m = open_menu(&s).unwrap();
            let list = items(&s, &m);
            let index = if index == usize::MAX { list.iter().position(|i| i.label == "/skill:contribution-triage").unwrap() } else { index };
            assert_eq!(list[index].id, "$contribution-triage");
            choose(&mut s, &m, index, &mut Tx::default());
            let d = draft(&s).unwrap();
            assert_eq!(d.edit.pills().map(|(_, id)| id.to_owned()).collect::<Vec<_>>(), ["skill:contribution-triage"]);
            assert_eq!(super::super::chips::outgoing(&d.edit, &d.contexts).0, "$contribution-triage ");
        }
    }

    fn slash_labels(model: &str) -> Vec<String> {
        let mut s = App::new();
        s.client_settings.values.insert("planModeEnabled".into(), serde_json::json!(true));
        let d = draft_mut(&mut s);
        d.model = Some((model.into(), "m".into()));
        d.edit = rune::TextEdit::new("/");
        d.edit.set_caret(1);
        let m = open_menu(&s).unwrap();
        items(&s, &m).into_iter().map(|i| i.label).take(5).collect()
    }

    #[test]
    fn slash_commands_rank_by_name_then_description_and_mid_prompt_keep_built_ins() {
        let it = |n: &str, d: &str| Item { id: format!("/{n}"), label: format!("/{n}"), description: d.into(), folder: None, pr: None, context: None };
        let labels = |items: Vec<Item>| items.into_iter().map(|i| i.label).collect::<Vec<_>>();
        let list = vec![it("model", "Switch response model"), it("side", "Ask a side question"), it("review", "Review the diff"), it("mode", "x")];
        assert_eq!(labels(ranked_slash(list, 2, "mod")), ["/mode", "/model"]);
        let list = vec![it("model", "Switch response model"), it("review", "Review the side diff")];
        assert_eq!(labels(ranked_slash(list, 1, "side")), ["/review"]);
        let mut s = App::new();
        s.client_settings.values.insert("planModeEnabled".into(), serde_json::json!(true));
        let d = draft_mut(&mut s);
        d.model = Some(("codex".into(), "m".into()));
        d.edit = rune::TextEdit::new("fix it /");
        d.edit.set_caret(8);
        let m = open_menu(&s).unwrap();
        assert_eq!(m.kind, Kind::Slash);
        assert_eq!(labels(items(&s, &m)), ["/model", "/plan", "/default"]);
        let d = draft_mut(&mut s);
        d.edit = rune::TextEdit::new("/usr/bin");
        d.edit.set_caret(1);
        assert!(open_menu(&s).is_none());
    }

    #[test]
    fn a_hash_opens_pull_requests_only_before_a_name_like_word() {
        let kind = |text: &str| {
            let mut s = App::new();
            let d = draft_mut(&mut s);
            d.edit = rune::TextEdit::new(text);
            d.edit.set_caret(text.len());
            open_menu(&s).map(|m| m.kind)
        };
        assert_eq!(kind("see #"), Some(Kind::PullRequest));
        assert_eq!(kind("see #42"), Some(Kind::PullRequest));
        assert_eq!(kind("#fix-drafts_2"), Some(Kind::PullRequest));
        assert_eq!(kind("#-x"), None);
        assert_eq!(kind("#a.b"), None);
    }

    #[test]
    fn plan_commands_follow_model_only_for_plan_capable_providers() {
        assert_eq!(slash_labels("codex")[..3], ["/model", "/plan", "/default"]);
        assert_eq!(slash_labels("devReplay")[..1], ["/model"]);
        assert!(!slash_labels("codex").iter().any(|l| l == "/side" || l == "/btw"), "side chats start from a thread");
    }

    #[test]
    fn side_chat_commands_follow_the_built_ins_on_a_thread_whose_server_has_side_chats() {
        let mut s = App::new();
        s.client_settings.values.insert("planModeEnabled".into(), serde_json::json!(true));
        let id = s.state().threads.iter().find(|t| t["lineage"]["relationshipToParent"] != "subagent" && t["lineage"]["sideChat"] != true && t["delegatedFrom"].is_null()).map(|t| data::s(t, "id").to_owned()).unwrap();
        if let data::Source::Fixture(state) = &mut s.src {
            state.config.get_or_insert_with(|| json!({}))["environment"]["capabilities"]["sideChats"] = json!(true);
        }
        s.navigate(crate::Route::Thread(id));
        let labels = |s: &mut App, text: &str, caret: usize| {
            let d = draft_mut(s);
            d.model = Some(("codex".into(), "m".into()));
            d.edit = rune::TextEdit::new(text);
            d.edit.set_caret(caret);
            let m = open_menu(s).unwrap();
            items(s, &m).into_iter().map(|i| i.label).collect::<Vec<_>>()
        };
        assert_eq!(labels(&mut s, "/", 1)[..5], ["/model", "/plan", "/default", "/side", "/btw"]);
        assert!(!labels(&mut s, "fix it /", 8).contains(&"/btw".to_owned()));
    }

    #[test]
    fn slash_lists_skills_and_offers_compact_only_alone() {
        let labels = |model: &str, text: &str, thread: bool| {
            let mut s = App::new();
            // Compacting needs a thread with history (compactSlashCommandAvailable).
            if thread {
                super::super::on_replay_showcase(&mut s);
            }
            let d = draft_mut(&mut s);
            d.model = Some((model.into(), "m".into()));
            d.edit = rune::TextEdit::new(text);
            d.edit.set_caret(1);
            let m = open_menu(&s).unwrap();
            items(&s, &m).into_iter().map(|i| i.label).collect::<Vec<_>>()
        };
        assert!(labels("codex", "/", true).contains(&"/compact".to_owned()));
        assert!(!labels("codex", "/ and more", true).contains(&"/compact".to_owned()));
        assert!(!labels("codex", "/", false).contains(&"/compact".to_owned()), "FAILED: a draft has nothing to compact");
        assert!(labels("opencode", "/", false).contains(&"/skill:contribution-triage".to_owned()));
    }
}
