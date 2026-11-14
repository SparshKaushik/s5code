//! The detail panel's conversation model (pullRequestDetail.logic): the Summary tab's comment
//! filter, paging and reviewer verdicts, the Timeline's events, and the Code tab's commit scope.

use std::cmp::Reverse;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use rune::{Color, Cursor, El, col, el, icon, row, spacer, text};
use serde_json::{Value, json};

use super::avatars::{Avatars, avatar};
use super::pr_detail::{self, Ui};
use super::pr_row::{capitalized, term};
use super::pull_requests::relative;
use crate::App;
use crate::data::{s as field, ts};
use crate::icons as i;
use crate::kit::{self, Menu, MenuItem, Placement};
use crate::theme::{self, AlphaMul, T, radius, tw};

/// COMMENT_PAGE: conversations shown before "Show N older conversations".
pub const COMMENT_PAGE: usize = 10;
/// COMMIT_PAGE_SIZE in the Code tab's scope menu.
const COMMIT_PAGE: usize = 10;
/// The web keeps this in localStorage (`t3code:pull-request-comment-filter:v1`).
const FILTER_KEY: &str = "pullRequestCommentFilter";

pub fn list<'a>(a: Option<&'a Value>, key: &str) -> &'a [Value] {
    a.and_then(|a| a[key].as_array()).map(Vec::as_slice).unwrap_or(&[])
}

// ---- conversation items -----------------------------------------------------------------

/// PullRequestConversationItem: a loose comment, or a review thread's root and its replies.
pub struct Item<'a> {
    pub id: &'a str,
    pub at: Option<i64>,
    pub root: Cow<'a, Value>,
    pub replies: Vec<Cow<'a, Value>>,
    pub thread: Option<&'a Value>,
    /// Replies the host did not send.
    pub more: usize,
}

impl Item<'_> {
    pub fn comments(&self) -> impl Iterator<Item = &Value> {
        std::iter::once(&*self.root).chain(self.replies.iter().map(|c| &**c))
    }
}

/// pullRequestConversationItems: threads take their comments out of the flat list and land at
/// their first comment's time; loose comments keep the host's order.
pub fn items(a: &Value) -> Vec<Item<'_>> {
    let comments = list(Some(a), "comments");
    let by_id: HashMap<&str, &Value> = comments.iter().map(|c| (field(c, "id"), c)).collect();
    let mut claimed = HashSet::new();
    let mut threads = Vec::new();
    for th in list(Some(a), "reviewThreads") {
        let mut ordered: Vec<Cow<Value>> = th["comments"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| match by_id.get(field(c, "id")) {
                Some(flat) => Cow::Borrowed(*flat),
                None => {
                    let mut c = c.clone();
                    c["kind"] = json!("review-comment");
                    c["path"] = th["path"].clone();
                    c["reviewState"] = Value::Null;
                    Cow::Owned(c)
                }
            })
            .collect();
        ordered.sort_by_key(|c| ts(c, "createdAt").unwrap_or(0));
        if ordered.is_empty() {
            continue;
        }
        let more = th["commentCount"].as_u64().map_or(0, |n| (n as usize).saturating_sub(ordered.len()));
        let root = ordered.remove(0);
        claimed.extend(std::iter::once(&root).chain(&ordered).map(|c| field(c, "id").to_string()));
        threads.push(Item { id: field(th, "id"), at: ts(&root, "createdAt"), root, replies: ordered, thread: Some(th), more });
    }
    threads.sort_by_key(|i| i.at.unwrap_or(0));
    let mut threads = threads.into_iter().peekable();
    let mut out = Vec::new();
    for c in comments {
        if claimed.contains(field(c, "id")) {
            continue;
        }
        let at = ts(c, "createdAt");
        while let Some(next) = threads.next_if(|i| i.at.unwrap_or(0) < at.unwrap_or(0)) {
            out.push(next);
        }
        out.push(Item { id: field(c, "id"), at, root: Cow::Borrowed(c), replies: Vec::new(), thread: None, more: 0 });
    }
    out.extend(threads);
    out
}

