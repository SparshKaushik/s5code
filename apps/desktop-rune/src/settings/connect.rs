//! Web `CloudLinkRow` on this machine's card: its T3 Connect tunnel, publishing agent activity, and
//! holding webhooks while offline. State and preferences are the environment's `/api/connect/*`
//! routes; linking runs web's `linkPrimaryEnvironmentToCloud` against the relay with the signed-in
//! account (`crate::cloud`).
use crate::theme::{self, AlphaMul, T};
use crate::{App, kit};
use rune::{El, col, el, row, text};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Slot<O> = Arc<Mutex<Option<O>>>;

/// Web re-reads the link state every 30 s while the row is mounted, as the server rechecks the
/// tunnel's reachability.
const REFRESH: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct Ui {
    /// `GET /api/connect/link-state`: `None` until it answers.
    state: Option<Result<Value, String>>,
    read: Option<u64>,
    /// The switch being applied, and its background request.
    busy: Option<(Change, Slot<Result<(), Failure>>)>,
    /// Web `operationError`, shown under the row until the next change.
    error: Option<String>,
    /// The account's tunnel cap after the relay refused a tunnel at it.
    tunnel_limit: Option<u64>,
    /// What onboarding's Continue asked for, applying (`crate::cloud::onboarding`).
    onboarding: Option<Slot<Result<(), Failure>>>,
    /// Tunnels in use when the relay refused, so only a freed tunnel clears the alert.
    used_at_refusal: Option<u64>,
    /// Web `useManagedTunnelUsage`, and its request in flight.
    usage: Option<Usage>,
    usage_read: Option<Slot<Option<Usage>>>,
    /// When the next link-state read is due; a sleeping thread wakes the app then.
    next_read: Option<Instant>,
    /// Web `openAuthPrompt`: the sign-in dialog.
    prompt: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Usage {
    pub used: u64,
    pub limit: u64,
    /// `ManagedTunnelUpgrade`: `link`, `pro` or `premier`.
    pub upgrade: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    Tunnel(bool),
    Publish(bool),
    Hold(bool),
}

#[derive(Debug, PartialEq)]
enum Plan {
    Unlink,
    /// Link (again) in this mode, then save the preferences.
    Link { managed: bool, publish: bool, hold: Option<bool> },
    Preferences { publish: bool, hold: Option<bool> },
}

struct Failure {
    message: String,
    tunnel_limit: Option<u64>,
}

impl Failure {
    fn new(message: impl Into<String>) -> Self {
        Failure { message: message.into(), tunnel_limit: None }
    }
}

/// Web `isManagedTunnelActive`: an older server reports only `linked`.
fn managed_active(state: &Value) -> bool {
    state["linkRevoked"] != true && state["managedTunnelActive"].as_bool().unwrap_or(state["linked"] == true)
}

/// Web `reconcileCloudState`: unlink when nothing stays on, link again when the tunnel mode
/// changes or the link is gone, else only the preferences change.
fn plan(state: &Value, change: Change) -> Plan {
    let (mut managed, mut publish, mut hold) = (managed_active(state), state["publishAgentActivity"] == true, None);
    match change {
        Change::Tunnel(on) => managed = on,
        Change::Publish(on) => publish = on,
        Change::Hold(on) => hold = Some(on),
    }
    if !managed && !publish {
        return Plan::Unlink;
    }
    if state["linked"] != true || state["linkRevoked"] == true || managed_active(state) != managed {
        return Plan::Link { managed, publish, hold };
    }
    Plan::Preferences { publish, hold }
}

/// Web's success toast for each switch.
fn success(change: Change, publish: bool) -> (&'static str, &'static str) {
    match change {
        Change::Tunnel(true) => ("T3 Connect linked", "This environment is available through T3 Connect."),
        Change::Tunnel(false) if publish => ("T3 Connect tunnel disabled", "The managed tunnel was removed. Agent activity publishing stays on."),
        Change::Tunnel(false) => ("T3 Connect unlinked", "This environment is no longer available through T3 Connect."),
        Change::Publish(true) => ("Agent activity enabled", "This environment publishes agent activity to your mobile clients."),
        Change::Publish(false) => ("Agent activity disabled", "This environment will stop publishing agent activity."),
        Change::Hold(true) => ("Webhooks held while offline", "T3 Connect keeps webhook requests for up to 24 hours while this environment is offline."),
        Change::Hold(false) => ("Webhooks no longer held", "Requests to an offline environment now fail. Anything already held is still delivered."),
    }
}

/// The T3 Connect row's description, as web picks it.
fn description(signed_in: bool, state: Option<&Value>) -> &'static str {
    let Some(state) = state.filter(|_| signed_in) else {
        return if signed_in {
            "Make this environment available to your other devices through T3 Connect."
        } else {
            "Sign in to make this environment available to your other devices through T3 Connect."
        };
    };
    if state["linkRevoked"] == true {
        "This environment was removed from T3 Connect. Turn it on to link it again."
    } else if managed_active(state) {
        match state["reachability"]["status"].as_str() {
            Some("unreachable") => "T3 Connect is on, but other devices cannot reach this environment.",
            Some("reachable") => "This environment is available to your other devices through T3 Connect.",
            _ => "T3 Connect is on. Reachability has not been checked yet.",
        }
    } else if state["managedTunnelActive"] == false && state["linked"] == true {
        "T3 Connect is out of sync. This environment is linked only to publish agent activity, so other devices cannot connect. Turn T3 Connect on to repair it."
    } else {
        "Make this environment available to your other devices through T3 Connect."
    }
}

