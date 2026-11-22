//! Web `ManagedCodexSetup` (settings presentation): the ChatGPT account row of a managed Codex
//! instance, which installs Codex when it's missing, then signs in, and its read-only runtime.
use super::super::{failure_detail, resource};
use crate::kit;
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, TextEdit, col, el, row, text};
use serde_json::{Value, json};

#[derive(Default)]
pub struct Ui {
    /// The request in flight: instance, reply id, and the sign-in method an install leads to.
    pending: Option<(String, u64, Step)>,
    /// An install this page started, waiting to settle before the sign-in starts.
    after_install: Option<(String, Value, String)>,
    /// A sign-in this page started, waiting for its authorization page.
    open_flow: Option<(String, String)>,
    /// Web's `awaitingProvider === "sign-in"`.
    awaiting: Option<String>,
    error: Option<(String, String)>,
    /// The account picker's instance and selected method.
    picker: Option<(String, String)>,
    help_open: bool,
    callback: TextEdit,
    /// Web's `handoff`: the instance and the input of a sign-in run on the primary environment.
    handoff: Option<(String, Value)>,
    /// The handoff's authorization page was opened.
    handoff_opened: bool,
    /// The attempt whose finished profile was sent to the environment.
    imported: Option<String>,
    transfer_failed: bool,
}

#[derive(Clone)]
enum Step {
    Install(String),
    SignIn,
    /// `chatGptReconnectProfile` before a handoff.
    Reconnect,
    /// `chatGptImportProfile` after one.
    Import,
    Other,
}

const HANDOFF: &str = "provider.chatgpt.handoff.subscribe";
/// `CHATGPT_USAGE_URL`.
const USAGE_URL: &str = "https://chatgpt.com/#settings/Usage";
const HANDOFF_INTERRUPTED: &str = "ChatGPT sign-in on the primary environment was interrupted. Try again.";

/// `readSignInRouting`'s `primaryAuthEnvironmentId`: a remote environment signs in through this
/// machine's server, which can receive the provider's callback.
fn via_primary(s: &App) -> bool {
    let active = &s.ui.env.active;
    active != crate::env::LOCAL && !s.ui.desktop.local_off && !crate::settings::machines::loopback(s, active) && crate::env::connected(s, crate::env::LOCAL)
}

/// The handoff stream's latest state, for `id`.
fn handoff_state<'a>(s: &'a App, id: &str) -> Option<&'a Value> {
    let (_, input) = s.ui.settings.providers_ui.codex.handoff.as_ref().filter(|(owner, _)| owner == id)?;
    crate::env::source(s, crate::env::LOCAL)?.net_ref()?.resource(HANDOFF, input.clone())
}

fn end_handoff(s: &mut App) {
    if let Some((_, input)) = s.ui.settings.providers_ui.codex.handoff.take()
        && let Some(net) = crate::env::net_of(s, crate::env::LOCAL)
    {
        net.unwatch_resource(HANDOFF, input);
    }
}

/// The input that starts sign-in on the primary environment with the remote one's saved profile.
fn handoff_input(id: &str, environment: &str, attempt: &str, profile: Value) -> Value {
    json!({"instanceId": id, "environmentId": environment, "attemptId": attempt, "returnUrl": format!("s5code://app/settings/providers?instanceId={id}"), "profile": profile})
}

const RUNNING: [&str; 3] = ["downloading", "extracting", "verifying"];
const CHANGE_ACCOUNT: &str = "chatgpt-change-account";

/// `settleProviderInstall`: whether the started install left a usable runtime, `None` until settled.
pub fn settle_install(started: &Value, current: Option<&Value>) -> Option<bool> {
    let running = |v: &Value| RUNNING.contains(&v["phase"].as_str().unwrap_or(""));
    let state = if running(started) {
        current.filter(|c| c["operationId"] == started["operationId"])?
    } else {
        started
    };
    if running(state) {
        return None;
    }
    Some(!matches!(state["phase"].as_str(), Some("failed" | "cancelled")) && state["installedVersion"].is_string())
}

