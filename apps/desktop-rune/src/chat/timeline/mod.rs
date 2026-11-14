pub mod actions;
pub mod attachments;
pub mod chip;
mod changed;
pub mod error_banner;
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
mod secret;
pub mod banners;
mod child_approvals;
mod link;
mod worktree_setup;
mod inspector;
mod history;
mod md_links;
mod work;
mod lifecycle;
pub mod now;

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
    follow: Follow,
    secrets: secret::State,
    links: link::State,
    banners: banners::State,
    worktree: worktree_setup::State,
    inspector: inspector::State,
    history: history::State,
    md_links: md_links::State,
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
    pub errors: error_banner::State,
}

/// Each row's key in the virtual list, from its id so rows paged in above keep theirs; a
/// repeated id takes a suffix. Find scrolls to marks inside a row by this key.
pub(super) fn row_keys(rows: &[Row]) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    rows.iter()
        .map(|row| {
            let id = match row {
                Row::User(item) | Row::Assistant(item) | Row::Item(item) => s(item, "id"),
                Row::Working { id, .. }
                | Row::Work { id, .. }
                | Row::Tools { id, .. }
                | Row::Thinking { id, .. }
                | Row::Attempt { id, .. }
                | Row::Subagents { id, .. } => id.as_str(),
                Row::Request { request, .. } => s(request, "id"),
            };
            let key = format!("timeline-row-{id}");
            let n = seen.entry(key.clone()).or_default();
            *n += 1;
            if *n == 1 { key } else { format!("{key}-{n}") }
        })
        .collect()
}

fn toggle(app: &mut App, id: &str) {
    if !app.ui.chat.timeline.expanded.remove(id) {
        app.ui.chat.timeline.expanded.insert(id.to_owned());
    }
}

/// ScrollToEndButton's show state, set from the list's scroll offset without a view run: its
/// opacity, and an offset that moves it out of reach of the pointer while hidden.
struct Follow {
    opacity: rune::Live<f32>,
    offset: rune::Live<[f32; 2]>,
}

impl Default for Follow {
    fn default() -> Self {
        Follow { opacity: rune::Live::new(0.0), offset: rune::Live::new(HIDDEN) }
    }
}

const HIDDEN: [f32; 2] = [0.0, 100_000.0];

/// How far above the end the list must be before the button offers to go back.
const AWAY: f32 = 96.0;

/// The floating "Scroll to end" button, over the timeline just above the composer.
pub fn scroll_to_end(app: &App, cx: &mut Cx<App>, t: &T, lane: &crate::chat::details::Lane) -> Option<El> {
    let crate::Route::Thread(thread) = app.frame_route() else { return None };
    if is_empty(app, thread) || app.ui.chat.timeline.find.open {
        return None;
    }
    let composer_h = cx.layout_with("composer-lane", |r| r[3].round()).unwrap_or(112.0);
    let list = format!("timeline-{thread}");
    let follow = &app.ui.chat.timeline.follow;
    // The summary names what waits at the end, out of view while the button shows.
    let queued = app.state().thread_views.get(thread).map_or(0, |v| queue::workflow(&v.projection).queued.len());
    let mut button = crate::kit::button::Button::new("Scroll to end")
        .variant("glass")
        .size("xs")
        .icon(&icons::CHEVRON_DOWN)
        .build(t)
        .on_press_tx(move |_: &mut App, tx| tx.scroll_to_bottom_instant(list.clone()));
    if queued > 0 {
        button = button
            .label(format!("Scroll to end, {queued} queued"))
            .child(text(format!("· {queued} queued")).font(theme::XS_MEDIUM).color(t.c.muted_foreground).a11y_hidden());
    }
    Some(
        row()
            .key("timeline-scroll-to-end")
            .pin(Some(lane.left), None, None, Some(crate::chat::composer::BOTTOM + composer_h + 10.0 + banners::stack_h(cx)))
            .w(lane.width)
            .justify_center()
            .live_opacity(&follow.opacity)
            .live_offset(&follow.offset)
            .child(button),
    )
}

/// `shouldCollapseUserMessage`: over 600 visible characters or 8 lines.
fn collapses(visible: &str) -> bool {
    !visible.trim().is_empty() && (visible.chars().count() > 600 || visible.split('\n').count() > 8)
}

/// How long a copy button shows its check (ANCHORED_COPY_TOAST_TIMEOUT_MS).
const COPIED_MS: i64 = 1000;

fn copied(app: &App, content: &str) -> bool {
    app.ui.chat.timeline.copied.as_ref().is_some_and(|(c, at)| c == content && crate::data::now_ms() - at < COPIED_MS)
}

