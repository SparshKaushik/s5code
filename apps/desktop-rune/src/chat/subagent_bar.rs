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

fn option_value(options: &Value, id: &str) -> Option<Value> {
    match options {
        Value::Array(options) => options.iter().find(|o| s(o, "id") == id).map(|o| o["value"].clone()),
        Value::Object(options) => options.get(id).cloned(),
        _ => None,
    }
}

/// deriveReportedModelSelection: what the active provider thread reports it runs with.
pub fn reported_selection(projection: &Value) -> Option<&Value> {
    let thread = &projection["thread"];
    let active = s(thread, "activeProviderThreadId");
    let instance = s(&thread["modelSelection"], "instanceId");
    let provider = projection["providerThreads"].as_array()?.iter().find(|p| !active.is_empty() && s(p, "id") == active && s(p, "providerInstanceId") == instance)?;
    Some(&provider["nativeMetadata"]["modelSelection"]).filter(|m| m.is_object())
}

/// formatModelSelectionEffort: the label of the reasoning level the thread's selection states,
/// or that the provider reports for the same model.
pub fn effort_label(selection: &Value, model: Option<&Value>, reported: Option<&Value>) -> Option<String> {
    let descriptors = model?["capabilities"]["optionDescriptors"].as_array()?;
    let same = reported.filter(|r| s(r, "instanceId") == s(selection, "instanceId") && s(r, "model") == s(selection, "model"));
    ["reasoningEffort", "effort", "reasoning", "variant", "reasoning_effort", "thinking_level"].iter().find_map(|id| {
        let descriptor = descriptors.iter().find(|d| s(d, "id") == *id).filter(|d| s(d, "type") == "select")?;
        let reported = same.filter(|_| option_value(&selection["options"], id).is_none()).and_then(|r| option_value(&r["options"], id));
        let value = option_value(&selection["options"], id).or_else(|| reported.clone());
        if (value.is_none() && *id != "variant") || value.as_ref().is_some_and(Value::is_boolean) {
            return None;
        }
        let option = value.and_then(|v| descriptor["options"].as_array()?.iter().find(|o| o["id"] == v).cloned());
        let label = match option {
            Some(option) => s(&option, "label").to_owned(),
            None if reported.as_ref().is_some_and(|r| r == "default") => "Default".to_owned(),
            None if *id == "variant" => "Unknown".to_owned(),
            None => return None,
        };
        Some(label).filter(|l| !l.is_empty())
    })
}

/// resolveSelectableModel: the catalog model a slug names, as providers can report dated ids,
/// display names or aliases.
pub fn catalog_model<'a>(models: &'a [Value], slug: &str) -> Option<&'a Value> {
    let slug = slug.trim();
    let lower = slug.to_lowercase();
    let alias = |m: &Value| m["aliases"].as_array().is_some_and(|a| a.iter().any(|x| x.as_str().is_some_and(|x| x.to_lowercase() == lower)));
    if slug.is_empty() {
        return None;
    }
    models
        .iter()
        .find(|m| s(m, "slug") == slug)
        .or_else(|| models.iter().find(|m| s(m, "name").to_lowercase() == lower))
        .or_else(|| models.iter().find(|m| alias(m)))
}

/// getTriggerDisplayModelName: the short name, else the name, without its sub-provider prefix.
fn display_model_name(model: &Value) -> String {
    let name = [s(model, "shortName"), s(model, "name")].into_iter().find(|n| !n.is_empty()).unwrap_or(s(model, "slug"));
    let qualifier = s(model, "subProvider").trim();
    if qualifier.is_empty() || name.len() <= qualifier.len() || !name.is_char_boundary(qualifier.len()) || !name[..qualifier.len()].eq_ignore_ascii_case(qualifier) {
        return name.to_owned();
    }
    let rest = &name[qualifier.len()..];
    let stripped = rest.trim_start_matches(char::is_whitespace);
    let stripped = match stripped.strip_prefix(['.', ':', '/', '-']) {
        Some(after) => after.trim_start(),
        None if stripped.len() < rest.len() => stripped,
        None => return name.to_owned(),
    };
    if stripped.trim().is_empty() { name.to_owned() } else { stripped.trim().to_owned() }
}

