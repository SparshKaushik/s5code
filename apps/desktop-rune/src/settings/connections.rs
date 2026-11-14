use super::{controls as c, request};
use crate::env::{self, Saved};
use crate::kit::{self, Menu, MenuItem, Placement};
use crate::theme::{self, AlphaMul, T};
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
    page: Option<String>,
    local_disabled: bool,
    local_confirm: bool,
    local_error: Option<String>,
    /// The saved environments when Add environment opened; any change means the pairing landed.
    adding: Option<Vec<Saved>>,
    env_menu: Option<String>,
    remove: Option<(String, String)>,
    linking: bool,
    link_url: TextEdit,
    link_code: TextEdit,
    link_access: Option<&'static str>,
    link_pending: Option<u64>,
    link_error: Option<String>,
    forget: Option<(String, String)>,
    pub(super) access: super::access::State,
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
        match page.as_deref() {
            Some("connections") => {
                s.ui.settings.connections.local_disabled = desktop_settings().get::<bool>(LOCAL_KEY) == Some(false);
            }
            Some("access") => super::access::load(s),
            _ => {}
        }
        changed = true;
    }
    let ui = &mut s.ui.settings.connections;
    if matches!(&ui.adding, Some(before) if *before != s.ui.env.saved) {
        ui.adding = None;
        changed = true;
    }
    let pending = ui.link_pending;
    if let Some(reply) = take(s, pending) {
        s.ui.settings.connections.link_pending = None;
        match reply {
            Ok(_) => {
                close_link(s);
                request(s, "peerLinks.list", json!({}));
            }
            Err(error) => s.ui.settings.connections.link_error = Some(reason(&error)),
        }
        changed = true;
    }
    changed | super::access::poll(s)
}

pub fn escape(s: &mut App) -> bool {
    let ui = &mut s.ui.settings.connections;
    let closed = ui.env_menu.take().is_some()
        || ui.remove.take().is_some()
        || ui.forget.take().is_some()
        || std::mem::take(&mut ui.local_confirm)
        || ui.adding.take().is_some()
        || std::mem::take(&mut ui.linking);
    closed || super::access::escape(s)
}

pub fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    match s.ui.settings.connections.page.as_deref() {
        Some("connections") => {
            local_confirm(s, t, layers);
            add_dialog(s, t, layers);
            env_menu(s, t, layers);
            remove_dialog(s, t, layers);
            link_dialog(s, t, layers);
            forget_dialog(s, t, layers);
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
            s.ui.settings.connections.adding = Some(s.ui.env.saved.clone());
        })
        .build(t);
    let rows = if s.ui.env.saved.is_empty() {
        vec![empty(t)]
    } else {
        s.ui.env.saved.iter().map(|env| saved_row(s, t, env)).collect()
    };
    page = page.child(c::section_with_actions("Environments", vec![add], rows, t));
    page = page.child(c::section("SSH", vec![env::ssh::form(s, t)], t));
    if offers_links(s) {
        page = page.child(linked(s, t));
    }
    page
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

