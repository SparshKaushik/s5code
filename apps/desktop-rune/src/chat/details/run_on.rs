//! The run context menu's "Run on" group: every machine holding the draft's project
//! (`useLogicalProjectEnvironments`), moving the draft between them, and "Auto balance", which
//! picks the machine with the most free CPU and memory (`useDraftEnvironmentRouting`).

use std::collections::HashMap;

use rune::{El, Icon};
use serde_json::{Value, json};

use super::{draft_run, draft_run_mut, draft_worktree, radio_row};
use crate::chat::composer;
use crate::shell::palette;
use crate::shell::sidebar::folders::Grouping;
use crate::theme::T;
use crate::{App, Route, data, env, icons};

/// `HOST_RESOURCES_FRESH_MS`: an older snapshot is neither used nor kept.
const FRESH_MS: i64 = 15_000;

/// How the draft chose its machine (`environmentSelection`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Selection {
    #[default]
    Unset,
    Auto,
    Manual,
}

#[derive(Default)]
pub struct State {
    /// `server.getHostResources` by environment: when it came back, and the snapshot (None: failed).
    resources: HashMap<String, (i64, Option<Value>)>,
    inflight: Vec<(u64, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub env: String,
    pub project: String,
    pub label: String,
}

/// The machines the draft can run on, this machine first, then by label. A scratch draft lists
/// each machine that holds its own scratch project.
pub fn targets(s: &App) -> Vec<Target> {
    let Some(active) = composer::ctx(s).project else { return Vec::new() };
    let scratch = scratch_root(s, &env::of(s, active)).is_some_and(|root| same_root(data::s(active, "workspaceRoot"), &root));
    let grouping = Grouping::read(s);
    let logical = grouping.key(active);
    let mut out: Vec<Target> = Vec::new();
    for project in &s.state().projects {
        let key = env::of(s, project);
        if out.iter().any(|t| t.env == key) {
            continue;
        }
        let matches = if scratch {
            data::s(project, "id") == data::s(active, "id") || scratch_root(s, &key).is_some_and(|root| same_root(data::s(project, "workspaceRoot"), &root))
        } else {
            grouping.key(project) == logical
        };
        if matches {
            let label = Some(env::label(s, &key)).filter(|l| !l.is_empty()).unwrap_or_else(|| env_id(s, &key));
            out.push(Target { env: key, project: data::s(project, "id").to_string(), label });
        }
    }
    let primary = |t: &Target| t.env == env::LOCAL && !s.ui.desktop.local_off;
    out.sort_by(|a, b| primary(b).cmp(&primary(a)).then_with(|| a.label.cmp(&b.label)));
    out
}

fn scratch_root(s: &App, key: &str) -> Option<String> {
    let config = env::source(s, key)?.state().config.as_ref()?;
    config["scratchWorkspaceRoot"].as_str().filter(|r| !r.is_empty()).map(str::to_owned)
}

fn same_root(a: &str, b: &str) -> bool {
    a.trim_end_matches(['/', '\\']) == b.trim_end_matches(['/', '\\'])
}

/// The environment id the client settings key a machine's weight by.
fn env_id(s: &App, key: &str) -> String {
    if key == env::LOCAL { env::local_id(s).unwrap_or_default() } else { key.to_owned() }
}

fn current(s: &App) -> Option<String> {
    composer::ctx(s).project.map(|p| env::of(s, p))
}

/// `EnvironmentMachineIcon` for a machine's server config.
pub fn machine_icon(s: &App, key: &str) -> &'static Icon {
    use crate::settings::connections::{machine_icon, machine_kind};
    let config = env::source(s, key).and_then(|src| src.state().config.as_ref());
    machine_icon(machine_kind(config.and_then(|c| c["settings"]["environmentIcon"].as_str()), config.and_then(|c| c["environment"]["platform"]["machine"].as_str())))
}

fn enabled(s: &App) -> bool {
    crate::settings::value(s, "loadBalancingEnabled") == true
}

