//! The composer (apps/web ChatComposer, ComposerSurface, ComposerPrimaryActions): a glass card
//! floating over the bottom of the chat lane with the editor, attach, the model chip and send.
//! Drafts are kept per thread (and one for the home draft), as the web composer draft store does.
//! `menu` is the @-mention and slash drawer, `picker` the model picker popover.

mod attachments;
pub mod chips;
mod context;
mod menu;
mod picker;
pub mod provider_icons;
mod search;
mod stash;

use std::collections::HashMap;

use rune::{Color, Cursor, Cx, El, Key, KeyEvent, TextEdit, Tx, auto_grow, col, el, icon, row, spacer, text, text_input};
use serde_json::{Value, json};

use crate::chat::details::Lane;
use crate::chat::timeline::{plan, question};
use crate::data;
use crate::icons;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route};

pub const PLACEHOLDER: &str = "Ask anything. @ files, $ skills, / commands";
/// Distance from the composer to the bottom of the chat area (`pb-5`).
pub const BOTTOM: f32 = 20.0;

pub struct Draft {
    pub edit: TextEdit,
    /// Paths of files queued to send with the message.
    pub attachments: Vec<String>,
    /// A model picked in this draft, overriding the thread's.
    pub model: Option<(String, String)>,
    pub runtime_mode: Option<String>,
    pub interaction_mode: Option<String>,
    /// An effort level picked in this draft (the model's effort option), overriding the thread's.
    pub effort: Option<String>,
    /// Records behind the prompt's context chips (`chips::pill_id`); a chip deleted from the
    /// prompt leaves its record unsent.
    pub contexts: Vec<Value>,
    /// Other model options picked in this draft (speed, agent, switches), as `{id, value}`.
    pub options: Vec<Value>,
}

impl Default for Draft {
    fn default() -> Draft {
        Draft {
            edit: TextEdit::new(""),
            attachments: Vec::new(),
            model: None,
            runtime_mode: None,
            interaction_mode: None,
            effort: None,
            contexts: Vec::new(),
            options: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct Ui {
    pub drafts: HashMap<String, Draft>,
    /// Highlighted row in the open @ or / menu.
    pub menu_active: usize,
    /// The trigger token Escape closed, so the menu stays shut until the token changes.
    pub menu_dismissed: Option<String>,
    pub picker: picker::Ui,
    pub stash: stash::Stash,
    /// The permissions menu opened from the footer chip.
    pub permissions_open: bool,
    mention: menu::Mention,
    launches: Vec<(u64, String)>,
    /// Messages waiting on their attachments to be stored.
    uploads: Vec<attachments::Upload>,
    /// Commands whose failure shows in the route's error banner (setThreadError).
    sends: Vec<(u64, Outgoing)>,
    pub context: context::Hover,
    /// The queued run being rewritten in the composer: thread and run.
    editing: Option<(String, String)>,
    /// The queued message's text when its edit began, to tell a changed edit apart.
    editing_original: String,
    /// The draft whose send was refused for its length; the message follows the prompt.
    length_error: Option<String>,
    /// Threads launched in the background, which stay closed when their launch lands.
    background: Vec<String>,
    /// The prompt Up last recalled: the draft it went into, the message, and the text.
    recalled: Option<(String, String, String)>,
    /// Drafts on disk: read once connected, then the last written copy and the next write check.
    stored: Option<String>,
    store_at: Option<std::time::Instant>,
}

/// A dispatched command's draft key, and the draft it sent (`stored_draft`) to put back when it
/// fails, or null when nothing was cleared.
struct Outgoing {
    key: String,
    draft: Value,
}

/// The draft key for a route: the thread id, or `home` for the new-thread draft.
/// A thread's draft is keyed by its id; a new-thread draft by its project (one per project, as
/// the web composer draft store keeps them), a bare home route naming the default project.
pub fn draft_key(s: &App, route: &Route) -> String {
    match route {
        Route::Thread(id) => id.clone(),
        Route::Home { project_id: Some(id) } => home_key(id),
        _ => crate::chat::home::default_project(s).map_or_else(|| "home".to_owned(), |p| home_key(data::s(p, "id"))),
    }
}

pub fn home_key(project: &str) -> String {
    format!("home:{project}")
}

/// The draft being typed: a queued message under edit keeps its own (`queued-edit:` target),
/// so the thread draft survives the edit.
fn active_key(s: &App) -> String {
    match (s.frame_route(), &s.ui.chat.composer.editing) {
        (Route::Thread(id), Some((thread, run))) if id == thread => format!("queued-edit:{thread}:{run}"),
        (route, _) => draft_key(s, route),
    }
}

fn draft<'a>(s: &'a App) -> Option<&'a Draft> {
    s.ui.chat.composer.drafts.get(&active_key(s))
}

fn draft_mut(s: &mut App) -> &mut Draft {
    let key = active_key(s);
    s.ui.chat.composer.drafts.entry(key).or_default()
}

/// The queued run the open thread's composer is rewriting.
pub fn editing_run(s: &App) -> Option<&str> {
    match (s.frame_route(), &s.ui.chat.composer.editing) {
        (Route::Thread(id), Some((thread, run))) if id == thread => Some(run.as_str()),
        _ => None,
    }
}

pub fn edit_queued(s: &mut App, thread: &str, run: &str, text: &str) {
    s.ui.chat.composer.editing = Some((thread.to_owned(), run.to_owned()));
    s.ui.chat.composer.editing_original = text.to_owned();
    let d = draft_mut(s);
    d.edit = TextEdit::new(text);
    d.edit.set_caret(text.len());
    d.edit.focused = true;
}

/// Leaves a queued-message edit (recoverQueuedMessageEdit): a changed edit moves into the thread's
/// draft when that is empty, so nothing typed is lost; otherwise the edit goes.
pub fn cancel_queued_edit(s: &mut App) {
    end_queued_edit(s, true);
}

fn end_queued_edit(s: &mut App, recover: bool) {
    let Some(thread) = s.ui.chat.composer.editing.as_ref().map(|(thread, _)| thread.clone()) else { return };
    if editing_run(s).is_none() {
        return;
    }
    let key = active_key(s);
    let composer = &mut s.ui.chat.composer;
    let edit = composer.drafts.remove(&key);
    composer.editing = None;
    let changed = edit.as_ref().is_some_and(|d| d.edit.text != composer.editing_original || !d.attachments.is_empty());
    let thread_empty = composer.drafts.get(&thread).is_none_or(|d| d.edit.text.trim().is_empty() && d.attachments.is_empty());
    if let Some(edit) = edit.filter(|_| recover && changed && thread_empty) {
        let d = composer.drafts.entry(thread).or_default();
        d.edit = edit.edit;
        d.attachments = edit.attachments;
    }
    draft_mut(s).edit.focused = true;
}

/// How a message sent now is delivered (resolveComposerDispatchMode): an idle thread starts a
/// turn; a running one steers or queues per `followUpBehavior`, and the alternate key flips it.
/// A turn that cannot take steering, such as a compaction, always queues.
fn dispatch_mode(running: bool, can_steer: bool, follow_up: &str, alternate: bool) -> &'static str {
    if !running {
        return "auto";
    }
    if !can_steer {
        return "queue";
    }
    let queue_default = follow_up == "queue";
    if queue_default != alternate { "queue" } else { "steer" }
}

fn mode_for(s: &App, alternate: bool) -> &'static str {
    let can_steer = !ctx(s)
        .thread
        .and_then(|t| s.state().thread_views.get(data::s(t, "id")))
        .is_some_and(|v| crate::chat::timeline::queue::workflow(&v.projection).skips_compaction);
    let follow_up = crate::settings::value(s, "followUpBehavior");
    // A message sent while an approval waits redirects the agent (resolveComposerDispatchMode).
    let follow_up = if approvals(s).is_empty() { follow_up.as_str().unwrap_or("steer") } else { "steer" };
    dispatch_mode(running(s), can_steer, follow_up, alternate)
}

/// Approvals the open thread waits on; a message sent now declines them.
fn approvals(s: &App) -> Vec<Value> {
    let Some(id) = ctx(s).thread.map(|t| data::s(t, "id")) else { return Vec::new() };
    let Some(view) = s.state().thread_views.get(id) else { return Vec::new() };
    crate::chat::timeline::requests::pending_approvals(&view.projection).into_iter().cloned().collect()
}

/// Prompts Up can recall (buildComposerPromptHistoryEntries): what the user typed themselves,
/// oldest first, with consecutive repeats folded into the newest.
fn history_entries(messages: &[Value]) -> Vec<(&str, String)> {
    let mut out: Vec<(&str, String)> = Vec::new();
    for m in messages {
        let own = data::s(m, "role") == "user"
            && m["context"]["guest"].is_null()
            && m["scheduledTaskId"].is_null()
            && data::s(m, "createdBy") != "agent";
        if !own {
            continue;
        }
        let text = data::s(m, "text").trim();
        let text = text.strip_prefix("Ultrathink:\n").unwrap_or(text).trim();
        if text.is_empty() || text.starts_with(plan::IMPLEMENTATION_PREFIX.trim_end()) {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.1 == text => last.0 = data::s(m, "id"),
            _ => out.push((data::s(m, "id"), text.to_owned())),
        }
    }
    out
}

/// One Up (backward) or Down step (stepComposerPromptHistory): the recalled message and text,
/// `Some((None, ""))` to step past the newest back to an empty prompt, or None to leave the key.
fn step_history(entries: &[(&str, String)], recalled: Option<(&str, &str)>, current: &str, backward: bool) -> Option<(Option<String>, String)> {
    let active = recalled.filter(|(_, text)| *text == current).and_then(|(id, text)| {
        entries.iter().position(|(e, _)| *e == id).or_else(|| entries.iter().rposition(|(_, p)| p == text))
    });
    let pick = |i: usize| entries.get(i).map(|(id, p)| (Some(id.to_string()), p.clone()));
    match (backward, active) {
        (true, None) if !current.is_empty() => None,
        (true, None) => entries.len().checked_sub(1).and_then(pick),
        (true, Some(i)) => i.checked_sub(1).and_then(pick),
        (false, None) => None,
        (false, Some(i)) => pick(i + 1).or(Some((None, String::new()))),
    }
}

fn recall(s: &mut App, backward: bool) -> bool {
    let Some(thread) = ctx(s).thread.map(|t| data::s(t, "id").to_owned()) else { return false };
    let has_chips = draft(s).is_some_and(|d| d.contexts.iter().any(|c| d.edit.pills().any(|(_, id)| id == chips::pill_id(c))));
    if editing_run(s).is_some() || has_attachments(s) || has_chips {
        return false;
    }
    let key = active_key(s);
    let current = draft(s).map(|d| d.edit.text.clone()).unwrap_or_default();
    let caret = draft(s).map(|d| d.edit.range());
    // Only from the caret's edge, so arrows still move through a multi-line prompt.
    let at_edge = caret.is_none_or(|(start, end)| {
        if backward { !current[..start].contains('\n') } else { !current[end..].contains('\n') }
    });
    if !at_edge {
        return false;
    }
    let Some(view) = s.state().thread_views.get(&thread) else { return false };
    let entries = history_entries(rows_messages(&view.projection));
    let recalled = s.ui.chat.composer.recalled.as_ref().filter(|(k, ..)| *k == key).map(|(_, id, text)| (id.as_str(), text.as_str()));
    let Some((id, prompt)) = step_history(&entries, recalled, &current, backward) else { return false };
    s.ui.chat.composer.recalled = id.map(|id| (key, id, prompt.clone()));
    set_text(s, &prompt);
    true
}

fn rows_messages(p: &Value) -> &[Value] {
    p["messages"].as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// What the composer is writing into: a thread, or the home draft and its project.
pub struct Ctx<'a> {
    pub thread: Option<&'a Value>,
    pub project: Option<&'a Value>,
}

pub fn ctx(s: &App) -> Ctx<'_> {
    match s.frame_route() {
        Route::Thread(id) => {
            let thread = s.state().threads.iter().find(|t| data::s(t, "id") == id);
            let project = thread.and_then(|t| project_by_id(s, data::s(t, "projectId")));
            Ctx { thread, project }
        }
        _ => Ctx { thread: None, project: crate::chat::home::draft_project(s) },
    }
}

pub fn project_by_id<'a>(s: &'a App, id: &str) -> Option<&'a Value> {
    s.state().projects.iter().find(|p| data::s(p, "id") == id)
}

pub fn providers(s: &App) -> &[Value] {
    s.state().config.as_ref().and_then(|c| c["providers"].as_array()).map_or(&[], |v| v.as_slice())
}

pub fn provider<'a>(s: &'a App, instance: &str) -> Option<&'a Value> {
    providers(s).iter().find(|p| data::s(p, "instanceId") == instance)
}

fn selection_of(v: &Value) -> Option<(String, String)> {
    let instance = data::s(v, "instanceId");
    (!instance.is_empty()).then(|| (instance.to_string(), data::s(v, "model").to_string()))
}

/// Option ids that name a model's reasoning control (REASONING_EFFORT_OPTION_IDS, `thinking`).
const REASONING_IDS: [&str; 7] = ["reasoningEffort", "effort", "reasoning", "variant", "reasoning_effort", "thinking_level", "thinking"];

/// The selected model's option descriptors.
pub fn descriptors(s: &App) -> Vec<Value> {
    let Some((instance, slug)) = selection(s) else { return Vec::new() };
    provider(s, &instance)
        .and_then(|p| p["models"].as_array()?.iter().find(|m| data::s(m, "slug") == slug)?["capabilities"]["optionDescriptors"].as_array().cloned())
        .unwrap_or_default()
}

/// isReasoningControl.
fn is_reasoning(d: &Value) -> bool {
    (data::s(d, "type") == "select" && data::s(d, "role") == "reasoning") || REASONING_IDS.contains(&data::s(d, "id"))
}

/// The model's reasoning levels: the first reasoning select among its options.
pub fn effort_descriptor(s: &App) -> Option<Value> {
    descriptors(s).into_iter().find(|d| data::s(d, "type") != "boolean" && is_reasoning(d) && d["options"].is_array())
}

