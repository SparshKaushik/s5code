//! What the timeline's notices say (session-logic providerErrorPresentation, work-log
//! presentation contextCompactionLabel, handoff.ts, userMessage.ts): pure text from projection
//! items, so the views stay layout.

use super::rows::array;
use crate::data::s;
use serde_json::Value;

/// providerErrorPresentation: a provider error's label and the message under it.
pub fn provider_error(item: &Value) -> (String, String) {
    let failure = &item["failure"];
    let usage = s(failure, "class") == "usage_limit";
    let message = s(failure, "message").to_owned();
    let retry = &item["retry"];
    if !retry.is_object() {
        let title = s(item, "title").trim();
        let label = if usage {
            "Usage limit reached"
        } else if title.is_empty() {
            "Provider error"
        } else {
            title
        };
        return (label.to_owned(), message);
    }
    let attempt = retry["attempt"].as_u64().unwrap_or(0);
    let progress = match retry["maxAttempts"].as_u64() {
        Some(max) => format!("{attempt}/{max}"),
        None => attempt.to_string(),
    };
    let status = s(item, "status");
    let label = match status {
        "running" => format!("Retrying provider ({progress})"),
        "completed" => format!("Provider recovered ({progress} retries)"),
        "failed" => format!("{} after {progress} retries", if usage { "Usage limit reached" } else { "Provider error" }),
        _ => format!("Provider retry stopped ({progress})"),
    };
    let delay = retry["retryDelayMs"]
        .as_f64()
        .filter(|d| status == "running" && *d > 0.0)
        .map(|d| {
            if d < 1000.0 {
                format!(" Retrying in {d}ms.")
            } else {
                let secs = format!("{:.1}", d / 1000.0);
                format!(" Retrying in {}s.", secs.strip_suffix(".0").unwrap_or(&secs))
            }
        })
        .unwrap_or_default();
    (label, format!("{message}{delay}"))
}

fn trim_zeros(text: String) -> String {
    if let Some((int, frac)) = text.split_once('.')
        && frac.bytes().all(|b| b == b'0')
    {
        return int.to_owned();
    }
    text
}

