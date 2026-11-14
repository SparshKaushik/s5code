mod about;
mod access;
pub(crate) use access::grants as session_grants;
mod accent;
mod acp_icons;
mod add_provider;
mod provider_update;
pub(crate) mod auth;
mod auth_terminal;
mod background;
mod balance;
pub mod client;
mod connections;
mod controls;
mod devices;
mod devices_editor;
mod diff_scheme;
pub mod fonts;
mod keybindings;
mod models;
mod host_logos;
mod icon_index;
mod pages;
pub(crate) mod permissions;
mod qr;
pub(crate) mod telemetry;
pub(crate) mod project_icon;
mod projects;
mod providers;
mod scope;
pub mod search;
mod snooze;
mod source_control;
mod storage;
mod browser_profiles;
mod tasks;
pub(crate) mod themes;
pub use themes::apply as apply_theme;

use crate::theme::{self, AlphaMul, T};
use crate::{App, Appearance, Route, icons};
use rune::{Cx, El, TextEdit, col, el, icon, row, text};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Default)]
pub struct Ui {
    pub theme_edit_dark: bool,
    pub theme_import_open: bool,
    pub theme_import: TextEdit,
    pub theme_editor: Option<Value>,
    pub theme_name: TextEdit,
    pub theme_colors: HashMap<String, TextEdit>,
    pub peer_url: TextEdit,
    pub peer_code: TextEdit,
    pub loaded_page: Option<String>,
    pub active_resources: Vec<(String, Value)>,
    pub results: HashMap<String, Value>,
    pub form: HashMap<String, Value>,
    pub select_options: Vec<(Value, String)>,
    pub task_editor: Option<Value>,
    pub delete_task: Option<(Value, String)>,
    pub auth: auth::Ui,
    pub acp_icons: acp_icons::AcpIcons,
    pub auth_response: TextEdit,
    pub auth_fields: HashMap<String, TextEdit>,
    pub update_version: Option<String>,
    pub delete_thread: Option<(String, String)>,
    /// The project the Danger section is confirming removal of: id and title.
    pub delete_project: Option<(String, String)>,
    pub icon_picker: Option<project_icon::Draft>,
    pub accent: Option<accent::Draft>,
    pub provider_picker: bool,
    pub add_provider: add_provider::Wizard,
    /// The provider whose advisory popover is open, and which trigger (`row` or `header`) opened it.
    pub provider_advisory: Option<(String, &'static str)>,
    /// The sans and mono families last handed to Rune.
    pub fonts_applied: Option<(String, String)>,
    pub provider: Option<String>,
    pub project: Option<String>,
    pub environment_only: bool,
    pub environment_scope_open: bool,
    pub scope_open: bool,
    pub open_select: Option<String>,
    pub search: TextEdit,
    /// The highlighted search result (web's `activeResultIndex`).
    pub search_active: usize,
    pub edits: HashMap<String, TextEdit>,
    pub optimistic_settings: Option<Value>,
    pub pending: HashMap<u64, String>,
    pub error: Option<String>,
    /// Open `AdvancedSettings` folds by page; like web's per-fold state, leaving a page closes them.
    pub advanced: std::collections::HashSet<String>,
    pub keybindings: keybindings::Ui,
    pub keybinding_search: TextEdit,
    pub return_route: Option<Route>,
    pub models: models::Picker,
    pub snooze: snooze::Draft,
    pub profiles: browser_profiles::Ui,
    pub providers_ui: providers::Ui,
    pub source_control: source_control::Ui,
    pub connections: connections::Ui,
    pub maintenance: storage::Maintenance,
    pub devices: devices::Ui,
    pub session: permissions::Session,
    pub telemetry: telemetry::Ui,
}

pub fn appearance(client: &client::ClientSettings) -> Appearance {
    match client.values.get("appearance").and_then(Value::as_str) {
        Some("light") => Appearance::Light,
        Some("dark") => Appearance::Dark,
        _ => Appearance::System,
    }
}

pub fn server_settings(s: &App) -> &Value {
    s.ui.settings
        .optimistic_settings
        .as_ref()
        .or_else(|| s.state().config.as_ref()?.get("settings"))
        .unwrap_or(&Value::Null)
}
fn apply_patch(settings: &mut Value, patch: &Value) {
    if !settings.is_object() {
        *settings = json!({});
    }
    for (key, next) in patch.as_object().into_iter().flatten() {
        if matches!(
            key.as_str(),
            "providerInstances" | "projectSettingsOverrides"
        ) {
            if !settings[key].is_object() {
                settings[key] = json!({});
            }
            for (id, entry) in next.as_object().into_iter().flatten() {
                if entry.is_null() {
                    settings[key].as_object_mut().unwrap().remove(id);
                } else {
                    settings[key][id] = entry.clone();
                }
            }
        } else if key == "sourceControlHosts" {
            merge_host_settings(&mut settings[key], next);
        } else {
            settings[key] = next.clone();
        }
    }
}

/// `applyServerSettingsPatch` for `sourceControlHosts`: fields merge per host kind, and a
/// non-empty `tokens` map merges per lower-cased hostname.
fn merge_host_settings(saved: &mut Value, patch: &Value) {
    if !saved.is_object() {
        *saved = json!({});
    }
    for (kind, fields) in patch.as_object().into_iter().flatten() {
        let saved = &mut saved[kind];
        if !saved.is_object() {
            *saved = json!({});
        }
        for (field, next) in fields.as_object().into_iter().flatten() {
            match next.as_object().filter(|tokens| field == "tokens" && !tokens.is_empty()) {
                Some(tokens) => {
                    let previous = saved["tokens"].as_object().cloned().unwrap_or_default();
                    let mut merged = serde_json::Map::new();
                    for (host, token) in previous.iter().chain(tokens) {
                        merged.insert(host.trim().to_lowercase(), token.clone());
                    }
                    saved["tokens"] = Value::Object(merged);
                }
                None => saved[field.as_str()] = next.clone(),
            }
        }
    }
}

/// Web's "Auto-settle inactive threads" switch: on is a day count, off is `null` (never).
const AUTO_SETTLE_SWITCH: &str = "autoSettleInactive";
const AUTO_SETTLE_DAYS: &str = "sidebarAutoSettleAfterDays";

/// Rows for one field of an object setting, written as `parent.field`.
pub fn nested(key: &str) -> Option<(&'static str, &str)> {
    ["storageCleanup", "sourceControlWritingStyle"].into_iter().find_map(|parent| {
        key.strip_prefix(parent)
            .and_then(|rest| rest.strip_prefix('.'))
            .map(|field| (parent, field))
    })
}

/// An object setting with every field present, so a row can patch one field and send the
/// whole object (the optimistic copy is a shallow merge).
pub fn nested_object(s: &App, parent: &str) -> Value {
    complete_object(parent, value(s, parent))
}

fn complete_object(parent: &str, stored: Value) -> Value {
    let mut object = scope::defaults()[parent].clone();
    if !object.is_object() {
        object = json!({});
    }
    match stored {
        Value::Object(stored) => {
            for (field, v) in stored {
                object[field.as_str()] = v;
            }
        }
        // The legacy writing style was one instructions string (`LegacySourceControlWritingStyle`).
        Value::String(instructions) => {
            object["mode"] = json!(if instructions.is_empty() { "repo_conventions" } else { "custom" });
            object["customInstructions"] = json!(instructions);
        }
        _ => {}
    }
    object
}

pub fn value(s: &App, key: &str) -> Value {
    if key == AUTO_SETTLE_SWITCH {
        return json!(!value(s, AUTO_SETTLE_DAYS).is_null());
    }
    if let Some((parent, field)) = nested(key) {
        return nested_object(s, parent)[field].clone();
    }
    if let Some(key) = key.strip_prefix("project::") {
        if let Some((id, field)) = key.split_once("::") {
            return projects::value(s, id, field);
        }
    }
    if key.starts_with("form.") {
        return s.ui.settings.form.get(key).cloned().unwrap_or(Value::Null);
    }
    if let Some(key) = key.strip_prefix("snooze::") {
        return snooze::value(s, key);
    }
    if let Some(key) = key.strip_prefix("profile::") {
        return browser_profiles::value(s, key);
    }
    if let Some(key) = key.strip_prefix("schost::") {
        return source_control::value(s, key);
    }
    if let Some(key) = key.strip_prefix("bg::") {
        return background::value(s, key);
    }
    if let Some(key) = key.strip_prefix("storage::") {
        return storage::value(s, key);
    }
    if let Some(key) = key.strip_prefix("provider::") {
        if let Some((id, field)) = key.split_once("::") {
            return providers::field_value(s, id, field);
        }
    }
    if scope::project_key(key) {
        if let Some(project) = &s.ui.settings.project {
            if let Some(v) = server_settings(s)["projectSettingsOverrides"][project].get(key) {
                return v.clone();
            }
        }
    }
    s.client_settings
        .values
        .get(key)
        .cloned()
        .or_else(|| server_settings(s).get(key).cloned())
        .unwrap_or(Value::Null)
}

pub fn update(s: &mut App, key: &str, value: Value) {
    if key == AUTO_SETTLE_SWITCH {
        let days = scope::defaults()[AUTO_SETTLE_DAYS].clone();
        return update(s, AUTO_SETTLE_DAYS, if value == true { days } else { Value::Null });
    }
    if let Some((parent, field)) = nested(key) {
        let mut object = nested_object(s, parent);
        object[field] = value;
        return update(s, parent, object);
    }
    if let Some(key) = key.strip_prefix("project::") {
        if let Some((id, field)) = key.split_once("::") {
            projects::update(s, id, field, value);
            return;
        }
    }
    if key.starts_with("form.") {
        s.ui.settings.form.insert(key.into(), value);
        return;
    }
    if let Some(key) = key.strip_prefix("snooze::") {
        snooze::set(s, key, value);
        return;
    }
    if let Some(key) = key.strip_prefix("profile::") {
        browser_profiles::rename(s, key, value);
        return;
    }
    if let Some(key) = key.strip_prefix("schost::") {
        source_control::update(s, key, value);
        return;
    }
    if let Some(key) = key.strip_prefix("bg::") {
        background::set(s, key, value);
        return;
    }
    if let Some(key) = key.strip_prefix("storage::") {
        storage::set(s, key, value);
        return;
    }
    if let Some(key) = key.strip_prefix("provider::") {
        if let Some((id, field)) = key.split_once("::") {
            providers::update(s, id, field, value);
            return;
        }
    }
    if s.client_settings.values.contains_key(key) || key == "appearance" {
        s.client_settings.values.insert(key.into(), value);
        if key == "appearance" {
            s.appearance = appearance(&s.client_settings);
        }
        if matches!(s.src, crate::data::Source::Live(_)) {
            s.client_settings.save();
        }
    } else {
        let patch = if let Some(project) =
            s.ui.settings
                .project
                .as_ref()
                .filter(|_| scope::project_key(key))
        {
            scope::override_patch(server_settings(s), project, key, Some(value))
        } else {
            json!({key:value})
        };
        request(s, "server.updateSettings", json!({"patch":patch}));
    }
}

pub fn request(s: &mut App, method: &str, payload: Value) {
    if method == "server.updateSettings" && matches!(s.src, crate::data::Source::Live(_)) {
        let mut current = server_settings(s).clone();
        apply_patch(&mut current, &payload["patch"]);
        providers::apply_extras(&mut current, &payload);
        s.ui.settings.optimistic_settings = Some(current);
    }
    let project = payload["projectId"].as_str().unwrap_or_default().to_owned();
    if let Some(net) = crate::env::net_for(s, &project) {
        let id = net.request(method, payload);
        s.ui.settings.pending.insert(id, method.into());
    }
}

pub fn http(s: &mut App, method: &str, path: &str, payload: Value) {
    if let Some(net) = s.src.net() {
        let id = net.http_request(method, path, payload);
        s.ui.settings.pending.insert(id, path.into());
    }
}

/// What a settings field shows for a stored value: strings as-is, `null` empty, numbers as JSON.
pub fn input_text(current: &Value) -> String {
    match current {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// The value a field's text commits, matching the contracts schema for `key`; `None` keeps the
/// stored value (e.g. an out-of-range day count, like web's `AutoSettleDaysInput`).
pub fn parse_input(key: &str, current: &Value, text: &str) -> Option<Value> {
    if key == "sidebarAutoSettleAfterDays" {
        return text
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|days| (1..=90).contains(days))
            .map(Value::from);
    }
    if current.is_number() {
        return serde_json::from_str(text.trim()).ok().filter(Value::is_number);
    }
    // Every text setting is a `TrimmedString`.
    Some(json!(text.trim()))
}

/// Commits one field's text if the user changed it. The runtime reaches every field's `on_edit`
/// (focus sync), so an edit entry alone doesn't mean anything was typed.
pub fn commit_text(s: &mut App, key: &str, text: &str) -> bool {
    let current = value(s, key);
    if text == input_text(&current) {
        return false;
    }
    match parse_input(key, &current, text).filter(|next| *next != current) {
        Some(next) => {
            update(s, key, next);
            true
        }
        None => false,
    }
}

pub fn commit_edits(s: &mut App, all: bool) -> bool {
    let keys: Vec<_> =
        s.ui.settings
            .edits
            .iter()
            .filter(|(_, edit)| all || !edit.focused)
            .map(|(key, _)| key.clone())
            .collect();
    let mut changed = false;
    for key in keys {
        let edit = s.ui.settings.edits.remove(&key).unwrap();
        changed |= commit_text(s, &key, &edit.text);
    }
    changed
}

/// The readable message inside an Effect-RPC failure cause: a typed error's `message` (or, for
/// `ServerSettingsError`, the sentence its `message` getter builds), never the raw cause JSON.
fn failure_detail(cause: &Value) -> Option<String> {
    let reasons = cause.as_array().map(Vec::as_slice).unwrap_or(std::slice::from_ref(cause));
    reasons.iter().find_map(|reason| {
        let error = if reason["_tag"] == "Fail" { &reason["error"] } else { reason };
        if let Some(text) = error.as_str().filter(|t| !t.is_empty()) {
            return (reason["_tag"] == "Fail").then(|| text.to_owned());
        }
        if let Some(message) = error["message"].as_str().or(error["detail"].as_str()).filter(|m| !m.is_empty()) {
            return Some(message.to_owned());
        }
        (error["_tag"] == "ServerSettingsError").then(|| {
            let field = |k: &str| error[k].as_str().map(str::to_owned);
            match error["operation"].as_str() {
                Some("write-managed-setting") => format!(
                    "{} is managed by {} and cannot be changed.",
                    field("managedSetting").unwrap_or("This setting".into()),
                    field("managedBy").unwrap_or("your organization".into())
                ),
                operation => format!(
                    "Server settings {} failed{}{} at {}.",
                    operation.unwrap_or("update"),
                    field("providerInstanceId").map(|p| format!(" for provider {p}")).unwrap_or_default(),
                    field("environmentVariable").map(|v| format!(" and environment variable {v}")).unwrap_or_default(),
                    field("settingsPath").unwrap_or_default()
                ),
            }
        })
    })
}

/// Web's wording: settings saves toast "Setting not saved" / "Could not save on <env>: …".
pub fn failure_message(method: &str, environment: &str, cause: &Value) -> String {
    if method == "server.updateSettings" {
        let detail = failure_detail(cause)
            .unwrap_or_else(|| "The save failed. Try reconnecting and saving again.".into());
        return format!("Setting not saved. Could not save on {environment}: {detail}");
    }
    failure_detail(cause).unwrap_or_else(|| "The request failed. Try reconnecting and trying again.".into())
}

pub fn resource<'a>(s: &'a App, method: &str, payload: Value) -> Option<&'a Value> {
    match &s.src {
        crate::data::Source::Live(net) => net.resource(method, payload),
        _ => None,
    }
}

