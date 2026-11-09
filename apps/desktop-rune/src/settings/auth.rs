//! Web `ProviderAuthenticationSection`: the "Account" row that signs a provider instance in,
//! on the provider page and in Add provider's sign-in step. Starts use the server's own
//! callback, so a local server catches the redirect and a remote one takes the pasted URL.
use super::request;
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons, kit};
use rune::{El, TextEdit, col, el, row, text};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Default)]
pub struct Ui {
    /// The chosen sign-in method per instance; absent is "Provider default".
    method: HashMap<String, String>,
    method_open: Option<String>,
    reveal_email: bool,
    /// The instance whose sign-out is being confirmed.
    logout: Option<String>,
    terminal_input: TextEdit,
}

pub fn open_url(s: &mut App, url: &str) {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        s.ui.settings.error = Some("Only HTTP and HTTPS links can be opened.".into());
        return;
    }
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("rundll32").arg("url.dll,FileProtocolHandler").arg(url).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(error) = result {
        s.ui.settings.error = Some(error.to_string());
    }
}

fn environment(s: &App) -> String {
    s.state().config.as_ref().and_then(|c| c["environment"]["label"].as_str()).unwrap_or("this environment").to_owned()
}

/// What the row derives from the live provider and its latest auth state (web `:58-107`).
#[derive(Debug, PartialEq)]
pub struct Derived {
    pub active: bool,
    pub signed_in: bool,
    pub discovering: bool,
    pub needs_external_setup: bool,
    pub uses_credentials: bool,
}

pub fn derive(p: &Value, state: Option<&Value>) -> Derived {
    let state = state.unwrap_or(&Value::Null);
    let phase = state["phase"].as_str();
    let active = matches!(phase, Some("starting" | "waiting" | "verifying"));
    let status = p["auth"]["status"].as_str();
    let signed_in = status == Some("authenticated") || (status == Some("unknown") && phase == Some("succeeded"));
    let acp = p["driver"] == "acpRegistry";
    let methods = state["methods"].as_array();
    let error = state.get("error").is_some();
    Derived {
        active,
        signed_in,
        discovering: acp && !active && !signed_in && !error && methods.is_none(),
        needs_external_setup: !active && !signed_in && (p["setup"]["canAuthenticate"] == false || (acp && methods.is_some_and(Vec::is_empty))),
        uses_credentials: p["driver"] == "antigravity" && matches!(p["auth"]["type"].as_str(), Some("gemini-api-key" | "agent-platform")),
    }
}

/// The row's description, in web's priority order.
fn description(p: &Value, state: &Value, d: &Derived, env: &str, reveal: bool) -> String {
    let email = p["auth"]["email"].as_str().filter(|e| !e.is_empty());
    let interaction = state["interaction"]["type"].as_str();
    match state["phase"].as_str() {
        _ if d.signed_in && !d.active && email.is_some() => {
            let email = email.unwrap_or_default();
            let shown = if reveal { email.to_owned() } else { redact(email) };
            format!("Signed in as {shown}")
        }
        Some("starting") => state["message"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| (if d.uses_credentials { "Connecting…" } else { "Starting sign-in…" }).into()),
        Some("verifying") => (if d.uses_credentials { "Checking credentials and loading models…" } else { "Checking your account…" }).into(),
        Some("waiting") => match interaction {
            Some("terminal") => "Complete sign-in in the terminal below.".into(),
            Some("credentials") => "Enter your credentials below.".into(),
            _ => "Finish signing in in your browser.".into(),
        },
        _ if d.signed_in => "Signed in.".into(),
        _ if d.discovering => "Discovering sign-in methods…".into(),
        _ if d.needs_external_setup => "No in-app sign-in advertised. Follow the provider's docs to finish setup.".into(),
        _ if d.uses_credentials => format!("Connect on {env} to load models."),
        _ => format!("Sign in on {env}."),
    }
}

fn redact(email: &str) -> String {
    match email.split_once('@') {
        Some((local, domain)) => format!("{}@{domain}", "•".repeat(local.chars().count().clamp(3, 8))),
        None => "•".repeat(8),
    }
}

