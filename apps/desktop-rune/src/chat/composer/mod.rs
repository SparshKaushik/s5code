//! The composer (apps/web ChatComposer, ComposerSurface, ComposerPrimaryActions): a glass card
//! floating over the bottom of the chat lane with the editor, attach, the model chip and send.
//! Drafts are kept per thread (and one for the home draft), as the web composer draft store does.
//! `menu` is the @-mention and slash drawer, `picker` the model picker popover.

mod menu;
mod picker;
pub mod provider_icons;

use std::collections::HashMap;

use rune::{Color, Cursor, Cx, El, Key, KeyEvent, TextEdit, Tx, auto_grow, col, el, icon, row, spacer, text, text_input};
use serde_json::{Value, json};

use crate::chat::details::Lane;
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
}

impl Default for Draft {
    fn default() -> Draft {
        Draft { edit: TextEdit::new(""), attachments: Vec::new(), model: None, runtime_mode: None, interaction_mode: None }
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
    /// payload, and the thread a launch starts.
    uploads: Vec<(u64, Value, Option<String>)>,
}

/// The draft key for a route: the thread id, or `home` for the new-thread draft.
pub fn draft_key(route: &Route) -> String {
    match route {
        Route::Thread(id) => id.clone(),
        _ => "home".to_string(),
    }
}

fn draft<'a>(s: &'a App) -> Option<&'a Draft> {
    s.ui.chat.composer.drafts.get(&draft_key(s.frame_route()))
}

fn draft_mut(s: &mut App) -> &mut Draft {
    let key = draft_key(s.frame_route());
    s.ui.chat.composer.drafts.entry(key).or_default()
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

fn send(s: &mut App) {
    let Some(d) = draft(s) else { return };
    let text = d.edit.text.trim().to_string();
    if text.is_empty() && d.attachments.is_empty() {
        return;
    }
    // A file that can no longer be read keeps the draft as it is.
    let Some(uploads) = d.attachments.iter().map(|p| upload(p)).collect::<Option<Vec<_>>>() else { return };
    let model = selection(s).map(|(instance, model)| json!({"instanceId": instance, "model": model}));
    let running = running(s);
    let (runtime, interaction) = (runtime_mode(s), interaction_mode(s));
    let route = s.frame_route().clone();
    let project_id = ctx(s).project.map(|p| data::s(p, "id").to_string());
    let message_id = s5_net::uuid();
    let (thread_id, payload, launch) = match route {
        Route::Thread(id) => {
            let mut cmd = json!({
                "type": "message.dispatch", "commandId": s5_net::uuid(), "createdBy": "user", "creationSource": "web",
                "threadId": id, "messageId": message_id, "text": text, "attachments": [],
                "deliveryIntent": if running { "steer" } else { "auto" },
                "dispatchMode": {"type": "start_immediately"},
            });
            if let Some(m) = model {
                cmd["modelSelection"] = m;
            }
            (id, cmd, false)
        }
        _ => {
            let Some(project_id) = project_id else { return };
            let thread_id = format!("thread:project:{project_id}:{}", s5_net::uuid());
            let params = json!({
                "commandId": s5_net::uuid(), "creationSource": "web", "threadId": thread_id, "projectId": project_id,
                "title": "New thread", "generateTitle": true, "modelSelection": model, "runtimeMode": runtime,
                "interactionMode": interaction, "workspaceStrategy": {"type": "root"},
                "initialMessage": {"messageId": message_id, "text": text, "attachments": []},
            });
            (thread_id, params, true)
        }
    };
    let launch = launch.then(|| thread_id.clone());
    let Some(net) = s.src.net() else { return };
    let composer = &mut s.ui.chat.composer;
    if uploads.is_empty() {
        deliver(net, &mut composer.launches, payload, launch, json!([]));
    } else {
        let request = net.request(
            "assets.persistChatAttachments",
            json!({"threadId": thread_id, "messageId": message_id, "attachments": uploads}),
        );
        composer.uploads.push((request, payload, launch));
    }
    sent(s);
}

/// Image types the server stores (PROVIDER_SEND_TURN_SUPPORTED_IMAGE_MIME_TYPES), by extension.
const IMAGE_TYPES: [(&str, &str); 5] =
    [("png", "image/png"), ("jpg", "image/jpeg"), ("jpeg", "image/jpeg"), ("gif", "image/gif"), ("webp", "image/webp")];
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

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
fn deliver(net: &mut s5_net::Net, launches: &mut Vec<(u64, String)>, mut payload: Value, launch: Option<String>, attachments: Value) {
    match launch {
        Some(thread_id) => {
            payload["initialMessage"]["attachments"] = attachments;
            launches.push((net.request("orchestration.launchThread", payload), thread_id));
        }
        None => {
            payload["attachments"] = attachments;
            net.dispatch(payload);
        }
    }
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
    }
}

