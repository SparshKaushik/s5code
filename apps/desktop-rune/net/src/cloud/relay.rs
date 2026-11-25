//! The relay (`packages/contracts/src/relay.ts`) and the environment hop after it, as
//! `ManagedRelayClient` and `RemoteEnvironmentAuthorization.authorizeDpop` do them:
//!
//! 1. exchange the Clerk session JWT for a DPoP relay token (`POST /v1/client/dpop-token`);
//! 2. ask the relay to connect an environment, getting its endpoint and a bootstrap credential;
//! 3. exchange that credential at the environment's `/oauth/token` for a DPoP access token;
//! 4. mint a key-bound RPC ticket and open `/ws` with it and an upgrade proof in the query.
//!
//! Steps 1–3 run once per token lifetime; step 4 runs for every socket.

use serde_json::{Value, json};

use super::dpop::Key;
use super::{Failure, agent, failure, read_json};

pub const CLIENT_ID: &str = "t3-web";
pub const CONNECT_SCOPE: &str = "environment:connect";
/// What discovery asks for: status checks and the connect that usually follows.
pub const STATUS_SCOPE: &str = "environment:status environment:connect";
const TOKEN_EXCHANGE: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
const JWT: &str = "urn:ietf:params:oauth:token-type:jwt";
const ACCESS_TOKEN: &str = "urn:ietf:params:oauth:token-type:access_token";
const ENVIRONMENT_BOOTSTRAP: &str = "urn:t3:params:oauth:token-type:environment-bootstrap";
pub const TICKET_PATH: &str = "/api/auth/websocket-ticket/rpc";
/// Servers older than ticket audiences only mint at the bare path, and answer 404 at the other.
const LEGACY_TICKET_PATH: &str = "/api/auth/websocket-ticket";

/// `normalizeSecureRelayUrl`: an HTTPS origin with nothing after it.
pub fn relay_origin(input: &str) -> Option<String> {
    let url = url::Url::parse(input.trim()).ok()?;
    let bare = url.scheme() == "https" && url.username().is_empty() && url.password().is_none() && url.query().is_none() && url.fragment().is_none() && url.path().chars().all(|c| c == '/');
    bare.then(|| url.origin().ascii_serialization())
}

/// `environmentEndpointUrl`: a path on an environment's HTTP base.
pub fn endpoint_url(http_base: &str, path: &str) -> Result<String, Failure> {
    url::Url::parse(http_base).and_then(|base| base.join(path)).map(|u| u.to_string()).map_err(|e| Failure::new(format!("Invalid environment address: {e}")))
}

pub fn token_form(clerk_jwt: &str, relay: &str, scope: &str) -> Vec<(&'static str, String)> {
    vec![
        ("grant_type", TOKEN_EXCHANGE.into()),
        ("subject_token", clerk_jwt.into()),
        ("subject_token_type", JWT.into()),
        ("requested_token_type", ACCESS_TOKEN.into()),
        ("resource", relay.into()),
        ("scope", scope.into()),
        ("client_id", CLIENT_ID.into()),
    ]
}

pub fn environment_token_form(credential: &str) -> Vec<(&'static str, String)> {
    vec![
        ("grant_type", TOKEN_EXCHANGE.into()),
        ("subject_token", credential.into()),
        ("subject_token_type", ENVIRONMENT_BOOTSTRAP.into()),
        ("requested_token_type", ACCESS_TOKEN.into()),
        ("client_label", "S5 Code (Rune)".into()),
        ("client_device_type", "desktop".into()),
        ("client_os", std::env::consts::OS.into()),
    ]
}

/// An access token and when it lapses, in seconds from the Unix epoch.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub access: String,
    pub expires_at: u64,
}

impl Token {
    /// Renewed a minute early, like web's `DPOP_ACCESS_TOKEN_REFRESH_SKEW_MS`.
    pub fn fresh(&self, now: u64) -> bool {
        self.expires_at > now + 60
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}

/// A relay `RelayDpopAccessTokenResponse`, refused unless it grants the scope asked for.
pub fn relay_token(response: &Value, scope: &str, now: u64) -> Result<Token, String> {
    let access = response["access_token"].as_str().filter(|t| !t.is_empty()).ok_or("The relay returned no access token.")?;
    if response["token_type"] != "DPoP" {
        return Err("The relay returned a token that is not bound to this device.".into());
    }
    let mut granted: Vec<&str> = response["scope"].as_str().unwrap_or_default().split_whitespace().collect();
    let mut asked: Vec<&str> = scope.split_whitespace().collect();
    granted.sort_unstable();
    asked.sort_unstable();
    if granted != asked {
        return Err(format!("The relay granted \"{}\" instead of \"{scope}\".", response["scope"].as_str().unwrap_or_default()));
    }
    Ok(Token { access: access.to_owned(), expires_at: now + response["expires_in"].as_u64().unwrap_or(0) })
}

/// An environment linked to the account (`RelayClientEnvironmentRecord`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Linked {
    pub id: String,
    pub label: String,
    pub http: String,
    pub ws: String,
    /// `linkedAt`, ISO-8601.
    pub linked_at: String,
    /// The endpoint is a managed (`cloudflare_tunnel`) tunnel, not only activity publishing.
    pub managed: bool,
}

