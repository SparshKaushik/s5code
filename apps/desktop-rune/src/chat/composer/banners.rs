//! ComposerBannerStack: notices about sending from this composer, each its own card attached
//! above it. The machine's own notices come first, then what needs you; under a question only a
//! machine problem stays. The items: Codex feedback (ComposerFeedback), the fallback model
//! (ComposerUsageLimits), the usage-limit recovery and system notices (`timeline::banners`), and
//! the managed runtime-mode ceiling (ChatComposer).

use std::collections::HashMap;

use rune::{Color, El, Tx, col, el, icon, row, text};
use serde_json::{Value, json};

use crate::data::{iso_ms, s};
use crate::kit::button::{Button, icon_button};
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route, icons};

/// How far a card tucks under the one below it (`--chat-composer-attachment-overlap`).
const OVERLAP: f32 = 17.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Variant {
    Default,
    Error,
    Info,
    Success,
    Warning,
}

/// ComposerBannerStackItem. `environment` is the stack's "environment" group, which sorts first.
pub struct Item {
    pub id: String,
    pub variant: Variant,
    pub environment: bool,
    pub icon: &'static rune::Icon,
    pub title: String,
    pub title_control: Option<El>,
    pub description: Option<String>,
    pub actions: Vec<El>,
    pub dismiss: Option<El>,
    pub children: Option<El>,
}