pub fn poll(s: &mut App) -> bool {
    let page = if let Route::Settings(page) = &s.route {
        Some(page.clone())
    } else {
        None
    };
    if page != s.ui.settings.loaded_page {
        if page.as_deref() == Some("connections") {
            request(s, "peerLinks.list", json!({}));
        }
        if page.as_deref() == Some("archived") {
            request(s, projects::ARCHIVED_SNAPSHOT, json!({}));
        }
        if page.as_deref() == Some("source-control") {
            source_control::scan(s);
        }
        if page.is_some() {
            permissions::load(s);
        }
        s.ui.settings.loaded_page = page;
        s.ui.settings.error = None;
        s.ui.settings.advanced.clear();
        accent::close(s);
        s.ui.settings.icon_picker = None;
    }
    let mut wanted = Vec::new();
    if let Route::Settings(path) = &s.route {
        match path.as_str() {
            "scheduled-tasks" => wanted.push(("scheduledTasks.subscribe".to_owned(), json!({}))),
            "access" => wanted.push(("subscribeAuthAccess".to_owned(), json!({}))),
            "devices" => wanted.push(("subscribeDeviceState".to_owned(), json!({}))),
            "maintenance" | "storage" => wanted.extend(storage::report_resource(s)),

            _ => {}
        }
    }
    if matches!(&s.route,Route::Settings(p) if p=="providers") {
        // A subscription opened before the server knows the instance fails for good.
        if let Some(id) = providers::selected_id(s).filter(|id| crate::chat::composer::provider(s, id).is_some()) {
            wanted.push((
                "provider.auth.subscribe".to_owned(),
                json!({"instanceId":id}),
            ));
        }
    }
    for resource in add_provider::wanted(s) {
        if !wanted.contains(&resource) {
            wanted.push(resource);
        }
    }
    if wanted != s.ui.settings.active_resources {
        if let Some(net) = s.src.net() {
            for (method, payload) in &s.ui.settings.active_resources {
                if !wanted.contains(&(method.clone(), payload.clone())) {
                    net.unwatch_resource(method, payload.clone());
                }
            }
            for (method, payload) in &wanted {
                net.watch_resource(method, payload.clone());
            }
        }
        s.ui.settings.active_resources = wanted;
    }
    let ids: Vec<_> = s.ui.settings.pending.keys().copied().collect();
    let mut changed = commit_edits(s, false) | add_provider::poll(s) | acp_icons::poll(s) | auth_terminal::poll(s) | source_control::poll(s) | storage::poll(s) | connections::poll(s) | devices::poll(s) | permissions::poll(s) | telemetry::poll(s);
    for id in ids {
        if let Some(reply) = s.src.net().and_then(|n| n.take_reply(id)) {
            let method = s.ui.settings.pending.remove(&id).unwrap();
            match reply {
                Ok(result) => {
                    if method == "server.updateSettings" {
                        s.ui.settings.error = None;
                    }
                    if method == "scheduledTasks.upsert" {
                        s.ui.settings.task_editor = None;
                    }
                    if matches!(method.as_str(), "server.upsertKeybinding" | "server.removeKeybinding") {
                        keybindings::saved(s, method == "server.upsertKeybinding");
                    }
                    if matches!(method.as_str(), "peerLinks.link" | "peerLinks.unlink") {
                        request(s, "peerLinks.list", json!({}));
                    }
                    if method == "server.updateSettings" && std::mem::take(&mut s.ui.settings.source_control.rescan) {
                        source_control::scan(s);
                    }
                    if method == "thread action" {
                        request(s, projects::ARCHIVED_SNAPSHOT, json!({}));
                    }
                    s.ui.settings.results.insert(method, result);
                }
                Err(error) => {
                    let environment = s
                        .state()
                        .config
                        .as_ref()
                        .and_then(|c| c["environment"]["label"].as_str())
                        .unwrap_or("this environment")
                        .to_owned();
                    let message = failure_message(&method, &environment, &error);
                    match message.strip_prefix("Setting not saved. ") {
                        Some(detail) => crate::shell::palette::toast_error(s, "Setting not saved", Some(detail)),
                        None => s.ui.settings.error = Some(message),
                    }
                }
            }
            changed = true;
        }
    }
    // Fixtures never confirm a write, so a staged copy stays.
    if matches!(s.src, crate::data::Source::Live(_))
        && !s.ui.settings.pending.values().any(|method| method == "server.updateSettings")
        && s.ui.settings.optimistic_settings.take().is_some()
    {
        changed = true;
    }
    changed
}