pub fn linked(response: &Value) -> Result<Vec<Linked>, String> {
    let rows = response["environments"].as_array().ok_or("The relay returned no environment list.")?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let field = |v: &Value| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
            Some(Linked {
                id: field(&row["environmentId"])?,
                label: field(&row["label"]).unwrap_or_default(),
                http: field(&row["endpoint"]["httpBaseUrl"])?,
                ws: field(&row["endpoint"]["wsBaseUrl"])?,
                linked_at: field(&row["linkedAt"]).unwrap_or_default(),
                managed: row["endpoint"]["providerKind"] == "cloudflare_tunnel",
            })
        })
        .collect())
}

/// A phone registered for the account's notifications (`RelayClientDeviceRecord`).
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub id: String,
    pub label: String,
    pub android: bool,
    pub ios_major: Option<u64>,
    pub app_version: Option<String>,
    pub notifications: bool,
    /// Approvals, input requests, completions, failures.
    pub alerts: [bool; 4],
    pub live_activities: bool,
    /// `updatedAt`, ISO-8601.
    pub updated_at: String,
}

pub fn devices(response: &Value) -> Result<Vec<Device>, String> {
    let rows = response["devices"].as_array().ok_or("The relay returned no device list.")?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let field = |v: &Value| v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
            let on = |v: &Value| v.as_bool().unwrap_or(false);
            let n = &row["notifications"];
            Some(Device {
                id: field(&row["deviceId"])?,
                label: field(&row["label"])?,
                android: row["platform"] == "android",
                ios_major: row["iosMajorVersion"].as_u64(),
                app_version: field(&row["appVersion"]),
                notifications: on(&n["enabled"]),
                alerts: [on(&n["notifyOnApproval"]), on(&n["notifyOnInput"]), on(&n["notifyOnCompletion"]), on(&n["notifyOnFailure"])],
                live_activities: on(&row["liveActivities"]["enabled"]),
                updated_at: field(&row["updatedAt"]).unwrap_or_default(),
            })
        })
        .collect())
}

impl Device {
    /// `mobileClientPlatformLabel`.
    pub fn platform(&self) -> String {
        let platform = match (self.android, self.ios_major) {
            (true, _) => "Android".to_owned(),
            (false, None) => "iOS".to_owned(),
            (false, Some(major)) => format!("iOS {major}"),
        };
        match &self.app_version {
            Some(version) => format!("{platform} · T3 Code {version}"),
            None => platform,
        }
    }

    /// `mobileClientNotificationDetail`.
    pub fn detail(&self) -> String {
        if !self.notifications {
            return "Push notifications are disabled on this device.".into();
        }
        let names = ["approvals", "input requests", "completions", "failures"];
        let on: Vec<&str> = names.iter().zip(self.alerts).filter(|(_, on)| *on).map(|(name, _)| *name).collect();
        if on.is_empty() { "Push notifications are enabled, but no alert types are selected.".into() } else { format!("Alerts enabled for {}.", on.join(", ")) }
    }
}

/// What `connectEnvironment` returns: where the environment is and a credential to spend there.
#[derive(Clone, Debug, PartialEq)]
pub struct Bootstrap {
    pub environment: Linked,
    pub credential: String,
}

pub fn bootstrap(response: &Value, expected: &str) -> Result<Bootstrap, String> {
    let id = response["environmentId"].as_str().unwrap_or_default();
    if id != expected {
        return Err(format!("Connected environment {id} does not match {expected}."));
    }
    let field = |v: &Value, what: &str| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned).ok_or(format!("The relay returned no {what}."));
    Ok(Bootstrap {
        environment: Linked { id: id.to_owned(), http: field(&response["endpoint"]["httpBaseUrl"], "endpoint")?, ws: field(&response["endpoint"]["wsBaseUrl"], "endpoint")?, ..Linked::default() },
        credential: field(&response["credential"], "credential")?,
    })
}

