mod about;
mod auth;
mod balance;
pub mod client;
mod connections;
mod controls;
mod devices;
mod keybindings;
mod models;
mod pages;
mod projects;
mod providers;
mod scope;
mod snooze;
mod browser_profiles;
mod tasks;
mod themes;
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
    pub auth_response: TextEdit,
    pub auth_fields: HashMap<String, TextEdit>,
    pub update_version: Option<String>,
    pub delete_thread: Option<(String, String)>,
    pub provider_picker: bool,
    pub provider: Option<String>,
    pub project: Option<String>,
    pub environment_only: bool,
    pub environment_scope_open: bool,
    pub scope_open: bool,
    pub open_select: Option<String>,
    pub search: TextEdit,
    pub edits: HashMap<String, TextEdit>,
    pub optimistic_settings: Option<Value>,
    pub pending: HashMap<u64, String>,
    pub error: Option<String>,
    pub advanced: bool,
    pub keybinding_editor: Option<String>,
    pub binding_when: TextEdit,
    pub shortcut: Option<String>,
    pub binding_target: Option<Value>,
    pub keybinding_search: TextEdit,
    pub return_route: Option<Route>,
    pub models: models::Picker,
    pub snooze: snooze::Draft,
    pub profiles: browser_profiles::Ui,
    pub providers_ui: providers::Ui,
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
        } else {
            settings[key] = next.clone();
        }
    }
}

pub fn value(s: &App, key: &str) -> Value {
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
    if let Some(key) = key.strip_prefix("provider::") {
        if let Some((id, field)) = key.split_once("::") {
            return providers::field_value(s, id, field);
        }
    }
    if let Some(field) = key.strip_prefix("storageCleanup.") {
        return server_settings(s)["storageCleanup"][field].clone();
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
        let patch = if let Some(field) = key.strip_prefix("storageCleanup.") {
            // The whole object, so the optimistic copy (a shallow merge) keeps the other rules.
            let mut cleanup = server_settings(s)["storageCleanup"].clone();
            cleanup[field] = value;
            json!({ "storageCleanup": cleanup })
        } else if let Some(project) =
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
    if let Some(net) = s.src.net() {
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
        let current = value(s, &key);
        let next = if current.is_number() {
            serde_json::from_str(&edit.text)
                .ok()
                .filter(Value::is_number)
        } else {
            Some(json!(edit.text))
        };
        if let Some(next) = next.filter(|n| *n != current) {
            update(s, &key, next);
            changed = true;
        }
    }
    changed
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
        s.ui.settings.loaded_page = page;
    }
    let mut wanted = Vec::new();
    if let Route::Settings(path) = &s.route {
        match path.as_str() {
            "scheduled-tasks" => wanted.push(("scheduledTasks.subscribe".to_owned(), json!({}))),
            "access" => wanted.push(("subscribeAuthAccess".to_owned(), json!({}))),
            "devices" => wanted.push(("subscribeDeviceState".to_owned(), json!({}))),

            _ => {}
        }
    }
    if matches!(&s.route,Route::Settings(p) if p=="providers") {
        if let Some(id) = providers::selected_id(s) {
            wanted.push((
                "provider.auth.subscribe".to_owned(),
                json!({"instanceId":id}),
            ));
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
    let mut changed = commit_edits(s, false);
    for id in ids {
        if let Some(reply) = s.src.net().and_then(|n| n.take_reply(id)) {
            let method = s.ui.settings.pending.remove(&id).unwrap();
            match reply {
                Ok(result) => {
                    if method == "scheduledTasks.upsert" {
                        s.ui.settings.task_editor = None;
                    }
                    if method == "server.upsertKeybinding" {
                        s.ui.settings.keybinding_editor = None;
                        s.ui.settings.shortcut = None;
                    }
                    if matches!(method.as_str(), "peerLinks.link" | "peerLinks.unlink") {
                        request(s, "peerLinks.list", json!({}));
                    }
                    s.ui.settings.results.insert(method, result);
                }
                Err(error) => {
                    s.ui.settings.error = Some(error.to_string());
                }
            }
            changed = true;
        }
    }
    if !s
        .ui
        .settings
        .pending
        .values()
        .any(|method| method == "server.updateSettings")
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
    let mut nav = col()
        .w(239.0)
        .h_full()
        .fill(t.c.sidebar)
        .px(8.0)
        .pt(8.0)
        .gap(16.0);
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
        .on_edit(|s: &mut App| &mut s.ui.settings.search);
    nav = nav.child(
        row()
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
            ),
    );
    let query = s.ui.settings.search.text.to_lowercase();
    for (index, (heading, items)) in NAV.iter().enumerate() {
        let mut group = col().gap(4.0).w_full();
        if index == 0 {
            group = group.mt(-8.0);
        }
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
            if !query.is_empty() && !title.to_lowercase().contains(&query) {
                continue;
            }
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
                        t.sb.sidebar_row_active
                    } else {
                        t.c.sidebar
                    })
                    .hover(t.sb.sidebar_row_hover)
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
        .child(nav.scroll(rune::ScrollSpec {
            stick_bottom: false,
        }))
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
    providers::picker(s, t, &mut overlays);
    providers::confirm(s, t, &mut overlays);
    themes::editor(s, t, &mut overlays);
    themes::import_dialog(s, t, &mut overlays);
    projects::confirm_delete(s, t, &mut overlays);
    tasks::editor(s, t, &mut overlays);
    about::update_dialog(s, t, &mut overlays);
    models::popover(s, t, &mut overlays);
    browser_profiles::overlays(s, t, &mut overlays);
    if let Some(menu) = scope::menu(s, t) {
        overlays.push(menu);
    }
    if let Some(editor) = keybindings::editor(s, t) {
        overlays.push(
            el().absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .center()
                .child(editor),
        );
    }
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
        el().absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .padding(40.0)
            .center()
            .fill(t.c.background.alpha_mul(if t.dark { 0.64 } else { 0.60 }))
            .backdrop_blur(4.0)
            .child(modal),
        overlays,
    )
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
        s.ui.settings.keybinding_editor = Some("New thread".into());
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
    ("settings-providers-models", |s| providers_scene(s, false, "opencode")),
    ("settings-providers-models-light", |s| providers_scene(s, true, "opencode")),
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

