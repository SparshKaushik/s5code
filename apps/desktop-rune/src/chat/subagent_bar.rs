//! ProviderSubagentBar: a provider-native subagent cannot take messages, so its thread shows this
//! bar in the composer's place: the model, what the subagent is doing, and the way to its parent.

use rune::{Color, Cx, El, col, el, row, text};
use serde_json::Value;

use crate::chat::composer;
use crate::chat::details::{Lane, child_work};
use crate::data::{s, ts};
use crate::theme::{self, T, radius};
use crate::{App, icons, kit};

/// isProviderNativeSubagentThread.
pub fn native(thread: &Value) -> bool {
    s(&thread["lineage"], "relationshipToParent") == "subagent" && s(thread, "creationSource") == "provider"
}

/// deriveProviderSubagentStatus: the latest root turn outside any run.
fn status_node(projection: &Value) -> Option<&Value> {
    projection["nodes"].as_array()?.iter().rev().find(|n| s(n, "kind") == "root_turn" && n["runId"].is_null())
}

/// formatDuration, for whole seconds.
fn duration(seconds: i64) -> String {
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let parts = [(seconds / 3600, "h"), (seconds % 3600 / 60, "m"), (seconds % 60, "s")];
    parts.iter().filter(|(n, _)| *n > 0).map(|(n, unit)| format!("{n}{unit}")).collect::<Vec<_>>().join(" ")
}

/// formatProviderSubagentStatus: "Working 12s", "Completed in 3m 4s", or the status alone.
pub fn status_label(node: Option<&Value>, now: i64) -> String {
    let Some(node) = node else { return "Starting".into() };
    let status = s(node, "status");
    let label = match status {
        "pending" | "running" => "Working",
        "waiting" => "Waiting",
        "completed" => "Completed",
        "interrupted" => "Interrupted",
        "failed" => "Failed",
        "cancelled" | "rolled_back" => "Cancelled",
        _ => "Idle",
    };
    let live = matches!(status, "pending" | "running" | "waiting");
    if !live && status != "completed" {
        return label.into();
    }
    let end = if live { Some(now) } else { ts(node, "completedAt") };
    match ts(node, "startedAt").zip(end) {
        Some((start, end)) => {
            let elapsed = duration(((end - start) / 1000).max(1));
            if live { format!("{label} {elapsed}") } else { format!("{label} in {elapsed}") }
        }
        None => label.into(),
    }
}

/// formatModelSelectionEffort: the label of the reasoning level the thread's selection states.
pub fn effort_label(selection: &Value, model: Option<&Value>) -> Option<String> {
    let descriptors = model?["capabilities"]["optionDescriptors"].as_array()?;
    let stated = |id: &str| match &selection["options"] {
        Value::Array(options) => options.iter().find(|o| s(o, "id") == id).map(|o| o["value"].clone()),
        Value::Object(options) => options.get(id).cloned(),
        _ => None,
    };
    ["reasoningEffort", "effort", "reasoning", "variant", "reasoning_effort", "thinking_level"].iter().find_map(|id| {
        let descriptor = descriptors.iter().find(|d| s(d, "id") == *id && s(d, "type") == "select")?;
        let value = stated(id).filter(|v| v.is_string())?;
        let option = descriptor["options"].as_array()?.iter().find(|o| o["id"] == value)?;
        Some(s(option, "label").to_owned()).filter(|l| !l.is_empty())
    })
}