fn start_label(state: &Value, d: &Derived) -> &'static str {
    match state["phase"].as_str() {
        _ if d.signed_in => "Change account",
        Some("failed" | "cancelled") => if d.uses_credentials { "Retry connection" } else { "Retry sign-in" },
        _ => if d.uses_credentials { "Connect" } else { "Sign in" },
    }
}

fn respond(s: &mut App, id: &str, state: &Value, response: Value) {
    request(s, "provider.auth.respond", json!({"instanceId": id, "flowId": state["flowId"], "interactionId": state["interaction"]["id"], "response": response}));
}

/// The "Account" row and whatever the current interaction needs below it, for live provider `p`.
pub fn view(s: &App, t: &T, p: &Value) -> El {
    let id = p["instanceId"].as_str().unwrap_or("").to_owned();
    if !p["setup"].is_object() {
        return el();
    }
    let state = super::resource(s, "provider.auth.subscribe", json!({"instanceId": id}));
    let d = derive(p, state);
    let state = state.cloned().unwrap_or(Value::Null);
    let error = state.get("error").is_some();
    let ui = &s.ui.settings.auth;
    let interaction = &state["interaction"];
    let url = match interaction["type"].as_str() {
        Some("browser" | "deviceCode") => interaction["url"].as_str(),
        _ => state["authorizationUrl"].as_str(),
    }
    .map(str::to_owned);
    let methods: Vec<Value> = state["methods"].as_array().cloned().unwrap_or_default();

    let mut copy = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(text("Account").font(theme::SM_MEDIUM).color(t.c.foreground));
    let detail = text(description(p, &state, &d, &environment(s), ui.reveal_email)).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap();
    let has_email = d.signed_in && !d.active && p["auth"]["email"].as_str().is_some_and(|e| !e.is_empty());
    copy = copy.child(if has_email {
        detail
            .role("button")
            .label(if ui.reveal_email { "Click to hide email" } else { "Click to reveal email" })
            .cursor(rune::Cursor::Pointer)
            .on_press(|s: &mut App| s.ui.settings.auth.reveal_email ^= true)
    } else {
        detail
    });
    if state["phase"] == "failed"
        && let Some(message) = state["message"].as_str()
    {
        copy = copy.child(text(message.to_owned()).font(theme::TEXT_XS.lh(18.0)).color(t.c.destructive).wrap());
    }

    let mut controls = row().gap(8.0).center_y();
    if !d.active && methods.len() > 1 {
        let chosen = ui.method.get(&id).and_then(|m| methods.iter().find(|x| x["id"] == m.as_str()));
        let name = chosen.and_then(|m| m["name"].as_str()).unwrap_or("Provider default").to_owned();
        let owned = id.clone();
        controls = controls.child(
            kit::Button::new(name)
                .variant("outline")
                .size("sm")
                .icon(&icons::CHEVRON_DOWN)
                .on_press(move |s: &mut App| {
                    let open = &mut s.ui.settings.auth.method_open;
                    *open = if open.as_deref() == Some(owned.as_str()) { None } else { Some(owned.clone()) };
                })
                .build(t)
                .label("Sign-in method")
                .expanded(ui.method_open.as_deref() == Some(id.as_str())),
        );
    }
    if let Some(url) = url.clone() {
        let consent = interaction["type"] == "browser" && interaction["requiresConsent"] == true;
        let (open_id, open_state, open_url_) = (id.clone(), state.clone(), url.clone());
        controls = controls.child(
            kit::Button::new("Open browser")
                .variant("outline")
                .size("sm")
                .on_press(move |s: &mut App| {
                    if consent {
                        respond(s, &open_id, &open_state, json!({"type": "browser", "action": "accept"}));
                    }
                    open_url(s, &open_url_);
                })
                .build(t),
        );
        let (copy_id, copy_state) = (id.clone(), state.clone());
        controls = controls.child(
            kit::Button::new("Copy sign-in link")
                .variant("ghost-muted")
                .size("icon-sm")
                .icon(&icons::COPY)
                .build(t)
                .on_press_tx(move |s: &mut App, tx: &mut rune::Tx| {
                    tx.copy(&url);
                    if consent {
                        respond(s, &copy_id, &copy_state, json!({"type": "browser", "action": "accept"}));
                    }
                }),
        );
    }
    let docs = p["setup"]["documentationUrl"].as_str().map(str::to_owned);
    if d.needs_external_setup && docs.is_some() {
        let docs = docs.unwrap_or_default();
        controls = controls.child(kit::Button::new("Open docs").variant("outline").size("sm").on_press(move |s: &mut App| open_url(s, &docs)).build(t));
    } else if d.active && state["flowId"].is_string() {
        let (cancel_id, flow) = (id.clone(), state["flowId"].clone());
        controls = controls.child(
            kit::Button::new("Cancel")
                .variant("ghost")
                .size("sm")
                .on_press(move |s: &mut App| request(s, "provider.auth.cancel", json!({"instanceId": cancel_id, "flowId": flow})))
                .build(t)
                .label("Cancel sign-in"),
        );
    } else if !d.active && !d.needs_external_setup && p["setup"]["canAuthenticate"] != false {
        let disabled = error || d.discovering || p["enabled"] == false || p["installed"] == false || state.is_null();
        let (start_id, known) = (id.clone(), methods.iter().filter_map(|m| m["id"].as_str().map(str::to_owned)).collect::<Vec<_>>());
        controls = controls.child(
            kit::Button::new(start_label(&state, &d))
                .variant("outline")
                .size("sm")
                .disabled(disabled)
                .on_press(move |s: &mut App| {
                    let mut input = json!({"instanceId": start_id});
                    if let Some(method) = s.ui.settings.auth.method.get(&start_id).filter(|m| known.contains(*m)) {
                        input["methodId"] = json!(method);
                    }
                    request(s, "provider.auth.start", input);
                })
                .build(t),
        );
    }
    if !d.active && d.signed_in && state["canLogout"].as_bool().unwrap_or(p["setup"]["canAuthenticate"] == true) {
        let owned = id.clone();
        controls = controls.child(kit::Button::new("Sign out").variant("ghost").size("sm").on_press(move |s: &mut App| s.ui.settings.auth.logout = Some(owned.clone())).build(t));
    }

    let mut out = col().w_full().gap(12.0).child(row().w_full().py(12.0).gap(32.0).center_y().child(copy).child(controls));

    if ui.method_open.as_deref() == Some(id.as_str()) && !d.active && methods.len() > 1 {
        let mut list = col().w_full().gap(2.0).padding(4.0).radius(8.0).border(1.0, t.c.border).role("listbox").label("Sign-in method");
        let current = ui.method.get(&id).cloned().unwrap_or_default();
        let options = std::iter::once((String::new(), "Provider default".to_owned()))
            .chain(methods.iter().map(|m| (m["id"].as_str().unwrap_or("").to_owned(), m["name"].as_str().unwrap_or("").to_owned())));
        for (method, name) in options {
            let (owned, chosen) = (id.clone(), method == current);
            let mut item = row()
                .w_full()
                .px(8.0)
                .py(6.0)
                .radius(6.0)
                .role("option")
                .checked(chosen)
                .cursor(rune::Cursor::Pointer)
                .hover(t.c.muted)
                .on_press(move |s: &mut App| {
                    let ui = &mut s.ui.settings.auth;
                    if method.is_empty() { ui.method.remove(&owned) } else { ui.method.insert(owned.clone(), method.clone()) };
                    ui.method_open = None;
                })
                .child(text(name).font(theme::TEXT_SM).color(t.c.foreground).grow(1.0));
            if chosen {
                item = item.child(rune::icon(&icons::CHECK, 14.0).color(t.c.foreground));
            }
            list = list.child(item);
        }
        out = out.child(list);
    }
    if let Some(code) = interaction["userCode"].as_str().filter(|_| interaction["type"] == "deviceCode") {
        out = out.child(
            row()
                .gap(4.0)
                .child(text("Enter code").font(theme::TEXT_SM).color(t.c.muted_foreground))
                .child(text(code.to_owned()).font(theme::MONO_SM).color(t.c.foreground).selectable(true))
                .child(text("in your browser.").font(theme::TEXT_SM).color(t.c.muted_foreground)),
        );
    }
    match interaction["type"].as_str() {
        Some("terminal") => {
            let output = interaction["output"].as_str().unwrap_or("");
            let tail: String = output.lines().rev().take(14).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
            let (send_id, send_state) = (id.clone(), state.clone());
            out = out.child(
                col()
                    .w_full()
                    .gap(8.0)
                    .padding(12.0)
                    .radius(6.0)
                    .border(1.0, t.c.border)
                    .fill(t.c.muted.alpha_mul(0.3))
                    .label("Provider sign-in terminal")
                    .child(text(strip_ansi(&tail)).font(theme::MONO_XS).color(t.c.foreground).wrap().selectable(true))
                    .child(
                        kit::Input::new(&ui.terminal_input)
                            .size("sm")
                            .font("mono")
                            .placeholder("Type a reply and press Enter")
                            .on_edit(|s: &mut App| &mut s.ui.settings.auth.terminal_input)
                            .on_submit(move |s: &mut App, _| {
                                let line = s.ui.settings.auth.terminal_input.text.clone();
                                s.ui.settings.auth.terminal_input.clear();
                                respond(s, &send_id, &send_state, json!({"type": "terminal", "data": format!("{line}\r")}));
                            })
                            .build(t)
                            .single_line(),
                    ),
            );
        }
        Some("credentials") => {
            let mut form = col().w_full().gap(8.0);
            for field in interaction["fields"].as_array().into_iter().flatten() {
                let key = field["name"].as_str().unwrap_or("").to_owned();
                let secret = field["secret"] == true;
                let edit = s.ui.settings.auth_fields.get(&key).cloned().unwrap_or_else(|| if secret { TextEdit::new("").secure() } else { TextEdit::new("") });
                let label = field["label"].as_str().unwrap_or("Credential").to_owned();
                let mut input = kit::Input::new(&edit)
                    .size("sm")
                    .on_edit(move |s: &mut App| s.ui.settings.auth_fields.entry(key.clone()).or_insert_with(|| if secret { TextEdit::new("").secure() } else { TextEdit::new("") }))
                    .build(t)
                    .single_line()
                    .label(label.clone());
                if secret {
                    input = input.secure();
                }
                form = form.child(col().w_full().gap(4.0).child(text(label).font(theme::XS_MEDIUM).color(t.c.muted_foreground)).child(input));
            }
            let (submit_id, submit_state, fields) = (id.clone(), state.clone(), interaction["fields"].clone());
            form = form.child(row().child(
                kit::Button::new("Connect")
                    .size("sm")
                    .on_press(move |s: &mut App| {
                        let values: serde_json::Map<String, Value> = fields
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|f| {
                                let name = f["name"].as_str()?;
                                Some((name.to_owned(), json!(s.ui.settings.auth_fields.get(name).map(|e| e.text.as_str()).unwrap_or(""))))
                            })
                            .collect();
                        respond(s, &submit_id, &submit_state, json!({"type": "credentials", "values": values}));
                        s.ui.settings.auth_fields.clear();
                    })
                    .build(t),
            ));
            out = out.child(form);
        }
        _ => {}
    }
    let paste = url.is_some() && if interaction["type"] == "browser" { interaction["acceptsCallback"] == true } else { interaction.is_null() };
    if paste {
        let (complete_id, flow) = (id.clone(), state["flowId"].clone());
        let empty = s.ui.settings.auth_response.text.trim().is_empty();
        out = out.child(
            col()
                .w_full()
                .gap(8.0)
                .child(text("If the final localhost page does not load, paste its full URL here.").font(theme::TEXT_SM).color(t.c.foreground).wrap())
                .child(
                    kit::Input::new(&s.ui.settings.auth_response)
                        .size("sm")
                        .on_edit(|s: &mut App| &mut s.ui.settings.auth_response)
                        .build(t)
                        .w_full()
                        .single_line()
                        .label("Callback URL"),
                )
                .child(
                    kit::Button::new("Continue")
                        .variant("outline")
                        .size("sm")
                        .disabled(empty)
                        .on_press(move |s: &mut App| {
                            let callback = s.ui.settings.auth_response.text.trim().to_owned();
                            request(s, "provider.auth.complete", json!({"instanceId": complete_id, "flowId": flow, "callbackUrl": callback}));
                            s.ui.settings.auth_response.clear();
                        })
                        .build(t)
                        .self_start(),
                ),
        );
    }
    if error {
        let retry = id.clone();
        out = out.child(
            row()
                .gap(8.0)
                .center_y()
                .child(text("Could not load sign-in status.").font(theme::TEXT_XS).color(t.c.destructive))
                .child(kit::Button::new("Retry").variant("ghost").size("xs").on_press(move |s: &mut App| {
                    if let Some(net) = s.src.net() {
                        let payload = json!({"instanceId": retry});
                        net.unwatch_resource("provider.auth.subscribe", payload.clone());
                        net.watch_resource("provider.auth.subscribe", payload);
                    }
                }).build(t)),
        );
    }
    out
}

