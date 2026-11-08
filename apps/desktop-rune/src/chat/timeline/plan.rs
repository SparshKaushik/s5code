//! Proposed plans (ProposedPlanCard, proposedPlan.ts) and the plan follow-up: in plan mode, a
//! settled turn with an active plan docks "Plan ready" on the composer, whose primary action
//! becomes Implement (empty draft) or Refine (typed draft).

use super::rows::array;
use crate::data::s;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route};
use crate::icons;
use crate::kit::{Button, Dialog, Input, Menu, MenuItem, Placement};
use rune::{Color, Cursor, El, TextEdit, Tx, col, el, icon, row, text};
use serde_json::{Value, json};

#[derive(Default)]
pub struct State {
    /// The open menu: a plan card's ("card:<item id>") or Implement's chevron ("implement").
    menu: Option<String>,
    /// "Save to workspace": the plan's markdown and the path field.
    save: Option<(String, TextEdit)>,
    saving: Option<u64>,
    /// "Implement in a new thread": the thread.create request, the thread, and its first message.
    launching: Option<(u64, String, Value)>,
}

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
    let id_for_menu = id.clone();
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
        .child(
            Button::new("Plan actions")
                .variant("outline")
                .size("icon-xs")
                .icon(&icons::ELLIPSIS)
                .on_press(move |app: &mut App| app.ui.chat.timeline.plan.menu = Some(format!("card:{id_for_menu}")))
                .build(t)
                .key(format!("plan-actions-{id}")),
        );
    let mut out = col()
        .w_full()
        .padding(20.0)
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
    let main = row()
        .key("plan-implement")
        .h(28.0)
        .px(16.0)
        .center()
        .fill(t.c.message_action)
        .hover(t.c.message_action_hover)
        .cursor(Cursor::Pointer)
        .label(label)
        .child(text(label).font(theme::SM_MEDIUM).color(t.c.message_action_foreground))
        .on_press(crate::chat::composer::submit);
    if typed {
        return Some(main.radius(radius::FULL));
    }
    // ComposerPrimaryActions: a split button whose chevron offers a fresh thread.
    let more = row()
        .key("plan-implement-more")
        .size(28.0, 28.0)
        .center()
        .corners([0.0, radius::FULL, radius::FULL, 0.0])
        .fill(t.c.message_action)
        .hover(t.c.message_action_hover)
        .cursor(Cursor::Pointer)
        .label("Implementation actions")
        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(t.c.message_action_foreground))
        .on_press(|app: &mut App| app.ui.chat.timeline.plan.menu = Some("implement".into()));
    Some(row().gap(1.0).child(main.corners([radius::FULL, 0.0, 0.0, radius::FULL])).child(more))
}

/// "Implement plan", or `Implement <title>` cut to 50 characters (buildPlanImplementationThreadTitle).
fn implementation_title(markdown: &str) -> String {
    let full = title(markdown).map_or_else(|| "Implement plan".to_owned(), |t| format!("Implement {t}"));
    let full = full.trim();
    if full.chars().count() <= 50 { full.to_owned() } else { format!("{}...", full.chars().take(50).collect::<String>()) }
}

/// The download name: the title in lowercase kebab case (buildProposedPlanMarkdownFilename).
fn filename(markdown: &str) -> String {
    let lowered = title(markdown).unwrap_or("plan").to_lowercase();
    let kept: String = lowered.chars().filter(|c| !"`'\".,!?()[]{}".contains(*c)).collect();
    let slug = kept.split(|c: char| !c.is_ascii_lowercase() && !c.is_ascii_digit()).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    format!("{}.md", if slug.is_empty() { "plan" } else { &slug })
}