/// A relay `RelayEnvironmentStatusResponse` for a listed environment.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub online: bool,
    /// `relayOfflineReasonMessage`: why an offline environment will not come back by itself.
    pub offline_reason: Option<String>,
    pub error: Option<String>,
    pub trace: Option<String>,
}

const TUNNEL_RELEASED: &str = "Offline for a while, so its T3 Connect tunnel was removed. Start T3 Code on that computer and update it to the latest version to reconnect.";

/// `validateStatus`: refused unless it is about the environment and endpoint listed.
pub fn status(response: &Value, listed: &Linked) -> Result<Status, String> {
    if response["environmentId"] != listed.id.as_str() {
        return Err("Relay returned status for a different environment.".into());
    }
    if response["endpoint"]["httpBaseUrl"] != listed.http.as_str() || response["endpoint"]["wsBaseUrl"] != listed.ws.as_str() {
        return Err("Relay returned status for a different environment endpoint.".into());
    }
    if response["descriptor"].is_object() && response["descriptor"]["environmentId"] != listed.id.as_str() {
        return Err("Relay returned a descriptor for a different environment.".into());
    }
    let text = |v: &Value| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    let online = match response["status"].as_str() {
        Some("online") => true,
        Some("offline") => false,
        _ => return Err("Relay returned an unknown environment status.".into()),
    };
    let offline_reason = (!online && response["offlineReason"] == "tunnel_released").then(|| TUNNEL_RELEASED.to_owned());
    Ok(Status { online, offline_reason, error: text(&response["error"]), trace: text(&response["traceId"]) })
}

/// `rpcSocketUrl` plus the ticket, and the upgrade proof's target: the socket URL as the HTTP
/// request the server sees, without its query.
pub fn socket_target(ws_base: &str) -> Result<(url::Url, String), Failure> {
    let mut url = url::Url::parse(ws_base).map_err(|e| Failure::new(format!("Invalid environment address: {e}")))?;
    let scheme = match url.scheme() {
        "wss" | "https" => "wss",
        "ws" | "http" => "ws",
        _ => return Err(Failure::new("Unsupported environment address.")),
    };
    let _ = url.set_scheme(scheme);
    if url.path().is_empty() || url.path() == "/" {
        url.set_path("/ws");
    }
    url.set_query(None);
    url.set_fragment(None);
    let host = url.host_str().unwrap_or_default();
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    let target = format!("{}://{host}{port}{}", if scheme == "wss" { "https" } else { "http" }, url.path());
    Ok((url, target))
}

/// The socket URL: the ticket and its upgrade proof, plus `appendClientConnectionParams`.
pub fn socket_url(ws_base: &str, ticket: &str, proof: &dyn Fn(&str) -> Result<String, String>) -> Result<String, Failure> {
    let (mut url, target) = socket_target(ws_base)?;
    let upgrade = proof(&target).map_err(Failure::new)?;
    url.query_pairs_mut()
        .append_pair("wsTicket", ticket)
        .append_pair("dpop", &upgrade)
        .append_pair("clientSurface", "desktop")
        .append_pair("clientDeviceType", "desktop")
        .append_pair("clientOs", std::env::consts::OS)
        .append_pair("connectionMethod", "relay");
    Ok(url.to_string())
}

/// Requests against one relay with one device key.
pub struct Relay<'a> {
    pub origin: String,
    pub key: &'a Key,
}