impl Item {
    pub fn new(id: impl Into<String>, variant: Variant, icon: &'static rune::Icon, title: impl Into<String>) -> Item {
        Item { id: id.into(), variant, environment: false, icon, title: title.into(), title_control: None, description: None, actions: Vec::new(), dismiss: None, children: None }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FeedbackStatus {
    Uploading,
    Sent(String),
    Failed(String),
}

#[derive(Default)]
pub struct State {
    pub(crate) system: super::system_banners::State,
    /// Thread id -> its `/feedback` submissions, oldest first.
    pub(crate) feedback: HashMap<String, Vec<(String, FeedbackStatus)>>,
    /// `provider.uploadFeedback` in flight: the request, its thread and submission.
    uploads: Vec<(u64, String, String)>,
}

fn ui(s: &mut App) -> &mut State {
    &mut s.ui.chat.banners
}

// --- Codex feedback ---------------------------------------------------------------------------

/// parseCodexFeedbackCommand: `/feedback` with an optional reason.
fn parse_feedback(text: &str) -> Option<Option<String>> {
    let text = text.trim();
    let rest = text.get(9..).filter(|_| text.get(..9).is_some_and(|c| c.eq_ignore_ascii_case("/feedback")))?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let reason = rest.trim();
    Some((!reason.is_empty()).then(|| reason.to_owned()))
}

/// codexFeedbackNotice.
fn feedback_notice(status: &FeedbackStatus) -> (&'static str, Option<String>) {
    match status {
        FeedbackStatus::Uploading => ("Sending feedback to OpenAI...", None),
        FeedbackStatus::Sent(id) => ("Feedback sent to OpenAI", Some(format!("Thread ID: {id}"))),
        FeedbackStatus::Failed(message) => ("Could not send feedback to OpenAI", Some(message.clone())),
    }
}

/// submitCodexFeedback: on a Codex thread, a bare `/feedback [reason]` uploads the session's
/// logs instead of sending a message. True when the draft was taken.
pub fn send_feedback(app: &mut App) -> bool {
    let Route::Thread(thread) = super::route(app) else { return false };
    if super::selection(app).and_then(|(i, _)| super::provider(app, &i)).is_none_or(|p| s(p, "driver") != "codex") {
        return false;
    }
    let Some(d) = super::draft(app).filter(|d| d.attachments.is_empty() && d.contexts.is_empty()) else { return false };
    let Some(reason) = parse_feedback(&d.edit.text) else { return false };
    let started = app.state().threads.iter().find(|t| s(t, "id") == thread).is_some_and(|t| !s(t, "activeProviderThreadId").is_empty());
    if !started {
        crate::shell::palette::toast(app, "Start a Codex thread first", Some("Send a message before you submit feedback."));
        return true;
    }
    let mut input = json!({"threadId": thread});
    if let Some(reason) = reason {
        input["reason"] = json!(reason);
    }
    let Some(net) = app.src.net() else { return true };
    let request = net.request("provider.uploadFeedback", input);
    let id = s5_net::uuid();
    super::draft_mut(app).edit.clear();
    let state = ui(app);
    state.feedback.entry(thread.clone()).or_default().push((id.clone(), FeedbackStatus::Uploading));
    state.uploads.push((request, thread, id));
    true
}

fn feedback_items(app: &App, t: &T) -> Vec<Item> {
    let Route::Thread(thread) = super::route(app) else { return Vec::new() };
    let Some(entries) = app.ui.chat.banners.feedback.get(&thread) else { return Vec::new() };
    entries
        .iter()
        .map(|(id, status)| {
            let variant = match status {
                FeedbackStatus::Failed(_) => Variant::Error,
                FeedbackStatus::Sent(_) => Variant::Success,
                FeedbackStatus::Uploading => Variant::Info,
            };
            let (title, description) = feedback_notice(status);
            let mut item = Item::new(format!("feedback:{id}"), variant, &icons::MESSAGE_SQUARE, title);
            item.description = description;
            if let FeedbackStatus::Sent(feedback) = status {
                let feedback = feedback.clone();
                item.actions.push(Button::new("Copy ID").variant("ghost").size("xs").build(t).on_press_tx(move |_: &mut App, tx: &mut Tx| tx.copy(feedback.clone())));
            }
            if *status != FeedbackStatus::Uploading {
                let (thread, id) = (thread.clone(), id.clone());
                item.dismiss = Some(dismiss(t, "Dismiss feedback notice", move |s: &mut App| {
                    if let Some(entries) = ui(s).feedback.get_mut(&thread) {
                        entries.retain(|(entry, _)| *entry != id);
                    }
                }));
            }
            item
        })
        .collect()
}

/// Takes the feedback uploads' replies.
pub fn poll(app: &mut App) -> bool {
    if app.ui.chat.banners.uploads.is_empty() {
        return false;
    }
    let Some(net) = app.src.net() else { return false };
    let state = &mut app.ui.chat.banners;
    let mut done = Vec::new();
    state.uploads.retain(|(request, thread, id)| match net.take_reply(*request) {
        Some(reply) => {
            done.push((thread.clone(), id.clone(), reply));
            false
        }
        None => true,
    });
    let changed = !done.is_empty();
    for (thread, id, reply) in done {
        let status = match reply {
            Ok(v) => FeedbackStatus::Sent(s(&v, "feedbackId").to_owned()),
            Err(e) => FeedbackStatus::Failed(crate::shell::palette::error_message(&e, "An error occurred.")),
        };
        if let Some(entry) = state.feedback.get_mut(&thread).and_then(|entries| entries.iter_mut().find(|(entry, _)| *entry == id)) {
            entry.1 = status;
        }
    }
    changed
}

// --- Fallback model ---------------------------------------------------------------------------

fn list(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}

fn selection_of(v: &Value) -> Option<(String, String)> {
    let instance = s(v, "instanceId");
    (!instance.is_empty()).then(|| (instance.to_owned(), s(v, "model").to_owned()))
}

/// exhaustedUsageWindow: a used-up window that has not reset yet, in the model's pool.
fn exhausted<'a>(provider: &'a Value, now: i64, model: &str) -> Option<&'a Value> {
    let mut windows = list(&provider["usageLimits"]["windows"]).iter().filter(|w| {
        w["usedPercent"].as_f64().unwrap_or(0.0) >= 100.0 && w["resetsAt"].as_str().and_then(iso_ms).is_none_or(|at| at > now)
    });
    match s(provider, "driver") {
        "cursor" => {
            let all: Vec<&Value> = windows.collect();
            let api = all.iter().copied().find(|w| s(w, "id") == "apiPercentUsed");
            let auto = all.iter().copied().find(|w| s(w, "id") == "autoPercentUsed");
            if ["auto", "default", "grok", "composer"].iter().any(|p| model.starts_with(*p)) { auto.and(api) } else { api }
        }
        "antigravity" => {
            let family = ["gemini", "claude", "gpt"].into_iter().find(|f| model.starts_with(&format!("{f}-")));
            windows.find(|w| {
                let pool = ["gemini", "claude", "gpt", "3p"].into_iter().find(|p| s(w, "id").starts_with(&format!("{p}-")));
                match (pool, family) {
                    (None, _) | (_, None) => true,
                    (Some(pool), Some(family)) => pool == family || (pool == "3p" && matches!(family, "claude" | "gpt")),
                }
            })
        }
        "opencode" => windows.find(|w| !s(w, "id").starts_with("go_") || model.starts_with("opencode-go/")),
        _ => windows.next(),
    }
}

