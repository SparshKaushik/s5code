//! Several models from one new-thread draft (useMultipleModelSend): Shift-picking models in the
//! picker gathers them, and Send starts one background thread per model, each in its own
//! worktree from the draft's base branch.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::{Outgoing, attachments, chips, ctx, deliver, draft, draft_key, interaction_mode, model_name, provider, restored, runtime_mode, selection, stored_draft};
use crate::shell::palette;
use crate::{App, Route, data};

/// Outgoing keys of fan-out launches: `fanout:{batch}:{index}`.
const KEY: &str = "fanout:";

#[derive(Default)]
pub struct State {
    /// The models picked together, by draft key (`draftFanoutSelectionsAtom`).
    picks: HashMap<String, Vec<(String, String)>>,
    batches: Vec<Batch>,
    next: u64,
}

/// One send: its draft and what it sent, and each model's outcome once the server answers.
struct Batch {
    id: u64,
    key: String,
    draft: Value,
    models: Vec<((String, String), Option<Result<(), String>>)>,
}

fn supported(s: &App) -> bool {
    s.state().config.as_ref().is_some_and(|c| c["environment"]["capabilities"]["requiredWorktreeBootstrap"] == true)
}

/// Shift-picking adds models only on a new-thread draft whose server can bootstrap worktrees.
pub fn enabled(s: &App) -> bool {
    !matches!(s.frame_route(), Route::Thread(_)) && supported(s)
}

pub fn picks(s: &App) -> Option<&Vec<(String, String)>> {
    s.ui.chat.composer.fanout.picks.get(&draft_key(s, s.frame_route()))
}

/// The draft runs each model in a new worktree (BranchToolbar's `forceNewWorktree`).
pub fn forces_worktree(s: &App) -> bool {
    picks(s).is_some()
}

/// `toggleModel`: one model left goes back to being the draft's single model.
pub fn toggle(s: &mut App, instance: &str, model: &str) {
    let key = draft_key(s, s.frame_route());
    let mut next = picks(s).cloned().or_else(|| selection(s).map(|m| vec![m])).unwrap_or_default();
    let before = next.len();
    next.retain(|(i, m)| !(i == instance && m == model));
    if next.len() == before {
        next.push((instance.to_owned(), model.to_owned()));
    }
    let picks = &mut s.ui.chat.composer.fanout.picks;
    if next.len() > 1 {
        picks.insert(key, next);
        return;
    }
    picks.remove(&key);
    if let Some(remaining) = next.pop() {
        super::draft_mut(s).model = Some(remaining);
    }
}

/// A plain pick replaces the gathered models.
pub fn clear(s: &mut App) {
    let key = draft_key(s, s.frame_route());
    s.ui.chat.composer.fanout.picks.remove(&key);
}

/// The model chip's label and tooltip: two names, then "N more".
pub fn chip_label(s: &App) -> Option<(String, String)> {
    let names: Vec<String> = picks(s)?.iter().map(|(i, m)| model_name(s, i, m)).collect();
    Some(label(&names))
}

fn label(names: &[String]) -> (String, String) {
    if names.is_empty() {
        return ("Choose models".into(), "Choose models".into());
    }
    let mut shown = names.iter().take(2).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > 2 {
        shown.push_str(&format!(", {} more", names.len() - 2));
    }
    (shown, names.join(", "))
}

