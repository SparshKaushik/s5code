//! Changing the open pull request in place (PullRequestTitleEditor, PullRequestMarkdownEditor,
//! PullRequestComposer with PullRequestCommentForm and PullRequestReviewForm, and
//! PullRequestReactionBar): the drafts, the requests they send, and what each reply does.

use std::collections::HashMap;

use rune::{Cursor, El, Key, KeyEvent, TextEdit, Tx, col, el, icon, row, spacer, text, text_input};
use serde_json::{Value, json};

use super::pr_detail::{can, reference};
use super::pr_row::{capitalized, term};
use crate::App;
use crate::data::s as field;
use crate::icons as i;
use crate::kit::{self, Menu, MenuItem, Placement, ToggleGroup};
use crate::theme::{self, AlphaMul, T, radius};

/// PULL_REQUEST_REACTION_ORDER, with each reaction's emoji and name.
const REACTIONS: [(&str, &str, &str); 8] = [
    ("thumbs-up", "👍", "thumbs up"),
    ("thumbs-down", "👎", "thumbs down"),
    ("laugh", "😄", "laugh"),
    ("hooray", "🎉", "hooray"),
    ("confused", "😕", "confused"),
    ("heart", "❤️", "heart"),
    ("rocket", "🚀", "rocket"),
    ("eyes", "👀", "eyes"),
];

/// The review form's VERDICTS, in its order.
const VERDICTS: [(&str, &str); 3] = [("comment", "Comment"), ("approve", "Approve"), ("request-changes", "Request changes")];
const EDITOR_MODES: [&str; 2] = ["Write", "Preview"];
const COMPOSER_MODES: [&str; 2] = ["Comment", "Review"];

#[derive(Default)]
pub struct Draft {
    pub edit: TextEdit,
    pub preview: bool,
}

impl Draft {
    /// A focused draft holding `text`, its caret at the end (autoFocus).
    pub fn new(text: &str) -> Self {
        let mut edit = TextEdit::new(text);
        edit.set_caret(text.len());
        edit.focused = true;
        Draft { edit, preview: false }
    }
}

#[derive(Default)]
pub struct CommentDraft {
    pub id: String,
    pub kind: String,
    pub draft: Draft,
}

#[derive(Default)]
pub struct Composer {
    /// Review was picked over Comment (requestedMode).
    pub review: bool,
    pub comment: TextEdit,
    pub summary: TextEdit,
    /// The verdict picked (requestedVerdict); Comment until one is.
    pub verdict: Option<&'static str>,
    pub verdict_menu: bool,
    /// "Close with comment" waiting on its second press.
    pub close_armed: bool,
}

#[derive(Default)]
pub struct Edit {
    /// The title draft while renaming (titleScope).
    pub title: Option<TextEdit>,
    /// The description draft while editing (bodyScope).
    pub body: Option<Draft>,
    /// The comment being edited (commentScope).
    pub comment: Option<CommentDraft>,
    pub composer: Composer,
    /// Reactions sent, by subject: the host's reading they lay over, then each content's wanted state.
    pub reactions: HashMap<String, (String, Vec<(&'static str, bool)>)>,
    /// The subject whose reaction picker is open.
    pub picker: Option<String>,
    pending: Vec<(u64, Pending)>,
}

#[derive(PartialEq)]
enum Pending {
    Title,
    Body,
    Comment(String),
    /// A posted comment, and the state change it carries: "comment", "close" or "reopen".
    Post(&'static str),
    Review { verdict: &'static str, body: String },
    Reaction { subject: String, content: &'static str },
}

#[derive(Clone, Copy, PartialEq)]
enum Target {
    Body,
    Comment,
}

fn edit(s: &mut App) -> &mut Edit {
    &mut s.ui.pages.detail.edit
}

fn busy(e: &Edit, f: impl Fn(&Pending) -> bool) -> bool {
    e.pending.iter().any(|(_, p)| f(p))
}

fn listed(list: &Value, item: &str) -> bool {
    list.as_array().is_some_and(|a| a.iter().any(|v| v == item))
}

// ---- rules ------------------------------------------------------------------------------

fn same_login(one: Option<&str>, other: Option<&str>) -> bool {
    matches!((one, other), (Some(a), Some(b)) if a.trim().to_lowercase() == b.trim().to_lowercase())
}

/// canEditPullRequestChangeRequest: the viewer's reported permission, else its author or a merger.
pub fn can_edit_change_request(d: &Value) -> bool {
    if d["capabilities"]["edit"]["changeRequest"] != true {
        return false;
    }
    if let Some(allowed) = d["viewerPermissions"]["editChangeRequest"].as_bool() {
        return allowed;
    }
    same_login(d["viewer"].as_str(), d["author"]["login"].as_str()) || listed(&d["viewerPermissions"]["actions"], "merge")
}

/// canEditPullRequestComment: hosts can't rewrite review summaries through comment mutations.
pub fn can_edit_comment(d: &Value, c: &Value) -> bool {
    if d["capabilities"]["edit"]["comment"] != true || !matches!(field(c, "kind"), "issue-comment" | "review-comment") {
        return false;
    }
    c["canEdit"].as_bool().unwrap_or_else(|| same_login(d["viewer"].as_str(), c["author"]["login"].as_str()))
}

/// The source control write grant of the environment that listed the open pull request.
fn writable(s: &App) -> bool {
    s.ui.pages.detail.entry.as_ref().is_some_and(|e| super::pr_row::can_write_on(s, e))
}

fn action_pending(s: &App) -> bool {
    s.ui.pages.detail.pending_action.is_some()
}

/// `PullRequestRef` for `entry` with the change's own fields.
fn payload(entry: &Value, fields: Value) -> Value {
    let mut p = reference(entry);
    if let (Some(p), Value::Object(f)) = (p.as_object_mut(), fields) {
        p.extend(f);
    }
    p
}

/// saveTitle: an empty or unchanged draft sends nothing and just closes.
pub fn title_save(d: &Value, entry: &Value, draft: &str) -> Option<Value> {
    let title = draft.trim();
    (!title.is_empty() && title != field(d, "title")).then(|| payload(entry, json!({"title": title})))
}

/// PullRequestCommentUpdateInput; a review summary has no endpoint to rewrite it.
pub fn comment_update(entry: &Value, id: &str, kind: &str, body: &str) -> Option<Value> {
    matches!(kind, "issue-comment" | "review-comment").then(|| payload(entry, json!({"commentId": id, "kind": kind, "body": body})))
}

/// PullRequestSubmitReviewInput, without line comments: this panel has no pending ones.
pub fn review_payload(entry: &Value, verdict: &str, body: &str) -> Value {
    payload(entry, json!({"verdict": verdict, "body": body, "comments": []}))
}

pub fn reaction_payload(entry: &Value, subject: &str, content: &str, reacted: bool) -> Value {
    payload(entry, json!({"subjectId": subject, "content": content, "reacted": reacted}))
}

/// The state change the comment form offers beside posting (followUpAction).
pub fn follow_up(d: &Value) -> Option<&'static str> {
    match field(d, "state") {
        "open" if can(d, "close") => Some("close"),
        "closed" if can(d, "reopen") => Some("reopen"),
        _ => None,
    }
}

fn can_comment(d: &Value) -> bool {
    d["capabilities"]["comment"] == true && d["viewerPermissions"]["comment"] == true
}

/// The verdicts both the host and the viewer allow, in the form's order.
pub fn verdicts(d: &Value) -> Vec<&'static str> {
    VERDICTS.iter().map(|v| v.0).filter(|v| listed(&d["capabilities"]["review"]["verdicts"], v) && listed(&d["viewerPermissions"]["verdicts"], v)).collect()
}

/// The composer's side: Review where only reviews are allowed, or where it was picked.
pub fn review_mode(can_comment: bool, verdicts: usize, requested_review: bool) -> bool {
    !can_comment || (verdicts > 0 && requested_review)
}

/// selectedVerdict: the one picked, Comment before any is, else the first offered.
fn selected_verdict(offered: &[&'static str], requested: Option<&'static str>) -> Option<&'static str> {
    let requested = requested.unwrap_or("comment");
    offered.iter().copied().find(|v| *v == requested).or_else(|| offered.first().copied())
}

/// reviewSummaryRequired: Forgejo wants a summary to request changes, even with line comments.
fn summary_required(d: &Value, verdict: &str) -> bool {
    verdict == "request-changes" && field(d, "provider") == "forgejo"
}

/// canSubmit: approving needs nothing, the other verdicts a summary.
pub fn can_submit(verdict: &str, summary: &str, required: bool) -> bool {
    let written = !summary.trim().is_empty();
    if verdict == "request-changes" && required { written } else { verdict == "approve" || written }
}

pub fn review_placeholder(verdict: &str, required: bool) -> &'static str {
    if can_submit(verdict, "", required) {
        "Summarize your review (optional)"
    } else if verdict == "request-changes" && required {
        "Summarize your review (required to request changes)"
    } else {
        "Summarize your review or add line comments"
    }
}

fn verdict_label(verdict: &str) -> &'static str {
    VERDICTS.iter().find(|v| v.0 == verdict).map_or("Comment", |v| v.1)
}

fn verdict_glyph(verdict: &str) -> &'static rune::Icon {
    match verdict {
        "approve" => &i::CHECK,
        "request-changes" => &i::CIRCLE_X,
        _ => &i::MESSAGE_SQUARE,
    }
}