/// The bar in the composer's place, for a provider-native subagent thread.
pub fn view(app: &App, cx: &mut Cx<App>, t: &T, lane: &Lane) -> Option<El> {
    let thread = composer::ctx(app).thread.filter(|thread| native(thread))?;
    let width = lane.width;
    let selection = &thread["modelSelection"];
    let provider = composer::provider(app, s(selection, "instanceId"));
    let slug = s(selection, "model");
    let model = provider.and_then(|p| p["models"].as_array()?.iter().find(|m| s(m, "slug") == slug));
    let model_label = model
        .and_then(|m| [s(m, "shortName"), s(m, "name")].into_iter().find(|n| !n.is_empty()))
        .unwrap_or(slug)
        .to_owned();
    let effort = effort_label(selection, model);
    let projection = app.state().thread_views.get(s(thread, "id")).map(|v| &v.projection);
    let status = status_label(projection.and_then(status_node), app.now_ms);
    let description = match &effort {
        Some(effort) => format!("{model_label}, {effort}"),
        None => model_label.clone(),
    };
    let mut name = row().gap(8.0).center_y().shrink().min_w(0.0);
    if let Some(provider) = provider {
        name = name.child(composer::provider_icon(t, s(provider, "driver"), 16.0));
    }
    name = name
        .child(text(model_label).font(theme::SM_MEDIUM).color(t.c.foreground).truncate().shrink().min_w(0.0))
        .children(effort.map(|effort| text(effort).font(theme::TEXT_SM).color(t.c.muted_foreground)));
    // Narrow composers drop the status to its own line.
    let stacked = width <= 560.0;
    let status_text = text(status.clone()).font(if stacked { theme::TEXT_XS } else { theme::TEXT_SM }).color(t.c.muted_foreground).truncate().min_w(0.0);
    let summary = if stacked { col().gap(2.0) } else { row().gap(12.0).center_y() };
    let summary = summary.grow(1.0).min_w(0.0).child(name).child(status_text.a11y_hidden());
    let parent = Some(s(&thread["lineage"], "parentThreadId")).filter(|p| !p.is_empty()).map(str::to_owned);
    let open_parent = parent.map(|parent| {
        kit::Button::new("Open parent")
            .icon(&icons::ARROW_UP_LEFT)
            .size(if width <= 320.0 { "icon-sm" } else { "sm" })
            .variant("ghost")
            .on_press(move |app: &mut App| crate::shell::sidebar::open_thread(app, &parent))
            .build(t)
    });
    let bar = row()
        .w_full()
        .min_h(48.0)
        .gap(12.0)
        .py(8.0)
        .pl(20.0)
        .pr(8.0)
        .center_y()
        .child(summary)
        .child(el().role("status").label(format!("{description} subagent: {status}")))
        .children((width > 640.0).then(|| text("Runs on its own").font(theme::TEXT_SM).color(t.c.muted_foreground)))
        .children(child_work::button(app, t, thread))
        .children(open_parent);
    let (fill, outline) = if t.dark { (t.c.surface_raised, Color::WHITE.alpha(0.05)) } else { (t.c.card, Color::BLACK.alpha(0.08)) };
    let mut surface = col().key("composer").w_full().radius(radius::XL2).fill(fill).border(1.0, outline).padding(1.0).child(bar);
    if !t.dark {
        surface = surface.shadow(1.0, 2.0, Color::BLACK.alpha(0.05));
    }
    Some(col().key("composer-lane").w(width).pin(Some(lane.left), None, None, Some(composer::bottom_inset(app, cx))).child(surface))
}

#[cfg(test)]
mod tests {
    use super::{duration, effort_label, native, status_label};
    use serde_json::json;

    #[test]
    fn only_provider_created_subagents_get_the_bar() {
        assert!(native(&json!({"lineage": {"relationshipToParent": "subagent"}, "creationSource": "provider"})));
        assert!(!native(&json!({"lineage": {"relationshipToParent": "subagent"}, "creationSource": "user"})));
        assert!(!native(&json!({"lineage": {"relationshipToParent": "sideChat"}, "creationSource": "provider"})));
    }

    #[test]
    fn status_reads_like_format_provider_subagent_status() {
        let at = |s: &str| format!("2026-01-01T00:{s}.000Z");
        let node = |status: &str| json!({"status": status, "startedAt": at("00:00"), "completedAt": at("03:04")});
        let now = crate::data::iso_ms(&at("00:12")).unwrap_or(0);
        assert_eq!(status_label(None, now), "Starting");
        assert_eq!(status_label(Some(&node("running")), now), "Working 12s");
        assert_eq!(status_label(Some(&node("completed")), now), "Completed in 3m 4s");
        assert_eq!(status_label(Some(&node("failed")), now), "Failed");
        assert_eq!(status_label(Some(&json!({"status": "waiting"})), now), "Waiting");
        assert_eq!(duration(3_725), "1h 2m 5s");
        assert_eq!(duration(3_600), "1h");
    }

    #[test]
    fn effort_names_the_stated_reasoning_level() {
        let model = json!({"capabilities": {"optionDescriptors": [
            {"id": "fastMode", "type": "boolean"},
            {"id": "reasoningEffort", "type": "select", "options": [{"id": "high", "label": "High"}, {"id": "low", "label": "Low"}]}]}});
        assert_eq!(effort_label(&json!({"options": [{"id": "reasoningEffort", "value": "high"}]}), Some(&model)).as_deref(), Some("High"));
        assert_eq!(effort_label(&json!({"options": {"reasoningEffort": "low"}}), Some(&model)).as_deref(), Some("Low"));
        assert_eq!(effort_label(&json!({}), Some(&model)), None);
        assert_eq!(effort_label(&json!({"options": {"reasoningEffort": "low"}}), None), None);
    }
}
