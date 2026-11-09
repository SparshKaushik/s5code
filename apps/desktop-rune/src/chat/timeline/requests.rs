use super::rows::array;
use crate::data::s;
use serde_json::{Map, Value, json};

pub fn command(thread: &str, request: &str, fields: Value) -> Value {
    let mut command = json!({"type":"runtime-request.respond","commandId":s5_net::uuid(),"threadId":thread,"requestId":request});
    for key in ["decision", "answers", "feedback"] {
        if let Some(value) = fields.get(key) {
            command[key] = value.clone();
        }
    }
    command
}
use std::collections::HashMap;

/// Approvals the open thread waits on (threadRequests.ts): pending requests that aren't
/// questions, auth refreshes or dynamic tool calls.
pub fn pending_approvals(p: &Value) -> Vec<&Value> {
    array(p, "runtimeRequests")
        .iter()
        .filter(|r| s(r, "status") == "pending" && !matches!(s(r, "kind"), "user_input" | "auth_refresh" | "dynamic_tool_call"))
        .collect()
}

fn live(request: &Value) -> bool {
    s(&request["responseCapability"], "type") == "live"
}

/// The first approval takes the composer's text as decline feedback, when its provider accepts it.
pub fn takes_feedback(approvals: &[&Value]) -> bool {
    approvals.first().is_some_and(|r| live(r) && r["responseCapability"]["supportsFeedback"] == true)
}

/// What a message sent while approvals wait does (useComposerSend): becomes the first approval's
/// decline feedback, or declines every live approval and then steers the turn. `Err` keeps the
/// draft, with why.
pub fn redirect(thread: &str, approvals: &[&Value], text: &str, extras: bool) -> Result<Vec<Value>, &'static str> {
    if takes_feedback(approvals) {
        if extras {
            return Err("Send approval feedback without attachments or added context.");
        }
        return Ok(vec![command(thread, s(approvals[0], "id"), json!({"decision": "decline", "feedback": text.trim()}))]);
    }
    Ok(approvals.iter().filter(|r| live(r)).map(|r| command(thread, s(r, "id"), json!({"decision": "decline"}))).collect())
}

/// One question's answer (pendingUserInput.ts resolvePendingUserInputAnswer): its own typed
/// answer when it takes one, else the picked option (a string) or options (an array).
pub fn resolve(question: &Value, selected: Option<&Vec<String>>, custom: Option<&str>) -> Option<Value> {
    if question["allowCustomAnswer"].as_bool() != Some(false)
        && let Some(answer) = custom.map(str::trim).filter(|a| !a.is_empty())
    {
        return Some(Value::String(answer.to_owned()));
    }
    let values = selected.filter(|v| !v.is_empty())?;
    Some(if question["multiSelect"].as_bool() == Some(true) { json!(values) } else { json!(values[0]) })
}

/// The answers resolved so far, keyed by question id.
pub fn answers(
    item: &Value,
    selected: &HashMap<String, Vec<String>>,
    custom: impl Fn(&str) -> Option<String>,
) -> Value {
    let mut result = Map::new();
    for question in array(item, "questions") {
        let id = s(question, "id");
        if let Some(answer) = resolve(question, selected.get(id), custom(id).as_deref()) {
            result.insert(id.to_owned(), answer);
        }
    }
    Value::Object(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn response_commands_preserve_decisions_and_answer_shapes() {
        for decision in ["accept", "decline", "cancel", "acceptForSession"] {
            let c = command(
                "thread",
                "request",
                json!({"decision":decision,"type":"ignored"}),
            );
            assert_eq!(c["type"], "runtime-request.respond");
            assert_eq!(c["threadId"], "thread");
            assert_eq!(c["requestId"], "request");
            assert_eq!(c["decision"], decision);
            assert!(c["answers"].is_null());
        }
        let c = command("thread", "request", json!({"answers":{"q":["one","two"]}}));
        assert_eq!(c["answers"], json!({"q":["one","two"]}));
        assert!(c["decision"].is_null());
    }
    #[test]
    fn a_send_redirects_waiting_approvals_by_feedback_or_by_declining_them() {
        let p = json!({"runtimeRequests": [
            {"id": "q", "kind": "user_input", "status": "pending"},
            {"id": "a", "kind": "command_execution_approval", "status": "pending", "responseCapability": {"type": "live"}},
            {"id": "gone", "kind": "file_change_approval", "status": "pending", "responseCapability": {"type": "not_resumable"}},
            {"id": "done", "kind": "command_execution_approval", "status": "resolved", "responseCapability": {"type": "live"}},
        ]});
        let approvals = pending_approvals(&p);
        assert_eq!(approvals.iter().map(|r| s(r, "id")).collect::<Vec<_>>(), ["a", "gone"]);
        let declines = redirect("t", &approvals, "do it differently", true).unwrap();
        assert_eq!(declines.len(), 1);
        assert_eq!((declines[0]["requestId"].as_str(), declines[0]["decision"].as_str()), (Some("a"), Some("decline")));
        assert!(declines[0]["feedback"].is_null());

        let p = json!({"runtimeRequests": [
            {"id": "a", "kind": "command_execution_approval", "status": "pending", "responseCapability": {"type": "live", "supportsFeedback": true}},
        ]});
        let approvals = pending_approvals(&p);
        let feedback = redirect("t", &approvals, " use npm ", false).unwrap();
        assert_eq!(feedback[0]["feedback"], "use npm");
        assert!(redirect("t", &approvals, "x", true).is_err());
    }
    #[test]
    fn single_answers_are_strings_multiple_answers_are_arrays_and_custom_takes_precedence() {
        let item = json!({"questions":[{"id":"single"},{"id":"multi","multiSelect":true},{"id":"custom"},{"id":"fixed","allowCustomAnswer":false}]});
        let selected = HashMap::from([
            ("single".to_owned(), vec!["one".to_owned()]),
            ("multi".to_owned(), vec!["one".to_owned(), "two".to_owned()]),
            ("custom".to_owned(), vec!["option".to_owned()]),
            ("fixed".to_owned(), vec!["option".to_owned()]),
        ]);
        let answers = answers(&item, &selected, |id| {
            matches!(id, "custom" | "fixed").then(|| "  typed  ".to_owned())
        });
        assert_eq!(
            answers,
            json!({"single":"one","multi":["one","two"],"custom":"typed","fixed":"option"})
        );
        let blank = resolve(&json!({"id":"q"}), Some(&vec!["opt".to_owned()]), Some("   "));
        assert_eq!(blank, Some(json!("opt")), "a blank typed answer falls back to the option");
        assert_eq!(resolve(&json!({"id":"q","multiSelect":true}), Some(&vec![]), None), None);
    }
}
