pub mod actions;
pub mod attachments;
pub mod chip;
mod changed;
pub mod find;
mod markdown;

mod language_icons;
pub mod plan;
pub mod question;
pub mod queue;
mod subagent;
pub mod requests;
mod rows;
mod scenes;

use crate::data::s;
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rows::{Row, array};
use rune::{Cx, El, GrowthSpec, ScrollSpec, col, el, icon, row, text};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub use scenes::SCENES;

#[derive(Default)]
pub struct Ui {
    assets: attachments::Assets,
    /// The sent chip whose details are open: message id and context id.
    pub chip: Option<(String, String)>,
    pub diff_target: Option<Value>,
    responding: HashMap<String, u64>,
    response_errors: HashMap<String, String>,
    pub expanded: HashSet<String>,
    pub question_index: HashMap<String, usize>,
    pub wrapped: HashSet<String>,
    /// Typed answers to pending questions, by `request/question`.
    pub custom_answers: HashMap<String, rune::TextEdit>,
    pub answers: HashMap<String, HashMap<String, Vec<String>>>,
    /// Questions whose panel is folded to its header, by `request/question`.
    pub collapsed_questions: HashSet<String>,
    /// A picked single choice waiting to move on: request, question index, due time.
    pub auto_advance: Option<(String, usize, i64)>,
    /// The text a copy button last copied, and when.
    copied: Option<(String, i64)>,
    pub find: find::Find,
    pub actions: actions::State,
    pub plan: plan::State,
}

/// A row's stable key in the virtual list; find scrolls to marks inside it.
pub(super) fn row_key(row: &Row, index: usize) -> String {
    let key = match row {
        Row::User(item) | Row::Assistant(item) | Row::Item(item) => s(item, "id"),
        Row::Working { id, .. } | Row::Work { id, .. } => id.as_str(),
        Row::Request { request, .. } => s(request, "id"),
    };
    format!("timeline-row-{key}-{index}")
}

fn toggle(app: &mut App, id: &str) {
    if !app.ui.chat.timeline.expanded.remove(id) {
        app.ui.chat.timeline.expanded.insert(id.to_owned());
    }
}

/// How long a copy button shows its check (ANCHORED_COPY_TOAST_TIMEOUT_MS).
const COPIED_MS: i64 = 1000;

fn copied(app: &App, content: &str) -> bool {
    app.ui.chat.timeline.copied.as_ref().is_some_and(|(c, at)| c == content && crate::data::now_ms() - at < COPIED_MS)
}

pub(super) fn copy_button(app: &App, content: &str, t: &T) -> El {
    let done = copied(app, content);
    let content = content.to_owned();
    row()
        .size(24.0, 24.0)
        .center()
        .child(if done { icon(&icons::CHECK, 12.0).color(t.c.primary) } else { icon(&icons::COPY, 12.0).color(t.c.muted_foreground) })
        .radius(4.0)
        .hover(t.c.muted)
        .focusable(true)
        .role("button")
        .label(if done { "Copied" } else { "Copy" })
        .on_press_tx(move |app: &mut App, tx| {
            tx.copy(content.clone());
            app.ui.chat.timeline.copied = Some((content.clone(), crate::data::now_ms()));
            tx.wake_after(std::time::Duration::from_millis(COPIED_MS as u64));
        })
}

/// The open thread's working directory: its worktree, else its project's root.
pub(super) fn workspace_root(app: &App) -> Option<String> {
    let crate::Route::Thread(id) = app.frame_route() else { return None };
    let state = app.state();
    let thread = state.threads.iter().find(|t| s(t, "id") == id)?;
    if let Some(path) = thread["worktreePath"].as_str().filter(|p| !p.is_empty()) {
        return Some(path.to_owned());
    }
    let project = state.projects.iter().find(|p| s(p, "id") == s(thread, "projectId"))?;
    project["workspaceRoot"].as_str().map(str::to_owned)
}

/// TimelineSystemDivider: a rule either side of an icon, a label and an optional action, with
/// any detail wrapping under it.
fn divider(t: &T, glyph: &'static rune::Icon, label: String, color: rune::Color, detail: Option<String>, action: Option<El>) -> El {
    let rule = || el().grow(1.0).h(1.0).fill(t.c.border.alpha_mul(0.7));
    let line = row()
        .w_full()
        .gap(12.0)
        .center_y()
        .child(rule())
        .child(icon(glyph, 12.0).color(color))
        .child(text(label).font(theme::TEXT_XS).color(color))
        .children(action)
        .child(rule());
    col()
        .w_full()
        .py(4.0)
        .gap(4.0)
        .child(line)
        .children(detail.map(|d| row().w_full().justify_center().child(text(d).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap())))
}