/// Web `formatConnectReachable`, or the unreachable check's reason, hint and fix.
fn reachability(state: &Value) -> Option<(String, bool)> {
    let r = &state["reachability"];
    match r["status"].as_str()? {
        "reachable" => {
            let mut parts = vec!["Tunnel connected".to_owned()];
            if let Some(n) = r["readyConnections"].as_i64() {
                parts.push(format!("{n} connection{}", if n == 1 { "" } else { "s" }));
            }
            if let Some(protocol) = r["protocol"].as_str() {
                parts.push(protocol.to_uppercase());
            }
            Some((parts.join(" · "), false))
        }
        "unreachable" => {
            let fix = r["fix"].as_str().map(|f| format!("Fix: {f}"));
            let parts: Vec<String> = [r["reason"].as_str().map(str::to_owned), r["hint"].as_str().map(str::to_owned), fix].into_iter().flatten().filter(|p| !p.is_empty()).collect();
            Some((parts.join(" "), true))
        }
        _ => None,
    }
}

fn signed_in(s: &App) -> bool {
    s.ui.cloud.identity.is_some()
}

/// Reads the link state while the rows show; signed out there is nothing to read.
fn refresh(s: &mut App) {
    if s.ui.settings.connect.read.is_some() {
        return;
    }
    if let Some(net) = s.src.net() {
        s.ui.settings.connect.read = Some(net.http_request("GET", "/api/connect/link-state", json!({})));
    }
}

/// Each visit reads the link state again, as web's mount revalidates it.
pub fn reset(s: &mut App) {
    let ui = &mut s.ui.settings.connect;
    if ui.busy.is_none() {
        (ui.state, ui.error, ui.tunnel_limit, ui.used_at_refusal, ui.usage, ui.next_read) = (None, None, None, None, None, None);
    }
}

fn toggle(s: &mut App, change: Change) {
    let ui = &mut s.ui.settings.connect;
    if ui.busy.is_some() {
        return;
    }
    let Some(Ok(state)) = ui.state.clone() else { return };
    ui.error = None;
    (ui.tunnel_limit, ui.used_at_refusal) = (None, None);
    if !super::permissions::can(s, "relay:read") || !super::permissions::can(s, "relay:write") {
        fail(s, Failure::new("This connection needs permission to view and manage T3 Connect settings."));
        return;
    }
    let Some(slot) = start(s, plan(&state, change)) else { return };
    s.ui.settings.connect.busy = Some((change, slot));
}

/// Runs `plan` in the background; a managed tunnel first needs the relay client (web
/// `ensureRelayClientAvailable`), which may ask to install it.
fn start(s: &mut App, plan: Plan) -> Option<Slot<Result<(), Failure>>> {
    let net = s.src.net()?;
    let (base, auth) = (net.base_url().to_owned(), net.auth().clone());
    let environment = s.state().config.as_ref().and_then(|c| c["environment"]["environmentId"].as_str()).unwrap_or_default().to_owned();
    let relay = crate::cloud::config().map(|c| c.relay);
    let slot: Slot<Result<(), Failure>> = Arc::default();
    let (out, waker) = (slot.clone(), s.ui.panel.waker.clone());
    let managed = matches!(plan, Plan::Link { managed: true, .. });
    let link = move || {
        std::thread::spawn(move || {
            let result = run(&plan, &base, &auth, &environment, relay.as_deref());
            *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
            if let Some(waker) = waker {
                waker();
            }
        });
    };
    if managed {
        let (failed, key) = (slot.clone(), s.ui.env.active.clone());
        crate::cloud::relay_client::ensure(s, &key, move |_, ready| match ready {
            Ok(()) => link(),
            Err(message) => *failed.lock().unwrap_or_else(|e| e.into_inner()) = Some(Err(Failure::new(message))),
        });
    } else {
        link();
    }
    Some(slot)
}