/// `settleAuthorizationPage`: the page once the flow waits on the browser, `Some(None)` once it
/// ends without one, `None` while it is still starting.
pub fn settle_page(state: Option<&Value>, flow: Option<&str>) -> Option<Option<String>> {
    let state = state?;
    let id = state["flowId"].as_str()?;
    if flow.is_some_and(|f| f != id) {
        return None;
    }
    match state["phase"].as_str() {
        Some("succeeded" | "failed" | "cancelled") => Some(None),
        Some("waiting") => {
            let interaction = &state["interaction"];
            let url = if interaction["type"] == "browser" { interaction["url"].as_str() } else { state["authorizationUrl"].as_str() };
            url.map(|u| Some(u.to_owned()))
        }
        _ => None,
    }
}

/// The runtime half of the account row's description.
pub fn runtime_description(installation: Option<&Value>) -> String {
    let i = installation.unwrap_or(&Value::Null);
    let mb = |v: &Value| v.as_f64().unwrap_or(0.0) / 1_000_000.0;
    match i["phase"].as_str() {
        Some("downloading") => {
            let total = if i["totalBytes"].is_null() { String::new() } else { format!(" of {:.1}", mb(&i["totalBytes"])) };
            format!("Downloading {:.1}{total} MB.", mb(&i["downloadedBytes"]))
        }
        Some("extracting") => "Installing Codex.".into(),
        Some("verifying") => "Checking Codex.".into(),
        _ if i["installedVersion"].is_string() => {
            let source = if i["source"] == "local" { "Using your installed Codex" } else { "Managed by T3 Code" };
            let version = i["installedVersion"].as_str().filter(|v| !v.is_empty()).map(|v| format!(" · v{v}")).unwrap_or_default();
            format!("{source}{version}.")
        }
        _ => i["message"].as_str().unwrap_or("T3 Code downloads and manages Codex for you.").to_owned(),
    }
}

fn send(s: &mut App, id: &str, step: Step, method: &str, payload: Value) {
    if s.ui.settings.providers_ui.codex.pending.is_some() {
        return;
    }
    s.ui.settings.providers_ui.codex.error = None;
    if let Some(reply) = super::send(s, method, payload) {
        s.ui.settings.providers_ui.codex.pending = Some((id.to_owned(), reply, step));
    }
}

fn sign_in(s: &mut App, id: &str, method: &str) {
    s.ui.settings.providers_ui.codex.after_install = None;
    s.ui.settings.providers_ui.codex.transfer_failed = false;
    if via_primary(s) {
        send(s, id, Step::Reconnect, "provider.chatgpt.reconnect-profile", json!({"instanceId": id, "methodId": method}));
        return;
    }
    send(s, id, Step::SignIn, "provider.auth.start", json!({"instanceId": id, "methodId": method, "callbackMode": "client"}));
}

/// Web's `setup`: install (or update) first when needed, then sign in with `method`.
fn setup(s: &mut App, id: &str, method: &str) {
    let installation = resource(s, "provider.install.subscribe", json!({"instanceId": id})).cloned().unwrap_or(Value::Null);
    let installed = installation["installedVersion"].is_string();
    let update = installed && installation["source"] != "local" && installation["version"].is_string() && installation["version"] != installation["installedVersion"];
    if installed && !update {
        sign_in(s, id, method);
    } else {
        send(s, id, Step::Install(method.to_owned()), "provider.install.start", json!({"instanceId": id}));
    }
}

fn open_page(s: &mut App, id: &str, flow: &str, url: &str) {
    if !super::super::codex_callback::sign_in(s, id, &json!(flow), url) {
        super::super::auth::open_url(s, url);
    }
}

pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    if let Some((id, reply, step)) = s.ui.settings.providers_ui.codex.pending.clone()
        && let Some(result) = super::take(s, reply)
    {
        changed = true;
        let ui = &mut s.ui.settings.providers_ui.codex;
        ui.pending = None;
        match (result, step) {
            (Ok(started), Step::Install(method)) => ui.after_install = Some((id, started, method)),
            (Ok(started), Step::SignIn) => {
                ui.awaiting = Some(id.clone());
                if let Some(flow) = started["flowId"].as_str() {
                    match settle_page(Some(&started), Some(flow)) {
                        Some(Some(url)) => open_page(s, &id, flow, &url),
                        Some(None) => {}
                        None => s.ui.settings.providers_ui.codex.open_flow = Some((id, flow.to_owned())),
                    }
                }
            }
            (Ok(profile), Step::Reconnect) => {
                let environment = s.state().config.as_ref().and_then(|c| c["environment"]["environmentId"].as_str()).unwrap_or_default().to_owned();
                let input = handoff_input(&id, &environment, &format!("rune:{}", s5_net::uuid()), profile);
                match crate::env::net_of(s, crate::env::LOCAL) {
                    Some(net) => {
                        net.watch_resource(HANDOFF, input.clone());
                        let ui = &mut s.ui.settings.providers_ui.codex;
                        (ui.handoff, ui.handoff_opened) = (Some((id, input)), false);
                    }
                    None => s.ui.settings.providers_ui.codex.error = Some((id, HANDOFF_INTERRUPTED.into())),
                }
            }
            (Ok(_), Step::Import) => {
                end_handoff(s);
                let ui = &mut s.ui.settings.providers_ui.codex;
                (ui.awaiting, ui.transfer_failed) = (Some(id), false);
            }
            (Ok(_), Step::Other) => {}
            (Err(error), step) => {
                ui.transfer_failed = matches!(step, Step::Import);
                ui.error = Some((id, failure_detail(&error).unwrap_or_else(|| "Codex setup failed. Try again.".into())));
            }
        }
    }
    changed |= poll_handoff(s);
    if let Some((id, started, method)) = s.ui.settings.providers_ui.codex.after_install.clone() {
        let current = resource(s, "provider.install.subscribe", json!({"instanceId": id})).cloned();
        match settle_install(&started, current.as_ref()) {
            Some(true) => {
                sign_in(s, &id, &method);
                changed = true;
            }
            Some(false) => {
                s.ui.settings.providers_ui.codex.after_install = None;
                changed = true;
            }
            None => {}
        }
    }
    if let Some((id, flow)) = s.ui.settings.providers_ui.codex.open_flow.clone() {
        let state = resource(s, "provider.auth.subscribe", json!({"instanceId": id})).cloned();
        if let Some(page) = settle_page(state.as_ref(), Some(&flow)) {
            s.ui.settings.providers_ui.codex.open_flow = None;
            if let Some(url) = page {
                open_page(s, &id, &flow, &url);
            }
            changed = true;
        }
    }
    if let Some(id) = s.ui.settings.providers_ui.codex.awaiting.clone() {
        let authenticated = crate::chat::composer::provider(s, &id).is_some_and(|p| p["auth"]["status"] == "authenticated");
        let phase = resource(s, "provider.auth.subscribe", json!({"instanceId": id})).and_then(|a| a["phase"].as_str()).map(str::to_owned);
        if authenticated || matches!(phase.as_deref(), Some("failed" | "cancelled")) {
            s.ui.settings.providers_ui.codex.awaiting = None;
            changed = true;
        } else if phase.as_deref() == Some("succeeded") {
            // The server doesn't refresh the provider snapshot when a sign-in flow succeeds.
            s.ui.settings.providers_ui.codex.awaiting = None;
            if let Some(net) = s.src.net() {
                net.request("server.refreshProviders", json!({"instanceId": id}));
            }
            changed = true;
        }
    }
    changed
}

/// Follows a primary-environment sign-in: opens its page once, and hands the finished profile to
/// the remote environment once per attempt. A dropped connection ends it rather than replaying it.
fn poll_handoff(s: &mut App) -> bool {
    let Some((id, input)) = s.ui.settings.providers_ui.codex.handoff.clone() else { return false };
    let state = handoff_state(s, &id).cloned();
    let failure = if !crate::env::connected(s, crate::env::LOCAL) || state.as_ref().is_some_and(|v| v.get("error").is_some()) {
        Some(HANDOFF_INTERRUPTED.to_owned())
    } else {
        state.as_ref().filter(|v| v["phase"] == "auth" && matches!(v["state"]["phase"].as_str(), Some("failed" | "cancelled"))).map(|v| v["state"]["message"].as_str().unwrap_or("ChatGPT sign-in could not finish. Try again.").to_owned())
    };
    if let Some(message) = failure {
        end_handoff(s);
        s.ui.settings.providers_ui.codex.error = Some((id, message));
        return true;
    }
    let Some(state) = state else { return false };
    if state["phase"] == "auth" && !s.ui.settings.providers_ui.codex.handoff_opened {
        if let Some(Some(url)) = settle_page(Some(&state["state"]), None) {
            s.ui.settings.providers_ui.codex.handoff_opened = true;
            super::super::auth::open_url(s, &url);
            return true;
        }
    }
    let attempt = input["attemptId"].as_str().map(str::to_owned);
    if state["phase"] == "finished" && s.ui.settings.providers_ui.codex.imported != attempt {
        s.ui.settings.providers_ui.codex.imported = attempt;
        import(s, &id, state["profile"].clone());
        return true;
    }
    false
}