/// MessageCopyButton: `label` ("Copy message", "Copy code", ...) is its tooltip until it copies.
pub(super) fn copy_button(app: &App, content: &str, t: &T, label: &str) -> El {
    let done = copied(app, content);
    let label = if done { "Copied".to_owned() } else { label.to_owned() };
    let content = content.to_owned();
    row()
        .size(24.0, 24.0)
        .center()
        .child(if done { icon(&icons::CHECK, 12.0).color(t.c.primary) } else { icon(&icons::COPY, 12.0).color(t.c.muted_foreground) })
        .radius(4.0)
        .hover(t.c.muted)
        .focusable(true)
        .role("button")
        .label(label.clone())
        .help(label)
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

/// The T3 wordmark (shell brand) centred in a square box, as web's square-sized T3Wordmark draws.
static T3_MARK: rune::Icon = rune::Icon { name: "t3-mark", view: 94.3941, body: "<path d=\"M 17.92 74.717 V 29.277 H 0 V 18.717 H 48.8 V 29.277 H 30.88 V 74.717 H 17.92 Z M 71.194 75.677 C 67.301 75.677 63.434 75.17 59.594 74.157 C 55.754 73.09 52.501 71.597 49.834 69.677 L 54.874 59.757 C 57.008 61.304 59.488 62.53 62.314 63.437 C 65.141 64.344 67.994 64.797 70.874 64.797 C 74.128 64.797 76.688 64.157 78.554 62.877 C 80.421 61.597 81.354 59.837 81.354 57.597 C 81.354 55.464 80.528 53.784 78.874 52.557 C 77.221 51.33 74.554 50.717 70.874 50.717 H 64.954 V 42.157 L 80.554 24.477 L 81.994 29.117 H 52.634 V 18.717 H 91.834 V 27.117 L 76.314 44.797 L 69.754 41.037 H 73.514 C 80.394 41.037 85.594 42.584 89.114 45.677 C 92.634 48.77 94.394 52.744 94.394 57.597 C 94.394 60.744 93.568 63.704 91.914 66.477 C 90.261 69.197 87.728 71.41 84.314 73.117 C 80.901 74.824 76.528 75.677 71.194 75.677 Z\" fill=\"currentColor\"/>" };

/// What pressing a divider does, with its accessible name and tooltip.
type Action = (String, Box<dyn Fn(&mut App)>);

fn open_thread_action(label: &str, thread: &str) -> Action {
    let thread = thread.to_owned();
    (label.to_owned(), Box::new(move |app: &mut App| crate::shell::sidebar::open_thread(app, &thread)) as Box<dyn Fn(&mut App)>)
}

/// TimelineSystemDivider: rules either side of an icon, a medium label and an inline detail;
/// with an action the middle is a pill button named by it.
fn divider(t: &T, glyph: Option<&'static rune::Icon>, label: String, color: rune::Color, detail: Option<El>, action: Option<Action>) -> El {
    let rule = || el().grow(1.0).h(1.0).fill(t.c.border.alpha_mul(0.7));
    let middle = row()
        .gap(6.0)
        .center_y()
        .shrink()
        .min_w(0.0)
        .py(4.0)
        .radius(theme::radius::FULL)
        .children(glyph.map(|g| icon(g, 12.0).color(color)))
        .child(text(label).font(theme::weight(theme::TEXT_2XS, theme::MEDIUM)).color(color))
        .children(detail);
    let middle = match action {
        Some((name, press)) => middle
            .px(10.0)
            .border(1.0, t.c.border.alpha_mul(0.7))
            .fill(t.c.background)
            .hover(t.c.accent)
            .focusable(true)
            .role("button")
            .cursor(rune::Cursor::Pointer)
            .label(name.clone())
            .help(name)
            .on_press(move |app: &mut App| press(app)),
        None => middle.px(8.0).flex_wrap().justify_center(),
    };
    row().w_full().gap(8.0).py(8.0).center_y().child(rule()).child(middle).child(rule())
}

/// A divider's detail: "· {detail}", dimmed, at most 320 wide.
fn divider_detail(detail: String, color: rune::Color, wrap: bool) -> El {
    let detail = text(format!("·\u{a0}{detail}")).font(theme::TEXT_2XS).color(color.alpha_mul(0.7)).max_w(320.0).min_w(0.0);
    if wrap { detail.wrap() } else { detail.truncate() }
}

/// The open thread's projection.
fn route_projection(app: &App) -> Option<&Value> {
    let crate::Route::Thread(id) = app.frame_route() else { return None };
    app.state().thread_views.get(id).map(|v| &v.projection)
}

/// ContextCompactionTimelineRow: the compaction between rules, its token counts around an arrow
/// and a failure's reason after it.
fn compaction_view(app: &App, item: &Value, t: &T) -> El {
    let items = route_projection(app).map(lifecycle::items).unwrap_or_default();
    let (title, idle) = lifecycle::compaction_context(&items, item);
    let words = lifecycle::compaction_text(item, title.as_deref(), idle);
    let failed = s(item, "status") == "failed";
    let failure = Some(s(item, "summary").trim()).filter(|f| failed && !f.is_empty());
    let color = if failed { t.c.destructive } else { t.c.muted_foreground };
    let line = if failed { t.c.destructive.alpha_mul(0.3) } else { t.c.border.alpha_mul(0.7) };
    let rule = || el().grow(1.0).h(1.0).fill(line);
    let mut content = row()
        .gap(6.0)
        .center_y()
        .shrink()
        .min_w(0.0)
        .child(icon(if failed { &icons::CIRCLE_ALERT } else { &icons::MINIMIZE_2 }, 12.0).color(color));
    let spoken = match words {
        lifecycle::CompactionText::Plain(label) => {
            content = content.child(text(label.clone()).font(theme::TEXT_XS).color(color));
            label
        }
        lifecycle::CompactionText::Tokens { lead, before, after } => {
            content = content
                .child(text(format!("{lead} {before}")).font(theme::TEXT_XS).color(color))
                .child(icon(&icons::ARROW_RIGHT, 12.0).color(color))
                .child(text(format!("{after} tokens")).font(theme::TEXT_XS).color(color));
            format!("{lead} {before} → {after} tokens")
        }
    };
    if let Some(failure) = failure {
        content = content.child(text(format!("· {failure}")).font(theme::TEXT_XS).color(color.alpha_mul(0.8)).truncate().shrink().min_w(0.0));
    }
    let spoken = match failure {
        Some(failure) => format!("{spoken}: {failure}"),
        None => spoken,
    };
    row().w_full().gap(12.0).py(4.0).center_y().label(spoken).child(rule()).child(content).child(rule())
}

/// HandoffRow: "Context handoff" with the providers it left, an arrow, and the one it reached.
fn handoff_view(app: &App, item: &Value, t: &T) -> El {
    let runs = route_projection(app).map_or(&[][..], |p| array(p, "runs"));
    let (from, to) = lifecycle::handoff_endpoints(item, runs);
    let providers = crate::chat::composer::providers(app);
    let failed = s(item, "status") == "failed";
    let color = if failed { t.c.destructive } else { t.c.muted_foreground };
    let endpoint = |(instance, model): &(String, String)| {
        let driver = providers.iter().find(|p| s(p, "instanceId") == instance.as_str()).map_or(instance.as_str(), |p| s(p, "driver"));
        row()
            .gap(6.0)
            .center_y()
            .min_w(0.0)
            .child(crate::chat::composer::provider_icon(t, driver, 12.0))
            .child(text(lifecycle::endpoint_label(providers, instance, model)).font(theme::weight(theme::TEXT_2XS, theme::MEDIUM)).color(color).truncate())
    };
    let mut detail = row().gap(6.0).center_y().flex_wrap().justify_center().min_w(0.0);
    for (index, from_endpoint) in from.iter().enumerate() {
        detail = detail.child(endpoint(from_endpoint));
        if index + 1 < from.len() {
            detail = detail.child(text(",").font(theme::TEXT_2XS).color(color));
        }
    }
    if !from.is_empty() {
        detail = detail.child(icon(&icons::ARROW_RIGHT, 12.0).color(color));
    }
    detail = detail.child(endpoint(&to));
    divider(t, Some(&icons::ARROW_RIGHT_LEFT), "Context handoff".into(), color, Some(detail), None)
}

/// The divider above a thread that another one started (MessagesTimeline's parentThreadLink):
/// a side chat's source, a subagent's parent, or the thread that delegated it.
fn parent_divider(app: &App, thread_id: &str, t: &T) -> Option<El> {
    let threads = &app.state().threads;
    let thread = threads.iter().find(|r| s(r, "id") == thread_id)?;
    let title_of = |id: &str| threads.iter().find(|r| s(r, "id") == id).map(|r| s(r, "title").to_owned()).filter(|t| !t.is_empty());
    let muted = t.c.muted_foreground;
    let lineage = &thread["lineage"];
    let parent = s(lineage, "parentThreadId");
    if !parent.is_empty() && (lineage["sideChat"] == true || s(lineage, "relationshipToParent") == "subagent") {
        let side = lineage["sideChat"] == true;
        let title = title_of(parent).unwrap_or_else(|| "Parent thread".to_owned());
        return Some(divider(
            t,
            Some(if side { &icons::MESSAGES_SQUARE } else { &icons::BOT }),
            (if side { "Context from" } else { "Subagent of" }).to_owned(),
            muted,
            Some(divider_detail(title, muted, false)),
            Some(open_thread_action("Open parent thread", parent)),
        ));
    }
    let from = &thread["delegatedFrom"];
    let parent = s(from, "threadId");
    if parent.is_empty() {
        return None;
    }
    let env = s(from, "environmentId");
    let key = if crate::env::local_id(app).as_deref() == Some(env) { crate::env::LOCAL.to_owned() } else { env.to_owned() };
    let place = Some(crate::env::label(app, &key))
        .filter(|l| !l.is_empty())
        .or_else(|| thread["linkOrigin"]["label"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let title = title_of(parent).unwrap_or_else(|| s(from, "title").to_owned());
    let detail = if place.is_empty() { title } else { format!("{title} on {place}") };
    // Only an environment this client is connected to can open the parent.
    let action = crate::env::connected(app, &key).then(|| {
        let (parent, key) = (parent.to_owned(), key.clone());
        (
            "Open parent thread".to_owned(),
            Box::new(move |app: &mut App| {
                crate::env::adopt(app, &parent, &key);
                crate::shell::sidebar::open_thread(app, &parent);
            }) as Box<dyn Fn(&mut App)>,
        )
    });
    Some(divider(t, Some(&icons::BOT), "Subagent of".into(), muted, Some(divider_detail(detail, muted, false)), action))
}

/// The usage-limit row's words: "Usage limit reached." and, when the provider said, when to retry
/// in this machine's zone.
fn usage_limit_label(app: &App, reset_at: &str) -> String {
    if reset_at.is_empty() {
        return "Usage limit reached.".to_owned();
    }
    let time = actions::upcoming(reset_at, app.now_ms, crate::data::Zone::Local, actions::hour12(app));
    format!("Usage limit reached. Retry after {time} ({}).", actions::zone_name())
}

/// A provider error, notice or retry in the timeline (TimelineWorkEntryRow): a failed error
/// leads with its label in bold and its message on its own line.
fn notice_view(app: &App, item: &Value, t: &T) -> El {
    let kind = s(item, "type");
    let failed_error = kind == "error" && s(item, "status") == "failed";
    let usage = kind == "error" && s(&item["failure"], "class") == "usage_limit";
    let (label, detail) = if kind == "error" {
        lifecycle::provider_error(item)
    } else {
        (s(item, "message").to_owned(), String::new())
    };
    let (label, detail) = if failed_error && usage {
        (usage_limit_label(app, s(&item["failure"], "resetAt")), String::new())
    } else if failed_error {
        (label, s(&item["failure"], "message").to_owned())
    } else {
        (label, detail)
    };
    let warning = kind == "system_notice" || (usage && s(item, "status") != "completed");
    let danger = !warning && (failed_error || (kind == "error" && item["retry"].is_null()));
    let color = if warning {
        t.c.warning
    } else if danger {
        t.c.destructive
    } else {
        t.c.muted_foreground
    };
    let strong = warning || danger;
    let head = row()
        .w_full()
        .gap(12.0)
        .center_y()
        .child(icon(&icons::CIRCLE_ALERT, 16.0).color(color))
        .child(
            text(label)
                .font(if strong { theme::SM_MEDIUM } else { theme::TEXT_SM })
                .color(if strong { color } else { t.c.foreground.alpha_mul(0.8) })
                .selectable(true)
                .wrap()
                .grow(1.0)
                .shrink()
                .min_w(0.0),
        )
        .children(actions::item_stamp(app, item, t));
    col()
        .w_full()
        .px(4.0)
        .py(4.0)
        .gap(4.0)
        .child(head)
        .children((!detail.trim().is_empty()).then(|| {
            text(detail).font(theme::TEXT_SM.lh(22.75)).color(t.c.foreground.alpha_mul(0.8)).selectable(true).wrap().ml(28.0)
        }))
        .children(worktree_setup::retry_button(app, item, t).map(|b| row().ml(28.0).pb(4.0).child(b)))
}

fn item_view(app: &App, item: &Value, t: &T) -> El {
    let kind = s(item, "type");
    let id = s(item, "id").to_owned();
    let expanded = app.ui.chat.timeline.expanded.contains(&id);
    if kind == "assistant_message" {
        // The same inset as a top-level assistant row.
        return col().w_full().px(4.0).child(markdown::view(app, s(item, "text"), &id, t));
    }
    if kind == "compaction" {
        return compaction_view(app, item, t);
    }
    // V2LifecycleRow.
    if kind == "run_interrupt_result" {
        let muted = t.c.muted_foreground;
        let detail = Some(s(item, "message").to_owned()).filter(|m| !m.is_empty()).map(|m| divider_detail(m, muted, true));
        return divider(t, Some(&icons::CIRCLE_STOP), "Run interrupted".into(), muted, detail, None);
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
            (action.to_owned(), Box::new(move |app: &mut App| app.navigate(crate::Route::Thread(target.clone()))) as Box<dyn Fn(&mut App)>)
        });
        return divider(t, Some(&icons::GIT_FORK), label.into(), t.c.muted_foreground, None, open);
    }
    if kind == "handoff" {
        return handoff_view(app, item, t);
    }
    if kind == "link_request" {
        let thread = match app.frame_route() {
            crate::Route::Thread(id) => id.as_str(),
            _ => "",
        };
        return link::view(app, item, thread, t);
    }
    if matches!(kind, "error" | "system_notice") {
        return notice_view(app, item, t);
    }
    if kind == "thread_created" {
        let title = s(item, "title");
        let target = s(item, "targetThreadId").to_owned();
        let name = if title.is_empty() { "created thread" } else { title };
        return row()
            .w_full()
            .gap(8.0)
            .py(4.0)
            .px(4.0)
            .center_y()
            .child(icon(&T3_MARK, 16.0).color(t.c.icon_muted))
            .child(
                text(if title.is_empty() { "Created thread".to_owned() } else { format!("Created thread · {title}") })
                    .font(theme::TEXT_SM)
                    .color(t.c.muted_foreground)
                    .truncate()
                    .grow(1.0)
                    .min_w(0.0),
            )
            .children((!target.is_empty()).then(|| {
                inline_button("Open chat", false, t)
                    .label(format!("Open {name}"))
                    .on_press(move |app: &mut App| crate::shell::sidebar::open_thread(app, &target))
            }));
    }
    if kind == "secret_request" {
        let thread = match app.frame_route() {
            crate::Route::Thread(id) => id.as_str(),
            _ => "",
        };
        return secret::view(app, item, thread, t);
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
            "system" => &T3_MARK,
            _ => &icons::ZAP,
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
    // failedToolIconClassName: a failed tool call tints its icon and says so to screen readers.
    let failed = s(item, "status") == "failed" && kind != "notification";
    let glyph_color = if failed { t.c.destructive.alpha_mul(0.4) } else { t.c.muted_foreground };
    let spoken = if failed { format!("{title}, tool call failed") } else { title.to_owned() };
    let ask = inspector::ask(app, item);
    // notificationChildThreadId: a subagent's or delegated task's notice opens its thread.
    let child_thread = Some(s(&item["source"], "childThreadId"))
        .filter(|c| kind == "notification" && !c.is_empty() && matches!(s(&item["source"], "kind"), "subagent" | "delegated_task"))
        .map(str::to_owned);
    let mut out = col().w_full().gap(4.0);
    let head = row()
        .gap(8.0)
        .py(4.0)
        .px(4.0)
        .radius(6.0)
        .hover(t.c.muted.alpha_mul(0.4))
        .focusable(true)
        .role("button")
        .label(spoken)
        .key(head_key.clone())
        .on_press_tx(move |app: &mut App, tx| {
            tx.keep_on_screen(head_key.clone());
            toggle(app, &id);
            if let Some(ask) = &ask
                && app.ui.chat.timeline.expanded.contains(&id)
            {
                inspector::fetch(app, ask);
            }
        })
        .child(if failed { icon(glyph, 16.0).color(glyph_color).help("Tool call failed") } else { icon(glyph, 16.0).color(glyph_color) })
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
        );
    out = out.child(match child_thread {
        Some(child) => row().w_full().gap(8.0).center_y().child(head.grow(1.0).min_w(0.0)).child(
            inline_button("Open subagent", false, t)
                .label("Open subagent thread")
                .on_press(move |app: &mut App| crate::shell::sidebar::open_thread(app, &child)),
        ),
        None => head.w_full(),
    });
    let inspected = (expanded && todo.is_none() && !matches!(kind, "reasoning" | "notification")).then(|| inspector::view(app, item, t)).flatten();
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
    } else if let Some(body) = inspected {
        out = out.child(body);
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
                .child(copy_button(app, &detail, t, "Copy")),
        );
    }
    if expanded {
        if kind == "file_change" {
            let route_thread = match app.frame_route() {
                crate::Route::Thread(id) => id.clone(),
                _ => String::new(),
            };
            let thread = Some(s(item, "sourceThreadId")).filter(|v| !v.is_empty()).map_or(route_thread, str::to_owned);
            let target = Some(s(item, "sourceItemId")).filter(|v| !v.is_empty()).unwrap_or(s(item, "id")).to_owned();
            out = out.child(
                row().px(28.0).child(
                    crate::kit::Button::new("Open diff")
                        .variant("outline")
                        .size("xs")
                        .on_press(move |app: &mut App| crate::panel::open_tool_diff(app, thread.clone(), target.clone()))
                        .build(t),
                ),
            );
        }
    }
    out
}