/// formatTokens: 950, 1.50K, 12.3K, 120K, 2M.
pub fn format_tokens(value: f64) -> String {
    let abs = value.abs();
    for (n, unit) in [(1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K")] {
        if abs >= n {
            let v = value / n;
            let digits: usize = if v.abs() >= 100.0 { 0 } else if v.abs() >= 10.0 { 1 } else { 2 };
            return format!("{}{unit}", trim_zeros(format!("{v:.digits$}")));
        }
    }
    format!("{}", value.round() as i64)
}

/// A compaction row's words: plain, or led into its token counts around an arrow.
#[derive(Debug, PartialEq)]
pub enum CompactionText {
    Plain(String),
    Tokens { lead: String, before: String, after: String },
}

/// IDLE_COMPACTING_LABELS and IDLE_COMPACTION_TITLES, by the compaction's idle reason.
fn idle_labels(reason: &str) -> Option<(&'static str, &'static str)> {
    match reason {
        "beforeCacheExpires" => Some(("Compacting before the cache expires…", "Context automatically compacted before cache expiry")),
        "onReturn" => Some(("Compacting…", "Context compacted before sending")),
        _ => None,
    }
}

/// contextCompactionLabel as ContextCompactionTimelineRow shows it, with the row's `title` (from
/// [`compaction_context`]) leading a finished compaction and `idle` naming a running one.
pub fn compaction_text(item: &Value, title: Option<&str>, idle: Option<&str>) -> CompactionText {
    let status = s(item, "status");
    if status != "running"
        && let (Some(before), Some(after)) = (item["beforeTokenCount"].as_f64(), item["afterTokenCount"].as_f64())
    {
        return CompactionText::Tokens {
            lead: title.unwrap_or("Context compacted").to_owned(),
            before: format_tokens(before),
            after: format_tokens(after),
        };
    }
    let label = match status {
        "failed" => "Context compaction failed",
        "cancelled" | "interrupted" => "Context compaction stopped",
        "pending" | "running" | "waiting" => idle.and_then(idle_labels).map_or("Compacting context", |(running, _)| running),
        _ => "Context compacted",
    };
    match title.and_then(|title| label.strip_prefix("Context compacted").map(|rest| format!("{title}{rest}"))) {
        Some(text) => CompactionText::Plain(text),
        None => CompactionText::Plain(label.to_owned()),
    }
}

/// isCompactionCommandMessage: a bare `/compact` prompt.
fn compaction_command(item: &Value) -> bool {
    s(item, "type") == "user_message" && s(item, "text").trim().eq_ignore_ascii_case("/compact") && array(item, "attachments").is_empty()
}

/// claudeResumeCompactionAge: the session age in an answered "Compact it before continuing?"
/// question whose answer was to compact.
pub fn resume_age(answer: &Value) -> Option<String> {
    let texts = &answer["questionTextById"];
    answer["answers"].as_object()?.iter().find_map(|(id, chosen)| {
        let question = texts[id.as_str()].as_str().unwrap_or(id.as_str());
        let rest = question.strip_prefix("This session is ")?;
        let (age, rest) = rest.split_once(" old and uses ")?;
        let count = rest.strip_suffix(" tokens. Compact it before continuing?")?;
        let age_ok = match age.split_once(' ') {
            Some((h, m)) => digits_then(h, 'h') && digits_then(m, 'm'),
            None => digits_then(age, 'm'),
        };
        let count_ok = !count.is_empty() && count.bytes().all(|b| b.is_ascii_digit() || b == b',');
        let compact = match chosen {
            Value::Array(values) => values.iter().any(|v| v == "Compact and continue"),
            other => other == "Compact and continue",
        };
        (age_ok && count_ok && compact).then(|| age.to_owned())
    })
}

fn digits_then(part: &str, unit: char) -> bool {
    part.strip_suffix(unit).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// compactionTitle plus the run's idle reason: an idle `/compact` names its compaction, else the
/// session age the user agreed to compact after in that run does.
pub fn compaction_context<'a>(items: &[&'a Value], compaction: &Value) -> (Option<String>, Option<&'a str>) {
    let run = s(compaction, "runId");
    if run.is_empty() {
        return (None, None);
    }
    let in_run = || items.iter().copied().filter(move |i| s(i, "runId") == run);
    let reason = in_run().find(|i| compaction_command(i)).and_then(|m| {
        let reason = s(m, "idleCompaction");
        if !reason.is_empty() {
            Some(reason)
        } else if s(m, "createdBy") == "agent" && s(m, "creationSource") == "server" {
            Some("beforeCacheExpires")
        } else {
            None
        }
    });
    if let Some((_, title)) = reason.and_then(idle_labels) {
        return (Some(title.to_owned()), reason);
    }
    let age = in_run().filter(|i| s(i, "type") == "user_input_request").find_map(|i| resume_age(&i["questionAnswer"]));
    (age.map(|age| format!("Context compacted after {age} idle")), reason)
}

/// The projection's items in timeline order, as rows::derive reads them.
pub fn items(p: &Value) -> Vec<&Value> {
    if p["visibleTurnItems"].is_array() {
        array(p, "visibleTurnItems").iter().map(|v| &v["item"]).collect()
    } else {
        array(p, "turnItems").iter().collect()
    }
}

/// resolveHandoffEndpoints: the (instance, model) pairs a handoff left, and the one it reached;
/// a missing model is "".
pub fn handoff_endpoints(item: &Value, runs: &[Value]) -> (Vec<(String, String)>, (String, String)) {
    let run_id = s(item, "runId");
    let run = runs.iter().find(|r| !run_id.is_empty() && s(r, "id") == run_id);
    let to = s(item, "toProviderInstanceId");
    let to_model = match item["toModel"].as_str() {
        Some(model) => model,
        None => run.filter(|r| s(r, "providerInstanceId") == to).map_or("", |r| s(&r["modelSelection"], "model")),
    };
    let ordinal = |r: &Value| r["ordinal"].as_f64().unwrap_or(0.0);
    let explicit = array(item, "fromModelSelections");
    let from = if !explicit.is_empty() {
        explicit.iter().map(|sel| (s(sel, "instanceId").to_owned(), s(sel, "model").to_owned())).collect()
    } else {
        let before = run.map(ordinal);
        array(item, "fromProviderInstanceIds")
            .iter()
            .filter_map(Value::as_str)
            .map(|id| {
                let mut latest: Option<&Value> = None;
                for r in runs.iter().filter(|r| s(r, "providerInstanceId") == id && before.is_none_or(|b| ordinal(*r) < b)) {
                    if latest.is_none_or(|l| ordinal(r) > ordinal(l)) {
                        latest = Some(r);
                    }
                }
                (id.to_owned(), latest.map_or("", |r| s(&r["modelSelection"], "model")).to_owned())
            })
            .collect()
    };
    (from, (to.to_owned(), to_model.to_owned()))
}