#[derive(Debug, PartialEq)]
struct Fallback {
    default: (String, String),
    fallback: (String, String),
    /// The provider that ran out, by name, and when its window resets.
    provider: String,
    resets_at: Option<String>,
    /// The dismissal key (`fallbackWindow`): one notice per provider and reset.
    window: String,
}

/// resolveNewThreadModelSelection: the fallback a new thread takes while its default model is
/// out of usage, when the fallback itself can run.
fn resolve_fallback(default: Option<(String, String)>, fallback: Option<(String, String)>, providers: &[Value], now: i64) -> Option<Fallback> {
    let (default, fallback) = (default?, fallback?);
    let find = |instance: &str| providers.iter().find(|p| s(p, "instanceId") == instance);
    let provider = find(&default.0)?;
    let window = exhausted(provider, now, &default.1)?;
    let other = find(&fallback.0)?;
    let runs = other["enabled"] == true && other["installed"] == true && s(&other["auth"], "status") != "unauthenticated" && s(other, "availability") != "unavailable";
    if !runs || exhausted(other, now, &fallback.1).is_some() {
        return None;
    }
    let resets_at = window["resetsAt"].as_str().map(str::to_owned);
    let name = s(provider, "displayName");
    Some(Fallback {
        window: format!("{}:{}", default.0, resets_at.as_deref().unwrap_or("")),
        provider: if name.is_empty() { s(provider, "driver").to_owned() } else { name.to_owned() },
        resets_at,
        default,
        fallback,
    })
}

fn settings(app: &App) -> &Value {
    app.state().config.as_ref().map_or(&Value::Null, |c| &c["settings"])
}

fn fallback_dismiss_key(app: &App) -> String {
    format!("fallbackNoticeDismissed:{}", app.ui.env.active)
}

/// The new-thread draft's fallback, unless a model was picked in it.
fn draft_fallback(app: &App) -> Option<Fallback> {
    if matches!(super::route(app), Route::Thread(_)) || super::draft(app).is_some_and(|d| d.model.is_some()) {
        return None;
    }
    let project = super::ctx(app).project.and_then(|p| selection_of(&p["defaultModelSelection"]));
    let default = project.or_else(|| selection_of(&settings(app)["defaultModelSelection"]));
    resolve_fallback(default, selection_of(&settings(app)["fallbackModelSelection"]), super::providers(app), app.now_ms)
}

/// The model a new-thread draft sends to while its default is out of usage
/// (draftFallback.selection), for the composer's model selection.
pub fn draft_fallback_selection(app: &App) -> Option<(String, String)> {
    draft_fallback(app).map(|f| f.fallback)
}

