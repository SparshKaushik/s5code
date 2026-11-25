//! The account's own T3 Connect pages (web `MobileClientsUserProfilePage` and
//! `T3ConnectUserProfilePage`): its phones, and the servers registered to it with Deregister.

use rune::{El, col, icon, row, text};
use s5_net::cloud::Failure;
use s5_net::cloud::relay::{self, Device, Linked};

use super::{Event, config, muted, session_token, spawn};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons, kit};

#[derive(Default)]
pub struct Account {
    /// The account's mobile clients once listed, kept through a failed refresh.
    pub devices: Option<Vec<Device>>,
    pub devices_error: Option<String>,
    pub devices_loading: bool,
    /// The server whose Deregister confirmation is open.
    pub confirming: Option<String>,
    pub deregistering: Option<String>,
    /// Deregistered servers by id and link time, hidden until a listing drops them.
    pub removed: Vec<(String, String)>,
}

pub fn refresh_devices(s: &mut App) {
    let Some(config) = config() else { return };
    if s.ui.cloud.identity.is_none() || s.ui.cloud.account.devices_loading {
        return;
    }
    s.ui.cloud.account.devices_loading = true;
    spawn(s, move || Event::Devices(session_token(&config).and_then(|jwt| relay::client_devices(&config.relay, &jwt))));
}

/// Lists the phones when the page first shows; a failed listing waits for Try again.
pub(super) fn poll(s: &mut App) {
    let account = &s.ui.cloud.account;
    if account.devices.is_none() && account.devices_error.is_none() && !account.devices_loading {
        refresh_devices(s);
    }
}

fn deregister(s: &mut App, linked: &Linked) {
    let Some(config) = config() else { return };
    if s.ui.cloud.account.deregistering.is_some() {
        return;
    }
    s.ui.cloud.account.deregistering = Some(linked.id.clone());
    let linked = linked.clone();
    spawn(s, move || {
        let result = session_token(&config).and_then(|jwt| relay::deregister(&config.relay, &jwt, &linked.id));
        Event::Deregistered(linked, result)
    });
}

pub(super) fn devices_landed(s: &mut App, result: Result<Vec<Device>, Failure>) {
    let account = &mut s.ui.cloud.account;
    account.devices_loading = false;
    match result {
        Ok(devices) => {
            account.devices = Some(devices);
            account.devices_error = None;
        }
        Err(failure) => account.devices_error = Some(failure.message),
    }
}

pub(super) fn deregistered(s: &mut App, linked: Linked, result: Result<(), Failure>) {
    s.ui.cloud.account.deregistering = None;
    match result {
        Ok(()) => {
            let account = &mut s.ui.cloud.account;
            account.confirming = None;
            account.removed.push((linked.id.clone(), linked.linked_at.clone()));
            super::refresh(s);
            crate::shell::palette::toast(s, "Server deregistered", Some("T3 Connect access was revoked and a host space is now available."));
        }
        Err(failure) => {
            let actions = failure
                .trace
                .clone()
                .map(|trace| -> crate::shell::palette::ToastAction {
                    ("Copy trace ID".to_owned(), false, std::rc::Rc::new(move |_: &mut App, tx: &mut rune::Tx| tx.copy(trace.clone())))
                })
                .into_iter()
                .collect();
            crate::shell::palette::toast_error_with(s, "Could not deregister server", Some(&failure.message), actions);
        }
    }
}

/// The account's servers, less those just deregistered.
fn servers(s: &App) -> Option<Result<Vec<Linked>, String>> {
    let removed = &s.ui.cloud.account.removed;
    s.ui.cloud.environments.clone().map(|listed| listed.map(|list| list.into_iter().filter(|l| !removed.iter().any(|(id, at)| *id == l.id && *at == l.linked_at)).collect()))
}

/// `Intl.DateTimeFormat` `dateStyle: "medium"`, with `timeStyle: "short"` when `time`.
fn local_date(iso: &str, time: bool) -> Option<String> {
    let full = crate::settings::tasks::draft::medium_short(crate::data::Zone::Local.wall(crate::data::iso_ms(iso)?));
    Some(if time { full } else { full.rsplit_once(", ").map(|(date, _)| date.to_owned()).unwrap_or(full) })
}

