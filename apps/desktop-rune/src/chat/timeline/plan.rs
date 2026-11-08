//! Proposed plans (ProposedPlanCard, proposedPlan.ts) and the plan follow-up: in plan mode, a
//! settled turn with an active plan docks "Plan ready" on the composer, whose primary action
//! becomes Implement (empty draft) or Refine (typed draft).

use super::rows::array;
use crate::data::s;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route};
use rune::{Color, Cursor, El, col, el, row, text};
use serde_json::Value;

pub const IMPLEMENTATION_PREFIX: &str = "PLEASE IMPLEMENT THIS PLAN:\n";

fn heading(line: &str) -> Option<&str> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let hashes = trimmed.len() - trimmed.trim_start_matches('#').len();
    let rest = &trimmed[hashes..];
    ((1..=6).contains(&hashes) && rest.starts_with([' ', '\t'])).then(|| rest.trim()).filter(|h| !h.is_empty())
}

/// The plan's first Markdown heading (proposedPlanTitle).
pub fn title(markdown: &str) -> Option<&str> {
    markdown.lines().find_map(heading)
}

/// The plan without its leading title and "Summary" heading (stripDisplayedPlanMarkdown).
pub fn displayed(markdown: &str) -> String {
    let lines: Vec<&str> = markdown.trim_end().lines().collect();
    let blank = |i: usize| lines.get(i).is_some_and(|l| l.trim().is_empty());
    let mut start = usize::from(lines.first().is_some_and(|l| heading(l).is_some()));
    while blank(start) {
        start += 1;
    }
    if lines.get(start).and_then(|l| heading(l)).is_some_and(|h| h.eq_ignore_ascii_case("summary")) {
        start += 1;
        while blank(start) {
            start += 1;
        }
    }
    lines.get(start..).unwrap_or_default().join("\n")
}

/// The first `max` non-blank lines, then "..." (buildCollapsedProposedPlanPreviewMarkdown).
pub fn preview(markdown: &str, max: usize) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut visible = 0;
    let mut more = false;
    let shown = displayed(markdown);
    for line in shown.trim_end().lines().map(str::trim_end) {
        let is_visible = !line.trim().is_empty();
        if is_visible && visible >= max {
            more = true;
            break;
        }
        out.push(line);
        visible += usize::from(is_visible);
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    if out.is_empty() {
        return title(markdown).unwrap_or("Plan preview unavailable.").to_owned();
    }
    if more {
        out.extend(["", "..."]);
    }
    out.join("\n")
}

/// The plan card in the timeline.
pub fn card(app: &App, item: &Value, t: &T) -> El {
    let id = s(item, "id").to_owned();
    let markdown = s(item, "markdown");
    let collapsible = markdown.len() > 900 || markdown.split('\n').count() > 20;
    let expanded = app.ui.chat.timeline.expanded.contains(&id);
    let find_open = app.ui.chat.timeline.find.open;
    let body = if collapsible && !expanded && !find_open { preview(markdown, 10) } else { displayed(markdown) };
    let badge = text("Plan")
        .font(theme::weight(theme::TEXT_XS, 500.0))
        .color(t.c.secondary_foreground)
        .px(6.0)
        .py(1.0)
        .radius(radius::SM)
        .fill(t.c.secondary);
    let head = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(badge)
        .child(text(title(markdown).unwrap_or("Proposed plan")).font(theme::SM_MEDIUM).color(t.c.foreground).truncate().grow(1.0).shrink().min_w(0.0))
        .child(super::copy_button(app, markdown, t));
    let mut out = col()
        .w_full()
        .padding(16.0)
        .gap(16.0)
        .radius(16.0)
        .border(1.0, t.c.border.alpha_mul(0.8))
        .fill(t.c.card.alpha_mul(0.7))
        .child(head)
        .child(super::markdown::view(app, &body, &id, t));
    if collapsible {
        let toggle_id = id.clone();
        let key = format!("plan-toggle-{id}");
        out = out.child(
            row().w_full().justify_center().child(
                crate::kit::Button::new(if expanded { "Collapse plan" } else { "Expand plan" })
                    .variant("outline")
                    .size("sm")
                    .build(t)
                    .key(key.clone())
                    .on_press_tx(move |app: &mut App, tx| {
                        tx.keep_on_screen(key.clone());
                        super::toggle(app, &toggle_id);
                    }),
            ),
        );
    }
    out
}