/// The toast a submitted review raises (VERDICTS' `sent`).
pub fn verdict_sent(verdict: &str, term: &str) -> String {
    match verdict {
        "approve" => format!("{} approved", capitalized(term)),
        "request-changes" => "Changes requested".into(),
        _ => "Review submitted".into(),
    }
}

/// pullRequestReactionsSignature: changes whenever the host reports different reactions.
pub fn signature(reactions: &[Value]) -> String {
    reactions.iter().map(|r| format!("{}:{}:{}", field(r, "content"), r["count"].as_u64().unwrap_or(0), u8::from(r["viewerHasReacted"] == true))).collect::<Vec<_>>().join(" ")
}

/// applyPendingPullRequestReactions: the host's reactions with the ones sent laid over them.
pub fn apply_pending(reactions: &[Value], pending: &[(&str, bool)]) -> Vec<Value> {
    if pending.is_empty() {
        return reactions.to_vec();
    }
    let mut by: HashMap<String, Value> = reactions.iter().map(|r| (field(r, "content").to_string(), r.clone())).collect();
    for &(content, reacted) in pending {
        match by.get(content).cloned() {
            None => {
                if reacted {
                    by.insert(content.to_string(), json!({"content": content, "count": 1, "actors": [], "viewerHasReacted": true}));
                }
            }
            Some(current) if (current["viewerHasReacted"] == true) == reacted => {}
            Some(mut current) => {
                let count = current["count"].as_i64().unwrap_or(0) + if reacted { 1 } else { -1 };
                if count <= 0 {
                    by.remove(content);
                } else {
                    current["count"] = json!(count);
                    current["viewerHasReacted"] = json!(reacted);
                    by.insert(content.to_string(), current);
                }
            }
        }
    }
    REACTIONS.iter().filter_map(|r| by.remove(r.0)).collect()
}

fn reaction_name(content: &str) -> &str {
    REACTIONS.iter().find(|r| r.0 == content).map_or(content, |r| r.2)
}