/// Opening a thread or the new-thread draft puts the caret in its composer (ChatView).
pub fn focus(s: &mut App, route: &Route) {
    s.ui.chat.composer.drafts.entry(draft_key(route)).or_default().edit.focused = true;
}

fn stop(s: &mut App) {
    let Some(t) = ctx(s).thread else { return };
    let (id, run) = (data::s(t, "id").to_string(), data::s(t, "activeRunId").to_string());
    let Some(net) = s.src.net() else { return };
    if run.is_empty() {
        net.dispatch(json!({"type": "thread.stop", "commandId": s5_net::uuid(), "threadId": id}));
    } else {
        net.dispatch(json!({"type": "run.interrupt", "commandId": s5_net::uuid(), "threadId": id, "runId": run}));
    }
}

/// Folds server replies the composer waits on (launches, file searches). Called from `poll`.
pub fn poll(s: &mut App) -> bool {
    let Some(net) = s.src.net() else { return false };
    let mut changed = false;
    let composer = &mut s.ui.chat.composer;
    let mut i = 0;
    while i < composer.uploads.len() {
        let Some(reply) = net.take_reply(composer.uploads[i].0) else {
            i += 1;
            continue;
        };
        let (_, payload, launch) = composer.uploads.remove(i);
        match reply {
            Ok(v) => deliver(net, &mut composer.launches, payload, launch, v["attachments"].clone()),
            Err(e) => eprintln!("composer: storing attachments failed: {e}"),
        }
        changed = true;
    }
    let mut opened = None;
    s.ui.chat.composer.launches.retain(|(request, thread_id)| match net.take_reply(*request) {
        Some(reply) => {
            if let Ok(v) = reply {
                opened = Some(v["threadId"].as_str().unwrap_or(thread_id).to_string());
            }
            false
        }
        None => true,
    });
    changed |= menu::poll(&mut s.ui.chat.composer.mention, net);
    if let Some(id) = opened {
        s.navigate(Route::Thread(id));
        changed = true;
    }
    changed
}

/// Keys inside the editor: the open menu's arrows, Return and Tab, Escape, and Shift+Tab for plan.
fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if s.ui.chat.composer.picker.open {
        return false;
    }
    if let Some(m) = menu::open_menu(s) {
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
    match e.key {
        Key::Tab if e.mods.shift => {
            if supports_plan(s) {
                toggle_plan(s);
            }
            true
        }
        Key::Escape if running(s) => {
            stop(s);
            true
        }
        _ => false,
    }
}

