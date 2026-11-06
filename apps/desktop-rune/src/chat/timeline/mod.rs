mod attachments;
mod markdown;

mod language_icons;
mod requests;
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
    pub diff_target: Option<Value>,
    responding: HashMap<String, u64>,
    response_errors: HashMap<String, String>,
    pub expanded: HashSet<String>,
    pub question_index: HashMap<String, usize>,
    pub wrapped: HashSet<String>,
    pub custom_answers: HashMap<String, rune::TextEdit>,
    pub answers: HashMap<String, HashMap<String, Vec<String>>>,
}

fn toggle(app: &mut App, id: &str) {
    if !app.ui.chat.timeline.expanded.remove(id) {
        app.ui.chat.timeline.expanded.insert(id.to_owned());
    }
}

fn button(label: &str, t: &T) -> El {
    text(label)
        .font(theme::TEXT_XS)
        .color(t.c.muted_foreground)
        .px(6.0)
        .py(4.0)
        .radius(4.0)
        .hover(t.c.muted)
        .focusable(true)
        .role("button")
}

pub(super) fn copy_button(content: &str, t: &T) -> El {
    let content = content.to_owned();
    row()
        .size(24.0, 24.0)
        .center()
        .child(icon(&icons::COPY, 12.0).color(t.c.muted_foreground))
        .radius(4.0)
        .hover(t.c.muted)
        .focusable(true)
        .role("button")
        .label("Copy")
        .on_press_tx(move |_: &mut App, tx| tx.copy(content.clone()))
}

