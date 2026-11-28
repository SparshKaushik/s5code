use super::{controls as c, request};
use crate::kit::{self, Menu, Placement};
use crate::{
    App, Route,
    theme::{self, AlphaMul, T},
};
use rune::{El, col, el, row, text};
use serde_json::{Value, json};
use std::cmp::Ordering;

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
    /// The track switch dialog lists its skipped machines.
    skipped_open: bool,
    /// When the version's "Copied!" tooltip hides.
    copied_until: i64,
}

const TRACK_KEY: &str = "updateChannel";

/// Web `Icons.tsx` `AppleIcon` and `AndroidIcon`, the Mobile app row's store marks.
static APPLE: rune::Icon = rune::Icon { name: "apple", view: 24.0, body: "<path d=\"M12.152 6.896c-.948 0-2.415-1.078-3.96-1.04-2.04.027-3.91 1.183-4.961 3.014-2.117 3.675-.546 9.103 1.519 12.09 1.013 1.454 2.208 3.09 3.792 3.039 1.52-.065 2.09-.987 3.935-.987 1.831 0 2.35.987 3.96.948 1.637-.026 2.676-1.48 3.676-2.948 1.156-1.688 1.636-3.325 1.662-3.415-.039-.013-3.182-1.221-3.22-4.857-.026-3.04 2.48-4.494 2.597-4.559-1.429-2.09-3.623-2.324-4.39-2.376-2-.156-3.675 1.09-4.61 1.09zM15.53 3.83c.843-1.012 1.4-2.427 1.245-3.83-1.207.052-2.662.805-3.532 1.818-.78.896-1.454 2.338-1.273 3.714 1.338.104 2.715-.688 3.559-1.701\" fill=\"currentColor\"/>" };
static ANDROID: rune::Icon = rune::Icon { name: "android", view: 24.0, body: "<path d=\"m8 3-1-2m9 2 1-2\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.4\" stroke-linecap=\"round\"/><path fill-rule=\"evenodd\" d=\"M5 10a7 7 0 0 1 14 0H5Zm4-4a.8.8 0 1 0 0 1.6A.8.8 0 0 0 9 6Zm6 0a.8.8 0 1 0 0 1.6A.8.8 0 0 0 15 6Z\" fill=\"currentColor\"/><path d=\"M5 11h14v7a2 2 0 0 1-2 2v2a1.5 1.5 0 0 1-3 0v-2h-4v2a1.5 1.5 0 0 1-3 0v-2a2 2 0 0 1-2-2v-7Z\" fill=\"currentColor\"/><rect x=\"1\" y=\"11\" width=\"3\" height=\"8\" rx=\"1.5\" fill=\"currentColor\"/><rect x=\"20\" y=\"11\" width=\"3\" height=\"8\" rx=\"1.5\" fill=\"currentColor\"/>" };

/// Web `MobileAppSettingsRow`'s stores: button label, store name, link.
const STORES: &[(&str, &str, &str)] = &[
    ("iPhone and iPad", "App Store", "https://apps.apple.com/us/app/t3-code-remote-claude-more/id6787819824"),
    ("Android", "Google Play", "https://play.google.com/store/apps/details?id=com.t3tools.t3code"),
];

/// Shared `cliReleaseChannelOf`: `X-nightly.YYYYMMDD.N` and `X-preview.YYYYMMDD.N`, else stable.
pub(crate) fn release_channel(version: &str) -> &'static str {
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
pub(crate) fn updates_denial(policy: Option<&Value>) -> Option<String> {
    let policy = policy?;
    (policy["limits"]["updates"]["mode"] == "off").then(|| format!("{} manages T3 Code updates.", organization(policy)))
}

/// This machine's policy refusing app updates, for the desktop updater.
pub(crate) fn managed_denial(s: &App) -> Option<String> {
    updates_denial(primary_policy(s))
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
            let movable = !on_track(version, channel) && track_move(config, false).is_ok();
            (movable && super::permissions::can_on(s, &key, "environment:maintain")).then(|| (key.clone(), crate::env::label(s, &key)))
        })
        .collect()
}

