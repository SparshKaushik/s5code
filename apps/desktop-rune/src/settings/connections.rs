use super::{controls as c, request};
use crate::env::{self, Saved};
use crate::kit::{self, Menu, MenuEntry, MenuItem, Placement};
use crate::theme::{self, AlphaMul, T};
use crate::shell::palette::{toast, toast_error};
use crate::{App, Route, icons};
use rune::{El, Icon, TextEdit, col, el, icon, row, text};
use serde_json::{Value, json};

/// Electron's `DesktopAppSettings` key; the default (on) is omitted from the file.
const LOCAL_KEY: &str = crate::host::LOCAL_ENVIRONMENT_KEY;

/// Web `AuthMcpClientAccess` options with `accessConfig` copy, in picker order.
const ACCESS: [(&str, &str, &str); 5] = [
    ("read-only", "Read only", "Read projects and threads. Cannot start, message or change anything."),
    ("approval-required", "Supervised", "Ask before commands and file changes."),
    ("auto-accept-edits", "Auto-accept edits", "Auto-approve edits, ask before other actions."),
    ("auto", "Auto", "Supported providers approve routine actions; others still ask."),
    ("full-access", "Full access", "Allow commands and edits without prompts."),
];

/// Web `resolveEnvironmentIconPickerLock`.
pub(super) fn icon_lock(connected: bool, may_write: bool) -> Option<&'static str> {
    if !connected {
        return Some("Connect to this environment to change its icon.");
    }
    (!may_write).then_some("Your session on this environment cannot change its settings.")
}

/// Web `ANOTHER_ADDRESS`: link by an address typed in.
const ANOTHER_ADDRESS: &str = "another-address";

/// Web `ENVIRONMENT_MACHINE_KINDS` with `ENVIRONMENT_MACHINE_KIND_LABELS`.
pub(super) const KINDS: [(&str, &str); 7] = [
    ("server", "Server"),
    ("cloud", "Cloud VM"),
    ("linux", "Linux/WSL"),
    ("desktop", "Desktop"),
    ("laptop", "Laptop"),
    ("mac-mini", "Mini PC"),
    ("mac-studio", "Workstation"),
];

static MAC_MINI: Icon = Icon {
    name: "environment-mac-mini",
    view: 24.0,
    body: r#"<rect width="18" height="12" x="3" y="6" rx="2.5" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" fill="none"/><circle cx="7.5" cy="12" r="1" fill="currentColor"/>"#,
};
static MAC_STUDIO: Icon = Icon {
    name: "environment-mac-studio",
    view: 24.0,
    body: r#"<rect width="18" height="14" x="3" y="5" rx="2" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="M7 15h.01M11 15h.01M15 15h.01" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

#[derive(Default)]
pub struct Ui {
    pub(super) page: Option<String>,
    local_disabled: bool,
    local_confirm: bool,
    local_error: Option<String>,
    /// The saved environments when Add environment opened; any change means the pairing landed.
    adding: Option<Vec<Saved>>,
    /// Add environment's SSH mode, and whether its host suggestions are showing.
    ssh: bool,
    ssh_suggest: bool,
    /// A remote pairing was in flight at the last poll.
    was_pairing: bool,
    env_menu: Option<String>,
    env_icon_menu: bool,
    /// Saved environments whose routes, permissions, or failure details are showing.
    routes_open: Vec<String>,
    permissions_open: Vec<String>,
    diagnosis_open: Vec<String>,
    /// A route being dragged (environment, position, distance), one to confirm removing, and
    /// the environment Add environment adds a route to.
    route_drag: Option<(String, usize, f32)>,
    route_remove: Option<(String, usize)>,
    route_target: Option<String>,
    remove: Option<(String, String)>,
    linking: bool,
    link_url: TextEdit,
    link_code: TextEdit,
    link_access: Option<&'static str>,
    link_pending: Option<u64>,
    /// Web `LinkMachineSelect`: the chosen environment key or [`ANOTHER_ADDRESS`], and whether
    /// its list is open.
    link_machine: Option<String>,
    link_machine_open: bool,
    /// The pairing code being minted on a machine, and machines where minting failed.
    link_mint: Option<(String, u64)>,
    mint_failed: Vec<String>,
    forget: Option<(String, String)>,
    pub(super) access: super::access::State,
    pub(super) machines: super::machines::Ui,
}

pub(super) fn desktop_settings() -> rune::prefs::Prefs {
    if rune::is_headless() {
        return rune::prefs::Prefs::memory();
    }
    rune::prefs::Prefs::at(crate::host::t3_home().join("userdata").join("desktop-settings.json"))
}

pub(super) fn take(s: &mut App, id: Option<u64>) -> Option<Result<Value, Value>> {
    s.src.net()?.take_reply(id?)
}

pub(super) fn reason(error: &Value) -> String {
    error["message"].as_str().map_or_else(|| error.to_string(), str::to_owned)
}

/// Web `resolveEnvironmentMachineKind`: the chosen icon, else the detected machine, else a server.
pub(super) fn machine_kind<'a>(chosen: Option<&'a str>, detected: Option<&'a str>) -> &'a str {
    chosen.filter(|kind| KINDS.iter().any(|(k, _)| k == kind)).or(detected).unwrap_or("server")
}

pub(super) fn machine_icon(kind: &str) -> &'static Icon {
    match kind {
        "cloud" => &icons::CLOUD,
        "linux" => &icons::TERMINAL,
        "desktop" => &icons::MONITOR,
        "laptop" => &icons::LAPTOP,
        "mac-mini" => &MAC_MINI,
        "mac-studio" => &MAC_STUDIO,
        _ => &icons::SERVER,
    }
}