/// Onboarding's Continue (web `reconcileCloudState`): it only turns links on.
fn onboarding_plan(state: &Value, managed: bool, publish: bool) -> Plan {
    if state["linked"] != true || state["linkRevoked"] == true || managed_active(state) != managed {
        return Plan::Link { managed, publish, hold: None };
    }
    Plan::Preferences { publish, hold: None }
}

/// What onboarding needs from the link state: whose account it is linked to and what it publishes.
fn link_state(state: &Value) -> crate::cloud::onboarding::LinkState {
    let linked = state["linked"] == true && state["linkRevoked"] != true;
    crate::cloud::onboarding::LinkState {
        linked_user: state["cloudUserId"].as_str().filter(|_| linked).map(str::to_owned),
        managed_tunnel: state["managedTunnelActive"].as_bool().unwrap_or(state["linked"] == true),
        publish_activity: state["publishAgentActivity"] == true,
    }
}

/// Web `managedTunnelCap`: a manual override is exact, else the T3 Chat plan raises the default.
fn cap(t3chat: &Value) -> (u64, Option<&'static str>) {
    if let Some(limit) = t3chat["maxTunnelsOverride"].as_u64() {
        return (limit, None);
    }
    if t3chat["available"] != true {
        return (3, None);
    }
    match t3chat["link"]["plan"].as_str() {
        Some("premier") => (100, None),
        Some("pro") => (10, Some("premier")),
        Some("free") => (3, Some("pro")),
        _ => (3, Some("link")),
    }
}

fn tunnels_in_use(environments: &Value) -> u64 {
    environments["environments"].as_array().into_iter().flatten().filter(|e| e["endpoint"]["providerKind"] == "cloudflare_tunnel").count() as u64
}

/// Web `refreshManagedTunnelUsage`: the account's environments and its T3 Chat plan.
fn read_usage(s: &mut App) {
    let Some(relay) = crate::cloud::config().map(|c| c.relay) else { return };
    let slot: Slot<Option<Usage>> = Arc::default();
    let (out, waker) = (slot.clone(), s.ui.panel.waker.clone());
    std::thread::spawn(move || {
        let get = |token: &str, path: &str| -> Option<Value> {
            let url = format!("{}{path}", relay.trim_end_matches('/'));
            agent().get(&url).set("Authorization", &format!("Bearer {token}")).call().ok()?.into_json().ok()
        };
        let usage = crate::cloud::account_token().ok().and_then(|token| {
            let used = tunnels_in_use(&get(&token, "/v1/environments")?);
            let (limit, upgrade) = cap(&get(&token, "/v1/client/t3chat-link")?);
            Some(Usage { used, limit, upgrade })
        });
        *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(usage);
        if let Some(waker) = waker {
            waker();
        }
    });
    s.ui.settings.connect.usage_read = Some(slot);
}

/// Wakes the app when the next link-state read is due.
fn schedule_read(s: &mut App) {
    s.ui.settings.connect.next_read = Some(Instant::now() + REFRESH);
    let waker = s.ui.panel.waker.clone();
    std::thread::spawn(move || {
        std::thread::sleep(REFRESH);
        if let Some(waker) = waker {
            waker();
        }
    });
}

fn env_post(base: &str, auth: &s5_net::Auth, path: &str, body: Value, action: &str) -> Result<Value, Failure> {
    let url = format!("{}{path}", base.trim_end_matches('/'));
    let request = auth.apply(agent().post(&url)).map_err(Failure::new)?;
    read(request.send_json(body), action)
}

fn relay_post(relay: &str, token: &str, path: &str, body: Value) -> Result<Value, Failure> {
    let url = format!("{}{path}", relay.trim_end_matches('/'));
    let response = agent().post(&url).set("Authorization", &format!("Bearer {token}")).send_json(body);
    read(response, &format!("{url} failed"))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().redirects(0).timeout(std::time::Duration::from_secs(30)).build()
}

/// A reply's JSON, or the failure web reports: its message after the action, and the tunnel cap
/// when the relay refused a link for being at it.
fn read(response: Result<ureq::Response, ureq::Error>, action: &str) -> Result<Value, Failure> {
    match response {
        Ok(response) => Ok(response.into_json().unwrap_or(Value::Null)),
        Err(ureq::Error::Status(code, response)) => {
            let body: Value = response.into_json().unwrap_or(Value::Null);
            Err(failure(action, code, &body))
        }
        Err(error) => Err(Failure::new(format!("{}: {error}", action.trim_end_matches(['.', ':'])))),
    }
}

fn failure(action: &str, code: u16, body: &Value) -> Failure {
    let error = if body["relayError"].is_object() { &body["relayError"] } else { body };
    let detail = error["message"].as_str().map(str::to_owned).unwrap_or_else(|| format!("HTTP {code}"));
    Failure {
        message: format!("{}: {detail}", action.trim_end_matches(['.', ':'])),
        tunnel_limit: (error["_tag"] == "RelayEnvironmentLinkLimitExceededError").then(|| error["maxTunnels"].as_u64()).flatten(),
    }
}

