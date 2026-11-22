//! The context window meter beside the send button (ContextWindowMeter, contextWindow.ts).

use crate::data::{iso_ms, s};
use crate::icons;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route};
use rune::{Color, Cursor, El, LineCap, Shape, canvas, col, el, icon, row, text};
use serde_json::{Value, json};

#[derive(Debug, PartialEq, Default)]
pub struct Usage {
    pub used: f64,
    pub max: Option<f64>,
    /// Tokens the thread processed in all, when the provider reports it.
    pub total: Option<f64>,
    pub compacts: bool,
    pub threshold: Option<f64>,
    /// The amount and currency the provider reports.
    pub cost: Option<(f64, String)>,
    /// When the usage was reported, ms.
    pub updated_at: Option<i64>,
    /// How long the provider keeps its prompt cache, from a live turn's report.
    pub cache_ttl: Option<i64>,
}

/// Hover state of the ring and its popover; either keeps the popover open.
#[derive(Default)]
pub struct Hover {
    pub(super) trigger: bool,
    card: bool,
}

impl Usage {
    fn percent(&self) -> Option<f64> {
        self.max.filter(|m| *m > 0.0).map(|m| (self.used / m * 100.0).min(100.0))
    }
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

/// The thread's latest context usage (deriveLatestContextWindowSnapshot): the live turn's
/// token usage, else the provider thread's, else the last compaction's result.
pub fn usage(p: &Value) -> Option<Usage> {
    let mut live: Option<&Value> = None;
    for turn in p["providerTurns"].as_array().into_iter().flatten() {
        let u = &turn["tokenUsage"];
        if u.is_object() && live.is_none_or(|l| iso_ms(s(u, "updatedAt")) >= iso_ms(s(l, "updatedAt"))) {
            live = Some(u);
        }
    }
    if let Some(u) = live {
        return Some(Usage {
            used: num(&u["usedTokens"]).unwrap_or(0.0).max(0.0),
            max: num(&u["maxTokens"]),
            compacts: true,
            threshold: num(&u["autoCompactThreshold"]),
            updated_at: iso_ms(s(u, "updatedAt")),
            cache_ttl: u["promptCacheTtlMs"].as_i64().filter(|n| *n > 0),
            ..Usage::default()
        });
    }
    let active = s(&p["thread"], "activeProviderThreadId");
    let provider = p["providerThreads"].as_array().into_iter().flatten().find(|t| !active.is_empty() && s(t, "id") == active);
    if let Some(t) = provider.filter(|t| t["contextUsage"].is_object()) {
        let u = &t["contextUsage"];
        let cost = num(&u["cost"]["amount"]).zip(u["cost"]["currency"].as_str().map(str::to_owned));
        return Some(Usage {
            used: num(&u["usedTokens"]).unwrap_or(0.0),
            max: num(&u["maxTokens"]),
            total: num(&u["totalProcessedTokens"]),
            compacts: u["compactsAutomatically"] == true,
            threshold: num(&u["autoCompactThreshold"]),
            cost,
            updated_at: iso_ms(s(t, "updatedAt")),
            ..Usage::default()
        });
    }
    let items = p["visibleTurnItems"].as_array().map(|v| v.iter().map(|e| &e["item"]).collect::<Vec<_>>()).unwrap_or_else(|| p["turnItems"].as_array().into_iter().flatten().collect());
    items
        .into_iter()
        .rev()
        .filter(|i| s(i, "type") == "compaction")
        .find_map(|i| {
            let used = num(&i["afterTokenCount"]).filter(|n| *n >= 0.0)?;
            Some(Usage { used, total: num(&i["beforeTokenCount"]), compacts: true, updated_at: iso_ms(s(i, "updatedAt")), ..Usage::default() })
        })
}

/// formatContextWindowTokens: 950, 1.2k, 42k, 1.5m.
pub fn tokens(n: f64) -> String {
    if n < 1_000.0 {
        format!("{}", n.round())
    } else if n < 10_000.0 {
        format!("{:.1}k", n / 1_000.0).replace(".0k", "k")
    } else if n < 999_500.0 {
        format!("{}k", (n / 1_000.0).round())
    } else {
        format!("{:.1}m", n / 1_000_000.0).replace(".0m", "m")
    }
}

fn percent_label(p: f64) -> String {
    if p < 10.0 { format!("{p:.1}%").replace(".0%", "%") } else { format!("{}%", p.round()) }
}

/// formatContextWindowCost: four decimals under a cent.
fn cost_label(amount: f64, currency: &str) -> String {
    if amount.abs() > 0.0 && amount.abs() < 0.01 { format!("{currency} {amount:.4}") } else { format!("{currency} {amount:.2}") }
}

/// formatContextWindowCompactionMessage.
fn compaction_message(model: Option<&str>, threshold: Option<f64>) -> String {
    match (threshold.filter(|n| *n > 0.0), model) {
        (Some(n), _) => format!("Compacts automatically at about {} tokens.", thousands(n as u64)),
        (None, Some(m)) => format!("Context for {m} compacts automatically when needed."),
        (None, None) => "Context compacts automatically when needed.".to_owned(),
    }
}

pub(super) fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Past 90% the ring and bar turn the error colour (ContextWindowMeter's isOverloaded).
fn usage_color(t: &T, percent: f64) -> Color {
    if percent > 90.0 { t.c.error } else { t.c.muted_foreground.alpha_mul(0.72) }
}

/// ThreadDetailsContext's row: the bar fills as context is used; the tick marks auto-compaction.
pub(crate) fn detail_row(app: &App) -> Option<crate::chat::details::usage::Row> {
    use crate::chat::details::usage::{Row, Tone};
    let u = current(app)?;
    let max = u.max.filter(|m| *m > 0.0);
    let used = max.and(u.percent()).map(|p| p.clamp(0.0, 100.0).round());
    let tick = max.zip(u.threshold).filter(|(m, th)| *th > 0.0 && th < m).map(|(m, th)| th / m * 100.0);
    let used_tokens = tokens(u.used);
    let mut help = vec![match used {
        Some(p) => format!("Context: {p}% used"),
        None => format!("Context: {used_tokens} tokens used"),
    }];
    if let Some(m) = max {
        help.push(format!("{} of {} tokens", thousands(u.used as u64), thousands(m as u64)));
    }
    if let Some(threshold) = u.threshold.filter(|n| *n > 0.0) {
        help.push(compaction_message(None, Some(threshold)));
    }
    let tone = match used {
        Some(p) if p >= 90.0 => Tone::Error,
        Some(p) if p >= 80.0 => Tone::Warning,
        _ => Tone::Plain,
    };
    let value = match max {
        Some(m) => format!("{used_tokens} / {}", tokens(m)),
        None => used_tokens,
    };
    Some(Row { label: "Context".into(), fill: used, tone, tick, value, help, muted: true })
}

fn current(app: &App) -> Option<Usage> {
    let Route::Thread(id) = super::route(app) else { return None };
    usage(&app.state().thread_views.get(&id)?.projection)
}

/// A user message that is more than a bare `/compact` (activeThreadHasCompactableConversation).
fn compactable(p: &Value) -> bool {
    let visible = p["visibleTurnItems"].as_array().map(|v| v.iter().map(|e| &e["item"]).collect::<Vec<_>>());
    let items = visible.unwrap_or_else(|| p["turnItems"].as_array().into_iter().flatten().collect());
    items.into_iter().any(|i| {
        s(i, "type") == "user_message" && (!s(i, "text").trim().eq_ignore_ascii_case("/compact") || i["attachments"].as_array().is_some_and(|a| !a.is_empty()))
    })
}

/// Why "Compact context" is off (ChatView's compactDisabledReason), or `None` when it can run.
/// `None` overall when the provider has no `/compact`, so the button stays out.
pub(super) fn compact_state(app: &App) -> Option<Option<&'static str>> {
    let (instance, _) = super::selection(app)?;
    let provider = super::provider(app, &instance)?;
    let supported = provider["slashCommands"].as_array().is_some_and(|c| c.iter().any(|c| s(c, "name") == "compact"));
    if !supported {
        return None;
    }
    if !crate::chat::can_operate(app) {
        return Some(Some(super::READ_ONLY));
    }
    if super::ctx(app).project.is_none() {
        return Some(Some("Choose a project before compacting"));
    }
    let Route::Thread(id) = super::route(app) else { return Some(Some(UNAVAILABLE)) };
    let has_history = app.state().thread_views.get(&id).is_some_and(|v| compactable(&v.projection));
    let busy = !has_history
        || super::running(app)
        || !super::approvals(app).is_empty()
        || super::asking(app)
        || super::plan_follow_up(app).is_some()
        || super::rewinding(app)
        || super::feedback_uploading(app)
        || super::disconnected(app)
        || super::no_provider(app)
        || super::send_busy(app);
    Some(busy.then_some(UNAVAILABLE))
}

const UNAVAILABLE: &str = "Compacting is unavailable right now";

/// COMPACTION_SUGGESTION_MIN_TOKENS.
const SUGGEST_MIN_TOKENS: f64 = 200_000.0;
/// Claude's hour-long prompt cache and some slack, when the turn reports no TTL.
const FALLBACK_CACHE_TTL_MS: i64 = 70 * 60_000;
/// CLAUDE_RESUME_COMPACTION_NEVER_ANSWER.
const NEVER_ANSWER: &str = "Don't ask again";

/// shouldOfferResumeCompaction: a Claude session big enough to matter whose prompt cache has
/// expired since its last report.
fn offers_resume(driver: &str, u: &Usage, now: i64) -> bool {
    driver == "claudeAgent"
        && u.used >= SUGGEST_MIN_TOKENS
        && u.updated_at.is_some_and(|at| now - at >= u.cache_ttl.unwrap_or(FALLBACK_CACHE_TTL_MS))
}

/// resolveIdleCompaction: the provider instance's own `idleCompaction`, else the server's.
fn idle_compaction<'a>(settings: &'a Value, instance: &str) -> &'a str {
    let own = s(&settings["providerInstances"][instance]["config"], "idleCompaction");
    if !own.is_empty() && own != "default" {
        return own;
    }
    match &settings["compactIdleThreads"] {
        _ if !s(settings, "idleCompaction").is_empty() => s(settings, "idleCompaction"),
        Value::String(mode) => mode.as_str(),
        Value::Bool(true) => "beforeCacheExpires",
        _ => "onReturn",
    }
}

