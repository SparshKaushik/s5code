//! Web Settings › Sharing & access (`ConnectionsSettings` view "access").

use super::about::about_row;
use super::connections::{desktop_settings, dot, info_row, machine_icon, machine_kind, reason, take, KINDS};
use super::{controls as c, http, update};
use crate::kit::{self, Menu, MenuEntry, MenuItem, Placement};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, TextEdit, col, el, icon, row, text};
use serde_json::{Value, json};
use std::collections::HashMap;

/// Web `AUTH_SCOPE_OPTIONS`: scope, title, description.
const SCOPES: [(&str, &str, &str); 16] = [
    ("orchestration:read", "View environment", "Read threads, status, checkpoints, and configuration."),
    ("orchestration:operate", "Operate tasks", "Start, update, and stop tasks."),
    ("settings:write", "Change environment settings", "Edit environment preferences and keybindings."),
    ("providers:manage", "Manage providers", "Configure, install, sign in to, and update providers and usage sources."),
    ("environment:maintain", "Maintain environment", "Update the server and control environment processes."),
    ("preview:operate", "Control previews", "Open browser previews and host browser automation."),
    ("diagnostics:read", "View diagnostics and usage", "Read process diagnostics, resource history, and usage totals."),
    ("terminal:read", "View terminals", "Read existing terminal output and status."),
    ("terminal:operate", "Use terminals", "Create terminals and send input to running shells."),
    ("source-control:write", "Change source control", "Commit, push, manage branches and repositories, and change pull requests."),
    ("filesystem:read", "Read files", "Browse host files, search workspaces, and inspect local changes."),
    ("filesystem:write", "Write files", "Edit workspace files and save plans to disk."),
    ("access:read", "View access", "Inspect pairing links and authorized clients."),
    ("access:write", "Manage access", "Issue and revoke credentials for other clients."),
    ("relay:read", "View relay", "Inspect managed relay connectivity."),
    ("relay:write", "Manage relay", "Change managed tunnel connectivity."),
];
const READ_ONLY: [&str; 4] = ["orchestration:read", "filesystem:read", "diagnostics:read", "terminal:read"];
/// `AuthStandardClientScopes` grants every scope.
const STANDARD: [&str; 16] = [
    "orchestration:read",
    "orchestration:operate",
    "settings:write",
    "providers:manage",
    "environment:maintain",
    "preview:operate",
    "diagnostics:read",
    "terminal:read",
    "terminal:operate",
    "source-control:write",
    "filesystem:read",
    "filesystem:write",
    "relay:read",
    "access:read",
    "access:write",
    "relay:write",
];

#[derive(Default)]
pub struct State {
    network: bool,
    tailscale_serve: bool,
    /// The second `s.now_ms` last read the exposure prefs at.
    prefs_read: i64,
    /// `GET /api/auth/session`.
    session: Option<Result<Value, Value>>,
    session_pending: Option<u64>,
    menu: bool,
    icon_menu: bool,
    permissions: bool,
    clients_open: bool,
    create: bool,
    label: TextEdit,
    scopes: Vec<&'static str>,
    creating: Option<u64>,
    create_error: Option<String>,
    revoking: Option<u64>,
    /// Credentials for links this window created; the server never returns them again.
    credentials: HashMap<String, String>,
    newest: Option<String>,
}

pub(super) fn load(s: &mut App) {
    let id = s.src.net().map(|net| net.http_request("GET", "/api/auth/session", json!({})));
    s.ui.settings.connections.access.session_pending = id;
    read_prefs(s);
}

fn read_prefs(s: &mut App) {
    let prefs = desktop_settings();
    let state = &mut s.ui.settings.connections.access;
    state.prefs_read = s.now_ms / 1000;
    state.network = prefs.get::<String>("serverExposureMode").as_deref() == Some("network-accessible");
    state.tailscale_serve = prefs.get::<bool>("tailscaleServeEnabled") == Some(true);
}