/// The selected model's effort descriptor id and its options, without the values a prompt
/// keyword turns on (ultrathink).
pub fn effort_options(s: &App) -> Option<(String, Vec<Value>)> {
    let d = effort_descriptor(s)?;
    let injected: Vec<&str> = d["promptInjectedValues"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let options = d["options"].as_array()?.iter().filter(|o| !injected.contains(&data::s(o, "id"))).cloned().collect();
    Some((data::s(&d, "id").to_owned(), options))
}

/// The footer's speed control (splitSpeed): Codex service tiers or a fast-mode switch.
pub fn speed_descriptor(s: &App) -> Option<Value> {
    let codex = selection(s).and_then(|(i, _)| provider(s, &i).map(|p| data::s(p, "driver") == "codex")).unwrap_or(false);
    descriptors(s).into_iter().find(|d| {
        (codex && data::s(d, "id") == "serviceTier" && data::s(d, "type") == "select") || (data::s(d, "id") == "fastMode" && data::s(d, "type") == "boolean")
    })
}

/// The options the effort column lists below the levels: everything but the levels and speed.
pub fn column_options(s: &App) -> Vec<Value> {
    let effort = effort_descriptor(s).map(|d| data::s(&d, "id").to_owned());
    let speed = speed_descriptor(s).map(|d| data::s(&d, "id").to_owned());
    descriptors(s)
        .into_iter()
        .filter(|d| {
            let id = data::s(d, "id");
            Some(id) != effort.as_deref() && Some(id) != speed.as_deref() && matches!(data::s(d, "type"), "boolean" | "select")
        })
        .collect()
}

/// A descriptor's value for the next message: the draft's pick, the thread's, what the provider
/// reports, else the default choice (or off).
pub fn option_value(s: &App, d: &Value) -> Value {
    let id = data::s(d, "id");
    if let Some(v) = draft(s).and_then(|draft| draft.options.iter().find(|o| data::s(o, "id") == id)).map(|o| o["value"].clone()) {
        return v;
    }
    if let Some(v) = source_options(s).into_iter().find(|o| data::s(o, "id") == id).map(|o| o["value"].clone()) {
        return v;
    }
    if !d["currentValue"].is_null() {
        return d["currentValue"].clone();
    }
    if data::s(d, "type") == "boolean" {
        return Value::Bool(false);
    }
    d["options"].as_array().and_then(|o| o.iter().find(|o| o["isDefault"] == true)).map_or(Value::Null, |o| o["id"].clone())
}

pub fn set_option(s: &mut App, id: &str, value: Value) {
    let d = draft_mut(s);
    d.options.retain(|o| data::s(o, "id") != id);
    d.options.push(json!({"id": id, "value": value}));
}

/// The level `step` places away from `current`, stopping at either end.
fn stepped(ids: &[String], current: Option<&str>, step: isize) -> Option<String> {
    let at = current.and_then(|c| ids.iter().position(|i| i == c))?;
    ids.get(at.checked_add_signed(step)?).cloned()
}

/// `composer.increaseEffort` / `composer.decreaseEffort` (stepReasoningEffort).
fn step_effort(s: &mut App, step: isize) {
    let Some((_, options)) = effort_options(s) else { return };
    let ids: Vec<String> = options.iter().map(|o| data::s(o, "id").to_owned()).collect();
    if let Some(next) = stepped(&ids, effort(s).as_deref(), step) {
        draft_mut(s).effort = Some(next);
    }
}

/// isClaudeUltrathinkPrompt: the word on its own, any case.
fn mentions_ultrathink(text: &str) -> bool {
    let lower = text.to_lowercase();
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    lower.match_indices("ultrathink").any(|(i, m)| !word(lower[..i].chars().next_back()) && !word(lower[i + m.len()..].chars().next()))
}

/// The options the selection already carries: the thread's (or, on a home draft, the source the
/// model came from) when the draft keeps that model, as `[{id, value}]`.
fn source_options(s: &App) -> Vec<Value> {
    let Some(thread) = ctx(s).thread else { return Vec::new() };
    let selection = &thread["modelSelection"];
    if draft(s).and_then(|d| d.model.clone()).is_some_and(|m| Some(m) != selection_of(selection)) {
        return Vec::new();
    }
    match &selection["options"] {
        Value::Array(a) => a.clone(),
        Value::Object(o) => o.iter().map(|(id, value)| json!({"id": id, "value": value})).collect(),
        _ => Vec::new(),
    }
}

/// The effort the next message runs at: the draft's pick, the thread's, else the default.
pub fn effort(s: &App) -> Option<String> {
    let (id, options) = effort_options(s)?;
    if let Some(e) = draft(s).and_then(|d| d.effort.clone()).filter(|e| options.iter().any(|o| data::s(o, "id") == e)) {
        return Some(e);
    }
    let listed = |v: &str| options.iter().any(|o| data::s(o, "id") == v);
    let thread = source_options(s).into_iter().find(|o| data::s(o, "id") == id).and_then(|o| o["value"].as_str().map(str::to_owned));
    let reported = effort_descriptor(s).and_then(|d| d["currentValue"].as_str().filter(|v| listed(*v)).map(str::to_owned));
    thread.or(reported).or_else(|| options.iter().find(|o| o["isDefault"] == true).map(|o| data::s(o, "id").to_owned()))
}

/// The modelSelection to send: the model plus its options, a picked effort replacing the thread's.
fn model_selection(s: &App) -> Option<Value> {
    let (instance, model) = selection(s)?;
    let mut options = source_options(s);
    let known: Vec<String> = descriptors(s).iter().map(|d| data::s(d, "id").to_owned()).collect();
    for picked in draft(s).map(|d| d.options.clone()).unwrap_or_default() {
        let id = data::s(&picked, "id").to_owned();
        if known.contains(&id) {
            options.retain(|o| data::s(o, "id") != id);
            options.push(picked);
        }
    }
    if let (Some(e), Some((id, _))) = (draft(s).and_then(|d| d.effort.clone()), effort_options(s)) {
        options.retain(|o| data::s(o, "id") != id);
        options.push(json!({"id": id, "value": e}));
    }
    let mut v = json!({"instanceId": instance, "model": model});
    if !options.is_empty() {
        v["options"] = Value::Array(options);
    }
    Some(v)
}

/// The model the next message goes to: the draft's pick, the thread's, else (on the home draft)
/// the project's most recent, its default, the settings default, then the first ready provider.
pub fn selection(s: &App) -> Option<(String, String)> {
    if let Some(m) = draft(s).and_then(|d| d.model.clone()) {
        return Some(m);
    }
    let c = ctx(s);
    if let Some(t) = c.thread {
        return selection_of(&t["modelSelection"]);
    }
    if let Some(p) = c.project {
        let pid = data::s(p, "id");
        let recent = s.state().threads.iter().filter(|t| data::s(t, "projectId") == pid).max_by_key(|t| data::ts(t, "updatedAt"));
        if let Some(m) = recent.and_then(|t| selection_of(&t["modelSelection"])).or_else(|| selection_of(&p["defaultModelSelection"])) {
            return Some(m);
        }
    }
    if let Some(m) = s.state().config.as_ref().and_then(|c| selection_of(&c["settings"]["defaultModelSelection"])) {
        return Some(m);
    }
    providers(s).iter().find(|p| data::s(p, "status") == "ready").and_then(|p| {
        let model = p["models"].as_array()?.first()?;
        Some((data::s(p, "instanceId").to_string(), data::s(model, "slug").to_string()))
    })
}

pub fn model_name(s: &App, instance: &str, model: &str) -> String {
    provider(s, instance)
        .and_then(|p| p["models"].as_array()?.iter().find(|m| data::s(m, "slug") == model).map(|m| data::s(m, "name").to_string()))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| model.to_string())
}

pub fn runtime_mode(s: &App) -> String {
    if let Some(m) = draft(s).and_then(|d| d.runtime_mode.clone()) {
        return m;
    }
    if let Some(t) = ctx(s).thread {
        return data::s(t, "runtimeMode").to_string();
    }
    default_runtime_mode(s)
}

pub fn interaction_mode(s: &App) -> String {
    if let Some(m) = draft(s).and_then(|d| d.interaction_mode.clone()) {
        return m;
    }
    ctx(s).thread.map(|t| data::s(t, "interactionMode").to_string()).filter(|m| !m.is_empty()).unwrap_or_else(|| "default".into())
}

/// A run is in flight on the open thread.
pub fn running(s: &App) -> bool {
    ctx(s).thread.is_some_and(|t| !t["activeRunId"].is_null())
}

/// `(mode, label, description, icon)` for each runtime mode (runtimeModeConfig.ts).
pub const RUNTIME_MODES: [(&str, &str, &str, &rune::Icon); 4] = [
    ("approval-required", "Supervised", "Ask before commands and file changes.", &icons::LOCK),
    ("auto-accept-edits", "Auto-accept edits", "Auto-approve edits, ask before other actions.", &icons::PEN_LINE),
    ("auto", "Auto", "Supported providers approve routine actions; others still ask.", &icons::SPARKLES),
    ("full-access", "Full access", "Allow commands and edits without prompts.", &icons::SHIELD_OFF),
];

/// A runtime mode the composer offers, and why an administrator's policy refuses it.
#[derive(Clone)]
pub struct ModeOption {
    pub mode: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub icon: &'static rune::Icon,
    pub disabled: Option<String>,
}

/// resolveCompatibleRuntimeMode + resolveRuntimeModeOptions: the provider's modes (all when it
/// names none), those above the policy's `maxRuntimeMode` disabled with the reason.
fn mode_options(supported: &[&str], policy: &Value) -> Vec<ModeOption> {
    let ceiling = RUNTIME_MODES.iter().position(|m| m.0 == data::s(&policy["limits"], "maxRuntimeMode"));
    RUNTIME_MODES
        .iter()
        .enumerate()
        .filter(|(_, m)| supported.is_empty() || supported.contains(&m.0))
        .map(|(rank, &(mode, label, description, icon))| ModeOption {
            mode,
            label,
            description,
            icon,
            disabled: ceiling.filter(|c| rank > *c).map(|c| format!("{} limits threads to {}.", data::s(policy, "organization"), RUNTIME_MODES[c].1)),
        })
        .collect()
}

pub fn runtime_mode_options(s: &App) -> Vec<ModeOption> {
    let supported: Vec<&str> = selection(s)
        .and_then(|(i, _)| provider(s, &i))
        .and_then(|p| p["supportedRuntimeModes"].as_array())
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let policy = s.state().config.as_ref().map_or(&Value::Null, |c| &c["managedPolicy"]);
    mode_options(&supported, policy)
}

/// The mode the thread really runs in: its own when the provider supports it, else the first
/// the provider does.
pub fn effective_runtime_mode(s: &App) -> String {
    let mode = runtime_mode(s);
    match runtime_mode_options(s).first() {
        Some(first) if !runtime_mode_options(s).iter().any(|o| o.mode == mode) => first.mode.to_owned(),
        _ => mode,
    }
}

/// Why Send is refused (sendDisabledReason): the thread's mode is above the policy's ceiling.
fn send_block(s: &App) -> Option<String> {
    let mode = effective_runtime_mode(s);
    runtime_mode_options(s).into_iter().find(|o| o.mode == mode).and_then(|o| o.disabled)
}

/// No model can be picked: the catalog loaded with no provider to send to.
fn no_provider(s: &App) -> bool {
    s.state().config.is_some() && selection(s).is_none_or(|(i, _)| provider(s, &i).is_none())
}

/// A new-thread draft with no project to start in.
fn project_required(s: &App) -> bool {
    !matches!(s.frame_route(), Route::Thread(_)) && ctx(s).project.is_none()
}

fn disconnected(s: &App) -> bool {
    matches!(s.state().conn, s5_net::Conn::Closed(_))
}

/// The driver a started thread is bound to (deriveLockedProvider), unless its provider session
/// can hand the conversation to another.
pub fn locked_driver(s: &App) -> Option<String> {
    let t = ctx(s).thread?;
    let started = !data::s(t, "latestRunId").is_empty() || t["itemCount"].as_u64().is_some_and(|n| n > 0);
    if !started {
        return None;
    }
    if s.state().thread_views.get(data::s(t, "id")).is_some_and(|v| crate::chat::timeline::queue::supports_handoff(&v.projection)) {
        return None;
    }
    // A provider missing from the catalog still binds the thread to its own driver.
    let driver = |instance: &str| provider(s, instance).map(|p| data::s(p, "driver").to_owned()).or_else(|| (!instance.is_empty()).then(|| instance.to_owned()));
    let session = provider(s, data::s(t, "providerInstanceId")).map(|p| data::s(p, "driver").to_owned());
    session
        .or_else(|| driver(data::s(&t["modelSelection"], "instanceId")))
        .or_else(|| draft(s).and_then(|d| d.model.as_ref()).and_then(|(i, _)| driver(i.as_str())))
}

/// deriveResumableRunId: the latest run that ran, when it was interrupted or stopped at a usage
/// limit.
fn resumable_run(p: &Value) -> Option<String> {
    let runs = p["runs"].as_array()?;
    let end = |r: &Value| data::ts(r, "completedAt").unwrap_or(i64::MAX);
    let ran = |r: &&Value| data::s(r, "status") != "queued" && !(data::s(r, "status") == "cancelled" && r["startedAt"].is_null());
    let run = runs.iter().filter(ran).max_by_key(|r| (end(*r), r["ordinal"].as_i64().unwrap_or(0)))?;
    let limited = data::s(run, "status") == "failed"
        && crate::chat::timeline::error_banner::runtime_error(p).is_some_and(|e| e.class.as_deref() == Some("usage_limit"));
    (data::s(run, "status") == "interrupted" || limited).then(|| data::s(run, "id").to_owned())
}

fn held_queue(p: &Value) -> bool {
    p["runs"].as_array().into_iter().flatten().any(|r| data::s(r, "status") == "queued" && r["queueHeld"] == true)
}

/// The Resume action shows on an idle, resumable thread whose draft is empty (showResumeAction).
fn show_resume(s: &App) -> bool {
    let Some(t) = ctx(s).thread else { return false };
    let Some(view) = s.state().thread_views.get(data::s(t, "id")) else { return false };
    let empty = draft(s).is_none_or(|d| d.edit.text.trim().is_empty() && d.attachments.is_empty() && d.contexts.is_empty());
    empty && editing_run(s).is_none() && !running(s) && (resumable_run(&view.projection).is_some() || held_queue(&view.projection))
}

/// useComposerResume: continues the interrupted run, then releases the turns held behind it.
fn resume(s: &mut App) {
    let Some(id) = ctx(s).thread.map(|t| data::s(t, "id").to_owned()) else { return };
    let Some(view) = s.state().thread_views.get(&id) else { return };
    let (run, held) = (resumable_run(&view.projection), held_queue(&view.projection));
    if run.is_none() && !held {
        return;
    }
    let model = model_selection(s);
    crate::chat::timeline::error_banner::set_local(s, &id, None);
    let Some(net) = s.src.net() else { return };
    let mut requests = Vec::new();
    if let Some(run) = run {
        let mut cmd = json!({
            "type": "message.dispatch", "commandId": s5_net::uuid(), "createdBy": "user", "creationSource": "web",
            "threadId": id, "messageId": s5_net::uuid(), "manualContinuationOfRunId": run,
            "text": "Continue where you left off.", "attachments": [], "dispatchMode": {"type": "start_immediately"},
        });
        if let Some(m) = model {
            cmd["modelSelection"] = m;
        }
        requests.push(net.dispatch(cmd));
    }
    if held {
        requests.push(net.dispatch(json!({"type": "queue.resume", "commandId": s5_net::uuid(), "threadId": id})));
    }
    for request in requests {
        watch(s, request, &id);
    }
}