/// Sends the draft to every picked model; false when the draft has no picks.
pub(super) fn send(s: &mut App) -> bool {
    let Some(picks) = picks(s).cloned() else { return false };
    let key = draft_key(s, s.frame_route());
    if !supported(s) {
        crate::chat::timeline::error_banner::set_local(s, &key, Some("Update this server before starting multiple models."));
        return true;
    }
    let project = ctx(s).project.map(|p| data::s(p, "id").to_string());
    let base = crate::chat::details::draft_base(s);
    let (Some(project), Some((branch, from_origin)), false) = (project, base, picks.is_empty()) else {
        palette::toast(s, "Choose models and a base branch", Some("Multiple models need a new thread in a Git project. Each gets its own worktree."));
        return true;
    };
    if let Some((_, model)) = picks.iter().find(|(i, _)| !provider(s, i).is_some_and(|p| p["enabled"] == true && data::s(p, "status") == "ready")) {
        crate::chat::timeline::error_banner::set_local(s, &key, Some(&format!("Provider for {model} is unavailable.")));
        return true;
    }
    let Some(d) = draft(s) else { return true };
    let snapshot = stored_draft(d);
    let (text, contexts) = chips::outgoing(&d.edit, &d.contexts);
    let text = text.trim().to_string();
    let paths: Vec<String> = d.attachments.iter().filter(|p| chips::keeps_attachment(p, &contexts)).cloned().collect();
    if text.is_empty() && paths.is_empty() && contexts.is_empty() {
        return true;
    }
    if let Err(message) = paths.iter().map(|p| attachments::stage(p)).collect::<Result<Vec<_>, _>>() {
        crate::chat::timeline::error_banner::set_local(s, &key, Some(&message));
        return true;
    }
    let (runtime, interaction) = (runtime_mode(s), interaction_mode(s));
    let mut workspace = json!({"type": "worktree", "baseRef": branch});
    if from_origin {
        workspace["startFromOrigin"] = json!(true);
    }
    crate::chat::details::take_draft_workspace(s);
    crate::chat::timeline::error_banner::set_local(s, &key, None);
    let id = s.ui.chat.composer.fanout.next;
    s.ui.chat.composer.fanout.next += 1;
    let Some(net) = s.src.net() else { return true };
    let composer = &mut s.ui.chat.composer;
    for (n, (instance, model)) in picks.iter().enumerate() {
        let thread_id = format!("thread:project:{project}:{}", s5_net::uuid());
        let message_id = s5_net::uuid();
        let mut payload = json!({
            "commandId": s5_net::uuid(), "creationSource": "web", "threadId": thread_id, "projectId": project,
            "title": "New thread", "generateTitle": true, "modelSelection": {"instanceId": instance, "model": model},
            "runtimeMode": runtime, "interactionMode": interaction, "workspaceStrategy": workspace,
            "initialMessage": {"messageId": message_id, "text": text, "attachments": []},
        });
        if !contexts.is_empty() {
            payload["initialMessage"]["context"] = json!({"version": 1, "records": contexts});
        }
        let outgoing = Outgoing { key: format!("{KEY}{id}:{n}"), draft: Value::Null };
        // Staged above, so each restage succeeds; every thread stores its own copy.
        let staged: Vec<_> = paths.iter().filter_map(|p| attachments::stage(p).ok()).collect();
        if staged.is_empty() {
            deliver(net, composer, payload, Some(thread_id), json!([]), outgoing);
        } else {
            composer.uploads.push(attachments::start(net, &thread_id, &message_id, staged, payload, Some(thread_id.clone()), outgoing));
        }
    }
    composer.fanout.batches.push(Batch { id, key, draft: snapshot, models: picks.into_iter().map(|m| (m, None)).collect() });
    let d = super::draft_mut(s);
    d.edit.clear();
    d.attachments.clear();
    d.contexts.clear();
    true
}

/// Takes a fan-out launch's outcome; false when `key` is not one.
pub(super) fn settle(state: &mut State, key: &str, outcome: Result<(), String>) -> bool {
    let Some((batch, n)) = key.strip_prefix(KEY).and_then(|k| k.split_once(':')) else { return false };
    let (Ok(batch), Ok(n)) = (batch.parse::<u64>(), n.parse::<usize>()) else { return false };
    if let Some(slot) = state.batches.iter_mut().find(|b| b.id == batch).and_then(|b| b.models.get_mut(n)) {
        slot.1 = Some(outcome);
    }
    true
}

/// Reports each finished batch: the threads started, and the models that failed, which stay
/// picked with the prompt back in the draft (or a "Restore prompt" toast once it moved on).
pub(super) fn finish(s: &mut App) -> bool {
    let batches = &mut s.ui.chat.composer.fanout.batches;
    let Some(at) = batches.iter().position(|b| b.models.iter().all(|(_, o)| o.is_some())) else { return false };
    let batch = batches.remove(at);
    let started = batch.models.iter().filter(|(_, o)| matches!(o, Some(Ok(())))).count();
    let failed: Vec<((String, String), String)> = batch.models.into_iter().filter_map(|(m, o)| o?.err().map(|e| (m, e))).collect();
    if started > 0 {
        palette::toast(s, &format!("Started {started} {} in background", if started == 1 { "thread" } else { "threads" }), None);
    }
    let Some(((_, model), error)) = failed.first().cloned() else { return true };
    s.ui.chat.composer.fanout.picks.insert(batch.key.clone(), failed.into_iter().map(|(m, _)| m).collect());
    if restore(s, &batch.key, &batch.draft) {
        palette::toast_error(s, &format!("Could not start {model}"), Some(&error));
        return true;
    }
    let (key, draft) = (batch.key, batch.draft);
    let action: palette::ToastAction = ("Restore prompt".into(), true, std::rc::Rc::new(move |s: &mut App, _: &mut rune::Tx| {
        if restore(s, &key, &draft) {
            if let Some(project) = key.strip_prefix("home:") {
                s.navigate(Route::Home { project_id: Some(project.to_owned()) });
            }
        } else {
            palette::toast(s, "A background prompt could not be sent", Some("Return to the original draft and send or clear its current prompt before restoring."));
        }
    }));
    palette::toast_with(s, "A background prompt could not be sent", Some("Your newer draft is unchanged. Restore the failed prompt when this composer is empty."), vec![action]);
    true
}

