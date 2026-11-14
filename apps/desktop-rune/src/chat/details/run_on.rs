//! "Auto balance" in the run context menu's "Run on" group: with load balancing on, a draft whose
//! project lives on several machines starts on the one with the most free CPU and memory
//! (`useDraftEnvironmentRouting`, `useLoadBalancedEnvironment`).

use std::collections::HashMap;

use rune::El;
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

/// `(environment, project)` for each machine holding the draft's logical project
/// (`useLogicalProjectEnvironments`).
fn holders(s: &App) -> Vec<(String, String)> {
    let Some(active) = composer::ctx(s).project else { return Vec::new() };
    let grouping = Grouping::read(s);
    let logical = grouping.key(active);
    let mut out: Vec<(String, String)> = Vec::new();
    for project in &s.state().projects {
        let key = env::of(s, project);
        if !out.iter().any(|(k, _)| *k == key) && grouping.key(project) == logical {
            out.push((key, data::s(project, "id").to_string()));
        }
    }
    out
}

fn current(s: &App) -> Option<String> {
    composer::ctx(s).project.map(|p| env::of(s, p))
}

/// The environment id the client settings key a machine's weight by.
fn env_id(s: &App, key: &str) -> String {
    if key == env::LOCAL { env::local_id(s).unwrap_or_default() } else { key.to_owned() }
}

fn weight(s: &App, key: &str) -> f64 {
    crate::settings::value(s, "loadBalancingWeights")[env_id(s, key)].as_f64().unwrap_or(50.0)
}