/// getTriggerDisplayModelName, as far as the catalog says: its short name, else its name.
fn model_display(model: &Value) -> String {
    ["shortName", "name", "slug"].iter().map(|k| s(model, k)).find(|v| !v.is_empty()).unwrap_or("").to_owned()
}

fn catalog_model<'a>(providers: &'a [Value], instance: &str, slug: &str) -> Option<&'a Value> {
    providers.iter().find(|p| s(p, "instanceId") == instance).and_then(|p| array(p, "models").iter().find(|m| s(m, "slug") == slug))
}

/// HandoffEndpoint's label: the model's display name, the raw model, else the provider's name.
pub fn endpoint_label(providers: &[Value], instance: &str, model: &str) -> String {
    let model = model.trim();
    if !model.is_empty() {
        return catalog_model(providers, instance, model).map_or_else(|| model.to_owned(), model_display);
    }
    providers.iter().find(|p| s(p, "instanceId") == instance).map(|p| s(p, "displayName")).filter(|n| !n.is_empty()).unwrap_or(instance).to_owned()
}

const EFFORT_IDS: [&str; 7] = ["reasoningEffort", "effort", "reasoning", "variant", "reasoning_effort", "thinking_level", "thinking"];

fn option_value<'a>(selection: &'a Value, id: &str) -> Option<&'a str> {
    match &selection["options"] {
        Value::Array(options) => options.iter().find(|o| s(o, "id") == id).and_then(|o| o["value"].as_str()),
        Value::Object(options) => options.get(id).and_then(Value::as_str),
        _ => None,
    }
}

/// resolveEffort: the selection's reasoning value and its label, from the model's reasoning select
/// (its stated value, else its current or default option), or the stated value off-catalog.
fn effort(selection: &Value, providers: &[Value]) -> Option<(String, String)> {
    let Some(model) = catalog_model(providers, s(selection, "instanceId"), s(selection, "model")) else {
        let value = EFFORT_IDS.iter().find_map(|id| option_value(selection, id))?;
        return Some((value.to_owned(), value.to_owned()));
    };
    let descriptor = array(&model["capabilities"], "optionDescriptors")
        .iter()
        .find(|d| s(d, "type") == "select" && (s(d, "role") == "reasoning" || EFFORT_IDS.contains(&s(d, "id"))))?;
    let id = s(descriptor, "id");
    let value = match option_value(selection, id) {
        Some(value) => value,
        None if id == "variant" => return None,
        None => Some(s(descriptor, "currentValue"))
            .filter(|v| !v.is_empty())
            .or_else(|| array(descriptor, "options").iter().find(|o| o["isDefault"] == true).map(|o| s(o, "id")))?,
    };
    let label = array(descriptor, "options").iter().find(|o| s(o, "id") == value).map(|o| s(o, "label")).filter(|l| !l.is_empty()).unwrap_or(value);
    Some((value.to_owned(), label.to_owned()))
}

/// ModelSwitchRow's label, "{old} → {new}" naming only what changed; None when nothing did.
pub fn model_switch_label(switch: &Value, providers: &[Value]) -> Option<String> {
    let (from, to) = (&switch["from"], &switch["to"]);
    if !from.is_object() || !to.is_object() {
        return None;
    }
    let changed = s(from, "model") != s(to, "model");
    let efforts = match (effort(from, providers), effort(to, providers)) {
        (Some(a), Some(b)) if a.0 != b.0 => Some((a.1, b.1)),
        _ => None,
    };
    if !changed && efforts.is_none() {
        return None;
    }
    let describe = |selection: &Value, effort: Option<&String>| {
        let name = changed.then(|| catalog_model(providers, s(selection, "instanceId"), s(selection, "model")).map_or_else(|| s(selection, "model").to_owned(), model_display));
        [name, effort.cloned()].into_iter().flatten().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
    };
    Some(format!("{} → {}", describe(from, efforts.as_ref().map(|e| &e.0)), describe(to, efforts.as_ref().map(|e| &e.1))))
}

/// Who a prompt reads as from (resolveUserMessagePresentation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attribution {
    Own,
    Guest,
    Automation,
    Agent,
    T3Code,
}