fn on_track(version: &str, channel: &str) -> bool {
    release_channel(version) != "preview" && update_channel(version) == channel
}

/// Shared `parseSemver`: the three core numbers and the prerelease identifiers.
fn semver(version: &str) -> Option<([u64; 3], Vec<&str>)> {
    let version = version.trim().split('+').next().unwrap_or_default();
    let (main, pre) = version.split_once('-').unwrap_or((version, ""));
    let mut parts: Vec<&str> = main.trim_start_matches('v').split('.').map(str::trim).filter(|p| !p.is_empty()).collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    parts.resize(3, "0");
    let mut core = [0; 3];
    for (slot, part) in core.iter_mut().zip(&parts) {
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *slot = part.parse().ok()?;
    }
    Some((core, pre.split('.').map(str::trim).filter(|p| !p.is_empty()).collect()))
}

/// Shared `compareSemverVersions`; versions that don't parse compare as text.
fn compare_versions(a: &str, b: &str) -> Ordering {
    let (Some((a_core, a_pre)), Some((b_core, b_pre))) = (semver(a), semver(b)) else { return a.cmp(b) };
    let identifier = |x: &str, y: &str| {
        let numeric = |v: &str| v.bytes().all(|b| b.is_ascii_digit());
        match (numeric(x), numeric(y)) {
            (true, true) => x.parse::<u64>().ok().cmp(&y.parse::<u64>().ok()),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => x.cmp(y),
        }
    };
    a_core.cmp(&b_core).then_with(|| match (a_pre.is_empty(), b_pre.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => a_pre.iter().zip(&b_pre).map(|(x, y)| identifier(*x, *y)).find(|o| o.is_ne()).unwrap_or_else(|| a_pre.len().cmp(&b_pre.len())),
    })
}

/// Web `resolveVersionMismatch`: whether `server` is behind `client`. Nightlies compare whole
/// versions; everything else compares the core.
fn behind(server: &str, client: &str) -> bool {
    let (server, client) = (server.trim(), client.trim());
    if server.is_empty() || client.is_empty() {
        return false;
    }
    let core_of = |v: &str| v.split(['-', '+']).next().unwrap_or(v).to_owned();
    let (server_core, client_core) = (core_of(server), core_of(client));
    if semver(&server_core).is_none() || semver(&client_core).is_none() {
        return server != client;
    }
    let nightly = |v: &str| semver(v).is_some_and(|(_, pre)| pre.first() == Some(&"nightly"));
    let order = if nightly(server) && nightly(client) { compare_versions(server, client) } else { compare_versions(&server_core, &client_core) };
    order == Ordering::Less
}

/// `trackSwitchMove`: how a switch moves a host, or why it never can from here.
fn track_move(config: &Value, same_track: bool) -> Result<&str, &'static str> {
    let environment = &config["environment"];
    if release_channel(environment["serverVersion"].as_str().unwrap_or_default()) == "preview" {
        return Err("runs a preview build");
    }
    let Some(method) = environment["capabilities"]["serverSelfUpdate"].as_str().filter(|m| *m != "unsupported") else {
        return Err("needs a manual update");
    };
    if method != "desktop-managed" {
        return Ok(method);
    }
    if environment["capabilities"]["desktopAppChannelSwitch"] != true {
        return Err("needs a newer desktop app");
    }
    // Squirrel.Mac renames the bundle for the new track and then cannot relaunch it.
    if !same_track && environment["platform"]["os"] == "darwin" {
        return Err("switch the Mac app on that Mac");
    }
    Ok(method)
}

/// `managedMachineUpdateDenial`: the server refuses these with the same message.
fn server_managed_denial(config: &Value, target: &str) -> Option<String> {
    let policy = &config["managedPolicy"];
    updates_denial(Some(policy)).or_else(|| {
        (policy["limits"]["updates"]["allowNightly"] == false && release_channel(target) != "stable")
            .then(|| format!("{} manages T3 Code updates. Only stable releases can be installed.", organization(policy)))
    })
}