impl Relay<'_> {
    fn proof(&self, method: &str, url: &str, token: Option<&str>) -> Result<String, Failure> {
        self.key.proof(method, url, token).map_err(Failure::new)
    }

    pub fn exchange(&self, clerk_jwt: &str, scope: &str) -> Result<Token, Failure> {
        let action = "exchange relay DPoP access token";
        let url = format!("{}/v1/client/dpop-token", self.origin);
        let form = token_form(clerk_jwt, &self.origin, scope);
        let pairs: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let response = agent().post(&url).set("DPoP", &self.proof("POST", &url, None)?).send_form(&pairs).map_err(|e| failure(action, e))?;
        relay_token(&read_json(action, response)?, scope, now()).map_err(Failure::new)
    }

    pub fn connect(&self, relay: &Token, environment: &str) -> Result<Bootstrap, Failure> {
        let action = "connect relay environment";
        let url = format!("{}/v1/environments/{environment}/connect", self.origin);
        let response = agent()
            .post(&url)
            .set("Authorization", &format!("DPoP {}", relay.access))
            .set("DPoP", &self.proof("POST", &url, Some(&relay.access))?)
            .send_json(json!({"clientKeyThumbprint": self.key.thumbprint()}))
            .map_err(|e| failure(action, e))?;
        bootstrap(&read_json(action, response)?, environment).map_err(Failure::new)
    }

    /// `getEnvironmentStatus`, with a token granted [`STATUS_SCOPE`].
    pub fn status(&self, relay: &Token, environment: &Linked) -> Result<Status, Failure> {
        let action = "check environment status";
        let url = format!("{}/v1/environments/{}/status", self.origin, environment.id);
        let response = agent()
            .post(&url)
            .set("Authorization", &format!("DPoP {}", relay.access))
            .set("DPoP", &self.proof("POST", &url, Some(&relay.access))?)
            .call()
            .map_err(|e| failure(action, e))?;
        status(&read_json(action, response)?, environment).map_err(Failure::new)
    }
}

/// The account's linked environments, with the Clerk session as a bearer token.
pub fn environments(origin: &str, clerk_jwt: &str) -> Result<Vec<Linked>, Failure> {
    let action = "list T3 Connect environments";
    let response = agent().get(&format!("{origin}/v1/environments")).set("Authorization", &format!("Bearer {clerk_jwt}")).call().map_err(|e| failure(action, e))?;
    linked(&read_json(action, response)?).map_err(Failure::new)
}

/// The account's mobile clients (`listDevicesV2`), with the Clerk session as a bearer token.
pub fn client_devices(origin: &str, clerk_jwt: &str) -> Result<Vec<Device>, Failure> {
    let action = "list mobile clients";
    let response = agent().get(&format!("{origin}/v2/client/devices")).set("Authorization", &format!("Bearer {clerk_jwt}")).call().map_err(|e| failure(action, e))?;
    devices(&read_json(action, response)?).map_err(Failure::new)
}

/// `unlinkEnvironment`: revokes the environment's T3 Connect access and frees its host space.
pub fn deregister(origin: &str, clerk_jwt: &str, environment: &str) -> Result<(), Failure> {
    let action = "deregister the server";
    agent().delete(&format!("{origin}/v1/client/environment-links/{environment}")).set("Authorization", &format!("Bearer {clerk_jwt}")).call().map_err(|e| failure(action, e))?;
    Ok(())
}

/// The environment's own DPoP access token, for a bootstrap the relay handed out.
pub fn environment_token(key: &Key, bootstrap: &Bootstrap) -> Result<Token, Failure> {
    let action = "authorize the environment";
    let url = endpoint_url(&bootstrap.environment.http, "/oauth/token")?;
    let form = environment_token_form(&bootstrap.credential);
    let pairs: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let response = agent().post(&url).set("DPoP", &key.proof("POST", &url, None).map_err(Failure::new)?).send_form(&pairs).map_err(|e| failure(action, e))?;
    let body = read_json(action, response)?;
    let access = body["access_token"].as_str().filter(|t| !t.is_empty()).ok_or_else(|| Failure::new("The environment returned no access token."))?;
    Ok(Token { access: access.to_owned(), expires_at: now() + body["expires_in"].as_f64().unwrap_or(0.0) as u64 })
}