/// The send in flight for the open draft: its files uploading, or its thread launching.
fn send_busy(s: &App) -> bool {
    let key = active_key(s);
    let c = &s.ui.chat.composer;
    c.uploads.iter().any(|u| u.outgoing.key == key) || c.sends.iter().any(|(r, o)| o.key == key && c.launches.iter().any(|(l, _)| l == r))
}

/// The mode new threads start in (`defaultRuntimeMode`); the footer names any other.
fn default_runtime_mode(s: &App) -> String {
    s.state().config.as_ref().map(|c| data::s(&c["settings"], "defaultRuntimeMode").to_string()).filter(|m| !m.is_empty()).unwrap_or_else(|| "full-access".into())
}

pub fn set_runtime_mode(s: &mut App, mode: &str) {
    draft_mut(s).runtime_mode = Some(mode.to_string());
    if let Route::Thread(id) = s.frame_route().clone()
        && let Some(net) = s.src.net()
    {
        net.dispatch(json!({"type": "thread.runtime-mode.set", "commandId": s5_net::uuid(), "threadId": id, "runtimeMode": mode}));
    }
}

fn toggle_plan(s: &mut App) {
    let next = if interaction_mode(s) == "plan" { "default" } else { "plan" };
    set_interaction_mode(s, next);
}

fn set_interaction_mode(s: &mut App, mode: &str) {
    draft_mut(s).interaction_mode = Some(mode.to_string());
    if let Route::Thread(id) = s.frame_route().clone()
        && let Some(net) = s.src.net()
    {
        net.dispatch(json!({"type": "thread.interaction-mode.set", "commandId": s5_net::uuid(), "threadId": id, "interactionMode": mode}));
    }
}

/// Submits the composer: sends the draft, or with a plan ready and nothing typed, implements it.
pub fn submit(s: &mut App) {
    send(s, false);
}

pub fn has_attachments(s: &App) -> bool {
    draft(s).is_some_and(|d| !d.attachments.is_empty())
}

/// The open thread is rewinding, so Send waits ("Rewinding conversation").
fn rewinding(s: &App) -> bool {
    matches!(s.frame_route(), Route::Thread(id) if crate::chat::timeline::actions::rewinding(s, id))
}

/// getComposerPromptLengthValidationMessage: what the provider accepts in one turn.
const MAX_PROMPT_CHARS: usize = 120_000;

fn prompt_length_message(text: &str) -> Option<String> {
    let over = text.trim().encode_utf16().count().checked_sub(MAX_PROMPT_CHARS).filter(|n| *n > 0)?;
    let unit = if over == 1 { "character" } else { "characters" };
    Some(format!(
        "Prompt is {} {unit} over the {}-character limit. Shorten or split it before sending.",
        context::thousands(over as u64),
        context::thousands(MAX_PROMPT_CHARS as u64)
    ))
}

/// The length message under the editor, once a send was refused for it; it clears as the prompt
/// shrinks back under the limit.
fn length_error(s: &App) -> Option<String> {
    if s.ui.chat.composer.length_error.as_deref() != Some(active_key(s).as_str()) {
        return None;
    }
    let d = draft(s)?;
    prompt_length_message(&chips::outgoing(&d.edit, &d.contexts).0)
}

/// parseStandaloneComposerSlashCommand: `/plan` or `/default` sent alone switches the mode.
fn standalone_mode(text: &str) -> Option<&'static str> {
    match text.trim().to_lowercase().as_str() {
        "/plan" => Some("plan"),
        "/default" => Some("default"),
        _ => None,
    }
}

fn send(s: &mut App, alternate: bool) {
    send_with(s, alternate, false);
}

/// Sends the draft. `background` is web's background submission intent: a new thread launches
/// without opening (composer.sendBackground), and a message to a thread opens a new draft in its
/// project after sending (composer.sendAndNewThread).
fn send_with(s: &mut App, alternate: bool, background: bool) {
    if rewinding(s) || send_busy(s) || send_block(s).is_some() || disconnected(s) || no_provider(s) || project_required(s) {
        return;
    }
    if let (Some(run), Route::Thread(thread)) = (editing_run(s).map(str::to_owned), s.frame_route().clone()) {
        let text = draft(s).map(|d| d.edit.text.trim().to_owned()).unwrap_or_default();
        if text.is_empty() {
            return;
        }
        if let Some(net) = s.src.net() {
            let request = net.dispatch(json!({"type": "queued-run.edit", "commandId": s5_net::uuid(), "threadId": thread, "runId": run, "text": text}));
            s.ui.chat.composer.sends.push((request, Outgoing { key: thread, draft: Value::Null }));
        }
        end_queued_edit(s, false);
        return;
    }
    let mode = mode_for(s, alternate);
    let key = active_key(s);
    let Some(d) = draft(s) else { return };
    let snapshot = stored_draft(d);
    let (text, contexts) = chips::outgoing(&d.edit, &d.contexts);
    let mut text = text.trim().to_string();
    if prompt_length_message(&text).is_some() {
        s.ui.chat.composer.length_error = Some(key);
        return;
    }
    if let Some(mode) = standalone_mode(&text).filter(|_| supports_plan(s) && d.attachments.is_empty() && contexts.is_empty()) {
        set_interaction_mode(s, mode);
        draft_mut(s).edit.clear();
        return;
    }
    // A file that can no longer be sent keeps the draft as it is and says why.
    let uploads = match d.attachments.iter().filter(|p| chips::keeps_attachment(p, &contexts)).map(|p| attachments::stage(p)).collect::<Result<Vec<_>, _>>() {
        Ok(uploads) => uploads,
        Err(message) => {
            crate::chat::timeline::error_banner::set_local(s, &key, Some(&message));
            return;
        }
    };
    if let Route::Thread(thread) = s.frame_route().clone()
        && !text.is_empty()
    {
        let pending = approvals(s);
        if !pending.is_empty() {
            use crate::chat::timeline::requests;
            let pending: Vec<&Value> = pending.iter().collect();
            match requests::redirect(&thread, &pending, &text, !uploads.is_empty() || !contexts.is_empty()) {
                Err(message) => {
                    crate::shell::palette::toast(s, message, None);
                    return;
                }
                Ok(commands) => {
                    let Some(net) = s.src.net() else { return };
                    for command in commands {
                        let request = net.dispatch(command);
                        s.ui.chat.composer.sends.push((request, Outgoing { key: key.clone(), draft: Value::Null }));
                    }
                    // Feedback is the whole reply; otherwise the declines go first, then the steer.
                    if requests::takes_feedback(&pending) {
                        sent(s);
                        return;
                    }
                }
            }
        }
    }
    let mut plan_ref = None;
    if text.is_empty()
        && uploads.is_empty()
        && contexts.is_empty()
        && let (Some((plan_id, markdown)), Route::Thread(id)) = (plan::follow_up(s), s.frame_route().clone())
    {
        // resolvePlanFollowUpSubmission: implementing leaves plan mode.
        text = format!("{}{}", plan::IMPLEMENTATION_PREFIX, markdown.trim());
        plan_ref = Some(json!({"threadId": id, "planId": plan_id}));
        set_interaction_mode(s, "default");
    }
    if text.is_empty() && uploads.is_empty() && contexts.is_empty() {
        return;
    }
    let model = model_selection(s);
    let (runtime, interaction) = (effective_runtime_mode(s), interaction_mode(s));
    let route = s.frame_route().clone();
    let project_id = ctx(s).project.map(|p| data::s(p, "id").to_string());
    let message_id = s5_net::uuid();
    let (thread_id, payload, launch) = match route {
        Route::Thread(id) => {
            let mut cmd = json!({
                "type": "message.dispatch", "commandId": s5_net::uuid(), "createdBy": "user", "creationSource": "web",
                "threadId": id, "messageId": message_id, "text": text, "attachments": [],
                "dispatchMode": {"type": if mode == "queue" { "queue_after_active" } else { "start_immediately" }},
            });
            if !contexts.is_empty() {
                cmd["context"] = json!({"version": 1, "records": contexts});
            }
            if mode != "queue" {
                cmd["deliveryIntent"] = json!(mode);
            }
            if let Some(m) = model {
                cmd["modelSelection"] = m;
            }
            if let Some(plan_ref) = plan_ref {
                cmd["sourcePlanRef"] = plan_ref;
            }
            (id, cmd, false)
        }
        _ => {
            let Some(project_id) = project_id else { return };
            let thread_id = crate::panel::drawer::take_draft_thread(s, &project_id).unwrap_or_else(|| format!("thread:project:{project_id}:{}", s5_net::uuid()));
            let preset = crate::chat::home::take_workspace(s, &project_id);
            let draft_run = crate::chat::details::take_draft_workspace(s);
            // A sidebar "new thread on branch" preset wins over the details card's env mode.
            let workspace = if preset == json!({"type": "root"}) { draft_run.unwrap_or(preset) } else { preset };
            let mut params = json!({
                "commandId": s5_net::uuid(), "creationSource": "web", "threadId": thread_id, "projectId": project_id,
                "title": "New thread", "generateTitle": true, "modelSelection": model, "runtimeMode": runtime,
                "interactionMode": interaction, "workspaceStrategy": workspace,
                "initialMessage": {"messageId": message_id, "text": text, "attachments": []},
            });
            if !contexts.is_empty() {
                params["initialMessage"]["context"] = json!({"version": 1, "records": contexts});
            }
            (thread_id, params, true)
        }
    };
    let launch = launch.then(|| thread_id.clone());
    if background && launch.is_some() {
        s.ui.chat.composer.background.push(thread_id.clone());
    }
    let new_draft = background && launch.is_none();
    s.ui.chat.composer.length_error = None;
    crate::chat::timeline::error_banner::set_local(s, &key, None);
    let outgoing = Outgoing { key, draft: snapshot };
    let Some(net) = s.src.net() else { return };
    let composer = &mut s.ui.chat.composer;
    if uploads.is_empty() {
        deliver(net, composer, payload, launch, json!([]), outgoing);
    } else {
        composer.uploads.push(attachments::start(net, &thread_id, &message_id, uploads, payload, launch, outgoing));
    }
    sent(s);
    if new_draft {
        crate::shell::sidebar::new_local(s);
    }
}

/// Pasted files join the draft's attachments (ChatComposer onPaste): copied files as they are,
/// raw image data as a temporary PNG. Text, and a queued-message edit, paste as usual.
fn on_paste(s: &mut App, content: &rune::ClipboardContent, _: &mut Tx) -> bool {
    if editing_run(s).is_some() {
        return false;
    }
    let files: Vec<String> = content.files().iter().map(|p| p.display().to_string()).collect();
    if !files.is_empty() {
        add_files(s, files);
        return true;
    }
    let Some(image) = content.image().filter(|_| content.text().trim().is_empty()) else { return false };
    let path = std::env::temp_dir().join(format!("pasted-image-{}.png", s5_net::uuid()));
    if std::fs::write(&path, image.png()).is_err() {
        return false;
    }
    add_files(s, [path.display().to_string()]);
    true
}

/// Queues files in the open route's draft; the route's banner says why one was refused. A folder
/// becomes an @ mention instead, relative to the workspace it sits in.
fn add_files(s: &mut App, paths: impl IntoIterator<Item = String>) {
    if editing_run(s).is_some() {
        return;
    }
    let key = active_key(s);
    let root = menu::search_root(s);
    let (folders, files): (Vec<String>, Vec<String>) = paths.into_iter().partition(|p| std::path::Path::new(p).is_dir());
    let d = draft_mut(s);
    for folder in folders {
        let path = std::path::Path::new(&folder);
        let relative = root.as_deref().and_then(|r| path.strip_prefix(r).ok()).map(|p| p.display().to_string()).filter(|p| !p.is_empty());
        let name = path.file_name().map_or_else(|| folder.clone(), |n| n.to_string_lossy().into_owned());
        d.edit.insert_pill(relative.unwrap_or_else(|| folder.clone()), &name);
        d.edit.insert(" ");
        d.edit.focused = true;
    }
    let error = attachments::intake(&mut d.attachments, files);
    crate::chat::timeline::error_banner::set_local(s, &key, error.as_deref());
}

/// Sends a message, or launches the thread it starts, with its stored attachments.
fn deliver(net: &mut s5_net::Net, composer: &mut Ui, mut payload: Value, launch: Option<String>, attachments: Value, outgoing: Outgoing) {
    let request = match launch {
        Some(thread_id) => {
            payload["initialMessage"]["attachments"] = attachments;
            let request = net.request("orchestration.launchThread", payload);
            composer.launches.push((request, thread_id));
            request
        }
        None => {
            payload["attachments"] = attachments;
            net.dispatch(payload)
        }
    };
    composer.sends.push((request, outgoing));
}

/// A sent home draft starts the next one fresh, so it picks up the new thread's model and the
/// default modes (useHandleNewThread); a thread draft keeps its overrides.
fn sent(s: &mut App) {
    let home = !matches!(s.frame_route(), Route::Thread(_));
    let d = draft_mut(s);
    if home {
        *d = Draft::default();
    } else {
        d.edit.clear();
        d.attachments.clear();
        d.contexts.clear();
    }
}

/// Puts a context chip at the caret of the open route's draft and gives it the caret
/// (addComposerContext): a terminal selection, a picked element, a PR.
pub fn add_context(s: &mut App, record: Value) {
    let d = draft_mut(s);
    d.edit.insert_pill(chips::pill_id(&record), &chips::sanitize_label(data::s(&record, "label"), data::s(&record, "kind")));
    d.edit.insert(" ");
    d.edit.focused = true;
    d.contexts.push(record);
}

/// "Add to chat" from a thread's terminal: the selection joins that thread's draft.
pub fn add_terminal_context(s: &mut App, thread: &str, terminal_id: &str, terminal_label: &str, line_start: usize, line_end: usize, text: &str) {
    add_thread_context(s, thread, chips::terminal(terminal_id, terminal_label, line_start, line_end, text));
}

