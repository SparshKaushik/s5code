//! ChatGPT sign-in through this computer (packages/shared/src/codexAuthCallback.ts and
//! codexAuthHandoff.ts). A client-mode start leaves the redirect to the client, so Rune listens
//! on the redirect's loopback port, then hands the code to `provider.auth.complete`. The server
//! keeps the PKCE verifier; only `?code=&state=` crosses.
//!
//! A `s5code://auth/codex?request=…` link does the same for a web client that can't listen:
//! the browser is sent back to the web page with the callback in its fragment.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use s5_net::url::Url;
use serde_json::{Value, json};

use crate::App;

const TIMEOUT: Duration = Duration::from_secs(300);
const INVALID: &str = "Invalid ChatGPT sign-in request.";
const FOREIGN: &str = "This redirect URL does not belong to the current sign-in.";
const PAGE: &str = "<!doctype html><html><head><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"color-scheme\" content=\"light dark\"><title>T3 Code</title><style>body{font-family:system-ui;display:grid;place-items:center;min-height:90vh;margin:0}main{max-width:360px;padding:32px}h1{font-size:24px}p{line-height:1.6;opacity:.7}</style></head><body><main><h1>Return to T3 Code</h1><p>Your sign-in response has been received. T3 Code is finishing the connection. You can close this tab.</p></main></body></html>";

#[derive(Debug, PartialEq)]
pub struct Request {
    pub authorization_url: String,
    pub redirect_uri: String,
    pub state: String,
}

fn word(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `codexAuthorizationRequest`.
pub fn authorization_request(value: &str) -> Result<Request, &'static str> {
    let url = Url::parse(value).map_err(|_| INVALID)?;
    if value.len() > 16_384 || url.scheme() != "https" || url.host_str() != Some("auth.openai.com") || url.port().is_some() || url.path() != "/api/accounts/authorize" || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(INVALID);
    }
    let single = |key: &str| {
        let values: Vec<String> = url.query_pairs().filter(|(k, _)| k == key).map(|(_, v)| v.into_owned()).collect();
        match values.as_slice() {
            [one] if !one.is_empty() => Ok(one.clone()),
            _ => Err(INVALID),
        }
    };
    let redirect_uri = single("redirect_uri")?;
    let state = single("state")?;
    let port = redirect_uri.strip_prefix("http://127.0.0.1:").and_then(|r| r.strip_suffix("/auth/callback")).unwrap_or("");
    let port_ok = !port.is_empty() && port.len() <= 5 && !port.starts_with('0') && port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u32>().is_ok_and(|p| p <= 65_535);
    let client = single("client_id")?;
    let client_ok = client == "dynamic_agent_client" || client.strip_prefix("oaiapp_").is_some_and(|rest| word(rest, 1, usize::MAX));
    if !port_ok || !word(&state, 16, 128) || single("response_type")? != "code" || single("code_challenge_method")? != "S256" || !word(&single("code_challenge")?, 43, 43) || !client_ok {
        return Err(INVALID);
    }
    Ok(Request { authorization_url: url.to_string(), redirect_uri, state })
}

/// `codexCallbackUrl`: browsers can hide `http://`, so a pasted callback may lack it.
pub fn callback_url(value: &str, redirect_uri: &str, state: &str) -> Result<String, &'static str> {
    let expected = Url::parse(redirect_uri).map_err(|_| FOREIGN)?;
    let pasted = value.trim_start();
    let has_scheme = pasted.split_once("://").is_some_and(|(scheme, _)| {
        let mut chars = scheme.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    });
    let callback = Url::parse(&if has_scheme { pasted.to_owned() } else { format!("{}://{pasted}", expected.scheme()) }).map_err(|_| FOREIGN)?;
    let all = |key: &str| callback.query_pairs().filter(|(k, _)| k == key).map(|(_, v)| v.into_owned()).collect::<Vec<_>>();
    let (states, codes, errors, clients) = (all("state"), all("code"), all("error"), all("client_id"));
    let answered = (codes.len() == 1 && !codes[0].is_empty() && errors.is_empty()) || (errors.len() == 1 && !errors[0].is_empty() && codes.is_empty());
    let client_ok = match clients.as_slice() {
        [] => true,
        [one] => one.strip_prefix("oaiapp_").is_some_and(|rest| word(rest, 1, usize::MAX)),
        _ => false,
    };
    if value.len() > 16_384 || callback.origin() != expected.origin() || callback.path() != expected.path() || !callback.username().is_empty() || callback.password().is_some() || callback.fragment().is_some() || states != [state] || !client_ok || !answered {
        return Err(FOREIGN);
    }
    Ok(callback.to_string())
}