/// A `SettingsRow` without a control.
pub(super) fn info_row(t: &T, title: &str, description: &str) -> El {
    col()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(4.0)
        .child(text(title.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(text(description.to_owned()).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap())
}

pub(super) fn dot(color: rune::Color) -> El {
    el().size(6.0, 6.0).radius(3.0).fill(color)
}

pub fn poll(s: &mut App) -> bool {
    let page = match &s.route {
        Route::Settings(p) if p == "connections" || p == "access" => Some(p.clone()),
        _ => None,
    };
    let mut changed = false;
    if page != s.ui.settings.connections.page {
        s.ui.settings.connections = Ui { page: page.clone(), ..Ui::default() };
        if page.as_deref() == Some("connections") {
            s.ui.settings.connections.local_disabled = desktop_settings().get::<bool>(LOCAL_KEY) == Some(false);
        }
        changed = true;
    }
    if let Some(label) = s.ui.env.route_added.take() {
        let ui = &s.ui.settings.connections;
        let alias = ui.route_target.as_deref().filter(|_| ui.ssh).and_then(|id| env::ssh::alias(s, id)).map(str::to_owned);
        close_add(s);
        let description = match alias {
            Some(alias) => format!("{label} can now be reached over SSH {alias}."),
            None => format!("{label} now has another way to connect."),
        };
        toast(s, "Route added", Some(&description));
        changed = true;
    }
    let added = match &s.ui.settings.connections.adding {
        Some(before) if *before != s.ui.env.saved => {
            Some(s.ui.env.saved.iter().find(|e| !before.iter().any(|b| b.id == e.id)).map(|e| e.id.clone()))
        }
        _ => None,
    };
    if let Some(id) = added {
        let ssh = s.ui.settings.connections.ssh;
        close_add(s);
        let alias = id.as_deref().and_then(|id| env::ssh::alias(s, id)).map(str::to_owned);
        match (id, alias) {
            (Some(_), Some(alias)) if ssh => {
                toast(s, "Environment connected", Some(&format!("{alias} is ready over an SSH-managed tunnel.")))
            }
            (Some(_), _) => toast(s, "Backend added", Some("The environment is saved and will reconnect on app startup.")),
            _ => {}
        }
        changed = true;
    }
    let pairing = env::pairing(s);
    if std::mem::replace(&mut s.ui.settings.connections.was_pairing, pairing) && !pairing {
        changed |= pairing_failed(s);
    }
    changed |= poll_machines(s);
    let ui = &mut s.ui.settings.connections;
    let pending = ui.link_pending;
    if let Some(reply) = take(s, pending) {
        s.ui.settings.connections.link_pending = None;
        match reply {
            Ok(linked) => {
                close_link(s);
                toast(s, &format!("Linked {}", linked["label"].as_str().unwrap_or("the environment")), None);
                request(s, "peerLinks.list", json!({}));
            }
            Err(error) => toast_error(s, "Could not link the environment", Some(&reason(&error))),
        }
        changed = true;
    }
    changed | super::machines::poll(s) | super::access::poll(s)
}

pub fn escape(s: &mut App) -> bool {
    let ui = &mut s.ui.settings.connections;
    ui.env_icon_menu = false;
    let closed = ui.env_menu.take().is_some()
        || ui.remove.take().is_some()
        || ui.route_remove.take().is_some()
        || ui.forget.take().is_some()
        || std::mem::take(&mut ui.local_confirm)
        || std::mem::take(&mut ui.ssh_suggest)
        || ui.adding.take().is_some()
        || std::mem::take(&mut ui.link_machine_open)
        || ui.machines.select.take().is_some()
        || ui.machines.confirm.take().is_some()
        || ui.machines.confirm_row.take().is_some();
    if closed {
        return true;
    }
    if ui.linking {
        // Web keeps the dialog open while a link is in flight.
        if ui.link_pending.is_none() && ui.link_mint.is_none() {
            close_link(s);
        }
        return true;
    }
    super::access::escape(s)
}

pub fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    match s.ui.settings.connections.page.as_deref() {
        Some("connections") => {
            local_confirm(s, t, layers);
            add_dialog(s, t, layers);
            env_menu(s, t, layers);
            remove_dialog(s, t, layers);
            route_remove_dialog(s, t, layers);
            link_dialog(s, t, layers);
            forget_dialog(s, t, layers);
            super::machines::confirm_dialog(s, t, layers);
            let _ = super::machines::sections(s, t, layers);
        }
        Some("access") => super::access::overlays(s, t, layers),
        _ => {}
    }
}

pub fn access(s: &App, t: &T) -> El {
    super::access::view(s, t)
}

/// Web `ConnectionsSettings` view "environments".
pub fn view(s: &App, t: &T) -> El {
    let mut page = col().w_full().gap(32.0);
    if crate::host::managed() {
        page = page.child(c::section("This app", vec![local_row(s, t)], t));
    }
    let add = kit::Button::new("Add environment")
        .variant("ghost-muted")
        .size("xs")
        .icon(&icons::PLUS)
        .on_press(|s: &mut App| {
            s.ui.env.error = None;
            s.ui.env.ssh.error = None;
            s.ui.settings.connections.route_target = None;
            s.ui.settings.connections.adding = Some(s.ui.env.saved.clone());
        })
        .build(t);
    let rows = if s.ui.env.saved.is_empty() {
        vec![empty(t)]
    } else {
        s.ui.env.saved.iter().map(|env| saved_row(s, t, env)).collect()
    };
    let actions = super::machines::update_all_button(s, t).into_iter().chain([add]).collect();
    page = page.child(c::section_with_actions("Environments", actions, rows, t));
    if offers_links(s) {
        page = page.child(linked(s, t));
    }
    // The open select's popup is mounted from `overlays`.
    page.children(super::machines::sections(s, t, &mut Vec::new()))
}

fn local_row(s: &App, t: &T) -> El {
    let ui = &s.ui.settings.connections;
    let enabled = !ui.local_disabled;
    let switch = kit::Switch::new(enabled)
        .label("Local environment")
        .hide_label()
        .disabled(ui.local_confirm)
        .on_toggle(|s: &mut App| s.ui.settings.connections.local_confirm = true)
        .build(&kit::theme(t));
    let description = if enabled {
        "Run agents on this computer. Turn off to use T3 Code only with remote environments."
    } else {
        "Turned off. Agents only run in remote environments."
    };
    let mut copy = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(text("Local environment").font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(text(description).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap());
    if let Some(error) = &ui.local_error {
        copy = copy.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap());
    }
    row().w_full().px(16.0).py(12.0).gap(32.0).center_y().child(copy).child(switch)
}

fn local_confirm(s: &App, t: &T, layers: &mut Vec<El>) {
    let ui = &s.ui.settings.connections;
    if !ui.local_confirm {
        return;
    }
    let enabled = !ui.local_disabled;
    let close = |s: &mut App| s.ui.settings.connections.local_confirm = false;
    let (title, description, action, variant) = if enabled {
        (
            "Turn off local environment?",
            "T3 Code will restart without running a server on this computer. Any agents and terminals running here will stop, and other devices will no longer be able to connect to this computer. Your projects, history, and remote environments are unaffected.",
            "Restart and turn off",
            "destructive",
        )
    } else {
        (
            "Turn on local environment?",
            "T3 Code will restart and start running a server on this computer again.",
            "Restart and turn on",
            "default",
        )
    };
    kit::Dialog::new("local-environment-confirm", title)
        .open(true)
        .alert()
        .description(description)
        .on_close(close)
        .child(
            row()
                .gap(8.0)
                .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                .child(
                    kit::Button::new(action)
                        .variant(variant)
                        .on_press(move |s: &mut App| set_local(s, !enabled))
                        .build(t),
                ),
        )
        .build(t, layers);
}

/// Electron's `setLocalEnvironmentEnabled`: persist, then restart the way the exposure change does.
fn set_local(s: &mut App, enabled: bool) {
    let ui = &mut s.ui.settings.connections;
    ui.local_confirm = false;
    let mut prefs = desktop_settings();
    let saved = if enabled { prefs.remove(LOCAL_KEY) } else { prefs.set(LOCAL_KEY, &false) };
    if saved.is_err() {
        ui.local_error = Some("Couldn't change this setting.".into());
        return;
    }
    ui.local_error = None;
    ui.local_disabled = !enabled;
    crate::host::apply_local_setting();
}

/// Web `savedBackendStatus`, from `env::status`'s wording.
fn saved_status(off: bool, status: &str) -> (String, bool) {
    if off {
        return ("Off".into(), false);
    }
    match status {
        "Connected" => ("Connected".into(), false),
        "Connecting..." => ("Connecting".into(), false),
        "Reconnecting..." => ("Reconnecting".into(), true),
        other => (other.trim_end_matches('.').to_owned(), true),
    }
}

/// Web `connectionRouteLabel` for a bearer route.
fn route_label(base: &str) -> String {
    match env::route_kind(base) {
        "loopback" => "This device".into(),
        "tailnet" => "Tailscale".into(),
        "vpn" => "VPN".into(),
        "lan" => "LAN".into(),
        _ => Some(env::route_host(base)).filter(|h| !h.is_empty()).unwrap_or_else(|| "Remote link".into()),
    }
}

/// Web `mcpUrlFromBase`: the route's origin with the path `/mcp`.
fn mcp_url(base: &str) -> Option<String> {
    let (scheme, rest) = base.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().filter(|a| !a.is_empty())?;
    Some(format!("{scheme}://{authority}/mcp"))
}

fn saved_row(s: &App, t: &T, saved: &Saved) -> El {
    let config = env::source(s, &saved.id).and_then(|src| src.state().config.as_ref());
    let kind = machine_kind(
        config.and_then(|c| c["settings"]["environmentIcon"].as_str()),
        config.and_then(|c| c["environment"]["platform"]["machine"].as_str()),
    );
    let off = s.ui.env.off.contains(&saved.id);
    let routes_open = s.ui.settings.connections.routes_open.contains(&saved.id);
    let (status, error) = match env::ssh::state(s, &saved.id).filter(|_| !off) {
        Some(state) => (state, false),
        None => saved_status(off, &env::status(s, &saved.id)),
    };
    // Web `environmentTransportLabel`: SSH and its target, else the address.
    let transport = env::ssh::transport(s, &saved.id).unwrap_or_else(|| saved.base.clone());
    let id = saved.id.clone();
    let route_count = env::routes(s, &saved.id).len();
    let fold = |name: &str, label: String, open: bool, toggle: fn(&mut App, &str)| {
        let key = saved.id.clone();
        row()
            .key(format!("saved-env-{name}-{}", saved.id))
            .gap(2.0)
            .center_y()
            .role("button")
            .label(label.clone())
            .expanded(open)
            .focusable(true)
            .child(text(label).font(theme::TEXT_XS).color(t.c.muted_foreground))
            .child(icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 12.0).color(t.c.muted_foreground))
            .on_press(move |s: &mut App| toggle(s, &key))
    };
    let routes = fold("routes", if route_count <= 1 { "Routes".into() } else { format!("{route_count} routes") }, routes_open, toggle_routes);
    let permissions_open = s.ui.settings.connections.permissions_open.contains(&saved.id);
    let permissions = fold("permissions", "Permissions".into(), permissions_open, toggle_permissions);
    let connected = env::connected(s, &saved.id);
    let gap = (!off).then(|| super::machines::version_gap(s, &saved.id)).flatten();
    let server_version = config.and_then(|c| c["environment"]["serverVersion"].as_str());
    // Web `useConnectionDiagnosis`: while switched on and not connected, what failed.
    let failure = match env::source(s, &saved.id).map(|src| &src.state().conn) {
        Some(s5_net::Conn::Closed(reason)) if !off && env::ssh::state(s, &saved.id).is_none() => Some(reason.as_str()),
        _ => None,
    };
    let diagnosis = failure.and_then(|reason| super::connection_diagnosis::present(&saved.label, reason, Some(env!("CARGO_PKG_VERSION")), server_version));
    let mut parts = vec![transport];
    if diagnosis.is_none() {
        parts.push(status);
    }
    if let Some((server, _)) = &gap {
        parts.push(server.clone());
    }
    let mut tip = env::status(s, &saved.id);
    if let Some((server, client)) = &gap {
        tip.push_str(&format!("\nUpdate available: {server} → {client}"));
    }
    let subtitle = row()
        .w_full()
        .min_w(0.0)
        .gap(4.0)
        .center_y()
        .child(
            text(parts.join(" · "))
                .font(theme::TEXT_XS)
                .color(if error && diagnosis.is_none() { t.c.destructive } else { t.c.muted_foreground })
                .truncate()
                .help(tip)
                .shrink()
                .min_w(0.0),
        )
        .child(text("·").font(theme::TEXT_XS).color(t.c.muted_foreground))
        .child(routes)
        .child(text("·").font(theme::TEXT_XS).color(t.c.muted_foreground))
        .child(permissions);
    let diagnosis_open = s.ui.settings.connections.diagnosis_open.contains(&saved.id);
    let mut identity = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(2.0)
        .opacity(if off { 0.6 } else { 1.0 })
        .child(text(saved.label.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate())
        .child(subtitle);
    // Web `ConnectionDiagnosisSummary`; Rune's transport has no attempt count or retry deadline.
    if let Some(d) = &diagnosis {
        identity = identity
            .child(text(d.headline.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap())
            .child(fold("diagnosis", "Details".into(), diagnosis_open, toggle_diagnosis));
    }
    let updating = super::machines::row_updating(s, &saved.id);
    // Web `ServerUpdateProgress`.
    if updating || super::machines::row_failure(s, &saved.id).is_some() {
        let (line, color) = match super::machines::row_failure(s, &saved.id).filter(|_| !updating) {
            Some(message) => (message.to_owned(), t.c.destructive),
            None => ("Downloading…".to_owned(), t.c.foreground),
        };
        identity = identity.child(
            row().mt(4.0).gap(8.0).center_y().child(el().w(6.0).h(6.0).radius(3.0).fill(color)).child(text(line.clone()).font(theme::XS_MEDIUM).color(color).truncate().help(line).shrink().min_w(0.0)),
        );
    }
    let switch = kit::Switch::new(!off)
        .label(format!("{} {}", if off { "Switch on" } else { "Switch off" }, saved.label))
        .hide_label()
        .size("sm")
        .on_toggle(move |s: &mut App| {
            let on = s.ui.env.off.contains(&id);
            env::set_enabled(s, &id, on);
        })
        .build(&kit::theme(t));
    // Web's row `ServerUpdateAction` (icon appearance) while this server is behind the client.
    let update = (connected && gap.is_some() && !updating && super::machines::row_update(s, &saved.id)).then(|| {
        let label = if super::machines::row_failure(s, &saved.id).is_some() { "Retry update" } else { "Update" };
        let key = saved.id.clone();
        kit::icon_button(&icons::CIRCLE_ARROW_UP, label)
            .variant("ghost-muted")
            .size("icon-xs")
            .on_press(move |s: &mut App| super::machines::update_row(s, &key))
            .build(t)
            .help(label)
    });
    let more = saved.id.clone();
    let line = row()
        .w_full()
        .gap(12.0)
        .center_y()
        .child(icon(machine_icon(kind), 16.0).color(t.c.muted_foreground).opacity(if off { 0.6 } else { 1.0 }))
        .child(identity)
        .child(
            row()
                .gap(4.0)
                .center_y()
                .children(env::ssh::row_action(s, t, &saved.id))
                .children(update)
                .child(switch)
                .child(
                    row().key(format!("saved-env-more-{}", saved.id)).child(
                        kit::icon_button(&icons::ELLIPSIS, &format!("More actions for {}", saved.label))
                            .variant("ghost-muted")
                            .size("icon-xs")
                            .on_press(move |s: &mut App| s.ui.settings.connections.env_menu = Some(more.clone()))
                            .build(t),
                    ),
                ),
        );
    let mut body = col().key(format!("saved-env-{}", saved.id)).w_full().px(16.0).py(10.0).child(line);
    if let Some(d) = diagnosis.filter(|_| diagnosis_open) {
        body = body.child(diagnosis_panel(t, saved, d).ml(28.0));
    }
    if routes_open {
        body = body.child(routes_list(s, t, saved).ml(28.0));
    }
    if permissions_open {
        let session = super::permissions::reply_on(s, &saved.id);
        body = body.child(
            col().ml(28.0).mt(8.0).child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.5))).child(super::access::session_permissions(t, session, connected, true)),
        );
    }
    body
}

