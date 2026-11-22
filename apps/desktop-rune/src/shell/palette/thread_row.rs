//! A thread row's extras in the palette, as `useCommandPaletteThreadItems` renders them:
//! `ThreadRowLeadingStatus`, `ThreadCommandSubtitle` and the trailing PR number and machine.

use rune::{Color, El, icon, row, text};
use serde_json::Value;

use crate::shell::sidebar::logic::{self, Thread};
use crate::theme::{self, AlphaMul, T, tw};
use crate::{App, data, icons};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Pill {
    Approval,
    Input,
    Guest,
    Working,
    Connecting,
    Waiting,
    Failed,
    PlanReady,
    Completed,
}

impl Pill {
    fn label(self) -> &'static str {
        match self {
            Pill::Approval => "Pending Approval",
            Pill::Input => "Awaiting Input",
            Pill::Guest => "Guest Request",
            Pill::Working => "Working",
            Pill::Connecting => "Connecting",
            Pill::Waiting => "Waiting",
            Pill::Failed => "Failed",
            Pill::PlanReady => "Plan Ready",
            Pill::Completed => "Completed",
        }
    }

    fn color(self, t: &T) -> Color {
        let pick = |dark: Color, light: Color| if t.dark { dark } else { light };
        match self {
            Pill::Approval => pick(tw::AMBER_300.alpha_mul(0.9), tw::AMBER_600),
            Pill::Input => pick(tw::INDIGO_300.alpha_mul(0.9), tw::INDIGO_600),
            Pill::Guest => t.sb.guest_foreground,
            Pill::Working | Pill::Connecting => pick(tw::SKY_300.alpha_mul(0.8), tw::SKY_600),
            Pill::Waiting => t.sb.sidebar_muted_foreground,
            Pill::Failed => pick(tw::RED_300.alpha_mul(0.9), tw::RED_600),
            Pill::PlanReady => pick(tw::VIOLET_300.alpha_mul(0.9), tw::VIOLET_600),
            Pill::Completed => pick(tw::EMERALD_300.alpha_mul(0.9), tw::EMERALD_600),
        }
    }
}

/// `resolveThreadStatusPill`.
pub(super) fn pill(th: &Thread) -> Option<Pill> {
    if th.pending_approval {
        return Some(Pill::Approval);
    }
    if th.pending_input {
        return Some(Pill::Input);
    }
    if th.guests > 0 {
        return Some(Pill::Guest);
    }
    let runtime = th.runtime.as_ref().map(|r| r.status.as_str());
    match runtime {
        Some("running") if !th.blocked_on_wait => return Some(Pill::Working),
        Some("waiting") => return Some(Pill::Working),
        Some("preparing" | "starting" | "queued") => return Some(Pill::Connecting),
        Some("running") => return Some(Pill::Waiting),
        _ => {}
    }
    let run = th.run.as_ref();
    let seen_after = |done: Option<i64>| match (done, th.last_visited_at) {
        (Some(done), Some(seen)) if seen != i64::MIN => done <= seen,
        _ => false,
    };
    // Unopened threads' failures still count as unseen, unlike completions.
    if run.is_some_and(|r| r.status == "failed") && !seen_after(run.and_then(|r| r.completed_at)) {
        return Some(Pill::Failed);
    }
    let settled = runtime.is_none_or(|s| !logic::runtime_active(s));
    if !th.pending_input && th.plan_mode && th.actionable_plan && settled {
        return Some(Pill::PlanReady);
    }
    th.unread().then_some(Pill::Completed)
}

