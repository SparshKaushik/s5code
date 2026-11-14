use super::{controls as c, request};
use crate::kit::{self, Menu, Placement};
use crate::{
    App, Route,
    theme::{self, AlphaMul, T},
};
use rune::{El, col, el, row, text};
use serde_json::{Value, json};

#[derive(Default)]
pub struct Ui {
    /// The saved update track, read once per visit (Electron's `updateState.channel`).
    track: Option<String>,
    /// The track a switch is being confirmed to, its release lookup, and the reply.
    switch: Option<String>,
    lookup: Option<u64>,
    release: Option<Result<Value, String>>,
    track_open: bool,
    /// The store whose QR popover is open.
    store: Option<usize>,
}

const TRACK_KEY: &str = "updateChannel";

/// Web `MobileAppSettingsRow`'s stores: button label, store name, link.
const STORES: &[(&str, &str, &str)] = &[
    ("iPhone and iPad", "App Store", "https://apps.apple.com/us/app/t3-code-remote-claude-more/id6787819824"),
    ("Android", "Google Play", "https://play.google.com/store/apps/details?id=com.t3tools.t3code"),
];

/// Shared `cliReleaseChannelOf`: `X-nightly.YYYYMMDD.N` and `X-preview.YYYYMMDD.N`, else stable.
fn release_channel(version: &str) -> &'static str {
    let Some((base, rest)) = version.split_once('-') else { return "stable" };
    if base.is_empty() || base.contains('+') {
        return "stable";
    }
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    for channel in ["nightly", "preview"] {
        let tail = rest.strip_prefix(channel).and_then(|r| r.strip_prefix('.'));
        if let Some((date, n)) = tail.and_then(|t| t.split_once('.')) {
            if date.len() == 8 && digits(date) && digits(n) {
                return channel;
            }
        }
    }
    "stable"
}

/// Shared `desktopUpdateChannelOf`.
fn update_channel(version: &str) -> &'static str {
    if release_channel(version) == "nightly" { "nightly" } else { "latest" }
}

/// Web `channelSwitchLabel`.
fn track_label(channel: &str) -> &'static str {
    if channel == "nightly" { "Nightly" } else { "Stable" }
}

/// The policy of the environment this app hosts (web `usePrimaryManagedPolicy`).
fn primary_policy(s: &App) -> Option<&Value> {
    let config = crate::env::source(s, crate::env::LOCAL)?.state().config.as_ref()?;
    let policy = &config["managedPolicy"];
    policy["organization"].is_string().then_some(policy)
}

fn organization(policy: &Value) -> &str {
    policy["organization"].as_str().unwrap_or_default()
}

/// Contracts `managedUpdatesDenial`.
fn updates_denial(policy: Option<&Value>) -> Option<String> {
    let policy = policy?;
    (policy["limits"]["updates"]["mode"] == "off").then(|| format!("{} manages T3 Code updates.", organization(policy)))
}

/// Web's update-track description when the policy allows only stable releases.
fn nightly_denied(policy: &Value, selected: &str) -> String {
    format!(
        "{} manages T3 Code updates. Nightly builds are not available{}",
        organization(policy),
        if selected == "nightly" { "; switch to Stable to keep getting updates." } else { "." }
    )
}

/// Connected machines a track switch would move (web `awaitsTrackSwitch`): servers this app
/// doesn't host, off `channel`, that this session may update.
fn awaiting(s: &App, channel: &str) -> Vec<(String, String)> {
    crate::env::keys(s)
        .into_iter()
        .filter(|key| key != crate::env::LOCAL && crate::env::connected(s, key))
        .filter_map(|key| {
            let config = crate::env::source(s, &key)?.state().config.as_ref()?;
            let version = config["environment"]["serverVersion"].as_str()?;
            let updatable = config["environment"]["capabilities"]["serverSelfUpdate"].as_str().is_some_and(|c| c != "unsupported");
            let on_track = release_channel(version) != "preview" && update_channel(version) == channel;
            (updatable && !on_track && super::permissions::can_on(s, &key, "environment:maintain")).then(|| (key.clone(), crate::env::label(s, &key)))
        })
        .collect()
}