pub(super) fn poll(s: &mut App) -> bool {
    if !matches!(&s.route, crate::Route::Settings(p) if p == "access") {
        return false;
    }
    let mut changed = false;
    let state = &s.ui.settings.connections.access;
    let (session, creating, revoking) = (state.session_pending, state.creating, state.revoking);
    let stale = state.prefs_read != s.now_ms / 1000;
    if let Some(reply) = take(s, session) {
        let state = &mut s.ui.settings.connections.access;
        state.session_pending = None;
        state.session = Some(reply);
        changed = true;
    }
    if let Some(reply) = take(s, creating) {
        let state = &mut s.ui.settings.connections.access;
        state.creating = None;
        match reply {
            Ok(created) => {
                if let (Some(id), Some(credential)) = (created["id"].as_str(), created["credential"].as_str()) {
                    state.credentials.insert(id.to_owned(), credential.to_owned());
                    state.newest = Some(id.to_owned());
                }
                state.create = false;
                state.label.clear();
                state.create_error = None;
            }
            Err(error) => state.create_error = Some(reason(&error)),
        }
        changed = true;
    }
    if let Some(reply) = take(s, revoking) {
        s.ui.settings.connections.access.revoking = None;
        if let Err(error) = reply {
            s.ui.settings.error = Some(reason(&error));
        }
        changed = true;
    }
    if stale {
        let before = (s.ui.settings.connections.access.network, s.ui.settings.connections.access.tailscale_serve);
        read_prefs(s);
        let state = &s.ui.settings.connections.access;
        changed |= before != (state.network, state.tailscale_serve);
    }
    changed
}

pub(super) fn escape(s: &mut App) -> bool {
    let state = &mut s.ui.settings.connections.access;
    if state.menu {
        state.menu = false;
        state.icon_menu = false;
        return true;
    }
    std::mem::take(&mut state.create)
}

pub(super) fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    machine_menu(s, t, layers);
    create_dialog(s, t, layers);
}

/// Web `formatExpiresInLabel`.
fn expires_in(expires_ms: i64, now_ms: i64) -> String {
    let diff = expires_ms - now_ms;
    if diff <= 0 {
        return "Expired".into();
    }
    let total = diff / 1000;
    if total < 5 {
        return "Expires in a moment".into();
    }
    if total < 60 {
        return format!("Expires in {total}s");
    }
    let parts: Vec<String> = [(total / 86_400, 'd'), (total % 86_400 / 3600, 'h'), (total % 3600 / 60, 'm'), (total % 60, 's')]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, unit)| format!("{n}{unit}"))
        .collect();
    format!("Expires in {}", parts.join(" "))
}

/// Web `summarizeAuthorizedClients`.
fn summarize(clients: usize, links: usize) -> String {
    let mut summary = format!("{clients} {}", if clients == 1 { "client" } else { "clients" });
    if links > 0 {
        summary += &format!(" · {links} {}", if links == 1 { "pairing link" } else { "pairing links" });
    }
    summary
}

/// Web `togglePairingScopeSelection`: View terminals and Use terminals travel together.
fn toggle_scope(current: &[&'static str], scope: &'static str, checked: bool) -> Vec<&'static str> {
    let mut next: Vec<&'static str> = current.iter().copied().filter(|s| *s != scope).collect();
    if !checked {
        if scope == "terminal:read" {
            next.retain(|s| *s != "terminal:operate");
        }
        return next;
    }
    next.push(scope);
    if scope == "terminal:operate" && !next.contains(&"terminal:read") {
        next.push("terminal:read");
    }
    next
}

use super::permissions::grants;

fn delegatable(session: Option<&Result<Value, Value>>) -> Vec<&'static str> {
    match session {
        Some(Ok(session)) => SCOPES.iter().map(|(scope, ..)| *scope).filter(|scope| grants(session, scope)).collect(),
        _ => Vec::new(),
    }
}

