//! Pairing with another T3 server, as the web's "Add environment" does
//! (client-runtime `preparePairingRegistration`): read its descriptor, then redeem the one-time
//! pairing code for a bearer session.

use serde_json::Value;

/// `resolveRemotePairingTarget` (packages/shared/src/remote.ts): a pairing URL (`…/pair#token=`,
/// `?token=`, or a hosted link's `?host=&token=`), or a host plus a pairing code, as the server's
/// http origin and the credential to redeem.
pub fn resolve_pairing(host: &str, code: &str) -> Result<(String, String), String> {
    let url = normalize(host)?;
    let query = |name: &str| url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.trim().to_owned()).filter(|v| !v.is_empty());
    let token = url::form_urlencoded::parse(url.fragment().unwrap_or("").as_bytes())
        .find(|(k, _)| k == "token")
        .map(|(_, v)| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .or_else(|| query("token"));
    if let (Some(token), Some(hosted)) = (&token, query("host")) {
        return Ok((origin(&normalize(&hosted)?), token.clone()));
    }
    let credential = token.or_else(|| Some(code.trim().to_owned()).filter(|c| !c.is_empty())).ok_or("Enter a pairing code.")?;
    Ok((origin(&url), credential))
}

fn normalize(raw: &str) -> Result<url::Url, String> {
    let trimmed = raw.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return Err("Enter a backend host.".into());
    }
    let has_scheme = trimmed.split_once("://").is_some_and(|(scheme, _)| !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)));
    let input = if has_scheme { trimmed.to_owned() } else { format!("https://{trimmed}") };
    let url = url::Url::parse(&input).map_err(|_| format!("{trimmed} is not a valid backend URL."))?;
    if !matches!(url.scheme(), "http" | "https" | "ws" | "wss") || url.host_str().is_none() {
        return Err(format!("{trimmed} is not a valid backend URL."));
    }
    Ok(url)
}

fn origin(url: &url::Url) -> String {
    let scheme = match url.scheme() {
        "ws" | "http" => "http",
        _ => "https",
    };
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    }
}

/// `/.well-known/t3/environment`: `environmentId`, `label`, platform and versions.
pub fn fetch_descriptor(base: &str) -> Result<Value, String> {
    let url = format!("{}/.well-known/t3/environment", base.trim_end_matches('/'));
    let descriptor: Value = ureq::get(&url).call().map_err(|e| format!("Could not reach {base}: {e}"))?.into_json().map_err(|e| e.to_string())?;
    if descriptor["environmentId"].as_str().is_none_or(str::is_empty) {
        return Err(format!("{base} is not a T3 Code server."));
    }
    Ok(descriptor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_inputs_resolve_like_the_web() {
        let ok = |host: &str, code: &str| resolve_pairing(host, code).unwrap();
        assert_eq!(ok("https://box.tail.ts.net/pair#token=ABC", ""), ("https://box.tail.ts.net".into(), "ABC".into()));
        assert_eq!(ok("http://10.0.0.2:3773/pair?token=XYZ", "ignored"), ("http://10.0.0.2:3773".into(), "XYZ".into()));
        assert_eq!(ok("box.local:3773", " CODE "), ("https://box.local:3773".into(), "CODE".into()));
        assert_eq!(ok("wss://box.local/ws", "CODE"), ("https://box.local".into(), "CODE".into()));
        assert_eq!(ok("https://app.t3.codes/pair?host=http://box:3773&token=T", ""), ("http://box:3773".into(), "T".into()));
        assert_eq!(resolve_pairing("", "CODE").unwrap_err(), "Enter a backend host.");
        assert_eq!(resolve_pairing("box.local", "  ").unwrap_err(), "Enter a pairing code.");
        assert!(resolve_pairing("ftp://box", "CODE").is_err());
    }
}