pub(super) fn poll(s: &mut App) -> bool {
    if !matches!(&s.route, Route::Settings(p) if p == "about") {
        return false;
    }
    let mut changed = false;
    if s.ui.settings.about.track.is_none() {
        s.ui.settings.about.track = Some(crate::desktop::prefs().get::<String>(TRACK_KEY).unwrap_or_else(|| "latest".into()));
        changed = true;
    }
    if let Some(id) = s.ui.settings.about.lookup {
        if let Some(reply) = s.src.net().and_then(|n| n.take_reply(id)) {
            let ui = &mut s.ui.settings.about;
            ui.lookup = None;
            ui.release = Some(reply.map_err(|e| e["message"].as_str().map_or_else(|| e.to_string(), str::to_owned)));
            changed = true;
        }
    }
    changed
}

fn open_switch(s: &mut App, channel: String) {
    let id = s.src.net().map(|net| net.request("server.getNpmRelease", json!({"spec": channel})));
    let ui = &mut s.ui.settings.about;
    ui.release = id.is_none().then(|| Err("Connect to an environment to check releases.".into()));
    ui.lookup = id;
    ui.switch = Some(channel);
}

fn close_switch(s: &mut App) {
    let ui = &mut s.ui.settings.about;
    ui.switch = None;
    ui.lookup = None;
    ui.release = None;
}

pub(super) fn escape(s: &mut App) -> bool {
    let ui = &mut s.ui.settings.about;
    if ui.store.take().is_some() || std::mem::take(&mut ui.track_open) {
        return true;
    }
    if ui.switch.is_some() {
        close_switch(s);
        return true;
    }
    false
}

pub fn view(s: &App, t: &T) -> El {
    let version = s
        .state()
        .config
        .as_ref()
        .and_then(|c| c["environment"]["serverVersion"].as_str())
        .unwrap_or("Unknown");
    let update = s.ui.settings.results.get("server.getNpmRelease");
    let target = update.and_then(|v| v["version"].as_str());
    let can_update = s
        .state()
        .config
        .as_ref()
        .and_then(|c| c["environment"]["capabilities"]["serverSelfUpdate"].as_str())
        .is_some_and(|capability| capability != "unsupported");
    let policy = primary_policy(s);
    let mut app = Vec::new();
    if let Some(policy) = policy {
        let control = match policy["supportUrl"].as_str() {
            Some(url) => {
                let url = url.to_owned();
                c::button("Get help", t).on_press(move |s: &mut App| super::auth::open_url(s, &url))
            }
            None => el(),
        };
        app.push(about_row(
            t,
            "Organization",
            &format!(
                "This environment is managed by {}. Some settings and features follow its policy.",
                organization(policy)
            ),
            control,
        ));
    }
    let denial = updates_denial(policy);
    // Web's About version row: Electron's app update, which Rune builds have no feed for.
    app.push(about_row(
        t,
        &format!("Version {}", env!("CARGO_PKG_VERSION")),
        denial.as_deref().unwrap_or(crate::desktop::NO_UPDATE_FEED),
        c::button("Check for Updates", t).on_press(crate::desktop::check_for_updates),
    ));
    app.push(track_row(s, t, policy, denial.is_some()));
    // Web keeps this under Connections (`ServerUpdateAction`); here it names the server apart.
    let mut controls = row().gap(8.0).child(
        c::button("Check Server Release", t)
            .on_press(|s: &mut App| request(s, "server.getNpmRelease", json!({"spec":"latest"}))),
    );
    // One update state with the chat banner (`chat::server_update`).
    use crate::chat::server_update::{self as update, Status};
    let description = match update::status(s) {
        Status::Running { stage, target, .. } => {
            controls = controls.child(kit::Button::new(stage.label()).size("sm").variant("outline").disabled(true).build(t));
            format!("Version {version} · Updating to {target}")
        }
        Status::Failed { target, message, .. } if !update::dismissed(s) => {
            let retry = target.clone();
            controls = controls
                .child(c::button("Retry", t).disabled(!can_update).on_press(move |s: &mut App| update::start(s, &retry)))
                .child(c::button("Dismiss", t).on_press(update::dismiss));
            message.clone()
        }
        _ => {
            if let Some(target) = target.filter(|target| *target != version) {
                let target = target.to_owned();
                controls = controls.child(
                    c::button("Update", t)
                        .disabled(!can_update)
                        .on_press(move |s: &mut App| s.ui.settings.update_version = Some(target.clone())),
                );
            }
            match target {
                Some(v) if v != version => format!("Version {version} · Latest release: {v}"),
                _ => format!("Version {version}"),
            }
        }
    };
    app.push(about_row(t, "Server", &description, controls));
    // Web's `CliCommandSettingsRow`, desktop only.
    #[cfg(unix)]
    if let Some(cli) = crate::desktop::cli::row(s) {
        let button = c::button(cli.action, t).disabled(cli.disabled).on_press(crate::desktop::cli::press);
        app.push(about_row(t, "t3 command", &cli.description, button));
    }
    app.push(mobile_row(s, t));
    let telemetry_off = policy
        .filter(|p| p["limits"]["telemetry"] == false)
        .map(|p| format!("How we handle your data. {} turned off anonymous usage data.", organization(p)));
    col()
        .w_full()
        .gap(32.0)
        .child(c::section("App", app, t))
        .child(c::section(
            "Legal",
            vec![
                about_row(
                    t,
                    "Privacy policy",
                    telemetry_off
                        .as_deref()
                        .unwrap_or("How we handle your data, including the anonymous usage data T3 Code collects."),
                    c::button("View policy", t).on_press(|s: &mut App| {
                        super::auth::open_url(s, "https://t3.codes/privacy-policy")
                    }),
                ),
                about_row(
                    t,
                    "Open source licenses",
                    "Notices for dependencies, assets, and optional tools used by T3 Code.",
                    c::button("View licenses", t).on_press(|s: &mut App| {
                        s.navigate(Route::Settings("open-source-licenses".into()))
                    }),
                ),
            ],
            t,
        ))
}