fn selected(scopes: &[&'static str], delegatable: &[&'static str]) -> Vec<&'static str> {
    SCOPES
        .iter()
        .map(|(scope, ..)| *scope)
        .filter(|scope| scopes.contains(scope) && delegatable.contains(scope))
        .collect()
}

/// Shared `sessionPermissionsUnavailableMessage`.
fn unavailable(connected: bool, failed: bool, rejected: bool) -> &'static str {
    if !connected {
        "Connect to this environment to view this session’s permissions."
    } else if rejected {
        "This environment did not accept your session. Reconnect to try again."
    } else if failed {
        "Could not verify your permissions. Retrying automatically."
    } else {
        "Checking permissions…"
    }
}

/// Web `missingTailscaleEndpointDescription`; Rune never probes Tailscale, so no endpoint is known.
fn tailscale_description(network: bool, serve: bool) -> &'static str {
    if !network && !serve {
        "Turn on Network access to detect Tailscale."
    } else {
        "Start Tailscale to set up HTTPS access through MagicDNS."
    }
}

/// Web `sortDesktopClientSessions`: this session, then connected ones, then newest.
fn sort_clients(clients: &mut [&Value]) {
    clients.sort_by(|a, b| {
        (b["current"] == true)
            .cmp(&(a["current"] == true))
            .then((b["connected"] == true).cmp(&(a["connected"] == true)))
            .then(crate::data::ts(b, "issuedAt").cmp(&crate::data::ts(a, "issuedAt")))
    });
}

/// Web `resolveEnvironmentMachineKind`, and the detected kind the icon picker clears back to.
fn kinds(s: &App) -> (&str, &str, bool) {
    let detected = s.state().config.as_ref().and_then(|c| c["environment"]["platform"]["machine"].as_str());
    let resolved = machine_kind(super::server_settings(s)["environmentIcon"].as_str(), detected);
    (resolved, detected.unwrap_or("server"), detected.is_some())
}

/// The `environmentIcon` setting for a picked kind: unset when it is what the server detects.
fn icon_setting(kind: &str, detected: &str) -> Value {
    if kind == detected { Value::Null } else { json!(kind) }
}

fn display_url(s: &App) -> Option<&str> {
    match &s.src {
        crate::data::Source::Live(net) => Some(net.base_url()),
        crate::data::Source::Fixture(_) => s.ui.desktop.server.as_ref().map(|(url, _)| url.as_str()),
    }
}