/// Terminal output without its escape sequences, for the plain-text view of the sign-in PTY.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.next() {
                Some('[') => {
                    while let Some(&n) = chars.peek() {
                        chars.next();
                        if ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(n) = chars.next() {
                        if n == '\u{7}' || (n == '\u{1b}' && chars.peek() == Some(&'\\')) {
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

/// The sign-out confirmation (web's `AlertDialog`).
pub fn confirm(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(id) = s.ui.settings.auth.logout.clone() else { return };
    let live = crate::chat::composer::provider(s, &id);
    let name = live.and_then(|p| p["displayName"].as_str().or(p["driver"].as_str())).unwrap_or(id.as_str()).to_owned();
    let close = |s: &mut App| s.ui.settings.auth.logout = None;
    kit::Dialog::new("provider-sign-out", "Sign out?")
        .open(true)
        .alert()
        .description(format!("Sign out of {name} on {}? This stops running threads that share this sign-in. Thread history is kept.", environment(s)))
        .on_close(close)
        .child(
            row()
                .gap(8.0)
                .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                .child(
                    kit::Button::new("Sign out")
                        .variant("destructive")
                        .on_press(move |s: &mut App| {
                            request(s, "provider.auth.logout", json!({"instanceId": id}));
                            s.ui.settings.auth.logout = None;
                        })
                        .build(t),
                ),
        )
        .build(t, layers);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_row_follows_web_states() {
        let p = json!({"driver": "acpRegistry", "setup": {"canAuthenticate": true}, "auth": {"status": "unknown"}});
        assert!(derive(&p, None).discovering);
        let empty = json!({"phase": "idle", "methods": []});
        assert!(derive(&p, Some(&empty)).needs_external_setup);
        let waiting = json!({"phase": "waiting", "methods": [{"id": "login"}], "interaction": {"type": "terminal"}});
        let d = derive(&p, Some(&waiting));
        assert!(d.active && !d.discovering);
        assert_eq!(description(&p, &waiting, &d, "Mac", false), "Complete sign-in in the terminal below.");
        let done = json!({"phase": "succeeded", "methods": [{"id": "login"}]});
        let d = derive(&p, Some(&done));
        assert!(d.signed_in);
        assert_eq!(start_label(&done, &d), "Change account");
        let codex = json!({"driver": "codex", "setup": {"canAuthenticate": true}, "auth": {"status": "authenticated", "email": "ada@example.com"}});
        let d = derive(&codex, Some(&json!({"phase": "idle"})));
        assert_eq!(description(&codex, &json!({"phase": "idle"}), &d, "Mac", false), "Signed in as •••@example.com");
        let failed = json!({"phase": "failed"});
        let d = derive(&json!({"driver": "cursor", "setup": {}, "auth": {}}), Some(&failed));
        assert_eq!(start_label(&failed, &d), "Retry sign-in");
        assert_eq!(strip_ansi("\u{1b}[1;32mOpen\u{1b}[0m this\r\n"), "Open this\n");
    }
}