/// `providerAuthReturnUrl`: an app or known web page under /welcome or /settings, with only
/// the query keys those pages read.
pub fn return_url(value: &str) -> Option<String> {
    let mut url = Url::parse(value).ok()?;
    let desktop = ["s5code", "s5code-dev", "t3code", "t3code-dev"].contains(&url.scheme()) && url.host_str() == Some("app");
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "0.0.0.0" | "[::1]"));
    let origin = url.origin().ascii_serialization();
    let web = ["http", "https"].contains(&url.scheme()) && (loopback || origin == "https://app.s5code.touchtech.club" || origin == "https://app.t3.codes");
    let path = url.path().to_owned();
    if !url.username().is_empty() || url.password().is_some() || (!desktop && !web) || (path != "/welcome" && path != "/settings" && !path.starts_with("/settings/")) {
        return None;
    }
    let kept: Vec<(String, String)> = url.query_pairs().filter(|(k, _)| path != "/welcome" && ["machine", "project", "checkout", "environmentId", "instanceId"].contains(&k.as_ref())).map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    if kept.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(kept);
    }
    let agents = url.fragment().and_then(|f| f.strip_prefix("agents:")).is_some_and(|rest| word(rest, 1, usize::MAX));
    if path != "/welcome" || !agents {
        url.set_fragment(None);
    }
    Some(url.to_string())
}

#[derive(Debug, PartialEq, Clone)]
pub struct Handoff {
    authorization_url: String,
    return_url: String,
    environment_id: String,
    instance_id: String,
    flow_id: String,
}

/// `readCodexAuthHandoff` for this build's `s5code` scheme.
pub fn read_handoff(value: &str) -> Option<Handoff> {
    let url = Url::parse(value).ok()?;
    let requests: Vec<String> = url.query_pairs().filter(|(k, _)| k == "request").map(|(_, v)| v.into_owned()).collect();
    if value.len() > 32_768 || url.scheme() != "s5code" || url.host_str() != Some("auth") || url.path() != "/codex" || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() || requests.len() != 1 {
        return None;
    }
    let v: Value = serde_json::from_str(&requests[0]).ok()?;
    let field = |key: &str, max: usize| v[key].as_str().filter(|s| s.len() <= max).map(str::to_owned);
    let handoff = Handoff {
        authorization_url: field("authorizationUrl", 16_384)?,
        return_url: field("returnUrl", 4_096)?,
        environment_id: field("environmentId", 4_096).filter(|s| !s.trim().is_empty())?,
        instance_id: field("instanceId", 4_096).filter(|s| !s.trim().is_empty())?,
        flow_id: field("flowId", 128).filter(|s| !s.is_empty())?,
    };
    authorization_request(&handoff.authorization_url).ok()?;
    return_url(&handoff.return_url)?;
    Some(handoff)
}

/// `encodeURIComponent`.
fn encode_component(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// `codexAuthDeliveryUrl`: the code travels in the return page's fragment, which never
/// reaches a web server.
pub fn delivery_url(h: &Handoff, callback: &str) -> Result<String, &'static str> {
    let request = authorization_request(&h.authorization_url)?;
    callback_url(callback, &request.redirect_uri, &request.state)?;
    let mut url = return_url(&h.return_url).and_then(|d| Url::parse(&d).ok()).ok_or("Invalid T3 Code return address.")?;
    let return_hash = url.fragment().map(|f| format!("#{f}")).unwrap_or_default();
    let delivery = json!({"environmentId": h.environment_id, "instanceId": h.instance_id, "flowId": h.flow_id, "callbackUrl": callback, "returnHash": return_hash});
    url.set_fragment(Some(&format!("codex-auth={}", encode_component(&delivery.to_string()))));
    Ok(url.to_string())
}