/// pullRequestReviewOutcome: the verdicts among a host's review states.
pub fn outcome(state: &str) -> Option<&'static str> {
    match state.trim().to_lowercase().replace('_', "-").as_str() {
        "approved" => Some("approved"),
        "changes-requested" => Some("changes-requested"),
        "dismissed" => Some("dismissed"),
        _ => None,
    }
}

/// isPullRequestCommentFinished.
pub fn finished(c: &Value, thread: Option<&Value>) -> bool {
    thread.is_some_and(|th| th["isResolved"] == true) || outcome(field(c, "reviewState")) == Some("dismissed")
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Filter {
    pub bots: bool,
    pub resolved: bool,
}

pub fn filter(s: &App) -> Filter {
    let saved = s.client_settings.values.get(FILTER_KEY);
    let flag = |key: &str| saved.and_then(|v| v[key].as_bool()).unwrap_or(false);
    Filter { bots: flag("bots"), resolved: flag("resolved") }
}

fn set_filter(s: &mut App, f: Filter) {
    s.client_settings.values.insert(FILTER_KEY.into(), json!({"bots": f.bots, "resolved": f.resolved}));
    s.client_settings.save();
}

/// filterPullRequestConversationItems: any kept member keeps the conversation, and a bot's
/// approval or change request is a verdict rather than chatter.
pub fn keep(item: &Item, f: Filter) -> bool {
    item.comments().any(|c| {
        let verdict = matches!(outcome(field(c, "reviewState")), Some("approved" | "changes-requested"));
        (f.bots || c["author"]["isBot"] != true || verdict) && (f.resolved || !finished(c, item.thread))
    })
}

pub fn comment_count<'a: 'b, 'b>(items: impl IntoIterator<Item = &'b Item<'a>>) -> usize {
    items.into_iter().map(|i| 1 + i.replies.len()).sum()
}

/// pagePullRequestConversationItems: the newest `count`, and how many stay hidden.
pub fn page<I>(items: &[I], count: usize) -> (&[I], usize) {
    let start = items.len().saturating_sub(count);
    (&items[start..], start)
}

pub fn filter_label(hidden: usize) -> String {
    if hidden > 0 { format!("{hidden} hidden") } else { "Filter".into() }
}

pub fn older_label(hidden: usize) -> String {
    format!("Show {} older conversation{} ({hidden} hidden)", hidden.min(COMMENT_PAGE), if hidden == 1 { "" } else { "s" })
}

pub fn all_hidden(hidden: usize) -> String {
    if hidden == 1 { "The only comment is hidden by the filter.".into() } else { format!("All {hidden} comments are hidden by the filter.") }
}

pub fn truncated_label(loaded: usize) -> String {
    format!("This conversation is only partially loaded. {loaded} comment{} loaded; open it on the host to read the rest.", if loaded == 1 { " is" } else { "s are" })
}

