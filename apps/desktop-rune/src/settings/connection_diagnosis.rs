//! client-runtime `presentConnectionDiagnosis` for direct routes, the only kind Rune has: what
//! failed on the way to a saved environment, what to try, and the facts for a bug report.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hop {
    Ok,
    Failed,
    Unchecked,
}

#[derive(Debug, PartialEq)]
pub struct Diagnosis {
    pub headline: String,
    pub hops: Vec<(String, Hop)>,
    pub steps: Vec<String>,
    pub facts: Vec<(&'static str, String)>,
}

/// Web `ConnectionAttemptError.reason` read off the transport's message.
fn reason(error: &str) -> &'static str {
    match http_status(error) {
        Some(401) => "authentication",
        Some(403) => "permission",
        Some(_) => "transport",
        None if error.contains("no inbound frame") || error.contains("timed out") => "timeout",
        None => "network",
    }
}

fn http_status(error: &str) -> Option<u16> {
    error.strip_prefix("HTTP error: ")?.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

/// The diagnosis of a connection that closed with `error`, or None while nothing has failed.
/// The failure never names a hop on a direct route, so the device's hop stays unchecked.
pub fn present(host: &str, error: &str, client_version: Option<&str>, server_version: Option<&str>) -> Option<Diagnosis> {
    if error.is_empty() {
        return None;
    }
    let reason = reason(error);
    let refused = matches!(reason, "authentication" | "permission");
    let headline = if refused { format!("T3 Code on {host} did not accept this device") } else { format!("T3 Code on {host} did not answer") };
    let mut facts = vec![("Cause", reason.to_owned())];
    if let Some(status) = http_status(error) {
        facts.push(("HTTP status", status.to_string()));
    }
    facts.push(("Route", "Direct".to_owned()));
    for (label, value) in [("App version", client_version), ("Host version", server_version), ("Error", Some(error))] {
        if let Some(value) = value.filter(|v| !v.is_empty()) {
            facts.push((label, value.to_owned()));
        }
    }
    Some(Diagnosis {
        headline,
        hops: vec![("This device".to_owned(), Hop::Unchecked), (format!("T3 Code on {host}"), Hop::Failed)],
        steps: vec![format!("Make sure T3 Code is running on {host}."), "Retry. If it keeps failing, copy the diagnostics.".to_owned()],
        facts,
    })
}

/// Web `connectionDiagnosisText`: one plain-text block for bug reports.
pub fn text(host: &str, d: &Diagnosis) -> String {
    let mark = |hop: Hop| match hop {
        Hop::Ok => "ok",
        Hop::Failed => "FAILED",
        Hop::Unchecked => "not checked",
    };
    let mut lines = vec![format!("T3 Code connection to {host}: {}", d.headline), String::new()];
    lines.extend(d.hops.iter().map(|(label, hop)| format!("{label}: {}", mark(*hop))));
    lines.push(String::new());
    lines.extend(d.facts.iter().map(|(label, value)| format!("{label}: {value}")));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_direct_failure_reads_like_present_connection_diagnosis() {
        assert_eq!(present("Box", "", None, None), None, "nothing failed yet");
        let d = present("Box", "connection refused", Some("0.2.0"), Some("0.1.9")).unwrap();
        assert_eq!(d.headline, "T3 Code on Box did not answer");
        assert_eq!(d.hops, [("This device".into(), Hop::Unchecked), ("T3 Code on Box".into(), Hop::Failed)]);
        assert_eq!(d.steps, ["Make sure T3 Code is running on Box.", "Retry. If it keeps failing, copy the diagnostics."]);
        assert_eq!(
            text("Box", &d),
            "T3 Code connection to Box: T3 Code on Box did not answer\n\nThis device: not checked\nT3 Code on Box: FAILED\n\nCause: network\nRoute: Direct\nApp version: 0.2.0\nHost version: 0.1.9\nError: connection refused"
        );
        let refused = present("Box", "HTTP error: 401 Unauthorized", None, None).unwrap();
        assert_eq!(refused.headline, "T3 Code on Box did not accept this device");
        assert_eq!(&refused.facts[..2], [("Cause", "authentication".into()), ("HTTP status", "401".into())]);
        assert_eq!(present("Box", "no inbound frame for 15 seconds", None, None).unwrap().facts[0].1, "timeout");
    }
}
