//! A pending question (ComposerPendingUserInputPanel): a drawer on top of the composer with the
//! active question and its options, digits 1-9 picking them. While it is open the composer types
//! that question's own answer and its send button steps through the questions.

use super::requests;
use super::rows::array;
use crate::data::s;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route, icons};
use rune::{Color, Cursor, El, Key, KeyEvent, TextEdit, Tx, col, el, icon, row, text};
use serde_json::{Value, json};

/// How long a picked single-choice option shows before the panel moves on.
const AUTO_ADVANCE_MS: i64 = 200;

/// The open thread's first pending question.
pub struct Active<'a> {
    pub thread: String,
    pub request: &'a Value,
    /// The `user_input_request` turn item carrying the questions (else the request itself).
    pub item: &'a Value,
}

impl<'a> Active<'a> {
    pub fn id(&self) -> &'a str {
        s(self.request, "id")
    }
    fn questions(&self) -> &'a [Value] {
        array(self.item, "questions")
    }
    fn can_respond(&self) -> bool {
        s(&self.request["responseCapability"], "type") != "not_resumable"
    }
    fn dismissible(&self) -> bool {
        s(self.item, "responseMode") == "message" || s(&self.request["responseCapability"], "type") == "message"
    }
}

fn turn_items(p: &Value) -> impl Iterator<Item = &Value> {
    array(p, "visibleTurnItems").iter().map(|v| &v["item"]).chain(array(p, "turnItems"))
}

pub fn active(app: &App) -> Option<Active<'_>> {
    let Route::Thread(thread) = app.frame_route() else { return None };
    let p = &app.state().thread_views.get(thread)?.projection;
    array(p, "runtimeRequests").iter().filter(|r| s(r, "status") == "pending").find_map(|request| {
        let item = turn_items(p).find(|i| s(i, "requestId") == s(request, "id") && !s(request, "id").is_empty());
        let question = s(request, "kind") == "user_input" || item.is_some_and(|i| s(i, "type") == "user_input_request");
        question.then(|| Active { thread: thread.clone(), request, item: item.unwrap_or(request) })
    })
}

/// Where the answers to one request stand.
#[derive(Debug, PartialEq)]
pub struct Progress {
    pub index: usize,
    pub last: bool,
    /// The active question has an answer.
    pub can_advance: bool,
    /// Every question has an answer.
    pub complete: bool,
}

fn custom_key(request: &str, question: &str) -> String {
    format!("{request}/{question}")
}

fn custom_text(app: &App, request: &str, question: &str) -> Option<String> {
    app.ui.chat.timeline.custom_answers.get(&custom_key(request, question)).map(|e| e.text.clone())
}

fn selected<'a>(app: &'a App, request: &str, question: &str) -> Option<&'a Vec<String>> {
    app.ui.chat.timeline.answers.get(request).and_then(|a| a.get(question))
}

pub fn progress(app: &App, a: &Active) -> Progress {
    let questions = a.questions();
    let index = app.ui.chat.timeline.question_index.get(a.id()).copied().unwrap_or(0).min(questions.len().saturating_sub(1));
    let answered = |q: &Value| {
        let qid = s(q, "id");
        requests::resolve(q, selected(app, a.id(), qid), custom_text(app, a.id(), qid).as_deref()).is_some()
    };
    Progress {
        index,
        last: index + 1 >= questions.len(),
        can_advance: questions.get(index).is_some_and(answered),
        complete: questions.iter().all(answered),
    }
}

fn active_question<'a>(app: &App, a: &Active<'a>) -> Option<&'a Value> {
    let questions = a.questions();
    questions.get(progress(app, a).index)
}

fn takes_own_answer(q: &Value) -> bool {
    q["allowCustomAnswer"].as_bool() != Some(false)
}

/// The composer's editor while a question is open: the active question's own answer.
pub struct Editor {
    pub edit: TextEdit,
    pub key: String,
    pub placeholder: &'static str,
}

pub fn editor(app: &App) -> Option<Editor> {
    let a = active(app)?;
    let q = active_question(app, &a)?;
    let key = custom_key(a.id(), s(q, "id"));
    let choice_only = !takes_own_answer(q) && !array(q, "options").is_empty();
    let placeholder = if choice_only { "Choose an option above" } else { "Type your own answer, or leave this blank to use the selected option" };
    let edit = app.ui.chat.timeline.custom_answers.get(&key).cloned().unwrap_or_else(|| {
        let mut edit = TextEdit::new(s(q, "initialAnswer"));
        edit.focused = true;
        edit
    });
    Some(Editor { edit, key, placeholder })
}