/// Whether the menu offers "Auto balance": load balancing is on and the draft's project lives on
/// more than one machine and is not the scratch project (`canAutoBalanceEnvironments`).
fn offers_auto(s: &App) -> bool {
    let Some(project) = composer::ctx(s).project.filter(|_| draft_run(s).is_some()) else { return false };
    let scratch = palette::actions::is_scratch_project(s, project);
    crate::settings::value(s, "loadBalancingEnabled") == true && !scratch && holders(s).len() > 1
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
    holders(s)
        .into_iter()
        .map(|(key, _)| key)
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
        let (total, available, cores) = (r["totalMemoryBytes"].as_f64().unwrap_or(0.0), r["availableMemoryBytes"].as_f64().unwrap_or(0.0), r["cpuCount"].as_f64().unwrap_or(0.0));
        let Some(cpu) = r["cpuUtilization"].as_f64().filter(|c| *c < 0.95) else { continue };
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
    let rows: Vec<_> = candidates
        .iter()
        .map(|key| {
            let (received, snapshot) = resources.get(key).map_or((0, None), |(at, r)| (*at, r.as_ref()));
            (key.clone(), snapshot, received, weight(s, key))
        })
        .collect();
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

/// Moves the draft, with its text and run context, to the balancer's machine.
fn balance_to(s: &mut App, key: &str) {
    let Some((_, project)) = holders(s).into_iter().find(|(k, _)| k == key) else { return };
    let from = composer::draft_key(s, s.frame_route());
    let to = composer::home_key(&project);
    if from != to {
        if let Some(draft) = s.ui.chat.composer.drafts.remove(&from) {
            s.ui.chat.composer.drafts.insert(to.clone(), draft);
        }
        if let Some(run) = s.ui.chat.details.drafts.remove(&from) {
            s.ui.chat.details.drafts.insert(to, run);
        }
        s.navigate(Route::Home { project_id: Some(project) });
    }
    let run = draft_run_mut(s);
    run.selection = Selection::Auto;
    run.balanced = Some(key.to_owned());
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

/// Choosing this machine while the balancer owns the draft keeps it here (`environmentSelection: "manual"`).
pub fn keep_here(s: &mut App) {
    s.ui.chat.details.menu = None;
    if automatic(s) {
        let run = draft_run_mut(s);
        run.selection = Selection::Manual;
        run.balanced = None;
    }
}

/// Send waits for the balancer (`loadBalancingSendWarning`); true when it held the send.
pub fn holds_send(s: &mut App) -> bool {
    if !needs_balancing(s) {
        return false;
    }
    if pending(s, &candidates(s)) {
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
        balance_to(s, &key);
        changed = true;
    }
    changed
}

/// The menu's "Auto balance" row, when offered.
pub fn auto_row(s: &App, t: &T) -> Option<El> {
    let label = auto_label(s);
    offers_auto(s).then(|| radio_row(t, &icons::SCALE, label.unwrap_or("Auto balance"), label.is_some(), choose_auto))
}

/// This machine's first project and the same repository on "Build box".
pub fn two_machines(s: &mut App) -> String {
    let identity = json!({"canonicalKey": "github.com/acme/s5code", "rootPath": "/repo"});
    let crate::data::Source::Fixture(local) = env::local_mut(s) else { return String::new() };
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
    s.navigate(Route::Home { project_id: Some(here) });
    "box-project".into()
}

/// The run context menu on a project two machines hold, with load balancing on.
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
        let rows = |w: f64| vec![("a".to_string(), Some(&big), 1_000, 50.0), ("b".to_string(), Some(&busy), 1_000, 50.0), ("c".to_string(), Some(&full), 1_000, 50.0), ("d".to_string(), Some(&big), 1_000, w)];
        assert_eq!(choose(&rows(50.0)[..3], 2_000), Some("a".into()), "a busy CPU or full memory never wins");
        assert_eq!(choose(&rows(100.0), 2_000), Some("d".into()), "the weight scales the score");
        assert_eq!(choose(&rows(100.0), 1_000 + FRESH_MS + 1), None, "stale snapshots are skipped");
        assert_eq!(choose(&rows(0.0), 2_000), Some("a".into()), "manual only is never picked");
    }

    #[test]
    fn auto_balance_moves_the_draft_to_the_roomiest_machine() {
        let mut s = App::new();
        let there = two_machines(&mut s);
        let key = composer::draft_key(&s, &s.route.clone());
        s.ui.chat.composer.drafts.insert(key, composer::Draft { edit: rune::TextEdit::new("ship it"), model: Some(("opencode".into(), "default".into())), ..composer::Draft::default() });
        let text = |s: &App| s.ui.chat.composer.drafts.get(&composer::draft_key(s, &s.route)).map(|d| d.edit.text.clone());
        assert!(!offers_auto(&s) && auto_label(&s).is_none(), "off until load balancing is on");

        s.client_settings.values.insert("loadBalancingEnabled".into(), json!(true));
        assert!(automatic(&s) && needs_balancing(&s), "a fresh draft is the balancer's");
        assert_eq!(candidates(&s), [env::LOCAL.to_string(), "box".to_string()]);
        assert!(holds_send(&mut s), "send waits for the pick");

        let now = data::now_ms();
        s.ui.chat.details.run_on.resources.insert(env::LOCAL.into(), (now, Some(snapshot(4.0, 0.1, 80.0))));
        s.ui.chat.details.run_on.resources.insert("box".into(), (now, Some(snapshot(16.0, 0.1, 80.0))));
        poll(&mut s);
        assert_eq!(s.route, Route::Home { project_id: Some(there) }, "the roomier machine takes the draft");
        assert_eq!(current(&s).as_deref(), Some("box"));
        assert_eq!(text(&s).as_deref(), Some("ship it"), "the draft moves with it");
        assert_eq!(draft_run(&s).map(|r| (r.selection, r.balanced)), Some((Selection::Auto, Some("box".to_string()))));
        assert_eq!(auto_label(&s), Some("Auto balance"));
        assert!(!holds_send(&mut s));

        // A machine picked by hand holds until "Auto balance" hands the draft back.
        keep_here(&mut s);
        assert_eq!(draft_run(&s).map(|r| (r.selection, r.balanced)), Some((Selection::Manual, None)));
        assert!(!automatic(&s) && auto_label(&s).is_none());
        choose_auto(&mut s);
        assert!(needs_balancing(&s));

        // "Manual only" drops a machine; a failed check says the balancer has nowhere to go.
        s.client_settings.values.insert("loadBalancingWeights".into(), json!({"box": 0}));
        assert_eq!(candidates(&s), [env::LOCAL.to_string()]);
        s.ui.chat.details.run_on.resources.insert(env::LOCAL.into(), (data::now_ms(), None));
        assert_eq!(auto_label(&s), Some("Auto balance unavailable"));

        // Attachments keep a hand-picked machine.
        draft_run_mut(&mut s).selection = Selection::Manual;
        let key = composer::draft_key(&s, &s.route);
        s.ui.chat.composer.drafts.get_mut(&key).unwrap().attachments.push("/tmp/a.png".into());
        choose_auto(&mut s);
        assert_eq!(draft_run(&s).map(|r| r.selection), Some(Selection::Manual));
    }

    #[test]
    fn the_scene_offers_auto_balance() {
        let mut s = App::new();
        scene(&mut s);
        assert!(offers_auto(&s) && automatic(&s));
        assert_eq!(auto_label(&s), Some("Auto balance"), "fixtures never ask for resources");
    }
}