fn supports_plan(s: &App) -> bool {
    selection(s).and_then(|(i, _)| provider(s, &i).map(|p| p["showInteractionModeToggle"] == true)).unwrap_or(false)
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &T, lane: &Lane) -> El {
    let empty = Draft::default();
    let d = draft(s).unwrap_or(&empty);
    let field = text_input(&d.edit, theme::TEXT_SM.lh(22.75))
        .key("composer-editor")
        .w_full()
        .color(t.c.foreground)
        .placeholder(PLACEHOLDER, t.c.placeholder)
        .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
        .on_edit(|s: &mut App| &mut draft_mut(s).edit)
        .on_submit(|s: &mut App, _| send(s))
        .on_input_key(on_key)
        .on_input_change(|s: &mut App, _| {
            s.ui.chat.composer.menu_active = 0;
            menu::refresh(s);
        });
    let editor = col().w_full().px(16.0).pt(16.0).child(auto_grow(field, 2, 8));

    let mut body = col().key("composer").w_full().radius(radius::XL2).fill(composer_fill(t)).border(1.0, composer_outline(t)).padding(1.0).pb(11.0);
    if !t.dark {
        body = body.shadow(1.0, 2.0, Color::BLACK.alpha(0.05));
    }
    if !d.attachments.is_empty() {
        body = body.child(attachment_chips(t, &d.attachments));
    }
    let body = body.child(editor).child(footer(s, t));

    let menu = menu::view(s, cx, t);
    let mut stack = col().key("composer-lane").w(lane.width).pin(Some(lane.left), None, None, Some(BOTTOM));
    if let Some(menu) = menu {
        stack = stack.child(menu.mx(22.0).mb(-17.0));
    }
    // A pending question sits directly above the composer, as on the web.
    if let crate::Route::Thread(id) = s.frame_route() {
        stack = stack.child(crate::chat::timeline::composer_requests(s, t, id));
    }
    stack.child(body)
}

/// The composer's own overlays (the model picker), attached after the body so they draw above.
pub fn overlays(s: &App, cx: &mut Cx<App>, t: &T) -> Vec<El> {
    picker::view(s, cx, t)
}

fn composer_fill(t: &T) -> Color {
    if t.dark { t.c.surface_raised } else { t.c.card }
}

fn composer_outline(t: &T) -> Color {
    if t.dark { Color::WHITE.alpha(0.05) } else { Color::BLACK.alpha(0.08) }
}

fn footer(s: &App, t: &T) -> El {
    let mut left = row().gap(4.0).center_y().child(
        icon_button(t, &icons::PAPERCLIP, "Attach files", false, t.c.muted_foreground).key("composer-attach").on_press_tx(|_: &mut App, tx| attach(tx)),
    );
    if interaction_mode(s) == "plan" {
        left = left.child(
            control(t, &icons::PENCIL_RULER, "Plan", t.c.foreground.alpha_mul(0.8))
                .help("Plan mode. Click to return to normal build mode")
                .on_press(toggle_plan),
        );
    }
    let mut right = row().h(28.0).gap(8.0).center_y();
    if let Some((instance, model)) = selection(s) {
        let driver = provider(s, &instance).map(|p| data::s(p, "driver").to_string()).unwrap_or_default();
        let name = model_name(s, &instance, &model);
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
                .on_press(|s: &mut App| picker::toggle(s)),
        );
    }
    right = right.child(primary_action(s, t));
    row().w_full().px(8.0).pt(10.0).center_y().child(left).child(spacer()).child(right)
}

fn primary_action(s: &App, t: &T) -> El {
    let sendable = draft(s).is_some_and(|d| !d.edit.text.trim().is_empty() || !d.attachments.is_empty());
    if running(s) && !sendable {
        return el()
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
            .on_press(stop);
    }
    let label = if running(s) { "Steer message" } else { "Submit message" };
    let (bg, fg) = if sendable { (t.c.message_action, t.c.message_action_foreground) } else { (t.c.foreground.alpha_mul(0.12), t.c.muted_foreground) };
    let mut b = el().key("composer-send").size(32.0, 32.0).center().radius(radius::FULL).fill(bg).label(label).child(icon(&icons::ARROW_UP, 14.0).color(fg));
    if sendable {
        b = b.hover(t.c.message_action_hover).cursor(Cursor::Pointer).help(label).on_press(send);
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
        _ => icon(&icons::BOT, size).color(t.c.muted_foreground),
    }
}

fn set_text(s: &mut App, text: &str) {
    let d = draft_mut(s);
    d.edit.replace_text(text);
    d.edit.set_caret(text.len());
    d.edit.focused = true;
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
    let id = draft_key(s.frame_route());
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
    fn navigating_focuses_the_route_draft() {
        let mut s = App::new();
        let id = data::s(&s.state().threads[0], "id").to_string();
        s.navigate(Route::Thread(id.clone()));
        assert!(s.ui.chat.composer.drafts[&id].edit.focused);
        s.navigate(Route::Home { project_id: None });
        assert!(s.ui.chat.composer.drafts["home"].edit.focused);
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
}
