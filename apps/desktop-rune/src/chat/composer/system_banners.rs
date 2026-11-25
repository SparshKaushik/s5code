//! Auto-balanced machines' update notice and the shared server-update actions.

use rune::{Cursor, El, ScrollSpec, Tx, col, el, row, text};
use serde_json::{Value, json};

use super::banners::{self, Item, Variant};
use crate::chat::{details::run_on, server_update::{self as update, Status}};
use crate::data::s;
use crate::kit::{self, Button, Dialog, Placement, Popover};
use crate::settings::{about, machines, permissions};
use crate::theme::{self, T};
use crate::{App, env, icons};

const POPOVER: &str = "auto-balance-server-updates";
const DISMISSALS: &str = "t3code:version-mismatch-dismissals:v1";
const CROSS_TRACK: &str = "Switch tracks in About → Update track";

#[derive(Default)]
pub(crate) struct State {
    open: bool,
    confirmation: Option<Confirmation>,
}

struct Confirmation {
    message: String,
    detail: &'static str,
    targets: Vec<(String, String)>,
}

fn config<'a>(app: &'a App, key: &str) -> &'a Value {
    env::source(app, key).and_then(|src| src.state().config.as_ref()).unwrap_or(&Value::Null)
}

fn capabilities(config: &Value) -> &Value {
    &config["environment"]["capabilities"]
}

fn desktop_managed(config: &Value) -> bool {
    capabilities(config)["serverSelfUpdate"] == "desktop-managed"
}

fn remote_update(config: &Value) -> bool {
    capabilities(config)["serverSelfUpdate"].is_string()
        && (!desktop_managed(config) || capabilities(config)["desktopAppUpdate"] == true)
}

fn crosses_track(config: &Value, target: &str) -> bool {
    config["environment"]["serverVersion"].as_str().is_some_and(|server| machines::crosses_track(server, target))
}

fn eligible(config: &Value, target: &str) -> bool {
    remote_update(config)
        && about::updates_denial(config.get("managedPolicy")).is_none()
        && !crosses_track(config, target)
}

struct Machine<'a> {
    key: String,
    label: String,
    config: &'a Value,
    state: &'a Status,
    target: String,
    connected: bool,
    dismissal: Option<String>,
}

fn dismissal_key(app: &App, key: &str, client: &str, server: &str) -> String {
    let id = if key == env::LOCAL { env::local_id(app) } else { Some(key.to_owned()) };
    format!("{}:{client}:{server}", id.as_deref().unwrap_or(key))
}

fn dismissals(app: &App) -> &[Value] {
    app.client_settings.values.get(DISMISSALS).and_then(|v| v["keys"].as_array()).map_or(&[], Vec::as_slice)
}

fn machine(app: &App, key: String) -> Option<Machine<'_>> {
    let config = config(app, &key);
    let state = update::status_on(app, &key);
    let gap = machines::config_version_gap(config);
    let dismissal = gap.as_ref().map(|(server, client)| dismissal_key(app, &key, client, server));
    let target = match state {
        Status::Idle => {
            if dismissal.as_ref().is_some_and(|k| dismissals(app).iter().any(|v| v.as_str() == Some(k))) {
                return None;
            }
            gap?.1.to_owned()
        }
        Status::Running { target, .. } => target.clone(),
        Status::Failed { target, .. } => {
            if update::dismissed(app, &key) {
                return None;
            }
            target.clone()
        }
    };
    Some(Machine {
        connected: env::connected(app, &key),
        label: env::label(app, &key),
        key,
        config,
        state,
        target,
        dismissal,
    })
}

fn machines(app: &App) -> Vec<Machine<'_>> {
    if !run_on::automatic(app) {
        return Vec::new();
    }
    let holders = run_on::holders(app);
    env::keys(app).into_iter()
        .filter(|key| holders.iter().any(|(holder, _)| holder == key))
        .filter_map(|key| machine(app, key))
        .collect()
}

#[derive(PartialEq, Eq, Debug)]
struct Summary {
    running: usize,
    failed: usize,
    manual: usize,
    total: usize,
    targets: usize,
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "machine" } else { "machines" }
}

