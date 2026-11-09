//! The new-thread draft that `/` opens (routes/_chat.index.tsx starts a draft in the most recent
//! project): the empty-conversation prompt, with the provider status banner above it. Sending
//! from the composer launches the thread and navigates to it.

use rune::{Cursor, Cx, El, col, el, icon, row, spacer, text};
use serde_json::{Value, json};

use crate::chat::composer;
use crate::chat::details::Lane;
use crate::data;
use crate::icons;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route};

#[derive(Default)]
pub struct Ui {
    /// Provider instances whose status banner the user dismissed.
    pub dismissed: Vec<String>,
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
    let st = s.state();
    if let Route::Home { project_id: Some(id) } = s.frame_route()
        && let Some(p) = composer::project_by_id(s, id)
    {
        return Some(p);
    }
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

/// The provider status banner, floating at the top of the lane on any chat route.
pub fn banner(s: &App, t: &T, lane: &Lane) -> Option<El> {
    let banner = provider_banner(s, t)?;
    let right_inset = (lane.left * 2.0 + lane.width).max(0.0);
    Some(row().pin(Some(0.0), Some(0.0), None, None).w(right_inset).pt(12.0).justify_center().child(banner))
}

/// ProviderStatusBanner: the selected provider is unauthenticated or failing.
pub fn provider_banner(s: &App, t: &T) -> Option<El> {
    let (instance, _) = composer::selection(s)?;
    let p = composer::provider(s, &instance)?;
    if data::s(p, "status") != "error" || s.ui.chat.home.dismissed.contains(&instance) {
        return None;
    }
    let name = data::s(p, "displayName").to_string();
    let unauthenticated = data::s(&p["auth"], "status") == "unauthenticated" || data::s(p, "message").contains("not authenticated");
    let title = if unauthenticated { format!("{name} is unauthenticated") } else { format!("{name} is unavailable") };
    let message = data::s(p, "message").to_string();
    let fg = t.c.error_foreground;
    let refresh_id = instance.clone();
    let refresh = row()
        .key("banner-refresh")
        .h(24.0)
        .px(7.0)
        .gap(6.0)
        .center_y()
        .radius(theme::layout::CONTROL_RADIUS)
        .fill(t.c.foreground.alpha_mul(0.035))
        .border(1.0, t.c.foreground.alpha_mul(0.11))
        .hover(t.c.accent)
        .cursor(Cursor::Pointer)
        .label(format!("Refresh {name} provider status"))
        .child(icon(&icons::REFRESH_CW, 14.0).color(t.c.muted_foreground))
        .child(text("Refresh").font(theme::XS_MEDIUM).color(t.c.foreground))
        .on_press(move |s: &mut App| {
            if let Some(net) = s.src.net() {
                net.request("server.refreshProviders", json!({"instanceId": refresh_id}));
            }
        });
    let dismiss_id = instance.clone();
    let dismiss = el()
        .key("banner-dismiss")
        .size(24.0, 24.0)
        .center()
        .radius(theme::layout::CONTROL_RADIUS)
        .hover(t.c.accent)
        .cursor(Cursor::Pointer)
        .label(format!("Dismiss {name} provider error"))
        .child(icon(&icons::X, 14.0).color(t.c.muted_foreground))
        .on_press(move |s: &mut App| s.ui.chat.home.dismissed.push(dismiss_id.clone()));
    Some(
        row()
            .key("provider-banner")
            .items_start()
            .gap(8.0)
            // 12 of padding inside the 1-point border, plus the 2-point lead the line box adds.
            .px(15.0)
            .pt(15.0)
            .pb(13.0)
            .radius(radius::XL)
            // alert-glass: the page background with a 4% destructive tint.
            .fill(t.c.destructive.alpha_mul(0.04))
            .border(1.0, t.c.error.alpha_mul(0.32))
            .role("alert")
            .child(icon(&icons::INFO, 16.0).color(t.c.error).mt(2.0))
            .child(
                col()
                    .gap(2.0)
                    .shrink()
                    .min_w(0.0)
                    .child(text(title).font(theme::SM_MEDIUM).color(fg))
                    .child(text(message).font(theme::TEXT_SM).color(fg.alpha_mul(0.8)).wrap().max_lines(3))
                    .child(row().w_full().pt(6.0).child(spacer()).child(refresh)),
            )
            .child(dismiss.mt(-2.0)),
    )
}