/// The stored answer edit behind [`editor`], created on first use.
pub fn editor_edit<'a>(app: &'a mut App, key: &str) -> &'a mut TextEdit {
    app.ui.chat.timeline.custom_answers.entry(key.to_owned()).or_insert_with(|| {
        let mut edit = TextEdit::new("");
        edit.focused = true;
        edit
    })
}

/// Typing an answer replaces the picked options, as the web's draft does.
pub fn on_editor_change(app: &mut App) {
    let Some(a) = active(app) else { return };
    let Some(q) = active_question(app, &a) else { return };
    let (request, qid) = (a.id().to_owned(), s(q, "id").to_owned());
    if custom_text(app, &request, &qid).is_some_and(|t| !t.trim().is_empty())
        && let Some(answers) = app.ui.chat.timeline.answers.get_mut(&request)
    {
        answers.remove(&qid);
    }
}

fn set_index(app: &mut App, request: &str, index: usize) {
    app.ui.chat.timeline.question_index.insert(request.to_owned(), index);
    app.ui.chat.timeline.auto_advance = None;
}

/// Enter or the send button: the next question, or on the last one the answers.
pub fn advance(app: &mut App) {
    let Some(a) = active(app) else { return };
    if !a.can_respond() {
        return;
    }
    let p = progress(app, &a);
    let (thread, request) = (a.thread.clone(), a.id().to_owned());
    if p.last {
        if !p.complete {
            return;
        }
        let item = a.item.clone();
        let selected = app.ui.chat.timeline.answers.get(&request).cloned().unwrap_or_default();
        let answers = requests::answers(&item, &selected, |qid| custom_text(app, &request, qid));
        app.ui.chat.timeline.auto_advance = None;
        super::respond(app, &thread, &request, json!({"answers": answers}));
    } else if p.can_advance {
        set_index(app, &request, p.index + 1);
    }
}

pub fn previous(app: &mut App) {
    let Some(a) = active(app) else { return };
    let index = progress(app, &a).index;
    let request = a.id().to_owned();
    set_index(app, &request, index.saturating_sub(1));
}

fn dismiss(app: &mut App) {
    let Some(a) = active(app) else { return };
    let (thread, request) = (a.thread.clone(), a.id().to_owned());
    let command = json!({"type": "thread.user-input.dismiss", "commandId": s5_net::uuid(), "threadId": thread, "requestId": request});
    super::send(app, &request, command);
}

/// Picks option `value` of the active question; a single choice moves on shortly after.
fn choose(app: &mut App, value: &str, tx: &mut Tx) {
    let Some(a) = active(app) else { return };
    if !a.can_respond() || super::responding(app, a.id()) {
        return;
    }
    let Some(q) = active_question(app, &a) else { return };
    let (request, qid, multi, index) = (a.id().to_owned(), s(q, "id").to_owned(), q["multiSelect"].as_bool() == Some(true), progress(app, &a).index);
    // The option replaces a typed answer; that text goes back to the thread's draft.
    if let Some(edit) = app.ui.chat.timeline.custom_answers.get_mut(&custom_key(&request, &qid)) {
        let typed = edit.text.trim().to_owned();
        edit.clear();
        if !typed.is_empty() {
            crate::chat::composer::append_to_draft(app, &typed);
        }
    }
    let values = app.ui.chat.timeline.answers.entry(request.clone()).or_default().entry(qid).or_default();
    if multi {
        match values.iter().position(|v| v == value) {
            Some(i) => {
                values.remove(i);
            }
            None => values.push(value.to_owned()),
        }
        return;
    }
    *values = vec![value.to_owned()];
    app.ui.chat.timeline.auto_advance = Some((request, index, crate::data::now_ms() + AUTO_ADVANCE_MS));
    tx.wake_after(std::time::Duration::from_millis(AUTO_ADVANCE_MS as u64));
}

/// Runs a due auto-advance. Called from the timeline's poll.
pub fn poll(app: &mut App) -> bool {
    let Some((request, index, at)) = app.ui.chat.timeline.auto_advance.clone() else { return false };
    if crate::data::now_ms() < at {
        return false;
    }
    app.ui.chat.timeline.auto_advance = None;
    let still = active(app).is_some_and(|a| a.id() == request && progress(app, &a).index == index);
    if still {
        advance(app);
    }
    true
}