pub const NAV: &[(&str, &[(&str, &str)])] = &[
    (
        "",
        &[
            ("general", "General"),
            ("appearance", "Appearance"),
            ("chat", "Chat"),
            ("diffs", "Diffs & pull requests"),
            ("keybindings", "Keybindings"),
            ("snap-shot", "SnapShots"),
        ],
    ),
    (
        "Agents",
        &[
            ("providers", "Providers"),
            ("threads", "Threads"),
            ("browser", "Browser"),
            ("devices", "Devices"),
            ("scheduled-tasks", "Scheduled tasks"),
        ],
    ),
    (
        "Code",
        &[
            ("projects", "Projects"),
            ("source-control", "Source control"),
            ("worktrees", "Worktrees"),
        ],
    ),
    (
        "Connections",
        &[
            ("connections", "Environments"),
            ("access", "Sharing & access"),
        ],
    ),
    (
        "",
        &[
            ("maintenance", "Maintenance"),
            ("labs", "Labs"),
            ("about", "About"),
            ("archived", "Archived threads"),
        ],
    ),
];

pub fn label(path: &str) -> &str {
    NAV.iter()
        .flat_map(|(_, items)| items.iter())
        .find(|(p, _)| *p == path)
        .map(|(_, l)| *l)
        .unwrap_or(path)
}

