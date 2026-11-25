//! Clerk's Frontend API as `@clerk/electron` drives it: requests marked native carry the client
//! JWT as a bearer token and get a fresh one back in `Authorization`. A sign-in opens the provider
//! in the system browser and returns on `s5code://app/?__clerk_oauth_attempt=…` with a
//! `rotating_token_nonce`, which completes it.

use serde_json::Value;

use super::{Failure, agent, failure, read_json};

const API_VERSION: &str = "2026-08-20";
const JS_VERSION: &str = "6.40.0";
const ELECTRON_SDK_VERSION: &str = "0.1.3";
const ATTEMPT: &str = "__clerk_oauth_attempt";

/// The Frontend API origin a publishable key names: `pk_live_` + base64 of `host$`.
pub fn frontend_api(publishable_key: &str) -> Option<String> {
    use base64::Engine;
    let encoded = publishable_key.trim().strip_prefix("pk_live_").or_else(|| publishable_key.trim().strip_prefix("pk_test_"))?;
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD.decode(encoded.trim_end_matches('=')).ok()?;
    let host = String::from_utf8(bytes).ok()?;
    let host = host.strip_suffix('$')?;
    (!host.is_empty() && !host.contains(['/', '?', '#', '@'])).then(|| format!("https://{host}"))
}

pub fn redirect_url(attempt: &str) -> String {
    format!("s5code://app/?{ATTEMPT}={attempt}")
}

/// What the browser hands back on the deep link.
#[derive(Clone, Debug, PartialEq)]
pub enum Callback {
    Completed { attempt: String, nonce: String },
    Failed { attempt: String, code: String },
}

/// The sign-in result in a deep link, or `None` for any other link.
pub fn callback(link: &str) -> Option<Callback> {
    let url = url::Url::parse(link).ok()?;
    if url.scheme() != "s5code" || url.host_str() != Some("app") || !matches!(url.path(), "" | "/") {
        return None;
    }
    let param = |name: &str| url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned()).filter(|v| !v.is_empty());
    let attempt = param(ATTEMPT)?;
    if let Some(nonce) = param("rotating_token_nonce") {
        return Some(Callback::Completed { attempt, nonce });
    }
    let code = param("__clerk_error_code").or_else(|| param("__clerk_status")).unwrap_or_else(|| "unknown".into());
    Some(Callback::Failed { attempt, code })
}

/// The URL of one Frontend API call, with the query clerk-js and the Electron SDK add.
pub fn url(fapi: &str, path: &str, extra: &[(&str, &str)]) -> String {
    let mut url = format!("{}{path}?", fapi.trim_end_matches('/'));
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.extend_pairs(extra.iter().copied());
    query.append_pair("__clerk_api_version", API_VERSION);
    query.append_pair("_clerk_js_version", JS_VERSION);
    query.append_pair("_is_native", "1");
    query.append_pair("_electron_sdk_version", ELECTRON_SDK_VERSION);
    url.push_str(&query.finish());
    url
}

/// A started sign-in: its id and the provider page to open in the browser.
pub fn started(response: &Value) -> Result<(String, String), String> {
    let sign_in = &response["response"];
    let id = sign_in["id"].as_str().filter(|s| !s.is_empty()).ok_or("Clerk did not return a sign-in.")?;
    let page = sign_in["first_factor_verification"]["external_verification_redirect_url"].as_str().filter(|s| s.starts_with("https://")).ok_or("Clerk did not return a sign-in page.")?;
    Ok((id.to_owned(), page.to_owned()))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Session(String),
    /// The provider account has no user yet: Clerk makes one by transferring to a sign-up.
    Transfer,
    Incomplete(String),
}

/// A reloaded sign-in, or a transferred sign-up.
pub fn outcome(response: &Value) -> Outcome {
    let attempt = &response["response"];
    if attempt["status"] == "complete"
        && let Some(session) = attempt["created_session_id"].as_str().filter(|s| !s.is_empty())
    {
        return Outcome::Session(session.to_owned());
    }
    if attempt["first_factor_verification"]["status"] == "transferable" || attempt["verifications"]["external_account"]["status"] == "transferable" {
        return Outcome::Transfer;
    }
    let error = &attempt["first_factor_verification"]["error"];
    Outcome::Incomplete(error["long_message"].as_str().or(error["message"].as_str()).or(attempt["status"].as_str()).unwrap_or("unknown").to_owned())
}

/// The signed-in user, as the Settings section names them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Identity {
    pub session: String,
    pub user: String,
    pub email: String,
    pub name: String,
}