/// The environment's own address as the relay should record it, and the local port it serves.
fn endpoint(base: &str, managed: bool) -> (Value, Value) {
    let ws = base.replacen("https://", "wss://", 1).replacen("http://", "ws://", 1);
    let (https, rest) = base.split_once("://").map_or((false, base), |(scheme, rest)| (scheme == "https", rest));
    let authority = rest.split('/').next().unwrap_or_default();
    let port: u16 = authority.rsplit_once(':').and_then(|(_, p)| p.parse().ok()).unwrap_or(if https { 443 } else { 80 });
    (
        json!({"httpBaseUrl": base, "wsBaseUrl": ws, "providerKind": if managed { "cloudflare_tunnel" } else { "manual" }}),
        json!({"localHttpHost": "127.0.0.1", "localHttpPort": port}),
    )
}

fn run(plan: &Plan, base: &str, auth: &s5_net::Auth, environment: &str, relay: Option<&str>) -> Result<(), Failure> {
    let preferences = |publish: bool, hold: Option<bool>| {
        let mut body = json!({"publishAgentActivity": publish});
        if let Some(hold) = hold {
            body["holdWebhooksWhileOffline"] = json!(hold);
        }
        env_post(base, auth, "/api/connect/preferences", body, "Could not update environment cloud preferences.").map(drop)
    };
    match plan {
        Plan::Preferences { publish, hold } => preferences(*publish, *hold),
        Plan::Unlink => {
            env_post(base, auth, "/api/connect/unlink", json!({}), "Could not unlink the environment from cloud.")?;
            // Web revokes the relay's record too, and only logs when that fails.
            if let (Some(relay), Ok(token)) = (relay, crate::cloud::account_token()) {
                let url = format!("{}/v1/client/environment-links/{environment}", relay.trim_end_matches('/'));
                let _ = agent().delete(&url).set("Authorization", &format!("Bearer {token}")).call();
            }
            Ok(())
        }
        Plan::Link { managed, publish, hold } => {
            let relay = relay.ok_or_else(|| Failure::new("T3CODE_RELAY_URL is not configured."))?;
            let token = crate::cloud::account_token().map_err(|_| Failure::new("Sign in to T3 Connect before enabling this."))?;
            let flags = json!({"notificationsEnabled": true, "liveActivitiesEnabled": true, "managedTunnelsEnabled": managed});
            let challenge = relay_post(relay, &token, "/v1/client/environment-link-challenges", flags.clone())?;
            let (endpoint, origin) = endpoint(base, *managed);
            let proof = env_post(
                base,
                auth,
                "/api/connect/link-proof",
                json!({"challenge": challenge["challenge"], "relayIssuer": relay, "endpoint": endpoint, "origin": origin}),
                "Could not obtain environment link proof.",
            )?;
            let mut request = flags;
            request["proof"] = proof;
            let link = relay_post(relay, &token, "/v1/client/environment-links", request)?;
            if link["environmentId"] != environment {
                return Err(Failure::new("Relay returned credentials for a different environment."));
            }
            if link["endpoint"]["providerKind"] != endpoint["providerKind"] {
                return Err(Failure::new("Relay returned credentials for a different endpoint provider."));
            }
            env_post(
                base,
                auth,
                "/api/connect/relay-config",
                json!({
                    "relayUrl": relay,
                    "relayIssuer": link["relayIssuer"],
                    "cloudUserId": link["cloudUserId"],
                    "environmentCredential": link["environmentCredential"],
                    "cloudMintPublicKey": link["cloudMintPublicKey"],
                    "endpointRuntime": link["endpointRuntime"],
                }),
                "Could not configure environment relay access.",
            )?;
            preferences(*publish, *hold)
        }
    }
}

/// Web `reportUpdateFailure`, or the upgrade prompt when the relay refused a tunnel at the cap.
fn fail(s: &mut App, failure: Failure) {
    let ui = &mut s.ui.settings.connect;
    if failure.tunnel_limit.is_some() {
        ui.tunnel_limit = failure.tunnel_limit;
        ui.used_at_refusal = ui.usage.map(|u| u.used);
        return;
    }
    ui.error = Some(failure.message.clone());
    crate::shell::palette::toast_error(s, "Could not update T3 Connect", Some(&failure.message));
}

pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    let showing = matches!(&s.route, crate::Route::Settings(p) if p == "access") || s.ui.cloud.onboarding.is_some();
    let ready = showing && crate::cloud::config().is_some() && signed_in(s) && super::permissions::can(s, "relay:read");
    let due = s.ui.settings.connect.next_read.is_some_and(|at| Instant::now() >= at);
    if ready && (s.ui.settings.connect.state.is_none() || due) {
        s.ui.settings.connect.next_read = None;
        refresh(s);
    }
    if ready && s.ui.settings.connect.usage.is_none() && s.ui.settings.connect.usage_read.is_none() {
        read_usage(s);
    }
    let usage = s.ui.settings.connect.usage_read.as_ref().and_then(|slot| slot.lock().unwrap_or_else(|e| e.into_inner()).take());
    if let Some(usage) = usage {
        let ui = &mut s.ui.settings.connect;
        ui.usage_read = None;
        ui.usage = usage;
        if ui.tunnel_limit.is_some() && ui.used_at_refusal.is_none() {
            ui.used_at_refusal = usage.map(|u| u.used);
        }
        changed = true;
    }
    if s.ui.settings.connect.prompt && signed_in(s) {
        s.ui.settings.connect.prompt = false;
        changed = true;
    }
    if let Some(id) = s.ui.settings.connect.read
        && let Some(reply) = s.src.net().and_then(|n| n.take_reply(id))
    {
        let ui = &mut s.ui.settings.connect;
        ui.read = None;
        ui.state = Some(reply.map_err(|e| {
            let message = e["message"].as_str().unwrap_or("The request failed.");
            format!("Could not read environment cloud link state: {message}")
        }));
        let manage = super::permissions::can(s, "relay:read") && super::permissions::can(s, "relay:write");
        let link = s.ui.settings.connect.state.as_ref().and_then(|r| r.as_ref().ok()).filter(|_| manage).map(link_state);
        crate::cloud::onboarding::set_link_state(s, link);
        if showing {
            schedule_read(s);
        }
        changed = true;
    }
    if let Some((managed, publish)) = crate::cloud::onboarding::take_publish(s) {
        let slot = match s.ui.settings.connect.state.clone() {
            Some(Ok(state)) => start(s, onboarding_plan(&state, managed, publish)),
            _ => None,
        };
        match slot {
            Some(slot) => s.ui.settings.connect.onboarding = Some(slot),
            None => crate::cloud::onboarding::published(s, Err("Could not read environment cloud link state.".into())),
        }
        changed = true;
    }
    let applied = s.ui.settings.connect.onboarding.as_ref().and_then(|slot| slot.lock().unwrap_or_else(|e| e.into_inner()).take());
    if let Some(result) = applied {
        s.ui.settings.connect.onboarding = None;
        if let Err(failure) = &result {
            s.ui.settings.connect.tunnel_limit = failure.tunnel_limit;
        }
        crate::cloud::onboarding::published(s, result.map_err(|f| f.message));
        refresh(s);
        crate::cloud::refresh(s);
        changed = true;
    }
    let done = s.ui.settings.connect.busy.as_ref().and_then(|(change, slot)| Some((*change, slot.lock().unwrap_or_else(|e| e.into_inner()).take()?)));
    if let Some((change, result)) = done {
        s.ui.settings.connect.busy = None;
        let publish = s.ui.settings.connect.state.as_ref().and_then(|r| r.as_ref().ok()).is_some_and(|st| st["publishAgentActivity"] == true);
        match result {
            Ok(()) => {
                let (title, description) = success(change, publish);
                crate::shell::palette::toast(s, title, Some(description));
            }
            Err(failure) => fail(s, failure),
        }
        // A failed change may have partly applied: read the state again either way. Any link
        // attempt can change the account's tunnel count.
        refresh(s);
        crate::cloud::refresh(s);
        if s.ui.settings.connect.usage_read.is_none() {
            read_usage(s);
        }
        changed = true;
    }
    changed
}