fn providers_custom_model(s: &mut App, light: bool) {
    providers_scene(s, light, "claudeAgent");
    let ui = &mut s.ui.settings.providers_ui;
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
    if matches!(e.key, rune::Key::Escape) {
        if s.ui.settings.models.open.take().is_some()
            || browser_profiles::escape(s)
            || s.ui.settings.providers_ui.confirm.take().is_some()
        {
            return true;
        }
        if s.ui.settings.open_select.take().is_some() {
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
        if s.ui.settings.keybinding_editor.take().is_some() {
            return true;
        }
        close(s);
        return true;
    }
    if s.ui.settings.keybinding_editor.is_some()
        && s.ui.settings.open_select.is_none()
        && s.ui.settings.shortcut.is_none()
        && !s.ui.settings.binding_when.focused
    {
        let key = match &e.key {
            rune::Key::Text(s) => {
                if s == " " {
                    "space".into()
                } else {
                    s.to_lowercase()
                }
            }
            rune::Key::Command(c) => c.to_string(),
            rune::Key::Function(n) => format!("f{n}"),
            rune::Key::Enter => "enter".into(),
            rune::Key::Tab => "tab".into(),
            rune::Key::Backspace => "backspace".into(),
            rune::Key::Delete => "delete".into(),
            rune::Key::Up => "up".into(),
            rune::Key::Down => "down".into(),
            rune::Key::Left => "left".into(),
            rune::Key::Right => "right".into(),
            _ => return false,
        };
        let mut parts = Vec::new();
        if e.mods.cmd {
            parts.push("mod".to_owned());
        }
        if e.mods.ctrl && !e.mods.cmd {
            parts.push("ctrl".to_owned());
        }
        if e.mods.alt {
            parts.push("alt".to_owned());
        }
        if e.mods.shift {
            parts.push("shift".to_owned());
        }
        parts.push(key);
        s.ui.settings.shortcut = Some(parts.join("+"));
        return true;
    }
    false
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
}