fn join_names(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [one, other] => format!("{one} and {other}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

/// pullRequestReactionTooltip: hosts leave the viewer out of the actors, so "You" leads only while
/// the count has room for it.
pub fn reaction_tooltip(r: &Value) -> String {
    let count = r["count"].as_u64().unwrap_or(0) as usize;
    let actors: Vec<String> = r["actors"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
    let mut names = Vec::new();
    if r["viewerHasReacted"] == true && actors.len() < count {
        names.push("You".to_string());
    }
    names.extend(actors);
    names.truncate(count.min(3));
    let others = count - names.len();
    if others > 0 {
        let noun = match (names.is_empty(), others == 1) {
            (false, true) => "other",
            (false, false) => "others",
            (true, true) => "person",
            (true, false) => "people",
        };
        names.push(format!("{others} {noun}"));
    }
    format!("{} reacted with {} emoji", join_names(&names), reaction_name(field(r, "content")))
}

// ---- requests ---------------------------------------------------------------------------

fn failure_title(p: &Pending) -> &'static str {
    match p {
        Pending::Title => "The title could not be saved",
        Pending::Body => "Could not save the description",
        Pending::Comment(_) => "Could not save the comment",
        Pending::Post(_) => "Could not post the comment",
        Pending::Review { .. } => "The review could not be submitted",
        Pending::Reaction { .. } => "The reaction could not be saved",
    }
}

/// Sends through the environment that listed the open pull request.
fn send(s: &mut App, method: &str, input: Value, pending: Pending) -> bool {
    let Some(entry) = s.ui.pages.detail.entry.clone() else { return false };
    match super::pull_requests::net_for(s, &entry) {
        Some(net) => {
            let id = net.request(method, input);
            edit(s).pending.push((id, pending));
            true
        }
        None => {
            crate::shell::palette::toast_error_brief(s, failure_title(&pending), Some("Changes need a connected environment."));
            false
        }
    }
}

/// Takes the replies to what this module sent.
pub fn poll(s: &mut App) -> bool {
    if s.ui.pages.detail.edit.pending.is_empty() {
        return false;
    }
    let Some(entry) = s.ui.pages.detail.entry.clone() else { return false };
    let ids: Vec<u64> = edit(s).pending.iter().map(|(id, _)| *id).collect();
    let mut changed = false;
    for id in ids {
        let Some(reply) = super::pull_requests::net_for(s, &entry).and_then(|net| net.take_reply(id)) else { continue };
        let Some(at) = edit(s).pending.iter().position(|(held, _)| *held == id) else { continue };
        let (_, pending) = edit(s).pending.remove(at);
        finish(s, pending, reply);
        changed = true;
    }
    changed
}

/// A failure toasts and keeps the draft to try again; a success closes the draft and rereads the
/// pull request (onSaved, onCommented, onReviewSubmitted, onRefresh).
fn finish(s: &mut App, pending: Pending, reply: Result<Value, Value>) {
    let word = s.ui.pages.detail.detail.as_ref().map_or("pull request", term);
    if let Err(cause) = &reply {
        let hint = match &pending {
            Pending::Title => Some("The host refused the new title."),
            Pending::Review { .. } => Some("The host refused the review. Your draft is still here to try again."),
            _ => None,
        };
        let detail = hint.map(|hint| super::pr_row::readable(&super::pull_requests::error_text(cause, ""), hint));
        if let Pending::Reaction { subject, content } = &pending
            && let Some(held) = edit(s).reactions.get_mut(subject)
        {
            held.1.retain(|(c, _)| c != content);
        }
        crate::shell::palette::toast_error_brief(s, failure_title(&pending), detail.as_deref());
        return;
    }
    let e = edit(s);
    match pending {
        Pending::Title => e.title = None,
        Pending::Body => e.body = None,
        Pending::Comment(id) => {
            if e.comment.as_ref().is_some_and(|c| c.id == id) {
                e.comment = None;
            }
        }
        Pending::Post(action) => {
            e.composer.comment.clear();
            // The posted comment shows in the detail the action rereads.
            if action != "comment" && !action_pending(s) {
                super::pr_detail::run(s, action);
                return;
            }
        }
        Pending::Review { verdict, body } => {
            // Summary edits made while it was sending stay.
            if e.composer.summary.text == body {
                e.composer.summary.clear();
            }
            crate::shell::palette::toast(s, &verdict_sent(verdict, word), None);
        }
        Pending::Reaction { .. } => {}
    }
    super::pr_detail::refresh(s);
}

fn start_title(s: &mut App) {
    let title = s.ui.pages.detail.detail.as_ref().map(|d| field(d, "title").to_string()).unwrap_or_default();
    edit(s).title = Some(Draft::new(&title).edit);
}

fn save_title(s: &mut App) {
    if !writable(s) || busy(edit(s), |p| *p == Pending::Title) {
        return;
    }
    let u = &s.ui.pages.detail;
    let (Some(entry), Some(d), Some(draft)) = (&u.entry, &u.detail, &u.edit.title) else { return };
    match title_save(d, entry, &draft.text) {
        Some(input) => {
            send(s, "pullRequests.update", input, Pending::Title);
        }
        None => edit(s).title = None,
    }
}

fn start_body(s: &mut App) {
    let body = s.ui.pages.detail.detail.as_ref().map(|d| field(d, "body").to_string()).unwrap_or_default();
    edit(s).body = Some(Draft::new(&body));
}

fn start_comment(s: &mut App, c: &Value) {
    let draft = Draft::new(field(c, "body"));
    edit(s).comment = Some(CommentDraft { id: field(c, "id").to_string(), kind: field(c, "kind").to_string(), draft });
}

fn draft_mut(s: &mut App, target: Target) -> &mut Draft {
    let e = edit(s);
    match target {
        Target::Body => e.body.get_or_insert_with(Draft::default),
        Target::Comment => &mut e.comment.get_or_insert_with(CommentDraft::default).draft,
    }
}

fn is_saving(e: &Edit, target: Target) -> bool {
    busy(e, |p| match target {
        Target::Body => *p == Pending::Body,
        Target::Comment => matches!(p, Pending::Comment(_)),
    })
}

/// The markdown editor's onSave: a description may be emptied, a comment may not.
fn save(s: &mut App, target: Target) {
    if !writable(s) || is_saving(edit(s), target) {
        return;
    }
    let u = &s.ui.pages.detail;
    let Some(entry) = &u.entry else { return };
    let request = match (target, &u.edit.body, &u.edit.comment) {
        (Target::Body, Some(draft), _) => Some(("pullRequests.update", payload(entry, json!({"body": draft.edit.text})), Pending::Body)),
        (Target::Comment, _, Some(c)) if !c.draft.edit.text.trim().is_empty() => {
            comment_update(entry, &c.id, &c.kind, &c.draft.edit.text).map(|input| ("pullRequests.updateComment", input, Pending::Comment(c.id.clone())))
        }
        _ => None,
    };
    if let Some((method, input, pending)) = request {
        send(s, method, input, pending);
    }
}

fn cancel(s: &mut App, target: Target) {
    match target {
        Target::Body => edit(s).body = None,
        Target::Comment => edit(s).comment = None,
    }
}

/// The comment form's submit: posts, then closes or reopens when that button sent it.
fn post(s: &mut App, action: &'static str) {
    let body = edit(s).composer.comment.text.trim().to_string();
    if body.is_empty() || busy(edit(s), |p| matches!(p, Pending::Post(_))) || action_pending(s) || !writable(s) {
        return;
    }
    let Some(entry) = s.ui.pages.detail.entry.clone() else { return };
    edit(s).composer.close_armed = false;
    send(s, "pullRequests.comment", payload(&entry, json!({"body": body})), Pending::Post(action));
}

/// useInlineActionConfirm for "Close with comment": the first press arms, the second posts.
fn arm_close(s: &mut App) {
    if edit(s).composer.close_armed {
        post(s, "close");
    } else {
        edit(s).composer.close_armed = true;
    }
}

fn submit_review(s: &mut App) {
    if !writable(s) || busy(edit(s), |p| matches!(p, Pending::Review { .. })) {
        return;
    }
    let u = &s.ui.pages.detail;
    let (Some(entry), Some(d)) = (&u.entry, &u.detail) else { return };
    let Some(verdict) = selected_verdict(&verdicts(d), u.edit.composer.verdict) else { return };
    let body = u.edit.composer.summary.text.clone();
    if !can_submit(verdict, &body, summary_required(d, verdict)) {
        return;
    }
    let input = review_payload(entry, verdict, &body);
    send(s, "pullRequests.submitReview", input, Pending::Review { verdict, body });
}

/// The reaction bar's toggle: shown at once, dropped again if the host refuses it.
fn toggle_reaction(s: &mut App, subject: &str, sig: &str, content: &'static str, reacted: bool) {
    if !writable(s) {
        return;
    }
    let Some(entry) = s.ui.pages.detail.entry.clone() else { return };
    let pending = Pending::Reaction { subject: subject.to_string(), content };
    if !send(s, "pullRequests.setReaction", reaction_payload(&entry, subject, content, reacted), pending) {
        return;
    }
    let held = edit(s).reactions.entry(subject.to_string()).or_insert_with(|| (sig.to_string(), Vec::new()));
    if held.0 != sig {
        *held = (sig.to_string(), Vec::new());
    }
    match held.1.iter_mut().find(|(c, _)| *c == content) {
        Some(wanted) => wanted.1 = reacted,
        None => held.1.push((content, reacted)),
    }
}

// ---- view -------------------------------------------------------------------------------

/// PullRequestEditButton: a ghost pencil the row's hover reveals.
fn pencil(t: &T, label: &str, press: impl Fn(&mut App) + 'static) -> El {
    kit::Button::new("").variant("ghost-muted").size("icon-xs").icon(&i::PENCIL).on_press(press).build(t).label(label.to_string()).help(label.to_string()).group_opacity(0.0, 1.0)
}

/// The Textarea: a multi-line field `rows` lines tall.
fn area(t: &T, key: String, edit: &TextEdit, rows: f32, label: String, placeholder: &str) -> El {
    let font = theme::TEXT_SM;
    text_input(edit, font)
        .key(key)
        .role("textbox")
        .label(label)
        .placeholder(placeholder.to_string(), t.c.contrast_placeholder)
        .w_full()
        .min_h(rows * font.line_height + 16.0)
        .px(11.0)
        .py(8.0)
        .radius(8.0)
        .fill(if t.dark { t.c.input.alpha_mul(0.32) } else { t.c.background })
        .border(1.0, t.c.contrast_input)
        .color(t.c.contrast_foreground)
        .caret(t.c.contrast_foreground, t.c.primary.alpha_mul(0.3))
        .focus_ring(3.0, -3.0, t.c.ring.alpha_mul(0.24))
}

fn submits(e: &KeyEvent) -> bool {
    e.key == Key::Enter && (e.mods.cmd || e.mods.ctrl) && !e.mods.shift && !e.mods.alt
}

/// PullRequestTitleEditor: the title with its pencil, or the field renaming it (Enter saves,
/// Escape cancels).
pub fn title(s: &App, t: &T, d: &Value) -> El {
    let e = &s.ui.pages.detail.edit;
    let Some(draft) = &e.title else {
        let shown = text(field(d, "title").to_string()).font(theme::weight(theme::TEXT_LG.lh(26.0), theme::SEMIBOLD)).color(t.c.foreground).wrap().grow(1.0).shrink().min_w(0.0);
        let mut line = row().w_full().gap(4.0).items_start().hover_group().child(shown);
        if can_edit_change_request(d) {
            line = line.child(pencil(t, "Edit title", start_title).mt(3.0));
        }
        return line;
    };
    let saving = busy(e, |p| *p == Pending::Title);
    let mut input = kit::Input::new(draft).size("sm").disabled(saving);
    if !saving {
        input = input.on_edit(|s: &mut App| edit(s).title.get_or_insert_with(TextEdit::default)).on_submit(|s: &mut App, _: &mut Tx| save_title(s));
    }
    let mut input = input.build(t).single_line().key("pr-edit-title").label(format!("{} title", capitalized(term(d))));
    if !saving {
        input = input.on_input_key(|s: &mut App, e: &KeyEvent, _: &mut Tx| {
            if e.key != Key::Escape {
                return false;
            }
            edit(s).title = None;
            true
        });
    }
    let empty = draft.text.trim().is_empty();
    let buttons = row()
        .w_full()
        .gap(8.0)
        .justify_end()
        .child(kit::Button::new("Cancel").variant("ghost").size("xs").disabled(saving).on_press(|s: &mut App| edit(s).title = None).build(t))
        .child(kit::Button::new(if saving { "Saving..." } else { "Save" }).variant("outline").size("xs").disabled(!writable(s) || saving || empty).on_press(save_title).build(t));
    col().w_full().gap(8.0).child(input).child(buttons)
}

/// PullRequestMarkdownEditor: Write or Preview, Cancel and Save; Cmd/Ctrl+Enter saves, Escape
/// cancels.
fn markdown_editor(s: &App, t: &T, target: Target, draft: &Draft, key: String, label: String, placeholder: &str) -> El {
    let saving = is_saving(&s.ui.pages.detail.edit, target);
    let empty = draft.edit.text.trim().is_empty();
    let allow_empty = target == Target::Body;
    let mode = ToggleGroup::new(&EDITOR_MODES, &[usize::from(draft.preview)])
        .on_pick(move |s: &mut App, n: usize, _: &mut Tx| {
            if !is_saving(edit(s), target) {
                draft_mut(s, target).preview = n == 1;
            }
        })
        .build(&kit::theme(t))
        .label("Markdown editor mode");
    let body = if draft.preview {
        let shown = if empty { text("Nothing to preview.").font(theme::TEXT_XS).color(t.c.muted_foreground) } else { super::pr_detail::markdown(t, &draft.edit.text) };
        col().w_full().px(12.0).py(8.0).radius(radius::LG).border(1.0, t.c.border.alpha_mul(0.6)).child(shown)
    } else {
        let input = area(t, key, &draft.edit, 6.0, label, placeholder);
        if saving {
            input.disabled(true).opacity(0.64)
        } else {
            input.on_edit(move |s: &mut App| &mut draft_mut(s, target).edit).on_input_key(move |s: &mut App, e: &KeyEvent, tx: &mut Tx| {
                if submits(e) {
                    if !tx.key_repeat() {
                        save(s, target);
                    }
                    return true;
                }
                if e.key != Key::Escape {
                    return false;
                }
                cancel(s, target);
                true
            })
        }
    };
    let buttons = row()
        .w_full()
        .gap(8.0)
        .justify_end()
        .child(kit::Button::new("Cancel").variant("ghost").size("xs").disabled(saving).on_press(move |s: &mut App| cancel(s, target)).build(t))
        .child(kit::Button::new(if saving { "Saving..." } else { "Save" }).variant("outline").size("xs").disabled(!writable(s) || saving || (empty && !allow_empty)).on_press(move |s: &mut App| save(s, target)).build(t));
    col().w_full().gap(8.0).child(row().child(mode)).child(body).child(buttons)
}

/// The Description section's body: the text with its pencil, or the editor changing it.
pub fn description(s: &App, t: &T, d: &Value, shown: El) -> El {
    if let Some(draft) = &s.ui.pages.detail.edit.body {
        let word = term(d);
        return markdown_editor(s, t, Target::Body, draft, "pr-edit-body".into(), format!("{} description", capitalized(word)), &format!("Describe this {word}"));
    }
    let mut line = row().w_full().gap(4.0).items_start().hover_group().child(shown.grow(1.0).shrink().min_w(0.0));
    if can_edit_change_request(d) {
        line = line.child(pencil(t, "Edit description", start_body));
    }
    line
}

/// CommentBody: the comment's text with its pencil, or the editor changing it; nothing when it has
/// neither text nor a way to write one.
pub fn comment_body(s: &App, t: &T, c: &Value, shown: Option<El>) -> Option<El> {
    let u = &s.ui.pages.detail;
    let Some(d) = &u.detail else { return shown };
    let id = field(c, "id");
    if let Some(editing) = u.edit.comment.as_ref().filter(|e| e.id == id) {
        return Some(markdown_editor(s, t, Target::Comment, &editing.draft, format!("pr-edit-comment-{id}"), "Edit comment".into(), ""));
    }
    let editable = can_edit_comment(d, c);
    if shown.is_none() && !editable {
        return None;
    }
    let mut line = row().w_full().gap(4.0).items_start().hover_group().child(shown.unwrap_or_else(el).grow(1.0).shrink().min_w(0.0));
    if editable {
        let comment = c.clone();
        line = line.child(pencil(t, "Edit comment", move |s: &mut App| start_comment(s, &comment)));
    }
    Some(line)
}

/// PullRequestReactionBar for a comment: its reactions, then the picker where the host takes them.
pub fn reaction_bar(s: &App, t: &T, c: &Value) -> Option<El> {
    let u = &s.ui.pages.detail;
    let d = u.detail.as_ref()?;
    let can_react = d["capabilities"]["reactions"] == true && writable(s);
    let subject = field(c, "id").to_string();
    let host = c["reactions"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let sig = signature(host);
    let overlay = u.edit.reactions.get(&subject).filter(|(at, _)| *at == sig).map_or(&[][..], |(_, wanted)| wanted.as_slice());
    let shown = apply_pending(host, overlay);
    if shown.is_empty() && !can_react {
        return None;
    }
    let mut bar = row().gap(4.0).center_y().flex_wrap().shrink().min_w(0.0);
    for r in &shown {
        let content = field(r, "content");
        let reacted = r["viewerHasReacted"] == true;
        let count = r["count"].as_u64().unwrap_or(0);
        let emoji = REACTIONS.iter().find(|x| x.0 == content).map_or(content, |x| x.1);
        let mut pill = row()
            .key(format!("pr-reaction-{subject}-{content}"))
            .h(24.0)
            .px(8.0)
            .gap(4.0)
            .center_y()
            .radius(radius::FULL)
            .border(1.0, if reacted { t.c.primary.alpha_mul(0.6) } else { t.c.border.alpha_mul(0.7) })
            .fill(if reacted { t.c.primary.alpha_mul(0.1) } else { t.c.muted.alpha_mul(0.4) })
            .role("button")
            .label(format!("{}, {count}", reaction_name(content)))
            .help(reaction_tooltip(r))
            .child(text(emoji.to_string()).font(theme::TEXT_XS))
            .child(text(count.to_string()).font(theme::TEXT_XS.features("'tnum' 1")).color(if reacted { t.c.foreground } else { t.c.muted_foreground }));
        if can_react && let Some(&(known, ..)) = REACTIONS.iter().find(|x| x.0 == content) {
            let (subject, sig) = (subject.clone(), sig.clone());
            pill = pill.hover(t.c.primary.alpha_mul(0.12)).cursor(Cursor::Pointer).on_press(move |s: &mut App| toggle_reaction(s, &subject, &sig, known, !reacted));
        }
        bar = bar.child(pill);
    }
    if !can_react {
        return Some(bar);
    }
    let toggle = subject.clone();
    bar = bar.child(
        row()
            .key(format!("pr-reaction-add-{subject}"))
            .h(24.0)
            .px(6.0)
            .center_y()
            .radius(radius::FULL)
            .border(1.0, t.c.border.alpha_mul(0.7))
            .hover(t.c.accent.alpha_mul(0.5))
            .cursor(Cursor::Pointer)
            .role("button")
            .label("Add a reaction")
            .help("Add a reaction")
            .child(icon(&i::SMILE_PLUS, 14.0).color(t.c.muted_foreground))
            .on_press(move |s: &mut App| {
                let picker = &mut edit(s).picker;
                *picker = if picker.as_deref() == Some(toggle.as_str()) { None } else { Some(toggle.clone()) };
            }),
    );
    if u.edit.picker.as_deref() == Some(subject.as_str()) {
        let mut picker = row().key(format!("pr-reaction-picker-{subject}")).gap(2.0).padding(2.0).center_y().radius(radius::MD).border(1.0, t.c.border).fill(t.c.background);
        for (content, emoji, name) in REACTIONS {
            let reacted = shown.iter().any(|r| field(r, "content") == content && r["viewerHasReacted"] == true);
            let (subject, sig) = (subject.clone(), sig.clone());
            let mut option = row().key(format!("pr-reaction-pick-{content}")).size(28.0, 28.0).center().radius(radius::MD).hover(t.c.accent).cursor(Cursor::Pointer).role("button").label(name).help(name).child(text(emoji).font(theme::TEXT_BASE));
            if reacted {
                option = option.fill(t.c.primary.alpha_mul(0.1));
            }
            picker = picker.child(option.on_press(move |s: &mut App| {
                edit(s).picker = None;
                toggle_reaction(s, &subject, &sig, content, !reacted);
            }));
        }
        bar = bar.child(picker);
    }
    Some(bar)
}

/// PullRequestComposer, at the end of the Summary: Comment and Review where both are allowed,
/// else whichever one is.
pub fn composer(s: &App, t: &T, d: &Value, overlays: &mut Vec<El>) -> Option<El> {
    let c = &s.ui.pages.detail.edit.composer;
    let commenting = can_comment(d);
    let offered = verdicts(d);
    if !commenting && offered.is_empty() {
        return None;
    }
    let review = review_mode(commenting, offered.len(), c.review);
    let word = term(d);
    let head = if commenting && !offered.is_empty() {
        ToggleGroup::new(&COMPOSER_MODES, &[usize::from(review)]).on_pick(|s: &mut App, n: usize, _: &mut Tx| edit(s).composer.review = n == 1).build(&kit::theme(t)).label("Composer mode")
    } else {
        text(if review { format!("Review {word}") } else { format!("Comment on {word}") }).font(theme::SM_MEDIUM).color(t.c.foreground)
    };
    let form = if review { review_form(s, t, d, &offered, overlays) } else { comment_form(s, t, d) };
    let card = col()
        .w_full()
        .gap(12.0)
        .padding(12.0)
        .radius(radius::LG)
        .border(1.0, t.c.border.alpha_mul(0.6))
        .role("group")
        .label(format!("{} composer", capitalized(word)))
        .child(row().child(head))
        .child(form);
    Some(col().key("pr-composer").w_full().px(16.0).pt(16.0).child(card))
}

/// PullRequestCommentForm: the comment, with closing or reopening it alongside where allowed.
fn comment_form(s: &App, t: &T, d: &Value) -> El {
    let c = &s.ui.pages.detail.edit.composer;
    let posting = s.ui.pages.detail.edit.pending.iter().find_map(|(_, p)| match p {
        Pending::Post(action) => Some(*action),
        _ => None,
    });
    let locked = posting.is_some() || action_pending(s);
    let off = c.comment.text.trim().is_empty() || locked || !writable(s);
    let input = area(t, "pr-composer-comment".into(), &c.comment, 3.0, format!("Comment on this {}", term(d)), "Leave a comment");
    let input = if locked {
        input.disabled(true).opacity(0.64)
    } else {
        input.on_edit(|s: &mut App| &mut edit(s).composer.comment).on_input_key(|s: &mut App, e: &KeyEvent, tx: &mut Tx| {
            if !submits(e) {
                return false;
            }
            if !tx.key_repeat() {
                post(s, "comment");
            }
            true
        })
    };
    let mut buttons = row().w_full().gap(8.0).justify_end().flex_wrap();
    match follow_up(d) {
        Some("close") => {
            let label = match (posting, c.close_armed) {
                (Some("close"), _) => "Closing...",
                (_, true) => "Confirm close?",
                _ => "Close with comment",
            };
            buttons = buttons.child(kit::Button::new(label).variant("destructive-outline").size("xs").icon(&i::GIT_PULL_REQUEST_CLOSED).disabled(off).on_press(arm_close).build(t));
        }
        Some(_) => {
            let label = if posting == Some("reopen") { "Reopening..." } else { "Reopen with comment" };
            buttons = buttons.child(kit::Button::new(label).variant("outline").size("xs").icon(&i::GIT_PULL_REQUEST_ARROW).disabled(off).on_press(|s: &mut App| post(s, "reopen")).build(t));
        }
        None => {}
    }
    let label = if posting == Some("comment") { "Posting..." } else { "Comment" };
    buttons = buttons.child(kit::Button::new(label).variant("outline").size("xs").icon(&i::SEND).disabled(off).on_press(|s: &mut App| post(s, "comment")).build(t));
    col().w_full().gap(8.0).child(input).child(buttons)
}

/// PullRequestReviewForm: the summary, the verdict where more than one is offered, and Submit.
fn review_form(s: &App, t: &T, d: &Value, offered: &[&'static str], overlays: &mut Vec<El>) -> El {
    let c = &s.ui.pages.detail.edit.composer;
    let pending = busy(&s.ui.pages.detail.edit, |p| matches!(p, Pending::Review { .. }));
    let selected = selected_verdict(offered, c.verdict);
    let placeholder = selected.map_or("Summarize your review (optional)", |v| review_placeholder(v, summary_required(d, v)));
    let input = area(t, "pr-composer-review".into(), &c.summary, 3.0, "Review summary".into(), placeholder).on_edit(|s: &mut App| &mut edit(s).composer.summary);
    let mut footer = row().w_full().gap(8.0).center_y();
    if offered.len() > 1
        && let Some(current) = selected
    {
        let trigger = kit::Button::new(verdict_label(current))
            .variant("outline")
            .size("xs")
            .icon(verdict_glyph(current))
            .disabled(pending)
            .on_press(|s: &mut App| {
                let c = &mut edit(s).composer;
                c.verdict_menu = !c.verdict_menu;
            })
            .build(t)
            .label("Review verdict");
        let mut menu = Menu::new("pr-review-verdict").trigger(trigger).placement(Placement::Top).min_width(180.0).open(c.verdict_menu && !pending).on_close(|s: &mut App| edit(s).composer.verdict_menu = false);
        for &verdict in offered {
            menu = menu.item(MenuItem::new(verdict_label(verdict)).icon(verdict_glyph(verdict)).checked(verdict == current).on_select(move |s: &mut App| {
                let c = &mut edit(s).composer;
                c.verdict = Some(verdict);
                c.verdict_menu = false;
            }));
        }
        footer = footer.child(menu.build(&kit::theme(t), overlays));
    }
    let ready = selected.is_some_and(|v| can_submit(v, &c.summary.text, summary_required(d, v)));
    let submit = kit::Button::new(if pending { "Submitting..." } else { "Submit review" }).size("xs").disabled(pending || !ready || !writable(s)).on_press(submit_review).build(t);
    col().w_full().gap(8.0).child(input).child(footer.child(spacer()).child(submit))
}

/// The recorded detail as a host that renames, edits comments, reacts and takes every verdict,
/// seen by its author; the recording predates those fields.
pub fn scene_caps(u: &mut super::pr_detail::Ui) {
    let Some(d) = &mut u.detail else { return };
    let all = VERDICTS.map(|v| v.0);
    d["viewer"] = json!("sparsh");
    d["capabilities"]["edit"] = json!({"changeRequest": true, "comment": true});
    d["capabilities"]["reactions"] = json!(true);
    d["capabilities"]["review"]["verdicts"] = json!(all);
    d["viewerPermissions"]["verdicts"] = json!(all);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> Value {
        json!({"projectId": "p", "host": "github.com", "repository": "o/r", "number": 7, "url": "https://github.com/o/r/pull/7", "provider": "github"})
    }

    fn pr(patch: Value) -> Value {
        let mut d = json!({
            "title": "Fix the clamp",
            "body": "Clamps it.",
            "state": "open",
            "provider": "github",
            "viewer": "Sparsh",
            "author": {"login": "sparsh"},
            "capabilities": {"comment": true, "actions": ["close", "reopen", "merge"], "edit": {"changeRequest": true, "comment": true}, "review": {"verdicts": ["approve", "comment", "request-changes"]}},
            "viewerPermissions": {"comment": true, "actions": ["close", "reopen"], "verdicts": ["comment", "approve"]},
        });
        for (k, v) in patch.as_object().unwrap() {
            d[k] = v.clone();
        }
        d
    }

    /// An app holding `d` open, granted or refused source control writes.
    fn app(d: Value, write: bool) -> App {
        let mut s = App::new();
        s.ui.pages.detail.entry = Some(entry());
        s.ui.pages.detail.detail = Some(d);
        let scope = if write { "source-control:write" } else { "orchestration:read" };
        s.ui.pages.pull_requests.row.session.data = Some(json!({"authenticated": true, "permissions": [scope]}));
        s
    }

    #[test]
    fn requests_carry_the_reference_and_only_the_change() {
        let e = entry();
        let d = pr(json!({}));
        let base = |extra: Value| {
            let mut p = json!({"projectId": "p", "host": "github.com", "repository": "o/r", "number": 7});
            p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            p
        };
        assert_eq!(title_save(&d, &e, "  Fix the clamp everywhere "), Some(base(json!({"title": "Fix the clamp everywhere"}))));
        assert_eq!(title_save(&d, &e, " Fix the clamp "), None, "an unchanged title only closes");
        assert_eq!(title_save(&d, &e, "   "), None, "an empty title only closes");
        assert_eq!(comment_update(&e, "c1", "issue-comment", "  edited\n"), Some(base(json!({"commentId": "c1", "kind": "issue-comment", "body": "  edited\n"}))));
        assert_eq!(comment_update(&e, "r1", "review", "x"), None, "review summaries have no endpoint");
        assert_eq!(review_payload(&e, "approve", ""), base(json!({"verdict": "approve", "body": "", "comments": []})));
        assert_eq!(reaction_payload(&e, "c2", "rocket", false), base(json!({"subjectId": "c2", "content": "rocket", "reacted": false})));
    }

    #[test]
    fn editing_follows_the_hosts_permission_then_authorship() {
        let d = pr(json!({}));
        assert!(can_edit_change_request(&d), "the author, ignoring case");
        let stranger = pr(json!({"viewer": "theo", "viewerPermissions": {"actions": []}}));
        assert!(!can_edit_change_request(&stranger));
        let merger = pr(json!({"viewer": "theo", "viewerPermissions": {"actions": ["merge"]}}));
        assert!(can_edit_change_request(&merger), "whoever may merge may edit");
        let refused = pr(json!({"viewerPermissions": {"actions": ["merge"], "editChangeRequest": false}}));
        assert!(!can_edit_change_request(&refused), "a reported permission wins");
        let no_edits = pr(json!({"capabilities": {"edit": {"changeRequest": false, "comment": false}}}));
        assert!(!can_edit_change_request(&no_edits));

        let mine = json!({"id": "c", "kind": "issue-comment", "author": {"login": "SPARSH"}});
        assert!(can_edit_comment(&d, &mine));
        assert!(!can_edit_comment(&d, &json!({"id": "r", "kind": "review", "author": {"login": "sparsh"}})), "not a review summary");
        assert!(!can_edit_comment(&d, &json!({"id": "c", "kind": "review-comment", "author": {"login": "theo"}})));
        assert!(can_edit_comment(&d, &json!({"id": "c", "kind": "review-comment", "author": {"login": "theo"}, "canEdit": true})), "the host's own answer wins");
        assert!(!can_edit_comment(&no_edits, &mine));
    }

    #[test]
    fn the_title_editor_saves_a_change_and_cancels_back_to_the_title() {
        let mut s = app(pr(json!({})), true);
        start_title(&mut s);
        assert_eq!(s.ui.pages.detail.edit.title.as_ref().map(|e| e.text.as_str()), Some("Fix the clamp"));
        save_title(&mut s);
        assert!(s.ui.pages.detail.edit.title.is_none(), "saving the unchanged title closes it");

        start_title(&mut s);
        s.ui.pages.detail.edit.title.as_mut().unwrap().replace_text("Fix the clamp twice");
        save_title(&mut s);
        assert!(s.ui.pages.detail.edit.title.is_some(), "with nowhere to send it the draft stays");
        finish(&mut s, Pending::Title, Err(json!({"_tag": "PullRequestOperationError", "detail": "nope"})));
        assert!(s.ui.pages.detail.edit.title.is_some(), "a refusal keeps the draft to retry");
        s.ui.pages.detail.entry = None;
        finish(&mut s, Pending::Title, Ok(json!({})));
        assert!(s.ui.pages.detail.edit.title.is_none(), "a save closes it");

        start_body(&mut s);
        cancel(&mut s, Target::Body);
        assert!(s.ui.pages.detail.edit.body.is_none());
        start_comment(&mut s, &json!({"id": "c2", "kind": "issue-comment", "body": "Old"}));
        assert_eq!(s.ui.pages.detail.edit.comment.as_ref().map(|c| (c.id.as_str(), c.draft.edit.text.as_str())), Some(("c2", "Old")));
        finish(&mut s, Pending::Comment("c9".into()), Ok(json!({})));
        assert!(s.ui.pages.detail.edit.comment.is_some(), "another comment's save leaves this draft open");
        finish(&mut s, Pending::Comment("c2".into()), Ok(json!({})));
        assert!(s.ui.pages.detail.edit.comment.is_none());
    }

    #[test]
    fn writes_wait_on_the_source_control_grant() {
        let mut s = app(pr(json!({})), false);
        assert!(!writable(&s));
        start_title(&mut s);
        s.ui.pages.detail.edit.title.as_mut().unwrap().replace_text("Renamed");
        save_title(&mut s);
        assert!(s.ui.pages.detail.edit.title.is_some(), "nothing happens without the grant");
        let c = &mut s.ui.pages.detail.edit.composer;
        c.comment.replace_text("LGTM");
        c.close_armed = true;
        post(&mut s, "close");
        assert!(s.ui.pages.detail.edit.composer.close_armed, "nothing is sent without the grant");
        s.ui.pages.pull_requests.row.session.data = Some(json!({"authenticated": true, "permissions": ["source-control:write"]}));
        assert!(writable(&s));
        post(&mut s, "close");
        assert!(!s.ui.pages.detail.edit.composer.close_armed, "with it the comment goes out");
    }

    #[test]
    fn the_composer_offers_what_the_host_and_viewer_allow() {
        let d = pr(json!({}));
        assert_eq!(verdicts(&d), vec!["comment", "approve"], "the form's order, both sides allowing");
        assert_eq!(follow_up(&d), Some("close"));
        assert_eq!(follow_up(&pr(json!({"state": "closed"}))), Some("reopen"));
        assert_eq!(follow_up(&pr(json!({"state": "merged"}))), None);
        assert_eq!(follow_up(&pr(json!({"viewerPermissions": {"actions": []}}))), None);

        assert!(!review_mode(true, 2, false));
        assert!(review_mode(true, 2, true));
        assert!(!review_mode(true, 0, true), "nothing to review with");
        assert!(review_mode(false, 1, false));

        assert_eq!(selected_verdict(&["comment", "approve"], None), Some("comment"));
        assert_eq!(selected_verdict(&["approve", "request-changes"], None), Some("approve"));
        assert_eq!(selected_verdict(&["comment", "approve"], Some("approve")), Some("approve"));
        assert_eq!(selected_verdict(&[], None), None);

        assert!(can_submit("approve", "", false));
        assert!(!can_submit("comment", "  ", false) && can_submit("comment", "ok", false));
        assert!(!can_submit("request-changes", "", true) && can_submit("request-changes", "fix", true));
        assert!(summary_required(&pr(json!({"provider": "forgejo"})), "request-changes"));
        assert!(!summary_required(&d, "request-changes"));
        assert_eq!(review_placeholder("approve", false), "Summarize your review (optional)");
        assert_eq!(review_placeholder("request-changes", true), "Summarize your review (required to request changes)");
        assert_eq!(review_placeholder("comment", false), "Summarize your review or add line comments");
        assert_eq!(verdict_sent("approve", "merge request"), "Merge request approved");
        assert_eq!(verdict_sent("request-changes", "pull request"), "Changes requested");
        assert_eq!(verdict_sent("comment", "pull request"), "Review submitted");
    }

    #[test]
    fn a_sent_review_or_comment_clears_only_what_it_sent() {
        let mut s = app(pr(json!({})), true);
        s.ui.pages.detail.entry = None;
        let c = &mut s.ui.pages.detail.edit.composer;
        c.summary.replace_text("Looks good, one more thing");
        c.comment.replace_text("Thanks!");
        finish(&mut s, Pending::Review { verdict: "comment", body: "Looks good".into() }, Ok(json!({})));
        assert_eq!(s.ui.pages.detail.edit.composer.summary.text, "Looks good, one more thing", "edits made while sending stay");
        finish(&mut s, Pending::Review { verdict: "comment", body: "Looks good, one more thing".into() }, Err(json!({})));
        assert_eq!(s.ui.pages.detail.edit.composer.summary.text, "Looks good, one more thing", "a refusal keeps the draft");
        finish(&mut s, Pending::Review { verdict: "comment", body: "Looks good, one more thing".into() }, Ok(json!({})));
        assert_eq!(s.ui.pages.detail.edit.composer.summary.text, "");
        finish(&mut s, Pending::Post("comment"), Err(json!({})));
        assert_eq!(s.ui.pages.detail.edit.composer.comment.text, "Thanks!");
        finish(&mut s, Pending::Post("comment"), Ok(json!({})));
        assert_eq!(s.ui.pages.detail.edit.composer.comment.text, "");
    }

    #[test]
    fn reactions_lay_over_the_host_until_it_reports_them() {
        let host = vec![
            json!({"content": "rocket", "count": 1, "actors": [], "viewerHasReacted": true}),
            json!({"content": "thumbs-up", "count": 2, "actors": ["theo", "julius"], "viewerHasReacted": false}),
        ];
        assert_eq!(signature(&host), "rocket:1:1 thumbs-up:2:0");
        assert_eq!(apply_pending(&host, &[]), host, "nothing pending keeps the host's order");
        let shown = apply_pending(&host, &[("rocket", false), ("thumbs-up", true), ("heart", true), ("eyes", false)]);
        assert_eq!(shown, vec![json!({"content": "thumbs-up", "count": 3, "actors": ["theo", "julius"], "viewerHasReacted": true}), json!({"content": "heart", "count": 1, "actors": [], "viewerHasReacted": true})]);

        assert_eq!(reaction_tooltip(&host[1]), "theo and julius reacted with thumbs up emoji");
        assert_eq!(reaction_tooltip(&json!({"content": "heart", "count": 5, "actors": ["a", "b"], "viewerHasReacted": true})), "You, a, b, and 2 others reacted with heart emoji");
        assert_eq!(reaction_tooltip(&json!({"content": "eyes", "count": 2, "actors": ["a"], "viewerHasReacted": true})), "You and a reacted with eyes emoji");
        assert_eq!(reaction_tooltip(&json!({"content": "laugh", "count": 1, "actors": [], "viewerHasReacted": false})), "1 person reacted with laugh emoji");

        let mut s = app(pr(json!({})), true);
        s.ui.pages.detail.entry = None;
        s.ui.pages.detail.edit.reactions.insert("c1".into(), ("sig".into(), vec![("heart", true), ("rocket", false)]));
        finish(&mut s, Pending::Reaction { subject: "c1".into(), content: "heart" }, Err(json!({})));
        assert_eq!(s.ui.pages.detail.edit.reactions["c1"].1, vec![("rocket", false)], "a refused reaction drops back to the host's");
    }
}