/// A finished in-app sign-in: (environment, instance, flow, callback or error).
type Finished = (String, String, Value, Result<String, String>);
static FINISHED: Mutex<Vec<Finished>> = Mutex::new(Vec::new());
/// Open listeners by OAuth state, with the instance that started each.
static LISTENERS: Mutex<Option<HashMap<String, (String, Arc<AtomicBool>)>>> = Mutex::new(None);

enum Purpose {
    Complete { environment: String, instance: String, flow: Value },
    Handoff(Handoff),
}

fn open_browser(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(target_os = "macos"))]
    let program = "xdg-open";
    std::process::Command::new(program).arg(url).spawn().is_ok()
}

/// `receiveCodexAuthCallback`: listens on the redirect's port before opening the browser.
fn receive(request: Request, purpose: Purpose) -> Result<(), String> {
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut listeners = LISTENERS.lock().unwrap();
        let listeners = listeners.get_or_insert_default();
        if listeners.contains_key(&request.state) {
            return Err("This sign-in is already open on this computer.".into());
        }
        let instance = match &purpose {
            Purpose::Complete { instance, .. } => instance.clone(),
            Purpose::Handoff(h) => h.instance_id.clone(),
        };
        listeners.insert(request.state.clone(), (instance, cancelled.clone()));
    }
    let release = |state: &str| {
        if let Some(listeners) = LISTENERS.lock().unwrap().as_mut() {
            listeners.remove(state);
        }
    };
    let port: u16 = Url::parse(&request.redirect_uri).ok().and_then(|u| u.port()).unwrap_or(0);
    let listener = match TcpListener::bind(("127.0.0.1", port)).and_then(|l| l.set_nonblocking(true).map(|_| l)) {
        Ok(l) => l,
        Err(_) => {
            release(&request.state);
            return Err("The ChatGPT callback port is in use on this computer. Close the other sign-in and try again, or paste the redirect URL in T3 Code.".into());
        }
    };
    if !open_browser(&request.authorization_url) {
        release(&request.state);
        return Err("Could not open your sign-in browser.".into());
    }
    std::thread::spawn(move || {
        let deadline = Instant::now() + TIMEOUT;
        let outcome = loop {
            if cancelled.load(Ordering::SeqCst) {
                break Err("Sign-in cancelled on this computer.".to_owned());
            }
            if Instant::now() >= deadline {
                break Err("Sign-in expired. Try again.".to_owned());
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Some(callback) = answer(stream, &request, &purpose) {
                        break Ok(callback);
                    }
                }
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        };
        release(&request.state);
        if let Purpose::Complete { environment, instance, flow } = purpose {
            FINISHED.lock().unwrap().push((environment, instance, flow, outcome));
        }
    });
    Ok(())
}

/// One request on the listener; the callback URL when it was the sign-in's redirect.
fn answer(mut stream: TcpStream, request: &Request, purpose: &Purpose) -> Option<String> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut head = Vec::new();
    let mut buf = [0u8; 4096];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 16_384 {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    let head = String::from_utf8_lossy(&head);
    let mut line = head.lines().next().unwrap_or("").split(' ');
    let (method, target) = (line.next().unwrap_or(""), line.next().unwrap_or(""));
    let base = Url::parse(&request.redirect_uri).ok()?;
    let callback = (method == "GET").then(|| base.join(target).ok()).flatten().and_then(|u| callback_url(u.as_str(), &request.redirect_uri, &request.state).ok());
    let security = "cache-control: no-store\r\nreferrer-policy: no-referrer\r\nx-content-type-options: nosniff\r\nconnection: close\r\n";
    let destination = match (&callback, purpose) {
        (Some(callback), Purpose::Handoff(h)) => delivery_url(h, callback).ok(),
        _ => None,
    };
    let response = match (&callback, destination) {
        (None, _) => {
            let body = "This response does not belong to the active sign-in.";
            format!("HTTP/1.1 400 Bad Request\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len())
        }
        (Some(_), Some(location)) => format!("HTTP/1.1 303 See Other\r\nlocation: {location}\r\ncontent-length: 0\r\n{security}\r\n"),
        (Some(_), None) => format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-security-policy: default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'\r\ncontent-length: {}\r\n{security}\r\n{PAGE}",
            PAGE.len()
        ),
    };
    let _ = stream.write_all(response.as_bytes());
    callback
}