static SNAP_SHOT_ICON: rune::Icon = rune::Icon {
    name: "snap-shot",
    view: 24.0,
    body: r#"<path d="M8 3H6a3 3 0 0 0-3 3v2M16 3h2a3 3 0 0 1 3 3v2M21 16v2a3 3 0 0 1-3 3h-2M8 21H6a3 3 0 0 1-3-3v-2" stroke="currentColor" stroke-width="2" stroke-linecap="round" fill="none"/><rect width="10" height="8" x="7" y="8" rx="2" stroke="currentColor" stroke-width="2" fill="none"/><circle cx="12" cy="12" r="1.5" stroke="currentColor" stroke-width="2" fill="none"/>"#,
};

fn nav_icon(path: &str) -> &'static rune::Icon {
    match path {
        "snap-shot" => &SNAP_SHOT_ICON,
        "archived" => &icons::ARCHIVE,
        "appearance" => &icons::PALETTE,
        "keybindings" => &icons::KEYBOARD,
        "browser" => &icons::GLOBE,
        "providers" => &icons::BOT,
        "chat" => &icons::MESSAGE_SQUARE_TEXT,
        "diffs" => &icons::FILE_DIFF,
        "projects" => &icons::PANELS_TOP_LEFT,
        "worktrees" => &icons::FOLDER_TREE,
        "threads" => &icons::MESSAGES_SQUARE,
        "devices" => &icons::SMARTPHONE,
        "source-control" => &icons::GIT_BRANCH,
        "access" => &icons::SHIELD_CHECK,
        "maintenance" => &icons::WRENCH,
        "labs" => &icons::FLASK_CONICAL,
        "about" => &icons::INFO,
        "secrets" => &icons::KEY_ROUND,
        "scheduled-tasks" => &icons::CALENDAR_CLOCK,
        "connections" => &icons::SERVER,
        _ => &icons::SETTINGS_2,
    }
}

/// Web's search results in place of the page list: icon, title, and the page it lives on.
fn search_results(s: &App, t: &T, query: &str) -> El {
    let results = search::search(query);
    if results.is_empty() {
        return el()
            .w_full()
            .py(24.0)
            .center()
            .role("status")
            .child(text("No settings found").font(theme::TEXT_XS).color(t.sb.muted_foreground));
    }
    let active = s.ui.settings.search_active.min(results.len() - 1);
    let mut list = col().gap(4.0).w_full().role("listbox").label("Settings search results");
    for (index, item) in results.into_iter().enumerate() {
        let target = item.to.clone();
        list = list.child(
            row()
                .key(format!("settings-result-{index}"))
                .w_full()
                .min_h(40.0)
                .px(8.0)
                .py(4.0)
                .gap(8.0)
                .items_start()
                .radius(8.0)
                .fill(if index == active { t.c.sidebar_row_selected } else { t.c.sidebar })
                .hover(t.c.sidebar_row_hover)
                .role("option")
                .label(item.title.clone())
                .on_hover(move |s: &mut App| s.ui.settings.search_active = index)
                .on_press(move |s: &mut App| open_result(s, &target))
                .child(rune::icon(nav_icon(&item.to), 16.0).color(t.sb.sidebar_icon_color).mt(2.0))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .child(text(item.title.clone()).font(theme::SM_MEDIUM).color(t.sb.foreground).truncate())
                        .child(
                            text(label(&item.to).to_owned())
                                .font(theme::TEXT_2XS)
                                .color(t.sb.muted_foreground.alpha_mul(0.75))
                                .truncate(),
                        ),
                ),
        );
    }
    list
}

fn open_result(s: &mut App, page: &str) {
    commit_edits(s, true);
    s.ui.settings.search.clear();
    s.ui.settings.search_active = 0;
    s.ui.settings.open_select = None;
    s.navigate(Route::Settings(page.to_owned()));
}