/// useProposedPlanSend's new-thread path: create a sibling thread on the same branch and
/// worktree, then send it the plan with a link back to its source.
fn implement_in_new_thread(app: &mut App) {
    let (Some((plan_id, markdown)), Route::Thread(source)) = (follow_up(app), app.frame_route().clone()) else { return };
    let composer = crate::chat::composer::ctx(app);
    let (Some(thread), Some(project)) = (composer.thread, composer.project) else { return };
    let id = format!("thread:project:{}:{}", s(project, "id"), s5_net::uuid());
    let model = crate::chat::composer::selection(app).map(|(instance, model)| json!({"instanceId": instance, "model": model}));
    let create = json!({
        "type": "thread.create", "commandId": s5_net::uuid(), "createdBy": "user", "creationSource": "web",
        "threadId": id, "projectId": s(project, "id"), "title": implementation_title(&markdown),
        "modelSelection": model.clone().unwrap_or_else(|| thread["modelSelection"].clone()),
        "runtimeMode": crate::chat::composer::runtime_mode(app), "interactionMode": "default",
        "branch": thread["branch"].clone(), "worktreePath": thread["worktreePath"].clone(),
    });
    let mut message = json!({
        "type": "message.dispatch", "commandId": s5_net::uuid(), "createdBy": "user", "creationSource": "web",
        "threadId": id, "messageId": s5_net::uuid(), "text": format!("{}{}", IMPLEMENTATION_PREFIX, markdown.trim()),
        "attachments": [], "deliveryIntent": "auto", "dispatchMode": {"type": "start_immediately"},
        "sourcePlanRef": {"threadId": source, "planId": plan_id},
    });
    if let Some(model) = model {
        message["modelSelection"] = model;
    }
    let Some(net) = app.src.net() else { return };
    let request = net.dispatch(create);
    app.ui.chat.timeline.plan.launching = Some((request, id, message));
}

fn download(markdown: String, tx: &mut Tx) {
    let options = rune::FileDialogOptions { title: "Download plan".into(), file_name: Some(filename(&markdown)), ..Default::default() }
        .filter("Markdown", &["md"]);
    tx.save_file(options, move |app: &mut App, picked: rune::FileDialogResult, _| {
        if let Ok(Some(path)) = picked
            && let Err(e) = std::fs::write(&path, format!("{}\n", markdown.trim_end()))
        {
            crate::shell::palette::toast(app, "Could not download plan", Some(&e.to_string()));
        }
    });
}

fn save(app: &mut App) {
    let Some((markdown, path)) = app.ui.chat.timeline.plan.save.as_ref() else { return };
    let relative = path.text.trim().to_owned();
    if relative.is_empty() {
        return crate::shell::palette::toast(app, "Enter a workspace path", None);
    }
    let Some(cwd) = super::workspace_root(app) else { return };
    let contents = format!("{}\n", markdown.trim_end());
    let Some(net) = app.src.net() else { return };
    let request = net.request("projects.writeFile", json!({"cwd": cwd, "relativePath": relative, "contents": contents}));
    app.ui.chat.timeline.plan.saving = Some(request);
}

/// Folds the replies plan actions wait on. Called from the timeline's `poll`.
pub fn poll(app: &mut App) -> bool {
    let Some(net) = app.src.net() else { return false };
    let state = &mut app.ui.chat.timeline.plan;
    let saved = state.saving.and_then(|r| net.take_reply(r));
    let created = state.launching.as_ref().and_then(|(r, ..)| net.take_reply(*r));
    let mut changed = false;
    if let Some(reply) = saved {
        state.saving = None;
        changed = true;
        match reply {
            Ok(v) => {
                state.save = None;
                crate::shell::palette::toast(app, "Plan saved to workspace", v["relativePath"].as_str());
            }
            Err(e) => crate::shell::palette::toast(app, "Could not save plan", Some(&e.to_string())),
        }
    }
    if let Some(reply) = created {
        let (_, thread, message) = app.ui.chat.timeline.plan.launching.take().expect("launching");
        changed = true;
        match reply {
            Ok(_) => {
                if let Some(net) = app.src.net() {
                    net.dispatch(message);
                }
                app.navigate(Route::Thread(thread));
            }
            Err(e) => crate::shell::palette::toast(app, "Could not start implementation thread", Some(&e.to_string())),
        }
    }
    changed
}