/// Web's desktop `update-track` row: Stable or Nightly, for this app and every connected machine.
fn track_row(s: &App, t: &T, policy: Option<&Value>, managed: bool) -> El {
    let ui = &s.ui.settings.about;
    let selected = ui.track.clone().unwrap_or_else(|| "latest".into());
    let denied = policy.filter(|p| p["limits"]["updates"]["allowNightly"] == false);
    // Rune has no update feed, so only connected servers can be off the track.
    let unswitched = awaiting(s, &selected).len();
    let description: El = if let Some(policy) = denied {
        text(nightly_denied(policy, &selected)).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap()
    } else if unswitched > 0 {
        let other = track_label(if selected == "nightly" { "latest" } else { "nightly" });
        let count = if unswitched == 1 { "1 machine still runs".to_owned() } else { format!("{unswitched} machines still run") };
        let review = selected.clone();
        row()
            .gap(4.0)
            .center_y()
            .child(text(format!("{count} {other}.")).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground))
            .child(
                kit::Button::new("Review")
                    .variant("link")
                    .size("xs")
                    .on_press(move |s: &mut App| open_switch(s, review.clone()))
                    .build(t),
            )
    } else {
        text("Switches this app and every connected machine whose database the target release supports.")
            .font(theme::TEXT_XS.lh(18.0))
            .color(t.c.muted_foreground)
            .wrap()
    };
    let control = track_select(s, managed).open(false).build(&kit::theme(t), &mut Vec::new());
    row()
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
                .child(text("Update track").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(description),
        )
        .child(control)
}

fn track_select(s: &App, managed: bool) -> kit::Select {
    let ui = &s.ui.settings.about;
    let selected = ui.track.as_deref().unwrap_or("latest");
    let busy = ui.switch.is_some();
    let mut select = kit::Select::new("update-track", &["latest", "nightly"])
        .label("Update track")
        .width(160.0)
        .size("sm")
        .current(Some(selected))
        .option_name("latest", "Stable")
        .option_name("nightly", "Nightly")
        .open(ui.track_open && !busy && !managed)
        .disabled(busy || managed)
        .on_toggle(|s: &mut App| s.ui.settings.about.track_open ^= true)
        .on_close(|s: &mut App| s.ui.settings.about.track_open = false)
        .on_choose(|s: &mut App, value: &str, _: &mut rune::Tx| {
            s.ui.settings.about.track_open = false;
            if s.ui.settings.about.track.as_deref() != Some(value) {
                open_switch(s, value.to_owned());
            }
        });
    let denied = primary_policy(s).is_some_and(|p| p["limits"]["updates"]["allowNightly"] == false);
    if denied && selected != "nightly" {
        select = select.disabled_option("nightly");
    }
    select
}