fn linked_at(linked: &Linked) -> String {
    local_date(&linked.linked_at, false).map(|date| format!("Linked {date}")).unwrap_or_else(|| "Link date unavailable".into())
}

fn updated_at(device: &Device) -> String {
    local_date(&device.updated_at, true).map(|date| format!("Updated {date}")).unwrap_or_else(|| "Update time unavailable".into())
}

/// Web `ClerkUserProfilePage`: a title, what the page is for, and Refresh.
fn page(title: &str, description: &str, refresh: El, rows: Vec<El>, t: &T) -> El {
    let mut card = vec![row().w_full().px(16.0).py(12.0).child(muted(description, t))];
    card.extend(rows);
    crate::settings::controls::section_with_actions(title, vec![refresh], card, t)
}

fn refresh_button(busy: bool, on_press: fn(&mut App), t: &T) -> El {
    kit::Button::new("Refresh").variant("ghost-muted").size("sm").icon(&icons::REFRESH_CW).disabled(busy).on_press(on_press).build(t)
}

fn empty(glyph: &'static rune::Icon, title: &str, description: &str, t: &T) -> El {
    col()
        .w_full()
        .px(16.0)
        .py(24.0)
        .gap(8.0)
        .items_center()
        .child(row().w(32.0).h(32.0).center().radius(8.0).fill(t.c.muted).child(icon(glyph, 16.0).color(t.c.muted_foreground)))
        .child(text(title.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(text(description.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().align(rune::Align::Center))
}

fn failed(title: &str, message: &str, retry: Option<El>, t: &T) -> El {
    row()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(12.0)
        .center_y()
        .role("alert")
        .child(col().grow(1.0).min_w(0.0).gap(2.0).child(text(title.to_owned()).font(theme::XS_MEDIUM).color(t.c.destructive)).child(muted(message.to_owned(), t)))
        .children(retry)
}

fn device_row(device: &Device, t: &T) -> El {
    let status = |label: &str, on: bool| kit::badge(&format!("{label}: {}", if on { "On" } else { "Off" }), if on { "success" } else { "outline" }, "", t);
    row()
        .key(format!("mobile-client-{}", device.id))
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(12.0)
        .child(icon(&icons::SMARTPHONE, 16.0).color(t.c.muted_foreground))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .child(
                    row()
                        .w_full()
                        .gap(12.0)
                        .child(
                            col()
                                .grow(1.0)
                                .min_w(0.0)
                                .gap(2.0)
                                .child(text(device.label.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate())
                                .child(text(device.platform()).font(theme::TEXT_XS).color(t.c.muted_foreground)),
                        )
                        .child(text(updated_at(device)).font(theme::TEXT_2XS).color(t.c.muted_foreground.alpha_mul(0.75))),
                )
                .child(row().mt(8.0).gap(6.0).flex_wrap().child(status("Push notifications", device.notifications)).child(status("Live Activities", device.live_activities)))
                .child(text(device.detail()).mt(6.0).font(theme::TEXT_XS).color(t.c.muted_foreground.alpha_mul(0.8)).wrap()),
        )
}

/// Web `MobileClientsUserProfilePage`.
fn mobile_clients(s: &App, t: &T) -> El {
    let account = &s.ui.cloud.account;
    let mut rows = vec![];
    if let Some(error) = &account.devices_error {
        let retry = kit::Button::new("Try again").variant("outline").size("xs").disabled(account.devices_loading).on_press(refresh_devices).build(t);
        rows.push(failed("Could not load mobile clients", error, Some(retry), t));
    }
    match &account.devices {
        None if account.devices_error.is_none() => rows.push(row().w_full().px(16.0).py(12.0).role("status").label("Loading mobile clients").child(muted("Loading mobile clients…", t))),
        None => {}
        Some(devices) if devices.is_empty() => rows.push(empty(&icons::SMARTPHONE, "No mobile clients", "Install S5 Code on your phone and sign in to get push notifications and Live Activities.", t)),
        Some(devices) => rows.extend(devices.iter().map(|d| device_row(d, t))),
    }
    page("Mobile clients", "Mobile devices that get notifications from your environments.", refresh_button(account.devices_loading, refresh_devices, t), rows, t)
}

fn server_row(s: &App, linked: &Linked, t: &T) -> El {
    let account = &s.ui.cloud.account;
    let pending = account.deregistering.is_some();
    let open = account.confirming.as_deref() == Some(linked.id.as_str());
    let label = if linked.label.is_empty() { linked.id.clone() } else { linked.label.clone() };
    let id = linked.id.clone();
    let toggle = kit::Button::new("Deregister")
        .variant("destructive-outline")
        .size("sm")
        .disabled(pending)
        .on_press(move |s: &mut App| {
            let confirming = &mut s.ui.cloud.account.confirming;
            *confirming = if confirming.as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) };
        })
        .build(t);
    let mut body = col().grow(1.0).min_w(0.0).child(
        row()
            .w_full()
            .gap(16.0)
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(4.0)
                    .child(text(label.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate())
                    .child(muted(format!("{} · {}", linked_at(linked), if linked.managed { "Managed tunnel" } else { "Activity publishing only" }), t)),
            )
            .child(toggle),
    );
    if open {
        let target = linked.clone();
        body = body.child(
            col()
                .key(format!("t3-connect-deregister-{}", linked.id))
                .mt(12.0)
                .w_full()
                .px(20.0)
                .py(16.0)
                .radius(8.0)
                .border(1.0, t.c.input)
                .fill(t.c.muted.alpha_mul(0.32))
                .role("group")
                .label(format!("Confirm deregistration of {label}"))
                .child(text("Deregister server").font(theme::weight(theme::TEXT_SM, theme::SEMIBOLD)).color(t.c.foreground))
                .child(row().mt(4.0).child(muted(format!("“{label}” will be removed from this account."), t)))
                .child(row().mt(16.0).max_w(576.0).child(muted(
                    "T3 Connect access will be revoked, any managed tunnel will be removed, and a host space will become available. Local connections on your devices are not changed.",
                    t,
                )))
                .child(
                    row()
                        .mt(16.0)
                        .w_full()
                        .gap(8.0)
                        .child(rune::el().grow(1.0))
                        .child(kit::Button::new("Cancel").variant("ghost").size("sm").disabled(pending).on_press(|s: &mut App| s.ui.cloud.account.confirming = None).build(t))
                        .child(
                            kit::Button::new(if pending { "Deregistering…" } else { "Deregister" })
                                .variant("destructive")
                                .size("sm")
                                .disabled(pending)
                                .on_press(move |s: &mut App| deregister(s, &target))
                                .build(t),
                        ),
                ),
        );
    }
    row()
        .key(format!("t3-connect-server-{}", linked.id))
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(12.0)
        .child(icon(&icons::SERVER, 16.0).color(t.c.muted_foreground))
        .child(body)
}

/// Web `T3ConnectUserProfilePage`. Its Settings pointer reads "above" here, since it shares the
/// page with this device's connections.
fn registered(s: &App, t: &T) -> El {
    let mut rows = vec![];
    let listed = servers(s);
    if let Some(Err(message)) = &listed {
        rows.push(failed("Could not load T3 Connect environments", message, None, t));
    }
    match listed {
        None => rows.push(row().w_full().px(16.0).py(12.0).role("status").child(muted("Loading environments…", t))),
        Some(Ok(list)) if list.is_empty() => {
            rows.push(empty(&icons::SERVER, "No T3 Connect environments", "Link an environment from its local Settings to make it available through T3 Connect.", t))
        }
        Some(Ok(list)) => rows.extend(list.iter().map(|l| server_row(s, l, t))),
        Some(Err(_)) => {}
    }
    let busy = s.ui.cloud.loading || s.ui.cloud.account.deregistering.is_some();
    page("Registered environments", "Environments registered to your account. Connections on this device are managed above.", refresh_button(busy, super::refresh, t), rows, t)
}

/// Settings → Connections, below T3 Connect, while signed in.
pub fn sections(s: &App, t: &T) -> Vec<El> {
    if config().is_none() || s.ui.cloud.identity.is_none() || s.ui.cloud.signing_in.is_some() {
        return vec![];
    }
    vec![registered(s, t), mobile_clients(s, t)]
}

/// Scenes: two phones, and Deregister open on the first server.
pub(crate) fn scene(s: &mut App) {
    let list = serde_json::json!({"devices": [
        {"deviceId": "d-1", "label": "Ada's iPhone", "platform": "ios", "iosMajorVersion": 26, "appVersion": "1.6.0",
         "notifications": {"enabled": true, "notifyOnApproval": true, "notifyOnInput": true, "notifyOnCompletion": false, "notifyOnFailure": true},
         "liveActivities": {"enabled": true}, "updatedAt": "2026-11-24T16:42:00.000Z"},
        {"deviceId": "d-2", "label": "Pixel 9", "platform": "android", "iosMajorVersion": null, "appVersion": "1.5.2",
         "notifications": {"enabled": false, "notifyOnApproval": true, "notifyOnInput": true, "notifyOnCompletion": true, "notifyOnFailure": true},
         "liveActivities": {"enabled": false}, "updatedAt": "2026-11-19T09:05:00.000Z"}]});
    s.ui.cloud.account.devices = relay::devices(&list).ok();
    s.ui.cloud.account.confirming = Some("env-box".into());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_in() -> App {
        let mut s = App::new();
        super::super::scene_signed_in(&mut s);
        s
    }

    #[test]
    fn a_deregistered_server_hides_until_the_listing_drops_it_and_a_failure_keeps_it() {
        let mut s = signed_in();
        let listed = |s: &App| match servers(s) {
            Some(Ok(list)) => list.into_iter().map(|l| l.id).collect::<Vec<_>>(),
            _ => vec![],
        };
        let boxed = s.ui.cloud.environments.clone().unwrap().unwrap()[0].clone();
        s.ui.cloud.account.confirming = Some("env-box".into());
        s.ui.cloud.account.deregistering = Some("env-box".into());
        deregistered(&mut s, boxed.clone(), Err(Failure { message: "Relay unavailable".into(), trace: Some("trace-1".into()), status: Some(503) }));
        assert_eq!(listed(&s), ["env-box", "env-studio"]);
        assert_eq!(s.ui.cloud.account.confirming.as_deref(), Some("env-box"), "a failure leaves the confirmation open");
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Could not deregister server"));
        assert_eq!(s.ui.cloud.account.deregistering, None);

        deregistered(&mut s, boxed.clone(), Ok(()));
        assert_eq!(listed(&s), ["env-studio"], "hidden before the relay's listing catches up");
        assert_eq!(s.ui.cloud.account.confirming, None);
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Server deregistered"));

        let mut relinked = s.ui.cloud.environments.clone().unwrap().unwrap();
        relinked[0].linked_at = "2026-11-25T00:00:00.000Z".into();
        s.ui.cloud.environments = Some(Ok(relinked));
        assert_eq!(listed(&s), ["env-box", "env-studio"], "linked again later, it shows again");
    }

    #[test]
    fn mobile_clients_keep_their_list_through_a_failed_refresh() {
        let mut s = signed_in();
        scene(&mut s);
        s.ui.cloud.account.devices_loading = true;
        devices_landed(&mut s, Err(Failure::new("Could not list mobile clients.")));
        assert_eq!(s.ui.cloud.account.devices.as_ref().map(Vec::len), Some(2));
        assert_eq!(s.ui.cloud.account.devices_error.as_deref(), Some("Could not list mobile clients."));
        assert!(!s.ui.cloud.account.devices_loading);
        devices_landed(&mut s, Ok(vec![]));
        assert_eq!((s.ui.cloud.account.devices.as_ref().map(Vec::len), s.ui.cloud.account.devices_error.as_deref()), (Some(0), None));
    }

    #[test]
    fn dates_read_like_intl_medium() {
        assert_eq!(linked_at(&Linked { linked_at: "not a date".into(), ..Linked::default() }), "Link date unavailable");
        assert!(linked_at(&Linked { linked_at: "2026-09-14T12:00:00.000Z".into(), ..Linked::default() }).starts_with("Linked Sep 1"));
        assert!(!linked_at(&Linked { linked_at: "2026-09-14T12:00:00.000Z".into(), ..Linked::default() }).contains(':'), "a date, no time");
    }
}