/// Digits 1-9 pick an option when no text field has the keyboard.
pub fn on_key(app: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    let Key::Text(t) = &e.key else { return false };
    if e.mods.cmd || e.mods.ctrl || e.mods.alt {
        return false;
    }
    let Some(digit) = t.parse::<usize>().ok().filter(|d| (1..=9).contains(d)) else { return false };
    let Some(a) = active(app) else { return false };
    let Some(q) = active_question(app, &a) else { return false };
    if app.ui.chat.timeline.collapsed_questions.contains(&custom_key(a.id(), s(q, "id"))) {
        return false;
    }
    let Some(option) = array(q, "options").get(digit - 1) else { return false };
    let value = option["value"].as_str().unwrap_or(s(option, "label")).to_owned();
    choose(app, &value, tx);
    true
}

/// The drawer above the composer.
pub fn panel(app: &App, t: &T) -> Option<El> {
    let a = active(app)?;
    let q = active_question(app, &a)?;
    let questions = a.questions();
    let p = progress(app, &a);
    let request = a.id().to_owned();
    let collapse_key = custom_key(&request, s(q, "id"));
    let collapsed = app.ui.chat.timeline.collapsed_questions.contains(&collapse_key);
    let disabled = !a.can_respond() || super::responding(app, &request);
    let custom_active = custom_text(app, &request, s(q, "id")).is_some_and(|t| !t.trim().is_empty());

    let mut title = row().grow(1.0).min_w(0.0).gap(4.0).center_y().child(text(s(q, "header")).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().shrink());
    if collapsed {
        title = title
            .child(text("·").font(theme::TEXT_XS).color(t.c.muted_foreground.alpha_mul(0.4)).mx(4.0))
            .child(text(s(q, "question")).font(theme::TEXT_XS).color(t.c.secondary_label).truncate().grow(1.0).shrink().min_w(0.0));
    }
    let mut actions = row().center_y();
    if questions.len() > 1 {
        actions = actions.child(text(format!("{}/{}", p.index + 1, questions.len())).font(theme::TEXT_XS).color(t.c.muted_foreground).px(6.0));
    }
    actions = actions.child(el().size(24.0, 24.0).center().child(icon(if collapsed { &icons::CHEVRON_UP } else { &icons::CHEVRON_DOWN }, 14.0).color(t.c.muted_foreground)));
    if a.dismissible() {
        actions = actions.child(
            el().key("question-dismiss")
                .size(24.0, 24.0)
                .center()
                .radius(radius::MD)
                .hover(t.c.accent)
                .cursor(Cursor::Pointer)
                .label("Dismiss question without answering")
                .help("Dismiss question without answering")
                .child(icon(&icons::X, 14.0).color(t.c.muted_foreground))
                .on_press(dismiss),
        );
    }
    let head = row()
        .key("question-toggle")
        .w_full()
        .px(16.0)
        .pt(6.0)
        .pb(2.0)
        .gap(4.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .help(if collapsed { "Show the question and its options" } else { "Hide the question and its options" })
        .on_press(move |app: &mut App| {
            let set = &mut app.ui.chat.timeline.collapsed_questions;
            if !set.remove(&collapse_key) {
                set.insert(collapse_key.clone());
            }
        })
        .child(title)
        .child(actions);
    let mut drawer = col()
        .key("question-panel")
        .w_full()
        .corners([radius::XL2, radius::XL2, 0.0, 0.0])
        .fill(if t.dark { t.c.surface_raised } else { t.c.card })
        .border(1.0, if t.dark { Color::WHITE.alpha(0.05) } else { Color::BLACK.alpha(0.08) })
        // The composer overlaps the drawer's bottom edge.
        .pb(17.0 + 4.0)
        .child(head);
    if collapsed {
        return Some(drawer);
    }
    let multi = q["multiSelect"].as_bool() == Some(true);
    let mut options = col().w_full().pt(6.0);
    for (i, option) in array(q, "options").iter().enumerate() {
        let label = s(option, "label");
        let value = option["value"].as_str().unwrap_or(label).to_owned();
        let picked = !custom_active && selected(app, &request, s(q, "id")).is_some_and(|v| v.contains(&value));
        let mut body = col().grow(1.0).min_w(0.0).gap(2.0).child(text(label).font(theme::SM_MEDIUM).color(if picked { t.c.foreground } else { t.c.foreground.alpha_mul(0.7) }));
        let description = s(option, "description");
        if !description.is_empty() && description != label {
            body = body.child(text(description).font(theme::TEXT_2XS).color(t.c.secondary_label));
        }
        let mut line = row()
            .key(format!("question-option-{i}"))
            .w_full()
            .gap(10.0)
            .py(6.0)
            .pl(8.0)
            .radius(radius::MD)
            .role("button")
            .label(label)
            .child(row().h(20.0).center_y().child(mark(multi, picked, t)))
            .child(body);
        if i < 9 {
            line = line.child(row().h(20.0).w(24.0).center().child(crate::kit::kbd(&(i + 1).to_string(), t)));
        }
        if disabled {
            line = line.opacity(0.5);
        } else {
            if !picked {
                line = line.hover(t.c.muted.alpha_mul(0.3));
            }
            line = line.cursor(Cursor::Pointer).focusable(true).on_press_tx(move |app: &mut App, tx| choose(app, &value, tx));
        }
        options = options.child(line);
    }
    let body = col()
        .w_full()
        .pl(24.0)
        .pr(16.0)
        .pb(4.0)
        .child(col().w_full().pt(8.0).child(super::markdown::view(app, s(q, "question"), &format!("question-{request}"), t)))
        .child(options.ml(-8.0))
        .children(app.ui.chat.timeline.response_errors.get(&request).map(|error| text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).pt(8.0)));
    drawer = drawer.child(col().w_full().max_h(384.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(body));
    Some(drawer)
}

/// The radio or checkbox in front of an option, drawn as ui/radio-group and ui/checkbox are.
fn mark(multi: bool, on: bool, t: &T) -> El {
    let base = el().size(16.0, 16.0).center();
    if multi {
        return if on {
            base.radius(4.0).fill(t.c.primary).child(icon(&icons::CHECK, 12.0).color(t.c.primary_foreground))
        } else {
            base.radius(4.0).border(1.0, t.c.input).fill(if t.dark { t.c.input.alpha_mul(0.32) } else { t.c.background })
        };
    }
    if on {
        base.radius(radius::FULL).fill(t.c.primary).child(el().size(6.0, 6.0).radius(radius::FULL).fill(t.c.primary_foreground))
    } else {
        base.radius(radius::FULL).border(1.0, t.c.input).fill(if t.dark { t.c.input.alpha_mul(0.32) } else { t.c.background })
    }
}

/// The composer's send area while a question is open (ComposerPrimaryActions pendingAction):
/// Previous, then Next question or Submit.
pub fn actions(app: &App, t: &T, stop: Option<El>) -> Option<El> {
    let a = active(app)?;
    let p = progress(app, &a);
    let responding = super::responding(app, a.id());
    let mut out = row().gap(8.0).center_y().children(stop);
    if p.index > 0 {
        let mut back = row()
            .key("question-previous")
            .h(28.0)
            .px(10.0)
            .center()
            .radius(theme::layout::CONTROL_RADIUS)
            .border(1.0, t.c.border)
            .fill(t.c.background)
            .label("Previous question")
            .child(text("Previous").font(theme::SM_MEDIUM).color(t.c.foreground));
        if responding {
            back = back.opacity(0.64);
        } else {
            back = back.hover(t.c.accent).cursor(Cursor::Pointer).on_press(previous);
        }
        out = out.child(back);
    }
    let label = if responding {
        "Submitting..."
    } else if !p.last {
        "Next question"
    } else if p.index > 0 {
        "Submit answers"
    } else {
        "Submit answer"
    };
    let enabled = a.can_respond() && !responding && if p.last { p.complete } else { p.can_advance };
    let mut pill = row()
        .key("question-submit")
        .h(28.0)
        .px(16.0)
        .center()
        .radius(radius::FULL)
        .fill(t.c.message_action)
        .label(label)
        .child(text(label).font(theme::SM_MEDIUM).color(t.c.message_action_foreground));
    if enabled {
        pill = pill.hover(t.c.message_action_hover).cursor(Cursor::Pointer).on_press(advance);
    } else {
        pill = pill.opacity(0.64);
    }
    Some(out.child(pill))
}
