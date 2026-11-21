//! createPullRequestRouter: a pull request read or write that another machine shared in Settings
//! may answer. Credentials stay on their machines; only a host and account the source verified
//! cross the wire, as `host` and `expectedAccountId`. The answer lands on `App::src` under the id
//! [`send`] returned, so callers read it as they read any reply.
use serde_json::{Value, json};
use crate::App;

const READS: &[&str] = &[
    "pullRequests.summary",
    "pullRequests.stack",
    "pullRequests.detail",
    "pullRequests.preview",
    "pullRequests.checks",
    "pullRequests.activity",
    "pullRequests.threadComments",
    "pullRequests.diffFileContents",
    "pullRequests.filesViewed",
    "pullRequests.changeBreakdown",
    "pullRequests.reviewerCandidates",
    "pullRequests.labelCandidates",
];
// Review guides are not routed: each server keeps its own.
const WRITES: &[&str] = &[
    "pullRequests.setFilesViewed",
    "pullRequests.runAction",
    "pullRequests.update",
    "pullRequests.comment",
    "pullRequests.updateComment",
    "pullRequests.submitReview",
    "pullRequests.replyToThread",
    "pullRequests.setReaction",
    "pullRequests.setThreadResolution",
    "pullRequests.requestReviewers",
    "pullRequests.setLabels",
];
const PROBE_MS: i64 = 2_000;
const READ_MS: i64 = 30_000;
const REPORT_MS: i64 = 5_000;

#[derive(Default)]
pub struct Ui {
    routes: Vec<Routed>,
    /// `pullRequests.reportState` sends, whose answers nobody reads.
    reports: Vec<(String, u64, i64)>,
}

struct Routed {
    /// What the caller holds; no server answers it.
    id: u64,
    origin: String,
    method: String,
    /// The caller's input, for the stale read web falls back to.
    original: Value,
    input: Value,
    read: bool,
    candidates: Vec<String>,
    next: usize,
    /// `pullRequests.routing`'s answer on the origin; null sends the source's own read.
    identity: Value,
    step: Step,
    error: Option<Value>,
    fell_back: bool,
    /// Whether each step asks Settings again; tests hand over the candidates directly.
    recheck: bool,
}

#[derive(Clone)]
enum Step {
    Probe(u64, i64),
    Check(String, u64, i64),
    Run(String, u64, i64),
}

/// Sends `method` for the pull request in `input` from `env`, through another machine when
/// Settings shares one with it. `None` when `env` has no client.
pub(crate) fn send(s: &mut App, env: &str, method: &str, input: Value) -> Option<u64> {
    let (read, write) = (READS.contains(&method), WRITES.contains(&method));
    if !(read || write) || !is_ref(&input) {
        return crate::env::net_of(s, env).map(|n| n.request(method, input));
    }
    crate::env::net_of(s, env)?;
    let mut candidates = crate::settings::machines::pull_request_route(s, env, write, method == "pullRequests.checks");
    // readOrWrite: a source on this machine answers first; the others only once it fails.
    if read && crate::settings::machines::loopback(s, env) {
        candidates.retain(|k| k != env);
        candidates.insert(0, env.to_owned());
    }
    start(s, env, method, input, read, candidates, true)
}

fn start(s: &mut App, env: &str, method: &str, original: Value, read: bool, candidates: Vec<String>, recheck: bool) -> Option<u64> {
    if candidates.len() < 2 {
        return crate::env::net_of(s, env).map(|n| n.request(method, original));
    }
    let mut input = original.clone();
    if read {
        input["allowStale"] = json!(false);
    }
    let net = crate::env::net_of(s, env)?;
    let probe = net.request("pullRequests.routing", reference(&input));
    let id = net.reserve_id();
    let now = crate::data::now_ms();
    s.ui.pages.route.routes.push(Routed {
        id,
        origin: env.to_owned(),
        method: method.to_owned(),
        original,
        input,
        read,
        candidates,
        next: 0,
        identity: Value::Null,
        step: Step::Probe(probe, now),
        error: None,
        fell_back: false,
        recheck,
    });
    Some(id)
}