pub(super) fn view(s: &App, t: &T) -> El {
    let config = s.state().config.as_ref();
    let label = config.and_then(|c| c["environment"]["label"].as_str()).unwrap_or("This machine");
    let (kind, ..) = kinds(s);
    let header = row()
        .w_full()
        .h(28.0)
        .px(16.0)
        .gap(8.0)
        .center_y()
        .child(icon(machine_icon(kind), 16.0).color(t.c.foreground))
        .child(
            text(label.to_owned())
                .font(theme::weight(theme::TEXT_BASE, theme::SEMIBOLD))
                .color(t.c.foreground)
                .truncate()
                .grow(1.0),
        )
        .child(
            row().key("access-machine-more").child(
                kit::icon_button(&icons::ELLIPSIS, "More actions for this machine")
                    .variant("ghost-muted")
                    .size("icon-xs")
                    .on_press(|s: &mut App| s.ui.settings.connections.access.menu = true)
                    .build(t),
            ),
        );
    let version = [config.and_then(|c| c["environment"]["serverVersion"].as_str()), display_url(s)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let status = if config.is_some() {
        text("Up to date").font(theme::TEXT_XS).color(t.c.muted_foreground)
    } else {
        el()
    };
    let mut rows = vec![about_row(t, "Version", if version.is_empty() { "Loading…" } else { version.as_str() }, status)];
    rows.extend(crate::desktop::exposure::rows(s, t));
    rows.push(permissions(s, t));
    col()
        .w_full()
        .gap(32.0)
        .child(col().w_full().gap(10.0).child(header).child(c::card(rows, t)))
        .child(clients(s, t))
}

/// Web's `<details>` "Permissions" with `SessionPermissions`.
fn permissions(s: &App, t: &T) -> El {
    let state = &s.ui.settings.connections.access;
    let open = state.permissions;
    let summary = row()
        .w_full()
        .min_h(44.0)
        .gap(8.0)
        .center_y()
        .focusable(true)
        .role("button")
        .label("Permissions")
        .expanded(open)
        .child(text("Permissions").font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(
            icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 12.0).color(t.c.muted_foreground),
        )
        .on_press(|s: &mut App| {
            let state = &mut s.ui.settings.connections.access;
            state.permissions = !state.permissions;
        });
    let mut details = col().w_full().px(16.0).child(summary);
    if !open {
        return details;
    }
    let connected = matches!(s.state().conn, s5_net::Conn::Open);
    let session = state.session.as_ref();
    details = details.child(match session {
        Some(Ok(session)) if connected && session["authenticated"] == true => {
            let mut list = col().w_full().gap(8.0);
            for pair in SCOPES.chunks(2) {
                list = list.child(row().w_full().gap(24.0).children(pair.iter().map(|(scope, title, _)| {
                    row()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(12.0)
                        .items_start()
                        .child(text(*title).font(theme::TEXT_XS).color(t.c.foreground).wrap().grow(1.0))
                        .child(
                            text(if grants(session, scope) { "Allowed" } else { "Not granted" })
                                .font(theme::TEXT_XS)
                                .color(t.c.muted_foreground),
                        )
                })));
            }
            col()
                .w_full()
                .py(12.0)
                .gap(12.0)
                .child(
                    text("These permissions apply to this client’s current connection.")
                        .font(theme::TEXT_XS)
                        .color(t.c.muted_foreground)
                        .wrap(),
                )
                .child(list)
        }
        _ => {
            let failed = matches!(session, Some(Err(_)));
            let rejected = matches!(session, Some(Ok(v)) if v["authenticated"] == false);
            text(unavailable(connected, failed, rejected)).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().py(12.0)
        }
    });
    details
}

/// Web `FoldedSettingsSection` "Authorized clients".
fn clients(s: &App, t: &T) -> El {
    let state = &s.ui.settings.connections.access;
    let snapshot = super::resource(s, "subscribeAuthAccess", json!({}));
    let now = s.now_ms;
    let mut links: Vec<&Value> = snapshot
        .into_iter()
        .filter_map(|v| v["payload"]["pairingLinks"].as_array())
        .flatten()
        .filter(|link| crate::data::ts(link, "expiresAt").is_none_or(|expires| expires > now))
        .collect();
    links.sort_by_key(|link| std::cmp::Reverse(crate::data::ts(link, "createdAt")));
    let mut sessions: Vec<&Value> =
        snapshot.into_iter().filter_map(|v| v["payload"]["clientSessions"].as_array()).flatten().collect();
    sort_clients(&mut sessions);
    let open = state.clients_open;
    let revoking = state.revoking.is_some();
    let others = snapshot.is_none() || sessions.iter().any(|session| session["current"] != true);
    let header = row()
        .w_full()
        .px(16.0)
        .gap(16.0)
        .center_y()
        .child(
            row()
                .grow(1.0)
                .min_w(0.0)
                .min_h(44.0)
                .py(8.0)
                .gap(8.0)
                .center_y()
                .focusable(true)
                .role("button")
                .label("Authorized clients")
                .expanded(open)
                .child(
                    icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 16.0)
                        .color(t.c.muted_foreground),
                )
                .child(text("Authorized clients").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(
                    text(summarize(sessions.len(), links.len()))
                        .font(theme::TEXT_XS)
                        .color(t.c.muted_foreground)
                        .truncate()
                        .shrink()
                        .min_w(0.0),
                )
                .on_press(|s: &mut App| {
                    let state = &mut s.ui.settings.connections.access;
                    state.clients_open = !state.clients_open;
                }),
        )
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(
                    kit::Button::new(if revoking { "Revoking…" } else { "Revoke others" })
                        .variant("destructive-outline")
                        .size("xs")
                        .disabled(revoking || !others)
                        .on_press(revoke_others)
                        .build(t),
                )
                .child(kit::Button::new("Create link").size("xs").icon(&icons::PLUS).on_press(open_create).build(t)),
        );
    let mut card = vec![header];
    if !open {
        let newest = state.newest.as_deref();
        if let Some(link) = links.iter().find(|link| newest.is_some() && link["id"].as_str() == newest) {
            card.push(link_row(s, t, link));
        }
        return c::card(card, t);
    }
    let mut rows: Vec<El> = links.iter().map(|link| link_row(s, t, link)).collect();
    rows.extend(sessions.iter().map(|session| client_row(t, session)));
    if rows.is_empty() {
        card.push(
            text("No pairing links or client sessions.")
                .font(theme::TEXT_XS)
                .color(t.c.muted_foreground)
                .px(16.0)
                .py(12.0),
        );
        return c::card(card, t);
    }
    let mut list = col().w_full();
    for (i, entry) in rows.into_iter().enumerate() {
        if i > 0 {
            list = list.child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.5)));
        }
        list = list.child(entry);
    }
    card.push(kit::scroll_area(
        "authorized-clients",
        col().w_full().max_h(360.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(list),
        t,
    ));
    c::card(card, t)
}

