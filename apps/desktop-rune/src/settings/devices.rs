use super::{controls as c, request, value};
use crate::{
    App,
    theme::{self, T},
};
use rune::{El, col, row, text};
use serde_json::json;

pub fn view(s: &App, t: &T) -> El {
    let config = s.state().config.as_ref();
    let enabled = config.is_some_and(|c| c["settings"]["enableDeviceSupport"] == true);
    let mut rows = vec![
        row()
            .w_full()
            .padding(16.0)
            .gap(16.0)
            .center_y()
            .child(
                col()
                    .grow(1.0)
                    .gap(4.0)
                    .child(
                        text("Device support")
                            .font(theme::SM_MEDIUM)
                            .color(t.c.foreground),
                    )
                    .child(
                        text("Use iOS Simulators and Android devices from this environment.")
                            .font(theme::TEXT_XS)
                            .color(t.c.muted_foreground)
                            .wrap(),
                    ),
            )
            .child(
                c::button(if enabled { "Disable" } else { "Set up devices" }, t).on_press(
                    move |s: &mut App| {
                        request(
                            s,
                            "device.configure",
                            json!({"enabled":!enabled,"onboardingCompleted":true}),
                        )
                    },
                ),
            ),
    ];
    if enabled {
        let agent = value(s, "enableAgentDeviceAccess") == true;
        rows.push(
            row()
                .w_full()
                .padding(16.0)
                .gap(16.0)
                .center_y()
                .child(
                    text("Agent device access")
                        .font(theme::SM_MEDIUM)
                        .color(t.c.foreground)
                        .grow(1.0),
                )
                .child(
                    c::button(
                        if agent {
                            "Revoke agent access"
                        } else {
                            "Allow agent access"
                        },
                        t,
                    )
                    .on_press(move |s: &mut App| {
                        request(s, "device.configure", json!({"agentAccessEnabled":!agent}))
                    }),
                ),
        );
        rows.push(
            row()
                .w_full()
                .padding(16.0)
                .child(c::button("Refresh devices", t).on_press(|s: &mut App| {
                    request(s, "device.list", json!({"inspectOnly":true}))
                })),
        );
    }
    let mut page = col()
        .w_full()
        .gap(32.0)
        .child(c::section("Devices", rows, t));
    if let Some(state) = super::resource(s, "subscribeDeviceState", json!({})) {
        if let Some(detail) = state["hostStatusDetail"].as_str() {
            page = page.child(
                text(detail.to_owned())
                    .font(theme::TEXT_SM)
                    .color(t.c.muted_foreground)
                    .wrap(),
            );
        }
        let mut hosts = Vec::new();
        for host in ["hosts", "easHosts", "simlockHosts"]
            .iter()
            .flat_map(|key| state[*key].as_array().into_iter().flatten())
        {
            hosts.push(
                col()
                    .padding(16.0)
                    .gap(4.0)
                    .child(
                        text(host["label"].as_str().unwrap_or("Device host").to_owned())
                            .font(theme::SM_MEDIUM)
                            .color(t.c.foreground),
                    )
                    .child(
                        text(format!(
                            "{} · Hub {} · Agent Device {}",
                            host["kind"].as_str().unwrap_or(""),
                            if host["hubInstalled"] == true {
                                "installed"
                            } else {
                                "not installed"
                            },
                            if host["agentDeviceInstalled"] == true {
                                "installed"
                            } else {
                                "not installed"
                            }
                        ))
                        .font(theme::TEXT_XS)
                        .color(t.c.muted_foreground),
                    ),
            );
        }
        if !hosts.is_empty() {
            page = page.child(c::section("Hosts", hosts, t));
        }
        let mut devices = Vec::new();
        for device in state["devices"].as_array().into_iter().flatten() {
            devices.push(
                col()
                    .padding(16.0)
                    .gap(4.0)
                    .child(
                        text(device["name"].as_str().unwrap_or("Device").to_owned())
                            .font(theme::SM_MEDIUM)
                            .color(t.c.foreground),
                    )
                    .child(
                        text(format!(
                            "{} · {}",
                            device["platform"].as_str().unwrap_or(""),
                            device["state"].as_str().unwrap_or("")
                        ))
                        .font(theme::TEXT_XS)
                        .color(t.c.muted_foreground),
                    ),
            );
        }
        if !devices.is_empty() {
            page = page.child(c::section("Available devices", devices, t));
        }
    }
    page
}