/// Moves each routed request on by the answers that arrived, and hands finished ones to their caller.
pub(crate) fn pump(s: &mut App) -> bool {
    let now = crate::data::now_ms();
    let mut changed = false;
    for (env, id, at) in std::mem::take(&mut s.ui.pages.route.reports) {
        if super::pull_requests::take(s, &env, id).is_none() && now - at < REPORT_MS {
            s.ui.pages.route.reports.push((env, id, at));
        }
    }
    let mut kept = Vec::new();
    for mut r in std::mem::take(&mut s.ui.pages.route.routes) {
        let (env, id, at, limit) = match r.step.clone() {
            Step::Probe(id, at) => (r.origin.clone(), id, at, PROBE_MS),
            Step::Check(env, id, at) => (env, id, at, PROBE_MS),
            Step::Run(env, id, at) => (env, id, at, if r.read { READ_MS } else { i64::MAX }),
        };
        let reply = match super::pull_requests::take(s, &env, id) {
            Some(reply) => reply,
            None if now - at >= limit => Err(unavailable("The environment did not respond to the PR request.")),
            None => {
                kept.push(r);
                continue;
            }
        };
        changed = true;
        let done = match r.step.clone() {
            Step::Probe(..) => {
                r.identity = reply.ok().filter(|v| v["host"].is_string() && v["accountId"].is_string()).unwrap_or(Value::Null);
                if r.identity.is_null() {
                    // The source answers only for a host whose account it can verify.
                    r.candidates = vec![r.origin.clone()];
                }
                r.next = 0;
                advance(s, &mut r, now)
            }
            Step::Check(env, ..) => {
                if reply.ok().is_some_and(|alternate| same_account(&r.identity, &alternate)) && allowed(s, &r, &env) {
                    run(s, &mut r, &env, now)
                } else {
                    r.next += 1;
                    advance(s, &mut r, now)
                }
            }
            Step::Run(env, ..) => match reply {
                Ok(v) => {
                    if env != r.origin
                        && let Some(state) = v.get("state").filter(|x| x.is_string())
                    {
                        report(s, &r, state.clone(), v.get("isDraft").cloned(), now);
                    }
                    Some(Ok(answer(&r, v)))
                }
                Err(e) => {
                    let more = r.next + 1 < r.candidates.len();
                    if (r.read || rejected_before_dispatch(&e)) && more {
                        r.error = Some(e);
                        r.next += 1;
                        advance(s, &mut r, now)
                    } else {
                        fall_back(s, &mut r, e, now)
                    }
                }
            },
        };
        match done {
            Some(reply) => deliver(s, &r, reply),
            None => kept.push(r),
        }
    }
    kept.append(&mut s.ui.pages.route.routes);
    s.ui.pages.route.routes = kept;
    changed
}

/// Starts the next candidate: the origin runs at once, another machine first proves its account.
fn advance(s: &mut App, r: &mut Routed, now: i64) -> Option<Result<Value, Value>> {
    while let Some(env) = r.candidates.get(r.next).cloned() {
        if env == r.origin {
            return run(s, r, &env, now);
        }
        let host = r.identity["host"].clone();
        let net = if allowed(s, r, &env) { crate::env::net_of(s, &env) } else { None };
        match net {
            // An older server would discard expectedAccountId, so it must answer for the host first.
            Some(net) => {
                r.step = Step::Check(env, net.request("pullRequests.routingIdentity", json!({"host": host})), now);
                return None;
            }
            None => r.next += 1,
        }
    }
    let e = r.error.take().unwrap_or_else(|| unavailable("The environment was removed."));
    fall_back(s, r, e, now)
}

fn run(s: &mut App, r: &mut Routed, env: &str, now: i64) -> Option<Result<Value, Value>> {
    let input = if r.identity.is_null() { r.input.clone() } else { routed_input(&r.input, &r.identity) };
    match crate::env::net_of(s, env) {
        Some(net) => {
            r.step = Step::Run(env.to_owned(), net.request(&r.method, input), now);
            None
        }
        None => {
            r.error = Some(unavailable("The environment was removed."));
            r.next += 1;
            advance(s, r, now)
        }
    }
}

/// readOrWrite: a failed strict summary or detail settles for what the source holds.
fn fall_back(s: &mut App, r: &mut Routed, e: Value, now: i64) -> Option<Result<Value, Value>> {
    let stale_ok = matches!(r.method.as_str(), "pullRequests.summary" | "pullRequests.detail") && r.original["allowStale"] != false;
    if r.fell_back || !r.read || !stale_ok {
        return Some(Err(e));
    }
    r.fell_back = true;
    r.identity = Value::Null;
    r.next = r.candidates.len();
    match crate::env::net_of(s, &r.origin) {
        Some(net) => {
            r.step = Step::Run(r.origin.clone(), net.request(&r.method, r.original.clone()), now);
            None
        }
        None => Some(Err(e)),
    }
}