/// Web `PairingLinkListRow`, without the share panel.
fn link_row(s: &App, t: &T, link: &Value) -> El {
    let id = link["id"].as_str().unwrap_or_default().to_owned();
    let credential = s.ui.settings.connections.access.credentials.get(&id).cloned();
    let scopes = link["permissions"].as_array().or(link["scopes"].as_array()).map_or(0, Vec::len);
    let expires = crate::data::ts(link, "expiresAt").map(|e| expires_in(e, s.now_ms)).unwrap_or_default();
    let mut copy = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(
            row()
                .min_h(20.0)
                .gap(6.0)
                .center_y()
                .child(dot(t.c.warning))
                .child(
                    text(link["label"].as_str().unwrap_or("Pairing link").to_owned())
                        .font(theme::SM_MEDIUM)
                        .color(t.c.foreground),
                ),
        )
        .child(
            text(format!("{expires} · {scopes} {}", if scopes == 1 { "scope" } else { "scopes" }))
                .font(theme::TEXT_XS)
                .color(t.c.muted_foreground),
        );
    if credential.is_none() {
        copy = copy.child(
            text("Create a new link to share from this client.")
                .font(theme::TEXT_2XS)
                .color(t.c.muted_foreground.alpha_mul(0.7)),
        );
    }
    let mut actions = row().gap(8.0).center_y();
    if let Some(code) = credential {
        actions = actions.child(
            kit::Button::new("Copy code")
                .variant("outline")
                .size("xs")
                .build(t)
                .on_press_tx(move |_: &mut App, tx| tx.copy(code.clone())),
        );
    }
    actions = actions.child(
        kit::Button::new("Revoke")
            .variant("destructive-outline")
            .size("xs")
            .on_press(move |s: &mut App| http(s, "POST", "/api/auth/pairing-links/revoke", json!({"id": id})))
            .build(t),
    );
    row().w_full().px(16.0).py(12.0).gap(12.0).center_y().child(copy).child(actions)
}

