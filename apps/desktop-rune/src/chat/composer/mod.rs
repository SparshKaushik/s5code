//! The composer (apps/web ChatComposer, ComposerSurface, ComposerPrimaryActions): a glass card
//! floating over the bottom of the chat lane with the editor, attach, the model chip and send.
//! Drafts are kept per thread (and one for the home draft), as the web composer draft store does.
//! `menu` is the @-mention and slash drawer, `picker` the model picker popover.

pub mod chips;
mod context;
mod menu;
mod picker;
pub mod provider_icons;

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
}

impl Default for Draft {
    fn default() -> Draft {
        Draft { edit: TextEdit::new(""), attachments: Vec::new(), model: None, runtime_mode: None, interaction_mode: None, effort: None, contexts: Vec::new() }
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
    mention: menu::Mention,
    launches: Vec<(u64, String)>,
    /// Messages waiting on `assets.persistChatAttachments`: the request, the command or launch
    /// payload, the thread a launch starts, and the send it belongs to.
    uploads: Vec<(u64, Value, Option<String>, Outgoing)>,
    /// Commands whose failure shows in the route's error banner (setThreadError).
    sends: Vec<(u64, Outgoing)>,
    pub context: context::Hover,
    /// The queued run being rewritten in the composer: thread and run.
    editing: Option<(String, String)>,
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
    let d = draft_mut(s);
    d.edit = TextEdit::new(text);
    d.edit.set_caret(text.len());
    d.edit.focused = true;
}

pub fn cancel_queued_edit(s: &mut App) {
    if editing_run(s).is_some() {
        let key = active_key(s);
        s.ui.chat.composer.drafts.remove(&key);
        s.ui.chat.composer.editing = None;
        draft_mut(s).edit.focused = true;
    }
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
    if editing_run(s).is_some() || has_attachments(s) {
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

/// The selected model's effort descriptor (`effort` or `reasoningEffort`) and its options,
/// without the values a prompt keyword turns on (ultrathink).
pub fn effort_options(s: &App) -> Option<(String, Vec<Value>)> {
    let (instance, slug) = selection(s)?;
    let model = provider(s, &instance)?["models"].as_array()?.iter().find(|x| data::s(x, "slug") == slug)?.clone();
    let descriptors = model["capabilities"]["optionDescriptors"].as_array()?;
    let d = descriptors.iter().find(|d| matches!(data::s(d, "id"), "effort" | "reasoningEffort"))?;
    let injected: Vec<&str> = d["promptInjectedValues"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let options = d["options"].as_array()?.iter().filter(|o| !injected.contains(&data::s(o, "id"))).cloned().collect();
    Some((data::s(d, "id").to_owned(), options))
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
    let thread = source_options(s).into_iter().find(|o| data::s(o, "id") == id).and_then(|o| o["value"].as_str().map(str::to_owned));
    thread.or_else(|| options.iter().find(|o| o["isDefault"] == true).map(|o| data::s(o, "id").to_owned()))
}

/// The modelSelection to send: the model plus its options, a picked effort replacing the thread's.
fn model_selection(s: &App) -> Option<Value> {
    let (instance, model) = selection(s)?;
    let mut options = source_options(s);
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
    s.state().config.as_ref().map(|c| data::s(&c["settings"], "defaultRuntimeMode").to_string()).filter(|m| !m.is_empty()).unwrap_or_else(|| "full-access".into())
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

fn send(s: &mut App, alternate: bool) {
    if rewinding(s) {
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
        cancel_queued_edit(s);
        return;
    }
    let mode = mode_for(s, alternate);
    let key = active_key(s);
    let Some(d) = draft(s) else { return };
    let snapshot = stored_draft(d);
    let (text, contexts) = chips::outgoing(&d.edit, &d.contexts);
    let mut text = text.trim().to_string();
    // A file that can no longer be read keeps the draft as it is.
    let Some(uploads) = d.attachments.iter().filter(|p| chips::keeps_attachment(p, &contexts)).map(|p| upload(p)).collect::<Option<Vec<_>>>() else { return };
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
    let (runtime, interaction) = (runtime_mode(s), interaction_mode(s));
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
            let thread_id = format!("thread:project:{project_id}:{}", s5_net::uuid());
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
    crate::chat::timeline::error_banner::set_local(s, &key, None);
    let outgoing = Outgoing { key, draft: snapshot };
    let Some(net) = s.src.net() else { return };
    let composer = &mut s.ui.chat.composer;
    if uploads.is_empty() {
        deliver(net, composer, payload, launch, json!([]), outgoing);
    } else {
        let request = net.request(
            "assets.persistChatAttachments",
            json!({"threadId": thread_id, "messageId": message_id, "attachments": uploads}),
        );
        composer.uploads.push((request, payload, launch, outgoing));
    }
    sent(s);
}

/// Image types the server stores (PROVIDER_SEND_TURN_SUPPORTED_IMAGE_MIME_TYPES), by extension.
const IMAGE_TYPES: [(&str, &str); 5] =
    [("png", "image/png"), ("jpg", "image/jpeg"), ("jpeg", "image/jpeg"), ("gif", "image/gif"), ("webp", "image/webp")];
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

/// Pasted images join the draft's attachments (ChatComposer onPaste): copied image files as they
/// are, raw image data as a temporary PNG. Text, and a queued-message edit, paste as usual.
fn on_paste(s: &mut App, content: &rune::ClipboardContent, _: &mut Tx) -> bool {
    if editing_run(s).is_some() {
        return false;
    }
    let image_ext = |p: &std::path::Path| p.extension().and_then(|e| e.to_str()).is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()));
    let files: Vec<String> = content.files().iter().filter(|p| image_ext(p)).map(|p| p.display().to_string()).collect();
    if !files.is_empty() {
        draft_mut(s).attachments.extend(files);
        return true;
    }
    let Some(image) = content.image().filter(|_| content.text().trim().is_empty()) else { return false };
    let path = std::env::temp_dir().join(format!("pasted-image-{}.png", s5_net::uuid()));
    if std::fs::write(&path, image.png()).is_err() {
        return false;
    }
    draft_mut(s).attachments.push(path.display().to_string());
    true
}

/// An image file as `assets.persistChatAttachments` takes it, inlined as a data URL.
fn upload(path: &str) -> Option<Value> {
    use base64::Engine;
    let path = std::path::Path::new(path);
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let mime = IMAGE_TYPES.iter().find(|(e, _)| *e == ext)?.1;
    let bytes = std::fs::read(path).ok().filter(|b| b.len() <= MAX_IMAGE_BYTES)?;
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(json!({
        "type": "image", "name": path.file_name()?.to_str()?, "mimeType": mime,
        "sizeBytes": bytes.len(), "dataUrl": format!("data:{mime};base64,{data}"),
    }))
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
    let mut changed = persist(s);
    let Some(net) = s.src.net() else { return false };
    let composer = &mut s.ui.chat.composer;
    let mut failed = Vec::new();
    let mut i = 0;
    while i < composer.uploads.len() {
        let Some(reply) = net.take_reply(composer.uploads[i].0) else {
            i += 1;
            continue;
        };
        let (_, payload, launch, outgoing) = composer.uploads.remove(i);
        match reply {
            Ok(v) => deliver(net, composer, payload, launch, v["attachments"].clone(), outgoing),
            Err(e) => failed.push((outgoing, e)),
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
        match (reply, launch) {
            (Ok(v), Some(thread_id)) => opened = Some(v["threadId"].as_str().unwrap_or(&thread_id).to_string()),
            (Ok(_), None) => {}
            (Err(e), _) => failed.push((outgoing, e)),
        }
        changed = true;
    }
    for (outgoing, error) in failed {
        send_failed(s, outgoing, &error);
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
        if key.starts_with("queued-edit:") || (blank && d.model.is_none() && d.runtime_mode.is_none() && d.interaction_mode.is_none() && d.effort.is_none()) {
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
    })
}

/// Shows a command's failure in the banner of `key`'s route.
fn watch(s: &mut App, request: u64, key: &str) {
    s.ui.chat.composer.sends.push((request, Outgoing { key: key.to_owned(), draft: Value::Null }));
}

/// A failed send shows in its route's banner, and its draft comes back unless a new one was started.
fn send_failed(s: &mut App, outgoing: Outgoing, error: &Value) {
    let message = crate::shell::palette::error_message(error, "Failed to send message.");
    crate::chat::timeline::error_banner::set_local(s, &outgoing.key, Some(&message));
    if outgoing.draft.is_null() {
        return;
    }
    let blank = s.ui.chat.composer.drafts.get(&outgoing.key).is_none_or(|d| d.edit.text.trim().is_empty() && d.attachments.is_empty());
    if blank && let Some(draft) = restored(&json!({"draft": outgoing.draft})).remove("draft") {
        s.ui.chat.composer.drafts.insert(outgoing.key, draft);
    }
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
    match e.key {
        Key::Enter if e.mods.cmd && e.mods.shift => {
            if let Some(thread) = thread {
                crate::chat::timeline::queue::steer_next(s, &thread);
            }
            true
        }
        Key::Enter if !answering && (e.mods.cmd || plain) => {
            let text = draft(s).map(|d| d.edit.text.as_str()).unwrap_or("");
            let shortcut = crate::settings::value(s, "sendShortcut");
            let shortcut = shortcut.as_str().unwrap_or("enter");
            let needs_mod = shortcut == "mod-enter" || (shortcut == "mod-enter-multiline" && text.contains('\n'));
            if plain && needs_mod {
                draft_mut(s).edit.insert("\n");
            } else {
                // Mod+Enter while a turn runs is the alternate follow-up (steer <-> queue).
                send(s, e.mods.cmd);
            }
            true
        }
        Key::Up if e.mods.alt && !e.mods.shift && !answering => match thread {
            Some(thread) => crate::chat::timeline::queue::edit_latest(s, &thread),
            None => false,
        },
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

/// The composer's window-wide shortcuts (`modelPicker.toggle`, `composer.effort`,
/// `composer.mode`), which the web binds outside the terminal. Effort lives in the model picker.
pub fn shortcut(s: &mut App, e: &KeyEvent) -> bool {
    if !matches!(s.frame_route(), Route::Thread(_) | Route::Home { .. }) || crate::panel::terminal_active(s) || ctx(s).project.is_none() {
        return false;
    }
    if crate::panel::shortcut(s, e, "modelPicker.toggle") || crate::panel::shortcut(s, e, "composer.effort") {
        picker::toggle(s);
        return true;
    }
    if crate::panel::shortcut(s, e, "composer.mode") && supports_plan(s) {
        toggle_plan(s);
        return true;
    }
    false
}

/// While an approval waits, the prompt says a message declines it (ChatComposer).
fn placeholder(s: &App) -> &'static str {
    let pending = approvals(s);
    if pending.is_empty() {
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
    let field = if let Some(q) = question::editor(s) {
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
                menu::refresh(s);
            })
    };
    let editor = col().w_full().px(16.0).pt(16.0).child(auto_grow(field, 2, 8));

    let mut body = col()
        .key("composer")
        .on_drop_files(|s: &mut App, files: &[std::path::PathBuf], _: &mut Tx| {
            if editing_run(s).is_none() {
                draft_mut(s).attachments.extend(files.iter().map(|p| p.display().to_string()));
            }
        })
        .accepts_extensions(&IMAGE_EXTENSIONS)
        .w_full().radius(radius::XL2).fill(composer_fill(t)).border(1.0, composer_outline(t)).padding(1.0).pb(11.0);
    if !t.dark {
        body = body.shadow(1.0, 2.0, Color::BLACK.alpha(0.05));
    }
    if !d.attachments.is_empty() {
        body = body.child(attachment_chips(t, &d.attachments));
    }
    let body = body.child(editor).child(footer(s, t));

    let menu = if question::active(s).is_some() { None } else { menu::view(s, cx, t) };
    let mut stack = col().key("composer-lane").w(lane.width).pin(Some(lane.left), None, None, Some(BOTTOM));
    if let Some(menu) = menu {
        stack = stack.child(menu.mx(22.0).mb(-17.0));
    }
    // A pending question sits directly above the composer, as on the web. The inset is padding
    // on a wrapper: a full-width child with margins would shift rather than shrink.
    let docked = crate::chat::timeline::composer_requests(s, t).or_else(|| plan::banner(s, t));
    if let Some(docked) = docked {
        stack = stack.child(col().w_full().px(22.0).mb(-17.0).child(docked));
    }
    stack.child(body)
}

/// The composer's own overlays (the model picker), attached after the body so they draw above.
pub fn overlays(s: &App, cx: &mut Cx<App>, t: &T) -> Vec<El> {
    let mut layers = picker::view(s, cx, t);
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

fn footer(s: &App, t: &T) -> El {
    let mut left = row().gap(4.0).center_y().child(
        icon_button(t, &icons::PAPERCLIP, "Attach images", false, t.c.muted_foreground).key("composer-attach").on_press_tx(|_: &mut App, tx| attach(tx)),
    );
    if supports_plan(s) {
        let (glyph, label, help) = if interaction_mode(s) == "plan" {
            (&icons::PENCIL_RULER, "Plan", "Plan mode. Click to return to normal build mode")
        } else {
            (&icons::BOT, "Build", "Default mode. Click to enter plan mode")
        };
        left = left.child(control(t, glyph, label, t.c.foreground.alpha_mul(0.8)).key("composer-interaction-mode").help(help).on_press(toggle_plan));
    }
    let mut right = row().h(28.0).gap(8.0).center_y();
    if let Some((instance, model)) = selection(s) {
        let driver = provider(s, &instance).map(|p| data::s(p, "driver").to_string()).unwrap_or_default();
        let name = model_name(s, &instance, &model);
        // ComposerModelEffort: the level's name, or bars when Appearance's effort display says
        // so; named levels such as Max keep their name.
        let bars_setting = crate::settings::value(s, "effortDisplay") == "bars";
        let effort = effort(s).and_then(|e| {
            let (_, options) = effort_options(s)?;
            let label = options.iter().find(|o| data::s(o, "id") == e).map(|o| data::s(o, "label").to_owned())?;
            Some(match effort_bars(&options, &e).filter(|_| bars_setting) {
                Some((count, filled)) => effort_glyph(count, filled, t.c.muted_foreground),
                None => text(label).font(theme::SM_MEDIUM).color(t.c.muted_foreground).truncate().max_w(96.0),
            })
        });
        right = right.child(
            row()
                .key("composer-model-chip")
                .h(28.0)
                .px(9.0)
                .gap(6.0)
                .center_y()
                .radius(theme::layout::CONTROL_RADIUS)
                .hover(t.c.accent)
                .cursor(Cursor::Pointer)
                .label(name.clone())
                .child(provider_icon(t, &driver, 16.0))
                .child(text(name).font(theme::SM_MEDIUM).color(t.c.foreground.alpha_mul(0.8)).truncate().max_w(224.0))
                .children(effort)
                .on_press(|s: &mut App| picker::toggle(s)),
        );
    }
    right = right.children(context::meter(s, t)).child(primary_action(s, t));
    row().w_full().px(8.0).pt(10.0).center_y().child(left).child(spacer()).child(right)
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
        let stop = running(s).then(|| stop_button(t));
        if let Some(actions) = question::actions(s, t, stop) {
            return actions;
        }
    }
    let typed = draft(s).is_some_and(|d| !d.edit.text.trim().is_empty());
    if let Some(action) = plan::action(s, t, typed) {
        return action;
    }
    let sendable = !rewinding(s) && draft(s).is_some_and(|d| !d.edit.text.trim().is_empty() || !d.attachments.is_empty());
    if running(s) && !sendable {
        return stop_button(t);
    }
    send_button(s, t, sendable)
}

fn stop_button(t: &T) -> El {
    el()
        .key("composer-stop")
        .size(32.0, 32.0)
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

fn send_button(s: &App, t: &T, sendable: bool) -> El {
    let editing = editing_run(s).is_some();
    let (mode, alternate) = (mode_for(s, false), mode_for(s, true));
    let label = match (editing, mode) {
        (true, _) => "Update queued message",
        (_, "queue") => "Queue message",
        (_, "steer") => "Steer message",
        _ => "Submit message",
    };
    // Mod-click sends the other way while a turn runs (ComposerPrimaryActions).
    let help = if !editing && mode != alternate {
        let key = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
        format!("Click to {mode}, {key}-click to {alternate}")
    } else {
        label.to_owned()
    };
    let (bg, fg) = if sendable { (t.c.message_action, t.c.message_action_foreground) } else { (t.c.foreground.alpha_mul(0.12), t.c.muted_foreground) };
    let mut b = el().key("composer-send").size(32.0, 32.0).center().radius(radius::FULL).fill(bg).label(label).child(icon(&icons::ARROW_UP, 14.0).color(fg));
    if sendable {
        b = b.hover(t.c.message_action_hover).cursor(Cursor::Pointer).help(help).on_press_tx(|s: &mut App, tx| send(s, tx.mods().cmd));
    } else if rewinding(s) {
        b = b.help("Rewinding conversation");
    }
    b
}

fn attachment_chips(t: &T, paths: &[String]) -> El {
    let mut chips = row().flex_wrap().gap(6.0).px(12.0).pt(12.0);
    for (i, path) in paths.iter().enumerate() {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
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
                .child(icon(&icons::FILE, 14.0).color(t.c.muted_foreground))
                .child(text(name).font(theme::XS_MEDIUM).color(t.c.foreground.alpha_mul(0.8)).truncate().max_w(180.0))
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

fn attach(tx: &mut Tx) {
    let exts: Vec<&str> = IMAGE_TYPES.iter().map(|(e, _)| *e).collect();
    let options = rune::FileDialogOptions { title: "Attach images".into(), ..Default::default() }.filter("Images", &exts);
    tx.open_file(options, |s: &mut App, picked: rune::FileDialogResult, _| {
        if let Ok(Some(path)) = picked {
            draft_mut(s).attachments.push(path.display().to_string());
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

/// The showcase thread's live turn reporting `used` of a 200k window.
fn context_scene(s: &mut App, used: f64) {
    on_replay_showcase(s);
    let Route::Thread(id) = s.route.clone() else { return };
    if let data::Source::Fixture(state) = &mut s.src
        && let Some(v) = state.thread_views.get_mut(&id)
    {
        v.projection["providerTurns"] = json!([{"tokenUsage": {"usedTokens": used, "maxTokens": 200_000, "updatedAt": "2026-12-01T00:00:00.000Z"}}]);
    }
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
    fn uploads_inline_supported_images_only() {
        let dir = std::env::temp_dir().join(format!("s5-upload-{}", s5_net::uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("shot.PNG");
        std::fs::write(&png, [1u8, 2, 3]).unwrap();
        let notes = dir.join("notes.txt");
        std::fs::write(&notes, "hi").unwrap();
        let v = upload(png.to_str().unwrap()).unwrap();
        assert_eq!(v, json!({"type": "image", "name": "shot.PNG", "mimeType": "image/png", "sizeBytes": 3, "dataUrl": "data:image/png;base64,AQID"}));
        assert!(upload(notes.to_str().unwrap()).is_none());
        assert!(upload(dir.join("gone.png").to_str().unwrap()).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
}