fn item_view(app: &App, item: &Value, t: &T) -> El {
    let kind = s(item, "type");
    let id = s(item, "id").to_owned();
    let expanded = app.ui.chat.timeline.expanded.contains(&id);
    if kind == "assistant_message" {
        return markdown::view(app, s(item, "text"), &id, t);
    }
    if kind == "todo_list" {
        let mut card = col()
            .w_full()
            .gap(6.0)
            .padding(12.0)
            .radius(8.0)
            .border(1.0, t.c.border);
        card = card.child(text("Plan").font(theme::SM_MEDIUM).color(t.c.foreground));
        for step in array(item, "steps") {
            card = card.child(
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
        return card;
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
        return row()
            .w_full()
            .gap(12.0)
            .py(4.0)
            .child(el().grow(1.0).h(1.0).fill(t.c.border.alpha_mul(0.7)))
            .child(icon(&icons::MINIMIZE_2, 12.0).color(color))
            .child(text(label).font(theme::TEXT_XS).color(color))
            .child(el().grow(1.0).h(1.0).fill(t.c.border.alpha_mul(0.7)));
    }
    if matches!(kind, "error" | "notification" | "system_notice") {
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
    if kind == "checkpoint" {
        let files = array(item, "files");
        let checkpoint = item.clone();
        let additions: u64 = files.iter().filter_map(|f| f["additions"].as_u64()).sum();
        let deletions: u64 = files.iter().filter_map(|f| f["deletions"].as_u64()).sum();
        return row()
            .focusable(true)
            .role("button")
            .label("View changes")
            .radius(6.0)
            .hover(t.c.muted)
            .on_press(move |app: &mut App| crate::panel::open_diff(app, checkpoint.clone()))
            .gap(6.0)
            .px(4.0)
            .py(6.0)
            .child(icon(&icons::FILES, 14.0).color(t.c.muted_foreground))
            .child(
                text(format!(
                    "{} file{} changed  +{} −{}",
                    files.len(),
                    if files.len() == 1 { "" } else { "s" },
                    additions,
                    deletions
                ))
                .font(theme::TEXT_XS)
                .color(t.c.muted_foreground),
            );
    }
    if kind == "proposed_plan" {
        return col()
            .padding(12.0)
            .border(1.0, t.c.border)
            .radius(8.0)
            .child(
                text("Proposed plan")
                    .font(theme::SM_MEDIUM)
                    .color(t.c.foreground),
            )
            .child(markdown::view(app, s(item, "markdown"), &id, t));
    }
    let glyph = match kind {
        "reasoning" => &icons::BRAIN,
        "command_execution" => &icons::TERMINAL,
        "file_change" => &icons::SQUARE_PEN,
        _ => &icons::WRENCH,
    };
    let title = if kind == "reasoning" {
        "Thought"
    } else if kind == "command_execution" && !s(item, "input").is_empty() {
        s(item, "input")
    } else if kind == "file_change" {
        s(item, "fileName")
    } else {
        s(item, "title")
    };
    let title = if title.is_empty() { kind } else { title };
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
            .on_press(move |app: &mut App| toggle(app, &id))
            .child(icon(glyph, 16.0).color(t.c.muted_foreground))
            .child(
                text(title)
                    .font(theme::TEXT_SM)
                    .color(t.c.muted_foreground)
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
    if expanded {
        let detail = if kind == "reasoning" {
            s(item, "text").to_owned()
        } else {
            ["input", "output", "fileName"]
                .iter()
                .filter_map(|k| {
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
                .child(copy_button(&detail, t)),
        );
    }
    out
}

fn respond(app: &mut App, thread: &str, request: &str, fields: Value) {
    if app.ui.chat.timeline.responding.contains_key(request) {
        return;
    }
    if let Some(net) = app.src.net() {
        let command = requests::command(thread, request, fields);
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
    let mut changed = attachments::poll(app);
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

fn request_view(app: &App, thread_id: &str, request: &Value, item: Option<&Value>, t: &T) -> El {
    let item = item.unwrap_or(request);
    let request_id = s(request, "id");
    let live = s(&request["responseCapability"], "type") == "live";
    let question = s(request, "kind") == "user_input" || s(item, "type") == "user_input_request";
    let can_answer = s(&request["responseCapability"], "type") != "not_resumable";
    let mut card = col().gap(4.0).w_full();
    if question {
        card = card
            .padding(16.0)
            .radius(16.0)
            .fill(t.c.secondary)
            .border(1.0, t.c.border);
    }
    if question {
        let questions = array(item, "questions");
        let index = app
            .ui
            .chat
            .timeline
            .question_index
            .get(request_id)
            .copied()
            .unwrap_or(0)
            .min(questions.len().saturating_sub(1));
        card = card.child(
            text(format!(
                "{} · {}/{}",
                questions
                    .get(index)
                    .map(|q| s(q, "header"))
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Question"),
                index + 1,
                questions.len()
            ))
            .font(theme::SM_MEDIUM)
            .color(t.c.foreground),
        );
        for q in questions.iter().skip(index).take(1) {
            let qid = s(q, "id");
            card = card.child(
                text(s(q, "question"))
                    .font(theme::TEXT_SM)
                    .color(t.c.foreground),
            );
            for option in array(q, "options") {
                let label = s(option, "label");
                let value = option["value"].as_str().unwrap_or(label);
                let selected = app
                    .ui
                    .chat
                    .timeline
                    .answers
                    .get(request_id)
                    .and_then(|a| a.get(qid))
                    .is_some_and(|a| a.iter().any(|a| a == value));
                let multiple = q["multiSelect"].as_bool().unwrap_or(false);
                let (r, q, l) = (request_id.to_owned(), qid.to_owned(), value.to_owned());
                card = card.child(
                    button(&format!("{} {label}", if selected { "●" } else { "○" }), t)
                        .w_full()
                        .py(6.0)
                        .on_press(move |app: &mut App| {
                            let values = app
                                .ui
                                .chat
                                .timeline
                                .answers
                                .entry(r.clone())
                                .or_default()
                                .entry(q.clone())
                                .or_default();
                            if multiple {
                                if let Some(i) = values.iter().position(|v| v == &l) {
                                    values.remove(i);
                                } else {
                                    values.push(l.clone());
                                }
                            } else {
                                *values = vec![l.clone()];
                            }
                        }),
                );
                if !s(option, "description").is_empty() && s(option, "description") != label {
                    card = card.child(
                        text(s(option, "description"))
                            .font(theme::TEXT_XS)
                            .color(t.c.muted_foreground),
                    );
                }
            }
            if q["allowCustomAnswer"].as_bool() != Some(false) || array(q, "options").is_empty() {
                let key = format!("{request_id}/{qid}");
                let edit = app
                    .ui
                    .chat
                    .timeline
                    .custom_answers
                    .get(&key)
                    .cloned()
                    .unwrap_or_default();
                card = card.child(
                    rune::text_input(&edit, theme::TEXT_SM)
                        .w_full()
                        .min_h(36.0)
                        .padding(8.0)
                        .radius(6.0)
                        .border(1.0, t.c.border)
                        .color(t.c.foreground)
                        .placeholder("Type your answer…", t.c.muted_foreground)
                        .on_edit(move |app: &mut App| {
                            app.ui
                                .chat
                                .timeline
                                .custom_answers
                                .entry(key.clone())
                                .or_default()
                        }),
                );
            }
        }
        let (thread, r) = (thread_id.to_owned(), request_id.to_owned());
        let question_item = item.clone();
        if can_answer && index + 1 < questions.len() {
            let r = r.clone();
            card = card.child(button("Next", t).on_press(move |app: &mut App| {
                app.ui
                    .chat
                    .timeline
                    .question_index
                    .insert(r.clone(), index + 1);
            }));
        } else if can_answer {
            card = card.child(button("Submit answers", t).on_press(move |app: &mut App| {
                let selected = app
                    .ui
                    .chat
                    .timeline
                    .answers
                    .get(&r)
                    .cloned()
                    .unwrap_or_default();
                let answers = requests::answers(&question_item, &selected, |qid| {
                    app.ui
                        .chat
                        .timeline
                        .custom_answers
                        .get(&format!("{r}/{qid}"))
                        .map(|e| e.text.clone())
                });
                respond(app, &thread, &r, json!({"answers":answers}));
            }));
        } else {
            card = card.child(
                text("Provider process is gone. Interrupt or restart the run to respond.")
                    .font(theme::TEXT_XS)
                    .color(t.c.muted_foreground),
            );
        }
        if index > 0 {
            let r = request_id.to_owned();
            card = card.child(button("Previous", t).on_press(move |app: &mut App| {
                app.ui
                    .chat
                    .timeline
                    .question_index
                    .insert(r.clone(), index - 1);
            }));
        }
    } else {
        let detail = ["prompt", "detail", "command", "input", "title"]
            .iter()
            .map(|k| &item[*k])
            .find(|v| !v.is_null())
            .cloned()
            .unwrap_or(Value::Null);
        card = card.child(
            row()
                .gap(12.0)
                .child(icon(&icons::TERMINAL, 16.0).color(t.c.muted_foreground))
                .child(
                    text(
                        detail
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| detail.to_string()),
                    )
                    .font(theme::MONO_XS.lh(20.0))
                    .color(t.c.foreground),
                ),
        );
        if live {
            let more_key = format!("approval-more-{request_id}");
            let more = app.ui.chat.timeline.expanded.contains(&more_key);
            let mut actions = row()
                .gap(8.0)
                .pl(28.0)
                .child(icon(&icons::SHIELD, 12.0).color(t.c.muted_foreground))
                .child(
                    text("Waiting for approval ·")
                        .font(theme::TEXT_XS)
                        .color(t.c.muted_foreground),
                );
            let options: Vec<_> = if array(item, "options").is_empty() {
                vec![
                    ("Decline", "decline"),
                    ("Approve", "accept"),
                    ("Always allow this session", "acceptForSession"),
                    ("Cancel", "cancel"),
                ]
            } else {
                array(item, "options")
                    .iter()
                    .map(|o| (s(o, "label"), s(o, "decision")))
                    .collect()
            };
            let has_more = options
                .iter()
                .any(|(_, d)| !matches!(*d, "accept" | "decline"));
            for (label, decision) in options {
                if !more && !matches!(decision, "accept" | "decline") {
                    continue;
                }
                let decision = decision.to_owned();
                let (thread, r) = (thread_id.to_owned(), request_id.to_owned());
                actions = actions.child(button(label, t).on_press(move |app: &mut App| {
                    respond(app, &thread, &r, json!({"decision":decision}))
                }));
            }
            if has_more {
                actions = actions.child(
                    button(if more { "Less" } else { "More" }, t)
                        .on_press(move |app: &mut App| toggle(app, &more_key)),
                );
            }
            card = card.child(actions);
        } else {
            card = card.child(
                text("Provider process is gone. Interrupt or restart the run to respond.")
                    .font(theme::TEXT_XS)
                    .color(t.c.muted_foreground),
            );
        }
    }
    if app.ui.chat.timeline.responding.contains_key(request_id) {
        card = card.child(
            text("Responding…")
                .font(theme::TEXT_XS)
                .color(t.c.muted_foreground),
        );
    }
    if let Some(error) = app.ui.chat.timeline.response_errors.get(request_id) {
        card = card.child(text(error).font(theme::TEXT_XS).color(t.c.destructive));
    }
    card
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
    let mut stack = col()
        .w(lane.width)
        .growth(GrowthSpec::default())
        .virtualize(100.0);
    for (index, r) in rows.into_iter().enumerate() {
        let (key, body) = match r {
            Row::User(item) => {
                let mut content = col().gap(8.0);
                for attachment in array(item, "attachments") {
                    content = content.child(attachments::view(app, attachment, t));
                }
                if !s(item, "text").is_empty() {
                    content = content.child(
                        col()
                            .padding(12.0)
                            .radius(16.0)
                            .fill(t.c.message_surface)
                            .child(
                                text(s(item, "text"))
                                    .font(theme::TEXT_SM.lh(22.75))
                                    .color(t.c.message_foreground)
                                    .selectable(true),
                            ),
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
                                .child(copy_button(s(item, "text"), t).group_opacity(0.0, 1.0)),
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
                    .child(markdown::view(app, s(item, "text"), s(item, "id"), t))
                    .child(row().child(copy_button(s(item, "text"), t).group_opacity(0.0, 1.0))),
            ),
            Row::Work { id, label, items } => {
                let expanded = app.ui.chat.timeline.expanded.contains(&id);
                let toggle_id = id.clone();
                let mut body = col().w_full().gap(6.0).child(
                    row()
                        .w_full()
                        .py(4.0)
                        .px(4.0)
                        .gap(4.0)
                        .focusable(true)
                        .role("button")
                        .label(&label)
                        .on_press(move |app: &mut App| toggle(app, &toggle_id))
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
                .key(format!("timeline-row-{key}-{index}"))
                .w_full()
                .py(3.0)
                .child(body),
        );
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
        // The composer floats over the bottom of the list; the last row scrolls clear of it.
        .pb(composer_h + crate::chat::composer::BOTTOM + 21.0)
        .child(stack)
}

/// Question panels belong above the composer; approval details also remain inline.
pub fn composer_requests(app: &App, t: &T, thread_id: &str) -> El {
    let Some(view) = app.state().thread_views.get(thread_id) else {
        return el();
    };
    let mut out = col().w_full();
    for row in rows::derive(&view.projection, app.now_ms) {
        if let Row::Request { request, item } = row {
            if s(request, "kind") == "user_input"
                || item.is_some_and(|i| s(i, "type") == "user_input_request")
            {
                out = out.child(request_view(app, thread_id, request, item, t));
                break;
            }
        }
    }
    out
}