/// A prompt's attribution, the text people see, and the scheduled task that sent it.
pub fn presentation(item: &Value) -> (Attribution, &str, Option<&str>) {
    let text = s(item, "text");
    if item["context"]["guest"].is_object() {
        return (Attribution::Guest, text, None);
    }
    let task = s(item, "scheduledTaskId");
    if !task.is_empty() {
        return (Attribution::Automation, text, Some(task));
    }
    if let Some(rest) = legacy_automation(text) {
        let id = [s(item, "messageId"), s(item, "id")].into_iter().find(|v| !v.is_empty()).unwrap_or("");
        let task = legacy_task(id);
        if task.is_some() || s(item, "createdBy") == "agent" {
            return (Attribution::Automation, rest, task);
        }
    }
    let attribution = match (s(item, "createdBy"), s(item, "creationSource")) {
        ("agent", "server") => Attribution::T3Code,
        ("agent", _) => Attribution::Agent,
        _ => Attribution::Own,
    };
    (attribution, text, None)
}

/// LEGACY_AUTOMATION_PREFIX: `[Triggered by schedule task: …]` and a blank line, stripped.
fn legacy_automation(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("[Triggered by schedule task: ")?;
    let end = rest.find(|c: char| c == '\r' || c == '\n')?;
    let (head, tail) = rest.split_at(end);
    if head.len() < 2 || !head.ends_with(']') {
        return None;
    }
    let newline = |t: &str| -> Option<usize> { if t.starts_with("\r\n") { Some(2) } else if t.starts_with('\n') { Some(1) } else { None } };
    let tail = &tail[newline(tail)?..];
    Some(&tail[newline(tail)?..])
}