/// The open plan menu and the "Save to workspace" dialog.
pub fn overlays(app: &App, t: &T) -> Vec<El> {
    let mut layers = Vec::new();
    let Route::Thread(thread_id) = app.frame_route() else { return layers };
    let state = &app.ui.chat.timeline.plan;
    let close = |app: &mut App| app.ui.chat.timeline.plan.menu = None;
    match state.menu.as_deref() {
        Some("implement") => Menu::new("plan-implement-menu")
            .anchor("plan-implement-more")
            .placement(Placement::TopEnd)
            .open(true)
            .on_close(close)
            .item(MenuItem::new("Implement in a new thread").on_select(|app: &mut App| {
                app.ui.chat.timeline.plan.menu = None;
                implement_in_new_thread(app);
            }))
            .overlay(&crate::kit::theme(t), &mut layers),
        Some(menu) if menu.starts_with("card:") => {
            let id = &menu["card:".len()..];
            let markdown = app
                .state()
                .thread_views
                .get(thread_id)
                .and_then(|v| array(&v.projection, "turnItems").iter().find(|i| s(i, "id") == id).map(|i| s(i, "markdown").to_owned()))
                .unwrap_or_default();
            let (copy, down, keep) = (markdown.clone(), markdown.clone(), markdown);
            let mut menu = Menu::new("plan-card-menu")
                .anchor(format!("plan-actions-{id}"))
                .placement(Placement::BottomEnd)
                .open(true)
                .on_close(close)
                .item(MenuItem::new("Copy to clipboard").on_select_tx(move |app: &mut App, tx: &mut Tx| {
                    app.ui.chat.timeline.plan.menu = None;
                    tx.copy(copy.clone());
                }))
                .item(MenuItem::new("Download as markdown").on_select_tx(move |app: &mut App, tx: &mut Tx| {
                    app.ui.chat.timeline.plan.menu = None;
                    download(down.clone(), tx);
                }));
            if super::workspace_root(app).is_some() {
                menu = menu.item(MenuItem::new("Save to workspace").on_select(move |app: &mut App| {
                    let state = &mut app.ui.chat.timeline.plan;
                    state.menu = None;
                    let mut path = TextEdit::new(filename(&keep));
                    path.focused = true;
                    state.save = Some((keep.clone(), path));
                }));
            }
            menu.overlay(&crate::kit::theme(t), &mut layers);
        }
        _ => {}
    }
    if let Some((_, path)) = &state.save {
        let root = super::workspace_root(app).unwrap_or_default();
        let saving = state.saving.is_some();
        let cancel = |app: &mut App| app.ui.chat.timeline.plan.save = None;
        Dialog::new("plan-save", "Save plan to workspace")
            .open(true)
            .description(format!("Enter a path relative to {root}."))
            .on_close(cancel)
            .child(
                col()
                    .w_full()
                    .gap(16.0)
                    .child(
                        col()
                            .w_full()
                            .gap(6.0)
                            .child(text("Workspace path").font(theme::SM_MEDIUM).color(t.c.foreground))
                            .child(
                                Input::new(path)
                                    .placeholder("plan.md")
                                    .font("mono")
                                    .on_edit(|app: &mut App| &mut app.ui.chat.timeline.plan.save.as_mut().expect("save dialog").1)
                                    .on_submit(|app: &mut App, _| save(app))
                                    .build(t),
                            ),
                    )
                    .child(
                        row()
                            .w_full()
                            .justify_end()
                            .gap(8.0)
                            .child(Button::new("Cancel").variant("outline").on_press(cancel).build(t))
                            .child(Button::new(if saving { "Saving..." } else { "Save" }).disabled(saving).on_press(save).build(t)),
                    ),
            )
            .build(t, &mut layers);
    }
    layers
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

    #[test]
    fn new_thread_title_and_download_name_follow_the_plan_title() {
        assert_eq!(implementation_title("# Ship it\n- a"), "Implement Ship it");
        assert_eq!(implementation_title("- no title"), "Implement plan");
        let long = format!("# {}", "x".repeat(60));
        assert_eq!(implementation_title(&long), format!("Implement {}...", "x".repeat(40)));
        assert_eq!(filename("# Ship it: (v2) now!\n"), "ship-it-v2-now.md");
        assert_eq!(filename("no title"), "plan.md");
    }
}