fn weight(s: &App, key: &str) -> f64 {
    crate::settings::value(s, "loadBalancingWeights")[env_id(s, key)].as_f64().unwrap_or(50.0)
}

/// Whether the menu offers "Auto balance": a draft whose project lives on more than one
/// machine (`canAutoBalanceEnvironments`) with load balancing on.
fn offers_auto(s: &App) -> bool {
    if draft_run(s).is_none() || !enabled(s) {
        return false;
    }
    let targets = targets(s);
    let scratch = current(s).and_then(|key| scratch_root(s, &key)).zip(composer::ctx(s).project).is_some_and(|(root, p)| same_root(data::s(p, "workspaceRoot"), &root));
    targets.len() > 1 && !scratch
}

/// `automaticEnvironment`: the draft's machine is the balancer's to pick.
pub fn automatic(s: &App) -> bool {
    let Some(run) = draft_run(s) else { return false };
    offers_auto(s)
        && run.selection != Selection::Manual
        && (!composer::has_attachments(s) || run.balanced.is_some())
        && (run.base.is_none() || run.selection == Selection::Auto)
        && draft_worktree(s).is_none()
}

fn needs_balancing(s: &App) -> bool {
    automatic(s) && draft_run(s).is_some_and(|r| r.balanced.is_none())
}

/// Connected machines with a nonzero weight whose provider matches the composer's and can run.
fn candidates(s: &App) -> Vec<String> {
    let selected = composer::selection(s).map(|(instance, _)| instance);
    let driver = selected.as_deref().and_then(|i| composer::provider(s, i)).map(|p| data::s(p, "driver").to_string());
    targets(s)
        .into_iter()
        .map(|t| t.env)
        .filter(|key| env::connected(s, key) && weight(s, key) > 0.0)
        .filter(|key| {
            let config = env::source(s, key).and_then(|src| src.state().config.as_ref());
            let providers = config.and_then(|c| c["providers"].as_array()).map_or(&[][..], Vec::as_slice);
            providers.iter().any(|p| usable(p, selected.as_deref(), driver.as_deref()))
        })
        .collect()
}

fn usable(p: &Value, instance: Option<&str>, driver: Option<&str>) -> bool {
    instance.is_none_or(|i| data::s(p, "instanceId") == i)
        && driver.is_none_or(|d| data::s(p, "driver") == d)
        && p["enabled"] != false
        && p["installed"] != false
        && p["status"] != "error"
        && p["auth"]["status"] != "unauthenticated"
        && p["availability"] != "unavailable"
}

/// `chooseLoadBalancedEnvironment`: the fresh machine with the best weighted free capacity.
fn choose(candidates: &[(String, Option<&Value>, i64, f64)], now: i64) -> Option<String> {
    let mut best: Option<(String, f64)> = None;
    for (key, resources, received, weight) in candidates {
        let Some(r) = resources else { continue };
        let cpu = r["cpuUtilization"].as_f64();
        let (total, available, cores) = (r["totalMemoryBytes"].as_f64().unwrap_or(0.0), r["availableMemoryBytes"].as_f64().unwrap_or(0.0), r["cpuCount"].as_f64().unwrap_or(0.0));
        let Some(cpu) = cpu.filter(|c| *c < 0.95) else { continue };
        if !weight.is_finite() || *weight <= 0.0 || now - received > FRESH_MS || *received > now + 5_000 || total <= 0.0 || cores <= 0.0 {
            continue;
        }
        let memory = available / total;
        if memory <= 0.05 {
            continue;
        }
        let score = weight * cores * (1.0 - cpu) * memory;
        if best.as_ref().is_none_or(|(_, b)| score > *b) {
            best = Some((key.clone(), score));
        }
    }
    best.map(|(key, _)| key)
}

