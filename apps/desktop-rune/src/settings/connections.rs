use super::{controls as c, http, request};
use crate::{
    App,
    theme::{self, AlphaMul, T},
};
use rune::{El, col, el, row, text};
use serde_json::{Value, json};

pub fn view(s: &App, t: &T) -> El {
    let mut page = col().w_full().gap(32.0).child(c::section("Environments", crate::env::settings_rows(s, t), t));
    let mut links = Vec::new();
    if let Some(result) = s.ui.settings.results.get("peerLinks.list") {
        for link in result["links"].as_array().into_iter().flatten() {
            let id = link["environmentId"].clone();
            links.push(
                row()
                    .w_full()
                    .padding(16.0)
                    .gap(12.0)
                    .center_y()
                    .child(
                        col()
                            .grow(1.0)
                            .gap(4.0)
                            .child(
                                text(link["label"].as_str().unwrap_or("Environment").to_owned())
                                    .font(theme::SM_MEDIUM)
                                    .color(t.c.foreground),
                            )
                            .child(
                                text(link["status"].as_str().unwrap_or("").to_owned())
                                    .font(theme::TEXT_XS)
                                    .color(t.c.muted_foreground),
                            ),
                    )
                    .child(c::button("Unlink", t).on_press(move |s: &mut App| {
                        request(s, "peerLinks.unlink", json!({"environmentId":id}))
                    })),
            );
        }
    }
    links.push(col().padding(16.0).gap(12.0)
        .child(text("Link an environment").font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(crate::kit::Input::new(&s.ui.settings.peer_url).placeholder("Environment URL").on_edit(|s:&mut App|&mut s.ui.settings.peer_url).build(t).single_line())
        .child(crate::kit::Input::new(&s.ui.settings.peer_code).placeholder("Pairing code").on_edit(|s:&mut App|&mut s.ui.settings.peer_code).build(t).single_line().secure())
        .child(text("Access").font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(c::select(s,t,"form.peer.access","Environment access",&["read-only","approval-required","auto-accept-edits","auto","full-access"].map(|a|(json!(a),a.to_owned())),224.0))
        .child(row().gap(8.0).child(c::button("Link",t).disabled(s.ui.settings.peer_url.text.trim().is_empty()||s.ui.settings.peer_code.text.trim().is_empty()).on_press(|s:&mut App|{
            let access=s.ui.settings.form.get("form.peer.access").and_then(Value::as_str).unwrap_or("read-only");
            request(s,"peerLinks.link",json!({"url":s.ui.settings.peer_url.text.trim(),"pairingCode":s.ui.settings.peer_code.text.trim(),"access":access}));
            s.ui.settings.peer_code.clear();
        })).child(c::button("Refresh",t).on_press(|s:&mut App|request(s,"peerLinks.list",json!({}))))));
    page = page.child(c::section("Linked environments", links, t));
    page
}
pub fn access(s: &App, t: &T) -> El {
    let state = super::resource(s, "subscribeAuthAccess", json!({}));
    let mut links = Vec::new();
    let mut clients = Vec::new();
    for link in state.into_iter().flat_map(|v| {
        v["payload"]["pairingLinks"]
            .as_array()
            .into_iter()
            .flatten()
    }) {
        let id = link["id"].clone();
        links.push(
            row()
                .w_full()
                .padding(16.0)
                .gap(12.0)
                .center_y()
                .child(
                    text(link["label"].as_str().unwrap_or("Pairing link").to_owned())
                        .font(theme::TEXT_SM)
                        .color(t.c.foreground)
                        .grow(1.0),
                )
                .child(c::button("Revoke", t).on_press(move |s: &mut App| {
                    http(
                        s,
                        "POST",
                        "/api/auth/pairing-links/revoke",
                        json!({"id":id}),
                    )
                })),
        );
    }
    links.push(row().w_full().padding(16.0).child(
        c::button("Create pairing credential", t).on_press(|s: &mut App| {
            http(
                s,
                "POST",
                "/api/auth/pairing-token",
                json!({"label":"Rune desktop"}),
            )
        }),
    ));
    if let Some(credential) =
        s.ui.settings
            .results
            .get("/api/auth/pairing-token")
            .and_then(|v| v["credential"].as_str())
    {
        let code = credential.to_owned();
        links.push(col().w_full().padding(16.0).gap(8.0).child(text("Use this credential to connect another client. It is shown only until Settings closes.").font(theme::TEXT_XS).color(t.c.muted_foreground).wrap())
            .child(c::button("Copy credential",t).on_press_tx(move |_:&mut App,tx|tx.copy(code.clone()))));
    }
    for client in state.into_iter().flat_map(|v| {
        v["payload"]["clientSessions"]
            .as_array()
            .into_iter()
            .flatten()
    }) {
        let id = client["sessionId"].clone();
        let current = client["current"] == true;
        let live = current || client["connected"] == true;
        let info = &client["client"];
        let joined = [info["os"].as_str(), info["browser"].as_str()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
        let label = info["label"]
            .as_str()
            .map(str::to_owned)
            .or((!joined.is_empty()).then_some(joined))
            .unwrap_or_else(|| client["subject"].as_str().unwrap_or("Client").to_owned());
        let device = info["deviceType"].as_str().filter(|d| *d != "unknown").map(|d| {
            let mut chars = d.chars();
            chars.next().map(|c| c.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default()
        });
        let bits: Vec<String> = [device, info["os"].as_str().map(str::to_owned), info["browser"].as_str().map(str::to_owned), info["ipAddress"].as_str().map(str::to_owned)]
            .into_iter()
            .flatten()
            .collect();
        let mut heading = row()
            .min_h(20.0)
            .gap(6.0)
            .center_y()
            .child(el().size(6.0, 6.0).radius(3.0).fill(if live { t.c.success } else { t.c.muted_foreground.alpha_mul(0.3) }))
            .child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground));
        // Web's `ConnectedClientListRow`: the current session gets a chip, never a Revoke button.
        if current {
            heading = heading.child(
                el().px(4.0)
                    .py(2.0)
                    .radius(6.0)
                    .border(1.0, t.c.border.alpha_mul(0.5))
                    .fill(t.c.muted.alpha_mul(0.5))
                    .child(text("This device").font(theme::TEXT_3XS).color(t.c.muted_foreground.alpha_mul(0.8))),
            );
        }
        let mut entry = row()
            .w_full()
            .padding(16.0)
            .gap(12.0)
            .center_y()
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(4.0)
                    .child(heading)
                    .child(
                        text(if bits.is_empty() { if live { "Connected".to_owned() } else { "Not connected".to_owned() } } else { bits.join(" · ") })
                            .font(theme::TEXT_XS)
                            .color(t.c.muted_foreground),
                    ),
            );
        if !current {
            entry = entry.child(
                crate::kit::Button::new("Revoke")
                    .variant("destructive-outline")
                    .size("xs")
                    .on_press(move |s: &mut App| {
                        http(s, "POST", "/api/auth/clients/revoke", json!({"sessionId":id}))
                    })
                    .build(t),
            );
        }
        clients.push(entry);
    }
    if clients.is_empty() {
        clients.push(
            text("No connected clients")
                .padding(16.0)
                .font(theme::TEXT_SM)
                .color(t.c.muted_foreground),
        );
    }
    let mut page = col().w_full().gap(32.0);
    if let Some(network) = crate::desktop::exposure::network_row(s, t) {
        page = page.child(c::section("This machine", vec![network], t));
    }
    page.child(c::section("Pairing links", links, t))
        .child(c::section("Authorized clients", clients, t))
}