/// The open thread's actionable plan, when the composer should offer to implement it
/// (shouldShowPlanFollowUpPrompt over findLatestProposedPlan): `(plan id, markdown)`.
pub fn follow_up(app: &App) -> Option<(String, String)> {
    let Route::Thread(id) = app.frame_route() else { return None };
    if crate::chat::composer::interaction_mode(app) != "plan"
        || crate::chat::composer::running(app)
        || super::question::active(app).is_some()
        || crate::chat::composer::has_attachments(app)
    {
        return None;
    }
    let view = app.state().thread_views.get(id)?;
    let p = &view.projection;
    let plans: Vec<&Value> = array(p, "plans").iter().filter(|plan| s(plan, "kind") == "proposed_plan").collect();
    let latest_run = array(p, "runs").iter().max_by_key(|r| r["ordinal"].as_i64().unwrap_or(0)).map(|r| s(r, "id"));
    let of_run: Vec<&Value> = plans.iter().copied().filter(|plan| latest_run.is_some_and(|r| s(plan, "runId") == r)).collect();
    let pool = if of_run.is_empty() { plans } else { of_run };
    // The plan's turn item dates it (planItemTime) and carries its text when the plan is paged.
    let item_of = |plan: &Value| {
        array(p, "turnItems").iter().filter(|i| s(i, "type") == "proposed_plan" && s(i, "planId") == s(plan, "id")).max_by_key(|i| i["ordinal"].as_i64().unwrap_or(0))
    };
    let order = |plan: &Value| item_of(plan).and_then(|i| i["ordinal"].as_i64()).unwrap_or(-1);
    let plan = pool.into_iter().max_by(|a, b| order(a).cmp(&order(b)).then_with(|| s(a, "id").cmp(s(b, "id"))))?;
    if s(plan, "status") != "active" {
        return None;
    }
    let markdown = match s(plan, "markdown") {
        "" => item_of(plan).map(|i| s(i, "markdown")).unwrap_or_default(),
        text => text,
    };
    Some((s(plan, "id").to_owned(), markdown.to_owned()))
}

/// "Plan ready" docked on the composer (ComposerPlanFollowUpBanner).
pub fn banner(app: &App, t: &T) -> Option<El> {
    let (_, markdown) = follow_up(app)?;
    Some(
        row()
            .key("plan-follow-up")
            .w_full()
            .corners([radius::XL2, radius::XL2, 0.0, 0.0])
            .fill(if t.dark { t.c.surface_raised } else { t.c.card })
            .border(1.0, if t.dark { Color::WHITE.alpha(0.05) } else { Color::BLACK.alpha(0.08) })
            .px(16.0)
            .pt(6.0)
            .pb(17.0 + 6.0)
            .gap(6.0)
            .center_y()
            .child(el().size(6.0, 6.0).radius(radius::FULL).fill(t.c.primary))
            .child(text("Plan ready").font(theme::weight(theme::TEXT_XS, 500.0)).color(t.c.muted_foreground))
            .children(title(&markdown).map(|title| text(title.to_owned()).font(theme::TEXT_XS).color(t.c.foreground.alpha_mul(0.85)).truncate().grow(1.0).shrink().min_w(0.0))),
    )
}

/// The composer's primary action while a plan is ready: Refine with a draft, else Implement.
pub fn action(app: &App, t: &T, typed: bool) -> Option<El> {
    follow_up(app)?;
    let label = if typed { "Refine" } else { "Implement" };
    Some(
        row()
            .key("plan-implement")
            .h(28.0)
            .px(16.0)
            .center()
            .radius(radius::FULL)
            .fill(t.c.message_action)
            .hover(t.c.message_action_hover)
            .cursor(Cursor::Pointer)
            .label(label)
            .child(text(label).font(theme::SM_MEDIUM).color(t.c.message_action_foreground))
            .on_press(crate::chat::composer::submit),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_text_drops_its_title_and_summary_heading_and_previews_ten_lines() {
        let plan = "# Ship it\n\n## Summary\n\nDo the thing.\n\n- a\n- b\n";
        assert_eq!(title(plan), Some("Ship it"));
        assert_eq!(displayed(plan), "Do the thing.\n\n- a\n- b");
        let long: String = (1..=12).map(|i| format!("- step {i}\n")).collect();
        let shown = preview(&format!("# T\n{long}"), 10);
        assert!(shown.ends_with("- step 10\n\n..."), "{shown}");
        assert_eq!(preview("# Only a title", 10), "Only a title");
        assert_eq!(title("    # indented code"), None);
    }
}