/// Arrow keys move through the results and Enter opens the active one, as web's combobox does.
fn search_key(s: &mut App, e: &rune::KeyEvent) -> bool {
    if !s.ui.settings.search.focused {
        return false;
    }
    let results = search::search(&s.ui.settings.search.text);
    if results.is_empty() {
        return false;
    }
    let last = results.len() - 1;
    let active = s.ui.settings.search_active.min(last);
    match e.key {
        rune::Key::Down => s.ui.settings.search_active = (active + 1).min(last),
        rune::Key::Up => s.ui.settings.search_active = active.saturating_sub(1),
        rune::Key::Enter => {
            let page = results[active].to.clone();
            open_result(s, &page);
        }
        _ => return false,
    }
    true
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &T) -> El {
    let Route::Settings(path) = &s.route else {
        return el();
    };
    let path = match path.as_str() {
        "" => "general",
        "beta" => "labs",
        "integrations" => "browser",
        "storage" => "maintenance",
        p => p,
    };
    // The search stays put while the page list under it scrolls.
    let mut nav = col().w_full().px(8.0).pb(8.0).gap(16.0);
    let search = rune::text_input(&s.ui.settings.search, theme::SM_MEDIUM.lh(21.0))
        .key("settings-search")
        .single_line()
        .placeholder("Search", t.sb.muted_foreground)
        .grow(1.0)
        .min_w(0.0)
        .h(21.0)
        .px(0.0)
        .radius(6.0)
        .fill(t.c.sidebar)
        .color(t.sb.muted_foreground)
        .caret(t.sb.foreground, t.c.primary.alpha_mul(0.3))
        .on_edit(|s: &mut App| {
            s.ui.settings.search_active = 0;
            &mut s.ui.settings.search
        })
        .on_input_key(|s: &mut App, e, _| clear_search_on_escape(&mut s.ui.settings.search, e) || search_key(s, e));
    let search_row = row()
            .h(32.0)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .child(icon(&icons::SEARCH, 16.0).color(t.sb.muted_foreground))
            .child(search)
            .child(
                el().size(20.0, 20.0)
                    .center()
                    .radius(4.0)
                    .fill(t.c.foreground.alpha_mul(0.07))
                    .child(
                        text("/")
                            .font(theme::XS_MEDIUM)
                            .color(t.sb.muted_foreground),
                    ),
            );
    let query = s.ui.settings.search.text.clone();
    if !search::normalize(&query).is_empty() {
        nav = nav.child(search_results(s, t, &query));
    }
    for (heading, items) in NAV.iter().filter(|_| search::normalize(&query).is_empty()) {
        let mut group = col().gap(4.0).w_full();
        if !heading.is_empty() {
            group = group.child(
                text(*heading)
                    .font(theme::XS_MEDIUM)
                    .color(t.sb.muted_foreground)
                    .px(8.0)
                    .h(16.0),
            );
        }
        for (target, title) in *items {
            let icon = nav_icon(target);
            let target = target.to_string();
            let active = path == target;
            group = group.child(
                row()
                    .key(format!("settings-nav-{target}"))
                    .w_full()
                    .h(32.0)
                    .px(10.0)
                    .gap(8.0)
                    .center_y()
                    .radius(8.0)
                    .fill(if active {
                        t.c.sidebar_row_selected
                    } else {
                        t.c.sidebar
                    })
                    .hover(t.c.sidebar_row_hover)
                    .focusable(true)
                    .role("link")
                    .label(*title)
                    .on_press(move |s: &mut App| {
                        commit_edits(s, true);
                        s.navigate(Route::Settings(target.clone()));
                        s.ui.settings.open_select = None;
                    })
                    .child(rune::icon(icon, 16.0).color(if active {
                        t.c.foreground
                    } else {
                        t.sb.sidebar_icon_color
                    }))
                    .child(text(*title).font(theme::SM_MEDIUM).color(if active {
                        t.c.foreground
                    } else {
                        t.sb.muted_foreground.alpha_mul(0.8)
                    })),
            );
        }
        nav = nav.child(group);
    }
    let nav = col()
        .w(239.0)
        .h_full()
        .fill(t.c.sidebar)
        .child(col().w_full().px(8.0).pt(8.0).pb(8.0).child(search_row))
        // `SidebarContent`'s `scrollFade` at `--fade-size: 0.75rem`.
        .child(nav.grow(1.0).min_h(0.0).scroll(rune::ScrollSpec {
            stick_bottom: false,
        }).scroll_edge_fade(12.0))
        .child(el().h(1.0).w_full().fill(t.c.border));
    let mut header = row()
        .h(47.0)
        .w_full()
        .pl(20.0)
        .pr(8.0)
        .gap(12.0)
        .center_y()
        .child(
            text("Settings")
                .font(theme::TEXT_SM)
                .color(t.c.muted_foreground),
        )
        .child(text("/").font(theme::TEXT_SM).color(t.c.muted_foreground))
        .child(
            text(label(path))
                .font(theme::SM_MEDIUM)
                .color(t.c.foreground),
        )
        .child(el().grow(1.0));
    if path == "general" {
        header = header.child(
            crate::kit::Button::new("Restore device defaults")
                .variant("ghost")
                .size("xs")
                .icon(&icons::ROTATE_CCW)
                .disabled(
                    !client::ClientSettings::default()
                        .values
                        .iter()
                        .any(|(key, v)| s.client_settings.values.get(key) != Some(v)),
                )
                .build(t)
                .on_press(|s: &mut App| {
                    let defaults = client::ClientSettings::default();
                    s.client_settings.values.extend(defaults.values);
                    s.client_settings
                        .values
                        .insert("appearance".into(), json!("system"));
                    s.appearance = Appearance::System;
                    if matches!(s.src, crate::data::Source::Live(_)) {
                        s.client_settings.save();
                    }
                }),
        );
    }
    header = header.child(
        el().size(28.0, 28.0)
            .center()
            .radius(6.0)
            .hover(t.c.accent)
            .focusable(true)
            .label("Close Settings")
            .on_press(|s: &mut App| close(s))
            .child(icon(&icons::X, 16.0).color(t.c.muted_foreground)),
    );
    let mut content = col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .child(header)
        .child(el().h(1.0).w_full().fill(t.c.border));
    let page = pages::view(s, cx, t, path);
    content = content.child(
        col()
            .key(format!("settings-scroll-{path}"))
            .grow(1.0)
            .min_h(0.0)
            .w_full()
            .items_center()
            .scroll(rune::ScrollSpec {
                stick_bottom: false,
            })
            .child(page),
    );
    let modal = row()
        .key("settings-modal")
        .padding(1.0)
        .modal(true, close)
        .w_full()
        .max_w(1280.0)
        .h_full()
        .max_h(960.0)
        .radius(16.0)
        .clip_rounded(16.0)
        .border(1.0, t.c.border)
        .shadow(25.0, 50.0, rune::Color::BLACK.alpha(0.4))
        .focusable(true)
        .role("dialog")
        .label("Settings")
        .fill(t.c.background)
        .child(nav)
        .child(el().w(1.0).h_full().fill(t.c.border))
        .child(content);
    let mut overlays = Vec::new();
    add_provider::dialog(s, t, &mut overlays);
    accent::popover(s, t, &mut overlays);
    provider_update::popover(s, t, &mut overlays);
    providers::confirm(s, t, &mut overlays);
    auth::confirm(s, t, &mut overlays);
    themes::editor(s, t, &mut overlays);
    themes::import_dialog(s, t, &mut overlays);
    projects::confirm_delete(s, t, &mut overlays);
    projects::confirm_remove(s, t, &mut overlays);
    project_icon::dialog(s, t, &mut overlays);
    if crate::chat::details::scripts::settings_editor_open(s) {
        crate::chat::details::scripts::overlays(s, t, &mut overlays);
    }
    tasks::editor(s, t, &mut overlays);
    background::overlays(s, t, &mut overlays);
    about::update_dialog(s, t, &mut overlays);
    crate::desktop::exposure::confirm(s, t, &mut overlays);
    connections::overlays(s, t, &mut overlays);
    crate::env::ssh::password_dialog(s, t, &mut overlays);
    models::popover(s, t, &mut overlays);
    browser_profiles::overlays(s, t, cx.env().scale, &mut overlays);
    scope::menu(s, t, &mut overlays);
    devices::overlays(s, t, &mut overlays);
    keybindings::overlays(s, t, &mut overlays);
    permissions::overlays(s, t, &mut overlays);
    if let Some(key) = &s.ui.settings.open_select {
        let fallback: Vec<_> = pages::specs()
            .as_object()
            .into_iter()
            .flat_map(|p| p.values())
            .flat_map(|p| p.as_array().into_iter().flatten())
            .flat_map(|sec| sec["rows"].as_array().into_iter().flatten())
            .find(|r| r["key"].as_str() == Some(key))
            .into_iter()
            .flat_map(|r| r["options"].as_array().into_iter().flatten())
            .map(|o| (o[0].clone(), o[1].as_str().unwrap_or("").to_owned()))
            .collect();
        let options = if s.ui.settings.select_options.is_empty() {
            &fallback
        } else {
            &s.ui.settings.select_options
        };
        controls::select_menu(s, t, key, options, controls::select_width(key), &mut overlays);
    }
    rune_kit::root(
        // Web dims to `bg-background/60` under a 4 px blur that leaves the app barely legible;
        // without the blur only a deeper dim reads the same.
        crate::kit::motion::layer()
            .fill(t.c.background.alpha_mul(if t.dark { 0.9 } else { 0.85 }))
            .padding(40.0)
            .center()
            .child(crate::kit::motion::popup(modal)),
        overlays,
    )
    .transition(rune::Transition::none())
    .absolute(0.0, 0.0)
}