/// Web `ConnectionDiagnosisPanel`: hops in order, what to try, the facts, and the actions.
fn diagnosis_panel(t: &T, saved: &Saved, d: super::connection_diagnosis::Diagnosis) -> El {
    use super::connection_diagnosis::Hop;
    let muted = |line: String| text(line).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap();
    let mut hops = col().w_full().gap(2.0);
    for (label, hop) in &d.hops {
        let (mark, note) = match hop {
            Hop::Ok => ("✓", ""),
            Hop::Failed => ("✕", "Failed"),
            Hop::Unchecked => ("–", "Not checked"),
        };
        let color = if *hop == Hop::Failed { t.c.destructive } else { t.c.muted_foreground };
        hops = hops.child(
            row()
                .gap(8.0)
                .child(text(mark).font(theme::TEXT_XS).color(color).w(12.0))
                .child(text(label.clone()).font(theme::TEXT_XS).color(if *hop == Hop::Ok { t.c.foreground } else { color }))
                .child(text(note).font(theme::TEXT_XS).color(color)),
        );
    }
    let mut steps = col().w_full().gap(2.0).child(text("What to try").font(theme::XS_MEDIUM).color(t.c.foreground));
    for (i, step) in d.steps.iter().enumerate() {
        steps = steps.child(muted(format!("{}. {step}", i + 1)));
    }
    let mut facts = col().w_full().gap(2.0);
    for (label, value) in &d.facts {
        facts = facts.child(
            row()
                .w_full()
                .gap(12.0)
                .child(text(*label).font(theme::mono(theme::TEXT_XS)).color(t.c.muted_foreground).w(96.0))
                .child(text(value.clone()).font(theme::mono(theme::TEXT_XS)).color(t.c.foreground).wrap().grow(1.0).min_w(0.0)),
        );
    }
    let key = saved.id.clone();
    let report = super::connection_diagnosis::text(&saved.label, &d);
    let actions = row()
        .gap(8.0)
        .child(kit::Button::new("Retry now").variant("outline").size("xs").on_press(move |s: &mut App| retry_now(s, &key)).build(t))
        .child(kit::Button::new("Copy diagnostics").variant("outline").size("xs").build(t).on_press_tx(move |s: &mut App, tx| {
            tx.copy(report.clone());
            toast(s, "Diagnostics copied", None);
        }));
    col()
        .w_full()
        .mt(8.0)
        .pt(8.0)
        .gap(12.0)
        .child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.5)))
        .child(hops)
        .child(steps)
        .child(facts)
        .child(actions)
}

/// Web `environmentCatalog.retryNow`: skip the backoff wait and connect again.
fn retry_now(s: &mut App, key: &str) {
    if let Some(net) = env::net_of(s, key) {
        net.reconnect();
    }
}

fn toggle_permissions(s: &mut App, id: &str) {
    toggle(&mut s.ui.settings.connections.permissions_open, id);
    super::permissions::load(s);
}

fn toggle_diagnosis(s: &mut App, id: &str) {
    toggle(&mut s.ui.settings.connections.diagnosis_open, id);
}

fn toggle(open: &mut Vec<String>, id: &str) {
    if open.iter().any(|k| k == id) {
        open.retain(|k| k != id);
    } else {
        open.push(id.to_owned());
    }
}

fn toggle_routes(s: &mut App, id: &str) {
    let open = &mut s.ui.settings.connections.routes_open;
    if open.iter().any(|k| k == id) {
        open.retain(|k| k != id);
    } else {
        open.push(id.to_owned());
    }
}

/// One route row's height, which a drag moves past to change places.
const ROUTE_ROW: f32 = 48.0;

/// Web `connectionRouteLabel` and `connectionRouteAddress` for a saved route.
pub(super) fn route_presentation(s: &App, id: &str, route: &env::Alt) -> (String, String) {
    if !env::ssh::is_tunnel(s, id, &route.base) {
        return (route_label(&route.base), route.base.clone());
    }
    let target = env::ssh::remote_host(s, id).map(|(host, user)| user.map_or(host.clone(), |u| format!("{u}@{host}"))).unwrap_or_default();
    let label = if target.is_empty() { "SSH".to_owned() } else { format!("SSH {target}") };
    (label, env::ssh::alias(s, id).unwrap_or_default().to_owned())
}

/// Web `arrayMove`.
fn moved<T>(mut items: Vec<T>, from: usize, to: usize) -> Vec<T> {
    let to = to.min(items.len().saturating_sub(1));
    if from < items.len() {
        let item = items.remove(from);
        items.insert(to, item);
    }
    items
}

fn reorder_route(s: &mut App, id: &str, from: usize, to: usize) {
    if from != to {
        let routes = moved(env::routes(s, id), from, to);
        env::set_routes(s, id, routes);
    }
}