fn pick(s: &App, candidates: &[String], now: i64) -> Option<String> {
    let resources = &s.ui.chat.details.run_on.resources;
    let rows: Vec<_> = candidates.iter().map(|key| {
        let (received, snapshot) = resources.get(key).map_or((0, None), |(at, r)| (*at, r.as_ref()));
        (key.clone(), snapshot, received, weight(s, key))
    }).collect();
    choose(&rows, now)
}

fn pending(s: &App, candidates: &[String]) -> bool {
    s.ui.chat.details.run_on.inflight.iter().any(|(_, key)| candidates.contains(key))
}

/// The "Run on" label while the balancer owns the draft (`autoEnvironmentLabel`).
pub fn auto_label(s: &App) -> Option<&'static str> {
    if !automatic(s) {
        return None;
    }
    if draft_run(s).is_some_and(|r| r.balanced.is_some()) {
        return Some("Auto balance");
    }
    let candidates = candidates(s);
    if pending(s, &candidates) {
        return Some("Checking machines…");
    }
    let resources = &s.ui.chat.details.run_on.resources;
    let failed = pick(s, &candidates, data::now_ms()).is_none() && candidates.iter().any(|k| resources.get(k).is_some_and(|(_, r)| r.is_none()));
    Some(if failed { "Auto balance unavailable" } else { "Auto balance" })
}

/// The run context row's machine mark.
pub fn trigger_icon(s: &App) -> &'static Icon {
    match (auto_label(s), current(s)) {
        (Some(_), _) => &icons::SCALE,
        (None, Some(key)) => machine_icon(s, &key),
        (None, None) => &icons::SERVER,
    }
}

/// Moves the draft, with its text and run context, to `key`'s copy of the project.
fn retarget(s: &mut App, key: &str, selection: Selection, balanced: Option<String>) {
    let Some(target) = targets(s).into_iter().find(|t| t.env == key) else { return };
    let from = composer::draft_key(s, s.frame_route());
    let to = composer::home_key(&target.project);
    if from != to {
        if let Some(draft) = s.ui.chat.composer.drafts.remove(&from) {
            s.ui.chat.composer.drafts.insert(to.clone(), draft);
        }
        if let Some(run) = s.ui.chat.details.drafts.remove(&from) {
            s.ui.chat.details.drafts.insert(to, run);
        }
        s.navigate(Route::Home { project_id: Some(target.project) });
    }
    let run = draft_run_mut(s);
    run.selection = selection;
    run.balanced = balanced;
}

fn choose_machine(s: &mut App, key: &str) {
    s.ui.chat.details.menu = None;
    if current(s).as_deref() == Some(key) && auto_label(s).is_none() {
        return;
    }
    retarget(s, key, Selection::Manual, None);
}

fn choose_auto(s: &mut App) {
    s.ui.chat.details.menu = None;
    if composer::has_attachments(s) {
        palette::toast(s, "Keep attachments on this machine", Some("Remove attachments before choosing automatic routing, then attach them on the selected machine."));
        return;
    }
    s.ui.chat.details.run_on.resources.clear();
    if draft_worktree(s).is_some() {
        s.ui.chat.home.workspace = None;
    }
    let run = draft_run_mut(s);
    run.selection = Selection::Auto;
    run.balanced = None;
    run.base = None;
}

/// Send waits for the balancer (`loadBalancingSendWarning`); true when it held the send.
pub fn holds_send(s: &mut App) -> bool {
    if !needs_balancing(s) {
        return false;
    }
    let candidates = candidates(s);
    if pending(s, &candidates) {
        palette::toast(s, "Checking machine resources", Some("Resource checks are still running. You can choose a machine in the composer."));
    } else {
        palette::toast(s, "Choose a machine to continue", Some("No eligible machine has available resources. Choose a machine in the composer to override."));
    }
    true
}