/// modelFallbackBannerItem.
fn fallback_item(app: &App, t: &T) -> Option<Item> {
    let f = draft_fallback(app)?;
    if app.client_settings.values.get(&fallback_dismiss_key(app)).and_then(Value::as_str) == Some(f.window.as_str()) {
        return None;
    }
    let mut item = Item::new("fallback-model", Variant::Info, &icons::GAUGE, format!("Using {}", super::model_name(app, &f.fallback.0, &f.fallback.1)));
    let resets = f.resets_at.as_deref().map(|at| format!(" · resets {}", crate::chat::timeline::banners::upcoming_label(app, at))).unwrap_or_default();
    item.description = Some(format!("{} is out of usage{resets}", f.provider));
    let default = f.default.clone();
    let label = format!("Use {}", super::model_name(app, &default.0, &default.1));
    item.actions.push(Button::new(label).variant("ghost").size("xs").on_press(super::act(app, move |s: &mut App| super::draft_mut(s).model = Some(default.clone()))).build(t));
    let window = f.window;
    item.dismiss = Some(dismiss(t, "Dismiss fallback model notice", move |s: &mut App| {
        let key = fallback_dismiss_key(s);
        s.client_settings.values.insert(key, Value::String(window.clone()));
        s.client_settings.save();
    }));
    Some(item)
}

// --- Managed runtime-mode ceiling -------------------------------------------------------------

/// managedRuntimeModeDenial and clampManagedRuntimeMode: why the mode is refused, and the
/// broadest mode the policy still allows.
fn ceiling(app: &App) -> Option<(String, Option<(&'static str, &'static str)>)> {
    let mode = super::effective_runtime_mode(app);
    let options = super::runtime_mode_options(app);
    let denial = options.iter().find(|o| o.mode == mode)?.disabled.clone()?;
    Some((denial, options.iter().rev().find(|o| o.disabled.is_none()).map(|o| (o.mode, o.label))))
}

fn ceiling_item(app: &App, t: &T) -> Option<Item> {
    let (denial, fallback) = ceiling(app)?;
    let mut item = Item::new("managed-runtime-mode", Variant::Warning, &icons::BUILDING, denial);
    if let Some((mode, label)) = fallback {
        item.actions.push(Button::new(format!("Switch to {label}")).variant("ghost").size("xs").on_press(super::act(app, move |s: &mut App| super::set_runtime_mode(s, mode))).build(t));
    }
    Some(item)
}

// --- Stack ------------------------------------------------------------------------------------

pub fn dismiss(t: &T, label: &str, on_press: impl Fn(&mut App) + 'static) -> El {
    icon_button(&icons::X, label).variant("ghost").size("icon-xs").on_press(on_press).build(t)
}

/// ChatView's composerBannerItems, then ChatComposer's managed runtime-mode notice.
fn items(app: &App, t: &T) -> Vec<Item> {
    let mut items = feedback_items(app, t);
    items.extend(fallback_item(app, t));
    items.extend(crate::chat::timeline::banners::recovery_item(app, t));
    items.extend(crate::chat::timeline::banners::system_items(app, t));
    items.extend(super::system_banners::auto_item(app, t));
    items.extend(ceiling_item(app, t));
    items
}

/// The machine first, then what needs you; an open question keeps only a machine problem.
fn shelf(mut items: Vec<Item>, question: bool) -> Vec<Item> {
    items.sort_by_key(|i| !i.environment);
    if question {
        items.retain(|i| i.environment && matches!(i.variant, Variant::Error | Variant::Warning));
    }
    items
}

/// Mixes `tint` over `base` (the attached surface's 8% tint).
fn tinted(base: Color, tint: Color, amount: f32) -> Color {
    let mix = |a: f32, b: f32| a + (b - a) * amount;
    Color { r: mix(base.r, tint.r), g: mix(base.g, tint.g), b: mix(base.b, tint.b), a: base.a }
}