/// UserMessageProjectedStatus: a prompt that failed or was cancelled says so under it.
fn status_pill(status: &str, t: &T) -> El {
    text(status.to_owned())
        .font(theme::weight(theme::TEXT_3XS, theme::MEDIUM))
        .color(t.c.destructive)
        .px(6.0)
        .py(2.0)
        .radius(theme::radius::FULL)
        .border(1.0, t.c.destructive.alpha_mul(0.25))
        .fill(t.c.destructive.alpha_mul(0.08))
}

/// FoldedMessageHeader over an automation's or another agent's prompt: one line naming who sent
/// it and its first line; open, the whole prompt under it.
#[allow(clippy::too_many_arguments)]
fn folded_message(app: &App, item: &Value, who: lifecycle::Attribution, words: &str, open: bool, key: String, marks: Option<&find::Marks>, t: &T) -> El {
    let automation = who == lifecycle::Attribution::Automation;
    let sender = s(item, "senderThreadId");
    let label = if automation {
        "Automation".to_owned()
    } else {
        app.state()
            .threads
            .iter()
            .find(|r| !sender.is_empty() && s(r, "id") == sender)
            .map(|r| s(r, "title").trim().to_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Agent".to_owned())
    };
    let plain = crate::chat::composer::chips::display(words).0;
    let summary = plain.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_owned();
    let header = row()
        .w_full()
        .gap(6.0)
        .px(12.0)
        .py(8.0)
        .center_y()
        .radius(16.0)
        .focusable(true)
        .role("button")
        .cursor(rune::Cursor::Pointer)
        .label(format!("{label}: {summary}"))
        .expanded(open)
        .on_press(move |app: &mut App| toggle(app, &key))
        .child(icon(if automation { &icons::CALENDAR_CLOCK } else { &icons::MESSAGES_SQUARE }, 16.0).color(t.c.muted_foreground))
        .child(text(label).font(theme::TEXT_SM).color(t.c.muted_foreground).truncate().max_w(160.0))
        .child(text(summary).font(theme::TEXT_SM).color(t.c.message_foreground).truncate().grow(1.0).min_w(0.0))
        .child(icon(if open { &icons::CHEVRON_UP } else { &icons::CHEVRON_DOWN }, 14.0).color(t.c.muted_foreground));
    let mut bubble = col().radius(16.0).fill(t.c.message_surface).child(header);
    if open {
        let mut body = col().key(format!("user-fold-body:{}", s(item, "id"))).w_full().gap(8.0).px(12.0).pb(12.0);
        for attachment in array(item, "attachments").iter().filter(|a| !s(a, "name").starts_with("preview-annotation-")) {
            body = body.child(attachments::view(app, attachment, t));
        }
        if !words.is_empty() {
            body = body.child(markdown::user(app, words, s(item, "id"), t, marks));
        }
        bubble = bubble.w_full().child(body.max_h(384.0).scroll(ScrollSpec { stick_bottom: false }));
    }
    bubble
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

/// An RPC failure's typed error, out of its `[{_tag: "Fail", error}]` cause.
pub(crate) fn rpc_failure(cause: &Value) -> &Value {
    link::failure(cause)
}

pub fn poll(app: &mut App, tx: &mut rune::Tx) -> bool {
    let mut changed = attachments::poll(app) | question::poll(app) | actions::poll(app) | plan::poll(app) | secret::poll(app)
        | link::poll(app) | banners::poll(app) | child_approvals::poll(app) | worktree_setup::poll(app)
        | inspector::poll(app) | md_links::poll(app) | history::poll(app, tx) | crate::chat::server_update::poll(app);
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

/// An approval waiting in the timeline (TimelinePendingApprovalRow), or, with `child`, in a
/// child thread's card above the composer (ChildThreadApprovals).
fn request_view(app: &App, thread_id: &str, request: &Value, item: Option<&Value>, child: bool, t: &T) -> El {
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
    let mut actions = row().flex_wrap().gap(8.0).center_y();
    if live {
        let more_key = format!("approval-more-{request_id}");
        let more = app.ui.chat.timeline.expanded.contains(&more_key);
        if !child {
            actions = actions
                .child(row().gap(4.0).center_y().child(icon(&icons::SHIELD, 12.0).color(t.c.warning)).child(text("Waiting for approval").font(theme::TEXT_XS).color(t.c.warning)))
                .child(text("·").font(theme::TEXT_XS).color(t.c.muted_foreground));
        }
        let options: Vec<(String, String, String)> = if array(item, "options").is_empty() {
            [("Cancel", "cancel"), ("Decline", "decline"), ("Always allow this session", "acceptForSession"), ("Approve", "accept")]
                .iter()
                .map(|(l, d)| (l.to_string(), d.to_string(), String::new()))
                .collect()
        } else {
            array(item, "options").iter().map(|o| (s(o, "label").to_owned(), s(o, "decision").to_owned(), s(o, "warning").trim().to_owned())).collect()
        };
        let primary = |d: &str| matches!(d, "accept" | "decline");
        let has_more = options.iter().any(|(_, d, _)| !primary(d));
        let ordered = options.iter().filter(|(_, d, _)| primary(d)).chain(options.iter().filter(|(_, d, _)| more && !primary(d)));
        for (label, decision, warning) in ordered {
            let (thread, r, decision) = (thread_id.to_owned(), request_id.to_owned(), decision.clone());
            let mut b = inline_button(label, decision == "accept", t);
            // ProviderApprovalOption.warning: a risky choice carries a warning sign and says why.
            if !warning.is_empty() {
                b = row().gap(4.0).center_y().focusable(true).role("button").label(label).cursor(rune::Cursor::Pointer).help(warning.clone())
                    .child(icon(&icons::TRIANGLE_ALERT, 12.0).color(t.c.warning))
                    .child(text(label.clone()).font(theme::XS_MEDIUM).color(if decision == "accept" { t.c.foreground } else { t.c.muted_foreground }));
            }
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
    } else {
        let gone = if child {
            "Provider process is gone. Interrupt or restart the child thread to respond."
        } else {
            "Provider process is gone. Interrupt or restart the run to respond."
        };
        actions = actions.child(text(gone).font(theme::TEXT_XS).color(t.c.warning));
    }
    // A child card spells the explanation out under its actions instead.
    if !child && let Some(reason) = item["fullAccessOverride"].as_str() {
        actions = actions.child(inline_button("Why am I being asked this?", false, t).help(child_approvals::override_explanation(reason)));
    }
    notes = notes.child(actions);
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
    let keys = row_keys(&rows);
    let mut marks = find::Marks::for_rows(app, &rows, t.dark).into_iter();
    // A run of steers shows the marker once (continuesSteer).
    let mut prev_steer = false;
    let mut stack = col()
        .w(lane.width)
        .growth(GrowthSpec::default())
        .virtualize(100.0)
        // The composer floats over the list's bottom. Scroll extent counts the stack's own
        // padding but not the viewport's, so the clearance lives here.
        .pb(composer_h + crate::chat::composer::BOTTOM + 21.0 + banners::stack_h(cx));
    if let Some(control) = history::view(app, thread_id, t) {
        stack = stack.child(col().key("timeline-row-history").w_full().child(control));
    }
    if let Some(parent) = parent_divider(app, thread_id, t) {
        stack = stack.child(col().key("timeline-row-parent").w_full().child(parent));
    }
    for (index, r) in rows.into_iter().enumerate() {
        let marks = marks.next().flatten();
        let steer = if let Row::User(item) = &r { work::steer(item) } else { None };
        let steer_marker = steer.filter(|_| !prev_steer);
        prev_steer = steer.is_some();
        let (_, body) = match r {
            Row::User(item) => {
                let (who, words, task) = lifecycle::presentation(item);
                let folds = matches!(who, lifecycle::Attribution::Automation | lifecycle::Attribution::Agent);
                let open_key = format!("user-fold:{}", s(item, "id"));
                // A find match inside opens a folded prompt, as web's findReveal does.
                let open = !folds || marks.is_some() || app.ui.chat.timeline.expanded.contains(&open_key);
                let bubble = if folds {
                    folded_message(app, item, who, words, open, open_key, marks.as_ref(), t)
                } else {
                    let mut content = col().gap(8.0);
                    // A picked element's crop shows in its chip's details, not as an image (TimelineUserMessageRow).
                    for attachment in array(item, "attachments").iter().filter(|a| !s(a, "name").starts_with("preview-annotation-")) {
                        content = content.child(attachments::view(app, attachment, t));
                    }
                    if !s(item, "text").is_empty() {
                        use crate::chat::composer::chips;
                        // Context links read as their chips (ContextChip inline in the bubble).
                        let (shown, links) = chips::display(s(item, "text"));
                        // Text without chips renders as Markdown, as web's ChatMarkdown does.
                        let rich = links.is_empty();
                        let records = array(&item["context"], "records");
                        let chip_links = chip::links(&links, records);
                        let pills = links.into_iter().map(|c| {
                            let record = records.iter().find(|r| s(r, "contextId") == c.id);
                            (c.range, chips::style(&c.kind, record))
                        });
                        let highlights = if rich { Vec::new() } else { marks.as_ref().map_or_else(Vec::new, |m| m.plain(&shown)) };
                        let found = if rich { marks.is_some() } else { !highlights.is_empty() };
                        // A find match inside shows the whole message, as web's findReveal does.
                        let fold_key = format!("user-full:{}", s(item, "id"));
                        let foldable = collapses(&shown);
                        let folded = foldable && !found && !app.ui.chat.timeline.expanded.contains(&fold_key);
                        let rendered = rich.then(|| markdown::user(app, s(item, "text"), s(item, "id"), t, marks.as_ref()));
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
                        let body = rendered.unwrap_or(body);
                        // max-h-44 with a 1.75rem fade, as CollapsibleUserMessageBody folds.
                        let body = if folded { col().w_full().max_h(176.0).clip_fade(0.0, 0.0, 0.0, 28.0).child(body) } else { body };
                        let mut bubble = col().key(chip::bubble_key(s(item, "id"))).padding(12.0).radius(16.0).fill(t.c.message_surface).child(body);
                        if foldable {
                            let label = if folded { "Show full message" } else { "Show less" };
                            // A full-width row here would let the text measure at its unwrapped width.
                            bubble = bubble.child(
                                crate::kit::button::Button::new(label)
                                    .variant("ghost-muted")
                                    .size("xs")
                                    .on_press(move |app: &mut App| {
                                        if !app.ui.chat.timeline.expanded.remove(&fold_key) {
                                            app.ui.chat.timeline.expanded.insert(fold_key.clone());
                                        }
                                    })
                                    .build(t)
                                    .expanded(!folded)
                                    .self_end()
                                    .mt(6.0),
                            );
                        }
                        content = content.child(bubble);
                    }
                    content
                };
                let bubble = bubble.max_w(lane.width * 0.8);
                let status = s(item, "status");
                let pill = (!matches!(status, "" | "completed" | "pending" | "waiting")).then(|| status_pill(status, t));
                let switch = lifecycle::model_switch_label(&item["modelSwitch"], crate::chat::composer::providers(app));
                let sender = s(item, "senderThreadId").to_owned();
                let opener = match who {
                    lifecycle::Attribution::Automation if task.is_some() => Some(
                        inline_button("Open automation", false, t).on_press(|app: &mut App| app.navigate(crate::Route::Settings("scheduled-tasks".into()))),
                    ),
                    lifecycle::Attribution::Agent if !sender.is_empty() => {
                        Some(inline_button("Open thread", false, t).on_press(move |app: &mut App| crate::shell::sidebar::open_thread(app, &sender)))
                    }
                    _ => None,
                };
                (
                    s(item, "id").to_owned(),
                    col()
                        .w_full()
                        .hover_group()
                        .gap(4.0)
                        .pb(8.0)
                        .children(switch.map(|label| divider(t, None, label, t.c.muted_foreground, None, None)))
                        .children((who == lifecycle::Attribution::T3Code).then(|| {
                            row().w_full().justify_end().pr(4.0).child(text("Sent by T3 Code").font(theme::TEXT_2XS).color(t.c.muted_foreground.alpha_mul(0.7)))
                        }))
                        .children(steer_marker.map(|help| row().w_full().justify_end().pr(4.0).child(work::steer_marker(help, t))))
                        .child(row().w_full().justify_end().child(bubble))
                        .children(pill.map(|pill| row().w_full().justify_end().pr(4.0).child(pill)))
                        .children(open.then(|| {
                            row().w_full().justify_end().pr(4.0).child(
                                row()
                                    .gap(8.0)
                                    .center_y()
                                    .children(opener)
                                    .child(actions::user(app, thread_id, item, &targets, t))
                                    .group_opacity(0.0, 1.0),
                            )
                        })),
                )
            }
            Row::Assistant(item) => (
                s(item, "id").to_owned(),
                col()
                    .w_full()
                    .hover_group()
                    .px(4.0)
                    .gap(6.0)
                    // A finished reply with no words says so (TimelineAssistantRows).
                    .child(markdown::view_marked(
                        app,
                        if s(item, "text").is_empty() && item["streaming"] != true { "(empty response)" } else { s(item, "text") },
                        s(item, "id"),
                        t,
                        marks.as_ref(),
                    ))
                    .children(changes.get(s(item, "id")).map(|c| changed::card(app, c, t)))
                    .child(row().child(actions::assistant(app, thread_id, item, t).group_opacity(0.0, 1.0))),
            ),
            Row::Working { id, label, compacting } => (id, work::working_view(label, compacting, t)),
            Row::Work { id, label, items } => {
                let expanded = app.ui.chat.timeline.expanded.contains(&id);
                let toggle_id = id.clone();
                let head_key = format!("fold-head-{id}");
                let anchors = history::work_anchors(&items);
                let load = history::work_load(app, &anchors, expanded);
                let label = if load == history::WorkLoad::Failed { format!("{label} · Couldn't load work. Click to retry.") } else { label };
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
                            // An open fold whose work failed retries instead of closing.
                            let retry = history::clear_failed_work(app, &anchors) && app.ui.chat.timeline.expanded.contains(&toggle_id);
                            if !retry {
                                toggle(app, &toggle_id)
                            }
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
                if load == history::WorkLoad::Loading {
                    body = body.child(history::work_skeleton(t));
                }
                if expanded {
                    for entry in work::group(&items) {
                        body = body.child(match entry {
                            work::Entry::One(item) if s(item, "type") == "assistant_message" => {
                                col().w_full().px(4.0).child(markdown::view_marked(app, s(item, "text"), s(item, "id"), t, marks.as_ref()))
                            }
                            work::Entry::One(item) => item_view(app, item, t),
                            work::Entry::Group(id, items) => work::group_view(app, &id, &items, t),
                        });
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
            Row::Tools { id, items } => {
                let view = work::group_view(app, &id, &items, t);
                (id, view)
            }
            Row::Thinking { id } => (id, work::thinking_view(t)),
            Row::Attempt { id, items } => {
                let view = work::attempt_view(app, &id, &items, t);
                (id, view)
            }
            Row::Subagents { id, items } => {
                let view = subagent::group_view(app, &id, &items, t);
                (id, view)
            }
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
                    request_view(app, thread_id, request, item, false, t)
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
    if let Some(setup) = worktree_setup::view(app, thread_id, t) {
        stack = stack.child(col().key("timeline-row-worktree-setup").w_full().py(3.0).child(setup));
    }
    if let Some(present) = now::view(app, thread_id, t) {
        stack = stack.child(col().key("timeline-row-now").w_full().pt(3.0).child(present));
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
        .on_scroll_offset({
            let (opacity, offset) = (app.ui.chat.timeline.follow.opacity.clone(), app.ui.chat.timeline.follow.offset.clone());
            move |at: rune::ScrollPos| {
                let away = at.max[1] - at.offset[1] > AWAY;
                opacity.set(if away { 1.0 } else { 0.0 });
                offset.set(if away { [0.0, 0.0] } else { HIDDEN });
            }
        })
        // Rows share the composer's lane, which the details card narrows.
        .pl(lane.left)
        .pt(21.0)
        .child(stack)
}

/// A pending question's panel, docked on top of the composer.
pub fn composer_requests(app: &App, t: &T) -> Option<El> {
    question::panel(app, t)
}

#[cfg(test)]
mod tests {
    use super::collapses;

    #[test]
    fn long_user_messages_fold() {
        assert!(!collapses("short"));
        assert!(!collapses(&"a\n".repeat(7)));
        assert!(collapses(&"line\n".repeat(8)));
        assert!(collapses(&"x".repeat(601)));
        assert!(!collapses(&" ".repeat(700)));
    }
}