/// Puts a sent draft back when its composer is still empty.
fn restore(s: &mut App, key: &str, draft: &Value) -> bool {
    let blank = s.ui.chat.composer.drafts.get(key).is_none_or(|d| d.edit.text.trim().is_empty() && d.attachments.is_empty());
    if !blank {
        return false;
    }
    if let Some(d) = restored(&json!({"draft": draft})).remove("draft") {
        let model = s.ui.chat.composer.drafts.get(key).and_then(|d| d.model.clone());
        s.ui.chat.composer.drafts.insert(key.to_owned(), super::Draft { model: model.or(d.model.clone()), ..d });
    }
    true
}

/// A new-thread draft with three models picked and the run context menu open.
pub fn scene(s: &mut App) {
    (crate::shell::SCENES[0].1)(s);
    let project = s.state().projects.iter().map(|p| data::s(p, "id").to_string()).next().unwrap_or_default();
    s.navigate(Route::Home { project_id: Some(project) });
    let models: Vec<(String, String)> = super::providers(s).iter().filter(|p| p["enabled"] == true).flat_map(|p| p["models"].as_array().into_iter().flatten().take(2).map(move |m| (data::s(p, "instanceId").to_string(), data::s(m, "slug").to_string()))).take(3).collect();
    for (instance, model) in models {
        toggle(s, &instance, &model);
    }
    super::draft_mut(s).edit = rune::TextEdit::new("Fix the flaky login redirect test");
    s.ui.chat.details.menu = Some(crate::chat::details::Menu::RunContext);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chip_names_two_models_then_counts_the_rest() {
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(label(&[]).0, "Choose models");
        assert_eq!(label(&names(&["GPT-5", "Opus"])), ("GPT-5, Opus".into(), "GPT-5, Opus".into()));
        assert_eq!(label(&names(&["GPT-5", "Opus", "Grok", "Kimi"])), ("GPT-5, Opus, 2 more".into(), "GPT-5, Opus, Grok, Kimi".into()));
    }

    #[test]
    fn shift_picks_gather_models_and_the_last_one_stays_the_drafts_model() {
        let mut s = App::new();
        let project = s.state().projects.iter().map(|p| data::s(p, "id").to_string()).next().unwrap();
        s.navigate(Route::Home { project_id: Some(project) });
        let first = selection(&s).unwrap();
        toggle(&mut s, "opencode", "a");
        assert_eq!(picks(&s).cloned(), Some(vec![first.clone(), ("opencode".into(), "a".into())]), "the current model joins the first extra pick");
        assert!(forces_worktree(&s));
        toggle(&mut s, "opencode", "b");
        toggle(&mut s, &first.0, &first.1);
        assert_eq!(picks(&s).map(Vec::len), Some(2));
        toggle(&mut s, "opencode", "a");
        assert_eq!(picks(&s), None, "one model left is a single pick again");
        assert_eq!(selection(&s), Some(("opencode".into(), "b".into())));
    }

    #[test]
    fn a_batch_reports_started_threads_and_keeps_the_failed_models_with_the_prompt() {
        let mut s = App::new();
        let key = "home:p1".to_string();
        let draft = json!({"prompt": "ship it", "pills": [], "attachments": []});
        let models = vec![(("opencode".into(), "a".into()), None), (("opencode".into(), "b".into()), None), (("codex".into(), "c".into()), None)];
        s.ui.chat.composer.fanout.batches.push(Batch { id: 7, key: key.clone(), draft, models });
        let state = &mut s.ui.chat.composer.fanout;
        assert!(settle(state, "fanout:7:0", Ok(())));
        assert!(settle(state, "fanout:7:2", Err("no auth".into())));
        assert!(!settle(state, "home:p1", Ok(())), "other sends are not ours");
        assert!(!finish(&mut s), "waits for every model");
        assert!(settle(&mut s.ui.chat.composer.fanout, "fanout:7:1", Ok(())));
        assert!(finish(&mut s));
        assert_eq!(s.ui.chat.composer.fanout.picks.get(&key).cloned(), Some(vec![("codex".to_string(), "c".to_string())]));
        assert_eq!(s.ui.chat.composer.drafts.get(&key).map(|d| d.edit.text.clone()).as_deref(), Some("ship it"), "the empty composer gets the prompt back");
        assert!(s.ui.chat.composer.fanout.batches.is_empty());
    }
}