fn copy(title: &str, description: &str, t: &T) -> El {
    col().grow(1.0).min_w(0.0).gap(4.0).child(text(title.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground)).child(text(description.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap())
}

fn setting_row(title: &str, copy: El, control: Option<El>) -> El {
    let mut r = row().key(super::title_key(title)).w_full().px(16.0).py(12.0).gap(32.0).center_y().child(copy);
    if let Some(control) = control {
        r = r.child(control);
    }
    r
}

/// Web `CloudLinkRow`: nothing without T3 Connect in this build or relay access on this session.
pub fn rows(s: &App, t: &T) -> Vec<El> {
    let (read, write) = (super::permissions::can(s, "relay:read"), super::permissions::can(s, "relay:write"));
    if crate::cloud::config().is_none() || !(read || write) {
        return vec![];
    }
    if !read {
        return vec![setting_row("T3 Connect", copy("T3 Connect", "To edit these settings, pair this connection with both View relay and Manage relay permissions.", t), None)];
    }
    let ui = &s.ui.settings.connect;
    let signed_in = signed_in(s);
    let state = ui.state.as_ref().and_then(|r| r.as_ref().ok());
    let busy = ui.busy.as_ref().map(|(change, _)| *change);
    let disabled = !write || !signed_in || state.is_none() || busy.is_some();
    let reason = (!write).then_some("Your session does not have permission to manage T3 Connect access.");
    let switch = |on: bool, label: &str, change: fn(bool) -> Change| {
        let mut control = kit::Switch::new(on).label(label).hide_label().disabled(disabled).on_toggle(move |s: &mut App| toggle(s, change(!on))).build(&kit::theme(t));
        if let Some(reason) = reason {
            control = super::permissions::inert(label, control, reason.to_owned());
        }
        control
    };
    let sign_in = || {
        kit::Button::new("Sign in")
            .variant("outline")
            .size("sm")
            .on_press(|s: &mut App| s.ui.settings.connect.prompt = true)
            .build(t)
    };
    let managed = state.is_some_and(managed_active);
    let mut tunnel_copy = copy("T3 Connect", description(signed_in, state), t);
    let status = ui.error.clone().map(|e| (e, true)).or_else(|| ui.state.as_ref().and_then(|r| r.as_ref().err()).map(|e| (e.clone(), true))).or_else(|| state.filter(|_| managed).and_then(reachability));
    if let Some((status, error)) = status {
        tunnel_copy = tunnel_copy.child(text(status).font(theme::TEXT_XS).color(if error { t.c.destructive } else { t.c.muted_foreground }).wrap());
    }
    if signed_in {
        let alert = limit_alert_shown(ui);
        if let Some(usage) = ui.usage {
            tunnel_copy = tunnel_copy.child(meter(usage, alert, t));
        }
        if alert {
            tunnel_copy = tunnel_copy.child(limit_alert(ui.usage.and_then(|u| u.upgrade), t));
        }
    }
    let mut out = vec![setting_row(
        "T3 Connect",
        tunnel_copy,
        Some(if signed_in { switch(managed, "Enable T3 Connect", Change::Tunnel) } else { sign_in() }),
    )];
    if !signed_in {
        return out;
    }
    let publish = state.is_some_and(|st| st["publishAgentActivity"] == true);
    out.push(setting_row(
        "Publish agent activity",
        copy("Publish agent activity", "Send activity to mobile notifications and Live Activities without T3 Connect.", t),
        Some(switch(publish, "Publish agent activity to mobile clients", Change::Publish)),
    ));
    if managed {
        let hold = state.is_some_and(|st| st["holdWebhooksWhileOffline"] == true);
        out.push(setting_row(
            "Hold webhooks while offline",
            copy(
                "Hold webhooks while offline",
                "Keep webhook requests for up to 24 hours while this environment is offline, then deliver them. Off: T3 Connect only forwards requests and stores nothing.",
                t,
            ),
            Some(switch(hold, "Hold webhook requests while this environment is offline", Change::Hold)),
        ));
    }
    out
}


/// Web `T3ConnectTunnelUsage`: the refusal's alert stays until a tunnel frees or the cap rises.
fn limit_alert_shown(ui: &Ui) -> bool {
    let Some(limit) = ui.tunnel_limit else { return false };
    let freed = matches!((ui.used_at_refusal, ui.usage), (Some(at), Some(usage)) if usage.used < at);
    let raised = ui.usage.is_some_and(|u| u.limit > limit);
    !freed && !raised
}

/// Web `TunnelUsageMeter`: "2 of 3 tunnels in use" with one slot per tunnel up to ten.
fn meter(usage: Usage, alert: bool, t: &T) -> El {
    let full = usage.used >= usage.limit;
    let fill = if full { t.c.warning } else { t.c.foreground.alpha_mul(0.72) };
    let bar = if usage.limit <= 10 {
        (0..usage.limit).fold(row().w_full().h(6.0).gap(4.0), |bar, i| bar.child(el().h(6.0).grow(1.0).radius(3.0).fill(if i < usage.used { fill } else { t.c.muted })))
    } else {
        let share = (usage.used as f32 / usage.limit as f32).min(1.0);
        row().w_full().h(6.0).radius(3.0).fill(t.c.muted).child(el().h(6.0).grow(share).radius(3.0).fill(fill)).child(el().h(6.0).grow(1.0 - share))
    };
    let mut line = row().w_full().gap(12.0).center_y().child(
        text(format!("{} of {} tunnels in use", usage.used, usage.limit)).font(theme::TEXT_XS).color(if full { t.c.warning_foreground } else { t.c.foreground }).grow(1.0),
    );
    // At the cap the alert carries the same action.
    if usage.upgrade.is_some_and(|u| u != "link") && !alert {
        line = line.child(
            text("Get more tunnels")
                .font(theme::weight(theme::TEXT_XS, 500.0))
                .color(t.c.foreground)
                .role("link")
                .cursor(rune::Cursor::Pointer)
                .on_press(|s: &mut App| super::auth::open_url(s, T3CHAT_SUBSCRIPTION_URL)),
        );
    }
    col().max_w(384.0).w_full().gap(6.0).pt(6.0).child(line).child(bar.role("progressbar").label("T3 Connect tunnels in use").range_value(usage.used as f64, 0.0, usage.limit as f64))
}

const T3CHAT_SUBSCRIPTION_URL: &str = "https://t3.chat/settings/subscription";

/// Web `TunnelLimitAlert`. Rune has no account page, so linking T3 Chat or managing tunnels
/// offers no button; an upgrade opens the T3 Chat subscription.
fn limit_alert(upgrade: Option<&str>, t: &T) -> El {
    let description = match upgrade {
        Some("link") => "Switch to Tailscale or get more tunnels by linking your T3 Chat sub (sorry!)",
        Some(_) => "Switch to Tailscale or get more tunnels by upgrading your T3 Chat sub (sorry!)",
        None => "Switch to Tailscale or deregister an environment you no longer use (sorry!)",
    };
    let mut alert = row()
        .w_full()
        .gap(12.0)
        .padding(12.0)
        .radius(8.0)
        .border(1.0, t.c.warning.alpha_mul(0.32))
        .fill(t.c.warning_surface)
        .role("alert")
        .child(rune::icon(&crate::icons::TRIANGLE_ALERT, 16.0).color(t.c.warning_foreground))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text("You\u{2019}re out of tunnels (they cost us money).").font(theme::SM_MEDIUM).color(t.c.warning_foreground))
                .child(text(description).font(theme::TEXT_XS).color(t.c.warning_foreground.alpha_mul(0.8)).wrap()),
        );
    if matches!(upgrade, Some("pro" | "premier")) {
        alert = alert.child(kit::Button::new("Upgrade on T3 Chat").size("sm").on_press(|s: &mut App| super::auth::open_url(s, T3CHAT_SUBSCRIPTION_URL)).build(t));
    }
    alert.mt(6.0)
}