pub fn close(s: &mut App) {
    s.ui.settings.results.remove("/api/auth/pairing-token");
    s.ui.settings.auth_fields.clear();
    s.ui.settings.auth_response.clear();
    s.ui.settings.peer_code.clear();
    commit_edits(s, true);
    let route =
        s.ui.settings
            .return_route
            .take()
            .unwrap_or(Route::Home { project_id: None });
    s.navigate(route);
}

/// Settings opened by URL, as the references d10-d29 were: nothing to return to.
fn scene(s: &mut App, path: &str) {
    crate::scene_clock(s);
    s.client_settings = client::ClientSettings::default();
    s.route = Route::Settings(path.into());
    s.ui.settings = Ui::default();
}

/// Settings opened over "Replay showcase" with mod+comma (d09).
fn scene_from_thread(s: &mut App) {
    scene(s, "general");
    s.ui.settings.return_route = s.state().threads.iter().find(|t| t["title"] == "Replay showcase").map(|t| Route::Thread(crate::data::s(t, "id").into()));
}

pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("settings-connections-add-remote", |s| {
        scene(s, "connections");
        connections::scene(s, "add-remote");
    }),
    ("settings-connections-add-ssh", |s| {
        scene(s, "connections");
        connections::scene(s, "add-ssh");
    }),
    ("settings-connections-env-menu", |s| {
        scene(s, "connections");
        connections::scene(s, "env-menu");
    }),
    ("settings-connections-link", |s| {
        scene(s, "connections");
        connections::scene(s, "link");
    }),
    ("settings-devices-simlock", |s| {
        scene(s, "devices");
        devices_editor::open_simlock(s, None);
    }),
    ("settings-runtime-mode-open", |s| {
        scene(s, "threads");
        s.ui.settings.open_select = Some("defaultRuntimeMode".into());
    }),
    ("settings-general", |s| scene(s, "general")),
    ("settings-from-thread", scene_from_thread),
    ("settings-appearance", |s| scene(s, "appearance")),
    ("settings-providers", |s| scene(s, "providers")),
    ("settings-browser", |s| scene(s, "browser")),
    ("settings-keybindings", |s| scene(s, "keybindings")),
    ("settings-chat", |s| scene(s, "chat")),
    ("settings-diffs", |s| scene(s, "diffs")),
    ("settings-snap-shot", |s| scene(s, "snap-shot")),
    ("settings-threads", |s| scene(s, "threads")),
    ("settings-devices", |s| scene(s, "devices")),
    ("settings-scheduled-tasks", |s| scene(s, "scheduled-tasks")),
    ("settings-projects", |s| scene(s, "projects")),
    ("settings-source-control", |s| scene(s, "source-control")),
    ("settings-worktrees", |s| scene(s, "worktrees")),
    ("settings-connections", |s| scene(s, "connections")),
    ("settings-access", |s| scene(s, "access")),
    ("settings-access-desktop", |s| {
        scene(s, "access");
        crate::desktop::exposure::scene(s);
    }),
    ("settings-secrets", |s| scene(s, "secrets")),
    ("settings-maintenance", |s| scene(s, "maintenance")),
    ("settings-labs", |s| scene(s, "labs")),
    ("settings-about", |s| scene(s, "about")),
    ("settings-archived", |s| scene(s, "archived")),
    ("settings-storage", |s| scene(s, "storage")),
    ("settings-diagnostics", |s| scene(s, "diagnostics")),
    ("settings-open-source-licenses", |s| {
        scene(s, "open-source-licenses")
    }),
    ("settings-beta", |s| scene(s, "beta")),
    ("settings-integrations", |s| scene(s, "integrations")),
    ("settings-general-light", |s| {
        scene(s, "general");
        s.appearance = Appearance::Light;
    }),
    ("settings-appearance-light", |s| {
        scene(s, "appearance");
        s.appearance = Appearance::Light;
    }),
    ("settings-providers-light", |s| {
        scene(s, "providers");
        s.appearance = Appearance::Light;
    }),
    ("settings-select-open", |s| {
        scene(s, "general");
        s.ui.settings.open_select = Some("notificationMode".into());
    }),
    ("settings-switch-on", |s| {
        scene(s, "general");
        s.client_settings
            .values
            .insert("inAppNotificationsEnabled".into(), json!(true));
    }),
    ("settings-keybindings-editor", |s| {
        scene(s, "keybindings");
        keybindings::scene_editor(s);
    }),
    ("settings-threads-light", |s| {
        scene(s, "threads");
        s.appearance = Appearance::Light;
    }),
    ("settings-threads-model-picker", |s| threads_picker(s, false)),
    ("settings-threads-model-picker-light", |s| threads_picker(s, true)),
    ("settings-threads-snooze-edited", |s| threads_snooze(s, false)),
    ("settings-threads-snooze-edited-light", |s| threads_snooze(s, true)),
    ("settings-browser-light", |s| {
        scene(s, "browser");
        s.appearance = Appearance::Light;
    }),
    ("settings-browser-profiles", |s| browser_profiles_scene(s, false, None)),
    ("settings-browser-profiles-light", |s| browser_profiles_scene(s, true, None)),
    ("settings-browser-profile-menu", |s| browser_profiles_scene(s, false, Some("profile-work"))),
    ("settings-browser-profile-menu-light", |s| browser_profiles_scene(s, true, Some("profile-work"))),
    ("settings-browser-add-profile-menu", |s| browser_profiles_scene(s, false, Some("add"))),
    ("settings-browser-remove-profile", |s| {
        browser_profiles_scene(s, false, None);
        s.ui.settings.profiles.remove = Some(("profile-work".into(), "Work".into()));
    }),
    ("settings-browser-remove-profile-light", |s| {
        browser_profiles_scene(s, true, None);
        s.ui.settings.profiles.remove = Some(("profile-work".into(), "Work".into()));
    }),
    ("settings-providers-models", |s| providers_models(s, false)),
    ("settings-providers-models-light", |s| providers_models(s, true)),
    ("settings-providers-claude", |s| providers_scene(s, false, "claudeAgent")),
    ("settings-providers-claude-light", |s| providers_scene(s, true, "claudeAgent")),
    ("settings-providers-custom-model", |s| providers_custom_model(s, false)),
    ("settings-providers-custom-model-light", |s| providers_custom_model(s, true)),
    ("settings-providers-delete", |s| providers_delete(s, false)),
    ("settings-providers-delete-light", |s| providers_delete(s, true)),
    ("settings-providers-unavailable", |s| providers_state(s, false, s5_net::Conn::Open)),
    ("settings-providers-unavailable-light", |s| providers_state(s, true, s5_net::Conn::Open)),
    ("settings-providers-loading", |s| providers_state(s, false, s5_net::Conn::Connecting)),
    ("settings-providers-loading-light", |s| providers_state(s, true, s5_net::Conn::Connecting)),
    ("settings-providers-error", |s| providers_error(s, false)),
    ("settings-providers-error-light", |s| providers_error(s, true)),
    ("settings-providers-secrets", |s| providers_secrets(s, false, "codex_work")),
    ("settings-providers-secrets-light", |s| providers_secrets(s, true, "codex_work")),
    ("settings-providers-password", |s| providers_secrets(s, false, "opencode")),
    ("settings-providers-password-light", |s| providers_secrets(s, true, "opencode")),
];