/// `channelSwitchBlockReason`: crossing tracks needs the target release to know every
/// migration the database holds.
fn block_reason(from: &str, channel: &str, version: &str, release_migration: Option<i64>, migration: Option<i64>) -> Option<String> {
    if update_channel(from) == channel {
        return None;
    }
    let (from_label, to_label) = (track_label(update_channel(from)), track_label(channel));
    let Some(migration) = migration else {
        return Some(format!("its server predates database checks; update it on {from_label} first"));
    };
    let Some(release_migration) = release_migration else {
        return Some(format!("{version} predates database checks; wait for a newer {to_label} release"));
    };
    (migration > release_migration)
        .then(|| format!("its database has changes {version} does not support yet; stay on {from_label} until {to_label} has them"))
}

/// One machine's part in a track switch (web `collectChannelSwitchTargets`).
#[derive(Debug, PartialEq)]
enum Switch {
    /// `server.updateServer` with `input`; a track-only host saves the track and restarts nothing.
    Move { input: Value, desktop: bool, track_only: bool },
    Skip(String),
    Stay,
}

/// `unavailable` is web's `unavailableReason`; `release` is the track's npm release.
fn plan_switch(config: Option<&Value>, unavailable: Option<&str>, channel: &str, release: &Value) -> Switch {
    let version = release["version"].as_str().unwrap_or_default();
    let release_migration = release["databaseMigration"].as_i64();
    let input = |method: &str, target: &str| {
        let mut input = json!({"targetVersion": target});
        if method == "desktop-managed" {
            input["channel"] = json!(channel);
        }
        if let Some(migration) = release_migration {
            input["targetDatabaseMigration"] = json!(migration);
        }
        input
    };
    let server = config.and_then(|c| c["environment"]["serverVersion"].as_str());
    let same_track = server.is_some_and(|v| on_track(v, channel));
    if let (Some(config), Some(server)) = (config, server)
        && same_track
        && !behind(server, version)
    {
        // A desktop host already current still saves the track, which may differ.
        return match track_move(config, true) {
            Ok("desktop-managed") if unavailable.is_none() && server_managed_denial(config, version).is_none() => {
                Switch::Move { input: input("desktop-managed", server), desktop: true, track_only: true }
            }
            _ => Switch::Stay,
        };
    }
    let (Some(config), Some(server), None) = (config, server, unavailable) else {
        return Switch::Skip(unavailable.unwrap_or("not connected").to_owned());
    };
    if let Some(denial) = server_managed_denial(config, version) {
        return Switch::Skip(denial);
    }
    let method = match track_move(config, same_track) {
        Ok(method) => method,
        Err(reason) => return Switch::Skip(reason.to_owned()),
    };
    if let Some(blocked) = block_reason(server, channel, version, release_migration, config["environment"]["databaseMigration"].as_i64()) {
        return Switch::Skip(blocked);
    }
    // Service launchers only accept newer versions, so leaving for an older release is manual.
    if method == "boot-service" && compare_versions(version, server) != Ordering::Greater {
        let track = if channel == "nightly" { "nightly" } else { "stable" };
        return Switch::Skip(format!("run `t3 update --channel {track} --allow-downgrade` there"));
    }
    Switch::Move { input: input(method, version), desktop: method == "desktop-managed", track_only: false }
}