/// hasDismissedResumeCompaction: the provider's own resume question was answered "Don't ask again".
fn dismissed_resume(p: &Value) -> bool {
    let asks = |q: &str| q.starts_with("This session is ") && q.ends_with(" tokens. Compact it before continuing?");
    p["runtimeRequests"].as_array().into_iter().flatten().filter(|r| s(r, "kind") == "user_input" && s(r, "status") == "resolved").any(|r| {
        r["answers"].as_object().is_some_and(|answers| {
            answers.iter().any(|(question, answer)| asks(question) && (answer == NEVER_ANSWER || answer.as_array().is_some_and(|a| a.iter().any(|v| v == NEVER_ANSWER))))
        })
    })
}

/// ChatView's resumeCompactionTokens: the tokens a send would compact first on an idle Claude
/// thread whose cache has gone cold, unless compaction is off, dismissed or can't run now.
pub(super) fn resume_compaction(app: &App) -> Option<f64> {
    let Route::Thread(id) = super::route(app) else { return None };
    let (instance, _) = super::selection(app)?;
    let driver = s(super::provider(app, &instance)?, "driver");
    let p = &app.state().thread_views.get(&id)?.projection;
    let u = usage(p)?;
    let offered = idle_compaction(crate::settings::server_settings(app), &instance) != "off"
        && !dismissed_resume(p)
        && !super::held_queue(p)
        && matches!(compact_state(app), Some(None))
        && offers_resume(driver, &u, app.now_ms);
    offered.then_some(u.used)
}