/// The providers page with model preferences and a second Codex instance staged.
fn providers_scene(s: &mut App, light: bool, selected: &str) {
    scene(s, "providers");
    if light {
        s.appearance = Appearance::Light;
    }
    stage_settings(
        s,
        json!({
            "favorites": [
                {"provider": "opencode", "model": "opencode/big-pickle"},
                {"provider": "opencode", "model": "opencode/beam-free"}
            ],
            "providerModelPreferences": {
                "opencode": {"hiddenModels": ["opencode/flint-40b-free", "opencode/crouton-alpha-free"], "modelOrder": []}
            },
            "providerInstances": {
                "codex_work": {
                    "driver": "codex",
                    "displayName": "Codex (work)",
                    "config": {"homePath": "~/.codex-work"},
                    "environment": [{"name": "OPENAI_BASE_URL", "value": "https://proxy.internal/v1", "sensitive": false}]
                },
                "claudeAgent": {"driver": "claudeAgent", "config": {"customModels": ["claude-sonnet-5-preview"]}}
            }
        }),
    );
    s.ui.settings.provider = Some(selected.into());
}

/// A stored sensitive variable, a secret provider reference, and a stored OpenCode password.
fn providers_secrets(s: &mut App, light: bool, selected: &str) {
    providers_scene(s, light, selected);
    stage_settings(
        s,
        json!({"providerInstances": {
            "codex_work": {
                "driver": "codex",
                "displayName": "Codex (work)",
                "config": {"homePath": "~/.codex-work"},
                "environment": [
                    {"name": "OPENAI_BASE_URL", "value": "https://proxy.internal/v1", "sensitive": false},
                    {"name": "OPENAI_API_KEY", "value": "", "sensitive": true, "valueRedacted": true},
                    {"name": "GITHUB_TOKEN", "value": "", "sensitive": true,
                        "secret": {"provider": "onepassword", "reference": "op://Engineering/GitHub/token"}}
                ]
            },
            "opencode": {"driver": "opencode", "config": {"serverUrl": "http://127.0.0.1:4096", "serverPassword": "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}"}}
        }}),
    );
    s.ui.settings.providers_ui.scene_bottom = selected == "codex_work";
}

fn providers_models(s: &mut App, light: bool) {
    providers_scene(s, light, "opencode");
    s.ui.settings.providers_ui.scene_bottom = true;
}

fn providers_custom_model(s: &mut App, light: bool) {
    providers_scene(s, light, "claudeAgent");
    let ui = &mut s.ui.settings.providers_ui;
    ui.scene_bottom = true;
    ui.adding = true;
    ui.custom = rune::TextEdit::new("claude-sonnet-5-preview".to_owned());
    ui.error = Some("That custom model is already saved.".into());
}

fn providers_delete(s: &mut App, light: bool) {
    providers_scene(s, light, "codex_work");
    s.ui.settings.providers_ui.confirm = Some(("codex_work".into(), false));
}

/// No configuration for the environment: `conn` picks loading, unavailable, or the error.
fn providers_state(s: &mut App, light: bool, conn: s5_net::Conn) {
    scene(s, "providers");
    if light {
        s.appearance = Appearance::Light;
    }
    if let crate::data::Source::Fixture(st) = &mut s.src {
        st.config = None;
        st.conn = conn;
    }
}

fn providers_error(s: &mut App, light: bool) {
    providers_state(s, light, s5_net::Conn::Closed("WebSocket closed before the connection was established (1006)".into()));
}

/// Scenes stage server settings as the optimistic copy a live write would leave.
fn stage_settings(s: &mut App, patch: Value) {
    let mut current = server_settings(s).clone();
    apply_patch(&mut current, &patch);
    s.ui.settings.optimistic_settings = Some(current);
}

fn threads_picker(s: &mut App, light: bool) {
    scene(s, "threads");
    if light {
        s.appearance = Appearance::Light;
    }
    stage_settings(s, json!({"favorites": [{"provider": "opencode", "model": "opencode/big-pickle"}]}));
    models::toggle(s, "defaultModelSelection");
}

fn threads_snooze(s: &mut App, light: bool) {
    scene(s, "threads");
    if light {
        s.appearance = Appearance::Light;
    }
    // A passed "today" option, a duplicate, a same-minute wake, and the 8-option cap.
    stage_settings(
        s,
        json!({
            "fallbackModelSelection": {"instanceId": "opencode", "model": "opencode/big-pickle"},
            "snoozePresets": [
                {"kind": "after", "minutes": 30},
                {"kind": "after", "minutes": 90},
                {"kind": "at", "day": "today", "time": "07:00"},
                {"kind": "at", "day": "tomorrow", "time": "09:00"},
                {"kind": "at", "day": "tomorrow", "time": "09:00"},
                {"kind": "after", "minutes": 2880},
                {"kind": "at", "day": "saturday", "time": "10:30"},
                {"kind": "at", "day": "monday", "time": "09:00"}
            ]
        }),
    );
}

fn browser_profiles_scene(s: &mut App, light: bool, menu: Option<&str>) {
    scene(s, "browser");
    if light {
        s.appearance = Appearance::Light;
    }
    s.client_settings.values.insert(
        "browserProfiles".into(),
        json!([
            {"id": "profile-work", "name": "Work", "kind": "persistent"},
            {"id": "profile-testing", "name": "QA accounts", "kind": "persistent"}
        ]),
    );
    s.client_settings.values.insert("browserDefaultProfileId".into(), json!("profile-work"));
    s.ui.settings.profiles.menu = menu.map(str::to_owned);
}

pub fn on_key(s: &mut App, e: &rune::KeyEvent, _tx: &mut rune::Tx) -> bool {
    if !matches!(s.route, Route::Settings(_)) {
        return false;
    }
    if crate::chat::details::scripts::settings_editor_open(s) {
        // The action editor captures its shortcut; Escape closes it before Settings.
        return match e.key {
            rune::Key::Escape => crate::chat::details::scripts::escape(s),
            _ => crate::chat::details::scripts::on_key(s, e),
        };
    }
    if matches!(e.key, rune::Key::Escape) {
        if s.ui.settings.models.open.take().is_some()
            || s.ui.settings.icon_picker.take().is_some()
            || accent::close(s)
            || browser_profiles::escape(s)
            || connections::escape(s)
            || devices::escape(s)
            || s.ui.settings.providers_ui.confirm.take().is_some()
        {
            return true;
        }
        if s.ui.settings.open_select.take().is_some() || background::escape(s) {
            return true;
        }
        if s.ui.settings.theme_import_open {
            s.ui.settings.theme_import_open = false;
            return true;
        }
        if s.ui.settings.environment_scope_open {
            s.ui.settings.environment_scope_open = false;
            return true;
        }
        if s.ui.settings.scope_open {
            s.ui.settings.scope_open = false;
            return true;
        }
        if s.ui.settings.update_version.take().is_some()
            || s.ui.settings.theme_editor.take().is_some()
            || s.ui.settings.delete_thread.take().is_some()
            || s.ui.settings.delete_project.take().is_some()
            || s.ui.settings.delete_task.take().is_some()
        {
            return true;
        }
        if s.ui.settings.task_editor.take().is_some() {
            return true;
        }
        if s.ui.settings.open_select.take().is_some() {
            return true;
        }
        if keybindings::escape(s) {
            return true;
        }
        close(s);
        return true;
    }
    false
}

