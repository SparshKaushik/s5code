//! Deleting the last thread on a worktree offers to delete the worktree too (web's
//! `useThreadActions` delete): the question follows the delete, and Confirm force-removes it.
//! With the project's "Delete worktrees with deleted threads" rule on, the server does it instead.

use rune::{El, row};
use serde_json::{Value, json};

use crate::shell::palette;
use crate::theme::T;
use crate::{App, data, env};

#[derive(Clone, Debug, PartialEq)]
pub struct Orphan {
    env: String,
    thread: String,
    /// The project's checkout, which owns the worktree.
    cwd: String,
    path: String,
}

#[derive(Default)]
pub struct State {
    /// Waiting on the server's list of threads on the worktree (`shellThreadFamilies`).
    occupancy: Vec<(u64, Orphan)>,
    /// The open "Delete the worktree too?" question.
    ask: Option<Orphan>,
    /// `vcs.removeWorktree`, then its `vcs.refreshStatus` (true).
    removing: Vec<(u64, bool, Orphan)>,
}

/// One worktree written as `/repo/wt` or `/repo/wt/` (`normalizeProjectPathForComparison`).
fn same_path(a: &str, b: &str) -> bool {
    let key = |p: &str| p.trim().trim_end_matches(['/', '\\']).to_owned();
    key(a) == key(b)
}

/// `getOrphanedWorktreePathForThread`: the thread's worktree when no other thread is on it.
fn orphaned(threads: &[Value], id: &str) -> Option<String> {
    let path = threads.iter().find(|t| data::s(t, "id") == id)?["worktreePath"].as_str()?.trim();
    let shared = threads.iter().any(|t| data::s(t, "id") != id && t["worktreePath"].as_str().is_some_and(|p| same_path(p, path)));
    (!path.is_empty() && !shared).then(|| path.to_owned())
}

/// `formatWorktreePathForDisplay`: the worktree's folder name.
fn display(path: &str) -> String {
    let trimmed = path.trim();
    let normalized = trimmed.replace('\\', "/");
    let last = normalized.trim_end_matches('/').rsplit('/').next().unwrap_or_default().trim();
    if last.is_empty() { trimmed.to_owned() } else { last.to_owned() }
}

/// `ThreadWorktreeIndicator`'s tooltip on a sidebar row's worktree icon.
pub fn worktree_tip(path: &str, branch: &str) -> Option<String> {
    (!path.trim().is_empty()).then(|| format!("Worktree: {} ({branch})", display(path)))
}

/// Called before thread `id` is deleted, while the threads still list it.
pub fn before_delete(s: &mut App, id: &str) {
    let key = env::owner(s, id);
    if key == s.ui.env.active && !super::grants(s, "source-control:write") {
        return;
    }
    let Some(state) = env::source(s, &key).map(|src| src.state()) else { return };
    let Some(path) = orphaned(&state.threads, id) else { return };
    let project_id = state.threads.iter().find(|t| data::s(t, "id") == id).map(|t| data::s(t, "projectId").to_owned()).unwrap_or_default();
    let Some(project) = state.projects.iter().find(|p| data::s(p, "id") == project_id) else { return };
    let config = state.config.as_ref();
    let scratch = config.map(|c| data::s(c, "scratchWorkspaceRoot")).unwrap_or_default();
    let cwd = data::s(project, "workspaceRoot").to_owned();
    if !scratch.is_empty() && same_path(&cwd, scratch) {
        return;
    }
    if config.is_some_and(|c| crate::settings::storage::worktree_on_delete(&c["settings"], &project_id)) {
        return;
    }
    let families = config.is_some_and(|c| c["shellThreadFamilies"] == true);
    let orphan = Orphan { env: key, thread: id.to_owned(), cwd, path };
    if !families {
        s.ui.chat.details.cleanup.ask = Some(orphan);
        return;
    }
    // A shell without idle subagent children can't prove the worktree is free; the server decides.
    if let Some(request) = env::net_of(s, &orphan.env).map(|net| net.request("orchestration.getWorktreeThreadIds", json!({"worktreePath": orphan.path}))) {
        s.ui.chat.details.cleanup.occupancy.push((request, orphan));
    }
}

/// Only the deleted thread on the worktree; a failed read keeps it.
fn alone(reply: &Result<Value, Value>, thread: &str) -> bool {
    reply.as_ref().is_ok_and(|v| v["threadIds"].as_array().is_some_and(|ids| ids.iter().all(|t| t == thread)))
}

pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    for (request, orphan) in std::mem::take(&mut s.ui.chat.details.cleanup.occupancy) {
        match env::net_of(s, &orphan.env).map(|net| net.take_reply(request)) {
            None => {}
            Some(None) => s.ui.chat.details.cleanup.occupancy.push((request, orphan)),
            Some(Some(reply)) => {
                changed = true;
                if alone(&reply, &orphan.thread) {
                    s.ui.chat.details.cleanup.ask = Some(orphan);
                }
            }
        }
    }
    for (request, refresh, orphan) in std::mem::take(&mut s.ui.chat.details.cleanup.removing) {
        let Some(net) = env::net_of(s, &orphan.env) else { continue };
        match net.take_reply(request) {
            None => s.ui.chat.details.cleanup.removing.push((request, refresh, orphan)),
            Some(Ok(_)) if !refresh => {
                let next = net.request("vcs.refreshStatus", json!({"cwd": orphan.cwd}));
                s.ui.chat.details.cleanup.removing.push((next, true, orphan));
                changed = true;
            }
            Some(Ok(_)) => changed = true,
            Some(Err(e)) => {
                let message = palette::error_message(&e, "An error occurred.");
                if refresh {
                    palette::toast_error_brief(s, "Worktree deleted, but Git status refresh failed", Some(&message));
                } else {
                    palette::toast_error_brief(s, "Failed to delete worktree", Some(&format!("Could not remove {}. {message}", display(&orphan.path))));
                }
                changed = true;
            }
        }
    }
    changed
}