fn saved_row(s: &App, t: &T, saved: &Saved) -> El {
    let config = env::source(s, &saved.id).and_then(|src| src.state().config.as_ref());
    let kind = machine_kind(
        config.and_then(|c| c["settings"]["environmentIcon"].as_str()),
        config.and_then(|c| c["environment"]["platform"]["machine"].as_str()),
    );
    let id = saved.id.clone();
    row()
        .key(format!("saved-env-{}", saved.id))
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(12.0)
        .center_y()
        .child(icon(machine_icon(kind), 16.0).color(t.c.muted_foreground))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(text(saved.label.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate())
                .child({
                    // SavedEnvironmentRows: a short status, red while failing, the full text on hover.
                    let (status, failing) = env::row_status(s, &saved.id);
                    text(env::ssh::detail(s, &saved.id, &status).unwrap_or_else(|| format!("{} · {status}", saved.base)))
                        .font(theme::TEXT_XS)
                        .color(if failing { t.c.destructive_foreground } else { t.c.muted_foreground })
                        .truncate()
                        .help(env::status(s, &saved.id))
                }),
        )
        .children(env::ssh::row_action(s, t, &saved.id))
        .child(
            row().key(format!("saved-env-more-{}", saved.id)).child(
                kit::icon_button(&icons::ELLIPSIS, "More actions")
                    .variant("ghost-muted")
                    .size("icon-xs")
                    .on_press(move |s: &mut App| s.ui.settings.connections.env_menu = Some(id.clone()))
                    .build(t),
            ),
        )
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
    let Some(id) = &s.ui.settings.connections.env_menu else { return };
    let Some(saved) = s.ui.env.saved.iter().find(|e| &e.id == id) else { return };
    let target = (saved.id.clone(), saved.label.clone());
    let close = |s: &mut App| s.ui.settings.connections.env_menu = None;
    Menu::new("saved-env-menu")
        .anchor(format!("saved-env-more-{}", saved.id))
        .width(224.0)
        .placement(Placement::BottomEnd)
        .gutter(4.0)
        .open(true)
        .on_close(close)
        .item(MenuItem::new("Remove from this device…").variant("destructive").on_select(move |s: &mut App| {
            s.ui.settings.connections.env_menu = None;
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

/// Web `AddEnvironmentDialog`, remote link mode; pairing runs through `env`.
fn add_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    if s.ui.settings.connections.adding.is_none() {
        return;
    }
    let card = row()
        .grow(1.0)
        .min_w(0.0)
        .min_h(96.0)
        .padding(16.0)
        .gap(12.0)
        .items_start()
        .radius(8.0)
        .border(1.0, t.c.primary.alpha_mul(0.5))
        .fill(t.c.primary.alpha_mul(0.05))
        .child(
            el().size(32.0, 32.0)
                .radius(6.0)
                .center()
                .fill(t.c.primary.alpha_mul(0.1))
                .child(icon(&icons::CHEVRONS_LEFT_RIGHT_ELLIPSIS, 16.0).color(t.c.primary)),
        )
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(text("Remote link").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(
                    text("Enter a backend host and pairing code.")
                        .font(theme::TEXT_XS.lh(18.0))
                        .color(t.c.muted_foreground)
                        .wrap(),
                ),
        );
    kit::Dialog::new("add-environment", "Add Environment")
        .open(true)
        .description("Pair another environment to this client.")
        .width(768.0)
        .on_close(|s: &mut App| s.ui.settings.connections.adding = None)
        .child(row().w_full().gap(12.0).child(card).child(el().grow(1.0)))
        .child(remote_fields(s, t))
        .build(t, layers);
}

/// Web `renderRemoteModeBody` without the T3 Connect route offer.
fn remote_fields(s: &App, t: &T) -> El {
    let env = &s.ui.env;
    let pairing = env::pairing(s);
    let field = |label: &str, input: El| col().gap(6.0).child(text(label.to_owned()).font(theme::XS_MEDIUM).color(t.c.foreground)).child(input);
    let host = kit::Input::new(&env.host).placeholder("backend.example.com").on_edit(|s: &mut App| &mut s.ui.env.host).on_submit(|s: &mut App, _| env::add(s)).build(t).single_line();
    let code = kit::Input::new(&env.code).placeholder("PAIRCODE").on_edit(|s: &mut App| &mut s.ui.env.code).on_submit(|s: &mut App, _| env::add(s)).build(t).single_line();
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(
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
        kit::Button::new(if pairing { "Adding…" } else { "Add environment" })
            .variant("outline")
            .icon(&icons::PLUS)
            .disabled(pairing)
            .on_press(env::add)
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
        .on_press(|s: &mut App| s.ui.settings.connections.linking = true)
        .build(t);
    c::section_with_actions("Linked environments", vec![action], rows, t)
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
        .child(
            kit::Button::new("Forget")
                .variant("destructive-outline")
                .size("xs")
                .on_press(move |s: &mut App| s.ui.settings.connections.forget = Some(target.clone()))
                .build(t),
        )
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

fn close_link(s: &mut App) {
    let ui = &mut s.ui.settings.connections;
    ui.linking = false;
    ui.link_url.clear();
    ui.link_code.clear();
    ui.link_access = None;
    ui.link_error = None;
}

fn link(s: &mut App) {
    let ui = &s.ui.settings.connections;
    let (url, code) = (ui.link_url.text.trim().to_owned(), ui.link_code.text.trim().to_owned());
    if ui.link_pending.is_some() || url.is_empty() || code.is_empty() {
        return;
    }
    let access = ui.link_access.unwrap_or("approval-required");
    let id = s
        .src
        .net()
        .map(|net| net.request("peerLinks.link", json!({"url": url, "pairingCode": code, "access": access})));
    let ui = &mut s.ui.settings.connections;
    ui.link_error = None;
    ui.link_pending = id;
}

/// Web `LinkEnvironmentAction` for an address and pairing code.
fn link_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let ui = &s.ui.settings.connections;
    if !ui.linking {
        return;
    }
    let busy = ui.link_pending.is_some();
    let ready = !ui.link_url.text.trim().is_empty() && !ui.link_code.text.trim().is_empty();
    let current = ui.link_access.unwrap_or("approval-required");
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
                    if s.ui.settings.connections.link_pending.is_none() {
                        s.ui.settings.connections.link_access = Some(key);
                    }
                }),
        );
    }
    let mut form = col()
        .w_full()
        .gap(16.0)
        .child(field(
            "Address",
            kit::Input::new(&ui.link_url)
                .placeholder("https://box.example.ts.net")
                .disabled(busy)
                .on_edit(|s: &mut App| &mut s.ui.settings.connections.link_url)
                .on_submit(|s: &mut App, _| link(s))
                .build(t)
                .single_line(),
        ))
        .child(field(
            "Pairing code",
            kit::Input::new(&ui.link_code)
                .placeholder("From that environment's Connections, or t3 pair there")
                .disabled(busy)
                .on_edit(|s: &mut App| &mut s.ui.settings.connections.link_code)
                .on_submit(|s: &mut App, _| link(s))
                .build(t)
                .single_line(),
        ))
        .child(access);
    if let Some(error) = &ui.link_error {
        form = form.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap());
    }
    kit::Dialog::new("link-environment", "Link an environment")
        .open(true)
        .description("Agents here can then launch, message and wait on threads there. It appears in that environment's Connections, where it can be revoked.")
        .width(448.0)
        .on_close(close_link)
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
    fn machine_kind_prefers_a_known_choice_then_detection() {
        assert_eq!(machine_kind(Some("laptop"), Some("desktop")), "laptop");
        assert_eq!(machine_kind(Some("toaster"), Some("desktop")), "desktop");
        assert_eq!(machine_kind(None, None), "server");
        assert_eq!(access_label("approval-required"), "Supervised");
        assert_eq!(access_label("custom"), "custom");
    }
}