fn import(s: &mut App, id: &str, profile: Value) {
    send(s, id, Step::Import, "provider.chatgpt.import-profile", json!({"instanceId": id, "profile": profile}));
}

/// Web's `onModeChange`: enables the instance with `setupMode` set.
fn set_mode(s: &mut App, id: &str, mode: &str) {
    let mut instance = super::instance_of(s, id, "codex");
    instance["enabled"] = json!(true);
    if !instance["config"].is_object() {
        instance["config"] = json!({});
    }
    instance["config"]["enabled"] = json!(true);
    instance["config"]["setupMode"] = json!(mode);
    let patch = json!({"providerInstances": {id: instance.clone()}});
    super::upsert(s, id, instance, patch);
}

fn button(t: &T, label: &str, variant: &str, disabled: bool, press: impl Fn(&mut App) + 'static) -> El {
    kit::Button::new(label).variant(variant).size("sm").disabled(disabled).on_press(press).build(t)
}

pub fn view(s: &App, t: &T, id: &str, p: &Value, enabled: bool, read_only: bool) -> Vec<El> {
    let target = json!({"instanceId": id});
    let handoff = handoff_state(s, id);
    let handing_off = s.ui.settings.providers_ui.codex.handoff.as_ref().is_some_and(|(owner, _)| owner == id);
    let finished_profile = handoff.filter(|h| h["phase"] == "finished").map(|h| h["profile"].clone());
    let auth = handoff.filter(|h| h["phase"] == "auth").map(|h| &h["state"]).or_else(|| resource(s, "provider.auth.subscribe", target.clone()));
    let installation = resource(s, "provider.install.subscribe", target);
    let ui = &s.ui.settings.providers_ui.codex;
    let mine = |owner: &str| owner == id;
    let phase = auth.and_then(|a| a["phase"].as_str());
    let installed = installation.is_some_and(|i| i["installedVersion"].is_string());
    let authenticated = p["auth"]["status"] == "authenticated";
    let awaiting = ui.awaiting.as_deref().is_some_and(mine);
    let finishing = awaiting && !authenticated && phase == Some("succeeded");
    let waiting_for_state = awaiting && !authenticated && matches!(phase, None | Some("idle"));
    let auth_in_progress = finishing || waiting_for_state || handing_off || matches!(phase, Some("starting" | "waiting" | "verifying"));
    let install_active = installation.is_some_and(|i| RUNNING.contains(&i["phase"].as_str().unwrap_or("")));
    let pending = ui.pending.as_ref().is_some_and(|(owner, ..)| mine(owner)) || ui.after_install.as_ref().is_some_and(|(owner, ..)| mine(owner));
    let auth_active = auth_in_progress || (pending && !install_active);
    let query_error = [auth, installation].into_iter().flatten().any(|r| r.get("error").is_some());
    let unavailable = read_only || !enabled || pending || query_error;
    let busy = pending || auth_in_progress || install_active;
    let methods: Vec<Value> = auth.and_then(|a| a["methods"].as_array()).cloned().unwrap_or_default();
    let has_saved = methods.iter().any(|m| m["id"].as_str().is_some_and(|m| m.starts_with("chatgpt-profile:")));
    let email = |method: &str| methods.iter().find(|m| m["id"] == method).and_then(|m| m["accountEmail"].as_str()).map(str::to_owned);
    let url = auth.and_then(|a| if a["interaction"]["type"] == "browser" { a["interaction"]["url"].as_str() } else { a["authorizationUrl"].as_str() }).map(str::to_owned);
    let flow = auth.and_then(|a| a["flowId"].as_str()).map(str::to_owned);

    let description = if finishing {
        "Finishing sign-in...".to_owned()
    } else if install_active {
        runtime_description(installation)
    } else if auth_active || matches!(phase, Some("failed" | "cancelled")) {
        match email("chatgpt").filter(|_| phase == Some("waiting")) {
            Some(email) => format!("Continue as {email} on OpenAI."),
            None => auth.and_then(|a| a["message"].as_str()).unwrap_or("Finish signing in in your browser.").to_owned(),
        }
    } else if authenticated {
        match p["auth"]["email"].as_str().map(str::trim).filter(|e| !e.is_empty()) {
            Some(email) => format!("Signed in as {email}."),
            None => "Signed in with ChatGPT.".into(),
        }
    } else {
        email("chatgpt").unwrap_or_else(|| "Use your ChatGPT subscription.".into())
    };

    let owned = id.to_owned();
    let mut controls = row().gap(8.0).center_y().flex_wrap();
    if auth_active {
        let (page_id, page_flow, page_url) = (owned.clone(), flow.clone().unwrap_or_default(), url.clone().unwrap_or_default());
        let label = if finishing {
            "Finishing sign-in..."
        } else if finished_profile.is_some() {
            if ui.transfer_failed { "Retry connection" } else { "Finishing sign-in..." }
        } else if phase == Some("waiting") {
            "Open sign-in page"
        } else {
            "Signing in..."
        };
        let disabled = if finished_profile.is_some() { pending || !ui.transfer_failed } else { url.is_none() || phase != Some("waiting") };
        let retry = finished_profile.clone();
        controls = controls.child(button(t, label, "outline", disabled, move |s: &mut App| match &retry {
            Some(profile) => import(s, &page_id, profile.clone()),
            None if handing_off => super::super::auth::open_url(s, &page_url),
            None => open_page(s, &page_id, &page_flow, &page_url),
        }));
        if !finishing {
            let (cancel_id, cancel_flow) = (owned.clone(), flow.clone());
            controls = controls.child(button(t, "Cancel", "ghost", pending || read_only, move |s: &mut App| {
                if handing_off {
                    end_handoff(s);
                    s.ui.settings.providers_ui.codex.awaiting = None;
                    return;
                }
                let Some(flow) = cancel_flow.clone() else { return };
                let ui = &mut s.ui.settings.providers_ui.codex;
                (ui.after_install, ui.open_flow, ui.awaiting) = (None, None, None);
                super::super::codex_callback::cancel(&cancel_id);
                send(s, &cancel_id, Step::Other, "provider.auth.cancel", json!({"instanceId": cancel_id, "flowId": flow}));
            }));
        }
    } else if install_active {
        let operation = installation.and_then(|i| i["operationId"].as_str()).map(str::to_owned);
        let cancel_id = owned.clone();
        controls = controls.child(button(t, "Cancel", "ghost", pending || read_only, move |s: &mut App| {
            let Some(operation) = operation.clone() else { return };
            s.ui.settings.providers_ui.codex.after_install = None;
            send(s, &cancel_id, Step::Other, "provider.install.cancel", json!({"instanceId": cancel_id, "operationId": operation}));
        }));
    } else if authenticated {
        let (pick, out) = (owned.clone(), owned.clone());
        controls = controls
            .child(button(t, "Change account", "outline", unavailable || busy, move |s: &mut App| open_picker(s, &pick)))
            .child(button(t, "Disconnect", "ghost", unavailable || busy, move |s: &mut App| {
                send(s, &out, Step::Other, "provider.auth.logout", json!({"instanceId": out}));
            }));
    } else {
        let can = if installed { p["setup"]["canAuthenticate"] == true } else { p["setup"]["canInstall"] == true };
        let (go, other, existing) = (owned.clone(), owned.clone(), owned.clone());
        let label = if pending {
            "Setting up..."
        } else if has_saved {
            "Reconnect account"
        } else {
            "Continue with ChatGPT"
        };
        controls = controls.child(
            kit::Button::new(label)
                .size("sm")
                .disabled(unavailable || busy || !can)
                .on_press(move |s: &mut App| if has_saved { open_picker(s, &go) } else { setup(s, &go, "chatgpt") })
                .build(t),
        );
        if has_saved {
            controls = controls.child(button(t, "Use a different account", "ghost", unavailable || busy, move |s: &mut App| setup(s, &other, CHANGE_ACCOUNT)));
        }
        controls = controls.child(button(t, "Use existing CLI", "ghost", read_only || busy, move |s: &mut App| set_mode(s, &existing, "existing")));
    }
    let account = row()
        .key(format!("row-codex-account-{id}"))
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(32.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(text("ChatGPT account").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(text(description).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap()),
        )
        .child(controls.max_w(352.0).justify_end());
    let mut rows = vec![account];
    if authenticated {
        let usage = kit::Button::new("Manage usage").variant("ghost-muted").size("sm").icon(&icons::EXTERNAL_LINK).on_press(|s: &mut App| super::super::auth::open_url(s, USAGE_URL));
        rows.push(row().key(format!("row-codex-usage-{id}")).w_full().px(16.0).py(4.0).justify_end().child(usage.build(t)));
    }

    if let Some(warning) = auth.filter(|_| phase == Some("idle")).and_then(|a| a["message"].as_str()).filter(|m| m.starts_with("Signed out locally.")) {
        rows.push(text(warning.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().role("alert").px(16.0).py(8.0));
    }
    if let (Some(url), Some(flow)) = (url.clone().filter(|_| phase == Some("waiting") && !handing_off), flow.clone()) {
        let mut help = col().w_full().px(16.0).py(12.0).gap(12.0).child(
            row()
                .gap(6.0)
                .center_y()
                .role("button")
                .label("Having trouble signing in?")
                .expanded(ui.help_open)
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .on_press(|s: &mut App| s.ui.settings.providers_ui.codex.help_open ^= true)
                .child(text("Having trouble signing in?").font(theme::TEXT_XS).color(t.c.muted_foreground))
                .child(rune::icon(if ui.help_open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 12.0).color(t.c.muted_foreground)),
        );
        if ui.help_open {
            let empty = ui.callback.text.trim().is_empty();
            let (complete_id, open) = (owned.clone(), url.clone());
            help = help
                .child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.5)))
                .child(text("If sign-in doesn't return to T3 Code, paste the URL from the final localhost page.").font(theme::TEXT_XS).color(t.c.muted_foreground).wrap())
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .center_y()
                        .child(
                            kit::Input::new(&ui.callback)
                                .size("sm")
                                .placeholder("Paste the URL from the sign-in tab")
                                .on_edit(|s: &mut App| &mut s.ui.settings.providers_ui.codex.callback)
                                .build(t)
                                .secure()
                                .single_line()
                                .grow(1.0)
                                .label("ChatGPT sign-in redirect URL"),
                        )
                        .child(button(t, "Connect", "outline", pending || read_only || empty, move |s: &mut App| {
                            let value = s.ui.settings.providers_ui.codex.callback.text.trim().to_owned();
                            s.ui.settings.providers_ui.codex.callback.clear();
                            send(s, &complete_id, Step::Other, "provider.auth.complete", json!({"instanceId": complete_id, "flowId": flow, "callbackUrl": value}));
                        })),
                )
                .child(
                    kit::Button::new("Try sign-in in your browser")
                        .variant("ghost-muted")
                        .size("xs")
                        .icon(&icons::EXTERNAL_LINK)
                        .on_press(move |s: &mut App| super::super::auth::open_url(s, &open))
                        .build(t)
                        .self_start(),
                );
        }
        rows.push(help);
    }
    let error = ui.error.as_ref().filter(|(owner, _)| mine(owner)).map(|(_, e)| e.clone()).or_else(|| {
        if installation.is_some_and(|i| i["phase"] == "failed") {
            installation.and_then(|i| i["message"].as_str()).map(str::to_owned)
        } else {
            query_error.then(|| "Could not read Codex setup status. Reconnect and try again.".to_owned())
        }
    });
    if let Some(error) = error {
        rows.push(text(error).font(theme::TEXT_XS).color(t.c.destructive).wrap().role("alert").px(16.0).py(8.0));
    }
    rows
}

