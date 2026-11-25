//! Web `ProviderSetupSection`: Antigravity's Environment and Runtime rows (install, update,
//! cancel, remove the managed runtime) above its Account row.
use super::super::{failure_detail, resource};
use super::{control_row, plain_row};
use crate::kit;
use crate::theme::{self, T};
use crate::{App, icons};
use rune::{El, col, el, row, text};
use serde_json::{Value, json};

#[derive(Default)]
pub struct Ui {
    /// The install command in flight: its reply id and what it is doing.
    pending: Option<(u64, &'static str)>,
    error: Option<String>,
}

const RUNNING: [&str; 3] = ["downloading", "extracting", "verifying"];

fn mb(bytes: &Value) -> f64 {
    bytes.as_f64().unwrap_or(0.0) / 1_000_000.0
}

/// `installationStatusMessage`: `None` while the install state hasn't loaded.
pub fn install_status(installation: Option<&Value>, installed: bool, custom_binary: bool, enabled: bool) -> String {
    let Some(i) = installation else { return "Checking the Antigravity runtime.".into() };
    match i["phase"].as_str() {
        Some("downloading") => {
            let total = if i["totalBytes"].is_null() { String::new() } else { format!(" of {:.1} MB", mb(&i["totalBytes"])) };
            format!("Downloading {:.1} MB{total}.", mb(&i["downloadedBytes"]))
        }
        Some("extracting") => "Extracting Antigravity.".into(),
        Some("verifying") => "Checking the downloaded runtime.".into(),
        _ if installed => "Installed.".into(),
        _ if custom_binary && enabled => "The configured Antigravity runtime is unavailable.".into(),
        _ if custom_binary => "The configured Antigravity runtime has not been checked.".into(),
        _ if i["totalBytes"].as_f64().is_some_and(|b| b > 0.0) => format!("{} MB download.", mb(&i["totalBytes"]).ceil()),
        _ => "Not installed.".into(),
    }
}

/// The install button's label.
pub fn install_label(installation: Option<&Value>, installed: bool) -> &'static str {
    let i = installation.unwrap_or(&Value::Null);
    if i["installedVersion"].is_string() {
        if i["version"].is_string() && i["version"] != i["installedVersion"] { "Update Antigravity" } else { "Reinstall Antigravity" }
    } else if matches!(i["phase"].as_str(), Some("failed" | "cancelled")) {
        "Retry installation"
    } else if installed {
        "Install managed runtime"
    } else {
        "Install Antigravity"
    }
}

fn run(s: &mut App, label: &'static str, method: &str, payload: Value) {
    if s.ui.settings.providers_ui.setup.pending.is_some() {
        return;
    }
    s.ui.settings.providers_ui.setup.error = None;
    if let Some(id) = super::send(s, method, payload) {
        s.ui.settings.providers_ui.setup.pending = Some((id, label));
    }
}

pub fn poll(s: &mut App) -> bool {
    let Some((id, _)) = s.ui.settings.providers_ui.setup.pending else { return false };
    let Some(reply) = super::take(s, id) else { return false };
    let ui = &mut s.ui.settings.providers_ui.setup;
    ui.pending = None;
    if let Err(error) = reply {
        ui.error = Some(failure_detail(&error).unwrap_or_else(|| "Provider setup failed. Try again.".into()));
    }
    true
}