fn strip_comments(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// visibleBody: marker-only bodies are empty.
pub fn visible_body(body: &str) -> Option<String> {
    (!strip_comments(body).trim().is_empty()).then(|| body.trim().to_string())
}

/// The one-line preview of a collapsed comment, without markdown marks.
pub fn preview(body: &str) -> String {
    let lines: Vec<String> = strip_comments(body)
        .lines()
        .filter(|l| {
            let l = l.trim().trim_start_matches('>').trim();
            !(l.starts_with("[!") && l.ends_with(']'))
        })
        .map(|l| l.trim_start_matches(|c: char| c.is_whitespace() || ">#*-".contains(c)).replace(['*', '`'], ""))
        .collect();
    lines.join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The Comments section's "Filter" / "N hidden" menu.
pub fn filter_menu(s: &App, t: &T, f: Filter, hidden: usize, overlays: &mut Vec<El>) -> El {
    let trigger = kit::Button::new(filter_label(hidden))
        .variant("ghost-muted")
        .size("xs")
        .icon(&i::LIST_FILTER)
        .on_press(|s: &mut App| toggle_popup(s, "comment-filter"))
        .build(t)
        .label("Filter comments");
    Menu::new("pr-comment-filter")
        .trigger(trigger)
        .placement(Placement::BottomEnd)
        .min_width(200.0)
        .open(s.ui.pages.detail.popup == Some("comment-filter"))
        .on_close(|s: &mut App| s.ui.pages.detail.popup = None)
        .group_label("Show")
        .item(MenuItem::new("Bot comments").checked(f.bots).on_select(move |s: &mut App| set_filter(s, Filter { bots: !f.bots, ..f })))
        .item(MenuItem::new("Resolved or dismissed").checked(f.resolved).on_select(move |s: &mut App| set_filter(s, Filter { resolved: !f.resolved, ..f })))
        .build(&kit::theme(t), overlays)
}

fn toggle_popup(s: &mut App, name: &'static str) {
    let u = &mut s.ui.pages.detail;
    u.popup = if u.popup == Some(name) { None } else { Some(name) };
}

// ---- reviewer verdicts ------------------------------------------------------------------

struct Verdict {
    key: String,
    actor: Value,
    outcome: &'static str,
    at: Option<i64>,
    stale: bool,
}

/// latestPullRequestReviewOutcomes: each reviewer's newest verdict, stale when a commit landed
/// after it; dismissals drop out.
fn latest_outcomes(comments: &[Value], commits: &[Value]) -> Vec<Verdict> {
    let newest = commits.iter().filter_map(|c| ts(c, "committedDate")).max();
    let mut latest: Vec<Verdict> = Vec::new();
    for c in comments {
        let Some(o) = outcome(field(c, "reviewState")) else { continue };
        let key = c["author"]["login"].as_str().map_or_else(|| format!("ghost:{}", field(c, "id")), str::to_string);
        let at = ts(c, "createdAt");
        let verdict = Verdict { key, actor: c["author"].clone(), outcome: o, at, stale: at.zip(newest).is_some_and(|(at, newest)| at < newest) };
        match latest.iter_mut().find(|v| v.key == verdict.key) {
            Some(current) if current.at.zip(at).is_some_and(|(current, at)| current > at) => {}
            Some(current) => *current = verdict,
            None => latest.push(verdict),
        }
    }
    latest.retain(|v| v.outcome != "dismissed");
    latest
}

pub struct Reviewer {
    pub actor: Value,
    pub outcome: Option<&'static str>,
    pub stale: bool,
}

/// The Reviewers row: requested reviewers with their verdicts, then whoever already submitted
/// one (hosts drop reviewed actors from the request list). Logins compare without case.
pub fn reviewers(requested: &[Value], comments: &[Value], commits: &[Value]) -> Vec<Reviewer> {
    let verdicts = latest_outcomes(comments, commits);
    let login = |a: &Value| a["login"].as_str().map(str::to_lowercase);
    let same = |a: &Value, v: &Verdict| v.actor.is_object() && login(a) == login(&v.actor);
    let mut out: Vec<Reviewer> = requested
        .iter()
        .map(|a| {
            let v = verdicts.iter().find(|v| same(a, *v));
            Reviewer { actor: a.clone(), outcome: v.map(|v| v.outcome), stale: v.is_some_and(|v| v.stale) }
        })
        .collect();
    for v in verdicts.iter().filter(|v| !requested.iter().any(|a| same(a, *v))) {
        out.push(Reviewer { actor: v.actor.clone(), outcome: Some(v.outcome), stale: v.stale });
    }
    out
}

/// pullRequestReviewOutcomeLabel.
pub fn outcome_label(outcome: &str) -> &'static str {
    match outcome {
        "approved" => "Approved",
        "changes-requested" => "Changes requested",
        _ => "Review dismissed",
    }
}

fn outcome_glyph(outcome: &str) -> &'static rune::Icon {
    match outcome {
        "approved" => &i::CIRCLE_CHECK,
        "changes-requested" => &i::CIRCLE_X,
        _ => &i::CIRCLE_DASHED,
    }
}

fn outcome_ring(t: &T, outcome: &str) -> Color {
    match outcome {
        "approved" => if t.dark { tw::EMERALD_400 } else { tw::EMERALD_500 },
        "changes-requested" => t.c.destructive,
        _ => t.c.muted_foreground.alpha_mul(0.6),
    }
}

