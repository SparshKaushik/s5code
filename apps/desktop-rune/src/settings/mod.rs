pub mod client;
mod controls;
mod keybindings;
mod pages;

use crate::theme::{self, T};
use crate::{App, Appearance, Route, icons};
use rune::{Cx, El, TextEdit, col, el, icon, row, text};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Default)]
pub struct Ui {
    pub open_select: Option<String>,
    pub search: TextEdit,
    pub edits: HashMap<String, TextEdit>,
    pub pending: HashMap<u64, String>,
    pub error: Option<String>,
    pub advanced: bool,
    pub keybinding_editor: Option<String>,
    pub shortcut: Option<String>,
    pub binding_target: Option<Value>,
    pub keybinding_search: TextEdit,
    pub return_route: Option<Route>,
}

pub fn appearance(client: &client::ClientSettings) -> Appearance {
    match client.values.get("appearance").and_then(Value::as_str) {
        Some("light") => Appearance::Light,
        Some("dark") => Appearance::Dark,
        _ => Appearance::System,
    }
}

pub fn value(s: &App, key: &str) -> Value {
    s.client_settings
        .values
        .get(key)
        .cloned()
        .or_else(|| {
            s.state()
                .config
                .as_ref()?
                .get("settings")?
                .get(key)
                .cloned()
        })
        .unwrap_or(Value::Null)
}

pub fn update(s: &mut App, key: &str, value: Value) {
    if s.client_settings.values.contains_key(key) || key == "appearance" {
        s.client_settings.values.insert(key.into(), value);
        if key == "appearance" {
            s.appearance = appearance(&s.client_settings);
        }
        if matches!(s.src, crate::data::Source::Live(_)) {
            s.client_settings.save();
        }
    } else {
        request(s, "server.updateSettings", json!({"patch":{key:value}}));
    }
}

pub fn request(s: &mut App, method: &str, payload: Value) {
    if let Some(net) = s.src.net() {
        let id = net.request(method, payload);
        s.ui.settings.pending.insert(id, method.into());
    }
}

pub fn poll(s: &mut App) -> bool {
    let ids: Vec<_> = s.ui.settings.pending.keys().copied().collect();
    let mut changed = false;
    for id in ids {
        if let Some(reply) = s.src.net().and_then(|n| n.take_reply(id)) {
            s.ui.settings.pending.remove(&id);
            s.ui.settings.error = reply.err().map(|e| {
                e["message"]
                    .as_str()
                    .unwrap_or("Could not update settings")
                    .to_owned()
            });
            changed = true;
        }
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

fn nav_icon(path: &str) -> &'static rune::Icon {
    match path {
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
        .w(240.0)
        .h_full()
        .fill(t.c.sidebar)
        .px(8.0)
        .pt(8.0)
        .gap(16.0);
    let search = rune::text_input(&s.ui.settings.search, theme::TEXT_SM)
        .key("settings-search")
        .single_line()
        .placeholder("Search", t.sb.muted_foreground)
        .w_full()
        .h(32.0)
        .px(8.0)
        .radius(6.0)
        .fill(t.c.sidebar)
        .color(t.sb.muted_foreground)
        .on_edit(|s: &mut App| &mut s.ui.settings.search);
    nav = nav.child(
        row()
            .h(32.0)
            .gap(6.0)
            .center_y()
            .child(icon(&icons::SEARCH, 16.0).color(t.sb.muted_foreground))
            .child(search),
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
                    .h(24.0),
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
                    .px(8.0)
                    .gap(8.0)
                    .center_y()
                    .radius(6.0)
                    .fill(if active { t.c.accent } else { t.c.sidebar })
                    .hover(t.sb.accent)
                    .focusable(true)
                    .role("link")
                    .label(*title)
                    .on_press(move |s: &mut App| {
                        s.navigate(Route::Settings(target.clone()));
                        s.ui.settings.open_select = None;
                    })
                    .child(rune::icon(icon, 16.0).color(t.sb.muted_foreground))
                    .child(text(*title).font(theme::SM_MEDIUM).color(if active {
                        t.c.foreground
                    } else {
                        t.c.muted_foreground
                    })),
            );
        }
        nav = nav.child(group);
    }
    let nav = col()
        .w(240.0)
        .h_full()
        .child(nav.scroll(rune::ScrollSpec {
            stick_bottom: false,
        }))
        .child(el().h(1.0).w_full().fill(t.c.border));
    let mut header = row()
        .h(48.0)
        .w_full()
        .px(20.0)
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
        header = header.child(controls::button("Restore device defaults", t).on_press(
            |s: &mut App| {
                let defaults = client::ClientSettings::default();
                s.client_settings.values.extend(defaults.values);
                s.client_settings
                    .values
                    .insert("appearance".into(), json!("system"));
                s.appearance = Appearance::System;
                if matches!(s.src, crate::data::Source::Live(_)) {
                    s.client_settings.save();
                }
            },
        ));
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
        .modal(true, close)
        .w(1280.0)
        .h(820.0)
        .radius(16.0)
        .clip_rounded(16.0)
        .border(1.0, t.c.border)
        .fill(t.c.background)
        .child(nav)
        .child(el().w(1.0).h_full().fill(t.c.border))
        .child(content);
    let mut overlays = Vec::new();
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
        if let Some(spec) = pages::specs()
            .as_object()
            .into_iter()
            .flat_map(|p| p.values())
            .flat_map(|p| p.as_array().into_iter().flatten())
            .flat_map(|sec| sec["rows"].as_array().into_iter().flatten())
            .find(|r| r["key"].as_str() == Some(key))
        {
            let options: Vec<_> = spec["options"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|o| (o[0].clone(), o[1].as_str().unwrap_or("").to_owned()))
                .collect();
            overlays.push(controls::select_menu(
                s,
                t,
                key,
                &options,
                if key == "notificationMode" {
                    224.0
                } else {
                    160.0
                },
            ));
        }
    }
    rune_kit::root(
        el().w_full()
            .h_full()
            .center()
            .fill(t.c.background)
            .child(modal),
        overlays,
    )
}

pub fn close(s: &mut App) {
    let route =
        s.ui.settings
            .return_route
            .take()
            .unwrap_or(Route::Home { project_id: None });
    s.navigate(route);
}

fn scene(s: &mut App, path: &str) {
    crate::scene_clock(s);
    s.client_settings = client::ClientSettings::default();
    s.route = Route::Settings(path.into());
    s.ui.settings = Ui::default();
}

pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("settings-general", |s| scene(s, "general")),
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
];

pub fn on_key(s: &mut App, e: &rune::KeyEvent, _tx: &mut rune::Tx) -> bool {
    if !matches!(s.route, Route::Settings(_)) {
        return false;
    }
    if matches!(e.key, rune::Key::Escape) {
        if s.ui.settings.open_select.take().is_some() {
            return true;
        }
        if s.ui.settings.keybinding_editor.take().is_some() {
            return true;
        }
        close(s);
        return true;
    }
    if s.ui.settings.keybinding_editor.is_some() && s.ui.settings.shortcut.is_none() {
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