/// Clerk's sign-in prompt (web `openAuthPrompt`), from the settings overlay pass.
pub fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    let ui = &s.ui.settings.connect;
    if ui.prompt && !signed_in(s) {
        let cloud = &s.ui.cloud;
        let mut body = col().w_full().gap(12.0);
        body = match &cloud.signing_in {
            Some(pending) => body.child(
                row()
                    .w_full()
                    .gap(12.0)
                    .center_y()
                    .child(text(if pending.finishing { "Signing in\u{2026}" } else { "Finish signing in in your browser" }).font(theme::TEXT_SM).color(t.c.muted_foreground).grow(1.0))
                    .child(kit::Button::new("Cancel").variant("ghost").size("sm").disabled(pending.finishing).on_press(crate::cloud::cancel_sign_in).build(t)),
            ),
            None => body.child(crate::cloud::provider_buttons(s, t, false)),
        };
        if let Some(error) = &cloud.error {
            body = body.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap());
        }
        kit::Dialog::new("t3-connect-sign-in", "Sign in to T3 Connect")
            .open(true)
            .description("Make this environment available to your other devices through T3 Connect.")
            .on_close(|s: &mut App| s.ui.settings.connect.prompt = false)
            .child(body)
            .build(t, layers);
    }
}

/// Scenes: signed in with this environment linked over a healthy tunnel.
pub fn scene(s: &mut App, state: Value) {
    crate::cloud::scene_signed_in(s);
    s.ui.settings.connect.state = Some(Ok(state));
    s.ui.settings.connect.usage = Some(Usage { used: 2, limit: 3, upgrade: Some("link") });
}