/// Adds `record` to `thread`'s draft (a thread id or a new-thread draft's key), opening it first.
pub fn add_thread_context(s: &mut App, thread: &str, record: Value) {
    if draft_key(s, s.frame_route()) != thread {
        s.navigate(Route::Thread(thread.to_owned()));
    }
    add_context(s, record);
}

/// The contexts still in `thread`'s draft (their pill not deleted).
pub fn thread_contexts<'a>(s: &'a App, thread: &str) -> Vec<&'a Value> {
    let Some(d) = s.ui.chat.composer.drafts.get(thread) else { return Vec::new() };
    d.contexts.iter().filter(|c| d.edit.pills().any(|(_, id)| id == chips::pill_id(c))).collect()
}

/// Takes the context `context_id` and its pill out of `thread`'s draft.
pub fn remove_context(s: &mut App, thread: &str, context_id: &str) {
    let Some(d) = s.ui.chat.composer.drafts.get_mut(thread) else { return };
    let pills: Vec<String> = d.contexts.iter().filter(|c| data::s(c, "contextId") == context_id).map(chips::pill_id).collect();
    for pill in &pills {
        let range = d.edit.pills().find(|(_, id)| id == pill).map(|(r, _)| r);
        if let Some(range) = range {
            let end = range.end + usize::from(d.edit.text[range.end..].starts_with(' '));
            d.edit.set_selection(range.start, end);
            d.edit.insert("");
        }
    }
    d.contexts.retain(|c| data::s(c, "contextId") != context_id);
}

/// Opening a thread or the new-thread draft puts the caret in its composer (ChatView).
pub fn focus(s: &mut App, route: &Route) {
    let key = draft_key(s, route);
    s.ui.chat.composer.drafts.entry(key).or_default().edit.focused = true;
}

fn stop(s: &mut App) {
    let Some(t) = ctx(s).thread else { return };
    let (id, run) = (data::s(t, "id").to_string(), data::s(t, "activeRunId").to_string());
    let Some(net) = s.src.net() else { return };
    let request = if run.is_empty() {
        net.dispatch(json!({"type": "thread.stop", "commandId": s5_net::uuid(), "threadId": id}))
    } else {
        net.dispatch(json!({"type": "run.interrupt", "commandId": s5_net::uuid(), "threadId": id, "runId": run}))
    };
    s.ui.chat.composer.sends.push((request, Outgoing { key: id, draft: Value::Null }));
}

/// Folds server replies the composer waits on (launches, file searches). Called from `poll`.
pub fn poll(s: &mut App) -> bool {
    if s.src.net().is_none() {
        return false;
    }
    let mut changed = persist(s) | stash::persist(s);
    let Some(net) = s.src.net() else { return false };
    let composer = &mut s.ui.chat.composer;
    let mut failed = Vec::new();
    let mut i = 0;
    while i < composer.uploads.len() {
        let Some(stored) = attachments::advance(net, &mut composer.uploads[i]).transpose() else {
            i += 1;
            continue;
        };
        let upload = composer.uploads.remove(i);
        match stored {
            Ok(stored) => deliver(net, composer, upload.payload, upload.launch, stored, upload.outgoing),
            Err(e) => {
                let background = upload.launch.as_ref().map(|id| composer.background.contains(id));
                composer.background.retain(|b| Some(b) != upload.launch.as_ref());
                failed.push((upload.outgoing, e, background));
            }
        }
        changed = true;
    }
    let mut opened = None;
    let composer = &mut s.ui.chat.composer;
    let mut i = 0;
    while i < composer.sends.len() {
        let Some(reply) = net.take_reply(composer.sends[i].0) else {
            i += 1;
            continue;
        };
        let (request, outgoing) = composer.sends.remove(i);
        let launch = composer.launches.iter().position(|(r, _)| *r == request).map(|at| composer.launches.remove(at).1);
        // A background launch stays where the user is.
        let quiet = launch.as_ref().is_some_and(|id| composer.background.contains(id));
        if let Some(id) = &launch {
            composer.background.retain(|b| b != id);
        }
        match (reply, launch) {
            (Ok(v), Some(thread_id)) if !quiet => opened = Some(v["threadId"].as_str().unwrap_or(&thread_id).to_string()),
            (Ok(_), _) => {}
            (Err(e), launch) => failed.push((outgoing, e, launch.map(|_| quiet))),
        }
        changed = true;
    }
    for (outgoing, error, background) in failed {
        send_failed(s, outgoing, &error, background);
    }
    let Some(net) = s.src.net() else { return changed };
    changed |= menu::poll(&mut s.ui.chat.composer.mention, net);
    if let Some(id) = opened {
        crate::panel::adopt(s, &id);
        s.navigate(Route::Thread(id));
        changed = true;
    }
    changed
}

/// Drafts on disk, as the web composer draft store persists them: each draft's prompt with its
/// pill ranges, attachments and context records. Empty drafts and queued-message edits
/// stay out.
fn stored(drafts: &HashMap<String, Draft>) -> Value {
    let mut out = serde_json::Map::new();
    for (key, d) in drafts {
        let blank = d.edit.text.trim().is_empty() && d.attachments.is_empty();
        if key.starts_with("queued-edit:") || (blank && d.model.is_none() && d.runtime_mode.is_none() && d.interaction_mode.is_none() && d.effort.is_none() && d.options.is_empty()) {
            continue;
        }
        out.insert(key.clone(), stored_draft(d));
    }
    Value::Object(out)
}

fn stored_draft(d: &Draft) -> Value {
    let pills: Vec<Value> = d.edit.pills().map(|(r, id)| json!([r.start, r.end, id])).collect();
    let contexts: Vec<&Value> = d.contexts.iter().filter(|c| d.edit.pills().any(|(_, id)| id == chips::pill_id(c))).collect();
    json!({
        "prompt": d.edit.text, "pills": pills, "attachments": d.attachments, "model": d.model,
        "runtimeMode": d.runtime_mode, "interactionMode": d.interaction_mode, "effort": d.effort, "contexts": contexts,
        "options": d.options,
    })
}

/// Shows a command's failure in the banner of `key`'s route.
fn watch(s: &mut App, request: u64, key: &str) {
    s.ui.chat.composer.sends.push((request, Outgoing { key: key.to_owned(), draft: Value::Null }));
}

/// A failed send shows in its route's banner, and its draft comes back unless a new one was started.
/// `launch` is set for a new thread's launch: whether it went to the background.
fn send_failed(s: &mut App, outgoing: Outgoing, error: &Value, launch: Option<bool>) {
    let message = crate::shell::palette::error_message(error, "Failed to send message.");
    crate::chat::timeline::error_banner::set_local(s, &outgoing.key, Some(&message));
    if let Some(background) = launch
        && (background || draft_key(s, s.frame_route()) != outgoing.key)
    {
        launch_failed(s, &outgoing.key, background, &message);
    }
    if outgoing.draft.is_null() {
        return;
    }
    let blank = s.ui.chat.composer.drafts.get(&outgoing.key).is_none_or(|d| d.edit.text.trim().is_empty() && d.attachments.is_empty());
    if blank && let Some(draft) = restored(&json!({"draft": outgoing.draft})).remove("draft") {
        s.ui.chat.composer.drafts.insert(outgoing.key, draft);
    }
}

/// Web's toast for a new thread that failed to start out of sight: in the background, or after
/// the user left its draft. "Open draft" returns to the restored draft.
fn launch_failed(s: &mut App, key: &str, background: bool, message: &str) {
    let title = if background { "Background task failed" } else { "Could not start thread" };
    let project = key.strip_prefix("home:").map(str::to_owned);
    let open: crate::shell::palette::ToastAction = (
        "Open draft".to_owned(),
        true,
        std::rc::Rc::new(move |s: &mut App, _: &mut rune::Tx| s.navigate(Route::Home { project_id: project.clone() })),
    );
    crate::shell::palette::toast_error_brief(s, title, Some(message));
    crate::shell::palette::set_toast_actions(s, vec![open]);
}

fn restored(v: &Value) -> HashMap<String, Draft> {
    let strings = |v: &Value| v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect::<Vec<_>>()).unwrap_or_default();
    let text = |v: &Value| v.as_str().map(str::to_owned);
    let mut out = HashMap::new();
    for (key, d) in v.as_object().into_iter().flatten() {
        let prompt = data::s(d, "prompt");
        let mut edit = TextEdit::new(prompt);
        let mut pills: Vec<(usize, usize, &str)> = d["pills"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| Some((p[0].as_u64()? as usize, p[1].as_u64()? as usize, p[2].as_str()?)))
            .filter(|(a, b, _)| a < b && *b <= prompt.len() && prompt.is_char_boundary(*a) && prompt.is_char_boundary(*b))
            .collect();
        // Last first, so earlier ranges stay where they were.
        pills.sort_by_key(|p| std::cmp::Reverse(p.0));
        for (a, b, id) in pills {
            edit.replace_with_pill(a..b, id, &prompt[a..b]);
        }
        edit.set_caret(edit.text.len());
        edit.focused = false;
        let model = strings(&d["model"]);
        out.insert(
            key.clone(),
            Draft {
                edit,
                attachments: strings(&d["attachments"]),
                model: (model.len() == 2).then(|| (model[0].clone(), model[1].clone())),
                runtime_mode: text(&d["runtimeMode"]),
                interaction_mode: text(&d["interactionMode"]),
                effort: text(&d["effort"]),
                contexts: d["contexts"].as_array().cloned().unwrap_or_default(),
                options: d["options"].as_array().cloned().unwrap_or_default(),
            },
        );
    }
    out
}

/// Stored drafts with the legacy shared "home" draft under `home` (its project's key), else dropped.
fn migrated(mut drafts: HashMap<String, Draft>, home: Option<&str>) -> HashMap<String, Draft> {
    if let (Some(draft), Some(home)) = (drafts.remove("home"), home) {
        drafts.entry(home.to_owned()).or_insert(draft);
    }
    drafts
}

/// Reads the stored drafts once the shell has loaded (so legacy drafts find their project), then
/// writes them back at most every 300 ms when they change. A draft already touched this session
/// wins over its stored copy.
fn persist(s: &mut App) -> bool {
    if s.ui.chat.composer.stored.is_none() && !s.state().shell_ready {
        return false;
    }
    let path = crate::settings::client::sibling_path("composer-drafts.json");
    // Drafts once shared one "home" key; that draft moves to the default project's, or goes.
    let legacy_home = crate::chat::home::default_project(s).map(|p| home_key(data::s(p, "id")));
    let composer = &mut s.ui.chat.composer;
    if composer.stored.is_none() {
        let disk = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
        let mut changed = false;
        for (key, draft) in disk.as_ref().map(|d| migrated(restored(d), legacy_home.as_deref())).unwrap_or_default() {
            let slot = composer.drafts.entry(key).or_default();
            if slot.edit.text.is_empty() && slot.attachments.is_empty() {
                let focused = slot.edit.focused;
                *slot = draft;
                slot.edit.focused = focused;
                changed = true;
            }
        }
        composer.stored = Some(stored(&composer.drafts).to_string());
        return changed;
    }
    let now = std::time::Instant::now();
    if composer.store_at.is_some_and(|at| now < at) {
        return false;
    }
    composer.store_at = Some(now + std::time::Duration::from_millis(300));
    let next = stored(&composer.drafts).to_string();
    if composer.stored.as_deref() != Some(next.as_str()) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, &next) {
            eprintln!("composer: saving drafts failed: {e}");
        }
        composer.stored = Some(next);
    }
    false
}

/// Keys inside the editor: the open menu's arrows, Return and Tab, Escape, and Shift+Tab for plan.
/// Escape never stops a turn; the stop button does (composerCommandKey).
fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if s.ui.chat.composer.picker.open {
        return false;
    }
    if stash::on_key(s, e) {
        return true;
    }
    if let Some(m) = menu::open_menu(s).filter(|_| question::active(s).is_none()) {
        let count = menu::items(s, &m).len();
        let active = &mut s.ui.chat.composer.menu_active;
        match e.key {
            Key::Up if count > 0 => {
                *active = (*active + count - 1) % count;
                return true;
            }
            Key::Down if count > 0 => {
                *active = (*active + 1) % count;
                return true;
            }
            Key::Enter | Key::Tab if count > 0 && !e.mods.shift => {
                let i = (*active).min(count - 1);
                menu::choose(s, &m, i, tx);
                return true;
            }
            Key::Escape => {
                s.ui.chat.composer.menu_dismissed = Some(m.token.clone());
                return true;
            }
            _ => {}
        }
    }
    let thread = ctx(s).thread.map(|t| data::s(t, "id").to_owned());
    let plain = !e.mods.shift && !e.mods.cmd && !e.mods.alt && !e.mods.ctrl && !e.mods.logo;
    let answering = question::editor(s).is_some();
    // composerSubmissionIntentForKey: the bound send variants, each under its own `when`.
    let bound = SEND_COMMANDS.into_iter().find(|c| crate::panel::shortcut(s, e, c));
    match bound {
        Some("thread.steerQueuedMessage") => {
            if let Some(thread) = thread {
                crate::chat::timeline::queue::steer_next(s, &thread);
            }
            return true;
        }
        Some("thread.editQueuedMessage") if !answering => {
            return thread.is_some_and(|thread| crate::chat::timeline::queue::edit_latest(s, &thread));
        }
        Some("composer.sendAlternate" | "composer.sendBackground" | "composer.sendAndNewThread") if !answering && !tx.key_repeat() => {
            send_with(s, bound == Some("composer.sendAlternate"), bound != Some("composer.sendAlternate"));
            return true;
        }
        _ => {}
    }
    match e.key {
        Key::Enter if !answering && !e.mods.shift && !e.mods.alt && (e.mods.cmd || plain) => {
            let text = draft(s).map(|d| d.edit.text.as_str()).unwrap_or("");
            let shortcut = crate::settings::value(s, "sendShortcut");
            let shortcut = shortcut.as_str().unwrap_or("enter");
            let needs_mod = shortcut == "mod-enter" || (shortcut == "mod-enter-multiline" && text.contains('\n'));
            if plain && needs_mod {
                draft_mut(s).edit.insert("\n");
            } else {
                send(s, false);
            }
            true
        }
        Key::Up | Key::Down if plain && !answering => recall(s, e.key == Key::Up),
        Key::Escape if editing_run(s).is_some() => {
            cancel_queued_edit(s);
            true
        }
        Key::Tab if e.mods.shift => {
            if supports_plan(s) {
                toggle_plan(s);
            }
            true
        }
        _ => false,
    }
}

const SEND_COMMANDS: [&str; 5] = ["thread.steerQueuedMessage", "thread.editQueuedMessage", "composer.sendAlternate", "composer.sendBackground", "composer.sendAndNewThread"];