/// formatModelSlugName: `gpt-5-codex` as "GPT-5-Codex", `claude-opus-4-5` as "Claude Opus 4.5".
pub fn slug_name(slug: &str) -> String {
    let split = slug.rfind('/').map_or(0, |i| i + 1);
    let (prefix, name) = slug.split_at(split);
    let lower = name.to_ascii_lowercase();
    let numbered = |head: &str| lower.strip_prefix(head).and_then(|r| r.strip_prefix('-')).is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()));
    if numbered("gpt") {
        let mut out = String::from("GPT");
        let mut dash = false;
        for c in name[3..].chars() {
            out.push(if dash { c.to_ascii_uppercase() } else { c });
            dash = c == '-';
        }
        return format!("{prefix}{out}");
    }
    if !["claude-opus", "claude-sonnet", "claude-haiku", "claude-fable", "gemini", "grok", "composer"].into_iter().any(numbered) {
        return slug.to_owned();
    }
    // claude-opus-4-5 reads as version 4.5 when a one- or two-digit minor follows the major.
    let mut name = name.to_owned();
    let parts: Vec<&str> = lower.split('-').collect();
    if parts[0] == "claude" && parts.len() >= 4 && parts[2].chars().all(|c| c.is_ascii_digit()) {
        let minor = parts[3].split('[').next().unwrap_or("");
        if (1..=2).contains(&minor.len()) && minor.chars().all(|c| c.is_ascii_digit()) {
            let at = parts[0].len() + parts[1].len() + parts[2].len() + 2;
            name.replace_range(at..at + 1, ".");
        }
    }
    let words: Vec<String> = name
        .split('-')
        .map(|p| {
            let mut chars = p.chars();
            chars.next().map_or(String::new(), |c| c.to_uppercase().chain(chars).collect())
        })
        .collect();
    format!("{prefix}{}", words.join(" "))
}

/// PROVIDER_DISPLAY_NAMES.
fn driver_label(driver: &str) -> Option<&'static str> {
    Some(match driver {
        "antigravity" => "Antigravity",
        "codex" => "Codex",
        "claudeAgent" => "Claude",
        "cursor" => "Cursor",
        "grok" => "Grok",
        "fx" => "fx",
        "kiro" => "Kiro",
        "muse" => "Muse Code",
        "acpRegistry" => "ACP Registry",
        "pi" => "Pi",
        "opencode" => "OpenCode",
        _ => return None,
    })
}

/// humanizeSlug: `myCodex_work` as "My Codex Work".
fn humanize(slug: &str) -> String {
    let mut spaced = String::new();
    let mut prev: Option<char> = None;
    for c in slug.chars() {
        if c.is_ascii_uppercase() && prev.is_some_and(|p| p.is_ascii_lowercase()) {
            spaced.push(' ');
        }
        spaced.push(if c == '_' || c == '-' { ' ' } else { c });
        prev = Some(c);
    }
    let words: Vec<String> = spaced.split_whitespace().map(|w| {
        let mut chars = w.chars();
        chars.next().map_or(String::new(), |c| c.to_uppercase().chain(chars).collect())
    }).collect();
    words.join(" ")
}

/// resolveProviderInstanceDisplayName.
pub fn instance_name(provider: &Value) -> String {
    let driver = s(provider, "driver");
    let named = s(provider, "displayName").trim();
    let kind = driver_label(driver).map_or_else(|| humanize(driver), str::to_owned);
    if !named.is_empty() && named != kind {
        return named.to_owned();
    }
    let instance = s(provider, "instanceId");
    if instance != driver {
        let humanized = humanize(instance);
        if !humanized.is_empty() {
            return humanized;
        }
    }
    if named.is_empty() { kind } else { named.to_owned() }
}

