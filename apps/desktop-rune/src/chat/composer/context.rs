//! The context window meter beside the send button (ContextWindowMeter, contextWindow.ts).

use crate::data::{iso_ms, s};
use crate::theme::{AlphaMul, T};
use crate::{App, Route};
use rune::{Cursor, El, LineCap, Shape, canvas, el};
use serde_json::Value;

#[derive(Debug, PartialEq)]
pub struct Usage {
    pub used: f64,
    pub max: Option<f64>,
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
        return Some(Usage { used: num(&u["usedTokens"]).unwrap_or(0.0).max(0.0), max: num(&u["maxTokens"]) });
    }
    let active = s(&p["thread"], "activeProviderThreadId");
    let provider = p["providerThreads"].as_array().into_iter().flatten().find(|t| !active.is_empty() && s(t, "id") == active);
    if let Some(u) = provider.map(|t| &t["contextUsage"]).filter(|u| u.is_object()) {
        return Some(Usage { used: num(&u["usedTokens"]).unwrap_or(0.0), max: num(&u["maxTokens"]) });
    }
    let items = p["visibleTurnItems"].as_array().map(|v| v.iter().map(|e| &e["item"]).collect::<Vec<_>>()).unwrap_or_else(|| p["turnItems"].as_array().into_iter().flatten().collect());
    items.into_iter().rev().filter(|i| s(i, "type") == "compaction").find_map(|i| num(&i["afterTokenCount"]).filter(|n| *n >= 0.0)).map(|used| Usage { used, max: None })
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

/// The ring, with its numbers in the tooltip.
pub fn meter(app: &App, t: &T) -> Option<El> {
    let Route::Thread(id) = app.frame_route() else { return None };
    let u = usage(&app.state().thread_views.get(id)?.projection)?;
    let pct = u.percent();
    let help = match (pct, u.max) {
        (Some(p), Some(max)) => format!("Context window {} · {}/{}", percent_label(p), tokens(u.used), tokens(max)),
        _ => format!("Context window {} tokens used", tokens(u.used)),
    };
    let fraction = (pct.unwrap_or(0.0) / 100.0) as f32;
    let color = if fraction > 0.9 { t.c.destructive } else { t.c.muted_foreground.alpha_mul(0.72) };
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
    let mut shape = Shape::new().view_box(24.0, 24.0).stroke_with(circle(0.0, 1.0), true, 2.5, LineCap::Round, t.c.muted_foreground.alpha_mul(0.24));
    if fraction > 0.0 {
        shape = shape.stroke_with(circle(0.0, fraction), false, 2.5, LineCap::Round, color);
    }
    Some(
        el().key("composer-context")
            .size(28.0, 28.0)
            .center()
            .radius(crate::theme::layout::CONTROL_RADIUS)
            .hover(t.c.accent)
            .cursor(Cursor::Default)
            .label(help.clone())
            .help(help)
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
        assert_eq!(usage(&live), Some(Usage { used: 900.0, max: Some(1000.0) }));
        let provider = json!({"thread":{"activeProviderThreadId":"p"},"providerThreads":[{"id":"p","contextUsage":{"usedTokens":5,"maxTokens":null}}]});
        assert_eq!(usage(&provider), Some(Usage { used: 5.0, max: None }));
        let compacted = json!({"turnItems":[{"type":"compaction","afterTokenCount":42}]});
        assert_eq!(usage(&compacted), Some(Usage { used: 42.0, max: None }));
        assert_eq!(usage(&json!({})), None);
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
    }
}