/// Whether the open route's prompt has the keys (`composerFocus`).
pub fn focused(s: &App) -> bool {
    s.ui.chat.composer.drafts.get(&active_key(s)).is_some_and(|d| d.edit.focused)
}

/// The composer's window-wide shortcuts (`modelPicker.toggle`, `composer.effort`,
/// `composer.mode`), which the web binds outside the terminal. Effort lives in the model picker.
pub fn shortcut(s: &mut App, e: &KeyEvent) -> bool {
    if !matches!(s.frame_route(), Route::Thread(_) | Route::Home { .. }) || crate::panel::terminal_active(s) || ctx(s).project.is_none() {
        return false;
    }
    if picker::step_provider(s, e) {
        return true;
    }
    // ChatView binds steering the next queued message outside the prompt too.
    if let Some(thread) = ctx(s).thread.map(|t| data::s(t, "id").to_owned())
        && crate::panel::shortcut(s, e, "thread.steerQueuedMessage")
    {
        crate::chat::timeline::queue::steer_next(s, &thread);
        return true;
    }
    if crate::panel::shortcut(s, e, "modelPicker.toggle") || crate::panel::shortcut(s, e, "composer.effort") {
        picker::toggle(s);
        return true;
    }
    // composer.mode opens Permissions: the footer chip's menu, else the picker's.
    if crate::panel::shortcut(s, e, "composer.mode") {
        if permissions_chip_shown(s) {
            s.ui.chat.composer.permissions_open = true;
        } else {
            picker::open_permissions(s);
        }
        return true;
    }
    if crate::panel::shortcut(s, e, "composer.increaseEffort") {
        step_effort(s, 1);
        return true;
    }
    if crate::panel::shortcut(s, e, "composer.decreaseEffort") {
        step_effort(s, -1);
        return true;
    }
    // A pending question has no prompt to set aside, so the shortcut only shows the stash.
    if crate::panel::shortcut(s, e, "composer.stash") {
        if question::active(s).is_some() {
            stash::toggle(s);
        } else if editing_run(s).is_none() {
            stash::stash(s);
        }
        return true;
    }
    stash::on_key(s, e)
}

/// While an approval waits, the prompt says a message declines it (ChatComposer).
fn placeholder(s: &App) -> &'static str {
    let pending = approvals(s);
    if pending.is_empty() {
        if plan::follow_up(s).is_some() {
            return "Add feedback to refine the plan, or leave this blank to implement it";
        }
        if project_required(s) {
            return "Choose a project above to start a thread";
        }
        if no_provider(s) {
            return "Enable a provider in Settings to send a message";
        }
        PLACEHOLDER
    } else if crate::chat::timeline::requests::takes_feedback(&pending.iter().collect::<Vec<_>>()) {
        "Send feedback to decline this request"
    } else {
        "Send a message to decline and redirect the agent"
    }
}

fn supports_plan(s: &App) -> bool {
    selection(s).and_then(|(i, _)| provider(s, &i).map(|p| p["showInteractionModeToggle"] == true)).unwrap_or(false)
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &T, lane: &Lane) -> El {
    let empty = Draft::default();
    let d = draft(s).unwrap_or(&empty);
    // A pending question takes the editor for its own answer (ChatComposer's pending input).
    let field = if let Some(q) = question::editor(s).filter(|q| q.choice_only) {
        // The web disables the field; with nothing to type into, digits pick the options.
        text(q.placeholder).font(theme::TEXT_SM.lh(22.75)).color(t.c.placeholder).w_full()
    } else if project_required(s) {
        text(placeholder(s)).font(theme::TEXT_SM.lh(22.75)).color(t.c.placeholder).opacity(0.64).w_full().key("composer-editor")
    } else if let Some(q) = question::editor(s) {
        let key = q.key.clone();
        text_input(&q.edit, theme::TEXT_SM.lh(22.75))
            .key("composer-editor")
            .w_full()
            .color(t.c.foreground)
            .placeholder(q.placeholder, t.c.placeholder)
            .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
            .on_edit(move |s: &mut App| question::editor_edit(s, &key))
            .on_submit(|s: &mut App, _| question::advance(s))
            .on_input_key(on_key)
            .on_input_change(|s: &mut App, _| question::on_editor_change(s))
    } else {
        text_input(&d.edit, theme::TEXT_SM.lh(22.75))
            .key("composer-editor")
            .w_full()
            .color(t.c.foreground)
            .placeholder(placeholder(s), t.c.placeholder)
            .pill_style({
                let contexts = d.contexts.clone();
                move |id| chips::pill_style(id, &contexts)
            })
            .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
            .on_edit(|s: &mut App| &mut draft_mut(s).edit)
            .on_submit(|s: &mut App, _| send(s, false))
            .on_input_key(on_key)
            .on_input_paste(on_paste)
            .on_input_change(|s: &mut App, _| {
                s.ui.chat.composer.menu_active = 0;
                s.ui.chat.composer.stash.open = false;
                menu::refresh(s);
            })
    };
    let mut editor = col().w_full().px(16.0).pt(16.0).child(auto_grow(field, 2, 8));
    if let Some(message) = length_error(s) {
        editor = editor.child(text(message).font(theme::TEXT_XS).color(t.c.destructive).wrap().w_full().pt(8.0).role("alert"));
    }

    let mut body = col()
        .key("composer")
        .on_drop_files(|s: &mut App, files: &[std::path::PathBuf], _: &mut Tx| add_files(s, files.iter().map(|p| p.display().to_string())))
        .w_full().radius(radius::XL2).fill(composer_fill(t)).border(1.0, composer_outline(t)).padding(1.0).pb(11.0);
    if !t.dark {
        body = body.shadow(1.0, 2.0, Color::BLACK.alpha(0.05));
    }
    if !d.attachments.is_empty() {
        body = body.child(attachment_chips(t, &d.attachments));
    }
    let body = body.child(editor).child(footer(s, t));

    // The @ menu wins over the stash drawer (isStashMenuOpen && !composerMenuOpen).
    let menu = if question::active(s).is_some() { None } else { menu::view(s, cx, t) };
    let mut stack = col().key("composer-lane").w(lane.width).pin(Some(lane.left), None, None, Some(BOTTOM));
    if let Some(menu) = menu {
        stack = stack.child(menu.mx(22.0).mb(-17.0));
    } else if let Some(stash) = stash::menu(s, t) {
        // Full width, so the inset is the wrapper's padding (margins would shift it).
        stack = stack.child(col().w_full().px(22.0).mb(-17.0).child(stash));
    }
    // A pending question sits directly above the composer, as on the web. The inset is padding
    // on a wrapper: a full-width child with margins would shift rather than shrink.
    let docked = crate::chat::timeline::composer_requests(s, t).or_else(|| plan::banner(s, t)).or_else(|| crate::chat::details::mismatch_banner(s, t));
    // The stash tab rides the composer's right shoulder, beside anything docked there.
    let badge = stash::badge(s, t).filter(|_| !s.ui.chat.composer.stash.open);
    match (docked, badge) {
        (Some(docked), None) => stack = stack.child(col().w_full().px(22.0).mb(-17.0).child(docked)),
        (Some(docked), Some(badge)) => {
            stack = stack.child(row().w_full().px(22.0).mb(-17.0).gap(8.0).items_end().child(col().grow(1.0).shrink().min_w(0.0).child(docked)).child(badge));
        }
        (None, Some(badge)) => stack = stack.child(row().w_full().px(22.0).mb(-17.0).child(spacer()).child(badge)),
        (None, None) => {}
    }
    stack.child(body)
}

/// The composer's own overlays (the model picker), attached after the body so they draw above.
pub fn overlays(s: &App, cx: &mut Cx<App>, t: &T) -> Vec<El> {
    let mut layers = picker::view(s, cx, t);
    if s.ui.chat.composer.permissions_open {
        let menu = picker::runtime_menu(t, &runtime_mode_options(s), &effective_runtime_mode(s), "composer-permissions-menu", "composer-permissions", rune::Align::Start);
        layers.push(menu.modal(true, |s: &mut App| s.ui.chat.composer.permissions_open = false));
    }
    layers.extend(chip_details(s, t));
    layers.extend(context::popover(s, t));
    layers
}

/// A context chip's details while the caret rests against it (a click on a chip lands the caret
/// on its edge), above the composer as web's chip popover opens above the chip.
fn chip_details(s: &App, t: &T) -> Option<El> {
    if s.ui.chat.composer.picker.open || menu::open_menu(s).is_some() {
        return None;
    }
    let d = draft(s)?;
    let record = chip_at_caret(d)?;
    let card = crate::chat::timeline::chip::card(t, record, None);
    Some(card.anchored("composer", rune::Side::Top, rune::Align::Start, 8.0))
}

/// The record of the chip the caret touches, for the kinds whose details have a body.
fn chip_at_caret(d: &Draft) -> Option<&Value> {
    if !d.edit.focused || d.edit.anchor != d.edit.focus {
        return None;
    }
    let caret = d.edit.focus;
    let (_, id) = d.edit.pills().find(|(r, _)| r.start == caret || r.end == caret)?;
    let record = d.contexts.iter().find(|r| chips::pill_id(r) == id)?;
    matches!(data::s(record, "kind"), "terminal" | "element" | "preview-annotation" | "review-comment").then_some(record)
}

fn composer_fill(t: &T) -> Color {
    if t.dark { t.c.surface_raised } else { t.c.card }
}

fn composer_outline(t: &T) -> Color {
    if t.dark { Color::WHITE.alpha(0.05) } else { Color::BLACK.alpha(0.08) }
}

/// The permissions chip shows only while the thread differs from the default (accessDisplay).
fn permissions_chip_shown(s: &App) -> bool {
    let mode = effective_runtime_mode(s);
    !mode.is_empty() && mode != default_runtime_mode(s)
}

/// The model chip's name: the model's short name where it has one.
fn chip_name(s: &App, instance: &str, model: &str) -> String {
    provider(s, instance)
        .and_then(|p| p["models"].as_array()?.iter().find(|m| data::s(m, "slug") == model).map(|m| data::s(m, "shortName").to_owned()))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| model_name(s, instance, model))
}

/// The label of `d`'s chosen option (getProviderOptionCurrentLabel).
fn option_label(s: &App, d: &Value) -> Option<String> {
    let value = option_value(s, d);
    d["options"].as_array()?.iter().find(|o| o["id"] == value).map(|o| data::s(o, "label").to_owned()).filter(|l| !l.is_empty())
}

/// What follows the model name on its chip (describeComposerEffort): the effort word or bars,
/// a bolt for Fast, and the context window.
fn chip_suffix(s: &App, t: &T) -> Vec<El> {
    let mut out = Vec::new();
    let prompt = draft(s).map(|d| d.edit.text.as_str()).unwrap_or("");
    let injected = effort_descriptor(s).is_some_and(|d| d["promptInjectedValues"].as_array().is_some_and(|v| v.iter().any(|x| x == "ultrathink")));
    let word = |label: String| text(label).font(theme::SM_MEDIUM).color(t.c.muted_foreground).truncate().max_w(96.0);
    if injected && mentions_ultrathink(prompt) {
        out.push(word("Ultrathink".to_owned()));
    } else {
        // The level's name, or bars when Appearance's effort display says so; named levels such
        // as Max keep their name.
        let bars_setting = crate::settings::value(s, "effortDisplay") == "bars";
        let effort = effort(s).and_then(|e| {
            let (_, options) = effort_options(s)?;
            let label = options.iter().find(|o| data::s(o, "id") == e).map(|o| data::s(o, "label").to_owned())?;
            Some(match effort_bars(&options, &e).filter(|_| bars_setting) {
                Some((count, filled)) => effort_glyph(count, filled, t.c.muted_foreground),
                None => word(label),
            })
        });
        out.extend(effort);
    }
    let fast = speed_descriptor(s).is_some_and(|d| {
        if data::s(&d, "type") == "boolean" { option_value(s, &d) == true } else { option_label(s, &d).is_some_and(|l| l == "Fast" || l == "Ultrafast") }
    });
    if fast {
        out.push(icon(&icons::ZAP, 14.0).color(t.c.muted_foreground));
    }
    if let Some(window) = descriptors(s).iter().find(|d| data::s(d, "id") == "contextWindow").and_then(|d| option_label(s, d)) {
        out.push(text(window).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().max_w(96.0));
    }
    out
}

/// Without a provider to send to, the model chip's place offers provider setup.
fn provider_unavailable(s: &App, t: &T) -> El {
    let setup = providers(s)
        .iter()
        .any(|p| data::s(p, "driver") == "antigravity" || p["setup"]["canAuthenticate"] == true || p["setup"]["canInstall"] == true);
    let label = if setup { "Open provider settings" } else { "No provider available" };
    let control = control(t, &icons::CIRCLE_ALERT, label, t.c.foreground.alpha_mul(0.8)).key("composer-provider-unavailable").role("button");
    if setup { control.on_press(|s: &mut App| s.navigate(Route::Settings("providers".into()))) } else { control.opacity(0.64) }
}

/// [attach] … [model][permissions][Build/Plan] [meter][send] (ChatComposer's footer).
fn footer(s: &App, t: &T) -> El {
    let attach = icon_button(t, &icons::PAPERCLIP, "Attach files", false, t.c.muted_foreground).key("composer-attach").on_press_tx(|_: &mut App, tx| attach(tx));
    let mut controls = row().gap(2.0).center_y().shrink().min_w(0.0);
    if no_provider(s) {
        controls = controls.child(provider_unavailable(s, t));
    } else if let Some((instance, model)) = selection(s) {
        let driver = provider(s, &instance).map(|p| data::s(p, "driver").to_string()).unwrap_or_default();
        let name = chip_name(s, &instance, &model);
        controls = controls.child(
            row()
                .key("composer-model-chip")
                .h(28.0)
                .px(9.0)
                .gap(6.0)
                .center_y()
                .radius(theme::layout::CONTROL_RADIUS)
                .hover(t.c.accent)
                .cursor(Cursor::Pointer)
                .label(model_name(s, &instance, &model))
                .child(provider_icon(t, &driver, 16.0))
                .child(text(name).font(theme::SM_MEDIUM).color(t.c.foreground.alpha_mul(0.8)).truncate().max_w(224.0))
                .children(chip_suffix(s, t))
                .on_press(|s: &mut App| picker::toggle(s)),
        );
    }
    if permissions_chip_shown(s) {
        let mode = effective_runtime_mode(s);
        let (_, label, description, glyph) = RUNTIME_MODES.iter().find(|m| m.0 == mode).copied().unwrap_or(RUNTIME_MODES[0]);
        controls = controls.child(
            control(t, glyph, label, t.c.foreground.alpha_mul(0.8))
                .pl(6.0)
                .key("composer-permissions")
                .role("button")
                .label(format!("Permissions: {label}"))
                .help(format!("Permissions: {label}. {description}"))
                .on_press(|s: &mut App| s.ui.chat.composer.permissions_open = !s.ui.chat.composer.permissions_open),
        );
    }
    if supports_plan(s) {
        let (glyph, label, help) = if interaction_mode(s) == "plan" {
            (&icons::PENCIL_RULER, "Plan", "Plan mode. Click to return to normal build mode")
        } else {
            (&icons::BOT, "Build", "Default mode. Click to enter plan mode")
        };
        controls = controls.child(control(t, glyph, label, t.c.foreground.alpha_mul(0.8)).key("composer-interaction-mode").help(help).on_press(toggle_plan));
    }
    let right = row().h(28.0).gap(8.0).center_y().children(context::meter(s, t)).child(primary_action(s, t));
    row().w_full().px(8.0).pt(10.0).gap(8.0).center_y().child(attach).child(spacer()).child(controls).child(right)
}