/// The PR state glyph and the status pill before the title.
pub(super) fn leading(t: &T, th: &Thread) -> Option<El> {
    let mut out = row().gap(6.0).center_y().shrink();
    let mut any = false;
    if let Some(pr) = &th.pr {
        let (glyph, tone) = crate::shell::sidebar::row::pr_glyph(t, Some(pr.state));
        out = out.child(icon(glyph, 12.0).color(tone).shrink());
        any = true;
    }
    if let Some(p) = pill(th) {
        let color = p.color(t);
        out = out.child(
            row()
                .gap(4.0)
                .center_y()
                .role("img")
                .label(p.label())
                .help(p.label())
                .child(rune::el().size(6.0, 6.0).radius(3.0).fill(color))
                .child(text(p.label()).font(theme::TEXT_3XS).color(color)),
        );
        any = true;
    }
    any.then_some(out)
}

fn dot(t: &T) -> El {
    text("·").font(theme::TEXT_XS).color(t.c.contrast_muted_foreground.alpha_mul(0.5))
}

/// `ThreadCommandSubtitle`: favicon and project · machine · workspace and branch · harness ·
/// "Current thread".
pub(super) fn subtitle(s: &App, t: &T, v: &Value, th: &Thread, current: bool) -> Option<El> {
    let muted = t.c.contrast_muted_foreground.alpha_mul(0.7);
    let meta = |glyph: &'static rune::Icon| icon(glyph, 12.0).color(muted).shrink();
    let state = s.state();
    let project = state.projects.iter().find(|p| data::s(p, "id") == th.project_id);
    let title = project.map(|p| data::s(p, "title").trim()).filter(|t| !t.is_empty());
    let branch = th.branch.as_deref().map(str::trim).filter(|b| !b.is_empty());
    let driver = crate::chat::composer::provider(s, &th.provider).map(|p| data::s(p, "driver").to_owned());
    let harness = driver.as_deref().and_then(|d| crate::shell::sidebar::row::provider_mark(t, d, muted, 12.0));
    if title.is_none() && branch.is_none() && !current && harness.is_none() {
        return None;
    }
    let mut out = row().gap(4.0).center_y().min_w(0.0).shrink();
    let mut first = true;
    let mut sep = |out: El| if std::mem::replace(&mut first, false) { out } else { out.child(dot(t)) };
    if let (Some(title), Some(project)) = (title, project) {
        out = sep(out)
            .child(crate::shell::sidebar::favicon::view(s, t, project, 12.0).shrink())
            .child(text(title).font(theme::TEXT_XS).color(muted).truncate().shrink())
            .child(dot(t))
            .child(text(crate::env::location(s, v)).font(theme::TEXT_XS).color(muted));
    }
    if let Some(branch) = branch {
        let worktree = th.worktree.as_deref().is_some_and(|w| !w.trim().is_empty());
        out = sep(out)
            .child(meta(if worktree { &icons::FOLDER_GIT_2 } else { &icons::FOLDER }))
            .child(text(branch).font(theme::TEXT_XS).color(muted).truncate().shrink());
    }
    if let Some(harness) = harness {
        out = sep(out).child(harness);
    }
    if current {
        out = sep(out).child(text("Current thread").font(theme::TEXT_XS).color(muted));
    }
    Some(out)
}

/// `pullRequestMatchesSearchQuery`: every word of the query is in the thread's PR numbers, links
/// or titles.
pub(super) fn pr_matches(v: &Value, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return false;
    }
    let mut haystack = String::new();
    let links = v["pullRequests"].as_array().into_iter().flatten().chain([&v["linkedPullRequest"], &v["branchPullRequest"]]);
    for pr in links.filter(|pr| pr.is_object()) {
        let title = pr["snapshot"]["title"].as_str().or(pr["title"].as_str()).unwrap_or("");
        haystack.push_str(&format!(" #{} {} {}", pr["number"], data::s(pr, "url"), title).to_lowercase());
    }
    query.split_whitespace().all(|word| haystack.contains(word))
}