/// The active session of the `client` in any response (or a bare client, from `GET /v1/client`).
pub fn identity(response: &Value) -> Option<Identity> {
    let client = if response["client"].is_object() { &response["client"] } else { &response["response"] };
    let active = client["last_active_session_id"].as_str()?;
    let session = client["sessions"].as_array()?.iter().find(|s| s["id"] == active && s["status"] == "active")?;
    let user = &session["user"];
    let primary = user["primary_email_address_id"].as_str().unwrap_or_default();
    let email = user["email_addresses"].as_array().into_iter().flatten().find(|e| e["id"] == primary).and_then(|e| e["email_address"].as_str()).unwrap_or_default();
    let name = [user["first_name"].as_str(), user["last_name"].as_str()].into_iter().flatten().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    Some(Identity { session: active.to_owned(), user: user["id"].as_str().unwrap_or_default().to_owned(), email: email.to_owned(), name })
}

/// The session JWT from a token response (`{jwt}`, or wrapped as `{response: {jwt}}`).
pub fn token(response: &Value) -> Option<String> {
    response["jwt"].as_str().or(response["response"]["jwt"].as_str()).filter(|t| !t.is_empty()).map(str::to_owned)
}

/// The social providers the instance enables, as `(strategy, name)`.
pub fn providers(environment: &Value) -> Vec<(String, String)> {
    let Some(social) = environment["user_settings"]["social"].as_object() else { return vec![] };
    social
        .iter()
        .filter(|(_, p)| p["enabled"] == true && p["authenticatable"] != false)
        .map(|(key, p)| (p["strategy"].as_str().unwrap_or(key).to_owned(), p["name"].as_str().unwrap_or(key).to_owned()))
        .collect()
}

/// A native Clerk client. `client_jwt` is the device's credential: keep it like a password.
pub struct Clerk {
    pub fapi: String,
    pub client_jwt: Option<String>,
}

impl Clerk {
    fn call(&mut self, action: &str, method: &str, path: &str, query: &[(&str, &str)], form: Option<&[(&str, &str)]>) -> Result<Value, Failure> {
        let mut request = agent().request(method, &url(&self.fapi, path, query));
        if let Some(jwt) = &self.client_jwt {
            request = request.set("Authorization", &format!("Bearer {jwt}"));
        }
        let response = match form {
            Some(form) => request.send_form(form),
            None => request.call(),
        }
        .map_err(|e| failure(action, e))?;
        if let Some(jwt) = response.header("authorization").map(|h| h.strip_prefix("Bearer ").unwrap_or(h).trim().to_owned()).filter(|j| !j.is_empty()) {
            self.client_jwt = Some(jwt);
        }
        read_json(action, response)
    }

    pub fn environment(&mut self) -> Result<Value, Failure> {
        self.call("load sign-in options", "GET", "/v1/environment", &[], None)
    }

    pub fn client(&mut self) -> Result<Value, Failure> {
        self.call("load your T3 Connect session", "GET", "/v1/client", &[], None)
    }

    pub fn start_sign_in(&mut self, strategy: &str, attempt: &str) -> Result<(String, String), Failure> {
        let redirect = redirect_url(attempt);
        let form = [("strategy", strategy), ("redirect_url", redirect.as_str()), ("action_complete_redirect_url", redirect.as_str())];
        let response = self.call("start signing in", "POST", "/v1/client/sign_ins", &[], Some(&form))?;
        started(&response).map_err(Failure::new)
    }

    /// The session a returned browser sign-in made, with the user it belongs to.
    pub fn finish_sign_in(&mut self, sign_in: &str, nonce: &str) -> Result<Identity, Failure> {
        let mut response = self.call("finish signing in", "GET", &format!("/v1/client/sign_ins/{sign_in}"), &[("rotating_token_nonce", nonce)], None)?;
        if outcome(&response) == Outcome::Transfer {
            response = self.call("finish signing up", "POST", "/v1/client/sign_ups", &[], Some(&[("transfer", "true")]))?;
        }
        match outcome(&response) {
            Outcome::Session(session) => Ok(identity(&response).filter(|i| i.session == session).unwrap_or(Identity { session, ..Identity::default() })),
            Outcome::Transfer => Err(Failure::new("Could not finish signing in: the account needs to be set up on the web first.")),
            Outcome::Incomplete(why) => Err(Failure::new(format!("Could not finish signing in: {why}"))),
        }
    }

    /// A short-lived session JWT minted with `template` (the relay's audience).
    pub fn session_token(&mut self, session: &str, template: &str) -> Result<String, Failure> {
        let response = self.call("refresh your T3 Connect session", "POST", &format!("/v1/client/sessions/{session}/tokens/{template}"), &[], Some(&[]))?;
        token(&response).ok_or_else(|| Failure::new("Clerk returned no session token."))
    }