/// providerInstanceInitials: "Codex (hub)" is CH, "Theo's Codex" TC, "Gemini" GE.
pub fn initials(label: &str) -> String {
    let apostrophe = |c: char| c == '\'' || c == '\u{2019}';
    let punctuation = |c: char| (c.is_ascii_punctuation() && !"$+<=>^`|~".contains(c)) || "\u{2010}\u{2013}\u{2014}\u{2026}\u{201c}\u{201d}\u{2018}".contains(c);
    let cleaned: String = label.chars().map(|c| if punctuation(c) && !apostrophe(c) { ' ' } else { c }).collect();
    let words: Vec<&str> = cleaned.split_whitespace().map(|w| w.trim_matches(apostrophe)).filter(|w| !w.is_empty()).collect();
    match words.as_slice() {
        [] => String::new(),
        [word] => word.chars().take(2).collect::<String>().to_uppercase(),
        [first, second, ..] => [first, second].iter().filter_map(|w| w.chars().next()).collect::<String>().to_uppercase(),
    }
}

/// normalizeProviderAccentColor + providerAccentForegroundColor: the instance's accent and the
/// ink that reads on it.
fn accent(provider: &Value) -> Option<(Color, Color)> {
    let hex = s(provider, "accentColor").trim();
    let digits = hex.strip_prefix('#').filter(|d| d.len() == 6 && d.chars().all(|c| c.is_ascii_hexdigit()))?;
    let channel = |at: usize| {
        let v = u8::from_str_radix(&digits[at..at + 2], 16).unwrap_or(0) as f64 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    let luminance = 0.2126 * channel(0) + 0.7152 * channel(2) + 0.0722 * channel(4);
    let ink = if (luminance + 0.05) / 0.05 >= 1.05 / (luminance + 0.05) { Color::BLACK } else { Color::WHITE };
    Some((rune::color::hex(hex)?, ink))
}

/// ProviderInstanceIcon at 16px, with the instance's initials badge when it needs telling apart.
fn instance_icon(t: &T, provider: &Value, badge: bool) -> El {
    let mark = composer::provider_icon(t, s(provider, "driver"), 16.0);
    let mut icon = el().size(16.0, 16.0).shrink().child(mark);
    if badge {
        let (fill, ink) = accent(provider).unwrap_or((t.c.card, t.c.muted_foreground));
        icon = icon.child(
            row()
                .h(12.0)
                .min_w(12.0)
                .px(2.0)
                .center()
                .radius(radius::FULL)
                .fill(fill)
                .border(1.0, t.c.card)
                .shadow(1.0, 2.0, Color::BLACK.alpha(0.05))
                .pin(None, None, Some(-2.0), Some(-2.0))
                .a11y_hidden()
                .child(text(initials(&instance_name(provider))).font(theme::weight(theme::TEXT_5XS, theme::SEMIBOLD)).color(ink)),
        );
    }
    icon
}

/// The bar in the composer's place, for a provider-native subagent thread.
pub fn view(app: &App, cx: &mut Cx<App>, t: &T, lane: &Lane) -> Option<El> {
    let thread = composer::ctx(app).thread.filter(|thread| native(thread))?;
    let width = lane.width;
    let selection = &thread["modelSelection"];
    let instance = s(selection, "instanceId");
    let provider = composer::provider(app, instance);
    let slug = s(selection, "model");
    let models = provider.and_then(|p| p["models"].as_array()).map_or(&[][..], Vec::as_slice);
    let model_label = catalog_model(models, slug).map_or_else(|| slug_name(slug), display_model_name);
    let projection = app.state().thread_views.get(s(thread, "id")).map(|v| &v.projection);
    let effort = effort_label(selection, models.iter().find(|m| s(m, "slug") == slug), projection.and_then(reported_selection));
    let badge = provider.is_some_and(|p| accent(p).is_some() || crate::shell::sidebar::logic::shows_instance_badge(instance, composer::providers(app), &Value::Null));
    let status = status_label(projection.and_then(status_node), app.now_ms);
    let description = match &effort {
        Some(effort) => format!("{model_label}, {effort}"),
        None => model_label.clone(),
    };
    let mut name = row().gap(8.0).center_y().shrink().min_w(0.0);
    if let Some(provider) = provider {
        name = name.child(instance_icon(t, provider, badge));
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
    use super::{catalog_model, display_model_name, duration, effort_label, initials, instance_name, native, reported_selection, slug_name, status_label};
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
        assert_eq!(effort_label(&json!({"options": [{"id": "reasoningEffort", "value": "high"}]}), Some(&model), None).as_deref(), Some("High"));
        assert_eq!(effort_label(&json!({"options": {"reasoningEffort": "low"}}), Some(&model), None).as_deref(), Some("Low"));
        assert_eq!(effort_label(&json!({}), Some(&model), None), None);
        assert_eq!(effort_label(&json!({"options": {"reasoningEffort": "low"}}), None, None), None);
        let selection = json!({"instanceId": "codex", "model": "gpt-5"});
        let reported = json!({"instanceId": "codex", "model": "gpt-5", "options": [{"id": "reasoningEffort", "value": "low"}]});
        assert_eq!(effort_label(&selection, Some(&model), Some(&reported)).as_deref(), Some("Low"), "the provider's reported level fills in");
        let other = json!({"instanceId": "codex", "model": "gpt-4", "options": [{"id": "reasoningEffort", "value": "low"}]});
        assert_eq!(effort_label(&selection, Some(&model), Some(&other)), None, "a report for another model is ignored");
    }

    #[test]
    fn the_reported_selection_comes_from_the_active_provider_thread() {
        let projection = json!({"thread": {"activeProviderThreadId": "pt", "modelSelection": {"instanceId": "codex"}}, "providerThreads": [
            {"id": "old", "providerInstanceId": "codex", "nativeMetadata": {"modelSelection": {"model": "gpt-4"}}},
            {"id": "pt", "providerInstanceId": "codex", "nativeMetadata": {"modelSelection": {"model": "gpt-5"}}}]});
        assert_eq!(reported_selection(&projection).map(|m| m["model"].clone()), Some(json!("gpt-5")));
        assert_eq!(reported_selection(&json!({"thread": {}, "providerThreads": []})), None);
    }

    #[test]
    fn model_names_resolve_like_the_web_trigger() {
        let models = [json!({"slug": "gemini-3-pro", "name": "Gemini 3 Pro", "aliases": ["gemini-3-pro-preview-11-2025"]}),
            json!({"slug": "openrouter/kimi", "name": "OpenRouter: Kimi K2", "subProvider": "OpenRouter"})];
        let label = |slug: &str| catalog_model(&models, slug).map_or_else(|| slug_name(slug), display_model_name);
        assert_eq!(label("gemini-3-pro"), "Gemini 3 Pro");
        assert_eq!(label("Gemini-3-Pro-Preview-11-2025"), "Gemini 3 Pro", "a dated alias names its catalog model");
        assert_eq!(label("gemini 3 pro"), "Gemini 3 Pro");
        assert_eq!(label("openrouter/kimi"), "Kimi K2");
        assert_eq!(label("claude-opus-4-5"), "Claude Opus 4.5");
        assert_eq!(label("claude-sonnet-4-5-20250929"), "Claude Sonnet 4.5 20250929");
        assert_eq!(label("gpt-5.1-codex-max"), "GPT-5.1-Codex-Max");
        assert_eq!(label("acme/custom-model"), "acme/custom-model");
    }

    #[test]
    fn instance_badges_read_the_instance_initials() {
        assert_eq!(initials("Codex (hub)"), "CH");
        assert_eq!(initials("Theo's Codex"), "TC");
        assert_eq!(initials("Codex 'hub'"), "CH");
        assert_eq!(initials("Gemini"), "GE");
        assert_eq!(instance_name(&json!({"instanceId": "codex", "driver": "codex", "displayName": "Codex"})), "Codex");
        assert_eq!(instance_name(&json!({"instanceId": "codex_work", "driver": "codex", "displayName": "Codex"})), "Codex Work");
        assert_eq!(instance_name(&json!({"instanceId": "codex_work", "driver": "codex", "displayName": "Studio"})), "Studio");
    }
}