pub fn escape(s: &mut App) -> bool {
    s.ui.settings.providers_ui.codex.picker.take().is_some()
}

fn open_picker(s: &mut App, id: &str) {
    let first = resource(s, "provider.auth.subscribe", json!({"instanceId": id}))
        .and_then(|a| a["methods"].as_array())
        .and_then(|m| m.iter().find_map(|m| m["id"].as_str().filter(|i| i.starts_with("chatgpt-profile:")).map(str::to_owned)))
        .unwrap_or_else(|| CHANGE_ACCOUNT.into());
    s.ui.settings.providers_ui.codex.picker = Some((id.to_owned(), first));
}

/// Web's `ChatGptAccountPicker`: a saved ChatGPT profile, or a different account.
pub fn picker(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some((id, selected)) = s.ui.settings.providers_ui.codex.picker.clone() else { return };
    let methods: Vec<Value> = resource(s, "provider.auth.subscribe", json!({"instanceId": id})).and_then(|a| a["methods"].as_array()).cloned().unwrap_or_default();
    let mut options: Vec<(String, String)> = methods
        .iter()
        .filter_map(|m| Some((m["id"].as_str().filter(|i| i.starts_with("chatgpt-profile:"))?.to_owned(), m["name"].as_str().unwrap_or("").to_owned())))
        .collect();
    options.push((CHANGE_ACCOUNT.into(), "Use a different account".into()));
    let mut list = col().w_full().gap(8.0).role("radiogroup").label("ChatGPT account to connect");
    for (method, name) in options {
        let chosen = method == selected;
        list = list.child(
            row()
                .w_full()
                .px(12.0)
                .py(12.0)
                .gap(12.0)
                .center_y()
                .radius(8.0)
                .border(1.0, if chosen { t.c.primary } else { t.c.border })
                .role("radio")
                .checked(chosen)
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .on_press(move |s: &mut App| {
                    if let Some((_, selected)) = &mut s.ui.settings.providers_ui.codex.picker {
                        *selected = method.clone();
                    }
                })
                .child(
                    el().size(16.0, 16.0)
                        .radius(8.0)
                        .border(1.0, if chosen { t.c.primary } else { t.c.input })
                        .center()
                        .children(chosen.then(|| el().size(8.0, 8.0).radius(4.0).fill(t.c.primary))),
                )
                .child(text(name).font(theme::SM_MEDIUM).color(t.c.foreground).wrap().grow(1.0)),
        );
    }
    let close = |s: &mut App| s.ui.settings.providers_ui.codex.picker = None;
    kit::Dialog::new("codex-account-picker", "Reconnect ChatGPT")
        .open(true)
        .description("On OpenAI, sign in with the account you choose here.")
        .on_close(close)
        .child(list)
        .child(
            row().w_full().justify_end().child(
                kit::Button::new("Continue with ChatGPT")
                    .size("sm")
                        .on_press(move |s: &mut App| {
                        let Some((id, method)) = s.ui.settings.providers_ui.codex.picker.take() else { return };
                        setup(s, &id, &method);
                    })
                    .build(t),
            ),
        )
        .build(t, layers);
}