/// NoticeCard: icon, title and description on one line, then the actions and dismiss.
fn card(t: &T, item: Item, first: bool) -> El {
    let neutral = if t.dark { Color::WHITE.alpha(0.05) } else { Color::BLACK.alpha(0.08) };
    let base = if t.dark { t.c.surface_raised } else { t.c.card };
    let (outline, fill) = match item.variant {
        Variant::Error => (t.c.error.alpha_mul(0.32), tinted(base, t.c.error, 0.08)),
        Variant::Warning => (t.c.warning.alpha_mul(0.28), tinted(base, t.c.warning, 0.08)),
        _ => (neutral, base),
    };
    let title = item.title;
    let mut words = row()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(4.0)
        .center_y()
        .child(item.title_control.unwrap_or_else(|| text(title.clone()).font(theme::XS_MEDIUM).color(t.c.foreground).single_line().truncate().shrink().min_w(0.0).help_when_truncated(title)));
    if let Some(description) = item.description {
        words = words.child(text(description.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground).single_line().truncate().grow(1.0).shrink().min_w(0.0).help_when_truncated(description));
    }
    let icon_color = if item.environment && std::ptr::eq(item.icon, &icons::CIRCLE_ALERT) { t.c.error } else { t.c.muted_foreground };
    let mut head = row().w_full().min_h(28.0).gap(4.0).center_y().child(el().size(24.0, 24.0).center().child(icon(item.icon, 12.0).color(icon_color))).child(words);
    if !item.actions.is_empty() || item.dismiss.is_some() {
        head = head.child(row().gap(4.0).center_y().children(item.actions).children(item.dismiss));
    }
    let mut out = col().key(format!("composer-banner:{}", item.id)).w_full().fill(fill).border(1.0, outline).px(4.0).pt(5.0).pb(5.0 + OVERLAP).child(head).children(item.children);
    if first {
        out = out.corners([radius::XL2, radius::XL2, 0.0, 0.0]);
    }
    if matches!(item.variant, Variant::Error | Variant::Warning) {
        out = out.role("alert");
    }
    out
}