/// resolveEffortGlyph over the selectable levels: `(bars, filled)`, or `None` when the level is
/// named instead (Max, Ultracode) or the scale has fewer than 2 or more than 7 levels.
fn effort_bars(options: &[Value], current: &str) -> Option<(usize, usize)> {
    const NAMED: [&str; 2] = ["ultracode", "max"];
    if NAMED.contains(&current) {
        return None;
    }
    let levels: Vec<&str> = options.iter().map(|o| data::s(o, "id")).filter(|id| !NAMED.contains(id)).collect();
    if !(2..=7).contains(&levels.len()) {
        return None;
    }
    let off = matches!(levels[0], "off" | "none");
    let index = levels.iter().position(|id| *id == current)?;
    Some((levels.len() - usize::from(off), if off { index } else { index + 1 }))
}

/// EffortGlyph: rising 2-point bars on a 4-point step, their base 2 points above a 14-point box.
fn effort_glyph(count: usize, filled: usize, color: Color) -> El {
    let max_bar = 12.0;
    let step = ((max_bar / 2.0 / (count.max(2) - 1) as f32).round()).max(1.0);
    row().h(14.0).pb(2.0).gap(2.0).items_end().a11y_hidden().children((0..count).map(|i| {
        let h = max_bar - (count - 1 - i) as f32 * step;
        el().w(2.0).h(h).fill(if i < filled { color } else { color.alpha_mul(0.4) })
    }))
}

fn primary_action(s: &App, t: &T) -> El {
    if question::active(s).is_some() {
        let stop = running(s).then(|| stop_button(t, 28.0));
        if let Some(actions) = question::actions(s, t, stop) {
            return actions;
        }
    }
    let typed = draft(s).is_some_and(|d| !d.edit.text.trim().is_empty());
    let resume = show_resume(s);
    // With nothing typed, Resume wins over Implement (showPlanFollowUpPrompt && !canResume).
    if typed || !resume {
        if let Some(action) = plan::action(s, t, typed) {
            return action;
        }
    }
    let sendable = !rewinding(s) && draft(s).is_some_and(|d| !d.edit.text.trim().is_empty() || !d.attachments.is_empty());
    if running(s) && !sendable && editing_run(s).is_none() {
        return stop_button(t, 32.0);
    }
    send_button(s, t, sendable, resume)
}

/// The red stop button; 28 points beside a pending question's actions.
fn stop_button(t: &T, size: f32) -> El {
    el()
        .key("composer-stop")
        .size(size, size)
        .center()
        .radius(radius::FULL)
        .fill(t.c.destructive.alpha_mul(0.9))
        .hover(t.c.destructive)
        .cursor(Cursor::Pointer)
        .label("Stop generation")
        .help("Interrupt")
        .child(el().size(8.0, 8.0).radius(1.5).fill(Color::WHITE))
        .on_press(stop)
}

/// The send button (ComposerPrimaryActions): its glyph and tooltip follow what a click does.
fn send_button(s: &App, t: &T, sendable: bool, resume: bool) -> El {
    let editing = editing_run(s).is_some();
    let (mode, alternate) = (mode_for(s, false), mode_for(s, true));
    let busy = send_busy(s);
    let label = match (resume, editing, mode) {
        (true, ..) => "Resume thread",
        (_, true, _) => "Update queued message",
        (.., "queue") => "Queue message",
        (.., "steer") => "Steer message",
        _ => "Submit message",
    };
    let status = if disconnected(s) || no_provider(s) || project_required(s) {
        Some("Environment disconnected".to_owned())
    } else if let Some(reason) = send_block(s) {
        Some(reason)
    } else if rewinding(s) {
        Some("Rewinding conversation".to_owned())
    } else if busy {
        Some(if editing { "Updating queued message" } else { "Submitting message" }.to_owned())
    } else {
        None
    };
    // Mod-click (or the sendAlternate shortcut) sends the other way while a turn runs.
    let help = status.clone().unwrap_or_else(|| {
        if !resume && !editing && mode != alternate {
            let key = if cfg!(target_os = "macos") { "\u{2318}" } else { "Ctrl" };
            let shortcut = crate::shell::palette::shortcut_label(s, "composer.sendAlternate");
            let or = if shortcut.is_empty() { String::new() } else { format!(" or {shortcut}") };
            format!("Click to {mode}, {key}-click{or} to {alternate}")
        } else {
            label.to_owned()
        }
    });
    let enabled = !busy && status.is_none() && (sendable || resume);
    let (bg, fg) = if enabled { (t.c.message_action, t.c.message_action_foreground) } else { (t.c.foreground.alpha_mul(0.12), t.c.muted_foreground) };
    let glyph = if busy {
        crate::kit::Spinner::new().size("sm").build(&crate::kit::theme(t))
    } else if resume {
        icon(&icons::PLAY, 16.0).color(fg)
    } else if editing {
        icon(&icons::CHECK, 16.0).color(fg)
    } else if running(s) {
        icon(if mode == "queue" { &icons::LIST_PLUS } else { &icons::CORNER_UP_RIGHT }, 16.0).color(fg)
    } else {
        icon(&icons::ARROW_UP, 14.0).color(fg)
    };
    let b = el().key("composer-send").size(32.0, 32.0).center().radius(radius::FULL).fill(bg).label(status.unwrap_or_else(|| label.to_owned())).help(help).child(glyph);
    match (enabled, resume) {
        (false, _) => b,
        (true, true) => b.hover(t.c.message_action_hover).cursor(Cursor::Pointer).on_press(self::resume),
        (true, false) => b.hover(t.c.message_action_hover).cursor(Cursor::Pointer).on_press_tx(|s: &mut App, tx| send(s, tx.mods().cmd)),
    }
}

/// A chip per queued file (ComposerAttachmentStrip): its name and size, or in red why it cannot
/// be sent (moved, emptied or grown since it was added).
fn attachment_chips(t: &T, paths: &[String]) -> El {
    let mut chips = row().flex_wrap().gap(6.0).px(12.0).pt(12.0);
    for (i, path) in paths.iter().enumerate() {
        let name = attachments::file_name(path);
        let (detail, ink, help) = match attachments::check(path) {
            Ok(size) => (attachments::format_size(size), t.c.muted_foreground, path.clone()),
            Err(e) => ("Can't send".to_owned(), t.c.destructive, e),
        };
        chips = chips.child(
            row()
                .key(format!("composer-attachment-{i}"))
                .h(28.0)
                .pl(8.0)
                .pr(4.0)
                .gap(6.0)
                .center_y()
                .radius(theme::layout::CONTROL_RADIUS)
                .fill(t.c.foreground.alpha_mul(0.06))
                .help(help)
                .child(icon(&icons::FILE, 14.0).color(ink))
                .child(text(name).font(theme::XS_MEDIUM).color(t.c.foreground.alpha_mul(0.8)).truncate().max_w(180.0))
                .child(text(detail).font(theme::TEXT_XS).color(ink))
                .child(
                    el().size(20.0, 20.0)
                        .center()
                        .radius(radius::SM)
                        .hover(t.c.accent)
                        .cursor(Cursor::Pointer)
                        .label("Remove attachment")
                        .child(icon(&icons::X, 12.0).color(t.c.muted_foreground))
                        .on_press(move |s: &mut App| {
                            let d = draft_mut(s);
                            if i < d.attachments.len() {
                                d.attachments.remove(i);
                            }
                        }),
                ),
        );
    }
    chips
}

/// Any file: a filter list would narrow the macOS picker to its types.
fn attach(tx: &mut Tx) {
    let options = rune::FileDialogOptions { title: "Attach files".into(), ..Default::default() };
    tx.open_file(options, |s: &mut App, picked: rune::FileDialogResult, _| {
        if let Ok(Some(path)) = picked {
            add_files(s, [path.display().to_string()]);
        }
    });
}

/// A square ghost icon button (`Button size="icon-sm" variant="ghost"`): 28 points, 16 icon.
pub fn icon_button(t: &T, i: &'static rune::Icon, label: &str, active: bool, ink: Color) -> El {
    let mut b = el()
        .size(28.0, 28.0)
        .center()
        .radius(theme::layout::CONTROL_RADIUS)
        .hover(t.c.accent)
        .press(t.c.accent)
        .cursor(Cursor::Pointer)
        .label(label.to_string())
        .help(label.to_string())
        .child(icon(i, 16.0).color(ink));
    if active {
        b = b.fill(t.c.accent);
    }
    b
}

/// A composer footer control: icon plus label, 28 tall.
fn control(t: &T, i: &'static rune::Icon, label: &str, color: Color) -> El {
    row()
        .h(28.0)
        .px(8.0)
        .gap(6.0)
        .center_y()
        .radius(theme::layout::CONTROL_RADIUS)
        .hover(t.c.accent)
        .cursor(Cursor::Pointer)
        .child(icon(i, 16.0).color(color))
        .child(text(label.to_string()).font(theme::SM_MEDIUM).color(color))
}

fn srgb(hex: &str) -> Color {
    rune::color::hex(hex).unwrap_or(Color::WHITE)
}

/// A provider's mark (ProviderInstanceIcon): OpenAI and the ACP glyph in the foreground ink,
/// Claude in its orange, OpenCode two-toned; anything else a generic bot.
pub fn provider_icon(t: &T, driver: &str, size: f32) -> El {
    use provider_icons::*;
    let ink = if t.dark { Color::WHITE } else { Color::BLACK };
    match driver {
        "codex" => icon(&OPENAI, size).color(ink),
        "claudeAgent" => icon(&CLAUDE, size).color(srgb("#d97757")),
        "acpRegistry" => icon(&ACP_REGISTRY, size).color(ink),
        "opencode" => {
            let (outer, inner) = if t.dark { ("#F1ECEC", "#4B4646") } else { ("#211E1E", "#CFCECD") };
            el().size(size, size)
                .child(icon(&OPENCODE_INNER, size).color(srgb(inner)).absolute(0.0, 0.0))
                .child(icon(&OPENCODE, size).color(srgb(outer)).absolute(0.0, 0.0))
        }
        _ => provider_mark(t, driver, size).unwrap_or_else(|| icon(&icons::BOT, size).color(t.c.muted_foreground)),
    }
}

/// A provider package's layered mark (cursor, grok, kiro, ...), if the driver has one.
pub fn provider_mark(t: &T, driver: &str, size: f32) -> Option<El> {
    let layers = provider_icons::layers(driver)?;
    Some(layers.iter().fold(el().size(size, size), |mark, &(glyph, light, dark)| {
        mark.child(icon(glyph, size).color(srgb(if t.dark { dark } else { light })).absolute(0.0, 0.0))
    }))
}

/// Gives the open route's composer the caret, or takes it away (the find bar holds it).
pub fn set_focus(s: &mut App, focused: bool) {
    draft_mut(s).edit.focused = focused;
    if let Some(q) = question::editor(s) {
        question::editor_edit(s, &q.key).focused = focused;
    }
}

/// Adds a file to the open route's draft, as a picked element's crop rides beside its chip.
pub fn attach_path(s: &mut App, path: String) {
    draft_mut(s).attachments.push(path);
}

/// Puts `text` back into the open route's draft after what is already there.
/// Sent-form text appended to the draft after `gap` (a rewound prompt, a file's "Add to chat"):
/// file links become mentions and context links chips, with the `records` they were sent with.
pub fn append_with_chips(s: &mut App, text: &str, records: &[Value], gap: &str) {
    append_chips_to(draft_mut(s), text, records, gap);
}

/// Attachments queued in draft `key`.
pub fn draft_attachment_count(s: &App, key: &str) -> usize {
    s.ui.chat.composer.drafts.get(key).map_or(0, |d| d.attachments.len())
}

/// A rewound prompt, its chips and images back into draft `key`; the caret follows only on `focus`.
pub fn restore_rewound(s: &mut App, key: &str, text: &str, records: &[Value], files: Vec<String>, focus: bool) {
    let d = s.ui.chat.composer.drafts.entry(key.to_owned()).or_default();
    if !text.trim().is_empty() {
        append_chips_to(d, text.trim(), records, "\n\n");
    }
    d.attachments.extend(files);
    d.edit.focused = focus;
}

fn append_chips_to(d: &mut Draft, text: &str, records: &[Value], gap: &str) {
    let (shown_text, shown) = chips::display(text);
    d.edit.set_caret(d.edit.text.len());
    if !d.edit.text.trim().is_empty() && !d.edit.text.ends_with(gap) {
        d.edit.insert(gap);
    }
    let mut at = 0;
    for c in &shown {
        d.edit.insert(&shown_text[at..c.range.start]);
        let label = &shown_text[c.range.clone()];
        match records.iter().find(|r| data::s(r, "contextId") == c.id) {
            _ if c.kind == "mention" => d.edit.insert_pill(c.id.clone(), label),
            Some(record) => {
                d.edit.insert_pill(chips::pill_id(record), &chips::sanitize_label(data::s(record, "label"), data::s(record, "kind")));
                d.contexts.push(record.clone());
            }
            None => d.edit.insert(label),
        }
        at = c.range.end;
    }
    d.edit.insert(&shown_text[at..]);
    d.edit.focused = true;
}

pub fn append_to_draft(s: &mut App, text: &str) {
    let d = draft_mut(s);
    let next = if d.edit.text.trim().is_empty() { text.to_owned() } else { format!("{}\n\n{text}", d.edit.text.trim_end()) };
    d.edit.replace_text(&next);
    d.edit.set_caret(next.len());
}