/// Web's `CodexManagedRuntimeFields`: what T3 Code picked, shown but not editable.
pub fn runtime(s: &App, t: &T, id: &str, p: Option<&Value>) -> Vec<El> {
    let installation = resource(s, "provider.install.subscribe", json!({"instanceId": id}));
    let paths = p.map(|p| &p["runtimePaths"]);
    let shadow = paths.and_then(|r| r["shadowHomePath"].as_str()).filter(|v| !v.is_empty());
    let field = |value: Option<&str>, placeholder: &str, label: &str| {
        let (shown, color) = match value.filter(|v| !v.is_empty()) {
            Some(v) => (v.to_owned(), t.c.foreground),
            None => (placeholder.to_owned(), t.c.muted_foreground),
        };
        row()
            .w(320.0)
            .h(32.0)
            .px(10.0)
            .center_y()
            .radius(8.0)
            .border(1.0, t.c.input)
            .opacity(0.64)
            .label(label.to_owned())
            .help(shown.clone())
            .child(text(shown).font(theme::TEXT_SM).color(color).truncate())
    };
    let binary_placeholder = if installation.is_some_and(|i| i.get("error").is_some()) { "Could not read runtime path" } else { "Not installed" };
    vec![
        super::control_row(t, &super::field_key(id, "managed-binary"), "Binary path", "Selected by T3 Code.", field(installation.and_then(|i| i["executablePath"].as_str()), binary_placeholder, "Codex binary path")),
        super::control_row(
            t,
            &super::field_key(id, "managed-home"),
            "CODEX_HOME path",
            "Shared Codex config, sessions, and state.",
            field(paths.and_then(|r| r["homePath"].as_str()), "Unavailable", "Codex home path"),
        ),
        super::control_row(
            t,
            &super::field_key(id, "managed-shadow"),
            "Shadow home path",
            if shadow.is_some() { "Account-specific home sharing the Codex state above." } else { "This instance uses the shared Codex home directly." },
            field(shadow, if paths.is_some_and(Value::is_object) { "Not used" } else { "Unavailable" }, "Codex shadow home path"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_and_sign_ins_settle_like_web() {
        let running = json!({"phase": "downloading", "operationId": "op2", "installedVersion": null});
        assert_eq!(settle_install(&running, None), None, "FAILED: no state yet");
        assert_eq!(settle_install(&running, Some(&json!({"phase": "installed", "operationId": "op1", "installedVersion": "1"}))), None, "FAILED: an older operation doesn't settle it");
        assert_eq!(settle_install(&running, Some(&json!({"phase": "verifying", "operationId": "op2"}))), None);
        assert_eq!(settle_install(&running, Some(&json!({"phase": "installed", "operationId": "op2", "installedVersion": "1.2"}))), Some(true));
        assert_eq!(settle_install(&running, Some(&json!({"phase": "failed", "operationId": "op2", "installedVersion": "1.2"}))), Some(false));
        assert_eq!(settle_install(&json!({"phase": "installed", "installedVersion": "1"}), None), Some(true), "FAILED: an install that returns settled is settled");

        let waiting = json!({"flowId": "f", "phase": "waiting", "interaction": {"type": "browser", "url": "https://auth"}});
        assert_eq!(settle_page(Some(&waiting), Some("f")), Some(Some("https://auth".into())));
        assert_eq!(settle_page(Some(&waiting), Some("other")), None, "FAILED: another client's flow");
        assert_eq!(settle_page(Some(&json!({"flowId": "f", "phase": "waiting", "authorizationUrl": "https://a"})), None), Some(Some("https://a".into())));
        assert_eq!(settle_page(Some(&json!({"flowId": "f", "phase": "starting"})), Some("f")), None);
        assert_eq!(settle_page(Some(&json!({"flowId": "f", "phase": "failed"})), Some("f")), Some(None));
        assert_eq!(settle_page(Some(&json!({"flowId": null, "phase": "idle"})), None), None);

        let input = handoff_input("work", "env-2", "rune:1", Value::Null);
        assert_eq!(input["returnUrl"], "s5code://app/settings/providers?instanceId=work", "providerAuthReturnUrl accepts s5code://app/settings/…");
        assert_eq!((input["environmentId"].as_str(), input["profile"].is_null()), (Some("env-2"), true), "a first sign-in has no profile");
    }

    #[test]
    fn runtime_reads_like_web() {
        assert_eq!(runtime_description(Some(&json!({"phase": "downloading", "downloadedBytes": 2_500_000, "totalBytes": 10_000_000}))), "Downloading 2.5 of 10.0 MB.");
        assert_eq!(runtime_description(Some(&json!({"phase": "downloading", "downloadedBytes": 1_000_000, "totalBytes": null}))), "Downloading 1.0 MB.");
        assert_eq!(runtime_description(Some(&json!({"phase": "idle", "installedVersion": "0.9", "source": "managed"}))), "Managed by T3 Code · v0.9.");
        assert_eq!(runtime_description(Some(&json!({"phase": "idle", "installedVersion": "0.9", "source": "local"}))), "Using your installed Codex · v0.9.");
        assert_eq!(runtime_description(None), "T3 Code downloads and manages Codex for you.");
    }
}