/// The Compact/Keep toggle beside Send while a send would compact first (ComposerPrimaryActions).
pub(super) fn compact_toggle(app: &App, t: &T, amount: f64) -> El {
    let keep = super::keeps_full_history(app);
    let tokens = tokens(amount);
    let help = if keep {
        format!("Next send keeps all {tokens} tokens.\nClick to compact first")
    } else {
        format!("Next send compacts {tokens} tokens first.\nClick to keep full history")
    };
    let mut toggle = row()
        .key(super::keyed(app, "composer-compact-toggle"))
        .h(28.0)
        .px(8.0)
        .gap(6.0)
        .center_y()
        .radius(crate::theme::layout::CONTROL_RADIUS)
        .role("button")
        .label(format!("Compact {tokens} tokens of history before sending"))
        .help(help)
        .child(icon(&icons::MINIMIZE_2, 16.0).color(t.c.muted_foreground))
        .child(text(if keep { "Keep" } else { "Compact" }).font(theme::SM_MEDIUM).color(t.c.foreground.alpha_mul(0.8)))
        .child(text(tokens).font(theme::SM_MEDIUM).color(t.c.muted_foreground));
    if !keep {
        toggle = toggle.fill(t.c.accent.alpha_mul(0.6));
    }
    if crate::chat::can_operate(app) {
        toggle = toggle.hover(t.c.accent).cursor(Cursor::Pointer).on_press(super::act(app, super::toggle_full_history));
    } else {
        toggle = toggle.opacity(0.64);
    }
    toggle
}