/// Every machine but the one this app hosts, with what switching to `channel` does to it.
fn switch_plan(s: &App, channel: &str, release: &Value) -> Vec<(String, String, Switch)> {
    crate::env::keys(s)
        .into_iter()
        .filter(|key| key != crate::env::LOCAL)
        .map(|key| {
            let config = crate::env::source(s, &key).and_then(|src| src.state().config.as_ref());
            let unavailable = if config.is_none() || !crate::env::connected(s, &key) {
                Some("not connected")
            } else if !super::permissions::can_on(s, &key, "environment:maintain") {
                Some("no permission")
            } else {
                None
            };
            let plan = plan_switch(config, unavailable, channel, release);
            let label = crate::env::label(s, &key);
            (key, label, plan)
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
    ui.skipped_open = false;
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
    // Web's About version row: this app's own update (`desktop::updates`).
    let (status, next) = crate::desktop::updates::about(s);
    let button = match next {
        Some(label @ ("Check for Updates" | "Up to Date")) => c::button(label, t).on_press(crate::desktop::check_for_updates),
        Some(label) => c::button(label, t).on_press(crate::desktop::updates::press),
        None => c::press_if(c::button(crate::desktop::updates::about_busy_label(s), t), false, |_| {}),
    };
    app.push(
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
                    .child(version_title(t))
                    .child(text(status).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap()),
            )
            .child(button),
    );
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
        Status::Failed { target, message, .. } if !update::dismissed(s, &s.ui.env.active) => {
            let retry = target.clone();
            controls = controls
                .child(c::button("Retry", t).disabled(!can_update).on_press(move |s: &mut App| update::start(s, &retry)))
                .child(c::button("Dismiss", t).on_press(|s: &mut App| update::dismiss(s, &s.ui.env.active.clone())));
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

const COPIED_TIP: &str = "about-version-copied";
/// `ANCHORED_COPY_TOAST_TIMEOUT_MS`.
const COPIED_MS: i64 = 1000;

/// Web `AboutVersionTitle`: "Version", then the version itself, which copies on click.
fn version_title(t: &T) -> El {
    let version = crate::desktop::updates::version();
    let copy = text(version)
        .font(theme::mono(theme::weight(theme::TEXT_2XS, theme::MEDIUM)))
        .color(t.c.muted_foreground)
        .cursor(rune::Cursor::Pointer)
        .focusable(true)
        .role("button")
        .label(format!("Copy version {version}"))
        .help("Copy version")
        .on_press_tx(|s: &mut App, tx: &mut rune::Tx| {
            tx.copy(version.to_owned());
            s.ui.settings.about.copied_until = crate::data::now_ms() + COPIED_MS;
            tx.wake_after(std::time::Duration::from_millis(COPIED_MS as u64));
        });
    row()
        .gap(8.0)
        .center_y()
        .child(text("Version").key(super::title_key("Version")).font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(row().key(format!("{COPIED_TIP}-trigger")).child(copy))
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
                .child(text("Update track").key(super::title_key("Update track")).font(theme::SM_MEDIUM).color(t.c.foreground))
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
                    .icon(if i == 0 { &APPLE } else { &ANDROID })
                    .on_press(move |s: &mut App| s.ui.settings.about.store = (open != Some(i)).then_some(i))
                    .build(t)
                    .expanded(open == Some(i)),
            ),
        );
    }
    about_row(t, "Mobile app", "Connect to your server from your phone or tablet.", buttons).key(super::title_key("Mobile app"))
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
    // `showAnchoredCopySuccessToast`: a tooltip-styled "Copied!" on the version for a second.
    if crate::data::now_ms() < s.ui.settings.about.copied_until {
        let _ = kit::Tooltip::new(COPIED_TIP).label("Copied!").open(true).build(&kit::theme(t), layers);
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
    let plan = switch_plan(s, &channel, release);
    let moving: Vec<(&str, bool)> = plan
        .iter()
        .filter_map(|(_, name, step)| match step {
            Switch::Move { desktop, track_only: false, .. } => Some((name.as_str(), *desktop)),
            _ => None,
        })
        .collect();
    let skipped: Vec<(&str, &str)> = plan
        .iter()
        .filter_map(|(_, name, step)| match step {
            Switch::Skip(reason) => Some((name.as_str(), reason.as_str())),
            _ => None,
        })
        .collect();
    let crosses_local = update_channel(crate::desktop::updates::version()) != channel;
    let mut notes = Vec::new();
    if moving.is_empty() && skipped.is_empty() && !crosses_local {
        notes.push(format!("Every connected machine is already on {label}. This only saves the track."));
    }
    if crosses_local {
        notes.push(match crate::desktop::updates::disabled(s) {
            Some(reason) => format!("{reason} Reinstall this app yourself to switch."),
            None => format!("This app checks the {label} track for its update after the switch."),
        });
    }
    let mut body = col().w_full().gap(12.0);
    for note in notes {
        body = body.child(text(note).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap());
    }
    if !moving.is_empty() {
        let mut table = col().w_full().child(
            row()
                .w_full()
                .pb(6.0)
                .child(text("Machine").font(theme::TEXT_XS).color(t.c.muted_foreground).grow(1.0))
                .child(text("T3 Code").font(theme::TEXT_XS).color(t.c.muted_foreground)),
        );
        for (name, desktop) in &moving {
            table = table.child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.5))).child(
                row()
                    .w_full()
                    .py(6.0)
                    .child(text(name.to_string()).font(theme::TEXT_SM).color(t.c.foreground).truncate().grow(1.0).min_w(0.0))
                    .child(text(if *desktop { "Desktop app" } else { "Server" }).font(theme::TEXT_SM).color(t.c.foreground)),
            );
        }
        body = body.child(table);
    }
    if !skipped.is_empty() {
        let open = ui.skipped_open;
        body = body.child(
            row()
                .gap(4.0)
                .center_y()
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(format!("{} skipped", skipped.len()))
                .expanded(open)
                .child(rune::icon(if open { &crate::icons::CHEVRON_DOWN } else { &crate::icons::CHEVRON_RIGHT }, 14.0).color(t.c.muted_foreground))
                .child(text(format!("{} skipped", skipped.len())).font(theme::TEXT_XS).color(t.c.muted_foreground))
                .on_press(|s: &mut App| s.ui.settings.about.skipped_open ^= true),
        );
        if open {
            let mut list = col().w_full().gap(2.0).pl(18.0);
            for (name, reason) in &skipped {
                list = list.child(
                    row()
                        .w_full()
                        .gap(12.0)
                        .child(text(name.to_string()).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().max_w(128.0))
                        .child(text(reason.to_string()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().grow(1.0).min_w(0.0)),
                );
            }
            body = body.child(list);
        }
    }
    let description = format!("T3 Code {}", release["version"].as_str().unwrap_or_default());
    let target = release.clone();
    let confirm = kit::Button::new(format!("Switch to {label}"))
        .on_press(move |s: &mut App| {
            let _ = crate::desktop::prefs().set(TRACK_KEY, &channel);
            s.ui.settings.about.track = Some(channel.clone());
            for (key, _, step) in switch_plan(s, &channel, &target) {
                if let Switch::Move { input, .. } = step
                    && let Some(net) = crate::env::net_of(s, &key)
                {
                    net.request("server.updateServer", input);
                }
            }
            close_switch(s);
            crate::desktop::updates::set_channel(s, target["version"].as_str().map(str::to_owned));
        })
        .build(t);
    kit::Dialog::new("update-track-switch", format!("Switch to {label}?"))
        .alert()
        .open(true)
        .description(description)
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
                .child(text(title.to_owned()).key(super::title_key(title)).font(theme::SM_MEDIUM).color(t.c.foreground))
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

    #[test]
    fn versions_compare_like_shared_semver() {
        assert_eq!(compare_versions("0.3.0", "0.3.0-nightly.20261120.1"), Ordering::Greater);
        assert_eq!(compare_versions("0.3.0-nightly.20261120.1", "0.3.0-nightly.20261120.2"), Ordering::Less);
        assert_eq!(compare_versions("v1.2", "1.2.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.10.0", "0.9.9"), Ordering::Greater);
        assert!(behind("0.2.0", "0.3.0"));
        assert!(!behind("0.3.0-preview.20261101.1", "0.3.0"), "non-nightlies compare the core only");
        assert!(behind("0.3.0-nightly.20261101.1", "0.3.0-nightly.20261120.1"));
        assert!(!behind("0.3.0", ""));
    }

    #[test]
    fn track_switch_moves_and_skips_machines_like_web() {
        let config = |version: &str, method: &str, migration: Option<i64>| {
            let mut config = json!({"environment": {"serverVersion": version, "capabilities": {"serverSelfUpdate": method, "desktopAppChannelSwitch": true}, "platform": {"os": "linux"}}});
            if let Some(migration) = migration {
                config["environment"]["databaseMigration"] = json!(migration);
            }
            config
        };
        let stable = json!({"version": "0.3.0", "databaseMigration": 40});
        let plan = |config: &Value, unavailable: Option<&str>| plan_switch(Some(config), unavailable, "latest", &stable);
        let skip = |reason: &str| Switch::Skip(reason.to_owned());

        assert_eq!(
            plan(&config("0.2.1-nightly.20261113.4", "respawn", Some(30)), None),
            Switch::Move { input: json!({"targetVersion": "0.3.0", "targetDatabaseMigration": 40}), desktop: false, track_only: false }
        );
        assert_eq!(plan(&config("0.2.0", "respawn", Some(30)), None), Switch::Move { input: json!({"targetVersion": "0.3.0", "targetDatabaseMigration": 40}), desktop: false, track_only: false }, "behind on its own track");
        assert_eq!(plan(&config("0.3.0", "respawn", Some(40)), None), Switch::Stay);
        assert_eq!(
            plan(&config("0.3.0", "desktop-managed", Some(40)), None),
            Switch::Move { input: json!({"targetVersion": "0.3.0", "channel": "latest", "targetDatabaseMigration": 40}), desktop: true, track_only: true },
            "a current desktop host still saves the track"
        );
        assert_eq!(plan(&config("0.3.0-preview.20261101.1", "respawn", Some(30)), None), skip("runs a preview build"));
        assert_eq!(plan(&config("0.2.1-nightly.20261113.4", "respawn", Some(30)), Some("no permission")), skip("no permission"));
        assert_eq!(plan_switch(None, Some("not connected"), "latest", &stable), skip("not connected"));
        assert_eq!(plan(&config("0.2.1-nightly.20261113.4", "unsupported", Some(30)), None), skip("needs a manual update"));
        assert_eq!(
            plan(&config("0.3.1-nightly.20261120.1", "respawn", Some(42)), None),
            skip("its database has changes 0.3.0 does not support yet; stay on Nightly until Stable has them")
        );
        assert_eq!(plan(&config("0.2.1-nightly.20261113.4", "respawn", None), None), skip("its server predates database checks; update it on Nightly first"));
        assert_eq!(
            plan_switch(Some(&config("0.2.1-nightly.20261113.4", "respawn", Some(30))), None, "latest", &json!({"version": "0.3.0"})),
            skip("0.3.0 predates database checks; wait for a newer Stable release")
        );
        assert_eq!(
            plan(&config("0.4.0-nightly.20261120.1", "boot-service", Some(30)), None),
            skip("run `t3 update --channel stable --allow-downgrade` there")
        );
        let mut mac = config("0.2.1-nightly.20261113.4", "desktop-managed", Some(30));
        mac["environment"]["platform"]["os"] = json!("darwin");
        assert_eq!(plan(&mac, None), skip("switch the Mac app on that Mac"));
        let mut old_app = config("0.2.1-nightly.20261113.4", "desktop-managed", Some(30));
        old_app["environment"]["capabilities"]["desktopAppChannelSwitch"] = json!(false);
        assert_eq!(plan(&old_app, None), skip("needs a newer desktop app"));

        let mut managed = config("0.3.0", "respawn", Some(40));
        managed["managedPolicy"] = json!({"organization": "Acme", "limits": {"updates": {"allowNightly": false}}});
        let nightly = json!({"version": "0.3.1-nightly.20261120.1", "databaseMigration": 41});
        assert_eq!(
            plan_switch(Some(&managed), None, "nightly", &nightly),
            skip("Acme manages T3 Code updates. Only stable releases can be installed.")
        );
        managed["managedPolicy"]["limits"]["updates"]["mode"] = json!("off");
        assert_eq!(plan(&managed, None), Switch::Stay, "a current server stays even when updates are managed");
    }
}