/// Web `EnvironmentRoutesList`: the routes preferred first, each draggable by its grip (or moved
/// with the arrow keys there), removable while another remains, then Add route.
fn routes_list(s: &App, t: &T, saved: &Saved) -> El {
    let routes = env::routes(s, &saved.id);
    let in_use = env::route_in_use(s, &saved.id);
    let drag = s.ui.settings.connections.route_drag.as_ref().filter(|(k, ..)| *k == saved.id);
    let mut list = col().w_full().role("list").label(format!("Routes to {}, preferred first", saved.label));
    for (i, route) in routes.iter().enumerate() {
        let (label, address) = route_presentation(s, &saved.id, route);
        let mut heading = row().gap(6.0).center_y().child(text(label.clone()).font(theme::XS_MEDIUM).color(t.c.foreground));
        if in_use == Some(i) {
            heading = heading.child(
                text("In use")
                    .font(theme::TEXT_2XS)
                    .color(t.c.success_foreground)
                    .px(4.0)
                    .radius(2.0)
                    .fill(t.c.success.alpha_mul(0.12)),
            );
        }
        let (id, count) = (saved.id.clone(), routes.len());
        let keys = saved.id.clone();
        let grip = row()
            .key(format!("route-grip-{}-{i}", saved.id))
            .role("button")
            .label(format!("Reorder {label}, position {}", i + 1))
            .focusable(true)
            .size(24.0, 24.0)
            .center()
            .radius(4.0)
            .cursor(rune::Cursor::Grab)
            .focus_ring(2.0, -2.0, t.c.ring)
            .child(icon(&icons::GRIP_VERTICAL, 14.0).color(t.c.muted_foreground.alpha_mul(0.7)))
            .on_key(move |s: &mut App, e: &rune::KeyEvent, _: &mut rune::Tx| {
                let to = match e.key {
                    rune::Key::Up if i > 0 => i - 1,
                    rune::Key::Down if i + 1 < count => i + 1,
                    _ => return false,
                };
                reorder_route(s, &keys, i, to);
                true
            })
            .on_drag(move |s: &mut App, d: &mut rune::Drag, _: &mut rune::Tx| {
                let dy = d.dy * crate::desktop::from_window(1.0, 1.0).0;
                match d.phase {
                    rune::Phase::Down => {}
                    rune::Phase::Move => s.ui.settings.connections.route_drag = Some((id.clone(), i, dy)),
                    rune::Phase::Up => {
                        s.ui.settings.connections.route_drag = None;
                        if !d.cancelled() {
                            let to = (i as f32 + (dy / ROUTE_ROW).round()).clamp(0.0, (count - 1) as f32) as usize;
                            reorder_route(s, &id, i, to);
                        }
                    }
                }
            });
        let mut item = row()
            .key(format!("route-{}-{i}", saved.id))
            .role("listitem")
            .w_full()
            .h(ROUTE_ROW)
            .gap(8.0)
            .center_y()
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .child(heading)
                    .child(text(address).font(theme::TEXT_2XS).color(t.c.muted_foreground).truncate()),
            )
            .child(grip);
        if routes.len() > 1 {
            let (id, label) = (saved.id.clone(), label.clone());
            item = item.child(
                kit::icon_button(&icons::X, &format!("Remove {label} route"))
                    .variant("ghost-muted")
                    .size("icon-xs")
                    .on_press(move |s: &mut App| s.ui.settings.connections.route_remove = Some((id.clone(), i)))
                    .build(t)
                    .help("Remove route"),
            );
        }
        if let Some((.., dy)) = drag.filter(|(_, from, _)| *from == i) {
            item = item.offset(0.0, *dy).radius(6.0).fill(t.c.background).shadow(4.0, 12.0, rune::Color::BLACK.alpha_mul(0.2)).raise();
        }
        list = list.child(item);
    }
    let target = saved.id.clone();
    col()
        .w_full()
        .mt(8.0)
        .child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.5)))
        .child(col().w_full().py(8.0).child(list).child(
            row().ml(-6.0).pt(4.0).child(
                kit::Button::new("Add route")
                    .variant("ghost-muted")
                    .size("xs")
                    .icon(&icons::PLUS)
                    .on_press(move |s: &mut App| {
                        s.ui.env.error = None;
                        let ui = &mut s.ui.settings.connections;
                        ui.route_target = Some(target.clone());
                        ui.ssh = false;
                        ui.adding = Some(s.ui.env.saved.clone());
                    })
                    .build(t),
            ),
        ))
}

/// Web's confirm before `removeRoute`.
fn route_remove_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some((id, index)) = s.ui.settings.connections.route_remove.clone() else { return };
    let Some(route) = env::routes(s, &id).into_iter().nth(index) else { return };
    let (label, address) = route_presentation(s, &id, &route);
    let close = |s: &mut App| s.ui.settings.connections.route_remove = None;
    kit::Dialog::new("remove-route", format!("Remove {label} route?"))
        .open(true)
        .alert()
        .description(address)
        .on_close(close)
        .child(
            row()
                .gap(8.0)
                .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                .child(
                    kit::Button::new("Confirm")
                        .variant("destructive")
                        .on_press(move |s: &mut App| {
                            s.ui.settings.connections.route_remove = None;
                            let mut routes = env::routes(s, &id);
                            if routes.len() > 1 && index < routes.len() {
                                routes.remove(index);
                                env::set_routes(s, &id, routes);
                            }
                        })
                        .build(t),
                ),
        )
        .build(t, layers);
}

/// Web `EmptyRemoteEnvironments` without T3 Connect.
fn empty(t: &T) -> El {
    // `EmptyMedia variant="icon"`: two tilted cards behind the icon tile.
    let card = || el().size(36.0, 36.0).absolute(0.0, 0.0).radius(theme::radius::MD).border(1.0, t.c.border).fill(t.c.card);
    let media = el()
        .size(36.0, 36.0)
        // `mb-6` plus `Empty`'s `gap-6` before the header.
        .mb(48.0)
        .child(card().origin(0.0, 1.0).rotate(-10f32.to_radians()).scale(0.84).offset(-2.0, -1.0))
        .child(card().origin(1.0, 1.0).rotate(10f32.to_radians()).scale(0.84).offset(2.0, -1.0))
        .child(
            card()
                .center()
                .shadow(1.0, 2.0, rune::Color::BLACK.alpha(0.05))
                .child(icon(&icons::CHEVRONS_LEFT_RIGHT_ELLIPSIS, 18.0).color(t.c.foreground)),
        );
    col()
        .w_full()
        .min_h(208.0)
        .padding(48.0)
        .center()
        .child(media)
        .child(
            text("No saved remote environments")
                .font(theme::weight(theme::TEXT_XL, theme::SEMIBOLD))
                .color(t.c.foreground),
        )
        .child(
            text("Click “Add environment” to pair another environment.")
                .font(theme::TEXT_SM)
                .color(t.c.muted_foreground)
                .align(rune::Align::Center)
                .wrap()
                .max_w(384.0)
                .mt(4.0),
        )
}

fn env_menu(s: &App, t: &T, layers: &mut Vec<El>) {
    let ui = &s.ui.settings.connections;
    let Some(id) = &ui.env_menu else { return };
    let Some(saved) = s.ui.env.saved.iter().find(|e| &e.id == id) else { return };
    let target = (saved.id.clone(), saved.label.clone());
    let close = |s: &mut App| {
        let ui = &mut s.ui.settings.connections;
        ui.env_menu = None;
        ui.env_icon_menu = false;
    };
    let config = env::source(s, &saved.id).and_then(|src| src.state().config.as_ref());
    let detected_kind = config.and_then(|c| c["environment"]["platform"]["machine"].as_str());
    let resolved = machine_kind(config.and_then(|c| c["settings"]["environmentIcon"].as_str()), detected_kind);
    let detected = detected_kind.unwrap_or("server").to_owned();
    let lock = icon_lock(config.is_some(), super::permissions::can_on(s, &saved.id, "settings:write"));
    let mut icons_entries = Vec::new();
    if let Some(lock) = lock {
        icons_entries.push(MenuEntry::item(MenuItem::new(lock).disabled(true)));
        icons_entries.push(MenuEntry::Separator);
    }
    for (kind, label) in KINDS {
        let (detected, env_id) = (detected.clone(), saved.id.clone());
        let mut item = MenuItem::new(label).icon(machine_icon(kind)).radio(kind == resolved).disabled(lock.is_some());
        if kind == detected {
            item = item.trailing(
                text(if detected_kind.is_some() { "detected" } else { "default" }).font(theme::TEXT_XS).color(t.c.muted_foreground),
            );
        }
        icons_entries.push(MenuEntry::item(item.on_select(move |s: &mut App| {
            let icon = if kind == detected { Value::Null } else { json!(kind) };
            if let Some(net) = env::net_of(s, &env_id) {
                net.request("server.updateSettings", json!({"patch": {"environmentIcon": icon}}));
            }
            close(s);
        })));
    }
    let routes_open = ui.routes_open.contains(&saved.id);
    let toggle = saved.id.clone();
    let mut menu = Menu::new("saved-env-menu")
        .anchor(format!("saved-env-more-{}", saved.id))
        .width(224.0)
        .placement(Placement::BottomEnd)
        .gutter(4.0)
        .open(true)
        .on_close(close)
        .item(
            MenuItem::new("Icon")
                .icon(machine_icon(resolved))
                .submenu(icons_entries, ui.env_icon_menu)
                .submenu_min_width(200.0)
                .on_submenu_change(|s: &mut App, open| s.ui.settings.connections.env_icon_menu = open),
        )
        .item(MenuItem::new(if routes_open { "Hide routes" } else { "Routes" }).icon(&icons::ROUTE).on_select(move |s: &mut App| {
            toggle_routes(s, &toggle);
            close(s);
        }));
    if let Some(url) = mcp_url(&saved.base) {
        menu = menu.item(MenuItem::new("Copy MCP URL").on_select_tx(move |s: &mut App, tx| {
            tx.copy(url.clone());
            close(s);
            toast(s, "MCP URL copied", Some(&format!("Add it to an agent, e.g. claude mcp add --transport http t3 {url}")));
        }));
    }
    menu.separator()
        .item(MenuItem::new("Remove from this device…").variant("destructive").on_select(move |s: &mut App| {
            close(s);
            s.ui.settings.connections.remove = Some(target.clone());
        }))
        .overlay(&kit::theme(t), layers);
}