/// `sendStandaloneCommand("/compact")`: a turn of its own, whatever the composer holds.
fn compact(app: &mut App) {
    if !matches!(compact_state(app), Some(None)) {
        return;
    }
    let Route::Thread(thread) = super::route(app) else { return };
    let model = super::model_selection(app);
    let Some(net) = app.src.net() else { return };
    let mut cmd = json!({
        "type": "message.dispatch", "commandId": s5_net::uuid(), "createdBy": "user", "creationSource": "web",
        "threadId": thread, "messageId": s5_net::uuid(), "text": "/compact", "attachments": [],
        "dispatchMode": {"type": "start_immediately"},
    });
    if let Some(m) = model {
        cmd["modelSelection"] = m;
    }
    let request = net.dispatch(cmd);
    super::watch(app, request, &thread);
    app.ui.chat.composer.context = Hover::default();
}

/// The popover above the ring: usage, a bar, totals, cost, auto-compaction and Compact.
pub fn popover(app: &App, t: &T) -> Option<El> {
    let hover = &app.ui.chat.composer.context;
    if (!hover.trigger && !hover.card) || crate::settings::value(app, "contextWindowMeterEnabled") != true {
        return None;
    }
    let u = current(app)?;
    let pct = u.percent();
    let secondary = |v: String| text(v).font(theme::TEXT_2XS).color(t.c.secondary_label);
    let figures = match (pct, u.max) {
        (Some(p), Some(max)) => format!("{} · {}/{}", percent_label(p), tokens(u.used), tokens(max)),
        _ => tokens(u.used),
    };
    let line = |label: &str, value: String| {
        row().w_full().gap(12.0).center_y().child(secondary(label.to_owned()).grow(1.0)).child(text(value).font(theme::weight(theme::TEXT_2XS, theme::MEDIUM)).color(t.c.secondary_label))
    };
    let mut body = col().w_full().gap(8.0).child(
        row()
            .w_full()
            .gap(12.0)
            .center_y()
            .child(text("Context Window").font(theme::XS_MEDIUM).color(t.c.muted_foreground).grow(1.0))
            .child(secondary(figures)),
    );
    if u.max.is_some() {
        let fill = (pct.unwrap_or(0.0).clamp(0.0, 100.0) / 100.0) as f32;
        body = body.child(
            row()
                .key("context-window-bar")
                .role("progressbar")
                .label("Context window usage")
                .w_full()
                .h(6.0)
                .radius(3.0)
                .fill(t.c.muted.alpha_mul(0.6))
                .child(el().h(6.0).w_frac(fill).radius(3.0).fill(usage_color(t, pct.unwrap_or(0.0)))),
        );
    }
    if let Some(total) = u.total.filter(|n| *n > 0.0) {
        body = body.child(line("Total processed", tokens(total)));
    }
    if let Some((amount, currency)) = &u.cost {
        body = body.child(line("Cost", cost_label(*amount, currency)));
    }
    if u.compacts {
        let model = super::selection(app).map(|(i, m)| super::model_name(app, &i, &m));
        body = body.child(text(compaction_message(model.as_deref(), u.threshold)).font(theme::weight(theme::TEXT_2XS, theme::MEDIUM)).color(t.c.secondary_label).wrap().mt(4.0));
    }
    if let Some(reason) = compact_state(app) {
        body = body.child(
            crate::kit::button::Button::new("Compact context")
                .variant("outline")
                .size("xs")
                .icon(&icons::MINIMIZE_2)
                .disabled(reason.is_some())
                .on_press(super::act(app, compact))
                .build(t)
                .w_full()
                .justify_center()
                .mt(4.0),
        );
        if let Some(reason) = reason {
            body = body.child(secondary(reason.to_owned()).wrap());
        }
    }
    Some(
        col()
            .key("context-window-popover")
            .w(256.0)
            .padding(10.0)
            .radius(radius::LG)
            .fill(t.c.popover)
            .border(1.0, t.c.border)
            .shadow(8.0, 24.0, Color::BLACK.alpha(if t.dark { 0.32 } else { 0.08 }))
            .on_hover_change(|app: &mut App, inside, _| app.ui.chat.composer.context.card = inside)
            .child(body)
            .anchored(&super::keyed(app, "composer-context"), rune::Side::Top, rune::Align::End, 8.0),
    )
}