/// LEGACY_AUTOMATION_MESSAGE_ID: `scheduled-task-message:{task}:{n}:{scheduled|manual}`.
fn legacy_task(id: &str) -> Option<&str> {
    let rest = id.strip_prefix("scheduled-task-message:")?;
    let rest = rest.strip_suffix(":scheduled").or_else(|| rest.strip_suffix(":manual"))?;
    let (task, n) = rest.rsplit_once(':')?;
    (!task.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then_some(task)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_errors_name_their_retries() {
        let error = |status: &str, retry: Value, class: &str| {
            json!({"type": "error", "status": status, "failure": {"class": class, "message": "Overloaded."}, "retry": retry})
        };
        assert_eq!(provider_error(&error("failed", Value::Null, "provider_error")), ("Provider error".to_owned(), "Overloaded.".to_owned()));
        assert_eq!(provider_error(&error("failed", Value::Null, "usage_limit")).0, "Usage limit reached");
        let titled = json!({"type": "error", "status": "failed", "title": " Claude crashed ", "failure": {"message": "x"}});
        assert_eq!(provider_error(&titled).0, "Claude crashed");
        let retry = json!({"attempt": 2, "maxAttempts": 5, "retryDelayMs": 1500});
        assert_eq!(provider_error(&error("running", retry.clone(), "provider_error")), ("Retrying provider (2/5)".to_owned(), "Overloaded. Retrying in 1.5s.".to_owned()));
        assert_eq!(provider_error(&error("completed", retry.clone(), "provider_error")), ("Provider recovered (2/5 retries)".to_owned(), "Overloaded.".to_owned()));
        assert_eq!(provider_error(&error("failed", retry.clone(), "usage_limit")).0, "Usage limit reached after 2/5 retries");
        assert_eq!(provider_error(&error("cancelled", json!({"attempt": 3, "maxAttempts": null}), "provider_error")).0, "Provider retry stopped (3)");
        let quick = json!({"attempt": 1, "maxAttempts": 5, "retryDelayMs": 500});
        assert_eq!(provider_error(&error("running", quick, "provider_error")).1, "Overloaded. Retrying in 500ms.");
        let whole = json!({"attempt": 1, "maxAttempts": 5, "retryDelayMs": 2000});
        assert_eq!(provider_error(&error("running", whole, "provider_error")).1, "Overloaded. Retrying in 2s.");
    }

    #[test]
    fn compactions_read_their_counts_and_idle_titles() {
        assert_eq!(format_tokens(950.0), "950");
        assert_eq!(format_tokens(1500.0), "1.50K");
        assert_eq!(format_tokens(12_300.0), "12.3K");
        assert_eq!(format_tokens(120_000.0), "120K");
        assert_eq!(format_tokens(2_000_000.0), "2M");
        let done = json!({"type": "compaction", "status": "completed", "runId": "r", "beforeTokenCount": 182_000, "afterTokenCount": 24_000});
        assert_eq!(
            compaction_text(&done, None, None),
            CompactionText::Tokens { lead: "Context compacted".into(), before: "182K".into(), after: "24K".into() }
        );
        let bare = json!({"type": "compaction", "status": "completed", "runId": "r"});
        assert_eq!(compaction_text(&bare, Some("Context compacted after 3h 5m idle"), None), CompactionText::Plain("Context compacted after 3h 5m idle".into()));
        assert_eq!(compaction_text(&json!({"status": "running"}), None, Some("onReturn")), CompactionText::Plain("Compacting…".into()));
        assert_eq!(compaction_text(&json!({"status": "interrupted"}), None, None), CompactionText::Plain("Context compaction stopped".into()));

        let command = json!({"type": "user_message", "runId": "r", "text": "/compact", "attachments": [], "idleCompaction": "beforeCacheExpires"});
        let items = [&command, &done];
        assert_eq!(compaction_context(&items, &done), (Some("Context automatically compacted before cache expiry".to_owned()), Some("beforeCacheExpires")));
        let question = json!({"type": "user_input_request", "runId": "r", "questionAnswer": {
            "questionTextById": {"q": "This session is 3h 5m old and uses 182,000 tokens. Compact it before continuing?"},
            "answers": {"q": ["Compact and continue"]}}});
        let items = [&question, &done];
        assert_eq!(compaction_context(&items, &done).0.as_deref(), Some("Context compacted after 3h 5m idle"));
        let declined = json!({"answers": {"This session is 45m old and uses 9 tokens. Compact it before continuing?": "Don't ask again"}});
        assert_eq!(resume_age(&declined), None);
    }

    #[test]
    fn handoffs_and_model_switches_name_models() {
        let providers = vec![json!({"instanceId": "codex", "displayName": "Codex", "models": [
            {"slug": "gpt-5", "name": "GPT-5", "capabilities": {"optionDescriptors": [{"id": "reasoningEffort", "type": "select",
                "options": [{"id": "medium", "label": "Medium", "isDefault": true}, {"id": "high", "label": "High"}]}]}},
            {"slug": "gpt-5-mini", "name": "GPT-5 Mini"}]})];
        let runs = vec![
            json!({"id": "a", "ordinal": 1, "providerInstanceId": "claude", "modelSelection": {"model": "opus"}}),
            json!({"id": "b", "ordinal": 2, "providerInstanceId": "codex", "modelSelection": {"model": "gpt-5"}}),
        ];
        let handoff = json!({"runId": "b", "fromProviderInstanceIds": ["claude"], "toProviderInstanceId": "codex", "toModel": null});
        assert_eq!(handoff_endpoints(&handoff, &runs), (vec![("claude".to_owned(), "opus".to_owned())], ("codex".to_owned(), "gpt-5".to_owned())));
        assert_eq!(endpoint_label(&providers, "codex", "gpt-5"), "GPT-5");
        assert_eq!(endpoint_label(&providers, "claude", "opus"), "opus");
        assert_eq!(endpoint_label(&providers, "codex", ""), "Codex");

        let switch = |from: Value, to: Value| json!({"from": from, "to": to});
        let model = |m: &str| json!({"instanceId": "codex", "model": m});
        assert_eq!(model_switch_label(&switch(model("gpt-5"), model("gpt-5-mini")), &providers).as_deref(), Some("GPT-5 → GPT-5 Mini"));
        let high = json!({"instanceId": "codex", "model": "gpt-5", "options": [{"id": "reasoningEffort", "value": "high"}]});
        assert_eq!(model_switch_label(&switch(model("gpt-5"), high), &providers).as_deref(), Some("Medium → High"));
        assert_eq!(model_switch_label(&switch(model("gpt-5"), model("gpt-5")), &providers), None);
    }

    #[test]
    fn prompts_read_as_from_automations_agents_or_you() {
        assert_eq!(presentation(&json!({"text": "hi"})).0, Attribution::Own);
        assert_eq!(presentation(&json!({"text": "run", "scheduledTaskId": "t1"})), (Attribution::Automation, "run", Some("t1")));
        let legacy = json!({"text": "[Triggered by schedule task: Nightly]\n\nCheck CI", "messageId": "scheduled-task-message:t9:3:manual"});
        assert_eq!(presentation(&legacy), (Attribution::Automation, "Check CI", Some("t9")));
        let unproven = json!({"text": "[Triggered by schedule task: Nightly]\n\nCheck CI"});
        assert_eq!(presentation(&unproven).0, Attribution::Own);
        assert_eq!(presentation(&json!({"text": "x", "createdBy": "agent", "creationSource": "server"})).0, Attribution::T3Code);
        assert_eq!(presentation(&json!({"text": "x", "createdBy": "agent", "creationSource": "mcp"})).0, Attribution::Agent);
        assert_eq!(presentation(&json!({"text": "x", "createdBy": "agent", "context": {"guest": {"text": "hey"}}})).0, Attribution::Guest);
    }
}