/// Scenes: signed out, with the sign-in prompt open.
pub fn scene_prompt(s: &mut App) {
    s.ui.settings.connect.prompt = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switches_plan_like_reconcile_cloud_state() {
        let tunnel = json!({"linked": true, "managedTunnelActive": true, "publishAgentActivity": true});
        assert_eq!(plan(&tunnel, Change::Hold(true)), Plan::Preferences { publish: true, hold: Some(true) });
        assert_eq!(plan(&tunnel, Change::Publish(false)), Plan::Preferences { publish: false, hold: None });
        // Dropping the tunnel while publishing relinks publish-only.
        assert_eq!(plan(&tunnel, Change::Tunnel(false)), Plan::Link { managed: false, publish: true, hold: None });
        let publish_only = json!({"linked": true, "managedTunnelActive": false, "publishAgentActivity": true});
        assert_eq!(plan(&publish_only, Change::Publish(false)), Plan::Unlink);
        assert_eq!(plan(&publish_only, Change::Tunnel(true)), Plan::Link { managed: true, publish: true, hold: None });
        let unlinked = json!({"linked": false, "publishAgentActivity": false});
        assert_eq!(plan(&unlinked, Change::Publish(true)), Plan::Link { managed: false, publish: true, hold: None });
        // A revoked link counts as off, and turning it on links again.
        let revoked = json!({"linked": true, "linkRevoked": true, "publishAgentActivity": false});
        assert_eq!(plan(&revoked, Change::Tunnel(true)), Plan::Link { managed: true, publish: false, hold: None });
        // An older server without `managedTunnelActive` means a linked tunnel.
        assert_eq!(plan(&json!({"linked": true, "publishAgentActivity": false}), Change::Tunnel(false)), Plan::Unlink);
    }

    #[test]
    fn describes_the_link_like_web() {
        assert!(description(false, None).starts_with("Sign in to make"));
        assert!(description(true, Some(&json!({"linked": true, "linkRevoked": true}))).starts_with("This environment was removed"));
        let on = json!({"linked": true, "managedTunnelActive": true, "reachability": {"status": "unreachable", "reason": "No connections.", "hint": null, "fix": "Restart."}});
        assert_eq!(description(true, Some(&on)), "T3 Connect is on, but other devices cannot reach this environment.");
        assert_eq!(reachability(&on), Some(("No connections. Fix: Restart.".to_owned(), true)));
        let healthy = json!({"reachability": {"status": "reachable", "readyConnections": 4, "protocol": "quic"}});
        assert_eq!(reachability(&healthy), Some(("Tunnel connected · 4 connections · QUIC".to_owned(), false)));
        assert!(description(true, Some(&json!({"linked": true, "managedTunnelActive": false}))).starts_with("T3 Connect is out of sync"));
    }

    #[test]
    fn tunnel_usage_and_cap_follow_the_relay_and_t3_chat_plan() {
        let envs = json!({"environments": [{"endpoint": {"providerKind": "cloudflare_tunnel"}}, {"endpoint": {"providerKind": "manual"}}, {"endpoint": {"providerKind": "cloudflare_tunnel"}}]});
        assert_eq!(tunnels_in_use(&envs), 2);
        assert_eq!(cap(&json!({"available": false, "link": null})), (3, None));
        assert_eq!(cap(&json!({"available": true, "link": null})), (3, Some("link")));
        assert_eq!(cap(&json!({"available": true, "link": {"plan": "free"}})), (3, Some("pro")));
        assert_eq!(cap(&json!({"available": true, "link": {"plan": "pro"}})), (10, Some("premier")));
        assert_eq!(cap(&json!({"available": true, "link": {"plan": "premier"}})), (100, None));
        assert_eq!(cap(&json!({"available": true, "link": {"plan": "pro"}, "maxTunnelsOverride": 7})), (7, None));
        // The refusal's alert stays until a tunnel frees or the cap rises.
        let mut ui = Ui { tunnel_limit: Some(3), used_at_refusal: Some(3), usage: Some(Usage { used: 3, limit: 3, upgrade: None }), ..Ui::default() };
        assert!(limit_alert_shown(&ui));
        ui.usage = Some(Usage { used: 2, limit: 3, upgrade: None });
        assert!(!limit_alert_shown(&ui));
        ui.usage = Some(Usage { used: 3, limit: 10, upgrade: None });
        assert!(!limit_alert_shown(&ui));
    }


    #[test]
    fn relay_refusals_carry_the_tunnel_cap() {
        let refused = failure("https://relay/v1/client/environment-links failed", 403, &json!({"relayError": {"_tag": "RelayEnvironmentLinkLimitExceededError", "maxTunnels": 2, "message": "Limit reached"}}));
        assert_eq!(refused.tunnel_limit, Some(2));
        assert_eq!(refused.message, "https://relay/v1/client/environment-links failed: Limit reached");
        assert_eq!(failure("Could not unlink the environment from cloud.", 500, &Value::Null).message, "Could not unlink the environment from cloud: HTTP 500");
    }

    #[test]
    fn onboarding_links_only_what_differs_and_prefills_from_its_own_link() {
        let tunnel = json!({"linked": true, "managedTunnelActive": true, "publishAgentActivity": false, "cloudUserId": "user_1"});
        assert_eq!(onboarding_plan(&tunnel, true, true), Plan::Preferences { publish: true, hold: None });
        assert_eq!(onboarding_plan(&tunnel, false, true), Plan::Link { managed: false, publish: true, hold: None });
        assert_eq!(onboarding_plan(&json!({"linked": false}), true, false), Plan::Link { managed: true, publish: false, hold: None });
        let link = link_state(&tunnel);
        assert_eq!((link.linked_user.as_deref(), link.managed_tunnel, link.publish_activity), (Some("user_1"), true, false));
        assert_eq!(link_state(&json!({"linked": true, "linkRevoked": true, "cloudUserId": "user_1"})).linked_user, None, "a revoked link is nobody's");
    }
}