fn remove_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some((id, label)) = &s.ui.settings.connections.remove else { return };
    let id = id.clone();
    let close = |s: &mut App| s.ui.settings.connections.remove = None;
    kit::Dialog::new("remove-environment", format!("Remove {label} from this device?"))
        .open(true)
        .alert()
        .description("This forgets its pairing, credentials, and cached threads here.")
        .on_close(close)
        .child(
            row()
                .gap(8.0)
                .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                .child(
                    kit::Button::new("Remove")
                        .variant("destructive")
                        .on_press(move |s: &mut App| {
                            s.ui.settings.connections.remove = None;
                            env::remove(s, &id);
                        })
                        .build(t),
                ),
        )
        .build(t, layers);
}

fn close_add(s: &mut App) {
    let ui = &mut s.ui.settings.connections;
    ui.adding = None;
    ui.route_target = None;
    ui.ssh = false;
    ui.ssh_suggest = false;
}

/// Web's "Could not add backend" toast when a remote pairing fails; the error also stays inline.
fn pairing_failed(s: &mut App) -> bool {
    if s.ui.settings.connections.adding.is_none() {
        return false;
    }
    let Some(error) = s.ui.env.error.clone() else { return false };
    toast_error(s, "Could not add backend", Some(&error));
    true
}

fn add_remote(s: &mut App) {
    match s.ui.settings.connections.route_target.clone() {
        Some(id) => env::add_route(s, &id),
        None => env::add(s),
    }
    if !env::pairing(s) {
        pairing_failed(s);
    }
}

fn add_ssh(s: &mut App) {
    s.ui.settings.connections.ssh_suggest = false;
    match s.ui.settings.connections.route_target.clone() {
        Some(id) => env::add_ssh_route(s, &id),
        None => env::ssh::add(s),
    }
}

/// Web `AddEnvironmentDialog`: a Remote link or SSH mode card, then that mode's fields. Pairing
/// runs through `env`, SSH through `env::ssh`.
fn add_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let ui = &s.ui.settings.connections;
    if ui.adding.is_none() {
        return;
    }
    let busy = env::pairing(s) || env::ssh::adding(s);
    let route = ui.route_target.as_ref().map(|id| env::label(s, id));
    let (title, description) = match &route {
        Some(label) => (
            format!("Add a route to {label}"),
            "Pair this machine again over another address, such as its Tailscale name. It joins the existing routes instead of adding a second machine.",
        ),
        None => ("Add Environment".to_owned(), "Pair another environment to this client."),
    };
    let modes = row().w_full().gap(12.0).child(mode_card(t, false, !ui.ssh, busy)).child(mode_card(t, true, ui.ssh, busy));
    kit::Dialog::new("add-environment", title)
        .open(true)
        .description(description)
        .width(768.0)
        .on_close(close_add)
        .child(modes)
        .child(if ui.ssh { ssh_fields(s, t) } else { remote_fields(s, t) })
        .build(t, layers);
}

/// Web `renderConnectionModeCard`.
fn mode_card(t: &T, ssh: bool, selected: bool, busy: bool) -> El {
    let (title, description, glyph): (&str, &str, &'static Icon) = if ssh {
        ("SSH", "Use local SSH config, agent, and tunnels for the backend.", &icons::TERMINAL)
    } else {
        ("Remote link", "Enter a backend host and pairing code.", &icons::CHEVRONS_LEFT_RIGHT_ELLIPSIS)
    };
    let tile = el()
        .size(32.0, 32.0)
        .mt(2.0)
        .radius(6.0)
        .center()
        .border(1.0, if selected { t.c.primary.alpha_mul(0.3) } else { t.c.border.alpha_mul(0.7) })
        .fill(if selected { t.c.primary.alpha_mul(0.1) } else { t.c.background })
        .child(icon(glyph, 16.0).color(if selected { t.c.primary } else { t.c.muted_foreground }));
    let mut card = row()
        .key(format!("add-environment-mode-{}", if ssh { "ssh" } else { "remote" }))
        .grow(1.0)
        .min_w(0.0)
        .min_h(96.0)
        .padding(16.0)
        .gap(12.0)
        .items_start()
        .radius(8.0)
        .border(1.0, if selected { t.c.primary.alpha_mul(0.5) } else { t.c.border.alpha_mul(0.6) })
        .fill(if selected { t.c.primary.alpha_mul(0.05) } else { t.c.background.alpha_mul(0.0) })
        .role("button")
        .label(title)
        .checked(selected)
        .focusable(!busy)
        .child(tile)
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(text(title).font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(text(description).font(theme::TEXT_XS.lh(19.5)).color(t.c.muted_foreground).wrap()),
        );
    if !selected {
        card = card.hover(t.c.muted.alpha_mul(0.4));
    }
    if busy {
        return card;
    }
    card.on_press(move |s: &mut App| {
        let ui = &mut s.ui.settings.connections;
        ui.ssh = ssh;
        ui.ssh_suggest = false;
    })
}

fn field<'a>(t: &'a T) -> impl Fn(&str, El) -> El + 'a {
    move |label: &str, input: El| {
        col().gap(6.0).child(text(label.to_owned()).font(theme::XS_MEDIUM).color(t.c.foreground)).child(input)
    }
}

/// Web `renderRemoteModeBody` without the T3 Connect route offer.
fn remote_fields(s: &App, t: &T) -> El {
    let env = &s.ui.env;
    let pairing = env::pairing(s);
    let field = field(t);
    let host = kit::Input::new(&env.host)
        .placeholder("backend.example.com")
        .disabled(pairing)
        .on_edit(|s: &mut App| &mut s.ui.env.host)
        .on_submit(|s: &mut App, _| add_remote(s))
        .build(t)
        .single_line();
    let code = kit::Input::new(&env.code)
        .placeholder("PAIRCODE")
        .disabled(pairing)
        .on_edit(|s: &mut App| &mut s.ui.env.code)
        .on_submit(|s: &mut App, _| add_remote(s))
        .build(t)
        .single_line();
    let mut body = col().w_full().gap(16.0).child(
        col()
            .w_full()
            .gap(12.0)
            .child(row().w_full().gap(12.0).child(field("Host", host).grow(1.0).shrink().min_w(0.0)).child(field("Pairing code", code).w(160.0)))
            .child(text("Paste a full pairing URL here to fill both fields automatically.").font(theme::TEXT_2XS).color(t.c.muted_foreground).mt(4.0)),
    );
    if let Some(error) = &env.error {
        body = body.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap());
    }
    body.child(
        kit::Button::new(match (pairing, s.ui.settings.connections.route_target.is_some()) {
            (true, _) => "Adding…",
            (false, true) => "Add route",
            (false, false) => "Add environment",
        })
            .variant("outline")
            .icon(&icons::PLUS)
            .disabled(pairing)
            .on_press(add_remote)
            .build(t)
            .w_full(),
    )
}