/// PullRequestReviewOutcomeBadge.
pub fn outcome_badge(t: &T, outcome: &str) -> El {
    let c = t.c;
    let (bg, fg, edge) = match outcome {
        "approved" => (c.success.alpha_mul(0.16), c.success_foreground, Color::TRANSPARENT),
        "changes-requested" => (c.destructive.alpha_mul(0.16), c.destructive_foreground, Color::TRANSPARENT),
        _ => (c.background, c.contrast_foreground, c.contrast_input),
    };
    row()
        .h(16.0)
        .px(4.0)
        .gap(4.0)
        .center_y()
        .shrink()
        .radius(4.0)
        .fill(bg)
        .border(1.0, edge)
        .child(icon(outcome_glyph(outcome), 12.0).color(fg))
        .child(text(outcome_label(outcome)).font(theme::weight(theme::TEXT_3XS, theme::MEDIUM)).color(fg))
}

/// reviewStateLabel: a host state that is not a verdict, in sentence case.
pub fn review_state_label(state: &str) -> String {
    let words = state.to_lowercase().replace('_', " ");
    let mut chars = words.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// Overlapping avatars, each ringed and badged by its verdict, named in the tooltip.
pub fn reviewer_avatars(avatars: &Avatars, t: &T, list: &[Reviewer]) -> El {
    let mut out = row().gap(2.0).center_y();
    for (n, r) in list.iter().enumerate() {
        let login = r.actor["login"].as_str().unwrap_or("ghost");
        let named = match r.actor["name"].as_str() {
            Some(name) if !name.is_empty() && name != login => format!("{name} (@{login})"),
            _ => login.to_string(),
        };
        let tip = match r.outcome {
            Some(o) if r.stale => format!("{named}: {} earlier changes", outcome_label(o)),
            Some(o) => format!("{named}: {}", outcome_label(o)),
            None => named,
        };
        let ring = r.outcome.map_or(Color::TRANSPARENT, |o| if r.stale { outcome_ring(t, o).alpha_mul(0.35) } else { outcome_ring(t, o) });
        let mut chip = el()
            .key(format!("pr-reviewer-{n}"))
            .size(20.0, 20.0)
            .shrink()
            .center()
            .radius(radius::FULL)
            .border(2.0, ring)
            .label(tip.clone())
            .help(tip)
            .child(avatar(avatars, &r.actor, 16.0, t));
        if let Some(o) = r.outcome {
            let mark = if o == "approved" { &i::CHECK } else { &i::X };
            chip = chip.child(
                el().absolute(10.0, 10.0)
                    .size(12.0, 12.0)
                    .center()
                    .radius(radius::FULL)
                    .fill(outcome_ring(t, o))
                    .opacity(if r.stale { 0.5 } else { 1.0 })
                    .child(icon(mark, 8.0).color(t.c.background)),
            );
        }
        out = out.child(chip);
    }
    out
}

// ---- timeline -----------------------------------------------------------------------------

/// PullRequestTimelineEvent: the pull request opening, each commit, each conversation, then
/// its merge or close.
pub struct Event<'a> {
    pub kind: &'static str,
    pub at: Option<i64>,
    pub title: String,
    pub actor: Value,
    pub commit: Option<&'a Value>,
    pub item: Option<Item<'a>>,
}

/// buildPullRequestTimeline, newest first. A merge wins over a close: hosts set both.
pub fn timeline<'a>(d: &Value, a: &'a Value) -> Vec<Event<'a>> {
    let term = term(d);
    let lifecycle = |kind, at, title| Event { kind, at, title, actor: Value::Null, commit: None, item: None };
    let mut events = vec![Event { actor: d["author"].clone(), ..lifecycle("opened", ts(d, "createdAt"), format!("opened this {term}")) }];
    for c in list(Some(a), "commits") {
        events.push(Event {
            actor: c["authors"][0].clone(),
            commit: Some(c),
            ..lifecycle("commit", ts(c, "committedDate"), format!("Commit {}", field(c, "oid").chars().take(7).collect::<String>()))
        });
    }
    for item in items(a) {
        let (kind, title) = if field(&item.root, "kind") == "review" { ("review", "reviewed") } else { ("comment", "commented") };
        let (at, actor) = (item.at, item.root["author"].clone());
        events.push(Event { actor, item: Some(item), ..lifecycle(kind, at, title.to_string()) });
    }
    if let Some(at) = ts(d, "mergedAt") {
        events.push(lifecycle("merged", Some(at), format!("{} merged", capitalized(term))));
    } else if let Some(at) = ts(d, "closedAt") {
        events.push(lifecycle("closed", Some(at), format!("{} closed", capitalized(term))));
    }
    events.sort_by_key(|e| Reverse(e.at.unwrap_or(0)));
    events
}