/// Web `MobileAppSettingsRow`: a button per store, each opening its QR code.
fn mobile_row(s: &App, t: &T) -> El {
    let open = s.ui.settings.about.store;
    let mut buttons = row().gap(8.0);
    for (i, (label, ..)) in STORES.iter().enumerate() {
        buttons = buttons.child(
            row().key(format!("mobile-store-{i}")).child(
                kit::Button::new(*label)
                    .variant("outline")
                    .size("sm")
                    .on_press(move |s: &mut App| s.ui.settings.about.store = (open != Some(i)).then_some(i))
                    .build(t)
                    .expanded(open == Some(i)),
            ),
        );
    }
    about_row(t, "Mobile app", "Connect to your server from your phone or tablet.", buttons)
}

/// The store QR popover and the track switch dialog.
pub fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    if let Some(i) = s.ui.settings.about.store {
        let (_, store, url) = STORES[i];
        let link = url.to_owned();
        let body = col()
            .w_full()
            .padding(12.0)
            .gap(12.0)
            .items_center()
            .child(text(store).font(theme::SM_MEDIUM).color(t.c.foreground))
            .child(text("Scan with your phone camera.").font(theme::TEXT_XS).color(t.c.muted_foreground))
            .child(el().padding(6.0).radius(8.0).fill(rune::Color::WHITE).child(super::qr::view(url, 128.0)))
            .child(
                kit::Button::new(format!("Open {store}"))
                    .variant("outline")
                    .size("sm")
                    .on_press(move |s: &mut App| super::auth::open_url(s, &link))
                    .build(t),
            );
        Menu::new("mobile-store-popover")
            .anchor(format!("mobile-store-{i}"))
            .placement(Placement::BottomEnd)
            .gutter(4.0)
            .width(200.0)
            .open(true)
            .on_close(|s: &mut App| s.ui.settings.about.store = None)
            .row(body)
            .overlay(&kit::theme(t), layers);
    }
    if s.ui.settings.about.track_open {
        let managed = updates_denial(primary_policy(s)).is_some();
        track_select(s, managed).build(&kit::theme(t), layers);
    }
    switch_dialog(s, t, layers);
}

/// Web `UpdateTrackSwitchDialog`. Rune has no update feed: this app is reinstalled by hand, and
/// the switch moves connected servers.
fn switch_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let ui = &s.ui.settings.about;
    let Some(channel) = ui.switch.clone() else { return };
    let label = track_label(&channel);
    let cancel = kit::Button::new("Cancel").variant("outline").on_press(close_switch).build(t);
    let release = ui.release.as_ref().and_then(|r| r.as_ref().ok()).filter(|v| v["version"].is_string());
    let Some(release) = release else {
        let description = match &ui.release {
            None => "Checking releases…".to_owned(),
            Some(Err(message)) => message.clone(),
            Some(Ok(_)) => format!("Could not find the latest {label} release. Try again later."),
        };
        kit::Dialog::new("update-track-switch", format!("Switch to {label}"))
            .alert()
            .open(true)
            .description(description)
            .child(row().w_full().justify_end().child(cancel))
            .on_close(close_switch)
            .build(t, layers);
        return;
    };
    let version = release["version"].as_str().unwrap_or_default().to_owned();
    let machines = awaiting(s, &channel);
    let crosses_local = update_channel(env!("CARGO_PKG_VERSION")) != channel;
    let mut notes = Vec::new();
    if machines.is_empty() && !crosses_local {
        notes.push(format!("Every connected machine is already on {label}. This only saves the track."));
    }
    if crosses_local {
        notes.push("This app's updates are off here, so reinstall it yourself to switch.".to_owned());
    }
    let mut body = col().w_full().gap(12.0);
    for note in notes {
        body = body.child(text(note).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap());
    }
    if !machines.is_empty() {
        let mut table = col().w_full().child(
            row()
                .w_full()
                .pb(6.0)
                .child(text("Machine").font(theme::TEXT_XS).color(t.c.muted_foreground).grow(1.0))
                .child(text("T3 Code").font(theme::TEXT_XS).color(t.c.muted_foreground)),
        );
        for (_, name) in &machines {
            table = table.child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.5))).child(
                row()
                    .w_full()
                    .py(6.0)
                    .child(text(name.clone()).font(theme::TEXT_SM).color(t.c.foreground).truncate().grow(1.0).min_w(0.0))
                    .child(text("Server").font(theme::TEXT_SM).color(t.c.foreground)),
            );
        }
        body = body.child(table);
    }
    let migration = release["databaseMigration"].as_i64();
    let confirm = kit::Button::new(format!("Switch to {label}"))
        .on_press(move |s: &mut App| {
            let _ = crate::desktop::prefs().set(TRACK_KEY, &channel);
            s.ui.settings.about.track = Some(channel.clone());
            let mut payload = json!({"targetVersion": version, "channel": channel});
            if let Some(migration) = migration {
                payload["targetDatabaseMigration"] = json!(migration);
            }
            for (key, _) in awaiting(s, &channel) {
                if let Some(net) = crate::env::net_of(s, &key) {
                    net.request("server.updateServer", payload.clone());
                }
            }
            close_switch(s);
        })
        .build(t);
    kit::Dialog::new("update-track-switch", format!("Switch to {label}?"))
        .alert()
        .open(true)
        .description(format!("T3 Code {}", release["version"].as_str().unwrap_or_default()))
        .child(body)
        .child(row().w_full().gap(8.0).justify_end().child(cancel).child(confirm))
        .on_close(close_switch)
        .build(t, layers);
}

