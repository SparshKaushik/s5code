//! T3 Connect, the client side as Electron's desktop app speaks it: a Clerk session from a
//! browser sign-in ([`clerk`]), DPoP proofs bound to this device's key ([`dpop`]), and the relay
//! that turns that session into a socket on a linked environment ([`relay`]).
//!
//! Requests block, so callers run them off the UI thread.

pub mod clerk;
pub mod dpop;
pub mod relay;

use base64::Engine;
use serde_json::Value;

pub(crate) fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn b64url_decode(text: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(text.trim_end_matches('=')).ok()
}

/// The trace ID of a W3C `traceparent` header (web `TRACEPARENT_TRACE_ID`).
pub fn trace_id(traceparent: &str) -> Option<String> {
    let parts: Vec<&str> = traceparent.trim().split('-').collect();
    let hex = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_hexdigit());
    (parts.len() == 4 && hex(parts[0], 2) && hex(parts[1], 32) && hex(parts[2], 16) && hex(parts[3], 2)).then(|| parts[1].to_ascii_lowercase())
}

/// A cloud request that failed: what to show, and the trace ID the relay or server sent.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub message: String,
    pub trace: Option<String>,
    pub status: Option<u16>,
}

impl Failure {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), trace: None, status: None }
    }
    /// Refused credentials, as opposed to an outage worth retrying.
    pub fn denied(&self) -> bool {
        matches!(self.status, Some(401 | 403))
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// The detail of an error body: the relay's tagged errors (`RelayProtectedError`), Clerk's
/// `errors` list, or a plain `message`.
pub(crate) fn error_detail(body: &Value) -> Option<String> {
    if let Some(first) = body["errors"].as_array().and_then(|e| e.first()) {
        return first["long_message"].as_str().or(first["message"].as_str()).map(str::to_owned);
    }
    if body["_tag"] == "RelayAuthInvalidError" {
        return Some(format!("Relay authentication failed: {}", body["reason"].as_str().unwrap_or("invalid credentials")));
    }
    body["message"].as_str().or(body["reason"].as_str()).or(body["code"].as_str()).map(str::to_owned)
}

/// A failed status with its body and headers already read, as [`failure`] gets it from ureq.
pub(crate) fn status_failure(action: &str, status: u16, traceparent: Option<&str>, body: &str) -> Failure {
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let detail = error_detail(&parsed).unwrap_or_else(|| format!("HTTP {status}"));
    let trace = parsed["traceId"].as_str().map(str::to_owned).or_else(|| traceparent.and_then(trace_id));
    Failure { message: format!("Could not {action}: {detail}"), trace, status: Some(status) }
}

pub(crate) fn failure(action: &str, error: ureq::Error) -> Failure {
    match error {
        ureq::Error::Status(status, response) => {
            let traceparent = response.header("traceparent").map(str::to_owned);
            let body = response.into_string().unwrap_or_default();
            status_failure(action, status, traceparent.as_deref(), &body)
        }
        error => Failure::new(format!("Could not {action}. Check your network connection. ({error})")),
    }
}

pub(crate) fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().redirects(0).timeout(std::time::Duration::from_secs(30)).build()
}

pub(crate) fn read_json(action: &str, response: ureq::Response) -> Result<Value, Failure> {
    response.into_json().map_err(|e| Failure::new(format!("Could not {action}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trace_ids_come_from_traceparent_like_web() {
        assert_eq!(trace_id("00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01").as_deref(), Some("4bf92f3577b34da6a3ce929d0e0e4736"));
        assert_eq!(trace_id("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7"), None, "four fields");
        assert_eq!(trace_id("00-xyz-00f067aa0ba902b7-01"), None);
    }

    #[test]
    fn relay_error_bodies_keep_their_trace_and_reason() {
        let body = json!({"_tag":"RelayAuthInvalidError","code":"auth_invalid","reason":"token_expired","traceId":"0af7651916cd43dd8448eb211c80319c"}).to_string();
        let failure = status_failure("connect relay environment", 401, None, &body);
        assert_eq!(failure.message, "Could not connect relay environment: Relay authentication failed: token_expired");
        assert_eq!(failure.trace.as_deref(), Some("0af7651916cd43dd8448eb211c80319c"));
        assert!(failure.denied());
        let untyped = status_failure("list environments", 502, Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"), "<html>");
        assert_eq!(untyped.message, "Could not list environments: HTTP 502");
        assert_eq!(untyped.trace.as_deref(), Some("0af7651916cd43dd8448eb211c80319c"), "an untyped response still carries traceparent");
        assert!(!untyped.denied());
        let clerk = json!({"errors":[{"message":"Invalid","long_message":"Sign in is not complete.","code":"form_identifier_not_found"}]}).to_string();
        assert_eq!(status_failure("finish signing in", 422, None, &clerk).message, "Could not finish signing in: Sign in is not complete.");
    }
}