fn marker(content: El) -> El {
    row().w(32.0).h(32.0).shrink().center().child(content)
}

/// LifecycleEvent: opened, merged or closed, under the host's word for it.
pub fn lifecycle_row(s: &App, t: &T, e: &Event<'_>, term: &str) -> El {
    let (glyph, label) = match e.kind {
        "opened" => (&i::GIT_PULL_REQUEST_ARROW, format!("{} opened", capitalized(term))),
        "merged" => (&i::GIT_MERGE, format!("{} merged", capitalized(term))),
        _ => (&i::GIT_PULL_REQUEST_CLOSED, format!("{} closed", capitalized(term))),
    };
    let mut head = row().gap(6.0).center_y();
    let mut aria = e.title.clone();
    if let Some(login) = e.actor["login"].as_str() {
        aria = format!("{login} {aria}");
        head = head.child(text(login.to_string()).font(theme::weight(theme::TEXT_XS, theme::SEMIBOLD)).color(t.c.foreground));
    }
    head = head.child(text(label).font(theme::weight(theme::TEXT_XS, theme::SEMIBOLD)).color(t.c.foreground));
    let mut body = col().grow(1.0).shrink().min_w(0.0).py(6.0).child(head);
    if let Some(at) = e.at {
        body = body.child(text(relative(at, s.now_ms)).font(theme::TEXT_2XS).color(t.c.muted_foreground).mt(2.0));
    }
    row()
        .key(format!("pr-event-{}", e.kind))
        .w_full()
        .gap(16.0)
        .center_y()
        .label(aria)
        .child(marker(icon(glyph, 14.0).color(t.c.muted_foreground)))
        .child(body)
}

/// CommitEvent: opens the Code tab on the change this commit made.
pub fn commit_row(s: &App, t: &T, e: &Event<'_>) -> El {
    let c = e.commit.unwrap_or(&Value::Null);
    let oid = field(c, "oid").to_string();
    let headline = match field(c, "messageHeadline") {
        "" => "Untitled commit".to_string(),
        h => h.to_string(),
    };
    let mark = if e.actor.is_object() { avatar(&s.ui.pages.avatars, &e.actor, 28.0, t) } else { icon(&i::GIT_COMMIT_HORIZONTAL, 14.0).color(t.c.muted_foreground) };
    let small = theme::TEXT_3XS;
    let mut meta = row().mt(4.0).gap(8.0).center_y().child(text(oid.chars().take(7).collect::<String>()).font(theme::mono(small)).color(t.c.muted_foreground));
    if let Some(at) = e.at {
        meta = meta.child(text(relative(at, s.now_ms)).font(small).color(t.c.muted_foreground));
    }
    let mut out = row()
        .key(format!("pr-commit-{oid}"))
        .w_full()
        .gap(16.0)
        .center_y()
        .radius(radius::SM)
        .hover(t.c.accent.alpha_mul(0.4))
        .cursor(Cursor::Pointer)
        .role("button")
        .label(format!("View commit {oid}"))
        .child(marker(mark))
        .child(
            col()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .py(6.0)
                .child(text(headline).font(theme::weight(theme::TEXT_XS, theme::SEMIBOLD)).color(t.c.foreground).truncate())
                .child(meta),
        );
    if let (Some(add), Some(del)) = (c["additions"].as_u64(), c["deletions"].as_u64())
        && (add > 0 || del > 0)
    {
        let stat = theme::mono(small).features("'tnum' 1");
        out = out
            .child(spacer())
            .child(text(format!("+{}", crate::chat::details::thousands(add))).font(stat).color(t.c.diff_addition_foreground))
            .child(text(format!("-{}", crate::chat::details::thousands(del))).font(stat).color(t.c.diff_deletion_foreground));
    }
    out.on_press(move |s: &mut App| pr_detail::open_commit(s, oid.clone()))
}