/// The stack above the composer, ending in `drawer` (an open question, the plan follow-up or
/// the branch notice), each card tucked under the next.
pub fn dock(app: &App, t: &T, drawer: Option<El>) -> Option<El> {
    let question = super::asking(app);
    let mut cards: Vec<El> = shelf(items(app, t), question).into_iter().enumerate().map(|(n, item)| card(t, item, n == 0)).collect();
    cards.extend(drawer);
    let last = cards.len().checked_sub(1)?;
    Some(col().key("composer-banner-stack").w_full().children(cards.into_iter().enumerate().map(|(n, c)| if n < last { c.mb(-OVERLAP) } else { c })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch_config(app: &mut App, f: impl FnOnce(&mut Value)) {
        if let crate::data::Source::Fixture(state) = &mut app.src
            && let Some(config) = state.config.as_mut()
        {
            f(config);
        }
    }

    #[test]
    fn feedback_commands_take_an_optional_reason() {
        assert_eq!(parse_feedback("/feedback"), Some(None));
        assert_eq!(parse_feedback("  /FEEDBACK  it crashed \n"), Some(Some("it crashed".into())));
        assert_eq!(parse_feedback("/feedbacks"), None);
        assert_eq!(parse_feedback("send /feedback"), None);
        assert_eq!(feedback_notice(&FeedbackStatus::Uploading), ("Sending feedback to OpenAI...", None));
        assert_eq!(feedback_notice(&FeedbackStatus::Sent("fb-1".into())).1.as_deref(), Some("Thread ID: fb-1"));
        assert_eq!(feedback_notice(&FeedbackStatus::Failed("Nope".into())).0, "Could not send feedback to OpenAI");
    }

    #[test]
    fn a_fallback_applies_only_while_the_default_is_used_up_and_the_fallback_can_run() {
        let now = iso_ms("2026-03-10T12:00:00Z").unwrap();
        let window = |used: f64, resets: &str| json!({"usageLimits": {"windows": [{"id": "five_hour", "usedPercent": used, "resetsAt": resets}]}});
        let mut claude = window(100.0, "2026-03-10T15:30:00Z");
        claude["instanceId"] = json!("claude");
        claude["driver"] = json!("claudeAgent");
        claude["displayName"] = json!("Claude");
        let open = json!({"instanceId": "open", "driver": "opencode", "enabled": true, "installed": true, "auth": {"status": "authenticated"}});
        let pick = |i: &str, m: &str| Some((i.to_owned(), m.to_owned()));
        let resolved = resolve_fallback(pick("claude", "opus"), pick("open", "big-pickle"), &[claude.clone(), open.clone()], now).unwrap();
        assert_eq!((resolved.provider.as_str(), resolved.window.as_str()), ("Claude", "claude:2026-03-10T15:30:00Z"));
        assert_eq!(resolved.fallback, ("open".to_owned(), "big-pickle".to_owned()));
        assert!(resolve_fallback(pick("claude", "opus"), None, &[claude.clone(), open.clone()], now).is_none(), "no fallback configured");
        assert!(resolve_fallback(pick("claude", "opus"), pick("open", "x"), &[claude.clone(), open.clone()], iso_ms("2026-03-10T16:00:00Z").unwrap()).is_none(), "the window reset");
        let mut signed_out = open.clone();
        signed_out["auth"]["status"] = json!("unauthenticated");
        assert!(resolve_fallback(pick("claude", "opus"), pick("open", "x"), &[claude.clone(), signed_out], now).is_none());
        let mut spent = window(100.0, "2026-03-10T15:30:00Z");
        spent.as_object_mut().unwrap().extend(open.as_object().unwrap().clone());
        assert!(resolve_fallback(pick("claude", "opus"), pick("open", "x"), &[claude, spent], now).is_none(), "the fallback is used up too");
        let cursor = json!({"driver": "cursor", "usageLimits": {"windows": [{"id": "apiPercentUsed", "usedPercent": 100}]}});
        assert!(exhausted(&cursor, now, "gpt-5").is_some());
        assert!(exhausted(&cursor, now, "auto").is_none(), "Auto still has the Cursor pool");
    }

    #[test]
    fn a_mode_above_the_policy_ceiling_offers_the_broadest_allowed() {
        let mut app = App::new();
        app.navigate(Route::Home { project_id: None });
        // Claude names no runtime modes, so it offers all of them.
        super::super::draft_mut(&mut app).model = Some(("claudeAgent".into(), "claude-opus-5-5".into()));
        super::super::draft_mut(&mut app).runtime_mode = Some("full-access".into());
        assert_eq!(ceiling(&app), None);
        patch_config(&mut app, |c| c["managedPolicy"] = json!({"organization": "Example Corp", "limits": {"maxRuntimeMode": "auto-accept-edits"}}));
        assert_eq!(ceiling(&app), Some(("Example Corp limits threads to Auto-accept edits.".to_owned(), Some(("auto-accept-edits", "Auto-accept edits")))));
        super::super::draft_mut(&mut app).runtime_mode = Some("approval-required".into());
        assert_eq!(ceiling(&app), None);
    }

    #[test]
    fn the_stack_puts_the_machine_first_and_a_question_keeps_only_its_problems() {
        let item = |id: &str, variant: Variant, environment: bool| Item { environment, ..Item::new(id, variant, &icons::INFO, id) };
        let ids = |items: Vec<Item>| items.into_iter().map(|i| i.id).collect::<Vec<_>>();
        let all = || vec![item("feedback", Variant::Success, false), item("offline", Variant::Warning, true), item("ceiling", Variant::Warning, false), item("update", Variant::Default, true)];
        assert_eq!(ids(shelf(all(), false)), ["offline", "update", "feedback", "ceiling"]);
        assert_eq!(ids(shelf(all(), true)), ["offline"]);
    }
}