/// Web re-checks the permission before each step, as Settings may have changed it meanwhile.
fn allowed(s: &mut App, r: &Routed, env: &str) -> bool {
    !r.recheck || crate::settings::machines::pull_request_route(s, &r.origin, !r.read, r.method == "pullRequests.checks").iter().any(|k| k == env)
}

/// reportState: the origin owns the threads linked to this pull request, but a routed read never
/// reaches it, so it hears which state the read saw.
fn report(s: &mut App, r: &Routed, state: Value, draft: Option<Value>, now: i64) {
    let mut input = json!({"reference": reference(&r.input), "state": state});
    if let Some(draft) = draft.filter(Value::is_boolean) {
        input["isDraft"] = draft;
    }
    if let Some(net) = crate::env::net_of(s, &r.origin) {
        let id = net.request("pullRequests.reportState", input);
        s.ui.pages.route.reports.push((r.origin.clone(), id, now));
    }
}

fn deliver(s: &mut App, r: &Routed, reply: Result<Value, Value>) {
    // Parked environments' replies end on `App::src` (see `env::drain`), where callers read them.
    let net = if s.src.net().is_some() { s.src.net() } else { crate::env::net_of(s, &r.origin) };
    if let Some(net) = net {
        net.put_reply(r.id, reply);
    }
}

/// `PullRequestRef`'s fields: the routing probes take nothing else.
fn reference(input: &Value) -> Value {
    let mut out = json!({"projectId": input["projectId"], "repository": input["repository"], "number": input["number"]});
    if input["host"].is_string() {
        out["host"] = input["host"].clone();
    }
    out
}

fn is_ref(input: &Value) -> bool {
    input["projectId"].as_str().is_some_and(|p| !p.is_empty())
        && input["repository"].as_str().is_some_and(|r| !r.trim().is_empty())
        && input["number"].as_u64().is_some_and(|n| n > 0)
}

fn routed_input(input: &Value, identity: &Value) -> Value {
    let mut out = input.clone();
    out["host"] = identity["host"].clone();
    out["expectedAccountId"] = identity["accountId"].clone();
    out
}

/// The routing identity check: another machine may answer only as the same account on the same host.
fn same_account(identity: &Value, alternate: &Value) -> bool {
    let text = |v: &Value, k: &str| v[k].as_str().map(str::to_owned);
    text(identity, "provider").is_some()
        && text(identity, "provider") == text(alternate, "provider")
        && text(identity, "host").map(|h| h.to_lowercase()) == text(alternate, "host").map(|h| h.to_lowercase())
        && text(identity, "accountId").is_some()
        && text(identity, "accountId") == text(alternate, "accountId")
}

/// A routed answer reads as the origin's project; a detail also takes the origin's workspace.
fn answer(r: &Routed, mut v: Value) -> Value {
    if r.identity.is_null() {
        return v;
    }
    if let Some(o) = v.as_object_mut()
        && o.contains_key("projectId")
    {
        o.insert("projectId".into(), r.input["projectId"].clone());
        if r.method == "pullRequests.detail" {
            o.insert("projectTitle".into(), r.identity["projectTitle"].clone());
            o.insert("workspaceRoot".into(), r.identity["workspaceRoot"].clone());
        }
    }
    v
}

fn rejected_before_dispatch(e: &Value) -> bool {
    match e["_tag"].as_str().unwrap_or_default() {
        "EnvironmentRpcUnavailableError" | "EnvironmentNotRegisteredError" => true,
        "PullRequestOperationError" => e["operation"] == "routeIdentity",
        _ => false,
    }
}