/// Web search fields clear on Escape before Escape leaves Settings.
/// A search field's own Escape: clears its text first. The field sees the key before its editor
/// blurs and hands it on to `on_key`, which would close Settings.
pub fn clear_search_on_escape(search: &mut TextEdit, e: &rune::KeyEvent) -> bool {
    let clear = matches!(e.key, rune::Key::Escape) && !search.text.is_empty();
    if clear {
        search.clear();
    }
    clear
}

#[cfg(test)]
mod patch_tests {
    use super::*;
    #[test]
    fn rapid_scoped_changes_build_on_the_previous_pending_patch() {
        let mut settings = json!({"projectSettingsOverrides":{"p":{"future":true}}});
        for (key, value) in [
            ("defaultAutoPull", json!(true)),
            ("responseStreamingMode", json!("paragraph")),
        ] {
            let patch = scope::override_patch(&settings, "p", key, Some(value));
            apply_patch(&mut settings, &patch);
        }
        assert_eq!(
            settings["projectSettingsOverrides"]["p"],
            json!({"future":true,"defaultAutoPull":true,"responseStreamingMode":"paragraph"})
        );
    }
    #[test]
    fn host_settings_merge_per_kind_and_tokens_per_host() {
        let mut settings = json!({"sourceControlHosts": {
            "github": {"tokens": {"github.com": "a", "ghe.example": "b"}, "hosts": {"github.com": {"enabled": false}}},
            "gitcafe": {"token": "c"}
        }});
        apply_patch(&mut settings, &json!({"sourceControlHosts": {"github": {"tokens": {"GHE.example ": ""}}}}));
        apply_patch(&mut settings, &json!({"sourceControlHosts": {"bitbucket": {"email": "me@example.com"}}}));
        assert_eq!(
            settings["sourceControlHosts"],
            json!({
                "github": {"tokens": {"github.com": "a", "ghe.example": ""}, "hosts": {"github.com": {"enabled": false}}},
                "gitcafe": {"token": "c"},
                "bitbucket": {"email": "me@example.com"}
            })
        );
        // An empty token map clears them all, and `hosts` is replaced whole.
        apply_patch(&mut settings, &json!({"sourceControlHosts": {"github": {"tokens": {}, "hosts": {}}}}));
        assert_eq!(settings["sourceControlHosts"]["github"], json!({"tokens": {}, "hosts": {}}));
    }
    #[test]
    fn visiting_a_page_never_commits_an_untouched_field() {
        // `null` (auto-settle off) shows an empty field; leaving it empty must not write "".
        assert_eq!(input_text(&Value::Null), "");
        assert_eq!(input_text(&json!(3)), "3");
        assert_eq!(parse_input("sidebarAutoSettleAfterDays", &Value::Null, ""), None);
        assert_eq!(parse_input("sidebarAutoSettleAfterDays", &Value::Null, "7"), Some(json!(7)));
        assert_eq!(parse_input("sidebarAutoSettleAfterDays", &json!(3), "91"), None);
        assert_eq!(parse_input("sidebarAutoSettleAfterDays", &json!(3), "2.5"), None);
        assert_eq!(parse_input("branchNamePrefix", &json!("t3"), "me"), Some(json!("me")));
        assert_eq!(parse_input("reviewGuideInstructions", &json!(""), "  Skip tests\n"), Some(json!("Skip tests")));
    }
    #[test]
    fn a_nested_row_patches_its_whole_parent_object() {
        assert_eq!(nested("sourceControlWritingStyle.mode"), Some(("sourceControlWritingStyle", "mode")));
        assert_eq!(nested("storageCleanup.enabled"), Some(("storageCleanup", "enabled")));
        assert_eq!(nested("sourceControlWritingStyleX"), None);
        // The server may store the legacy instructions string or omit default fields.
        assert_eq!(
            complete_object("sourceControlWritingStyle", json!("Be brief")),
            json!({"mode":"custom","customInstructions":"Be brief","followChangeRequestTemplates":true})
        );
        assert_eq!(
            complete_object("sourceControlWritingStyle", json!({"mode":"conventional_commits"})),
            json!({"mode":"conventional_commits","customInstructions":"","followChangeRequestTemplates":true})
        );
    }
    #[test]
    fn failures_read_as_sentences_not_cause_json() {
        let die = json!([{"_tag":"Die","defect":"Expected \"Infinity\" at [\"patch\"]"}]);
        assert_eq!(
            failure_message("server.updateSettings", "Mac", &die),
            "Setting not saved. Could not save on Mac: The save failed. Try reconnecting and saving again."
        );
        let managed = json!([{"_tag":"Fail","error":{"_tag":"ServerSettingsError","settingsPath":"/s.json","operation":"write-managed-setting","managedSetting":"Providers","managedBy":"Acme"}}]);
        assert_eq!(
            failure_message("server.updateSettings", "Mac", &managed),
            "Setting not saved. Could not save on Mac: Providers is managed by Acme and cannot be changed."
        );
        let write = json!([{"_tag":"Fail","error":{"_tag":"ServerSettingsError","settingsPath":"/s.json","operation":"write"}}]);
        assert!(failure_message("server.updateSettings", "Mac", &write).ends_with("Server settings write failed at /s.json."));
        let typed = json!([{"_tag":"Fail","error":{"_tag":"X","message":"No such task."}}]);
        assert_eq!(failure_message("scheduledTasks.upsert", "Mac", &typed), "No such task.");
        let setup = json!([{"_tag":"Fail","error":{"_tag":"ProviderSetupError","detail":"Sign-in expired."}}]);
        assert_eq!(failure_message("provider.auth.start", "Mac", &setup), "Sign-in expired.");
    }
    #[test]
    fn escape_clears_a_search_before_closing() {
        let key = |key| rune::KeyEvent { key, mods: Default::default() };
        let mut search = TextEdit::new("theme");
        assert!(!clear_search_on_escape(&mut search, &key(rune::Key::Enter)));
        assert!(clear_search_on_escape(&mut search, &key(rune::Key::Escape)));
        assert_eq!(search.text, "");
        assert!(!clear_search_on_escape(&mut search, &key(rune::Key::Escape)), "empty search lets Escape through");
    }
}