/// Web `renderSshFields`: the host field suggests unsaved SSH config hosts (inline here, where
/// web floats an autocomplete popup), then username, port and an error alert.
fn ssh_fields(s: &App, t: &T) -> El {
    let ui = &s.ui.env.ssh;
    let adding = env::ssh::adding(s);
    let field = field(t);
    let host = kit::Input::new(&ui.host)
        .placeholder("Search hosts or type devbox")
        .disabled(adding)
        .on_edit(|s: &mut App| &mut s.ui.env.ssh.host)
        .on_change(|s: &mut App, _| s.ui.settings.connections.ssh_suggest = true)
        .on_submit(|s: &mut App, _| add_ssh(s))
        .build(t)
        .single_line();
    let mut host_field = col().w_full().gap(6.0).child(text("SSH host or alias").font(theme::XS_MEDIUM).color(t.c.foreground)).child(host);
    let suggestions = env::ssh::suggestions(s);
    if s.ui.settings.connections.ssh_suggest && !suggestions.is_empty() {
        let mut list = col()
            .key("add-environment-ssh-hosts")
            .w_full()
            .max_h(288.0)
            .padding(4.0)
            .radius(8.0)
            .border(1.0, t.c.border)
            .fill(t.c.popover)
            .scroll(rune::ScrollSpec { stick_bottom: false });
        for (alias, hostname) in suggestions {
            let picked = alias.clone();
            let mut item = row()
                .w_full()
                .h(32.0)
                .px(8.0)
                .gap(8.0)
                .center_y()
                .radius(4.0)
                .role("option")
                .label(alias.clone())
                .focusable(true)
                .hover(t.c.accent)
                .child(text(alias.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate());
            item = if hostname != alias && !hostname.is_empty() {
                item.child(text(hostname).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().grow(1.0).min_w(0.0))
            } else {
                item.child(el().grow(1.0))
            };
            list = list.child(item.on_press(move |s: &mut App| {
                s.ui.settings.connections.ssh_suggest = false;
                env::ssh::pick(s, &picked);
                add_ssh(s);
            }));
        }
        host_field = host_field.child(list);
    }
    let username = kit::Input::new(&ui.username)
        .placeholder("root")
        .disabled(adding)
        .on_edit(|s: &mut App| &mut s.ui.env.ssh.username)
        .on_submit(|s: &mut App, _| add_ssh(s))
        .build(t)
        .single_line();
    let port = kit::Input::new(&ui.port)
        .placeholder("22")
        .disabled(adding)
        .on_edit(|s: &mut App| &mut s.ui.env.ssh.port)
        .on_submit(|s: &mut App, _| add_ssh(s))
        .build(t)
        .single_line();
    let mut body = col()
        .w_full()
        .gap(12.0)
        .child(host_field)
        .child(row().w_full().gap(12.0).child(field("Username", username).grow(1.0).shrink().min_w(0.0)).child(field("Port", port).w(112.0)));
    if let Some(error) = &ui.error {
        // Web `Alert variant="error"`.
        body = body.child(
            row()
                .w_full()
                .px(14.0)
                .py(12.0)
                .gap(8.0)
                .radius(12.0)
                .border(1.0, t.c.destructive.alpha_mul(0.32))
                .fill(t.c.destructive.alpha_mul(0.06))
                .child(text(error.clone()).font(theme::TEXT_SM).color(t.c.destructive).wrap().grow(1.0).min_w(0.0)),
        );
    }
    body.child(
        kit::Button::new(match (adding, s.ui.settings.connections.route_target.is_some()) {
            (true, _) => "Adding…",
            (false, true) => "Add route",
            (false, false) => "Add environment",
        })
            .variant("outline")
            .icon(&icons::PLUS)
            .disabled(adding)
            .on_press(add_ssh)
            .build(t)
            .w_full(),
    )
}

/// Web `offersLinkedEnvironments`.
fn offers_links(s: &App) -> bool {
    s.state().config.as_ref().is_some_and(|c| c["environment"]["capabilities"]["peerLinks"] == true)
        && super::server_settings(s)["linkedEnvironmentsEnabled"] == true
}

#[derive(Debug, PartialEq)]
enum Health {
    Reachable,
    Expiring,
    Failing,
    Unreachable,
}

/// Web `peerLinkHealth` and the row's status copy.
fn link_status(link: &Value, now_ms: i64) -> (String, Health) {
    let expires = crate::data::ts(link, "expiresAt");
    if link["status"] == "expired" || expires.is_some_and(|e| e <= now_ms) {
        return ("Expired. Link it again with a new pairing code.".into(), Health::Failing);
    }
    if link["status"] == "unreachable" {
        let detail = link["lastError"].as_str().unwrap_or("Not answering right now.");
        return (detail.to_owned(), Health::Unreachable);
    }
    if let Some(error) = link["lastError"].as_str() {
        return (error.to_owned(), Health::Failing);
    }
    match expires.map(|e| ((e - now_ms) as f64 / 86_400_000.0).ceil() as i64) {
        Some(days) if days <= 5 => (
            format!("Expires in {days} {}. Link it again to renew.", if days == 1 { "day" } else { "days" }),
            Health::Expiring,
        ),
        _ => ("Reachable".into(), Health::Reachable),
    }
}

fn access_label(access: &str) -> &str {
    ACCESS.iter().find(|(key, ..)| *key == access).map_or(access, |(_, label, _)| *label)
}

/// Web `LinkedEnvironmentsSettings`.
fn linked(s: &App, t: &T) -> El {
    let mut rows = Vec::new();
    match s.ui.settings.results.get("peerLinks.list") {
        None => rows.push(info_row(t, "Loading linked environments…", "")),
        Some(result) => {
            let links: Vec<&Value> = result["links"].as_array().into_iter().flatten().collect();
            if links.is_empty() {
                rows.push(info_row(
                    t,
                    "No linked environments",
                    "Link another machine running T3 Code, such as a VPS, so agents here can launch and drive threads there.",
                ));
            }
            for link in links {
                rows.push(link_row(s, t, link));
            }
        }
    }
    let action = kit::Button::new("Link")
        .size("xs")
        .icon(&icons::PLUS)
        .on_press(open_link)
        .build(t);
    // Web shows Link only to a session that may link.
    let actions = if super::permissions::allowed(s, "peerLinks.link") { vec![action] } else { vec![] };
    c::section_with_actions("Linked environments", actions, rows, t)
}

fn link_row(s: &App, t: &T, link: &Value) -> El {
    let (status, health) = link_status(link, s.now_ms);
    let color = match health {
        Health::Reachable => t.c.success,
        Health::Expiring => t.c.warning,
        Health::Failing => t.c.destructive,
        Health::Unreachable => t.c.muted_foreground.alpha_mul(0.3),
    };
    let label = link["label"].as_str().unwrap_or("Environment").to_owned();
    let urls: Vec<&str> = link["urls"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let target = (link["environmentId"].as_str().unwrap_or_default().to_owned(), label.clone());
    row()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(12.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(
                    row()
                        .min_h(20.0)
                        .gap(6.0)
                        .center_y()
                        .child(dot(color))
                        .child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground)),
                )
                .child(
                    text(format!("{} · {status}", access_label(link["access"].as_str().unwrap_or_default())))
                        .font(theme::TEXT_XS)
                        .color(t.c.muted_foreground)
                        .wrap(),
                )
                .child(
                    text(urls.join(", "))
                        .font(theme::TEXT_XS)
                        .color(t.c.muted_foreground.alpha_mul(0.8))
                        .truncate(),
                ),
        )
        .children(super::permissions::allowed(s, "peerLinks.unlink").then(|| {
            kit::Button::new("Forget")
                .variant("destructive-outline")
                .size("xs")
                .on_press(move |s: &mut App| s.ui.settings.connections.forget = Some(target.clone()))
                .build(t)
        }))
}

fn forget_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some((id, label)) = &s.ui.settings.connections.forget else { return };
    let id = id.clone();
    let close = |s: &mut App| s.ui.settings.connections.forget = None;
    kit::Dialog::new("forget-link", format!("Forget {label}?"))
        .open(true)
        .alert()
        .description(format!(
            "Agents here stop working there. Revoke \"T3 Code · …\" in {label}'s Connections to end the session there too."
        ))
        .on_close(close)
        .child(
            row()
                .gap(8.0)
                .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                .child(
                    kit::Button::new("Forget")
                        .variant("destructive")
                        .on_press(move |s: &mut App| {
                            s.ui.settings.connections.forget = None;
                            request(s, "peerLinks.unlink", json!({"environmentId": id}));
                        })
                        .build(t),
                ),
        )
        .build(t, layers);
}

/// Web `LinkableMachine`: another environment this client knows.
struct Machine {
    key: String,
    id: String,
    label: String,
    kind: String,
    connected: bool,
    linked: bool,
    urls: Vec<String>,
}

/// Web `linkableMachines`' addresses: those the server reports, then the saved route. A
/// machine reached over SSH has none, since its forward lives in this client.
fn machine_urls(config: Option<&Value>, route: Option<&str>) -> Vec<String> {
    let reported = config.and_then(|c| c["directEndpoints"].as_array()).into_iter().flatten();
    let mut urls: Vec<String> = Vec::new();
    for url in reported.filter_map(|e| e["httpBaseUrl"].as_str()).chain(route) {
        let url = url.trim_end_matches('/').to_owned();
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    urls
}

/// Web `linkableMachines` over this client's environments other than the one in Settings.
fn machines(s: &App) -> Vec<Machine> {
    let links = s.ui.settings.results.get("peerLinks.list").and_then(|r| r["links"].as_array());
    let linked = |id: &str| links.into_iter().flatten().any(|l| l["environmentId"] == id && l["status"] != "expired");
    env::keys(s)
        .into_iter()
        .filter(|key| *key != s.ui.env.active)
        .filter_map(|key| {
            let config = env::source(s, &key).and_then(|src| src.state().config.as_ref());
            let id = if key == env::LOCAL { env::local_id(s)? } else { key.clone() };
            let saved = s.ui.env.saved.iter().find(|e| e.id == key);
            let route = saved.filter(|_| env::ssh::transport(s, &key).is_none()).map(|e| e.base.as_str());
            Some(Machine {
                label: env::label(s, &key),
                kind: machine_kind(
                    config.and_then(|c| c["settings"]["environmentIcon"].as_str()),
                    config.and_then(|c| c["environment"]["platform"]["machine"].as_str()),
                )
                .to_owned(),
                connected: env::connected(s, &key),
                linked: linked(&id),
                urls: machine_urls(config, route),
                key,
                id,
            })
        })
        .collect()
}

fn open_link(s: &mut App) {
    // Whether this client may mint a code on each machine is that machine's session.
    super::permissions::load(s);
    s.ui.settings.connections.linking = true;
}

fn close_link(s: &mut App) {
    let ui = &mut s.ui.settings.connections;
    ui.linking = false;
    ui.link_url.clear();
    ui.link_code.clear();
    ui.link_access = None;
    ui.link_machine = None;
    ui.link_machine_open = false;
    ui.mint_failed.clear();
}

/// A pairing code minted on another machine, which is then linked with.
fn poll_machines(s: &mut App) -> bool {
    let Some((key, id)) = s.ui.settings.connections.link_mint.clone() else { return false };
    let reply = env::net_of(s, &key).map(|net| net.take_reply(id));
    let code = match reply {
        Some(None) => return false,
        Some(Some(Ok(minted))) => minted["credential"].as_str().map(str::to_owned),
        _ => None,
    };
    s.ui.settings.connections.link_mint = None;
    match code {
        Some(code) => send_link(s, &code),
        None => {
            s.ui.settings.connections.mint_failed.push(key);
            toast_error(s, "Could not link the environment", Some("Could not get a pairing code from there. Paste a code from there instead."));
        }
    }
    true
}

/// What the dialog would link: the machine chosen (if any), whether it needs an address typed
/// and whether this client mints the code there.
fn link_target(s: &App) -> (Option<Machine>, bool, bool) {
    let ui = &s.ui.settings.connections;
    let all = machines(s);
    let selected = ui.link_machine.clone().or_else(|| all.is_empty().then(|| ANOTHER_ADDRESS.to_owned()));
    let machine = all.into_iter().find(|m| Some(&m.key) == selected.as_ref());
    let needs_address = selected.as_deref() == Some(ANOTHER_ADDRESS) || machine.as_ref().is_some_and(|m| m.urls.is_empty());
    let can_mint = machine.as_ref().is_some_and(|m| {
        m.connected && super::permissions::can_on(s, &m.key, "access:write") && !ui.mint_failed.contains(&m.key)
    });
    (machine, needs_address, can_mint)
}

fn link_ready(s: &App) -> bool {
    let ui = &s.ui.settings.connections;
    let (machine, needs_address, can_mint) = link_target(s);
    (machine.is_some() || needs_address)
        && (!needs_address || !ui.link_url.text.trim().is_empty())
        && (can_mint || !ui.link_code.text.trim().is_empty())
}

fn link(s: &mut App) {
    let ui = &s.ui.settings.connections;
    if ui.link_pending.is_some() || ui.link_mint.is_some() || !link_ready(s) {
        return;
    }
    let (machine, _, can_mint) = link_target(s);
    match machine.filter(|_| can_mint) {
        Some(machine) => {
            let access = ui.link_access.unwrap_or("approval-required");
            let payload = json!({"label": "T3 Code link", "scopes": mcp_client_scopes(access)});
            let id = env::net_of(s, &machine.key).map(|net| net.http_request("POST", "/api/auth/pairing-token", payload));
            s.ui.settings.connections.link_mint = id.map(|id| (machine.key, id));
        }
        None => {
            let code = ui.link_code.text.trim().to_owned();
            send_link(s, &code);
        }
    }
}

/// Contracts `mcpClientScopes`.
fn mcp_client_scopes(access: &str) -> Vec<&'static str> {
    match access {
        "read-only" => vec!["orchestration:read"],
        "full-access" => vec!["orchestration:read", "orchestration:operate", "settings:write", "source-control:write"],
        _ => vec!["orchestration:read", "orchestration:operate"],
    }
}

