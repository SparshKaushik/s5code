use super::{controls as c, http, request};
use crate::{
    App,
    theme::{self, T},
};
use rune::{El, col, row, text};
use serde_json::{Value, json};

pub fn view(s: &App, t: &T) -> El {
    let environment = &s.state().config.as_ref().unwrap_or(&Value::Null)["environment"];
    let mut page = col().w_full().gap(32.0).child(c::section(
        "Environments",
        vec![
            col()
                .padding(16.0)
                .gap(4.0)
                .child(
                    text(
                        environment["label"]
                            .as_str()
                            .unwrap_or("Environment")
                            .to_owned(),
                    )
                    .font(theme::SM_MEDIUM)
                    .color(t.c.foreground),
                )
                .child(
                    text(format!(
                        "{} · {}",
                        environment["platform"]["os"].as_str().unwrap_or(""),
                        environment["serverVersion"].as_str().unwrap_or("")
                    ))
                    .font(theme::TEXT_XS)
                    .color(t.c.muted_foreground),
                ),
        ],
        t,
    ));
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
        .child(c::switch(s,t,"linkedEnvironmentsEnabled","Allow linked environments"))
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
        clients.push(
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
                            text(
                                client["client"]["label"]
                                    .as_str()
                                    .or(client["client"]["deviceType"].as_str())
                                    .unwrap_or("Client")
                                    .to_owned(),
                            )
                            .font(theme::SM_MEDIUM)
                            .color(t.c.foreground),
                        )
                        .child(
                            text(if client["current"] == true {
                                "This client"
                            } else if client["connected"] == true {
                                "Connected"
                            } else {
                                "Disconnected"
                            })
                            .font(theme::TEXT_XS)
                            .color(t.c.muted_foreground),
                        ),
                )
                .child(
                    c::button("Revoke", t)
                        .disabled(client["current"] == true)
                        .on_press(move |s: &mut App| {
                            http(
                                s,
                                "POST",
                                "/api/auth/clients/revoke",
                                json!({"sessionId":id}),
                            )
                        }),
                ),
        );
    }
    if clients.is_empty() {
        clients.push(
            text("No connected clients")
                .padding(16.0)
                .font(theme::TEXT_SM)
                .color(t.c.muted_foreground),
        );
    }
    col()
        .w_full()
        .gap(32.0)
        .child(c::section("Pairing links", links, t))
        .child(c::section("Authorized clients", clients, t))
}