fn set_text(s: &mut App, text: &str) {
    let d = draft_mut(s);
    d.edit.replace_text(text);
    d.edit.set_caret(text.len());
    d.edit.focused = true;
}

/// Changes the open thread's projection in the fixture.
fn patch_projection(s: &mut App, patch: impl FnOnce(&mut Value)) {
    let Route::Thread(id) = s.route.clone() else { return };
    if let data::Source::Fixture(state) = &mut s.src
        && let Some(v) = state.thread_views.get_mut(&id)
    {
        patch(&mut v.projection);
    }
}

/// Changes the fixture's server config.
fn patch_config(s: &mut App, patch: impl FnOnce(&mut Value)) {
    if let data::Source::Fixture(state) = &mut s.src
        && let Some(config) = state.config.as_mut()
    {
        patch(config);
    }
}

fn meter_on(s: &mut App) {
    s.client_settings.values.insert("contextWindowMeterEnabled".into(), json!(true));
}

/// The showcase thread's live turn reporting `used` of a 200k window.
fn context_scene(s: &mut App, used: f64) {
    on_replay_showcase(s);
    meter_on(s);
    let Route::Thread(id) = s.route.clone() else { return };
    if let data::Source::Fixture(state) = &mut s.src
        && let Some(v) = state.thread_views.get_mut(&id)
    {
        v.projection["providerTurns"] = json!([{"tokenUsage": {"usedTokens": used, "maxTokens": 200_000, "updatedAt": "2026-12-01T00:00:00.000Z"}}]);
    }
}

/// The showcase thread with three stashed prompts of different ages.
fn stash_scene(s: &mut App) {
    on_replay_showcase(s);
    let now = s.now_ms;
    s.ui.chat.composer.stash.entries = vec![
        json!({"id": "stash-1", "createdAt": now - 4 * 60_000, "draft": {"prompt": "Refactor the websocket retry loop to back off exponentially and log each attempt"}}),
        json!({"id": "stash-2", "createdAt": now - 3 * 3_600_000, "draft": {"prompt": "Check these traces", "attachments": ["/tmp/trace-1.json", "/tmp/trace-2.json"]}}),
        json!({"id": "stash-3", "createdAt": now - 2 * 86_400_000, "draft": {"prompt": "", "attachments": ["/tmp/screenshot.png"]}}),
    ];
}

fn on_replay_showcase(s: &mut App) {
    crate::scene_clock(s);
    if let Some(id) = s.state().threads.iter().find(|t| t["title"] == "Replay showcase").map(|t| data::s(t, "id").to_string()) {
        s.navigate(Route::Thread(id));
    }
}

/// The references were captured when `server` was the only project, so the draft opens there.
fn on_home(s: &mut App) {
    crate::scene_clock(s);
    let server = s.state().projects.iter().find(|p| p["title"] == "server").map(|p| data::s(p, "id").to_string());
    s.navigate(Route::Home { project_id: server });
}

/// Marks the open thread as running, as the server reports a run in flight.
fn seed_running(s: &mut App) {
    let id = draft_key(s, s.frame_route());
    if let crate::data::Source::Fixture(state) = &mut s.src
        && let Some(t) = state.threads.iter_mut().find(|t| data::s(t, "id") == id)
    {
        t["activeRunId"] = json!("run:scene");
        t["status"] = json!("running");
    }
}

