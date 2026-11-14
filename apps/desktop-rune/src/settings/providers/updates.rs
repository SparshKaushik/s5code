//! Web `ProviderUpdatesAction`: one "Update all" for every connected environment this session can
//! manage providers on, updating each driver that can be updated in one click.
use super::DRIVERS;
use crate::kit;
use crate::theme::T;
use crate::{App, env};
use rune::El;
use serde_json::{Value, json};

#[derive(Default)]
pub struct Ui {
    runs: Vec<Run>,
}

struct Run {
    key: String,
    machine: String,
    driver: String,
    instance: String,
    id: u64,
    reply: Option<Result<Value, Value>>,
}

#[derive(Debug, PartialEq)]
pub struct Machine {
    pub key: String,
    pub label: String,
    /// `(driver, instance id)` of each provider to update.
    pub candidates: Vec<(String, String)>,
}

fn driver_name(driver: &str) -> &str {
    DRIVERS.iter().find(|(kind, ..)| *kind == driver).map_or(driver, |(_, label, _)| label)
}

/// `isProviderUpdateCandidate`.
fn candidate(p: &Value) -> bool {
    let compatibility = p["compatibilityAdvisory"]["latestVersionStatus"].as_str();
    p["enabled"] == true
        && !matches!(compatibility, Some("broken" | "unsupported"))
        && p["versionAdvisory"]["status"] == "behind_latest"
        && p["versionAdvisory"]["latestVersion"].is_string()
}

/// `isProviderUpdateActive`.
fn active(p: &Value) -> bool {
    matches!(p["updateState"]["status"].as_str(), Some("queued" | "running"))
}

/// `collectProviderUpdateCandidates`: one per driver, the default instance first, else the newest check.
fn collect(providers: &[Value]) -> Vec<&Value> {
    let mut chosen: Vec<&Value> = Vec::new();
    for p in providers.iter().filter(|p| candidate(p)) {
        let driver = p["driver"].as_str().unwrap_or_default();
        match chosen.iter().position(|c| c["driver"] == driver) {
            None => chosen.push(p),
            Some(i) => {
                let current = chosen[i];
                let checked = |v: &Value| v["checkedAt"].as_str().unwrap_or_default().to_owned();
                let keep_current = p["instanceId"] != driver && (current["instanceId"] == driver || checked(p) < checked(current));
                if !keep_current {
                    chosen[i] = p;
                }
            }
        }
    }
    chosen
}

/// `canOneClickUpdateProviderCandidate`: not already updating, and every behind instance of its
/// driver updates with the same command.
fn one_click(c: &Value, providers: &[Value]) -> bool {
    let updatable = |p: &Value| p["versionAdvisory"]["canUpdate"] == true && p["versionAdvisory"]["updateCommand"].is_string();
    if active(c) || !updatable(c) {
        return false;
    }
    let mut commands = Vec::new();
    for p in providers.iter().filter(|p| p["driver"] == c["driver"] && candidate(p)) {
        if !updatable(p) {
            return false;
        }
        let command = p["versionAdvisory"]["updateCommand"].as_str().unwrap_or_default();
        if !commands.contains(&command) {
            commands.push(command);
        }
    }
    commands.len() == 1
}

fn candidates(providers: &[Value]) -> Vec<(String, String)> {
    collect(providers)
        .into_iter()
        .filter(|c| one_click(c, providers))
        .map(|c| (c["driver"].as_str().unwrap_or_default().to_owned(), c["instanceId"].as_str().unwrap_or_default().to_owned()))
        .collect()
}

pub fn machines(s: &App) -> Vec<Machine> {
    env::keys(s)
        .into_iter()
        .filter_map(|key| {
            let state = env::source(s, &key)?.state();
            if !matches!(state.conn, s5_net::Conn::Open) || !crate::settings::permissions::can_on(s, &key, "providers:manage") {
                return None;
            }
            let providers = state.config.as_ref()?["providers"].as_array()?;
            let candidates = candidates(providers);
            (!candidates.is_empty()).then(|| Machine { label: env::label(s, &key), key, candidates })
        })
        .collect()
}

fn update_all(s: &mut App) {
    if !s.ui.settings.providers_ui.updates.runs.is_empty() {
        return;
    }
    let mut runs = Vec::new();
    for m in machines(s) {
        for (driver, instance) in m.candidates {
            let input = json!({"provider": driver, "instanceId": instance});
            let Some(id) = env::net_of(s, &m.key).map(|net| net.request("server.updateProvider", input)) else { continue };
            runs.push(Run { key: m.key.clone(), machine: m.label.clone(), driver, instance, id, reply: None });
        }
    }
    s.ui.settings.providers_ui.updates.runs = runs;
}

/// `getProviderUpdateRunOutcome`: the failure line, if it failed.
fn outcome(machine: &str, driver: &str, instance: &str, reply: &Result<Value, Value>) -> Option<String> {
    let label = format!("{machine} · {}", driver_name(driver));
    match reply {
        Err(error) => Some(format!("{label}: {}", error["message"].as_str().unwrap_or("Provider update failed."))),
        Ok(value) => {
            let state = value["providers"].as_array().and_then(|ps| ps.iter().find(|p| p["instanceId"] == instance)).map(|p| &p["updateState"]);
            match state {
                Some(state) if state["status"] == "succeeded" => None,
                state => Some(format!(
                    "{label}: {}",
                    state.and_then(|st| st["message"].as_str()).unwrap_or("Provider update did not finish.")
                )),
            }
        }
    }
}

