use super::{controls as c, request};
use crate::{
    App,
    theme::{self, T},
};
use rune::{El, col, row, text};
use serde_json::{Value, json};

pub fn open_url(s: &mut App, url: &str) {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        s.ui.settings.error = Some("Only HTTP and HTTPS links can be opened.".into());
        return;
    }
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("rundll32")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(error) = result {
        s.ui.settings.error = Some(error.to_string());
    }
}
pub fn view(s: &App, t: &T, p: &Value) -> El {
    let id = p["instanceId"].as_str().unwrap_or("").to_owned();
    let state = super::resource(s, "provider.auth.subscribe", json!({"instanceId":id}));
    if p["setup"]["canAuthenticate"] != true {
        return rune::el();
    }
    let mut content = col().w_full().padding(12.0).gap(12.0);
    let Some(state) = state else {
        let refresh = id.clone();
        return content.child(c::button("Refresh sign-in status", t).on_press(
            move |s: &mut App| {
                request(
                    s,
                    "server.refreshProviders",
                    json!({"instanceId":refresh,"fresh":true}),
                )
            },
        ));
    };
    if state.get("error").is_some() {
        let refresh = id.clone();
        return content
            .child(
                text("Could not load sign-in status.")
                    .font(theme::TEXT_XS)
                    .color(t.c.destructive),
            )
            .child(c::button("Retry", t).on_press(move |s: &mut App| {
                if let Some(net) = s.src.net() {
                    let payload = json!({"instanceId":refresh});
                    net.unwatch_resource("provider.auth.subscribe", payload.clone());
                    net.watch_resource("provider.auth.subscribe", payload);
                }
            }));
    }
    if let Some(message) = state["message"].as_str() {
        content = content.child(
            text(message.to_owned())
                .font(theme::TEXT_XS)
                .color(t.c.muted_foreground)
                .wrap(),
        );
    }
    let running = matches!(
        state["phase"].as_str(),
        Some("starting" | "waiting" | "verifying")
    );
    if !running && p["setup"]["canAuthenticate"] == true {
        let methods = state["methods"].as_array();
        if methods.is_some_and(|m| !m.is_empty()) {
            for method in methods.into_iter().flatten() {
                let provider = id.clone();
                let method_id = method["id"].clone();
                content=content.child(c::button(method["name"].as_str().unwrap_or("Sign in"),t).on_press(move |s:&mut App|request(s,"provider.auth.start",json!({"instanceId":provider,"methodId":method_id,"callbackMode":"client"}))));
            }
        } else {
            let provider = id.clone();
            content = content.child(c::button("Sign in", t).on_press(move |s: &mut App| {
                request(
                    s,
                    "provider.auth.start",
                    json!({"instanceId":provider,"callbackMode":"client"}),
                )
            }));
        }
    }
    if running {
        let flow = state["flowId"].clone();
        let interaction = &state["interaction"];
        let iid = interaction["id"].clone();
        let provider = id.clone();
        let cancel_flow = flow.clone();
        content = content.child(c::button("Cancel sign-in", t).on_press(move |s: &mut App| {
            request(
                s,
                "provider.auth.cancel",
                json!({"instanceId":provider,"flowId":cancel_flow}),
            )
        }));
        if let Some(url) = interaction["url"]
            .as_str()
            .or(state["authorizationUrl"].as_str())
        {
            let url = url.to_owned();
            let provider = id.clone();
            let consent = interaction["requiresConsent"] == true;
            let interaction_id = iid.clone();
            let flow_id = flow.clone();
            content=content.child(c::button("Open sign-in page",t).on_press(move |s:&mut App|{
                if consent {request(s,"provider.auth.respond",json!({"instanceId":provider,"flowId":flow_id,"interactionId":interaction_id,"response":{"type":"browser","action":"accept"}}));}
                open_url(s,&url);
            }));
        }
        if let Some(code) = interaction["userCode"].as_str() {
            content = content.child(
                text(format!("Enter code: {code}"))
                    .font(theme::MONO_XS)
                    .color(t.c.foreground),
            );
        }
        match interaction["type"].as_str() {
            Some("browser") if interaction["acceptsCallback"] == true => {
                let provider = id.clone();
                let flow_id = flow.clone();
                content=content.child(crate::kit::Input::new(&s.ui.settings.auth_response).placeholder("Paste callback URL").on_edit(|s:&mut App|&mut s.ui.settings.auth_response).build(t).single_line())
                    .child(c::button("Complete sign-in",t).on_press(move |s:&mut App|{let callback=s.ui.settings.auth_response.text.trim().to_owned();request(s,"provider.auth.complete",json!({"instanceId":provider,"flowId":flow_id,"callbackUrl":callback}));s.ui.settings.auth_response.clear();}));
            }
            Some("credentials") => {
                for field in interaction["fields"].as_array().into_iter().flatten() {
                    let key = field["name"].as_str().unwrap_or("").to_owned();
                    let owned = key.clone();
                    let edit =
                        s.ui.settings
                            .auth_fields
                            .get(&key)
                            .cloned()
                            .unwrap_or_else(|| rune::TextEdit::new("").secure());
                    content = content.child(
                        crate::kit::Input::new(&edit)
                            .placeholder(field["label"].as_str().unwrap_or("Credential"))
                            .on_edit(move |s: &mut App| {
                                s.ui.settings
                                    .auth_fields
                                    .entry(owned.clone())
                                    .or_insert_with(|| edit.clone())
                            })
                            .build(t)
                            .single_line()
                            .secure(),
                    );
                }
                let provider = id.clone();
                let fields = interaction["fields"].clone();
                content=content.child(c::button("Submit credentials",t).on_press(move |s:&mut App|{
                    let values:serde_json::Map<String,Value>=fields.as_array().into_iter().flatten().filter_map(|field|{let name=field["name"].as_str()?;Some((name.to_owned(),json!(s.ui.settings.auth_fields.get(name).map(|e|e.text.as_str()).unwrap_or(""))))}).collect();
                    request(s,"provider.auth.respond",json!({"instanceId":provider,"flowId":flow,"interactionId":iid,"response":{"type":"credentials","values":values}}));s.ui.settings.auth_fields.clear();
                }));
            }
            Some("terminal") => {
                content = content.child(
                    text(interaction["output"].as_str().unwrap_or("").to_owned())
                        .font(theme::MONO_XS)
                        .color(t.c.foreground)
                        .wrap(),
                );
            }
            _ => {}
        }
    } else if p["auth"]["status"] == "authenticated" {
        content = content.child(c::button("Sign out", t).on_press(move |s: &mut App| {
            request(s, "provider.auth.logout", json!({"instanceId":id}))
        }));
    }
    c::section("Authentication", vec![content], t)
}