/// A fresh socket URL on an environment, from its access token.
pub fn socket(key: &Key, environment: &Linked, token: &Token) -> Result<String, Failure> {
    let action = "open the environment connection";
    let mint = |path: &str| -> Result<ureq::Response, Failure> {
        let url = endpoint_url(&environment.http, path)?;
        let proof = key.proof("POST", &url, Some(&token.access)).map_err(Failure::new)?;
        agent().post(&url).set("Authorization", &format!("DPoP {}", token.access)).set("DPoP", &proof).call().map_err(|e| failure(action, e))
    };
    let response = match mint(TICKET_PATH) {
        Err(refused) if refused.status == Some(404) => mint(LEGACY_TICKET_PATH),
        other => other,
    }?;
    let ticket = read_json(action, response)?["ticket"].as_str().filter(|t| !t.is_empty()).map(str::to_owned).ok_or_else(|| Failure::new("The environment returned no connection ticket."))?;
    socket_url(&environment.ws, &ticket, &|target| key.proof("GET", target, Some(&ticket)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::dpop::verify;

    #[test]
    fn relay_urls_are_bare_https_origins() {
        assert_eq!(relay_origin("https://relay.s5code.dpdns.org/").as_deref(), Some("https://relay.s5code.dpdns.org"));
        assert_eq!(relay_origin("http://relay.example.com"), None);
        assert_eq!(relay_origin("https://relay.example.com/v1"), None);
        assert_eq!(relay_origin("https://user@relay.example.com"), None);
    }

    #[test]
    fn the_token_exchange_is_the_form_managed_relay_posts() {
        let form = token_form("clerk.jwt", "https://relay.s5code.dpdns.org", CONNECT_SCOPE);
        let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form.iter().map(|(k, v)| (*k, v.as_str()))).finish();
        assert_eq!(body, "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Atoken-exchange&subject_token=clerk.jwt&subject_token_type=urn%3Aietf%3Aparams%3Aoauth%3Atoken-type%3Ajwt&requested_token_type=urn%3Aietf%3Aparams%3Aoauth%3Atoken-type%3Aaccess_token&resource=https%3A%2F%2Frelay.s5code.dpdns.org&scope=environment%3Aconnect&client_id=t3-web");
        let environment = environment_token_form("bootstrap-1");
        assert_eq!(environment[2], ("subject_token_type", "urn:t3:params:oauth:token-type:environment-bootstrap".into()));
    }

    #[test]
    fn mobile_clients_parse_and_read_like_web() {
        let list = json!({"devices": [
            {"deviceId": "d-1", "label": "Pixel", "platform": "android", "iosMajorVersion": null, "androidApiLevel": 35, "appVersion": "1.4.0",
             "notifications": {"enabled": true, "notifyOnApproval": true, "notifyOnInput": false, "notifyOnCompletion": true, "notifyOnFailure": true},
             "liveActivities": {"enabled": false}, "updatedAt": "2026-11-20T10:05:00.000Z"},
            {"deviceId": "d-2", "label": "iPhone", "platform": "ios", "iosMajorVersion": 26, "appVersion": null,
             "notifications": {"enabled": true, "notifyOnApproval": false, "notifyOnInput": false, "notifyOnCompletion": false, "notifyOnFailure": false},
             "liveActivities": {"enabled": true}, "updatedAt": "2026-11-21T08:00:00.000Z"},
            {"deviceId": "d-3", "label": "Old", "platform": "ios", "iosMajorVersion": null, "appVersion": "1.0",
             "notifications": {"enabled": false, "notifyOnApproval": true, "notifyOnInput": true, "notifyOnCompletion": true, "notifyOnFailure": true},
             "liveActivities": {"enabled": false}, "updatedAt": "2026-11-21T08:00:00.000Z"},
            {"deviceId": " ", "label": "No id"}]});
        let devices = devices(&list).unwrap();
        assert_eq!(devices.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["d-1", "d-2", "d-3"]);
        assert_eq!(devices.iter().map(Device::platform).collect::<Vec<_>>(), ["Android · T3 Code 1.4.0", "iOS 26", "iOS · T3 Code 1.0"]);
        assert_eq!(devices[0].detail(), "Alerts enabled for approvals, completions, failures.");
        assert_eq!(devices[1].detail(), "Push notifications are enabled, but no alert types are selected.");
        assert_eq!(devices[2].detail(), "Push notifications are disabled on this device.");
        assert!(devices[1].live_activities && !devices[0].live_activities);
        assert!(super::devices(&json!({})).is_err());
    }

    #[test]
    fn relay_replies_parse_and_wrong_grants_are_refused() {
        let reply = json!({"access_token": "rat-1", "issued_token_type": ACCESS_TOKEN, "token_type": "DPoP", "expires_in": 600, "scope": "environment:connect"});
        let token = relay_token(&reply, CONNECT_SCOPE, 1_000).unwrap();
        assert_eq!(token, Token { access: "rat-1".into(), expires_at: 1_600 });
        assert!(token.fresh(1_500) && !token.fresh(1_541));
        assert!(relay_token(&json!({"access_token": "x", "token_type": "Bearer", "expires_in": 1, "scope": "environment:connect"}), CONNECT_SCOPE, 0).is_err());
        assert!(relay_token(&json!({"access_token": "x", "token_type": "DPoP", "expires_in": 1, "scope": "environment:status"}), CONNECT_SCOPE, 0).is_err());
        let list = json!({"environments": [
            {"environmentId": "env-1", "label": "Studio Mac", "endpoint": {"httpBaseUrl": "https://env-1.tunnel.example.com/", "wsBaseUrl": "wss://env-1.tunnel.example.com/", "providerKind": "cloudflare"}, "linkedAt": "2026-11-01T00:00:00.000Z"},
            {"environmentId": "env-2", "label": "Broken"}]});
        let studio = Linked { id: "env-1".into(), label: "Studio Mac".into(), http: "https://env-1.tunnel.example.com/".into(), ws: "wss://env-1.tunnel.example.com/".into(), linked_at: "2026-11-01T00:00:00.000Z".into(), managed: false };
        assert_eq!(linked(&list).unwrap(), vec![studio.clone()]);
        let tunnel = json!({"environments": [{"environmentId": "env-1", "label": "Studio Mac", "endpoint": {"httpBaseUrl": "https://env-1.tunnel.example.com/", "wsBaseUrl": "wss://env-1.tunnel.example.com/", "providerKind": "cloudflare_tunnel"}, "linkedAt": "2026-11-01T00:00:00.000Z"}]});
        assert_eq!(linked(&tunnel).unwrap(), vec![Linked { managed: true, ..studio }], "only a cloudflare_tunnel endpoint is a managed tunnel");
        let listed = &linked(&list).unwrap()[0];
        let online = json!({"environmentId": "env-1", "endpoint": {"httpBaseUrl": "https://env-1.tunnel.example.com/", "wsBaseUrl": "wss://env-1.tunnel.example.com/", "providerKind": "cloudflare"}, "status": "online", "checkedAt": "2026-11-25T00:00:00.000Z"});
        assert_eq!(status(&online, listed).unwrap(), Status { online: true, offline_reason: None, error: None, trace: None });
        let released = json!({"environmentId": "env-1", "endpoint": online["endpoint"], "status": "offline", "checkedAt": "x", "offlineReason": "tunnel_released", "error": "Tunnel not found", "traceId": "0af7651916cd43dd8448eb211c80319c"});
        let offline = status(&released, listed).unwrap();
        assert!(!offline.online && offline.offline_reason.as_deref().is_some_and(|m| m.starts_with("Offline for a while")));
        assert_eq!((offline.error.as_deref(), offline.trace.as_deref()), (Some("Tunnel not found"), Some("0af7651916cd43dd8448eb211c80319c")));
        let moved = json!({"environmentId": "env-1", "endpoint": {"httpBaseUrl": "https://elsewhere.example.com/", "wsBaseUrl": "wss://elsewhere.example.com/"}, "status": "online", "checkedAt": "x"});
        assert_eq!(status(&moved, listed).unwrap_err(), "Relay returned status for a different environment endpoint.");
        assert!(status(&json!({"environmentId": "env-2", "endpoint": online["endpoint"], "status": "online"}), listed).is_err());
        let granted = json!({"access_token": "rat-2", "token_type": "DPoP", "expires_in": 600, "scope": "environment:connect environment:status"});
        assert!(relay_token(&granted, STATUS_SCOPE, 0).is_ok(), "the grant may list its scopes in any order");
        let connected = json!({"environmentId": "env-1", "endpoint": {"httpBaseUrl": "https://env-1.tunnel.example.com/", "wsBaseUrl": "wss://env-1.tunnel.example.com/", "providerKind": "cloudflare"}, "credential": "boot-1", "expiresAt": "2026-11-22T00:05:00.000Z"});
        assert_eq!(bootstrap(&connected, "env-1").unwrap().credential, "boot-1");
        assert_eq!(bootstrap(&connected, "env-9").unwrap_err(), "Connected environment env-1 does not match env-9.");
    }

    #[test]
    fn the_socket_url_carries_a_ticket_and_an_upgrade_proof_for_it() {
        let key = Key::generate().unwrap();
        let url = socket_url("wss://env-1.tunnel.example.com/", "ticket-7", &|target| key.proof("GET", target, Some("ticket-7"))).unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        assert_eq!((parsed.scheme(), parsed.path()), ("wss", "/ws"));
        let param = |name: &str| parsed.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned());
        assert_eq!(param("wsTicket").as_deref(), Some("ticket-7"));
        assert_eq!(param("connectionMethod").as_deref(), Some("relay"));
        let proof = param("dpop").unwrap();
        assert_eq!(verify(&proof, "GET", "https://env-1.tunnel.example.com/ws", Some("ticket-7")), Ok(key.thumbprint()), "the server checks the upgrade as an https GET of the socket path");
        assert_eq!(socket_target("ws://192.168.1.4:3773/custom").unwrap().1, "http://192.168.1.4:3773/custom");
    }
}