    pub fn sign_out(&mut self, session: &str) -> Result<(), Failure> {
        self.call("sign out", "POST", &format!("/v1/client/sessions/{session}/remove"), &[], Some(&[])).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_publishable_key_names_the_frontend_api() {
        assert_eq!(frontend_api("pk_live_Y2xlcmsuczVjb2RlLnRvdWNodGVjaC5jbHViJA").as_deref(), Some("https://clerk.s5code.touchtech.club"));
        let secret = ["sk", "live", "Y2xlcmsuczVjb2RlLnRvdWNodGVjaC5jbHViJA"].join("_");
        assert_eq!(frontend_api(&secret), None, "never a secret key");
        assert_eq!(frontend_api("pk_live_bm8tZG9sbGFy"), None, "the host ends with $");
    }

    #[test]
    fn requests_carry_the_native_query_clerk_js_sends() {
        let url = url("https://clerk.example.com/", "/v1/client/sign_ins/sia_1", &[("rotating_token_nonce", "n+1")]);
        assert_eq!(url, "https://clerk.example.com/v1/client/sign_ins/sia_1?rotating_token_nonce=n%2B1&__clerk_api_version=2026-08-20&_clerk_js_version=6.40.0&_is_native=1&_electron_sdk_version=0.1.3");
    }

    #[test]
    fn deep_links_complete_or_fail_the_attempt_they_name() {
        assert_eq!(redirect_url("a-1"), "s5code://app/?__clerk_oauth_attempt=a-1");
        assert_eq!(callback("s5code://app/?__clerk_oauth_attempt=a-1&rotating_token_nonce=nonce-9"), Some(Callback::Completed { attempt: "a-1".into(), nonce: "nonce-9".into() }));
        assert_eq!(callback("s5code://app/?__clerk_oauth_attempt=a-1&__clerk_status=failed&__clerk_error_code=oauth_access_denied"), Some(Callback::Failed { attempt: "a-1".into(), code: "oauth_access_denied".into() }));
        assert_eq!(callback("s5code://app/settings/connections"), None);
        assert_eq!(callback("s5code://app/?rotating_token_nonce=x"), None, "only a link naming an attempt");
        assert_eq!(callback("https://app/?__clerk_oauth_attempt=a-1&rotating_token_nonce=x"), None);
    }

    /// Trimmed from a Frontend API `POST /v1/client/sign_ins` reply.
    fn started_reply() -> Value {
        json!({"response": {"object": "sign_in_attempt", "id": "sia_2abc", "status": "needs_first_factor",
            "first_factor_verification": {"status": "unverified", "strategy": "oauth_github",
                "external_verification_redirect_url": "https://github.com/login/oauth/authorize?client_id=x&state=y"}},
            "client": {"id": "client_1", "sessions": [], "last_active_session_id": null}})
    }

    #[test]
    fn sign_in_replies_parse_like_clerk_js() {
        assert_eq!(started(&started_reply()), Ok(("sia_2abc".into(), "https://github.com/login/oauth/authorize?client_id=x&state=y".into())));
        assert!(started(&json!({"response": {"id": "sia_1", "first_factor_verification": {"external_verification_redirect_url": "javascript:alert(1)"}}})).is_err());
        let complete = json!({"response": {"id": "sia_2abc", "status": "complete", "created_session_id": "sess_9"},
            "client": {"last_active_session_id": "sess_9", "sessions": [{"id": "sess_9", "status": "active", "user": {
                "id": "user_1", "first_name": "Ada", "last_name": "Lovelace", "primary_email_address_id": "idn_2",
                "email_addresses": [{"id": "idn_1", "email_address": "old@example.com"}, {"id": "idn_2", "email_address": "ada@example.com"}]}}]}});
        assert_eq!(outcome(&complete), Outcome::Session("sess_9".into()));
        assert_eq!(identity(&complete), Some(Identity { session: "sess_9".into(), user: "user_1".into(), email: "ada@example.com".into(), name: "Ada Lovelace".into() }));
        let transfer = json!({"response": {"status": "needs_identifier", "first_factor_verification": {"status": "transferable"}}});
        assert_eq!(outcome(&transfer), Outcome::Transfer);
        let denied = json!({"response": {"status": "needs_first_factor", "first_factor_verification": {"status": "failed", "error": {"message": "Denied", "long_message": "You denied access."}}}});
        assert_eq!(outcome(&denied), Outcome::Incomplete("You denied access.".into()));
        assert_eq!(token(&json!({"object": "token", "jwt": "eyJ.a.b"})).as_deref(), Some("eyJ.a.b"));
        assert_eq!(token(&json!({"response": {"jwt": "eyJ.c.d"}})).as_deref(), Some("eyJ.c.d"));
    }

    #[test]
    fn enabled_social_providers_are_offered() {
        let environment = json!({"user_settings": {"social": {
            "oauth_github": {"enabled": true, "authenticatable": true, "strategy": "oauth_github", "name": "GitHub"},
            "oauth_google": {"enabled": true, "authenticatable": true, "strategy": "oauth_google", "name": "Google"},
            "oauth_apple": {"enabled": false, "authenticatable": true, "strategy": "oauth_apple", "name": "Apple"}}}});
        assert_eq!(providers(&environment), vec![("oauth_github".into(), "GitHub".into()), ("oauth_google".into(), "Google".into())]);
    }
}