/// Scenes for the composer, topbar, home and details card.
pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("home-empty", on_home),
    ("home-typed", |s| {
        on_home(s);
        set_text(s, "Summarize the server's orchestration layer");
    }),
    ("composer-idle", on_replay_showcase),
    ("composer-typed", |s| {
        on_replay_showcase(s);
        set_text(s, "hi");
    }),
    ("composer-working", |s| {
        on_replay_showcase(s);
        seed_running(s);
    }),
    ("composer-model", |s| {
        on_replay_showcase(s);
        picker::toggle(s);
    }),
    ("composer-mention", |s| {
        on_replay_showcase(s);
        set_text(s, "@");
        menu::refresh(s);
    }),
    ("composer-slash", |s| {
        on_replay_showcase(s);
        set_text(s, "/");
        menu::refresh(s);
    }),
    ("composer-contexts", |s| {
        on_replay_showcase(s);
        set_text(s, "Why does ");
        for record in chips::samples() {
            add_context(s, record);
        }
    }),
    ("composer-chip-details", |s| {
        on_replay_showcase(s);
        set_text(s, "Why does ");
        for record in chips::samples() {
            add_context(s, record);
        }
        let d = draft_mut(s);
        let end = d.edit.pills().map(|(r, _)| r.end).next().unwrap_or(0);
        d.edit.set_caret(end);
    }),
    ("composer-pr", |s| {
        on_replay_showcase(s);
        set_text(s, "Review #");
        menu::refresh(s);
    }),
    ("composer-effort", |s| {
        on_replay_showcase(s);
        let d = draft_mut(s);
        d.model = Some(("claudeAgent".into(), "claude-opus-5-5".into()));
        d.effort = Some("high".into());
        picker::toggle(s);
    }),
    ("composer-effort-bars", |s| {
        on_replay_showcase(s);
        s.client_settings.values.insert("effortDisplay".into(), json!("bars"));
        let d = draft_mut(s);
        d.model = Some(("claudeAgent".into(), "claude-opus-5-5".into()));
        d.effort = Some("medium".into());
    }),
    ("composer-context", |s| context_scene(s, 42_400.0)),
    ("composer-context-full", |s| context_scene(s, 186_000.0)),
    ("composer-context-popover", |s| {
        on_replay_showcase(s);
        meter_on(s);
        let Route::Thread(id) = s.route.clone() else { return };
        if let data::Source::Fixture(state) = &mut s.src
            && let Some(v) = state.thread_views.get_mut(&id)
        {
            v.projection["thread"]["activeProviderThreadId"] = json!("scene-provider-thread");
            v.projection["providerThreads"] = json!([{"id": "scene-provider-thread", "contextUsage": {
                "usedTokens": 128_400, "maxTokens": 200_000, "totalProcessedTokens": 1_240_000,
                "compactsAutomatically": true, "cost": {"amount": 1.284, "currency": "USD"}}}]);
        }
        s.ui.chat.composer.context.trigger = true;
    }),
    ("composer-attachments", |s| {
        on_replay_showcase(s);
        draft_mut(s).attachments = vec!["/home/ubuntu/s5code/README.md".into(), "/home/ubuntu/s5code/package.json".into()];
    }),
    ("composer-file-chip", |s| {
        on_replay_showcase(s);
        let dir = std::env::temp_dir().join("s5-scene-attachments");
        let pdf = dir.join("design-review.pdf");
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::write(&pdf, vec![b'%'; 240 * 1024]);
        }
        set_text(s, "Summarize the review notes");
        draft_mut(s).attachments = vec![pdf.display().to_string(), "/nonexistent/moved-away.csv".into()];
    }),
    ("composer-stash-badge", |s| {
        stash_scene(s);
        set_text(s, "Now look at the reconnect path");
    }),
    ("composer-stash-menu", |s| {
        stash_scene(s);
        s.ui.chat.composer.stash.open = true;
    }),
    ("composer-model-search", |s| {
        on_replay_showcase(s);
        picker::toggle(s);
        let query = &mut s.ui.chat.composer.picker.query;
        query.replace_text("5");
        query.set_caret(1);
    }),
    ("composer-permissions", |s| {
        on_replay_showcase(s);
        draft_mut(s).runtime_mode = Some("approval-required".into());
    }),
    ("composer-permissions-menu", |s| {
        on_replay_showcase(s);
        draft_mut(s).runtime_mode = Some("approval-required".into());
        s.ui.chat.composer.permissions_open = true;
    }),
    ("composer-mode-shortcut", |s| {
        on_replay_showcase(s);
        picker::open_permissions(s);
    }),
    ("composer-permissions-limited", |s| {
        on_replay_showcase(s);
        patch_config(s, |c| c["managedPolicy"] = json!({"organization": "Example Corp", "lockedSettings": [], "limits": {"maxRuntimeMode": "auto-accept-edits"}}));
        draft_mut(s).runtime_mode = Some("approval-required".into());
        s.ui.chat.composer.permissions_open = true;
    }),
    ("composer-resume", |s| {
        on_replay_showcase(s);
        patch_projection(s, |p| {
            if let Some(run) = p["runs"].as_array_mut().and_then(|runs| runs.last_mut()) {
                run["status"] = json!("interrupted");
            }
        });
    }),
    ("composer-steer", |s| {
        on_replay_showcase(s);
        seed_running(s);
        set_text(s, "Also cover the reconnect path");
    }),
    ("composer-queue", |s| {
        on_replay_showcase(s);
        seed_running(s);
        s.client_settings.values.insert("followUpBehavior".into(), json!("queue"));
        set_text(s, "Then update the docs");
    }),
    ("composer-prompt-too-long", |s| {
        on_replay_showcase(s);
        let line = format!("{}\n", "lorem ipsum ".repeat(9));
        set_text(s, &line.repeat(120_050 / line.len() + 1));
        s.ui.chat.composer.length_error = Some(active_key(s));
    }),
    ("composer-no-provider", |s| {
        on_replay_showcase(s);
        patch_config(s, |c| c["providers"] = json!([]));
    }),
    ("composer-locked-provider", |s| {
        on_replay_showcase(s);
        patch_projection(s, |p| {
            p["providerThreads"] = json!([]);
            p["providerSessions"] = json!([{"id": "scene-session", "status": "ready", "capabilities": {"sessions": {"supportsProviderSwitchingViaHandoff": false}}}]);
        });
        picker::toggle(s);
    }),
    ("composer-favorites", |s| {
        on_replay_showcase(s);
        s.client_settings.values.insert("favorites".into(), json!([{"provider": "opencode", "model": "opencode/big-pickle"}, {"provider": "claudeAgent", "model": "claude-opus-5-5"}]));
        picker::toggle(s);
        s.ui.chat.composer.picker.rail = Some("favorites".into());
        s.ui.chat.composer.picker.hovered = Some(0);
    }),
    ("composer-model-options", |s| {
        on_replay_showcase(s);
        draft_mut(s).model = Some(("opencode".into(), "opencode/beam-free".into()));
        picker::toggle(s);
    }),
    ("composer-fast", |s| {
        on_replay_showcase(s);
        draft_mut(s).model = Some(("claudeAgent".into(), "claude-opus-5-5".into()));
        set_option(s, "fastMode", json!(true));
        set_text(s, "ultrathink about the retry loop");
        picker::toggle(s);
    }),
    ("topbar-rename", |s| {
        on_replay_showcase(s);
        crate::chat::topbar::start_rename(s);
    }),
    ("details-card", on_replay_showcase),
    ("details-hidden", |s| {
        on_replay_showcase(s);
        crate::chat::details::toggle(s, true);
    }),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removing_a_context_takes_its_pill_and_trailing_space() {
        let mut s = App::new();
        let record = chips::terminal("term-1", "Terminal 1", 1, 2, "ls");
        let id = data::s(&record, "contextId").to_owned();
        add_thread_context(&mut s, "t1", record);
        assert_eq!(thread_contexts(&s, "t1").len(), 1);
        remove_context(&mut s, "t1", &id);
        let d = &s.ui.chat.composer.drafts["t1"];
        assert_eq!(d.edit.text, "");
        assert!(d.contexts.is_empty());
    }

    #[test]
    fn a_picked_effort_rides_in_the_model_selection() {
        let mut s = App::new();
        s.navigate(Route::Home { project_id: None });
        draft_mut(&mut s).model = Some(("claudeAgent".into(), "claude-opus-5-5".into()));
        assert_eq!(effort(&s).as_deref(), Some("medium"));
        let ids: Vec<String> = effort_options(&s).unwrap().1.iter().map(|o| data::s(o, "id").to_owned()).collect();
        assert!(!ids.contains(&"ultrathink".to_owned()) && ids.contains(&"max".to_owned()));
        assert_eq!(model_selection(&s).unwrap(), json!({"instanceId": "claudeAgent", "model": "claude-opus-5-5"}));
        draft_mut(&mut s).effort = Some("high".into());
        assert_eq!(effort(&s).as_deref(), Some("high"));
        assert_eq!(model_selection(&s).unwrap()["options"], json!([{"id": "effort", "value": "high"}]));
        set_option(&mut s, "fastMode", json!(true));
        set_option(&mut s, "notAnOption", json!(1));
        assert_eq!(model_selection(&s).unwrap()["options"], json!([{"id": "fastMode", "value": true}, {"id": "effort", "value": "high"}]));
        let ids = ["low", "medium", "high"].map(String::from);
        assert_eq!(stepped(&ids, Some("medium"), 1).as_deref(), Some("high"));
        assert_eq!(stepped(&ids, Some("high"), 1), None);
        assert_eq!(stepped(&ids, Some("low"), -1), None);
        assert_eq!(stepped(&ids, None, 1), None);
        assert!(mentions_ultrathink("Ultrathink: fix it") && mentions_ultrathink("please ultrathink."));
        assert!(!mentions_ultrathink("ultrathinking") && !mentions_ultrathink("no_ultrathink"));
    }

    #[test]
    fn runtime_modes_follow_the_provider_and_the_policy_ceiling() {
        let modes = |o: &[ModeOption]| o.iter().map(|o| o.mode).collect::<Vec<_>>();
        let all = mode_options(&[], &Value::Null);
        assert_eq!(modes(&all), ["approval-required", "auto-accept-edits", "auto", "full-access"]);
        assert!(all.iter().all(|o| o.disabled.is_none()));
        assert_eq!(modes(&mode_options(&["approval-required", "full-access"], &Value::Null)), ["approval-required", "full-access"]);
        let policy = json!({"organization": "Example Corp", "limits": {"maxRuntimeMode": "auto-accept-edits"}});
        let reason = "Example Corp limits threads to Auto-accept edits.";
        let limited = mode_options(&[], &policy);
        assert_eq!(limited.iter().map(|o| o.disabled.as_deref()).collect::<Vec<_>>(), [None, None, Some(reason), Some(reason)]);
    }

    #[test]
    fn an_interrupted_run_or_a_held_queue_can_resume() {
        let runs = |runs: Value| json!({"runs": runs});
        let done = json!({"id": "r1", "status": "completed", "startedAt": "2026-01-01T00:00:00.000Z", "completedAt": "2026-01-01T00:01:00.000Z", "ordinal": 1});
        let interrupted = json!({"id": "r2", "status": "interrupted", "startedAt": "2026-01-01T00:02:00.000Z", "completedAt": "2026-01-01T00:03:00.000Z", "ordinal": 2});
        let held = json!({"id": "r3", "status": "queued", "ordinal": 3, "queueHeld": true});
        let later = json!({"id": "r4", "status": "completed", "startedAt": "2026-01-01T00:04:00.000Z", "completedAt": "2026-01-01T00:05:00.000Z", "ordinal": 4});
        let p = runs(json!([done.clone(), interrupted.clone(), held]));
        assert_eq!(resumable_run(&p).as_deref(), Some("r2"));
        assert!(held_queue(&p));
        let p = runs(json!([done, interrupted, later]));
        assert_eq!(resumable_run(&p), None);
        assert!(!held_queue(&p));
    }

    #[test]
    fn long_prompts_and_standalone_mode_commands_are_caught_before_sending() {
        assert_eq!(prompt_length_message(&format!("  {}  ", "a".repeat(120_000))), None);
        assert_eq!(
            prompt_length_message(&"a".repeat(120_001)).as_deref(),
            Some("Prompt is 1 character over the 120,000-character limit. Shorten or split it before sending.")
        );
        assert_eq!(
            prompt_length_message(&"\u{1F600}".repeat(61_000)).as_deref(),
            Some("Prompt is 2,000 characters over the 120,000-character limit. Shorten or split it before sending.")
        );
        assert_eq!(standalone_mode(" /PLAN "), Some("plan"));
        assert_eq!(standalone_mode("/default"), Some("default"));
        assert_eq!(standalone_mode("/plan the migration"), None);
    }

    #[test]
    fn a_changed_queued_edit_moves_into_an_empty_thread_draft_when_left() {
        let mut s = App::new();
        let id = data::s(&s.state().threads[0], "id").to_string();
        s.navigate(Route::Thread(id.clone()));
        edit_queued(&mut s, &id, "run-1", "original");
        cancel_queued_edit(&mut s);
        assert!(s.ui.chat.composer.editing.is_none());
        assert!(s.ui.chat.composer.drafts.get(&id).is_none_or(|d| d.edit.text.is_empty()));
        edit_queued(&mut s, &id, "run-1", "original");
        set_text(&mut s, "rewritten");
        cancel_queued_edit(&mut s);
        assert_eq!(s.ui.chat.composer.drafts[&id].edit.text, "rewritten");
        edit_queued(&mut s, &id, "run-1", "original");
        set_text(&mut s, "again");
        cancel_queued_edit(&mut s);
        assert_eq!(s.ui.chat.composer.drafts[&id].edit.text, "rewritten");
        assert!(!s.ui.chat.composer.drafts.keys().any(|k| k.starts_with("queued-edit:")));
    }

    #[test]
    fn a_rewound_prompt_returns_with_its_chips() {
        let mut s = App::new();
        s.navigate(Route::Home { project_id: None });
        set_text(&mut s, "draft");
        let pr = chips::samples().into_iter().find(|r| data::s(r, "kind") == "review-comment").unwrap();
        let sent = format!("open [a.rs](src/a.rs) and [{}](t3-context://v1/review-comment/{})", data::s(&pr, "label"), data::s(&pr, "contextId"));
        append_with_chips(&mut s, &sent, std::slice::from_ref(&pr), "\n\n");
        let d = draft(&s).unwrap();
        assert_eq!(d.edit.pills().count(), 2);
        let (text, contexts) = chips::outgoing(&d.edit, &d.contexts);
        assert_eq!(text, format!("draft\n\n{sent}"));
        assert_eq!(contexts.len(), 1);
    }

    #[test]
    fn effort_levels_map_to_bars_like_the_web_glyph() {
        let levels = |ids: &[&str]| ids.iter().map(|id| json!({"id": id, "label": id})).collect::<Vec<_>>();
        let codex = levels(&["low", "medium", "high", "xhigh"]);
        assert_eq!(effort_bars(&codex, "high"), Some((4, 3)));
        let claude = levels(&["low", "medium", "high", "max", "ultracode"]);
        assert_eq!(effort_bars(&claude, "low"), Some((3, 1)));
        assert_eq!(effort_bars(&claude, "max"), None);
        let off = levels(&["none", "low", "high"]);
        assert_eq!(effort_bars(&off, "none"), Some((2, 0)));
        assert_eq!(effort_bars(&off, "high"), Some((2, 2)));
        assert_eq!(effort_bars(&levels(&["on"]), "on"), None);
    }

    #[test]
    fn navigating_focuses_the_route_draft() {
        let mut s = App::new();
        let id = data::s(&s.state().threads[0], "id").to_string();
        s.navigate(Route::Thread(id.clone()));
        assert!(s.ui.chat.composer.drafts[&id].edit.focused);
        s.navigate(Route::Home { project_id: None });
        let key = draft_key(&s, &Route::Home { project_id: None });
        assert!(key.starts_with("home:") && s.ui.chat.composer.drafts[&key].edit.focused);
    }

    #[test]
    fn each_project_keeps_its_own_new_thread_draft() {
        let mut s = App::new();
        let projects = s.state().projects.iter().map(|p| data::s(p, "id").to_owned()).take(2).collect::<Vec<_>>();
        let [a, b] = &projects[..] else { panic!("two sample projects") };
        s.navigate(Route::Home { project_id: Some(a.clone()) });
        set_text(&mut s, "for a");
        s.navigate(Route::Home { project_id: Some(b.clone()) });
        assert!(draft(&s).is_none_or(|d| d.edit.text.is_empty()));
        set_text(&mut s, "for b");
        s.navigate(Route::Home { project_id: Some(a.clone()) });
        assert_eq!(draft(&s).unwrap().edit.text, "for a");

        let legacy = || HashMap::from([("home".to_owned(), Draft { edit: TextEdit::new("old"), ..Draft::default() })]);
        let key = home_key(a);
        assert_eq!(migrated(legacy(), Some(key.as_str()))[&key].edit.text, "old");
        assert!(migrated(legacy(), None).is_empty());
    }

    #[test]
    fn sending_resets_the_home_draft_but_keeps_thread_overrides() {
        let mut s = App::new();
        s.navigate(Route::Home { project_id: None });
        let d = draft_mut(&mut s);
        d.edit = TextEdit::new("hi");
        d.model = Some(("replay".into(), "write-approval".into()));
        d.runtime_mode = Some("approval-required".into());
        sent(&mut s);
        let d = draft(&s).unwrap();
        assert!(d.edit.text.is_empty() && d.model.is_none() && d.runtime_mode.is_none());

        let id = data::s(&s.state().threads[0], "id").to_string();
        s.navigate(Route::Thread(id));
        let d = draft_mut(&mut s);
        d.edit = TextEdit::new("hi");
        d.runtime_mode = Some("approval-required".into());
        sent(&mut s);
        let d = draft(&s).unwrap();
        assert!(d.edit.text.is_empty());
        assert_eq!(d.runtime_mode.as_deref(), Some("approval-required"));
    }

    #[test]
    fn drafts_round_trip_through_storage_with_their_chips() {
        let record = chips::terminal("t", "Terminal 1", 2, 2, "ls");
        let mut d = Draft::default();
        d.edit.insert("see ");
        d.edit.insert_pill(chips::pill_id(&record), "Terminal 1 line 2");
        d.edit.insert(" and @a");
        d.contexts = vec![record.clone(), chips::terminal("t", "Terminal 1", 5, 5, "deleted chip")];
        d.runtime_mode = Some("approval-required".into());
        d.options = vec![json!({"id": "fastMode", "value": true})];
        let mut drafts = HashMap::new();
        drafts.insert("thread-1".to_owned(), d);
        drafts.insert("home".to_owned(), Draft::default());
        drafts.insert("queued-edit:thread-1:run".to_owned(), Draft { edit: TextEdit::new("edit"), ..Draft::default() });
        let back = restored(&stored(&drafts));
        assert_eq!(back.keys().collect::<Vec<_>>(), [&"thread-1".to_owned()]);
        let d = &back["thread-1"];
        assert_eq!(d.edit.text, "see Terminal 1 line 2 and @a");
        assert_eq!(d.edit.pills().map(|(r, id)| (r, id.to_owned())).collect::<Vec<_>>(), [(4..21, chips::pill_id(&record))]);
        assert_eq!(d.contexts, [record]);
        assert_eq!(d.runtime_mode.as_deref(), Some("approval-required"));
        assert_eq!(d.options, [json!({"id": "fastMode", "value": true})]);
    }

    #[test]
    fn send_chords_resolve_by_route_run_and_focus_like_the_web() {
        let mut s = App::new();
        let id = |n: &str| json!({"type": "identifier", "name": n});
        let and = |l: Value, r: Value| json!({"type": "and", "left": l, "right": r});
        let enter = |alt: bool| json!({"key": "enter", "modKey": true, "altKey": alt});
        if let data::Source::Fixture(state) = &mut s.src {
            state.config.get_or_insert_with(|| json!({}))["keybindings"] = json!([
                {"command": "composer.sendAlternate", "shortcut": enter(false), "whenAst": and(id("composerFocus"), id("turnRunning"))},
                {"command": "composer.sendBackground", "shortcut": enter(false), "whenAst": and(id("composerFocus"), id("draftThreadRoute"))},
                {"command": "composer.sendBackground", "shortcut": enter(true), "whenAst": and(id("composerFocus"), id("draftThreadRoute"))},
                {"command": "composer.sendAndNewThread", "shortcut": enter(true), "whenAst": and(id("composerFocus"), json!({"type": "not", "node": id("draftThreadRoute")}))},
            ]);
        }
        let resolved = |s: &App, alt: bool| {
            let mut e = KeyEvent::from(Key::Enter);
            e.mods.cmd = true;
            e.mods.ctrl = !cfg!(target_os = "macos");
            e.mods.alt = alt;
            SEND_COMMANDS.into_iter().find(|c| crate::panel::shortcut(s, &e, c))
        };
        let project = s.state().projects.iter().map(|p| data::s(p, "id").to_owned()).next().unwrap();
        s.navigate(Route::Home { project_id: Some(project) });
        draft_mut(&mut s).edit.focused = true;
        assert_eq!(resolved(&s, false), Some("composer.sendBackground"), "FAILED: Mod+Enter on a draft starts it in the background");
        assert_eq!(resolved(&s, true), Some("composer.sendBackground"));
        on_replay_showcase(&mut s);
        draft_mut(&mut s).edit.focused = true;
        assert_eq!(resolved(&s, false), None, "FAILED: an idle thread sends Mod+Enter in the foreground");
        assert_eq!(resolved(&s, true), Some("composer.sendAndNewThread"));
        seed_running(&mut s);
        assert_eq!(resolved(&s, false), Some("composer.sendAlternate"), "FAILED: a running turn flips steer and queue");
        draft_mut(&mut s).edit.focused = false;
        assert_eq!(resolved(&s, false), None, "FAILED: the prompt must have the keys");
    }

    #[test]
    fn follow_ups_steer_or_queue_and_the_alternate_key_flips_them() {
        assert_eq!(dispatch_mode(false, true, "queue", true), "auto");
        assert_eq!(dispatch_mode(true, true, "steer", false), "steer");
        assert_eq!(dispatch_mode(true, true, "steer", true), "queue");
        assert_eq!(dispatch_mode(true, true, "queue", false), "queue");
        assert_eq!(dispatch_mode(true, true, "queue", true), "steer");
        assert_eq!(dispatch_mode(true, false, "steer", true), "queue");
    }

    #[test]
    fn up_recalls_typed_prompts_newest_first_and_down_returns_to_empty() {
        let messages = json!([
            {"id": "1", "role": "user", "text": "first"},
            {"id": "2", "role": "assistant", "text": "reply"},
            {"id": "3", "role": "user", "text": "Ultrathink:\nsecond"},
            {"id": "4", "role": "user", "text": "second"},
            {"id": "5", "role": "user", "text": "from a task", "scheduledTaskId": "task"},
            {"id": "6", "role": "user", "text": "PLEASE IMPLEMENT THIS PLAN:\n# Plan"},
            {"id": "7", "role": "user", "text": "agent", "createdBy": "agent"},
        ]);
        let entries = history_entries(messages.as_array().unwrap());
        assert_eq!(entries, [("1", "first".to_owned()), ("4", "second".to_owned())]);
        assert_eq!(step_history(&entries, None, "typed", true), None);
        let up = step_history(&entries, None, "", true).unwrap();
        assert_eq!(up, (Some("4".to_owned()), "second".to_owned()));
        let up = step_history(&entries, Some(("4", "second")), "second", true).unwrap();
        assert_eq!(up, (Some("1".to_owned()), "first".to_owned()));
        assert_eq!(step_history(&entries, Some(("1", "first")), "first", true), None);
        assert_eq!(step_history(&entries, Some(("1", "first")), "first", false).unwrap().1, "second");
        assert_eq!(step_history(&entries, Some(("4", "second")), "second", false), Some((None, String::new())));
        // An edited recall is the user's own text again.
        assert_eq!(step_history(&entries, Some(("4", "second")), "second!", true), None);
    }

    #[test]
    fn a_launch_that_fails_out_of_sight_toasts_like_the_web() {
        let mut s = App::new();
        let project = s.state().projects.iter().map(|p| data::s(p, "id").to_owned()).next().unwrap();
        let thread = s.state().threads.iter().map(|t| data::s(t, "id").to_owned()).next().unwrap();
        let draft = Outgoing { key: home_key(&project), draft: json!({"prompt": "ship it"}) };
        let error = json!({"message": "Provider is not ready"});
        let toast = |s: &App| crate::shell::palette::toast_title(s).map(str::to_owned);
        s.navigate(Route::Home { project_id: Some(project.clone()) });

        send_failed(&mut s, Outgoing { key: draft.key.clone(), draft: draft.draft.clone() }, &error, Some(false));
        assert_eq!(toast(&s), None, "FAILED: a failed launch on its own draft shows only the banner");
        send_failed(&mut s, Outgoing { key: thread.clone(), draft: Value::Null }, &error, None);
        assert_eq!(toast(&s), None, "FAILED: a failed message to a thread shows only the banner");

        send_failed(&mut s, Outgoing { key: draft.key.clone(), draft: draft.draft.clone() }, &error, Some(true));
        assert_eq!(toast(&s).as_deref(), Some("Background task failed"), "FAILED: a background launch failure toasts");

        s.navigate(Route::Thread(thread.clone()));
        send_failed(&mut s, draft, &error, Some(false));
        assert_eq!(toast(&s).as_deref(), Some("Could not start thread"), "FAILED: a launch failure after leaving its draft toasts");
        assert_eq!(s.ui.chat.composer.drafts.get(&home_key(&project)).map(|d| d.edit.text.as_str()), Some("ship it"), "FAILED: the draft comes back to open");
    }
}
