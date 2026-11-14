//! The new-thread draft that `/` opens (routes/_chat.index.tsx starts a draft in the most recent
//! project): the empty-conversation prompt. Sending
//! from the composer launches the thread and navigates to it.

use rune::{Cx, El, col, row, text};
use serde_json::{Value, json};

use crate::chat::composer;
use crate::chat::details::Lane;
use crate::data;
use crate::theme::{self, T};
use crate::{App, Route};

#[derive(Default)]
pub struct Ui {
    /// The checkout a "New thread on {branch}" draft launches in: `(project id, workspaceStrategy)`.
    pub workspace: Option<(String, Value)>,
}

/// "New thread on {branch}": a draft in `project` that launches on the thread's branch and checkout.
pub fn start_on_branch(s: &mut App, project: &str, branch: &str, worktree: Option<&str>) {
    s.ui.chat.home.workspace = Some((project.to_string(), workspace_strategy(branch, worktree)));
    crate::shell::sidebar::go(s, Route::Home { project_id: Some(project.to_string()) });
}

/// `workspaceStrategy` for a draft on `branch`, in its worktree when it has one.
pub fn workspace_strategy(branch: &str, worktree: Option<&str>) -> Value {
    match worktree.filter(|w| !w.is_empty()) {
        Some(path) => json!({"type": "existing_worktree", "worktreePath": path, "branch": branch}),
        None => json!({"type": "root", "branch": branch}),
    }
}

/// Takes the draft's launch strategy for `project`: its branch preset, else the project root.
pub fn take_workspace(s: &mut App, project: &str) -> Value {
    match s.ui.chat.home.workspace.take() {
        Some((id, strategy)) if id == project => strategy,
        _ => json!({"type": "root"}),
    }
}

/// The project the draft belongs to: the route's, else the one with the latest activity.
pub fn draft_project(s: &App) -> Option<&Value> {
    if let Route::Home { project_id: Some(id) } = s.frame_route()
        && let Some(p) = composer::project_by_id(s, id)
    {
        return Some(p);
    }
    default_project(s)
}

/// The project a bare new-thread draft opens in: the most recently active one.
pub fn default_project(s: &App) -> Option<&Value> {
    let st = s.state();
    // sortScopedProjectsForSidebar by "updated_at": a project with threads ranks by its latest
    // user message, else by its own timestamps; ties go by title.
    let thread_ts = |t: &Value| data::ts(t, "latestUserMessageAt").or_else(|| data::ts(t, "updatedAt")).or_else(|| data::ts(t, "createdAt")).unwrap_or(i64::MIN);
    let activity = |p: &Value| {
        let id = data::s(p, "id");
        let mut threads = st.threads.iter().filter(|t| data::s(t, "projectId") == id && t["archivedAt"].is_null()).peekable();
        if threads.peek().is_some() {
            threads.map(thread_ts).max().unwrap_or(i64::MIN)
        } else {
            data::ts(p, "updatedAt").or_else(|| data::ts(p, "createdAt")).unwrap_or(i64::MIN)
        }
    };
    st.projects
        .iter()
        .filter(|p| p["archivedAt"].is_null())
        .max_by(|a, b| activity(a).cmp(&activity(b)).then_with(|| data::s(b, "title").cmp(data::s(a, "title"))))
}

/// The empty conversation (MessagesTimeline's empty state): a prompt centred in the lane.
pub fn view(cx: &mut Cx<App>, t: &T, lane: &Lane) -> El {
    let composer_h = cx.layout_with("composer-lane", |r| r[3].round()).unwrap_or(112.0);
    col()
        .key("home")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .justify_center()
        .pl(lane.left)
        .pb(composer_h + composer::BOTTOM + 8.0)
        .child(row().w(lane.width).justify_center().child(text("Send a message to start the conversation.").font(theme::TEXT_SM).color(t.c.muted_foreground)))
}
