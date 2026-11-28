//! "Auto balance" in the run context menu's "Run on" group: with load balancing on, a draft whose
//! project lives on several machines starts on the one with the most free CPU and memory
//! (`useDraftEnvironmentRouting`, `useLoadBalancedEnvironment`).

use rune::El;
use serde_json::json;

use super::{draft_run, draft_run_mut, draft_worktree, radio_row};
use crate::chat::composer;
use crate::shell::palette;
use crate::theme::T;
use crate::{App, Route, data, env, icons};

/// How the draft chose its machine (`environmentSelection`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Selection {
    #[default]
    Unset,
    Auto,
    Manual,
}

/// The balancer's last look: resources were being checked, and a check ran since "Auto balance"
/// was chosen. The picking itself is `settings::machines::pick_environment`'s.
#[derive(Default)]
pub struct State {
    checking: bool,
    asked: bool,
}

/// `(environment, project)` for each machine holding the draft's project (shell's `hosts`).
pub(crate) fn holders(s: &App) -> Vec<(String, String)> {
    super::hosts(s)
        .into_iter()
        .filter_map(|id| s.state().projects.iter().find(|p| data::s(p, "id") == id).map(|p| (env::of(s, p), id)))
        .collect()
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

/// The "Run on" label while the balancer owns the draft (`autoEnvironmentLabel`).
pub fn auto_label(s: &App) -> Option<&'static str> {
    if !automatic(s) {
        return None;
    }
    if draft_run(s).is_some_and(|r| r.balanced.is_some()) {
        return Some("Auto balance");
    }
    let ui = &s.ui.chat.details.run_on;
    if ui.checking {
        return Some("Checking machines…");
    }
    Some(if ui.asked { "Auto balance unavailable" } else { "Auto balance" })
}

/// Moves the draft to the balancer's machine with shell's "Run on" move.
fn balance_to(s: &mut App, key: &str) {
    let Some((_, project)) = holders(s).into_iter().find(|(k, _)| k == key) else { return };
    super::move_draft(s, &project);
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
    s.ui.chat.details.run_on = State::default();
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
    if s.ui.chat.details.run_on.checking {
        palette::toast(s, "Checking machine resources", Some("Resource checks are still running. You can choose a machine in the composer."));
    } else {
        palette::toast(s, "Choose a machine to continue", Some("No eligible machine has available resources. Choose a machine in the composer to override."));
    }
    true
}

/// While the draft waits on the balancer, asks the shared picker each poll (it refreshes stale
/// snapshots) and moves the draft once it names a machine.
pub fn poll(s: &mut App) -> bool {
    if !needs_balancing(s) {
        return false;
    }
    let Some(project) = composer::ctx(s).project.map(|p| data::s(p, "id").to_string()) else { return false };
    let instance = composer::selection(s).map(|(instance, _)| instance);
    let picked = crate::settings::machines::pick_environment(s, &project, instance.as_deref());
    let checking = crate::settings::machines::checking(s);
    let before = (s.ui.chat.details.run_on.checking, s.ui.chat.details.run_on.asked);
    let ui = &mut s.ui.chat.details.run_on;
    ui.checking = checking;
    ui.asked |= checking;
    let mut changed = before != (ui.checking, ui.asked);
    if let Some((key, _)) = picked {
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
    use serde_json::Value;

    fn current(s: &App) -> Option<String> {
        composer::ctx(s).project.map(|p| env::of(s, p))
    }

    fn snapshot(cores: f64, cpu: f64, free: f64) -> Value {
        json!({"cpuCount": cores, "cpuUtilization": cpu, "totalMemoryBytes": 100.0, "availableMemoryBytes": free, "sampledAt": 0})
    }

    #[test]
    fn auto_balance_moves_the_draft_to_the_roomiest_machine() {
        let mut s = App::new();
        let there = two_machines(&mut s);
        let key = composer::draft_key(&s, &s.route.clone());
        s.ui.chat.composer.drafts.insert(key, composer::Draft { edit: rune::TextEdit::new("ship it"), model: Some(("opencode".into(), "default".into())), ..composer::Draft::default() });
        let text = |s: &App| s.ui.chat.composer.drafts.get(&composer::draft_key(s, &s.route)).map(|d| d.edit.text.clone());
        assert!(!offers_auto(&s) && auto_label(&s).is_none(), "off until load balancing is on");

        // Settings > Connections > Load balancing writes these keys (settings::machines).
        crate::settings::update(&mut s, "loadBalancingEnabled", json!(true));
        assert!(automatic(&s) && needs_balancing(&s), "a fresh draft is the balancer's");
        assert!(holds_send(&mut s), "send waits for the pick");

        // The snapshots Settings > Connections reads; one picker for both.
        crate::settings::machines::set_resources(&mut s, env::LOCAL, snapshot(4.0, 0.1, 80.0));
        crate::settings::machines::set_resources(&mut s, "box", snapshot(16.0, 0.1, 80.0));
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

        // "Manual only" leaves one machine, so nothing moves; a check that found nothing says so.
        crate::settings::update(&mut s, "loadBalancingWeights", json!({"box": 0}));
        assert!(!poll(&mut s) && needs_balancing(&s), "FAILED: one eligible machine is no pick");
        s.ui.chat.details.run_on.asked = true;
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