fn send_link(s: &mut App, code: &str) {
    let (machine, needs_address, _) = link_target(s);
    let ui = &s.ui.settings.connections;
    let urls = match &machine {
        Some(m) if !needs_address => m.urls.clone(),
        _ => vec![ui.link_url.text.trim().to_owned()],
    };
    let mut input = json!({"url": urls[0], "pairingCode": code, "access": ui.link_access.unwrap_or("approval-required")});
    if urls.len() > 1 {
        input["alternateUrls"] = json!(urls[1..]);
    }
    if let Some(m) = &machine {
        input["expectedEnvironmentId"] = json!(m.id);
        input["expectedLabel"] = json!(m.label);
    }
    let id = s.src.net().map(|net| net.request("peerLinks.link", input));
    s.ui.settings.connections.link_pending = id;
}

/// Web `LinkEnvironmentAction` for an address and pairing code.
fn link_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let ui = &s.ui.settings.connections;
    if !ui.linking {
        return;
    }
    let busy = ui.link_pending.is_some() || ui.link_mint.is_some();
    let ready = link_ready(s);
    let current = ui.link_access.unwrap_or("approval-required");
    let all = machines(s);
    let (machine, needs_address, can_mint) = link_target(s);
    let selected = ui.link_machine.clone().or_else(|| all.is_empty().then(|| ANOTHER_ADDRESS.to_owned()));
    let field = |label: &str, input: El| {
        col().w_full().gap(6.0).child(text(label.to_owned()).font(theme::XS_MEDIUM).color(t.c.foreground)).child(input)
    };
    let mut access = col().w_full().gap(8.0).child(text("Access").font(theme::XS_MEDIUM).color(t.c.foreground));
    for (key, label, description) in ACCESS {
        let selected = key == current;
        access = access.child(
            row()
                .w_full()
                .px(12.0)
                .py(10.0)
                .gap(10.0)
                .items_start()
                .radius(8.0)
                .border(1.0, if selected { t.c.primary.alpha_mul(0.5) } else { t.c.border })
                .fill(if selected { t.c.primary.alpha_mul(0.05) } else { t.c.background.alpha_mul(0.0) })
                .role("radio")
                .label(label)
                .checked(selected)
                .focusable(!busy)
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground))
                        .child(text(description).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()),
                )
                .on_press(move |s: &mut App| {
                    let ui = &mut s.ui.settings.connections;
                    if ui.link_pending.is_none() && ui.link_mint.is_none() {
                        ui.link_access = Some(key);
                    }
                }),
        );
    }
    let hint = |line: String| text(line).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap();
    let mut form = col().w_full().gap(16.0);
    let mut popup = Vec::new();
    if !all.is_empty() {
        // Web `LinkMachineSelect`: each machine's icon, name and badge by its id, then an address.
        let mut options: Vec<String> = all.iter().map(|m| m.key.clone()).collect();
        options.push(ANOTHER_ADDRESS.into());
        let mut select = kit::Select::new("link-machine", &options)
            .label("Machine")
            .placeholder("Choose a machine")
            .width(400.0)
            .current(selected.as_deref())
            .option_name(ANOTHER_ADDRESS, "Another address…")
            .open(ui.link_machine_open && !busy)
            .disabled(busy)
            .on_toggle(|s: &mut App| s.ui.settings.connections.link_machine_open ^= true)
            .on_close(|s: &mut App| s.ui.settings.connections.link_machine_open = false)
            .on_choose(|s: &mut App, key: &str, _: &mut rune::Tx| {
                let ui = &mut s.ui.settings.connections;
                ui.link_machine = Some(key.to_owned());
                ui.link_machine_open = false;
            });
        if let Some(m) = &machine {
            select = select.leading(icon(machine_icon(&m.kind), 16.0).color(t.c.foreground).opacity(0.8), 16.0);
        }
        for m in &all {
            select = select.option_name(m.key.clone(), m.label.clone()).option_detail(m.key.clone(), machine_icon(&m.kind), "");
            // Web `LinkMachineBadge`.
            if m.linked {
                select = select.option_trailing(m.key.clone(), kit::badge("Linked", "success", "sm", t));
            } else if !m.connected {
                select = select.option_trailing(m.key.clone(), kit::badge("Not connected", "outline", "sm", t));
            }
        }
        form = form.child(field("Machine", select.build(&kit::theme(t), &mut popup)));
    }
    if machine.as_ref().is_some_and(|m| m.linked) {
        form = form.child(hint("Already linked. Linking again renews it with the access you pick.".into()));
    }
    if needs_address {
        let mut address = col().w_full().gap(6.0).child(field(
            "Address",
            kit::Input::new(&ui.link_url)
                .placeholder("https://box.example.ts.net")
                .disabled(busy)
                .on_edit(|s: &mut App| &mut s.ui.settings.connections.link_url)
                .on_submit(|s: &mut App, _| link(s))
                .build(t)
                .single_line(),
        ));
        if let Some(m) = &machine {
            address = address.child(hint(format!(
                "This app has no address for {} that another machine can reach. Enter its https or Tailscale address.",
                m.label
            )));
        }
        form = form.child(address);
    }
    if selected.is_some() && !can_mint {
        let mut code = col().w_full().gap(6.0).child(field(
            "Pairing code",
            kit::Input::new(&ui.link_code)
                .placeholder(if machine.is_none() { "From that environment's Connections, or t3 pair there" } else { "Paste the code" })
                .disabled(busy)
                .on_edit(|s: &mut App| &mut s.ui.settings.connections.link_code)
                .on_submit(|s: &mut App, _| link(s))
                .build(t)
                .single_line(),
        ));
        if let Some(m) = &machine {
            code = code.child(hint(format!(
                "Create a link on {}: Settings → Connections → Create link, then paste its code here.",
                m.label
            )));
        }
        form = form.child(code);
    }
    form = form.child(access);
    let close = |s: &mut App| {
        let ui = &s.ui.settings.connections;
        if ui.link_pending.is_none() && ui.link_mint.is_none() {
            close_link(s);
        }
    };
    kit::Dialog::new("link-environment", "Link an environment")
        .open(true)
        .description("Agents here can then launch, message and wait on threads there. It appears in that environment's Connections, where it can be revoked.")
        .width(448.0)
        .on_close(close)
        .child(form)
        .child(
            row()
                .gap(8.0)
                .child(kit::Button::new("Cancel").variant("outline").disabled(busy).on_press(close_link).build(t))
                .child(
                    kit::Button::new(if busy { "Linking…" } else { "Link" })
                        .disabled(busy || !ready)
                        .on_press(link)
                        .build(t),
                ),
        )
        .build(t, layers);
    layers.extend(popup);
}