pub fn antigravity(s: &App, t: &T, id: &str, instance: &Value, live: Option<&Value>, enabled: bool, read_only: bool) -> Vec<El> {
    let environment = s.state().config.as_ref().and_then(|c| c["environment"]["label"].as_str()).unwrap_or("this environment").to_owned();
    let mut device = col().gap(8.0).items_end().child(text(environment.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap());
    if !enabled && !read_only {
        let owned = id.to_owned();
        device = device.child(
            kit::Button::new("Enable Antigravity")
                .variant("outline")
                .size("sm")
                .on_press(move |s: &mut App| super::update(s, &owned, "enabled", json!(true)))
                .build(t),
        );
    }
    let mut rows = vec![control_row(t, &super::field_key(id, "setup-environment"), "Environment", "Device that runs this provider.", device)];
    match live {
        _ if read_only => rows.push(plain_row(t, "Setup unavailable", "Provider setup is read-only.")),
        Some(p) if p["setup"].is_object() => rows.extend(actions(s, t, id, instance, p, enabled, &environment)),
        _ => rows.push(plain_row(t, "Update required", "Update this environment to manage Antigravity.")),
    }
    rows
}

fn actions(s: &App, t: &T, id: &str, instance: &Value, p: &Value, enabled: bool, environment: &str) -> Vec<El> {
    let target = json!({"instanceId": id});
    let auth = resource(s, "provider.auth.subscribe", target.clone());
    let installation = resource(s, "provider.install.subscribe", target.clone());
    let ui = &s.ui.settings.providers_ui.setup;
    let auth_active = auth.is_some_and(|a| matches!(a["phase"].as_str(), Some("starting" | "waiting" | "verifying")));
    let install_active = installation.is_some_and(|i| RUNNING.contains(&i["phase"].as_str().unwrap_or("")));
    let custom_binary = instance["config"]["binaryPath"].as_str().is_some_and(|b| !b.trim().is_empty());
    let installed = p["installed"] == true || (!custom_binary && installation.is_some_and(|i| i["installedVersion"].is_string()));
    let query_error = [auth, installation].into_iter().flatten().find_map(|r| r.get("error"));
    let disabled = ui.pending.is_some() || query_error.is_some();
    let status = install_status(installation, installed, custom_binary, enabled);
    let can_install = p["setup"]["canInstall"] == true;

    let mut notes = col().gap(8.0);
    if custom_binary {
        notes = notes.child(text("Uses the custom binary path below. Installation keeps that path.").font(theme::TEXT_XS).color(t.c.muted_foreground).wrap());
    }
    if !installed && !can_install {
        notes = notes.child(text("Automatic installation unavailable. Set a binary path or use another environment.").font(theme::TEXT_XS).color(t.c.muted_foreground).wrap());
    }
    let mut control = col().w(224.0).gap(8.0).items_end().child(text(status.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground).align(rune::Align::End).wrap());
    if let Some(i) = installation.filter(|i| i["phase"] == "downloading" && i["totalBytes"].as_f64().is_some_and(|b| b > 0.0)) {
        let ratio = (i["downloadedBytes"].as_f64().unwrap_or(0.0) / i["totalBytes"].as_f64().unwrap_or(1.0)).clamp(0.0, 1.0) as f32;
        control = control.child(
            el().w_full().h(4.0).radius(theme::radius::FULL).fill(t.c.muted).label("Antigravity download").child(el().w_frac(ratio).h(4.0).radius(theme::radius::FULL).fill(t.c.primary)),
        );
    }
    if let Some(message) = installation.and_then(|i| i["message"].as_str()).filter(|m| !install_active && *m != status) {
        control = control.child(text(message.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap());
    }
    let mut buttons = row().gap(8.0).center_y();
    if installation.is_some_and(|i| i["canRemove"] == true) && !install_active {
        let (owned, environment) = (id.to_owned(), environment.to_owned());
        buttons = buttons.child(
            kit::Button::new("Remove downloaded runtime")
                .variant("ghost")
                .size("icon-sm")
                .icon(&icons::TRASH_2)
                .disabled(disabled || auth_active)
                .on_press(move |s: &mut App| {
                    let owned = owned.clone();
                    crate::shell::confirm::request(
                        s,
                        format!("Remove the downloaded Antigravity runtime from {environment}? Google sign-in and thread history are kept."),
                        false,
                        move |s: &mut App| run(s, "Removing runtime", "provider.install.remove", json!({"instanceId": owned})),
                    );
                })
                .build(t)
                .help("Remove downloaded runtime"),
        );
    }
    if let Some(operation) = installation.filter(|_| install_active).and_then(|i| i["operationId"].as_str()) {
        let payload = json!({"instanceId": id, "operationId": operation});
        buttons = buttons.child(
            kit::Button::new("Cancel installation")
                .variant("outline")
                .size("sm")
                .disabled(disabled)
                .on_press(move |s: &mut App| run(s, "Cancelling installation", "provider.install.cancel", payload.clone()))
                .build(t),
        );
    } else if !install_active && can_install {
        let payload = target.clone();
        buttons = buttons.child(
            kit::Button::new(install_label(installation, installed))
                .variant("outline")
                .size("sm")
                .disabled(disabled || auth_active)
                .on_press(move |s: &mut App| run(s, "Starting installation", "provider.install.start", payload.clone()))
                .build(t),
        );
    }
    control = control.child(buttons);
    let mut runtime = col()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(8.0)
        .child(
            row()
                .w_full()
                .gap(32.0)
                .items_start()
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(4.0)
                        .child(text("Runtime").font(theme::SM_MEDIUM).color(t.c.foreground))
                        .child(text("Install and manage Antigravity.").font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap())
                        .child(notes),
                )
                .child(control),
        );
    if let Some((_, label)) = ui.pending {
        runtime = runtime.label(format!("{label}."));
    }
    let mut rows = vec![runtime, col().w_full().px(16.0).child(super::super::auth::view(s, t, p))];
    let error = ui.error.clone().or_else(|| query_error.map(|e| failure_detail(e).unwrap_or_else(|| "Could not load setup status.".into())));
    if let Some(error) = error {
        let mut block = col().w_full().px(16.0).py(12.0).gap(8.0).child(text(error).font(theme::TEXT_XS).color(t.c.destructive).wrap().role("alert"));
        if query_error.is_some() {
            let owned = id.to_owned();
            block = block.child(
                kit::Button::new("Retry setup status")
                    .variant("outline")
                    .size("sm")
                    .on_press(move |s: &mut App| {
                        if let Some(net) = s.src.net() {
                            let payload = json!({"instanceId": owned});
                            for method in ["provider.auth.subscribe", "provider.install.subscribe"] {
                                net.unwatch_resource(method, payload.clone());
                                net.watch_resource(method, payload.clone());
                            }
                        }
                    })
                    .build(t)
                    .self_start(),
            );
        }
        rows.push(block);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_status_and_install_label_follow_web() {
        assert_eq!(install_status(None, false, false, true), "Checking the Antigravity runtime.");
        let downloading = json!({"phase": "downloading", "downloadedBytes": 12_340_000, "totalBytes": 80_000_000});
        assert_eq!(install_status(Some(&downloading), false, false, true), "Downloading 12.3 MB of 80.0 MB.");
        let unknown_total = json!({"phase": "downloading", "downloadedBytes": 1_000_000, "totalBytes": null});
        assert_eq!(install_status(Some(&unknown_total), false, false, true), "Downloading 1.0 MB.");
        let idle = json!({"phase": "idle", "downloadedBytes": 0, "totalBytes": 80_200_000, "installedVersion": null});
        assert_eq!(install_status(Some(&idle), false, false, true), "81 MB download.");
        assert_eq!(install_status(Some(&idle), false, true, false), "The configured Antigravity runtime has not been checked.");
        assert_eq!(install_status(Some(&idle), true, false, true), "Installed.");

        assert_eq!(install_label(Some(&idle), false), "Install Antigravity");
        assert_eq!(install_label(Some(&idle), true), "Install managed runtime");
        assert_eq!(install_label(Some(&json!({"phase": "failed"})), false), "Retry installation");
        assert_eq!(install_label(Some(&json!({"installedVersion": "1.0", "version": "1.1"})), true), "Update Antigravity");
        assert_eq!(install_label(Some(&json!({"installedVersion": "1.1", "version": "1.1"})), true), "Reinstall Antigravity");
    }
}