/// The ring; hovering it opens `popover` after 150ms (PopoverTrigger openOnHover).
pub fn meter(app: &App, t: &T) -> Option<El> {
    if crate::settings::value(app, "contextWindowMeterEnabled") != true {
        return None;
    }
    let u = current(app)?;
    let pct = u.percent();
    let label = match (pct, u.max) {
        (Some(p), Some(_)) => format!("Context window {} used", percent_label(p)),
        _ => format!("Context window {} tokens used", tokens(u.used)),
    };
    let fraction = (pct.unwrap_or(0.0) / 100.0) as f32;
    let color = usage_color(t, pct.unwrap_or(0.0));
    let circle = |from: f32, to: f32| -> Vec<[f32; 2]> {
        let steps = ((to - from) * 64.0).ceil().max(1.0) as usize;
        (0..=steps)
            .map(|i| {
                // From 12 o'clock, clockwise (the web rotates its SVG -90°).
                let a = (from + (to - from) * i as f32 / steps as f32) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                [12.0 + 9.75 * a.cos(), 12.0 + 9.75 * a.sin()]
            })
            .collect()
    };
    let mut shape = Shape::new().view_box(24.0, 24.0).stroke_with(circle(0.0, 1.0), true, 3.0, LineCap::Round, t.c.muted_foreground.alpha_mul(0.24));
    if fraction > 0.0 {
        shape = shape.stroke_with(circle(0.0, fraction), false, 3.0, LineCap::Round, color);
    }
    let at = super::drawing(app);
    Some(
        el().key(super::keyed(app, "composer-context"))
            .size(28.0, 28.0)
            .center()
            .radius(crate::theme::layout::CONTROL_RADIUS)
            .hover(t.c.accent)
            .cursor(Cursor::Default)
            .label(label)
            .hover_delays(std::time::Duration::from_millis(150), std::time::Duration::from_millis(150))
            .on_hover_change(move |app: &mut App, inside, _| super::acting(app, at.clone(), |app| app.ui.chat.composer.context.trigger = inside))
            .child(canvas(shape).size(20.0, 20.0)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn usage_prefers_the_live_turn_then_the_provider_thread_then_compaction() {
        let live = json!({"providerTurns":[
            {"tokenUsage":{"usedTokens":900,"maxTokens":1000,"updatedAt":"2026-01-01T00:00:02Z"}},
            {"tokenUsage":{"usedTokens":100,"maxTokens":1000,"updatedAt":"2026-01-01T00:00:01Z"}}
        ]});
        assert_eq!(usage(&live), Some(Usage { used: 900.0, max: Some(1000.0), compacts: true, updated_at: iso_ms("2026-01-01T00:00:02Z"), ..Usage::default() }));
        let provider = json!({"thread":{"activeProviderThreadId":"p"},"providerThreads":[{"id":"p","contextUsage":{
            "usedTokens":5,"maxTokens":null,"compactsAutomatically":true,"autoCompactThreshold":160000,"cost":{"amount":0.004,"currency":"USD"}}}]});
        assert_eq!(
            usage(&provider),
            Some(Usage { used: 5.0, compacts: true, threshold: Some(160_000.0), cost: Some((0.004, "USD".into())), ..Usage::default() })
        );
        let compacted = json!({"turnItems":[{"type":"compaction","afterTokenCount":42,"beforeTokenCount":900}]});
        assert_eq!(usage(&compacted), Some(Usage { used: 42.0, total: Some(900.0), compacts: true, ..Usage::default() }));
        assert_eq!(usage(&json!({})), None);
    }

    #[test]
    fn a_cold_claude_session_over_200k_offers_to_compact_before_sending() {
        let hour = 60 * 60_000;
        let at = 1_000 * hour;
        let u = |used: f64, ttl: Option<i64>| Usage { used, updated_at: Some(at), cache_ttl: ttl, ..Usage::default() };
        assert!(offers_resume("claudeAgent", &u(240_000.0, None), at + 70 * 60_000));
        assert!(!offers_resume("claudeAgent", &u(240_000.0, None), at + 69 * 60_000), "FAILED: the cache may still be warm");
        assert!(offers_resume("claudeAgent", &u(240_000.0, Some(5 * 60_000)), at + 5 * 60_000), "FAILED: the turn's own TTL wins");
        assert!(!offers_resume("claudeAgent", &u(199_999.0, None), at + 2 * hour), "FAILED: too small to matter");
        assert!(!offers_resume("codex", &u(240_000.0, None), at + 2 * hour), "FAILED: Claude only");
        assert!(!offers_resume("claudeAgent", &Usage { used: 240_000.0, ..Usage::default() }, at), "FAILED: no report time, no offer");

        assert_eq!(idle_compaction(&json!({}), "claudeAgent"), "onReturn");
        assert_eq!(idle_compaction(&json!({"compactIdleThreads": true}), "claudeAgent"), "beforeCacheExpires");
        assert_eq!(idle_compaction(&json!({"compactIdleThreads": false, "idleCompaction": "off"}), "claudeAgent"), "off");
        let own = json!({"idleCompaction": "off", "providerInstances": {"work": {"config": {"idleCompaction": "onReturn"}}}});
        assert_eq!(idle_compaction(&own, "work"), "onReturn", "FAILED: the instance's own mode wins");
        assert_eq!(idle_compaction(&own, "claudeAgent"), "off");

        let question = "This session is 2h 5m old and uses 240,000 tokens. Compact it before continuing?";
        let answered = |answer: Value| json!({"runtimeRequests": [{"kind": "user_input", "status": "resolved", "answers": {question: answer}}]});
        assert!(dismissed_resume(&answered(json!("Don't ask again"))));
        assert!(dismissed_resume(&answered(json!(["Don't ask again"]))));
        assert!(!dismissed_resume(&answered(json!("Compact and continue"))));

        assert!(!compactable(&json!({"turnItems": [{"type": "user_message", "text": " /COMPACT ", "attachments": []}]})));
        assert!(compactable(&json!({"turnItems": [{"type": "user_message", "text": "hi", "attachments": []}]})));
    }

    #[test]
    fn token_counts_read_like_the_web() {
        assert_eq!(tokens(950.0), "950");
        assert_eq!(tokens(1_000.0), "1k");
        assert_eq!(tokens(1_240.0), "1.2k");
        assert_eq!(tokens(42_400.0), "42k");
        assert_eq!(tokens(1_500_000.0), "1.5m");
        assert_eq!(percent_label(4.0), "4%");
        assert_eq!(percent_label(4.24), "4.2%");
        assert_eq!(percent_label(42.4), "42%");
        assert_eq!(cost_label(0.004, "USD"), "USD 0.0040");
        assert_eq!(cost_label(1.5, "USD"), "USD 1.50");
        assert_eq!(compaction_message(None, Some(160_000.0)), "Compacts automatically at about 160,000 tokens.");
        assert_eq!(compaction_message(Some("GPT-5"), None), "Context for GPT-5 compacts automatically when needed.");
    }
}