/// Scenes: a saved environment on a fixture, and the connections overlay that is open.
pub(super) fn scene(s: &mut App, open: &str) {
    s.ui.env.saved.push(Saved {
        id: "env-box".into(),
        label: "Hetzner box".into(),
        base: "https://box.tail1234.ts.net".into(),
        bearer: String::new(),
    });
    env::park(s, "env-box", crate::data::Source::Fixture(crate::data::fixture_state()));
    // What's saved before the dialog opened; anything new since closes it as added.
    let before = s.ui.env.saved.clone();
    let ui = &mut s.ui.settings.connections;
    // Without the page, the first poll resets the overlays as a page change.
    ui.page = Some("connections".into());
    match open {
        "add-remote" => ui.adding = Some(before),
        "add-ssh" => {
            ui.adding = Some(before);
            ui.ssh = true;
        }
        "env-menu" => {
            ui.env_menu = Some("env-box".into());
            ui.routes_open.push("env-box".into());
        }
        "failing" => {
            ui.diagnosis_open.push("env-box".into());
            ui.permissions_open.push("env-box".into());
            if let Some((_, crate::data::Source::Fixture(state))) = s.ui.env.parked.iter_mut().find(|(k, _)| k == "env-box") {
                state.conn = s5_net::Conn::Closed("HTTP error: 502 Bad Gateway".into());
            }
        }
        "routes" | "route-remove" => {
            ui.routes_open.push("env-box".into());
            if open == "route-remove" {
                ui.route_remove = Some(("env-box".into(), 1));
            }
            let alt = |base: &str| env::Alt { base: base.into(), bearer: "token".into() };
            s.ui.env.alternates.insert("env-box".into(), vec![alt("http://192.168.1.20:3773"), alt("https://box.example.com")]);
        }
        "machines" | "update-confirm" => super::machines::scene(s, open),
        "link" => {
            // A second machine with the same name, told apart by its address.
            s.ui.env.saved.push(Saved { id: "env-box-2".into(), label: "Hetzner box".into(), base: "http://10.0.0.7:3773".into(), bearer: String::new() });
            env::park(s, "env-box-2", crate::data::Source::Fixture(crate::data::fixture_state()));
            let ui = &mut s.ui.settings.connections;
            ui.linking = true;
            ui.link_machine_open = true;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(status: &str, expires_at: &str, error: Option<&str>) -> Value {
        json!({"status": status, "expiresAt": expires_at, "lastError": error})
    }

    #[test]
    fn link_health_follows_web_peer_link_health() {
        let at = "2026-11-20T00:00:00.000Z";
        let expires = crate::data::iso_ms(at).unwrap();
        let day = 86_400_000;
        assert_eq!(link_status(&link("reachable", at, None), expires - 30 * day).1, Health::Reachable);
        assert_eq!(
            link_status(&link("reachable", at, None), expires - 2 * day + 1000),
            ("Expires in 2 days. Link it again to renew.".into(), Health::Expiring)
        );
        assert_eq!(link_status(&link("reachable", at, None), expires - day / 2).0, "Expires in 1 day. Link it again to renew.");
        assert_eq!(link_status(&link("reachable", at, None), expires + 1000).1, Health::Failing);
        assert_eq!(link_status(&link("expired", at, None), expires - 30 * day).1, Health::Failing);
        assert_eq!(
            link_status(&link("unreachable", at, None), expires - 30 * day),
            ("Not answering right now.".into(), Health::Unreachable)
        );
        assert_eq!(
            link_status(&link("reachable", at, Some("401 Unauthorized")), expires - 30 * day),
            ("401 Unauthorized".into(), Health::Failing)
        );
    }

    #[test]
    fn linkable_machines_offer_reported_addresses_then_the_saved_route() {
        let config = json!({"directEndpoints": [
            {"httpBaseUrl": "https://box.tail1234.ts.net/"},
            {"httpBaseUrl": "http://192.168.1.20:3773"},
        ]});
        assert_eq!(
            machine_urls(Some(&config), Some("https://box.tail1234.ts.net")),
            ["https://box.tail1234.ts.net", "http://192.168.1.20:3773"]
        );
        assert_eq!(machine_urls(None, Some("http://10.0.0.2:3773")), ["http://10.0.0.2:3773"]);
        // An SSH-only machine has no address another server can reach.
        assert!(machine_urls(None, None).is_empty());
        assert_eq!(mcp_client_scopes("read-only"), ["orchestration:read"]);
        assert_eq!(mcp_client_scopes("auto"), ["orchestration:read", "orchestration:operate"]);
        assert_eq!(mcp_client_scopes("full-access").len(), 4);
    }

    #[test]
    fn overlay_scenes_survive_the_first_poll() {
        let run = |name: &str| {
            let mut s = App::new();
            let scene = crate::settings::SCENES.iter().find(|(n, _)| *n == name).unwrap().1;
            scene(&mut s);
            poll(&mut s);
            s
        };
        assert!(run("settings-connections-add-remote").ui.settings.connections.adding.is_some(), "the scene's saved environment is not a new one");
        let s = run("settings-connections-route-remove");
        assert_eq!(env::routes(&s, "env-box").len(), 3);
        assert_eq!(s.ui.settings.connections.route_remove, Some(("env-box".into(), 1)));
        assert_eq!(run("settings-access-share").ui.settings.connections.page.as_deref(), Some("access"));
    }

    #[test]
    fn the_link_dialog_needs_a_machine_then_a_code_unless_it_can_mint_there() {
        let mut s = App::new();
        s.ui.env.saved.push(Saved { id: "env-box".into(), label: "Box".into(), base: "https://box.ts.net".into(), bearer: String::new() });
        let mut state = crate::data::empty_state();
        state.conn = s5_net::Conn::Open;
        state.shell_ready = true;
        env::park(&mut s, "env-box", crate::data::Source::Fixture(state));
        let all = machines(&s);
        assert_eq!(all.len(), 1);
        assert_eq!((all[0].id.as_str(), all[0].connected, all[0].linked), ("env-box", true, false));
        assert_eq!(all[0].urls, ["https://box.ts.net"]);
        assert!(!link_ready(&s), "a machine must be chosen first");
        s.ui.settings.connections.link_machine = Some("env-box".into());
        // A fixture stands for a session that may do anything, so this client mints there.
        assert!(link_target(&s).2 && link_ready(&s), "this client mints the code there");
        s.ui.settings.connections.mint_failed.push("env-box".into());
        assert!(!link_ready(&s), "after a failed mint the dialog asks for a code");
        s.ui.settings.connections.link_code = TextEdit::new("PAIR-1234");
        assert!(link_ready(&s));
        s.ui.settings.connections.link_machine = Some(ANOTHER_ADDRESS.into());
        s.ui.settings.connections.link_code = TextEdit::new("PAIR-1234");
        assert!(!link_ready(&s), "another address needs the address typed");
        s.ui.settings.connections.link_url = TextEdit::new("https://other.ts.net");
        assert!(link_ready(&s));
        s.ui.settings.results.insert("peerLinks.list".into(), json!({"links": [{"environmentId": "env-box", "status": "reachable"}]}));
        assert!(machines(&s)[0].linked);
    }

    #[test]
    fn the_icon_lock_reads_like_web_resolve_environment_icon_picker_lock() {
        assert_eq!(icon_lock(false, true), Some("Connect to this environment to change its icon."));
        assert_eq!(icon_lock(true, false), Some("Your session on this environment cannot change its settings."));
        assert_eq!(icon_lock(true, true), None);
    }

    #[test]
    fn dragging_a_route_moves_it_like_web_array_move() {
        assert_eq!(moved(vec!['a', 'b', 'c'], 0, 2), ['b', 'c', 'a']);
        assert_eq!(moved(vec!['a', 'b', 'c'], 2, 0), ['c', 'a', 'b']);
        assert_eq!(moved(vec!['a', 'b', 'c'], 1, 9), ['a', 'c', 'b']);
    }

    #[test]
    fn routes_read_like_web_connection_route_label() {
        assert_eq!(route_label("http://127.0.0.1:3773"), "This device");
        assert_eq!(route_label("http://localhost:3773/"), "This device");
        assert_eq!(route_label("https://box.tail1234.ts.net"), "Tailscale");
        assert_eq!(route_label("http://[fd7a:115c:a1e0::1]:3773"), "Tailscale");
        assert_eq!(route_label("http://100.101.2.3:3773"), "VPN");
        assert_eq!(route_label("http://192.168.1.20:3773"), "LAN");
        assert_eq!(route_label("http://devbox:3773"), "LAN");
        assert_eq!(route_label("https://Backend.Example.com./x"), "backend.example.com");
        assert_eq!(mcp_url("https://box.example.com:8443/app?x=1").as_deref(), Some("https://box.example.com:8443/mcp"));
        assert_eq!(mcp_url("not a url"), None);
        assert_eq!(saved_status(true, "Connected"), ("Off".into(), false));
        assert_eq!(saved_status(false, "Connecting..."), ("Connecting".into(), false));
        assert_eq!(saved_status(false, "Reconnecting..."), ("Reconnecting".into(), true));
    }

    #[test]
    fn machine_kind_prefers_a_known_choice_then_detection() {
        assert_eq!(machine_kind(Some("laptop"), Some("desktop")), "laptop");
        assert_eq!(machine_kind(Some("toaster"), Some("desktop")), "desktop");
        assert_eq!(machine_kind(None, None), "server");
        assert_eq!(access_label("approval-required"), "Supervised");
        assert_eq!(access_label("custom"), "custom");
    }
}