fn remove(s: &mut App) {
    let Some(orphan) = s.ui.chat.details.cleanup.ask.take() else { return };
    let params = json!({"cwd": orphan.cwd, "path": orphan.path, "force": true});
    if let Some(request) = env::net_of(s, &orphan.env).map(|net| net.request("vcs.removeWorktree", params)) {
        s.ui.chat.details.cleanup.removing.push((request, false, orphan));
    }
}

/// The destructive `dialogs.confirm`, split as ConfirmDialogHost splits its lines.
pub fn dialog(s: &App, t: &T, out: &mut Vec<El>) {
    let Some(orphan) = &s.ui.chat.details.cleanup.ask else { return };
    let close = |s: &mut App| s.ui.chat.details.cleanup.ask = None;
    let cancel = crate::kit::Button::new("Cancel").variant("outline").on_press(close).build(t);
    let confirm = crate::kit::Button::new("Confirm").variant("destructive").on_press(remove).build(t);
    crate::kit::Dialog::new("worktree-delete-too", "Delete the worktree too?")
        .alert()
        .width(448.0)
        .open(true)
        .description(format!("This thread is the only one linked to this worktree:\n{}", display(&orphan.path)))
        .child(row().w_full().gap(8.0).justify_end().child(cancel).child(confirm))
        .on_close(close)
        .build(t, out);
}

/// The question after deleting the last thread on a worktree.
pub fn scene(s: &mut App) {
    (crate::shell::SCENES[0].1)(s);
    let cwd = s.state().projects.first().map(|p| data::s(p, "workspaceRoot").to_owned()).unwrap_or_default();
    s.ui.chat.details.cleanup.ask = Some(Orphan { env: s.ui.env.active.clone(), thread: "t".into(), cwd, path: "/home/me/.t3/worktrees/s5code/fix-login-redirect".into() });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_worktree_no_other_thread_uses_is_offered() {
        let threads = vec![
            json!({"id": "a", "worktreePath": "/repo/wt/"}),
            json!({"id": "b", "worktreePath": "/repo/wt"}),
            json!({"id": "c", "worktreePath": " /repo/solo "}),
            json!({"id": "d", "worktreePath": null}),
        ];
        assert_eq!(orphaned(&threads, "a"), None, "FAILED: a trailing slash is the same worktree");
        assert_eq!(orphaned(&threads, "c").as_deref(), Some("/repo/solo"));
        assert_eq!(orphaned(&threads, "d"), None);
        assert_eq!(orphaned(&threads, "gone"), None);
        assert_eq!(display("C:\\wt\\fix-login\\"), "fix-login");
        assert_eq!(display("/"), "/");
        assert_eq!(worktree_tip("/repo/.t3/wt/fix-login/", "fix/login").as_deref(), Some("Worktree: fix-login (fix/login)"));
        assert_eq!(worktree_tip(" ", "main"), None);
    }

    #[test]
    fn the_server_list_keeps_a_worktree_a_subagent_still_uses() {
        assert!(alone(&Ok(json!({"threadIds": ["t"]})), "t"));
        assert!(alone(&Ok(json!({"threadIds": []})), "t"));
        assert!(!alone(&Ok(json!({"threadIds": ["t", "child"]})), "t"), "FAILED: another thread is on it");
        assert!(!alone(&Err(json!({"message": "boom"})), "t"), "FAILED: a failed read keeps the worktree");
    }

    #[test]
    fn deleting_the_last_thread_on_a_worktree_asks_unless_the_server_cleans_up() {
        let mut s = App::new();
        let project = s.state().projects[0].clone();
        let pid = data::s(&project, "id").to_owned();
        if let crate::data::Source::Fixture(state) = &mut s.src {
            state.threads.push(json!({"id": "wt-thread", "projectId": pid, "worktreePath": "/srv/worktrees/fix"}));
            if let Some(config) = state.config.as_mut() {
                config["shellThreadFamilies"] = json!(false);
            }
        }
        before_delete(&mut s, "wt-thread");
        let asked = s.ui.chat.details.cleanup.ask.clone().expect("FAILED: the last thread on a worktree asks");
        assert_eq!((asked.path.as_str(), asked.cwd.as_str()), ("/srv/worktrees/fix", data::s(&project, "workspaceRoot")));

        s.ui.chat.details.cleanup.ask = None;
        if let crate::data::Source::Fixture(state) = &mut s.src {
            if let Some(config) = state.config.as_mut() {
                config["settings"]["worktreeCleanup"] = json!({"mode": "custom", "rules": {"worktreeOnDelete": true}});
            }
        }
        before_delete(&mut s, "wt-thread");
        assert!(s.ui.chat.details.cleanup.ask.is_none(), "FAILED: the server deletes it under worktreeOnDelete");
    }
}