/// Web `ConnectedClientListRow`.
fn client_row(t: &T, client: &Value) -> El {
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
    let bits: Vec<String> = [
        device,
        info["os"].as_str().map(str::to_owned),
        info["browser"].as_str().map(str::to_owned),
        info["ipAddress"].as_str().map(str::to_owned),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut heading = row()
        .min_h(20.0)
        .gap(6.0)
        .center_y()
        .child(dot(if live { t.c.success } else { t.c.muted_foreground.alpha_mul(0.3) }))
        .child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground));
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
    let detail = if !bits.is_empty() {
        bits.join(" · ")
    } else if live {
        "Connected".to_owned()
    } else {
        "Not connected".to_owned()
    };
    let mut entry = row().w_full().px(16.0).py(12.0).gap(12.0).center_y().child(
        col()
            .grow(1.0)
            .min_w(0.0)
            .gap(4.0)
            .child(heading)
            .child(text(detail).font(theme::TEXT_XS).color(t.c.muted_foreground)),
    );
    if !current {
        entry = entry.child(
            kit::Button::new("Revoke")
                .variant("destructive-outline")
                .size("xs")
                .on_press(move |s: &mut App| http(s, "POST", "/api/auth/clients/revoke", json!({"sessionId": id})))
                .build(t),
        );
    }
    entry
}

fn revoke_others(s: &mut App) {
    if s.ui.settings.connections.access.revoking.is_some() {
        return;
    }
    let id = s.src.net().map(|net| net.http_request("POST", "/api/auth/clients/revoke-others", json!({})));
    s.ui.settings.connections.access.revoking = id;
}

fn open_create(s: &mut App) {
    let state = &mut s.ui.settings.connections.access;
    state.create = true;
    state.scopes = STANDARD.to_vec();
    state.label.clear();
    state.create_error = None;
}

fn close_create(s: &mut App) {
    s.ui.settings.connections.access.create = false;
}

fn set_scopes(s: &mut App, scopes: &[&'static str]) {
    let state = &mut s.ui.settings.connections.access;
    if state.creating.is_none() {
        state.scopes = scopes.to_vec();
    }
}

fn create(s: &mut App) {
    let state = &s.ui.settings.connections.access;
    let scopes = selected(&state.scopes, &delegatable(state.session.as_ref()));
    if state.creating.is_some() || scopes.is_empty() {
        return;
    }
    let mut payload = json!({"scopes": scopes});
    let label = state.label.text.trim();
    if !label.is_empty() {
        payload["label"] = json!(label);
    }
    let id = s.src.net().map(|net| net.http_request("POST", "/api/auth/pairing-token", payload));
    let state = &mut s.ui.settings.connections.access;
    state.create_error = None;
    state.creating = id;
}

/// Web `AuthorizedClientsHeaderAction`'s dialog.
fn create_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let state = &s.ui.settings.connections.access;
    if !state.create {
        return;
    }
    let creating = state.creating.is_some();
    let delegatable = delegatable(state.session.as_ref());
    let chosen = selected(&state.scopes, &delegatable);
    let k = kit::theme(t);
    let mut options = col().w_full();
    for (i, (scope, title, description)) in SCOPES.into_iter().filter(|(scope, ..)| delegatable.contains(scope)).enumerate() {
        if i > 0 {
            options = options.child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.6)));
        }
        let checked = state.scopes.contains(&scope);
        options = options.child(
            row()
                .w_full()
                .px(12.0)
                .py(10.0)
                .gap(12.0)
                .items_start()
                .hover(t.c.muted.alpha_mul(0.4))
                .child(kit::Checkbox::new(checked).hide_label().label(title).disabled(creating).build(&k).mt(2.0))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .child(text(title).font(theme::XS_MEDIUM).color(t.c.foreground))
                        .child(text(description).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()),
                )
                .on_press(move |s: &mut App| {
                    let state = &mut s.ui.settings.connections.access;
                    if state.creating.is_none() {
                        state.scopes = toggle_scope(&state.scopes, scope, !checked);
                    }
                }),
        );
    }
    let mut permissions = col()
        .w_full()
        .gap(12.0)
        .child(
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .child(text("Permissions").font(theme::XS_MEDIUM).color(t.c.foreground))
                        .child(text("Limit what the paired client can do.").font(theme::TEXT_XS).color(t.c.muted_foreground)),
                )
                .child(
                    row()
                        .gap(4.0)
                        .child(
                            kit::Button::new("Read only")
                                .variant("outline")
                                .size("xs")
                                .disabled(creating)
                                .on_press(|s: &mut App| set_scopes(s, &READ_ONLY))
                                .build(t),
                        )
                        .child(
                            kit::Button::new("Standard")
                                .variant("outline")
                                .size("xs")
                                .disabled(creating)
                                .on_press(|s: &mut App| set_scopes(s, &STANDARD))
                                .build(t),
                        ),
                ),
        )
        .child(kit::scroll_area(
            "pairing-scopes",
            col()
                .w_full()
                .max_h(280.0)
                .radius(8.0)
                .border(1.0, t.c.input)
                .fill(t.c.muted.alpha_mul(0.25))
                .scroll(rune::ScrollSpec { stick_bottom: false })
                .child(options),
            t,
        ));
    if chosen.is_empty() {
        permissions =
            permissions.child(text("Select at least one permission.").font(theme::TEXT_XS).color(t.c.destructive));
    } else if chosen.contains(&"access:write") {
        permissions = permissions.child(
            text("This client can create or revoke access for other devices.").font(theme::TEXT_XS).color(t.c.warning),
        );
    }
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(
            col().w_full().gap(6.0).child(text("Client label (optional)").font(theme::XS_MEDIUM).color(t.c.foreground)).child(
                kit::Input::new(&state.label)
                    .placeholder("e.g. Living room iPad")
                    .disabled(creating)
                    .on_edit(|s: &mut App| &mut s.ui.settings.connections.access.label)
                    .on_submit(|s: &mut App, _| create(s))
                    .build(t)
                    .single_line(),
            ),
        )
        .child(permissions);
    if let Some(error) = &state.create_error {
        body = body.child(text(format!("Could not create pairing URL. {error}")).font(theme::TEXT_XS).color(t.c.destructive).wrap());
    }
    kit::Dialog::new("create-pairing-link", "Create pairing link")
        .open(true)
        .description("Generate a one-time link that another device can use to pair with this backend as an authorized client.")
        .width(448.0)
        .on_close(close_create)
        .child(body)
        .child(
            row()
                .gap(8.0)
                .child(kit::Button::new("Cancel").variant("outline").disabled(creating).on_press(close_create).build(t))
                .child(
                    kit::Button::new(if creating { "Creating…" } else { "Create link" })
                        .disabled(creating || chosen.is_empty())
                        .on_press(create)
                        .build(t),
                ),
        )
        .build(t, layers);
}