fn item_view(app: &App, item: &Value, t: &T) -> El {
    let kind = s(item, "type");
    let id = s(item, "id").to_owned();
    let expanded = app.ui.chat.timeline.expanded.contains(&id);
    if kind == "assistant_message" {
        return markdown::view(app, s(item, "text"), &id, t);
    }
    if kind == "compaction" {
        let active = matches!(s(item, "status"), "pending" | "running" | "waiting");
        let failed = s(item, "status") == "failed";
        let label = if active {
            "Compacting context".to_owned()
        } else if failed {
            "Context compaction failed".to_owned()
        } else if let (Some(before), Some(after)) = (
            item["beforeTokenCount"].as_u64(),
            item["afterTokenCount"].as_u64(),
        ) {
            format!("Context compacted {before} → {after} tokens")
        } else {
            "Context compacted".to_owned()
        };
        let color = if failed {
            t.c.destructive
        } else {
            t.c.muted_foreground
        };
        return divider(t, &icons::MINIMIZE_2, label, color, None, None);
    }
    // V2LifecycleRow.
    if kind == "run_interrupt_result" {
        let detail = Some(s(item, "message").to_owned()).filter(|m| !m.is_empty());
        return divider(t, &icons::CIRCLE_STOP, "Run interrupted".into(), t.c.muted_foreground, detail, None);
    }
    if kind == "fork" {
        let from_run = s(&item["source"], "type") == "run";
        let (label, action, target) = if from_run {
            ("Forked from conversation", "Open source conversation", s(&item["source"], "threadId"))
        } else {
            ("Conversation fork", "Open fork", s(item, "targetThreadId"))
        };
        let open = (!target.is_empty()).then(|| {
            let target = target.to_owned();
            text(action)
                .font(theme::XS_MEDIUM)
                .color(t.c.foreground.alpha_mul(0.8))
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .on_press(move |app: &mut App| app.navigate(crate::Route::Thread(target.clone())))
        });
        return divider(t, &icons::GIT_FORK, label.into(), t.c.muted_foreground, None, open);
    }
    if matches!(
        kind,
        "error" | "system_notice" | "handoff" | "secret_request" | "link_request"
    ) {
        let detail = [
            s(&item["failure"], "message"),
            s(item, "message"),
            s(item, "text"),
            s(item, "title"),
        ]
        .into_iter()
        .find(|v| !v.is_empty())
        .unwrap_or("An error occurred");
        let mut notice = col()
            .gap(4.0)
            .child(text(detail).font(theme::TEXT_SM).color(if kind == "error" {
                t.c.destructive
            } else {
                t.c.muted_foreground
            }));
        if let Some(attempt) = item["retry"]["attempt"].as_u64() {
            notice = notice.child(
                text(format!(
                    "Retrying… (attempt {attempt}{})",
                    item["retry"]["maxAttempts"]
                        .as_u64()
                        .map(|max| format!("/{max}"))
                        .unwrap_or_default()
                ))
                .font(theme::TEXT_XS)
                .color(t.c.muted_foreground),
            );
        }
        return row()
            .gap(12.0)
            .px(4.0)
            .py(4.0)
            .child(
                icon(
                    if kind == "error" {
                        &icons::CIRCLE_ALERT
                    } else {
                        &icons::INFO
                    },
                    16.0,
                )
                .color(if kind == "error" {
                    t.c.destructive
                } else {
                    t.c.muted_foreground
                }),
            )
            .child(notice);
    }
    if kind == "proposed_plan" {
        return plan::card(app, item, t);
    }
    if kind == "subagent" {
        return subagent::view(app, item, t);
    }
    let glyph = match kind {
        // workEntryIconName: a notification's icon names its source.
        "notification" if s(item, "outcome") == "failed" => &icons::CIRCLE_ALERT,
        "notification" => match s(&item["source"], "kind") {
            "subagent" | "delegated_task" => &icons::BOT,
            "command" => &icons::TERMINAL,
            "monitor" => &icons::EYE,
            "background_task" => &icons::ZAP,
            _ => &icons::INFO,
        },
        "approval_request" | "user_input_request" => &icons::MESSAGE_CIRCLE,
        "reasoning" => &icons::BRAIN,
        "command_execution" => &icons::TERMINAL,
        "file_change" => &icons::SQUARE_PEN,
        "todo_list" => &icons::LIST_TODO,
        _ => &icons::WRENCH,
    };
    let head_key = format!("item-head-{id}");
    let path_label = rows::workspace_path(s(item, "fileName"), workspace_root(app).as_deref());
    let thought = s(item, "text").split_whitespace().collect::<Vec<_>>().join(" ");
    let todo = (kind == "todo_list").then(|| rows::todo_label(item));
    let fallback = rows::label(item);
    let title = if let Some(label) = &todo {
        label.as_str()
    } else if kind == "reasoning" && !thought.is_empty() {
        thought.as_str()
    } else if kind == "reasoning" {
        "Thought"
    } else if kind == "command_execution" && !s(item, "input").is_empty() {
        s(item, "input")
    } else if kind == "file_change" {
        path_label.as_str()
    } else {
        fallback.as_str()
    };
    let mut out = col().w_full().gap(4.0);
    out = out.child(
        row()
            .w_full()
            .gap(8.0)
            .py(4.0)
            .px(4.0)
            .radius(6.0)
            .hover(t.c.muted.alpha_mul(0.4))
            .focusable(true)
            .role("button")
            .label(title)
            .key(head_key.clone())
            .on_press_tx(move |app: &mut App, tx| {
                tx.keep_on_screen(head_key.clone());
                toggle(app, &id)
            })
            .child(icon(glyph, 16.0).color(t.c.muted_foreground))
            .child(
                text(title)
                    .font(theme::TEXT_SM)
                    .color(t.c.muted_foreground)
                    .truncate()
                    .grow(1.0)
                    .min_w(0.0),
            )
            .child(
                icon(
                    if expanded {
                        &icons::CHEVRON_DOWN
                    } else {
                        &icons::CHEVRON_RIGHT
                    },
                    12.0,
                )
                .color(t.c.muted_foreground),
            ),
    );
    if expanded && todo.is_some() {
        let mut steps = col().w_full().gap(6.0).px(28.0);
        for step in array(item, "steps") {
            steps = steps.child(
                row()
                    .gap(8.0)
                    .child(
                        icon(
                            if s(step, "status") == "completed" {
                                &icons::CHECK
                            } else {
                                &icons::CIRCLE
                            },
                            14.0,
                        )
                        .color(t.c.muted_foreground),
                    )
                    .child(
                        text(s(step, "text"))
                            .font(theme::TEXT_SM)
                            .color(t.c.foreground),
                    ),
            );
        }
        out = out.child(steps);
    } else if expanded {
        let detail = if kind == "reasoning" {
            s(item, "text").to_owned()
        } else if kind == "notification" {
            s(item, "detail").to_owned()
        } else {
            ["input", "output", "fileName"]
                .iter()
                .filter_map(|k| {
                    if *k == "fileName" && !s(item, "fileName").is_empty() {
                        return Some(path_label.clone());
                    }
                    let v = &item[*k];
                    if v.is_null() {
                        None
                    } else {
                        Some(
                            v.as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| v.to_string()),
                        )
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        out = out.child(
            col()
                .px(28.0)
                .child(markdown::view(app, &detail, s(item, "id"), t))
                .child(copy_button(app, &detail, t)),
        );
    }
    out
}

fn respond(app: &mut App, thread: &str, request: &str, fields: Value) {
    send(app, request, requests::command(thread, request, fields));
}

fn responding(app: &App, request: &str) -> bool {
    app.ui.chat.timeline.responding.contains_key(request)
}

/// Dispatches a command answering `request`, once at a time.
fn send(app: &mut App, request: &str, command: Value) {
    if responding(app, request) {
        return;
    }
    if let Some(net) = app.src.net() {
        let rpc = net.dispatch(command);
        app.ui
            .chat
            .timeline
            .responding
            .insert(request.to_owned(), rpc);
        app.ui.chat.timeline.response_errors.remove(request);
    }
}

pub fn poll(app: &mut App) -> bool {
    let mut changed = attachments::poll(app) | question::poll(app) | actions::poll(app) | plan::poll(app);
    let Some(net) = app.src.net() else {
        return changed;
    };
    let ui = &mut app.ui.chat.timeline;
    ui.responding.retain(|id, rpc| {
        let Some(reply) = net.take_reply(*rpc) else {
            return true;
        };
        changed = true;
        if let Err(error) = reply {
            ui.response_errors.insert(id.clone(), error.to_string());
        }
        false
    });
    changed
}

/// An approval's icon and name by request kind (TimelinePendingApprovalRow APPROVAL_PRESENTATION).
fn approval_presentation(kind: &str) -> (&'static rune::Icon, &'static str) {
    match kind {
        "file-change" => (&icons::SQUARE_PEN, "File change approval"),
        "file-read" => (&icons::EYE, "File read approval"),
        "permission" => (&icons::WRENCH, "App permission approval"),
        "mcp-elicitation" => (&icons::HAMMER, "App access approval"),
        _ => (&icons::TERMINAL, "Command approval"),
    }
}

/// An inline text action (InlineButton): medium xs, foreground or muted.
fn inline_button(label: &str, strong: bool, t: &T) -> El {
    let color = if strong { t.c.foreground } else { t.c.muted_foreground };
    text(label)
        .font(theme::XS_MEDIUM)
        .color(color)
        .focusable(true)
        .role("button")
        .cursor(rune::Cursor::Pointer)
}

fn request_view(app: &App, thread_id: &str, request: &Value, item: Option<&Value>, t: &T) -> El {
    let item = item.unwrap_or(request);
    let request_id = s(request, "id");
    let live = s(&request["responseCapability"], "type") == "live";
    let kind = [s(item, "requestKind"), s(request, "kind")].into_iter().find(|k| !k.is_empty()).unwrap_or("command");
    let (glyph, name) = approval_presentation(kind);
    let detail = ["prompt", "detail", "command", "input"]
        .iter()
        .find_map(|k| item[*k].as_str().filter(|v| !v.trim().is_empty()).map(str::to_owned).or_else(|| (!item[*k].is_null() && !item[*k].is_string()).then(|| item[*k].to_string())))
        .unwrap_or_else(|| name.to_owned());
    let mut card = col().gap(4.0).w_full().px(4.0).py(4.0).label(name);
    card = card.child(
        row()
            .w_full()
            .gap(12.0)
            .child(icon(glyph, 16.0).color(t.c.icon_muted))
            .child(text(detail).font(theme::MONO_XS.lh(20.0)).color(t.c.foreground).selectable(true).grow(1.0).shrink().min_w(0.0)),
    );
    let mut notes = col().pl(28.0).pb(4.0).gap(4.0);
    for key in ["appName", "reason"] {
        if let Some(v) = item[key].as_str().filter(|v| !v.is_empty()) {
            notes = notes.child(text(v).font(theme::TEXT_XS).color(t.c.muted_foreground).selectable(true));
        }
    }
    let responding = responding(app, request_id);
    if live {
        let more_key = format!("approval-more-{request_id}");
        let more = app.ui.chat.timeline.expanded.contains(&more_key);
        let mut actions = row()
            .flex_wrap()
            .gap(8.0)
            .center_y()
            .child(row().gap(4.0).center_y().child(icon(&icons::SHIELD, 12.0).color(t.c.warning)).child(text("Waiting for approval").font(theme::TEXT_XS).color(t.c.warning)))
            .child(text("·").font(theme::TEXT_XS).color(t.c.muted_foreground));
        let options: Vec<(String, String)> = if array(item, "options").is_empty() {
            [("Cancel", "cancel"), ("Decline", "decline"), ("Always allow this session", "acceptForSession"), ("Approve", "accept")]
                .iter()
                .map(|(l, d)| (l.to_string(), d.to_string()))
                .collect()
        } else {
            array(item, "options").iter().map(|o| (s(o, "label").to_owned(), s(o, "decision").to_owned())).collect()
        };
        let primary = |d: &str| matches!(d, "accept" | "decline");
        let has_more = options.iter().any(|(_, d)| !primary(d));
        let ordered = options.iter().filter(|(_, d)| primary(d)).chain(options.iter().filter(|(_, d)| more && !primary(d)));
        for (label, decision) in ordered {
            let (thread, r, decision) = (thread_id.to_owned(), request_id.to_owned(), decision.clone());
            let mut b = inline_button(label, decision == "accept", t);
            if !responding {
                b = b.on_press(move |app: &mut App| respond(app, &thread, &r, json!({"decision":decision})));
            }
            actions = actions.child(b.opacity(if responding { 0.64 } else { 1.0 }));
        }
        if has_more {
            actions = actions.child(
                inline_button(if more { "Less" } else { "More" }, false, t)
                    .label("More approval options")
                    .on_press(move |app: &mut App| toggle(app, &more_key)),
            );
        }
        notes = notes.child(actions);
    } else {
        notes = notes.child(
            text("Provider process is gone. Interrupt or restart the run to respond.")
                .font(theme::TEXT_XS)
                .color(t.c.warning),
        );
    }
    if let Some(error) = app.ui.chat.timeline.response_errors.get(request_id) {
        notes = notes.child(text(error).font(theme::TEXT_XS).color(t.c.destructive));
    }
    card.child(notes)
}

/// A loaded thread with nothing to show yet; the chat column shows the empty prompt instead.
pub fn is_empty(app: &App, thread_id: &str) -> bool {
    app.state().thread_views.get(thread_id).is_some_and(|v| v.synchronized && rows::derive(&v.projection, app.now_ms).is_empty())
}

pub fn view(app: &App, cx: &mut Cx<App>, t: &T, thread_id: &str, lane: &crate::chat::details::Lane) -> El {
    let Some(view) = app.state().thread_views.get(thread_id) else {
        return el().grow(1.0);
    };
    let composer_h = cx.layout_with("composer-lane", |r| r[3].round()).unwrap_or(112.0);
    let rows = rows::derive(&view.projection, app.now_ms);
    let targets = rows::revert_targets(&view.projection);
    let changes = changed::by_reply(&view.projection);
    let keys: Vec<String> = rows.iter().enumerate().map(|(i, r)| row_key(r, i)).collect();
    let mut marks = find::Marks::for_rows(app, &rows, t.dark).into_iter();
    let mut stack = col()
        .w(lane.width)
        .growth(GrowthSpec::default())
        .virtualize(100.0)
        // The composer floats over the list's bottom. Scroll extent counts the stack's own
        // padding but not the viewport's, so the clearance lives here.
        .pb(composer_h + crate::chat::composer::BOTTOM + 21.0);
    for (index, r) in rows.into_iter().enumerate() {
        let marks = marks.next().flatten();
        let (_, body) = match r {
            Row::User(item) => {
                let mut content = col().gap(8.0);
                // A picked element's crop shows in its chip's details, not as an image (TimelineUserMessageRow).
                for attachment in array(item, "attachments").iter().filter(|a| !s(a, "name").starts_with("preview-annotation-")) {
                    content = content.child(attachments::view(app, attachment, t));
                }
                if !s(item, "text").is_empty() {
                    use crate::chat::composer::chips;
                    // Context links read as their chips (ContextChip inline in the bubble).
                    let (shown, links) = chips::display(s(item, "text"));
                    let records = array(&item["context"], "records");
                    let chip_links = chip::links(&links, records);
                    let pills = links.into_iter().map(|c| {
                        let record = records.iter().find(|r| s(r, "contextId") == c.id);
                        (c.range, chips::style(&c.kind, record))
                    });
                    let highlights = marks.as_ref().map_or_else(Vec::new, |m| m.plain(&shown));
                    let mut body = text(shown)
                        .font(theme::TEXT_SM.lh(22.75))
                        .color(t.c.message_foreground)
                        .selectable(true)
                        .pills(pills)
                        .text_highlights(highlights);
                    if !chip_links.is_empty() {
                        let (pressed, records) = (s(item, "id").to_owned(), records.to_vec());
                        for link in chip_links {
                            body = body.link(link);
                        }
                        body = body.on_link(move |app: &mut App, id: &str, tx: &mut rune::Tx| chip::pressed(app, &pressed, id, &records, tx));
                    }
                    content = content.child(
                        col()
                            .key(chip::bubble_key(s(item, "id")))
                            .padding(12.0)
                            .radius(16.0)
                            .fill(t.c.message_surface)
                            .child(body),
                    );
                }
                let bubble = content.max_w(580.0);
                (
                    s(item, "id").to_owned(),
                    col()
                        .w_full()
                        .hover_group()
                        .gap(4.0)
                        .pb(8.0)
                        .child(row().w_full().justify_end().child(bubble))
                        .child(
                            row()
                                .w_full()
                                .justify_end()
                                .pr(4.0)
                                .child(actions::user(app, thread_id, item, &targets, t).group_opacity(0.0, 1.0)),
                        ),
                )
            }
            Row::Assistant(item) => (
                s(item, "id").to_owned(),
                col()
                    .w_full()
                    .hover_group()
                    .px(4.0)
                    .gap(6.0)
                    .child(markdown::view_marked(app, s(item, "text"), s(item, "id"), t, marks.as_ref()))
                    .children(changes.get(s(item, "id")).map(|c| changed::card(app, c, t)))
                    .child(row().child(actions::assistant(app, thread_id, item, t).group_opacity(0.0, 1.0))),
            ),
            Row::Working { id, label } => (
                id,
                col()
                    .w_full()
                    .pt(4.0)
                    .pb(8.0)
                    .child(
                        row()
                            .h(24.0)
                            .px(4.0)
                            .center_y()
                            .child(text(label).font(theme::TEXT_SM.lh(22.75)).color(t.c.muted_foreground)),
                    )
                    .child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.6))),
            ),
            Row::Work { id, label, items } => {
                let expanded = app.ui.chat.timeline.expanded.contains(&id);
                let toggle_id = id.clone();
                let head_key = format!("fold-head-{id}");
                let mut body = col().w_full().gap(6.0).child(
                    row()
                        .key(head_key.clone())
                        .w_full()
                        .py(4.0)
                        .px(4.0)
                        .gap(4.0)
                        .focusable(true)
                        .role("button")
                        .label(&label)
                        // Expanding at the bottom of a stuck list would push the header up.
                        .on_press_tx(move |app: &mut App, tx| {
                            tx.keep_on_screen(head_key.clone());
                            toggle(app, &toggle_id)
                        })
                        .child(
                            text(label)
                                .font(theme::TEXT_SM.lh(22.75))
                                .color(t.c.muted_foreground),
                        )
                        .child(
                            icon(
                                if expanded {
                                    &icons::CHEVRON_DOWN
                                } else {
                                    &icons::CHEVRON_RIGHT
                                },
                                14.0,
                            )
                            .color(t.c.muted_foreground),
                        ),
                );
                if expanded {
                    for item in items {
                        body = body.child(item_view(app, item, t));
                    }
                }
                (
                    id,
                    col()
                        .w_full()
                        .pb(8.0)
                        .child(body)
                        .child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.6))),
                )
            }
            Row::Item(item) => (s(item, "id").to_owned(), item_view(app, item, t)),
            Row::Request { request, item } => (
                s(request, "id").to_owned(),
                if s(request, "kind") == "user_input"
                    || item.is_some_and(|i| s(i, "type") == "user_input_request")
                {
                    row()
                        .gap(12.0)
                        .px(4.0)
                        .py(4.0)
                        .child(icon(&icons::MESSAGE_CIRCLE, 16.0).color(t.c.muted_foreground))
                        .child(
                            text("Input requested")
                                .font(theme::TEXT_SM)
                                .color(t.c.muted_foreground),
                        )
                } else {
                    request_view(app, thread_id, request, item, t)
                },
            ),
        };
        stack = stack.child(
            col()
                .key(keys[index].clone())
                .w_full()
                .py(3.0)
                .child(body),
        );
    }
    if let Some(queue) = queue::view(app, thread_id, t, lane.width) {
        stack = stack.child(col().key("timeline-row-queue").w_full().py(3.0).child(queue));
    }
    if let Some(error) = actions::error(app, t) {
        stack = stack.child(col().key("timeline-row-action-error").w_full().py(3.0).child(error));
    }
    col()
        .key(format!("timeline-{thread_id}"))
        .grow(1.0)
        .shrink()
        .min_h(0.0)
        .w_full()
        .scroll(ScrollSpec { stick_bottom: true })
        // Rows share the composer's lane, which the details card narrows.
        .pl(lane.left)
        .pt(21.0)
        .child(stack)
}

/// A pending question's panel, docked on top of the composer.
pub fn composer_requests(app: &App, t: &T) -> Option<El> {
    question::panel(app, t)
}