/// `getProviderUpdateRunToastView`: success, or an error listing each failure.
fn toast_view(settled: usize, failures: &[String]) -> Option<(bool, String, String)> {
    if settled == 0 {
        return None;
    }
    if failures.is_empty() {
        let title = if settled == 1 { "Provider updated".to_owned() } else { format!("{settled} providers updated") };
        let description = if settled == 1 { "New sessions will use the updated provider." } else { "New sessions will use the updated providers." };
        return Some((true, title, description.to_owned()));
    }
    let title = if failures.len() < settled {
        format!("{} of {settled} provider updates failed", failures.len())
    } else if settled == 1 {
        "Provider update failed".to_owned()
    } else {
        "Provider updates failed".to_owned()
    };
    Some((false, title, failures.join("\n")))
}

pub fn poll(s: &mut App) -> bool {
    if s.ui.settings.providers_ui.updates.runs.is_empty() {
        return false;
    }
    let mut runs = std::mem::take(&mut s.ui.settings.providers_ui.updates.runs);
    let mut changed = false;
    for run in runs.iter_mut().filter(|r| r.reply.is_none()) {
        match env::net_of(s, &run.key) {
            Some(net) => {
                if let Some(reply) = net.take_reply(run.id) {
                    run.reply = Some(reply);
                    changed = true;
                }
            }
            // The environment went away mid-update: web reads that as interrupted.
            None => {
                run.reply = Some(Err(Value::Null));
                run.id = u64::MAX;
                changed = true;
            }
        }
    }
    if runs.iter().any(|r| r.reply.is_none()) {
        s.ui.settings.providers_ui.updates.runs = runs;
        return changed;
    }
    let settled: Vec<&Run> = runs.iter().filter(|r| r.id != u64::MAX).collect();
    let failures: Vec<String> = settled.iter().filter_map(|r| outcome(&r.machine, &r.driver, &r.instance, r.reply.as_ref().unwrap())).collect();
    if let Some((ok, title, description)) = toast_view(settled.len(), &failures) {
        if ok {
            crate::shell::palette::toast(s, &title, Some(&description));
        } else {
            crate::shell::palette::toast_error(s, &title, Some(&description));
        }
    }
    true
}

/// The header button, or nothing when no environment has a provider to update.
pub fn action(s: &App, t: &T) -> Option<El> {
    let pending = !s.ui.settings.providers_ui.updates.runs.is_empty();
    let machines = machines(s);
    if machines.is_empty() && !pending {
        return None;
    }
    let tip = machines
        .iter()
        .map(|m| format!("{}: {}", m.label, m.candidates.iter().map(|(d, _)| driver_name(d)).collect::<Vec<_>>().join(", ")))
        .collect::<Vec<_>>()
        .join("\n");
    let mut button = kit::Button::new(if pending { "Updating…" } else { "Update all" }).variant("ghost-muted").size("xs").disabled(pending);
    if !pending {
        button = button.on_press(update_all);
    }
    Some(button.build(t).help(tip))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(instance: &str, driver: &str, command: Option<&str>, checked: &str) -> Value {
        json!({
            "instanceId": instance, "driver": driver, "enabled": true, "checkedAt": checked,
            "versionAdvisory": {"status": "behind_latest", "latestVersion": "2.0.0", "canUpdate": command.is_some(), "updateCommand": command},
        })
    }

    #[test]
    fn update_all_picks_one_click_candidates_like_web() {
        let codex = provider("codex", "codex", Some("npm i -g codex"), "2026-11-01");
        let codex_work = provider("codex_work", "codex", Some("npm i -g codex"), "2026-11-02");
        let claude = provider("claude_two", "claudeAgent", Some("npm i -g claude"), "2026-11-01");
        let claude_newer = provider("claude_three", "claudeAgent", Some("npm i -g claude"), "2026-11-03");
        assert_eq!(
            candidates(&[codex_work.clone(), codex.clone(), claude, claude_newer]),
            [("codex".to_owned(), "codex".to_owned()), ("claudeAgent".to_owned(), "claude_three".to_owned())],
            "the default instance wins, else the newest check"
        );
        let other_command = provider("codex_work", "codex", Some("brew upgrade codex"), "2026-11-02");
        assert!(candidates(&[codex.clone(), other_command]).is_empty(), "two update commands need a choice");
        let mut running = codex.clone();
        running["updateState"] = json!({"status": "running"});
        assert!(candidates(&[running]).is_empty());
        let mut broken = codex.clone();
        broken["compatibilityAdvisory"] = json!({"latestVersionStatus": "broken"});
        assert!(candidates(&[broken]).is_empty());
        assert!(candidates(&[provider("codex", "codex", None, "")]).is_empty());
    }

    #[test]
    fn update_all_toasts_read_like_web() {
        let ok = Ok(json!({"providers": [{"instanceId": "codex", "updateState": {"status": "succeeded"}}]}));
        assert_eq!(outcome("Box", "codex", "codex", &ok), None);
        let unchanged = Ok(json!({"providers": [{"instanceId": "codex", "updateState": {"status": "failed", "message": "npm exited 1"}}]}));
        assert_eq!(outcome("Box", "codex", "codex", &unchanged).as_deref(), Some("Box · Codex: npm exited 1"));
        assert_eq!(outcome("Box", "claudeAgent", "x", &Ok(json!({}))).as_deref(), Some("Box · Claude: Provider update did not finish."));
        assert_eq!(toast_view(1, &[]), Some((true, "Provider updated".into(), "New sessions will use the updated provider.".into())));
        assert_eq!(toast_view(3, &[]).unwrap().1, "3 providers updated");
        assert_eq!(toast_view(3, &["a".into()]).unwrap().1, "1 of 3 provider updates failed");
        assert_eq!(toast_view(2, &["a".into(), "b".into()]).unwrap(), (false, "Provider updates failed".into(), "a\nb".into()));
        assert_eq!(toast_view(0, &[]), None);
    }
}