impl Summary {
    fn new(machines: &[Machine<'_>]) -> Self {
        Self {
            running: machines.iter().filter(|m| matches!(m.state, Status::Running { .. })).count(),
            failed: machines.iter().filter(|m| matches!(m.state, Status::Failed { .. })).count(),
            manual: machines.iter().filter(|m| !remote_update(m.config)).count(),
            total: machines.len(),
            targets: machines.iter().filter(|m| can_target(m)).count(),
        }
    }

    fn title(&self) -> String {
        let (phrase, count) = if self.running > 0 {
            ("Updating", self.running)
        } else if self.failed > 0 {
            ("Could not update", self.failed)
        } else {
            ("Update available for", self.total)
        };
        format!("{phrase} {count} {}", plural(count))
    }

    fn action(&self) -> Option<String> {
        if self.running > 0 || self.targets == 0 {
            return None;
        }
        Some(if self.failed > 0 {
            "Retry".to_owned()
        } else if self.targets == self.total {
            "Update all".to_owned()
        } else {
            format!("Update {} {}", self.targets, plural(self.targets))
        })
    }
}

fn can_target(machine: &Machine<'_>) -> bool {
    machine.connected && remote_update(machine.config) && !matches!(machine.state, Status::Running { .. })
}

fn start(app: &mut App, targets: &[(String, String)]) {
    for (key, target) in targets {
        if env::connected(app, key) && eligible(config(app, key), target)
            && permissions::can_on(app, key, "environment:maintain")
        {
            let label = env::label(app, key);
            update::start_on(app, key, target, Some(label));
        }
    }
}

fn request(app: &mut App, targets: &[(String, String)], single_label: Option<&str>) {
    let desktop: Vec<String> = targets.iter().filter(|(key, target)| {
        let config = config(app, key);
        desktop_managed(config) && eligible(config, target)
    }).map(|(key, _)| env::label(app, key)).collect();
    if desktop.is_empty() {
        start(app, targets);
        return;
    }
    let (message, detail) = if let Some(label) = single_label {
        (format!("Update the T3 Code desktop app that runs the {label}?"), "It will close and relaunch on that machine.")
    } else {
        (format!("Update the T3 Code desktop apps on {}?", desktop.join(", ")), "They will close and relaunch on those machines.")
    };
    app.ui.chat.banners.system.confirmation = Some(Confirmation { message, detail, targets: targets.to_vec() });
}

fn manual_command(target: &str, installation: &Value) -> (String, bool) {
    let kind = s(installation, "kind");
    if kind == "npm-global" {
        let prefix = s(installation, "prefix").replace(''', "'\\''");
        return (format!("npm install --global --prefix '{prefix}' t3@{target}"), true);
    }
    let runner = match kind {
        "pnpm-dlx" => "pnpm dlx",
        "bunx" => "bunx",
        _ => "npx",
    };
    (format!("{runner} t3@{target}"), false)
}

fn note(t: &T, label: impl Into<String>) -> El {
    text(label.into()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()
}

fn update_action(app: &App, key: &str, label: &str, target: &str, action: &str, variant: &str, t: &T) -> El {
    let config = config(app, key);
    if let Some(denial) = about::updates_denial(config.get("managedPolicy")) {
        return note(t, denial);
    }
    let caps = capabilities(config);
    if desktop_managed(config) && !remote_update(config) {
        return note(t, "Update the desktop app on that machine to update this server.");
    }
    if !caps["serverSelfUpdate"].is_string() {
        let (command, global) = manual_command(target, &caps["serverInstallation"]);
        let (button, title, description) = if global {
            ("Copy update command", "Update command copied", format!("Run `{command}` on {label}, then restart t3 with your usual options."))
        } else {
            ("Copy relaunch command", "Relaunch command copied", format!("Stop t3 on {label}, then relaunch with `{command}` using the same subcommand and options. This does not update an installed t3 command."))
        };
        return Button::new(button).variant(variant).size("xs").build(t).on_press_tx(move |app: &mut App, tx: &mut Tx| {
            tx.copy(command.clone());
            crate::shell::palette::toast(app, title, Some(&description));
        });
    }
    if crosses_track(config, target) {
        return note(t, CROSS_TRACK);
    }
    let allowed = permissions::can_on(app, key, "environment:maintain");
    let targets = [(key.to_owned(), target.to_owned())];
    let label = label.to_owned();
    Button::new(action).variant(variant).size("xs").disabled(!allowed)
        .on_press(move |app: &mut App| request(app, &targets, Some(&label))).build(t)
}

pub(crate) fn retry(app: &App, key: &str, label: &str, t: &T) -> Option<El> {
    let config = config(app, key);
    let (_, client) = machines::config_version_gap(config)?;
    if desktop_managed(config) && !remote_update(config) {
        return None;
    }
    Some(update_action(app, key, label, client, "Retry", "ghost", t))
}

fn close(app: &mut App) {
    app.ui.chat.banners.system.open = false;
}

pub(super) fn auto_item(app: &App, t: &T) -> Option<Item> {
    let machines = machines(app);
    if machines.is_empty() {
        return None;
    }
    let summary = Summary::new(&machines);
    let title = summary.title();
    let variant = if summary.failed > 0 { Variant::Error } else { Variant::Default };
    let glyph = if summary.failed > 0 && summary.running == 0 { &icons::CIRCLE_ALERT } else { &icons::DOWNLOAD };
    let mut item = Item::new(POPOVER, variant, glyph, title.clone());
    item.environment = true;
    let open = app.ui.chat.banners.system.open;
    item.title_control = Some(
        row().key(format!("{POPOVER}-trigger")).min_w(0.0).shrink().focusable(true)
            .focus_ring(2.0, -1.0, t.c.ring).cursor(Cursor::Pointer)
            .role("button").label(format!("{title}. View machines")).expanded(open)
            .on_press(|app: &mut App| app.ui.chat.banners.system.open ^= true)
            .child(text(title).font(theme::XS_MEDIUM).color(t.c.foreground).single_line().truncate().a11y_hidden()),
    );
    if summary.manual > 0 {
        let verb = if summary.manual == 1 { "needs" } else { "need" };
        item.description = Some(format!("{} {verb} a manual update", summary.manual));
    }
    if let Some(label) = summary.action() {
        let targets: Vec<_> = machines.iter().filter(|m| can_target(m) && eligible(m.config, &m.target))
            .map(|m| (m.key.clone(), m.target.clone())).collect();
        item.actions.push(Button::new(label).variant("ghost").size("xs").disabled(targets.is_empty())
            .on_press(move |app: &mut App| request(app, &targets, None)).build(t));
    }
    if summary.running == 0 {
        let dismissed: Vec<_> = machines.into_iter().map(|m| (m.key, m.dismissal)).collect();
        item.dismiss = Some(banners::dismiss(t, "Dismiss update notice", move |app: &mut App| {
            let mut keys = dismissals(app).to_vec();
            for (key, mismatch) in &dismissed {
                update::dismiss(app, key);
                if let Some(mismatch) = mismatch && !keys.iter().any(|k| k.as_str() == Some(mismatch)) {
                    keys.push(json!(mismatch));
                }
            }
            app.client_settings.values.insert(DISMISSALS.to_owned(), json!({ "keys": keys }));
            app.client_settings.save();
            close(app);
        }));
    }
    Some(item)
}

fn machine_row(app: &App, machine: &Machine<'_>, t: &T) -> El {
    let heading = text(machine.label.clone()).font(theme::XS_MEDIUM).color(t.c.foreground);
    let detail = match machine.state {
        Status::Running { stage, .. } => progress(t, stage.label(), false),
        Status::Failed { message, .. } => progress(t, message, true),
        Status::Idle if !remote_update(machine.config) => col().gap(4.0)
            .child(note(t, "Manual update required"))
            .child(update_action(app, &machine.key, &machine.label, &machine.target, "Update", "outline", t).self_start()),
        Status::Idle if machine.connected => note(t, format!("Ready to update to {}", machine.target)),
        Status::Idle => note(t, "Reconnect this machine to update"),
    };
    col().w_full().gap(4.0).child(heading).child(detail)
}

fn progress(t: &T, message: &str, failed: bool) -> El {
    let ink = if failed { t.c.destructive } else { t.c.foreground };
    let font = if failed { theme::TEXT_XS } else { theme::XS_MEDIUM };
    let line = row().w_full().gap(8.0).center_y()
        .child(el().size(6.0, 6.0).radius(3.0).fill(ink).a11y_hidden())
        .child(text(message.to_owned()).font(font).color(ink).truncate().min_w(0.0).help_when_truncated(message.to_owned()));
    if failed { line.role("alert") } else { line }
}

pub(super) fn overlays(app: &App, t: &T, layers: &mut Vec<El>) {
    let state = &app.ui.chat.banners.system;
    if state.open {
        let machines = machines(app);
        if !machines.is_empty() && (!super::asking(app) || Summary::new(&machines).failed > 0) {
            let body = col().w_full().gap(12.0).children(machines.iter().map(|m| machine_row(app, m, t)));
            Popover::new(POPOVER)
                .open(true)
                .placement(Placement::TopStart)
                .width("md")
                .label("View machines")
                .on_close(close)
                .child(col().w_full().max_h(360.0).scroll(ScrollSpec { stick_bottom: false }).child(body))
                .build(&kit::theme(t), layers);
        }
    }
    if let Some(confirmation) = &state.confirmation {
        let cancel = |app: &mut App| app.ui.chat.banners.system.confirmation = None;
        Dialog::new("composer-server-update-confirm", &confirmation.message)
            .open(true)
            .alert()
            .description(confirmation.detail)
            .on_close(cancel)
            .child(row().gap(8.0)
                .child(Button::new("Cancel").variant("outline").on_press(cancel).build(t))
                .child(Button::new("Confirm").on_press(|app: &mut App| {
                    if let Some(confirm) = app.ui.chat.banners.system.confirmation.take() {
                        start(app, &confirm.targets);
                    }
                }).build(t)))
            .build(t, layers);
    }
}

pub(super) fn scene_available(app: &mut App) {
    run_on::two_machines(app);
    app.client_settings.values.insert("loadBalancingEnabled".into(), json!(true));
    app.client_settings.values.remove(DISMISSALS);
    if let Some(crate::data::Source::Fixture(local)) = env::source_mut(app, env::LOCAL)
        && let Some(config) = local.config.as_mut()
    {
        config["environment"]["serverVersion"] = json!("0.2.0");
    }
    if let Some(crate::data::Source::Fixture(remote)) = env::source_mut(app, "box")
        && let Some(config) = remote.config.as_mut()
    {
        config["environment"]["serverVersion"] = json!("0.1.6");
        config["environment"]["capabilities"]["serverSelfUpdate"] = json!("respawn");
    }
}

pub(super) fn scene_running(app: &mut App) {
    scene_available(app);
    update::show_on(app, "box", Status::Running { stage: update::Stage::Resuming, from: "0.1.6".into(), target: "0.1.7".into() });
}

pub(super) fn scene_failed(app: &mut App) {
    scene_available(app);
    update::show_on(app, "box", Status::Failed {
        stage: update::Stage::Downloading,
        from: "0.1.6".into(), target: "0.1.7".into(),
        message: "Server update failed: The new server did not start.".into(),
    });
}

pub(super) fn scene_machines(app: &mut App) {
    scene_available(app);
    if let Some(crate::data::Source::Fixture(remote)) = env::source_mut(app, "box")
        && let Some(config) = remote.config.as_mut()
    {
        config["environment"]["capabilities"]["serverSelfUpdate"] = Value::Null;
    }
    app.ui.chat.banners.system.open = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_balance_reads_parked_machines_and_dismisses_only_the_current_version_pair() {
        let mut app = App::new();
        scene_available(&mut app);
        assert_ne!(app.ui.env.active, "box");
        let eligible = machines(&app);
        assert_eq!(eligible.iter().map(|m| m.key.as_str()).collect::<Vec<_>>(), ["box"]);
        assert_eq!(Summary::new(&eligible).action().as_deref(), Some("Update all"));
        let key = eligible[0].dismissal.clone().unwrap();

        app.client_settings.values.insert(DISMISSALS.into(), json!({"keys": [key]}));
        assert!(machines(&app).is_empty());
        if let Some(crate::data::Source::Fixture(remote)) = env::source_mut(&mut app, "box") {
            remote.config.as_mut().unwrap()["environment"]["serverVersion"] = json!("0.1.5");
        }
        assert_eq!(machines(&app).len(), 1);
        run_on::keep_here(&mut app);
        assert!(machines(&app).is_empty());
    }

    #[test]
    fn aggregate_title_and_action_follow_running_failed_and_manual_machines() {
        let remote = json!({"environment": {"capabilities": {"serverSelfUpdate": "respawn"}}});
        let manual = json!({"environment": {"capabilities": {"serverSelfUpdate": "desktop-managed"}}});
        let running = Status::Running { stage: update::Stage::Resuming, from: "1.0.0".into(), target: "2.0.0".into() };
        let failed = Status::Failed { stage: update::Stage::Downloading, from: "1.0.0".into(), target: "2.0.0".into(), message: "failed".into() };
        let make = |config, state, connected| Machine {
            key: String::new(), label: "Box".into(), config, state, target: "2.0.0".into(), connected, dismissal: None,
        };
        for (rows, title, action, manual_count) in [
            (vec![make(&remote, &Status::Idle, true)], "Update available for 1 machine", Some("Update all"), 0),
            (vec![make(&remote, &Status::Idle, true), make(&manual, &Status::Idle, true)], "Update available for 2 machines", Some("Update 1 machine"), 1),
            (vec![make(&remote, &failed, true), make(&remote, &Status::Idle, false)], "Could not update 1 machine", Some("Retry"), 0),
            (vec![make(&remote, &running, false), make(&remote, &failed, true)], "Updating 1 machine", None, 0),
            (vec![make(&remote, &Status::Idle, false)], "Update available for 1 machine", None, 0),
        ] {
            let summary = Summary::new(&rows);
            assert_eq!((summary.title().as_str(), summary.action().as_deref(), summary.manual), (title, action, manual_count));
        }
        assert_eq!(manual_command("2.0.0", &json!({"kind": "npm-global", "prefix": "/it’s/'here"})).0, "npm install --global --prefix '/it’s/'\\''here' t3@2.0.0");
    }
}