/// Scenes: a managed environment, so Organization shows, and the track switch dialog open.
pub fn scene(s: &mut App, switching: bool) {
    if let crate::data::Source::Fixture(state) = &mut s.src
        && let Some(config) = state.config.as_mut()
    {
        config["managedPolicy"] = json!({"organization": "Acme", "supportUrl": "https://acme.example/help", "lockedSettings": []});
    }
    let ui = &mut s.ui.settings.about;
    ui.track = Some("latest".into());
    if switching {
        ui.switch = Some("nightly".into());
        ui.release = Some(Ok(json!({"version": "0.2.0-nightly.20261113.4"})));
    }
}

/// A `SettingsRow`: copy on the left, control on the right, `px-4 py-3` like every settings row.
pub fn about_row(t: &T, title: &str, description: &str, control: El) -> El {
    row()
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
                .child(text(title.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(text(description.to_owned()).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap()),
        )
        .child(control)
}
pub fn update_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(version) = &s.ui.settings.update_version else {
        return;
    };
    let version = version.clone();
    crate::kit::Dialog::new("update-server", "Update server?")
        .open(true)
        .alert()
        .description(format!(
            "Install {version}? The server will restart and connected clients will reconnect."
        ))
        .on_close(|s: &mut App| s.ui.settings.update_version = None)
        .child(
            row()
                .gap(8.0)
                .child(
                    c::button("Cancel", t)
                        .on_press(|s: &mut App| s.ui.settings.update_version = None),
                )
                .child(
                    crate::kit::Button::new("Update server")
                        .on_press(move |s: &mut App| {
                            crate::chat::server_update::start(s, &version);
                            s.ui.settings.update_version = None;
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
    fn release_tracks_and_policy_copy_read_like_web() {
        assert_eq!(release_channel("0.2.0"), "stable");
        assert_eq!(release_channel("0.2.0-nightly.20261113.4"), "nightly");
        assert_eq!(release_channel("0.2.0-preview.20261113.1"), "preview");
        assert_eq!(release_channel("0.2.0-nightly.2026111.4"), "stable", "the date has eight digits");
        assert_eq!(release_channel("0.2.0-beta.1"), "stable");
        assert_eq!(update_channel("0.2.0-preview.20261113.1"), "latest");
        assert_eq!(track_label("latest"), "Stable");
        let policy = json!({"organization": "Acme", "limits": {"updates": {"mode": "off", "allowNightly": false}}});
        assert_eq!(updates_denial(Some(&policy)).as_deref(), Some("Acme manages T3 Code updates."));
        assert_eq!(updates_denial(Some(&json!({"organization": "Acme"}))), None);
        assert_eq!(nightly_denied(&policy, "latest"), "Acme manages T3 Code updates. Nightly builds are not available.");
        assert_eq!(
            nightly_denied(&policy, "nightly"),
            "Acme manages T3 Code updates. Nightly builds are not available; switch to Stable to keep getting updates."
        );
    }
}