/// The PR badge when the query names it, a running terminal, and the machine of a thread that
/// is not on this one (`ThreadSearchPullRequestNumber`, `ThreadRowTrailingStatus`).
pub(super) fn trailing(s: &App, t: &T, v: &Value, th: &Thread, query: &str) -> Vec<El> {
    let mut out = vec![];
    if let Some(pr) = th.pr.as_ref().filter(|_| pr_matches(v, query)) {
        let (glyph, tone) = crate::shell::sidebar::row::pr_glyph(t, Some(pr.state));
        let glyph = if pr.stack { &icons::LAYERS } else { glyph };
        out.push(
            row()
                .gap(2.0)
                .center_y()
                .shrink()
                .child(icon(glyph, 12.0).color(tone))
                .child(text(pr.text.clone()).font(theme::TEXT_XS.features("'tnum' 1")).color(tone)),
        );
    }
    if crate::panel::terminal::running(s, &th.id) > 0 {
        let teal = if t.dark { tw::TEAL_300.alpha_mul(0.9) } else { tw::TEAL_600 };
        out.push(icon(&icons::TERMINAL, 12.0).color(teal).shrink().help("Terminal process running"));
    }
    let key = crate::env::of(s, v);
    if key != crate::env::LOCAL {
        let label = Some(crate::env::label(s, &key)).filter(|l| !l.is_empty()).unwrap_or_else(|| "Remote".into());
        out.push(
            icon(crate::settings::env_machine_icon(s, &key), 12.0)
                .color(t.c.contrast_muted_foreground.alpha_mul(0.6))
                .shrink()
                .label(label.clone())
                .help(label),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_status_pill_follows_the_web_order() {
        let th = |v: Value| Thread::from_shell(&v);
        let base = json!({"id": "t", "lastVisitedAt": "2026-01-01T00:00:00Z"});
        let with = |extra: Value| {
            let mut v = base.clone();
            v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            th(v)
        };
        assert_eq!(pill(&th(base.clone())), None);
        assert_eq!(pill(&with(json!({"pendingGuestMessageCount": 2, "status": "running", "latestRunId": "r"}))), Some(Pill::Guest));
        assert_eq!(pill(&with(json!({"status": "running", "latestRunId": "r"}))), Some(Pill::Working));
        assert_eq!(pill(&with(json!({"status": "running", "latestRunId": "r", "blockedOnThreadWait": true}))), Some(Pill::Waiting));
        assert_eq!(pill(&with(json!({"status": "starting", "latestRunId": "r"}))), Some(Pill::Connecting));
        let run = |status: &str, at: &str| json!({"latestRunId": "r", "status": status, "latestRunCompletedAt": at});
        assert_eq!(pill(&with(run("failed", "2026-01-02T00:00:00Z"))), Some(Pill::Failed));
        assert_eq!(pill(&with(run("failed", "2025-12-01T00:00:00Z"))), None);
        assert_eq!(pill(&with(run("idle", "2026-01-02T00:00:00Z"))), Some(Pill::Completed));
        // Never opened: a failure is unseen, a completion is not.
        let unopened = |status: &str| th(json!({"id": "u", "latestRunId": "r", "status": status, "latestRunCompletedAt": "2026-01-02T00:00:00Z"}));
        assert_eq!(pill(&unopened("failed")), Some(Pill::Failed));
        assert_eq!(pill(&unopened("idle")), None);
        assert_eq!(pill(&with(json!({"interactionMode": "plan", "hasActionableProposedPlan": true}))), Some(Pill::PlanReady));
    }

    #[test]
    fn a_pr_number_shows_only_when_every_query_word_names_it() {
        let v = json!({"pullRequests": [{"number": 42, "url": "https://github.com/o/r/pull/42", "snapshot": {"title": "Fix the sidebar"}}]});
        assert!(pr_matches(&v, "#42"));
        assert!(pr_matches(&v, "fix sidebar"));
        assert!(!pr_matches(&v, "fix palette"));
        assert!(!pr_matches(&v, " "));
        assert!(pr_matches(&json!({"linkedPullRequest": {"number": 7, "url": "u"}}), "#7"));
    }
}