/// Asks each candidate for its resources while the draft waits on the balancer, and moves the
/// draft once every answer is in.
pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    let now = data::now_ms();
    for (id, key) in std::mem::take(&mut s.ui.chat.details.run_on.inflight) {
        match s.src.net().and_then(|net| net.take_reply(id)) {
            Some(reply) => {
                s.ui.chat.details.run_on.resources.insert(key, (now, reply.ok()));
                changed = true;
            }
            None => s.ui.chat.details.run_on.inflight.push((id, key)),
        }
    }
    if !needs_balancing(s) {
        return changed;
    }
    let candidates = candidates(s);
    for key in &candidates {
        let ui = &s.ui.chat.details.run_on;
        let fresh = ui.resources.get(key).is_some_and(|(at, _)| now - at <= FRESH_MS);
        if fresh || ui.inflight.iter().any(|(_, k)| k == key) {
            continue;
        }
        if let Some(id) = env::net_of(s, key).map(|net| net.request("server.getHostResources", json!({}))) {
            s.ui.chat.details.run_on.inflight.push((id, key.clone()));
            changed = true;
        }
    }
    if !pending(s, &candidates)
        && let Some(key) = pick(s, &candidates, now)
    {
        retarget(s, &key, Selection::Auto, Some(key.clone()));
        changed = true;
    }
    changed
}

/// The menu's "Run on" rows: "Auto balance" when offered, then each machine.
pub fn rows(s: &App, t: &T) -> Vec<El> {
    let auto = auto_label(s);
    let mut rows = Vec::new();
    if offers_auto(s) {
        rows.push(radio_row(t, &icons::SCALE, auto.unwrap_or("Auto balance"), auto.is_some(), choose_auto));
    }
    let here = current(s);
    for target in targets(s) {
        let key = target.env.clone();
        let checked = auto.is_none() && here.as_deref() == Some(key.as_str());
        rows.push(radio_row(t, machine_icon(s, &key), &target.label, checked, move |s: &mut App| choose_machine(s, &key)));
    }
    rows
}

/// This machine's first project and the same repository on "Build box".
pub fn two_machines(s: &mut App) -> (String, String) {
    let identity = json!({"canonicalKey": "github.com/acme/s5code", "rootPath": "/repo"});
    let crate::data::Source::Fixture(local) = env::local_mut(s) else { return (String::new(), String::new()) };
    local.projects[0]["repositoryIdentity"] = identity.clone();
    let here = data::s(&local.projects[0], "id").to_string();
    let mut remote = data::empty_state();
    remote.conn = s5_net::Conn::Open;
    remote.shell_ready = true;
    remote.config = local.config.clone();
    remote.projects = vec![json!({"id": "box-project", "title": "s5code", "workspaceRoot": "/srv/s5code", "repositoryIdentity": identity})];
    s.ui.env.saved.push(env::Saved { id: "box".into(), label: "Build box".into(), base: "https://box.example".into(), bearer: String::new() });
    env::park(s, "box", crate::data::Source::Fixture(remote));
    env::drain(s);
    s.navigate(Route::Home { project_id: Some(here.clone()) });
    (here, "box-project".into())
}