fn unavailable(message: &str) -> Value {
    json!({"_tag": "EnvironmentRpcUnavailableError", "message": message})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Source;

    const IDENTITY: &str = r#"{"accountId": "acct-1", "host": "github.com", "provider": "github", "viewer": "sparsh", "projectTitle": "s5code", "workspaceRoot": "/w/s5code"}"#;

    fn app() -> (App, String) {
        let mut s = App::new();
        s.src = Source::Live(s5_net::Net::offline());
        crate::env::park(&mut s, "box", Source::Live(s5_net::Net::offline()));
        let origin = s.ui.env.active.clone();
        (s, origin)
    }

    fn step(s: &App) -> Step {
        s.ui.pages.route.routes[0].step.clone()
    }

    /// Answers the request the route waits on, on the client it went to.
    fn answer_with(s: &mut App, reply: Result<Value, Value>) {
        let (env, id) = match step(s) {
            Step::Probe(id, _) => (s.ui.env.active.clone(), id),
            Step::Check(env, id, _) | Step::Run(env, id, _) => (env, id),
        };
        crate::env::net_of(s, &env).unwrap().put_reply(id, reply);
        pump(s);
    }

    fn input() -> Value {
        json!({"projectId": "p1", "repository": "pingdotgg/t3code", "number": 42})
    }

    #[test]
    fn another_machine_answers_only_as_the_same_account_and_reads_as_the_origin() {
        let identity: Value = serde_json::from_str(IDENTITY).unwrap();
        let (mut s, origin) = app();
        let id = start(&mut s, &origin, "pullRequests.detail", input(), true, vec!["box".into(), origin.clone()], false).unwrap();
        answer_with(&mut s, Ok(identity.clone()));
        assert!(matches!(step(&s), Step::Check(env, ..) if env == "box"), "the alternative proves its account first");
        answer_with(&mut s, Ok(json!({"accountId": "acct-1", "host": "GitHub.com", "provider": "github", "viewer": "sparsh"})));
        assert!(matches!(step(&s), Step::Run(env, ..) if env == "box"));
        answer_with(&mut s, Ok(json!({"projectId": "theirs", "state": "open", "number": 42})));
        let got = s.src.net().unwrap().take_reply(id).unwrap().unwrap();
        assert_eq!((got["projectId"].as_str(), got["projectTitle"].as_str(), got["workspaceRoot"].as_str()), (Some("p1"), Some("s5code"), Some("/w/s5code")));
        assert_eq!(s.ui.pages.route.reports.len(), 1, "the origin hears the state the routed read saw");
        assert!(s.ui.pages.route.routes.is_empty());

        let routed = routed_input(&input(), &identity);
        assert_eq!((routed["host"].as_str(), routed["expectedAccountId"].as_str()), (Some("github.com"), Some("acct-1")));
    }

    #[test]
    fn another_account_a_failed_read_or_no_verified_host_falls_to_the_source() {
        let identity: Value = serde_json::from_str(IDENTITY).unwrap();
        let (mut s, origin) = app();
        start(&mut s, &origin, "pullRequests.activity", input(), true, vec!["box".into(), origin.clone()], false).unwrap();
        answer_with(&mut s, Ok(identity.clone()));
        answer_with(&mut s, Ok(json!({"accountId": "acct-2", "host": "github.com", "provider": "github", "viewer": "other"})));
        assert!(matches!(step(&s), Step::Run(env, ..) if env == origin), "another account never answers");

        let (mut s, origin) = app();
        start(&mut s, &origin, "pullRequests.activity", input(), true, vec!["box".into(), origin.clone()], false).unwrap();
        answer_with(&mut s, Ok(identity.clone()));
        answer_with(&mut s, Ok(identity.clone()));
        answer_with(&mut s, Err(json!({"message": "rate limited"})));
        assert!(matches!(step(&s), Step::Run(env, ..) if env == origin), "a failed read tries the next machine");

        let (mut s, origin) = app();
        start(&mut s, &origin, "pullRequests.comment", input(), false, vec!["box".into(), origin.clone()], false).unwrap();
        answer_with(&mut s, Ok(identity.clone()));
        answer_with(&mut s, Ok(identity));
        answer_with(&mut s, Err(json!({"message": "rate limited"})));
        assert!(s.ui.pages.route.routes.is_empty(), "a write the host refused is not sent twice");

        let (mut s, origin) = app();
        start(&mut s, &origin, "pullRequests.checks", input(), true, vec!["box".into(), origin.clone()], false).unwrap();
        answer_with(&mut s, Ok(Value::Null));
        assert!(matches!(step(&s), Step::Run(env, ..) if env == origin), "no verified host: the source answers");
        assert!(same_account(&serde_json::from_str(IDENTITY).unwrap(), &json!({"accountId": "acct-1", "host": "GITHUB.COM", "provider": "github"})));
        assert!(!same_account(&serde_json::from_str(IDENTITY).unwrap(), &json!({"accountId": "acct-1", "host": "github.com", "provider": "gitlab"})));
    }
}