/// Web `EnvironmentIconMenu` in the machine's "⋯" menu.
fn machine_menu(s: &App, t: &T, layers: &mut Vec<El>) {
    let state = &s.ui.settings.connections.access;
    if !state.menu {
        return;
    }
    let (resolved, detected, known) = kinds(s);
    let close = |s: &mut App| {
        let state = &mut s.ui.settings.connections.access;
        state.menu = false;
        state.icon_menu = false;
    };
    let entries = KINDS
        .into_iter()
        .map(|(kind, label)| {
            let detected = detected.to_owned();
            let mut item = MenuItem::new(label).icon(machine_icon(kind)).radio(kind == resolved);
            if kind == detected {
                item = item.trailing(
                    text(if known { "detected" } else { "default" }).font(theme::TEXT_XS).color(t.c.muted_foreground),
                );
            }
            MenuEntry::item(item.on_select(move |s: &mut App| {
                update(s, "environmentIcon", icon_setting(kind, &detected));
                close(s);
            }))
        })
        .collect();
    Menu::new("access-machine-menu")
        .anchor("access-machine-more")
        .width(200.0)
        .placement(Placement::BottomEnd)
        .gutter(4.0)
        .open(true)
        .on_close(close)
        .item(
            MenuItem::new("Icon")
                .icon(machine_icon(resolved))
                .submenu(entries, state.icon_menu)
                .submenu_min_width(200.0)
                .on_submenu_change(|s: &mut App, open| s.ui.settings.connections.access.icon_menu = open),
        )
        .overlay(&kit::theme(t), layers);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_labels_match_web_format_expires_in_label() {
        let now = 1_000_000;
        for (seconds, label) in [
            (0, "Expired"),
            (3, "Expires in a moment"),
            (42, "Expires in 42s"),
            (300, "Expires in 5m"),
            (303, "Expires in 5m 3s"),
            (7200, "Expires in 2h"),
            (7265, "Expires in 2h 1m 5s"),
            (86_400, "Expires in 1d"),
            (86_401, "Expires in 1d 1s"),
            (93_600, "Expires in 1d 2h"),
        ] {
            assert_eq!(expires_in(now + seconds * 1000, now), label, "{seconds}s");
        }
    }

    #[test]
    fn summary_counts_clients_and_pairing_links() {
        assert_eq!(summarize(1, 0), "1 client");
        assert_eq!(summarize(2, 1), "2 clients · 1 pairing link");
        assert_eq!(summarize(0, 3), "0 clients · 3 pairing links");
    }

    #[test]
    fn terminal_scopes_travel_together() {
        let all = ["terminal:read", "terminal:operate", "files:read"];
        assert_eq!(toggle_scope(&all, "terminal:read", false), ["files:read"]);
        assert_eq!(toggle_scope(&["files:read"], "terminal:operate", true), ["files:read", "terminal:operate", "terminal:read"]);
        assert_eq!(toggle_scope(&all, "terminal:operate", false), ["terminal:read", "files:read"]);
    }

    #[test]
    fn session_grants_follow_permissions_then_scopes_then_legacy_parents() {
        let session = json!({"authenticated": true, "permissions": ["terminal:read"], "scopes": ["orchestration:read"]});
        assert!(grants(&session, "terminal:read"));
        assert!(!grants(&session, "orchestration:read"), "permissions win over scopes");
        let legacy = json!({"authenticated": true, "scopes": ["orchestration:operate"], "auth": {}});
        assert!(grants(&legacy, "filesystem:write"));
        assert!(!grants(&legacy, "filesystem:read"));
        let modern = json!({"authenticated": true, "scopes": ["orchestration:operate"], "auth": {"serverUpdateScope": "environment:maintain"}});
        assert!(!grants(&modern, "filesystem:write"));
        assert!(!grants(&json!({"authenticated": false, "scopes": ["orchestration:read"]}), "orchestration:read"));
        let picked = selected(&STANDARD, &delegatable(Some(&Ok(session))));
        assert_eq!(picked, ["terminal:read"]);
    }

    #[test]
    fn clients_sort_current_then_connected_then_newest() {
        let rows = [
            json!({"sessionId": "old", "current": false, "connected": false, "issuedAt": "2026-01-01T00:00:00.000Z"}),
            json!({"sessionId": "new", "current": false, "connected": false, "issuedAt": "2026-02-01T00:00:00.000Z"}),
            json!({"sessionId": "live", "current": false, "connected": true, "issuedAt": "2025-01-01T00:00:00.000Z"}),
            json!({"sessionId": "me", "current": true, "connected": true, "issuedAt": "2024-01-01T00:00:00.000Z"}),
        ];
        let mut sorted: Vec<&Value> = rows.iter().collect();
        sort_clients(&mut sorted);
        let ids: Vec<&str> = sorted.iter().map(|v| v["sessionId"].as_str().unwrap()).collect();
        assert_eq!(ids, ["me", "live", "new", "old"]);
    }

    #[test]
    fn icon_and_tailscale_copy() {
        assert_eq!(icon_setting("laptop", "laptop"), Value::Null);
        assert_eq!(icon_setting("cloud", "server"), json!("cloud"));
        assert_eq!(tailscale_description(false, false), "Turn on Network access to detect Tailscale.");
        assert_eq!(tailscale_description(true, false), "Start Tailscale to set up HTTPS access through MagicDNS.");
        assert_eq!(unavailable(true, true, false), "Could not verify your permissions. Retrying automatically.");
    }
}