/// The run context menu on a project two machines hold, with "Auto balance" on.
pub fn scene(s: &mut App) {
    (crate::shell::SCENES[0].1)(s);
    two_machines(s);
    s.client_settings.values.insert("loadBalancingEnabled".into(), json!(true));
    s.ui.chat.details.menu = Some(super::Menu::RunContext);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(cores: f64, cpu: f64, free: f64) -> Value {
        json!({"cpuCount": cores, "cpuUtilization": cpu, "totalMemoryBytes": 100.0, "availableMemoryBytes": free, "sampledAt": 0})
    }

    #[test]
    fn the_balancer_takes_the_freshest_most_capable_machine() {
        let (big, busy, full) = (snapshot(8.0, 0.5, 50.0), snapshot(16.0, 0.96, 90.0), snapshot(16.0, 0.1, 4.0));
        let rows = |w: f64, at: i64| vec![("a".to_string(), Some(&big), 1_000, 50.0), ("b".to_string(), Some(&busy), 1_000, 50.0), ("c".to_string(), Some(&full), 1_000, 50.0), ("d".to_string(), Some(&big), at, w)];
        assert_eq!(choose(&rows(50.0, 1_000)[..3], 2_000), Some("a".into()), "a busy CPU or full memory never wins");
        assert_eq!(choose(&rows(100.0, 1_000), 2_000), Some("d".into()), "the weight scales the score");
        assert_eq!(choose(&rows(100.0, 1_000), 1_000 + FRESH_MS + 1), None, "stale snapshots are skipped");
        assert_eq!(choose(&rows(0.0, 1_000), 2_000), Some("a".into()), "manual only is never picked");
    }

    #[test]
    fn run_on_lists_every_machine_with_the_project_and_moves_the_draft() {
        let mut s = App::new();
        let (here, there) = two_machines(&mut s);
        let labels: Vec<_> = targets(&s).into_iter().map(|t| (t.env, t.project)).collect();
        assert_eq!(labels, [(env::LOCAL.to_string(), here.clone()), ("box".to_string(), there.clone())], "this machine first");

        let key = composer::draft_key(&s, &s.route.clone());
        s.ui.chat.composer.drafts.insert(key, composer::Draft { edit: rune::TextEdit::new("ship it"), model: Some(("opencode".into(), "default".into())), ..composer::Draft::default() });
        let text = |s: &App| s.ui.chat.composer.drafts.get(&composer::draft_key(s, &s.route)).map(|d| d.edit.text.clone());
        choose_machine(&mut s, "box");
        assert_eq!(s.route, Route::Home { project_id: Some(there.clone()) });
        assert_eq!(current(&s).as_deref(), Some("box"));
        assert_eq!(text(&s).as_deref(), Some("ship it"), "the draft moves with it");
        assert_eq!(draft_run(&s).map(|r| r.selection), Some(Selection::Manual));
        assert_eq!(auto_label(&s), None);

        // With load balancing on, a manual choice still holds; "Auto balance" hands it back.
        s.client_settings.values.insert("loadBalancingEnabled".into(), json!(true));
        assert!(!automatic(&s));
        choose_auto(&mut s);
        assert!(automatic(&s) && needs_balancing(&s));
        assert_eq!(candidates(&s), [env::LOCAL.to_string(), "box".to_string()]);
        assert!(holds_send(&mut s), "send waits for the pick");

        let now = data::now_ms();
        s.ui.chat.details.run_on.resources.insert(env::LOCAL.into(), (now, Some(snapshot(16.0, 0.1, 80.0))));
        s.ui.chat.details.run_on.resources.insert("box".into(), (now, Some(snapshot(4.0, 0.1, 80.0))));
        poll(&mut s);
        assert_eq!(s.route, Route::Home { project_id: Some(here) }, "the roomier machine takes the draft");
        assert_eq!(draft_run(&s).map(|r| (r.selection, r.balanced)), Some((Selection::Auto, Some(env::LOCAL.to_string()))));
        assert_eq!(auto_label(&s), Some("Auto balance"));
        assert_eq!(text(&s).as_deref(), Some("ship it"));
        assert!(!holds_send(&mut s));

        // A machine weighted "Manual only" is never a candidate; a failed check says so.
        choose_auto(&mut s);
        s.client_settings.values.insert("loadBalancingWeights".into(), json!({"box": 0}));
        assert_eq!(candidates(&s), [env::LOCAL.to_string()]);
        s.ui.chat.details.run_on.resources.insert(env::LOCAL.into(), (data::now_ms(), None));
        assert_eq!(auto_label(&s), Some("Auto balance unavailable"));
    }

    #[test]
    fn the_scene_offers_auto_balance_and_both_machines() {
        let mut s = App::new();
        scene(&mut s);
        assert!(offers_auto(&s) && automatic(&s));
        assert_eq!(targets(&s).len(), 2);
        assert_eq!(auto_label(&s), Some("Auto balance"), "fixtures never ask for resources");
    }
}