/// "Open browser" on a client-mode ChatGPT flow: the code comes back here. False when `url`
/// isn't one, so the caller opens it as a plain page.
pub fn sign_in(s: &mut App, instance: &str, flow: &Value, url: &str) -> bool {
    let Ok(request) = authorization_request(url) else { return false };
    if flow.is_null() {
        return false;
    }
    let purpose = Purpose::Complete { environment: s.ui.env.active.clone(), instance: instance.to_owned(), flow: flow.clone() };
    if let Err(e) = receive(request, purpose) {
        s.ui.settings.error = Some(e);
    }
    true
}

/// A handoff link from a web client on this computer.
pub fn handoff(s: &mut App, h: Handoff) {
    let Ok(request) = authorization_request(&h.authorization_url) else { return };
    if let Err(e) = receive(request, Purpose::Handoff(h)) {
        crate::shell::palette::toast(s, "Could not finish sign-in on this computer", Some(&e));
    }
}

/// `cancelCodexAuthCallback` for every listener the instance opened.
pub fn cancel(instance: &str) {
    for (owner, cancelled) in LISTENERS.lock().unwrap().iter().flat_map(|l| l.values()) {
        if owner == instance {
            cancelled.store(true, Ordering::SeqCst);
        }
    }
}

pub fn poll(s: &mut App) -> bool {
    let finished = std::mem::take(&mut *FINISHED.lock().unwrap());
    let changed = !finished.is_empty();
    for (environment, instance, flow, outcome) in finished {
        match outcome {
            Ok(callback) if environment == s.ui.env.active => {
                super::request(s, "provider.auth.complete", json!({"instanceId": instance, "flowId": flow, "callbackUrl": callback}));
            }
            Ok(_) => s.ui.settings.error = Some("Could not finish sign-in on this computer. Try again or paste the redirect URL below.".into()),
            Err(e) if e.starts_with("Sign-in cancelled") => {}
            Err(e) => s.ui.settings.error = Some(e),
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHALLENGE: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQ";

    fn authorize(extra: &str) -> String {
        format!("https://auth.openai.com/api/accounts/authorize?response_type=code&client_id=oaiapp_abc&redirect_uri=http%3A%2F%2F127.0.0.1%3A1455%2Fauth%2Fcallback&state=state-0123456789abcdef&code_challenge={CHALLENGE}&code_challenge_method=S256{extra}")
    }

    #[test]
    fn authorization_requests_follow_the_shared_rules() {
        let ok = authorization_request(&authorize("")).unwrap();
        assert_eq!((ok.redirect_uri.as_str(), ok.state.as_str()), ("http://127.0.0.1:1455/auth/callback", "state-0123456789abcdef"));
        for bad in [
            authorize("&state=again"),
            authorize("#frag"),
            authorize("").replace("auth.openai.com", "evil.example"),
            authorize("").replace("S256", "plain"),
            authorize("").replace("oaiapp_abc", "someone"),
            authorize("").replace("1455", "0455"),
            authorize("").replace("1455", "99999"),
            authorize("").replace("127.0.0.1", "localhost"),
            authorize("").replace("state-0123456789abcdef", "short"),
            authorize("").replace(CHALLENGE, "abc"),
        ] {
            assert_eq!(authorization_request(&bad), Err(INVALID), "{bad}");
        }
        assert!(authorization_request(&authorize("").replace("oaiapp_abc", "dynamic_agent_client")).is_ok());
    }

    #[test]
    fn callbacks_must_belong_to_the_sign_in() {
        let (redirect, state) = ("http://127.0.0.1:1455/auth/callback", "state-0123456789abcdef");
        assert_eq!(callback_url("http://127.0.0.1:1455/auth/callback?code=c1&state=state-0123456789abcdef", redirect, state).unwrap(), "http://127.0.0.1:1455/auth/callback?code=c1&state=state-0123456789abcdef");
        assert!(callback_url("127.0.0.1:1455/auth/callback?error=access_denied&state=state-0123456789abcdef", redirect, state).is_ok(), "a pasted URL may lack http://");
        assert!(callback_url("http://127.0.0.1:1455/auth/callback?code=c1&state=state-0123456789abcdef&client_id=oaiapp_x", redirect, state).is_ok());
        for bad in [
            "http://127.0.0.1:1455/auth/callback?code=c1&state=other-0123456789abcdef",
            "http://127.0.0.1:1456/auth/callback?code=c1&state=state-0123456789abcdef",
            "http://127.0.0.1:1455/other?code=c1&state=state-0123456789abcdef",
            "http://127.0.0.1:1455/auth/callback?code=c1&error=x&state=state-0123456789abcdef",
            "http://127.0.0.1:1455/auth/callback?state=state-0123456789abcdef",
            "http://127.0.0.1:1455/auth/callback?code=c1&state=state-0123456789abcdef#x",
            "http://127.0.0.1:1455/auth/callback?code=c1&state=state-0123456789abcdef&client_id=bad",
        ] {
            assert_eq!(callback_url(bad, redirect, state), Err(FOREIGN), "{bad}");
        }
    }

    #[test]
    fn return_urls_keep_only_known_pages_and_keys() {
        assert_eq!(return_url("s5code://app/settings/providers?instanceId=work&secret=1#x").as_deref(), Some("s5code://app/settings/providers?instanceId=work"));
        assert_eq!(return_url("https://app.s5code.touchtech.club/welcome?a=1#agents:env-1").as_deref(), Some("https://app.s5code.touchtech.club/welcome#agents:env-1"));
        assert_eq!(return_url("http://localhost:5733/settings").as_deref(), Some("http://localhost:5733/settings"));
        for bad in ["https://evil.example/settings", "s5code://app/threads/1", "s5code://other/settings", "https://u:p@app.t3.codes/settings"] {
            assert_eq!(return_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn handoffs_deliver_the_callback_in_the_return_fragment() {
        let request = json!({"authorizationUrl": authorize(""), "returnUrl": "https://app.s5code.touchtech.club/settings/providers?instanceId=codex", "environmentId": "env-1", "instanceId": "codex", "flowId": "f1"});
        let link = format!("s5code://auth/codex?request={}", encode_component(&request.to_string()));
        let h = read_handoff(&link).expect("a valid handoff");
        assert_eq!(read_handoff(&link.replace("s5code://", "t3code://")), None);
        assert_eq!(read_handoff(&format!("{link}#x")), None);
        let callback = "http://127.0.0.1:1455/auth/callback?code=c1&state=state-0123456789abcdef";
        let delivered = Url::parse(&delivery_url(&h, callback).unwrap()).unwrap();
        assert_eq!(delivered.path(), "/settings/providers");
        let fragment = delivered.fragment().unwrap().strip_prefix("codex-auth=").unwrap();
        let decoded: Value = serde_json::from_str(&s5_net::url::form_urlencoded::parse(format!("x={fragment}").as_bytes()).next().unwrap().1).unwrap();
        assert_eq!(decoded, json!({"environmentId": "env-1", "instanceId": "codex", "flowId": "f1", "callbackUrl": callback, "returnHash": ""}));
        assert!(delivery_url(&h, "http://127.0.0.1:1455/auth/callback?code=c1&state=wrong-0123456789abcdef").is_err());
    }
}