// ---- commit scope -----------------------------------------------------------------------

/// The scoped commit, while the pull request still has it: after a force-push it reads as
/// "All commits" instead of requesting a diff the host no longer knows.
pub fn scoped_commit(u: &Ui) -> Option<String> {
    let oid = u.commit.as_deref()?;
    list(u.activity.as_ref(), "commits").iter().any(|c| field(c, "oid") == oid).then(|| oid.to_string())
}

/// The Code tab's toolbar: the diff scope menu (All commits, or one commit, newest first) and
/// the file count.
pub fn scope_bar(s: &App, t: &T, overlays: &mut Vec<El>) -> Option<El> {
    let u = &s.ui.pages.detail;
    let mut commits: Vec<&Value> = list(u.activity.as_ref(), "commits").iter().collect();
    if commits.is_empty() {
        return None;
    }
    commits.sort_by_key(|c| Reverse(ts(c, "committedDate").unwrap_or(0)));
    let current = scoped_commit(u);
    let label = current.as_deref().and_then(|oid| commits.iter().find(|c| field(c, "oid") == oid)).map_or("All commits".to_string(), |c| field(c, "messageHeadline").to_string());
    let ink = t.c.secondary_foreground;
    let trigger = row()
        .h(24.0)
        .max_w(256.0)
        .px(8.0)
        .gap(4.0)
        .center_y()
        .radius(radius::MD)
        .fill(t.c.secondary)
        .cursor(Cursor::Pointer)
        .role("button")
        .label(format!("Diff scope: {label}"))
        .child(text(label).font(theme::XS_MEDIUM).color(ink).truncate().shrink().min_w(0.0))
        .child(icon(&i::CHEVRON_DOWN, 14.0).color(ink).opacity(0.7))
        .on_press(|s: &mut App| toggle_popup(s, "commit-scope"));
    let shown = u.scope_commits.max(COMMIT_PAGE);
    let mut menu = Menu::new("pr-commit-scope")
        .trigger(trigger)
        .placement(Placement::BottomStart)
        .min_width(240.0)
        .open(u.popup == Some("commit-scope"))
        .on_close(|s: &mut App| s.ui.pages.detail.popup = None)
        .item(MenuItem::new("All commits").radio(current.is_none()).on_select(|s: &mut App| pr_detail::scope_commit(s, None)));
    for c in commits.iter().take(shown) {
        let oid = field(c, "oid").to_string();
        let short = text(oid.chars().take(7).collect::<String>()).font(theme::mono(theme::TEXT_XS)).color(t.c.muted_foreground);
        menu = menu.item(
            MenuItem::new(field(c, "messageHeadline"))
                .radio(current.as_deref() == Some(oid.as_str()))
                .trailing(short)
                .on_select(move |s: &mut App| pr_detail::scope_commit(s, Some(oid.clone()))),
        );
    }
    if commits.len() > shown {
        menu = menu.item(MenuItem::new(format!("Show more ({} left)", commits.len() - shown)).on_select(move |s: &mut App| s.ui.pages.detail.scope_commits = shown + COMMIT_PAGE));
    }
    let mut bar = row().key("pr-code-toolbar").w_full().h(40.0).px(16.0).gap(12.0).center_y().child(menu.build(&kit::theme(t), overlays));
    if let Some(n) = u.files.as_ref().map(Vec::len) {
        bar = bar.child(text(format!("{n} {}", if n == 1 { "file" } else { "files" })).font(theme::TEXT_XS.features("'tnum' 1")).color(t.c.muted_foreground));
    }
    Some(col().w_full().shrink().child(bar).child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.6))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(id: &str, login: &str, at: &str, extra: Value) -> Value {
        let mut c = json!({"id": id, "kind": "issue-comment", "author": {"login": login}, "body": "hi", "createdAt": at, "reviewState": null});
        c.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        c
    }

    #[test]
    fn threads_land_at_their_first_comment_and_claim_their_comments() {
        let a = json!({
            "comments": [
                comment("c1", "a", "2026-11-06T07:00:00Z", json!({})),
                comment("tc1", "b", "2026-11-06T08:00:00Z", json!({"kind": "review-comment", "body": "flat copy"})),
                comment("c2", "a", "2026-11-06T09:00:00Z", json!({})),
            ],
            "reviewThreads": [{"id": "t1", "path": "x.rs", "isResolved": false, "commentCount": 4, "comments": [
                {"id": "tc2", "author": {"login": "a"}, "body": "reply", "createdAt": "2026-11-06T08:30:00Z"},
                {"id": "tc1", "author": {"login": "b"}, "body": "thread copy", "createdAt": "2026-11-06T08:00:00Z"},
            ]}],
        });
        let items = items(&a);
        assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), ["c1", "t1", "c2"]);
        let thread = &items[1];
        assert_eq!((thread.root["body"].clone(), thread.replies.len(), thread.more), (json!("flat copy"), 1, 2));
        assert_eq!((thread.replies[0]["kind"].clone(), thread.replies[0]["path"].clone()), (json!("review-comment"), json!("x.rs")));
        assert_eq!(comment_count(&items), 4);
    }

    #[test]
    fn the_default_filter_hides_bot_chatter_and_finished_conversations() {
        let bot = json!({"login": "macroscope", "isBot": true});
        let a = json!({
            "comments": [
                comment("chatter", "x", "2026-11-06T07:00:00Z", json!({"author": bot})),
                comment("bot-verdict", "x", "2026-11-06T07:10:00Z", json!({"author": bot, "kind": "review", "reviewState": "CHANGES_REQUESTED"})),
                comment("dismissed", "y", "2026-11-06T07:20:00Z", json!({"kind": "review", "reviewState": "DISMISSED"})),
                comment("human", "y", "2026-11-06T07:30:00Z", json!({})),
            ],
            "reviewThreads": [
                {"id": "resolved", "path": "a.rs", "isResolved": true, "comments": [{"id": "r1", "author": {"login": "y"}, "body": "x", "createdAt": "2026-11-06T08:00:00Z"}]},
                {"id": "bot-thread", "path": "a.rs", "isResolved": false, "comments": [
                    {"id": "b1", "author": bot, "body": "finding", "createdAt": "2026-11-06T08:10:00Z"},
                    {"id": "b2", "author": {"login": "y"}, "body": "fixed", "createdAt": "2026-11-06T08:20:00Z"},
                ]},
            ],
        });
        let items = items(&a);
        let kept = |f: Filter| items.iter().filter(|i| keep(i, f)).map(|i| i.id).collect::<Vec<_>>();
        assert_eq!(Filter::default(), Filter { bots: false, resolved: false });
        assert_eq!(kept(Filter::default()), ["bot-verdict", "human", "bot-thread"], "a human reply keeps the bot's finding");
        assert_eq!(kept(Filter { bots: true, resolved: false }), ["chatter", "bot-verdict", "human", "bot-thread"]);
        assert_eq!(kept(Filter { bots: false, resolved: true }), ["bot-verdict", "dismissed", "human", "resolved", "bot-thread"]);
        let shown = comment_count(items.iter().filter(|i| keep(i, Filter::default())));
        assert_eq!(comment_count(&items) - shown, 3);
        assert_eq!(filter_label(3), "3 hidden");
        assert_eq!(filter_label(0), "Filter");
        assert_eq!(all_hidden(1), "The only comment is hidden by the filter.");
        assert_eq!(all_hidden(4), "All 4 comments are hidden by the filter.");
    }

    #[test]
    fn conversations_page_from_the_newest() {
        let all: Vec<usize> = (0..23).collect();
        let (shown, hidden) = page(&all, COMMENT_PAGE);
        assert_eq!((shown.first(), shown.len(), hidden), (Some(&13), 10, 13));
        assert_eq!(page(&all, 30).1, 0);
        assert_eq!(older_label(13), "Show 10 older conversations (13 hidden)");
        assert_eq!(older_label(1), "Show 1 older conversation (1 hidden)");
        assert_eq!(truncated_label(1), "This conversation is only partially loaded. 1 comment is loaded; open it on the host to read the rest.");
        assert_eq!(truncated_label(40), "This conversation is only partially loaded. 40 comments are loaded; open it on the host to read the rest.");
    }

    #[test]
    fn reviewers_merge_requests_with_submitted_verdicts() {
        let requested = [json!({"login": "Julius"}), json!({"login": "theo"})];
        let comments = [
            comment("v1", "julius", "2026-11-06T09:00:00Z", json!({"kind": "review", "reviewState": "CHANGES_REQUESTED"})),
            comment("v2", "julius", "2026-11-06T08:00:00Z", json!({"kind": "review", "reviewState": "APPROVED"})),
            comment("v3", "macroscope", "2026-11-06T07:00:00Z", json!({"kind": "review", "reviewState": "APPROVED"})),
            comment("v4", "sam", "2026-11-06T07:00:00Z", json!({"kind": "review", "reviewState": "APPROVED"})),
            comment("v5", "sam", "2026-11-06T07:30:00Z", json!({"kind": "review", "reviewState": "DISMISSED"})),
            comment("note", "kim", "2026-11-06T07:30:00Z", json!({"kind": "review", "reviewState": "COMMENTED"})),
        ];
        let commits = [json!({"oid": "a", "committedDate": "2026-11-06T08:30:00Z"})];
        let got: Vec<_> = reviewers(&requested, &comments, &commits).into_iter().map(|r| (field(&r.actor, "login").to_string(), r.outcome, r.stale)).collect();
        assert_eq!(
            got,
            [
                ("Julius".to_string(), Some("changes-requested"), false),
                ("theo".to_string(), None, false),
                ("macroscope".to_string(), Some("approved"), true),
            ],
            "the newest verdict wins even out of order, dismissals drop out, plain reviews are not verdicts"
        );
        assert_eq!(review_state_label("CHANGES_REQUESTED"), "Changes requested");
    }

    #[test]
    fn the_timeline_opens_lists_and_ends_with_a_merge_over_a_close() {
        let d = json!({"provider": "gitlab", "createdAt": "2026-11-06T07:00:00Z", "author": {"login": "a"}, "mergedAt": "2026-11-06T10:00:00Z", "closedAt": "2026-11-06T10:00:00Z"});
        let a = json!({
            "commits": [{"oid": "1a2b3c4d5e", "messageHeadline": "fix", "committedDate": "2026-11-06T08:00:00Z", "authors": [{"login": "a"}]}],
            "comments": [comment("r1", "b", "2026-11-06T09:00:00Z", json!({"kind": "review", "reviewState": "APPROVED"}))],
            "reviewThreads": [],
        });
        let events = timeline(&d, &a);
        let got: Vec<_> = events.iter().map(|e| (e.kind, e.title.as_str())).collect();
        assert_eq!(got, [("merged", "Merge request merged"), ("review", "reviewed"), ("commit", "Commit 1a2b3c4"), ("opened", "opened this merge request")]);
        assert_eq!(events[3].actor["login"], "a");
        assert_eq!(events[2].actor["login"], "a");
        let mut closed = d.clone();
        closed["provider"] = json!("github");
        closed["mergedAt"] = Value::Null;
        assert_eq!(timeline(&closed, &a)[0].title, "Pull request closed");
        closed["closedAt"] = Value::Null;
        assert_eq!(timeline(&closed, &a).len(), 3);
    }

    #[test]
    fn a_scoped_commit_must_still_belong_to_the_pull_request() {
        let mut u = Ui::default();
        u.commit = Some("abc".into());
        u.activity = Some(json!({"commits": [{"oid": "abc"}]}));
        assert_eq!(scoped_commit(&u).as_deref(), Some("abc"));
        u.activity = Some(json!({"commits": [{"oid": "def"}]}));
        assert_eq!(scoped_commit(&u), None);
    }

    #[test]
    fn bodies_of_only_markers_are_empty_and_previews_drop_markdown() {
        assert_eq!(visible_body("<!-- bot:meta -->\n "), None);
        assert_eq!(visible_body(" Hi <!-- x --> "), Some("Hi <!-- x -->".into()));
        assert_eq!(preview("> [!NOTE]\n## **Fix** the `clamp`\n- then <!-- x -->ship"), "Fix the clamp then ship");
    }
}
