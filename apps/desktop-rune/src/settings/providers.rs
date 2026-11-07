//! The Providers page (`ProviderSettingsPanel`): the instance list on the left, the selected
//! instance's editor on the right (`ProviderInstanceCard`, `ProviderModelsSection`), and the
//! placeholders shown while the environment's configuration is missing.
//!
//! Instance writes are `server.updateSettings` with a `providerInstanceMutation`, as the web's
//! `usePersistEnvironmentProviderInstanceMutation` sends them; model preferences ride the same
//! RPC's `setModelFavorites` / `setModelsHidden` / `setProviderModelOrder` patch keys.

use super::{controls as c, request, server_settings};
use crate::chat::composer::{provider_icon, provider_icons::STAR_FILLED, providers as live_providers};
use crate::kit;
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, TextEdit, col, el, icon, row, text};
use serde_json::{Value, json};

/// Driver definitions in `providerClients` order: kind, label, whether it has a default slot.
const DRIVERS: &[(&str, &str, bool)] = &[
    ("codex", "Codex", true),
    ("claudeAgent", "Claude", true),
    ("cursor", "Cursor", true),
    ("grok", "Grok", true),
    ("fx", "fx", true),
    ("opencode", "OpenCode", true),
    ("antigravity", "Antigravity", true),
    ("kiro", "Kiro", true),
    ("muse", "Muse Code", true),
    ("pi", "Pi", true),
    ("acpRegistry", "ACP Registry", false),
];
const FILTER_THRESHOLD: usize = 8;

#[derive(Default)]
pub struct Ui {
    pub filter: TextEdit,
    pub custom: TextEdit,
    pub adding: bool,
    pub error: Option<String>,
    pub env_name: TextEdit,
    pub env_value: TextEdit,
    pub env_adding: bool,
    /// `(instance id, reset)` awaiting the remove or reset confirmation.
    pub confirm: Option<(String, bool)>,
    /// Scenes only: open the detail pane scrolled to its end, where Models sits.
    pub scene_bottom: bool,
}

pub struct Row {
    pub id: String,
    pub driver: String,
    explicit: Option<Value>,
    is_default: bool,
    dirty: bool,
}

fn label(driver: &str) -> &str {
    DRIVERS.iter().find(|(d, _, _)| *d == driver).map_or(driver, |(_, l, _)| l)
}

/// `resolveProviderInstanceEnabled`.
pub fn resolve_enabled(instance: &Value) -> bool {
    let config = instance["config"]["enabled"].as_bool();
    if instance["enabled"] == false || config == Some(false) {
        return false;
    }
    instance["enabled"].as_bool().or(config).unwrap_or_else(|| {
        !matches!(
            instance["driver"].as_str().unwrap_or(""),
            "cursor" | "grok" | "fx" | "kiro" | "muse" | "pi" | "opencode" | "antigravity"
        )
    })
}

/// The panel's rows: each driver's default slot when it is worth showing, then its other
/// instances, then instances of drivers this build doesn't know.
pub fn rows(s: &App) -> Vec<Row> {
    let cursor = live_providers(s).iter().any(|p| p["instanceId"] == "cursor");
    build_rows(&server_settings(s)["providerInstances"], cursor, s.ui.settings.provider.as_deref())
}

fn build_rows(instances: &Value, cursor_visible: bool, target: Option<&str>) -> Vec<Row> {
    let instances: Vec<(&String, &Value)> = instances.as_object().map(|m| m.iter().collect()).unwrap_or_default();
    let visible: Vec<_> = DRIVERS.iter().filter(|(d, _, _)| *d != "cursor" || cursor_visible).collect();
    let mut out = Vec::new();
    for &&(driver, _, has_default) in &visible {
        let explicit = instances.iter().find(|(id, _)| *id == driver).map(|(_, v)| (*v).clone());
        let dirty = explicit.is_some();
        let instance = explicit.clone().unwrap_or_else(|| json!({"driver": driver}));
        if (has_default || dirty)
            && (matches!(driver, "codex" | "claudeAgent") || dirty || resolve_enabled(&instance) || target == Some(driver))
        {
            out.push(Row { id: driver.to_owned(), driver: driver.to_owned(), explicit, is_default: true, dirty });
        }
        for &(id, v) in &instances {
            if id != driver && v["driver"] == driver {
                out.push(Row { id: id.clone(), driver: driver.to_owned(), explicit: Some(v.clone()), is_default: false, dirty: true });
            }
        }
    }
    for &(id, v) in &instances {
        let driver = v["driver"].as_str().unwrap_or("");
        if !visible.iter().any(|(d, _, _)| *d == driver) {
            let is_default = visible.iter().any(|(d, _, _)| d == id);
            out.push(Row { id: id.clone(), driver: driver.to_owned(), explicit: Some(v.clone()), is_default, dirty: true });
        }
    }
    out
}

/// The selected instance: the one the user picked while it still has a row, else the first.
pub fn selected_id(s: &App) -> Option<String> {
    let rows = rows(s);
    s.ui.settings
        .provider
        .as_ref()
        .filter(|id| rows.iter().any(|r| r.id == **id))
        .cloned()
        .or_else(|| rows.first().map(|r| r.id.clone()))
}

fn instance_of(s: &App, id: &str, driver: &str) -> Value {
    let v = &server_settings(s)["providerInstances"][id];
    if v.is_object() { v.clone() } else { json!({"driver": driver}) }
}

fn driver_of(s: &App, id: &str) -> String {
    rows(s).into_iter().find(|r| r.id == id).map(|r| r.driver).unwrap_or_else(|| {
        crate::chat::composer::provider(s, id).map(|p| crate::data::s(p, "driver").to_owned()).unwrap_or_default()
    })
}

fn display_name(instance: &Value) -> String {
    match instance["displayName"].as_str().map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => name.to_owned(),
        None => label(instance["driver"].as_str().unwrap_or("")).to_owned(),
    }
}

fn upsert(s: &mut App, id: &str, instance: Value, patch: Value) {
    request(
        s,
        "server.updateSettings",
        json!({"patch": patch, "providerInstanceMutation": {"operation": "upsert", "instanceId": id, "instance": instance}}),
    );
}

fn remove_instance(s: &mut App, id: &str) {
    request(s, "server.updateSettings", json!({"patch": {}, "providerInstanceMutation": {"operation": "remove", "instanceId": id}}));
}

/// Folds what `request` sends beyond a plain patch into the optimistic settings copy.
pub fn apply_extras(settings: &mut Value, payload: &Value) {
    let patch = &payload["patch"];
    if let Some(map) = settings.as_object_mut() {
        for key in ["setModelFavorites", "setModelsHidden", "setProviderModelOrder"] {
            map.remove(key);
        }
    }
    for f in patch["setModelFavorites"].as_array().into_iter().flatten() {
        let mut favorites = settings["favorites"].as_array().cloned().unwrap_or_default();
        favorites.retain(|x| !(x["provider"] == f["provider"] && x["model"] == f["model"]));
        if f["favorite"] == true {
            favorites.push(json!({"provider": f["provider"], "model": f["model"]}));
        }
        settings["favorites"] = json!(favorites);
    }
    let prefs = |settings: &mut Value, provider: &str| {
        if !settings["providerModelPreferences"].is_object() {
            settings["providerModelPreferences"] = json!({});
        }
        if !settings["providerModelPreferences"][provider].is_object() {
            settings["providerModelPreferences"][provider] = json!({"hiddenModels": [], "modelOrder": []});
        }
    };
    for h in patch["setModelsHidden"].as_array().into_iter().flatten() {
        let provider = h["provider"].as_str().unwrap_or("");
        prefs(settings, provider);
        let mut hidden = settings["providerModelPreferences"][provider]["hiddenModels"].as_array().cloned().unwrap_or_default();
        hidden.retain(|m| *m != h["model"]);
        if h["hidden"] == true {
            hidden.push(h["model"].clone());
        }
        settings["providerModelPreferences"][provider]["hiddenModels"] = json!(hidden);
    }
    if let Some(order) = patch.get("setProviderModelOrder") {
        let provider = order["provider"].as_str().unwrap_or("");
        prefs(settings, provider);
        settings["providerModelPreferences"][provider]["modelOrder"] = order["modelOrder"].clone();
    }
    let mutation = &payload["providerInstanceMutation"];
    if let Some(id) = mutation["instanceId"].as_str() {
        if !settings["providerInstances"].is_object() {
            settings["providerInstances"] = json!({});
        }
        let map = settings["providerInstances"].as_object_mut().unwrap();
        match mutation["operation"].as_str() {
            Some("remove") => {
                map.remove(id);
            }
            _ => {
                map.insert(id.to_owned(), mutation["instance"].clone());
            }
        }
    }
}

pub fn field_key(id: &str, field: &str) -> String {
    format!("provider::{id}::{field}")
}

pub fn field_value(s: &App, id: &str, field: &str) -> Value {
    let entry = &server_settings(s)["providerInstances"][id];
    if field == "displayName" || field == "enabled" { entry[field].clone() } else { entry["config"][field].clone() }
}

/// The instance after one field changes; empty text clears the field (`clearWhenEmpty: "omit"`).
pub fn config_patch(settings: &Value, id: &str, driver: &str, field: &str, value: Value) -> Value {
    let mut entry = settings["providerInstances"][id].clone();
    if !entry.is_object() {
        entry = json!({"driver": driver});
    }
    let empty = value.as_str().is_some_and(|v| v.trim().is_empty()) && field != "agentId";
    let target = if field == "displayName" || field == "enabled" {
        &mut entry
    } else {
        if !entry["config"].is_object() {
            entry["config"] = json!({});
        }
        &mut entry["config"]
    };
    if empty {
        target.as_object_mut().unwrap().remove(field);
    } else {
        target[field] = match value {
            Value::String(v) => json!(v.trim()),
            v => v,
        };
    }
    json!({"providerInstances": {id: entry}})
}

pub fn update(s: &mut App, id: &str, field: &str, value: Value) {
    let driver = driver_of(s, id);
    let previous = instance_of(s, id, &driver);
    let next = config_patch(server_settings(s), id, &driver, field, value)["providerInstances"][id].clone();
    // Turning off the text generation provider puts text generation back on its default.
    let selection = &server_settings(s)["textGenerationModelSelection"];
    let patch = if field == "enabled" && next["enabled"] == false && resolve_enabled(&previous) && selection["instanceId"] == id {
        json!({"textGenerationModelSelection": {"instanceId": "codex", "model": "gpt-6-luna", "options": [{"id": "reasoningEffort", "value": "low"}]}})
    } else {
        json!({})
    };
    upsert(s, id, next, patch);
}

fn select(s: &mut App, id: &str) {
    super::commit_edits(s, true);
    s.ui.settings.provider = Some(id.to_owned());
    let confirm = s.ui.settings.providers_ui.confirm.take();
    s.ui.settings.providers_ui = Ui { confirm, ..Ui::default() };
}

// --- Status -----------------------------------------------------------------------------------

struct Status {
    key: &'static str,
    headline: String,
    detail: Option<String>,
}

/// `getProviderStatusKey` + `getProviderSummary` for an instance with the given enablement.
fn status(live: Option<&Value>, enabled: bool) -> Status {
    let Some(p) = live else {
        return Status {
            key: if enabled { "warning" } else { "disabled" },
            headline: if enabled { "Checking provider status" } else { "Disabled" }.into(),
            detail: enabled.then(|| "Waiting for the server to report installation and authentication details.".into()),
        };
    };
    if !enabled {
        return Status { key: "disabled", headline: "Disabled".into(), detail: None };
    }
    let message = p["message"].as_str().map(str::to_owned);
    let key = match p["status"].as_str() {
        Some("ready") => "ready",
        Some("error") => "error",
        Some("disabled") => "disabled",
        _ => "warning",
    };
    let auth = &p["auth"];
    let auth_label = auth["label"].as_str().or(auth["type"].as_str());
    let with_label = |head: &str| auth_label.map_or(head.to_owned(), |l| format!("{head} · {l}"));
    let (headline, detail) = if p["status"] == "disabled" {
        ("Disabled".to_owned(), message.or(Some("This provider is installed but disabled for new sessions in T3 Code.".into())))
    } else if p["installed"] == false {
        ("Not found".to_owned(), message.or(Some("CLI not detected on PATH.".into())))
    } else if auth["status"] == "unauthenticated" {
        (with_label("Not authenticated"), message)
    } else if p["status"] == "warning" {
        ("Needs attention".to_owned(), message.or(Some("The provider is installed, but the server could not fully verify it.".into())))
    } else if p["status"] == "error" {
        ("Unavailable".to_owned(), message.or(Some("The provider failed its startup checks.".into())))
    } else if auth["status"] == "authenticated" {
        (with_label("Authenticated"), message)
    } else {
        ("Available".to_owned(), message)
    };
    Status { key, headline, detail }
}

fn compatibility_warning(live: Option<&Value>, enabled: bool) -> Option<&'static str> {
    if !enabled {
        return None;
    }
    match live?["compatibilityAdvisory"]["status"].as_str()? {
        "graceful" => Some("Limited support"),
        "unsupported" => Some("Unsupported"),
        "broken" => Some("Incompatible"),
        _ => None,
    }
}

/// Whether `getProviderVersionAdvisoryPresentation` has anything to show.
fn advisory(live: Option<&Value>, enabled: bool) -> Option<(&'static rune::Icon, &'static str)> {
    if compatibility_warning(live, enabled).is_some() {
        let title = match live?["compatibilityAdvisory"]["status"].as_str() {
            Some("graceful") => "Limited support",
            Some("unsupported") => "Unsupported version",
            _ => "Known broken version",
        };
        return Some((&icons::TRIANGLE_ALERT, title));
    }
    let latest_bad = matches!(live?["compatibilityAdvisory"]["latestVersionStatus"].as_str(), Some("broken" | "unsupported"));
    match live?["versionAdvisory"]["status"].as_str() {
        Some("current" | "unknown") | None => None,
        _ if latest_bad => None,
        _ => Some((&icons::CIRCLE_ARROW_UP, "Update available")),
    }
}

fn version_label(live: Option<&Value>) -> Option<String> {
    let v = live?["version"].as_str()?;
    Some(if v.starts_with(|c: char| c.is_ascii_digit()) { format!("v{v}") } else { v.to_owned() })
}

fn dot(key: &str, t: &T) -> Option<El> {
    let color = match key {
        "warning" => t.c.warning,
        "error" => t.c.destructive,
        _ => return None,
    };
    Some(el().size(6.0, 6.0).radius(theme::radius::FULL).fill(color))
}

// --- List -------------------------------------------------------------------------------------

fn list_row(s: &App, t: &T, r: &Row, selected: bool) -> El {
    let instance = r.explicit.clone().unwrap_or_else(|| json!({"driver": r.driver}));
    let live = crate::chat::composer::provider(s, &r.id);
    let enabled = resolve_enabled(&instance);
    let name = display_name(&instance);
    let st = status(live, enabled);
    let attention = matches!(st.key, "warning" | "error");
    let detail = compatibility_warning(live, enabled).map(str::to_owned).or(st.detail.clone());
    let line = match (&detail, attention) {
        (Some(d), true) => format!("{} · {d}", st.headline),
        _ => st.headline.clone(),
    };
    let mut title = row().w_full().gap(8.0).center_y().child(
        text(name.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate().shrink().min_w(0.0),
    );
    if let Some(v) = version_label(live) {
        title = title.child(text(v).font(theme::MONO_XS).color(t.c.muted_foreground).truncate().max_w(96.0));
    }
    if let Some((glyph, label)) = advisory(live, enabled) {
        let warn = glyph.name == "triangle-alert";
        title = title.child(
            kit::Button::new(format!("{label}, view details"))
                .variant("ghost-muted")
                .size("icon-micro")
                .build(t)
                .child(icon(glyph, 14.0).color(if warn { t.c.warning } else { t.c.muted_foreground })),
        );
    }
    let mut status_line = row().w_full().mt(2.0).gap(6.0).items_start();
    if let Some(d) = dot(st.key, t) {
        status_line = status_line.child(el().h(17.4).center_y().child(d));
    }
    status_line = status_line.child(
        text(line).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground.alpha_mul(0.8)).wrap().max_lines(2).grow(1.0).min_w(0.0),
    );
    let (id, toggle_id) = (r.id.clone(), r.id.clone());
    let body = row()
        .grow(1.0)
        .min_w(0.0)
        .gap(12.0)
        .items_start()
        .opacity(if !enabled && !selected { 0.6 } else { 1.0 })
        .focusable(true)
        .role("button")
        .label(format!("Select {name}"))
        .cursor(rune::Cursor::Pointer)
        .on_press(move |s: &mut App| select(s, &id))
        .child(el().size(20.0, 20.0).center().child(provider_icon(t, &r.driver, 16.0)))
        .child(col().grow(1.0).min_w(0.0).child(title).child(status_line));
    let mut line = row()
        .key(format!("provider-row-{}", r.id))
        .w_full()
        .min_h(72.0)
        .px(16.0)
        .py(12.0)
        .gap(12.0)
        .center_y()
        .child(body)
        .child(
            row().h(20.0).center_y().child(
                kit::Switch::new(enabled)
                    .label(format!("Enable {name}"))
                    .hide_label()
                    .on_toggle(move |s: &mut App| update(s, &toggle_id, "enabled", json!(!enabled)))
                    .build(&kit::theme(t)),
            ),
        );
    line = if selected { line.fill(t.c.foreground.alpha_mul(0.06)) } else { line.hover(t.c.muted.alpha_mul(0.25)) };
    line
}

fn list(s: &App, t: &T, rows: &[Row], selected: Option<&str>) -> El {
    let divider = || el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.5));
    let mut items = col().w_full();
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            items = items.child(divider());
        }
        items = items.child(list_row(s, t, r, selected == Some(r.id.as_str())));
    }
    if !rows.is_empty() {
        items = items.child(divider());
    }
    items = items.child(
        row()
            .key("providers-add-row")
            .w_full()
            .px(16.0)
            .py(12.0)
            .gap(12.0)
            .center_y()
            .hover(t.c.muted.alpha_mul(0.25))
            .focusable(true)
            .role("button")
            .label("Add provider")
            .cursor(rune::Cursor::Pointer)
            .on_press(|s: &mut App| s.ui.settings.provider_picker = true)
            .child(icon(&icons::PLUS, 16.0).color(t.c.muted_foreground))
            .child(text("Add provider").font(theme::TEXT_SM).color(t.c.muted_foreground)),
    );
    col()
        .w(272.0)
        .h_full()
        .fill(t.c.muted.alpha_mul(0.1))
        .child(kit::scroll_area("providers-list", col().w_full().grow(1.0).min_h(0.0).child(items).scroll(rune::ScrollSpec { stick_bottom: false }), t))
}

// --- Editor -----------------------------------------------------------------------------------

/// A `SettingsSection` inside the detail pane, which the web flattens: no card, only dividers.
fn flat_section(heading: El, action: Option<El>, children: Vec<El>, t: &T) -> El {
    let mut group = col().w_full().padding(1.0);
    for (i, child) in children.into_iter().enumerate() {
        if i > 0 {
            group = group.child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.5)));
        }
        group = group.child(child);
    }
    let mut header = row().w_full().min_h(28.0).px(16.0).gap(16.0).items_start().child(row().grow(1.0).min_w(0.0).min_h(28.0).gap(4.0).center_y().child(heading));
    if let Some(action) = action {
        header = header.child(row().min_h(28.0).min_w(28.0).center_y().justify_end().child(action));
    }
    col().w_full().gap(10.0).child(header).child(group)
}

fn heading(title: &str, t: &T) -> El {
    text(title.to_owned()).font(theme::weight(theme::TEXT_BASE, theme::SEMIBOLD)).color(t.c.foreground).truncate()
}

fn text_row(s: &App, cx: &rune::Cx<App>, t: &T, id: &str, field: &str, title: &str, description: &str, placeholder: &str) -> El {
    c::setting_row(
        s,
        cx,
        t,
        &json!({"key": field_key(id, field), "title": title, "description": description, "placeholder": placeholder, "width": 224}),
    )
}

/// `ProviderSettingsForm`'s text fields per driver: field, title, description, placeholder.
fn runtime_fields(driver: &str, config: &Value) -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    let binary = |description: &'static str, placeholder: &'static str| ("binaryPath", "Binary path", description, placeholder);
    let launch = |description: &'static str, placeholder: &'static str| ("launchArgs", "Launch arguments", description, placeholder);
    match driver {
        "codex" => vec![
            binary("Path to the Codex binary used by this instance.", "codex"),
            ("homePath", "CODEX_HOME path", "Custom Codex home and config directory.", "~/.codex"),
            (
                "shadowHomePath",
                "Shadow home path",
                "Account-specific Codex home. Keeps auth.json separate while sharing state from CODEX_HOME.",
                "~/.codex-t3/personal",
            ),
            launch("Additional CLI arguments passed to codex app-server on session start.", ""),
        ],
        "claudeAgent" => vec![
            binary("Path to the Claude binary used by this instance.", "claude"),
            (
                "homePath",
                "CLAUDE_CONFIG_DIR path",
                "Custom Claude home and config directory. Keeps .claude.json and .claude separate.",
                "~/.claude",
            ),
            launch("Additional CLI arguments passed on session start.", "e.g. --chrome"),
            (
                "autoCompactWindow",
                "Auto-compact after",
                "Compact after 100,000 to 1,000,000 tokens. Leave empty to use Claude's default.",
                "e.g. 300000",
            ),
        ],
        "opencode" => vec![
            binary("Path to the OpenCode binary.", "opencode"),
            ("serverUrl", "Server URL", "Leave blank to let T3 Code spawn the server when needed.", "http://127.0.0.1:4096"),
        ],
        "grok" => vec![binary("Path to the Grok CLI binary.", "grok")],
        "fx" => vec![binary("Path to the fx binary. All fx instances on this machine share its fx login.", "fx")],
        "kiro" => vec![binary("Path to the Kiro CLI binary.", "kiro-cli")],
        "muse" => vec![binary("Path to the Muse Code binary on this environment. Sign in with muse login on that host.", "muse")],
        "pi" => vec![
            binary("Path to the Pi coding agent binary.", "pi"),
            launch("Additional CLI arguments passed to pi --mode rpc on session start.", ""),
        ],
        "acpRegistry" => {
            let mut fields = Vec::new();
            if config["source"] != "local" {
                fields.push(("agentId", "Registry agent ID", "Agent identifier from the official ACP Registry, for example 'devin'.", "devin"));
            }
            fields.push((
                "commandPath",
                "Executable override",
                "Executable on this environment. For registry agents, this overrides the distribution executable while keeping its arguments and environment.",
                "Registry default",
            ));
            fields.push((
                "authMethodId",
                "Authentication method",
                "Optional ACP authentication method ID. By default, the first agent-managed method is selected.",
                "auto",
            ));
            fields
        }
        _ => vec![],
    }
}

fn header_action(s: &App, t: &T, r: &Row, live: Option<&Value>, enabled: bool) -> El {
    let mut action = row().gap(6.0).center_y();
    if let Some(v) = version_label(live) {
        action = action.child(text(v).font(theme::MONO_XS).color(t.c.muted_foreground));
    }
    if let Some((glyph, label)) = advisory(live, enabled) {
        let warn = glyph.name == "triangle-alert";
        action = action.child(
            kit::Button::new(format!("{label}, view details"))
                .variant("ghost-muted")
                .size("icon-xs")
                .build(t)
                .child(icon(glyph, 14.0).color(if warn { t.c.warning } else { t.c.muted_foreground })),
        );
    }
    let mut tail = row().gap(4.0).center_y();
    if r.is_default && r.dirty {
        let id = r.id.clone();
        tail = tail.child(
            el().size(20.0, 20.0).center().child(
                kit::Button::new(format!("Reset {} provider settings", label(&r.driver)))
                    .variant("ghost-muted")
                    .size("icon-micro")
                    .icon(&icons::UNDO_2)
                    .on_press(move |s: &mut App| s.ui.settings.providers_ui.confirm = Some((id.clone(), true)))
                    .build(t),
            ),
        );
    }
    if !r.is_default {
        let id = r.id.clone();
        tail = tail.child(
            kit::Button::new(format!("Delete instance {}", r.id))
                .variant("ghost-destructive")
                .size("icon-xs")
                .icon(&icons::TRASH_2)
                .on_press(move |s: &mut App| s.ui.settings.providers_ui.confirm = Some((id.clone(), false)))
                .build(t),
        );
    }
    action.child(tail)
}

fn status_row(t: &T, live: Option<&Value>, enabled: bool) -> El {
    let st = status(live, enabled);
    let attention = matches!(st.key, "warning" | "error");
    let detail = compatibility_warning(live, enabled).map(str::to_owned).or(st.detail.clone());
    let email = live.and_then(|p| p["auth"]["email"].as_str()).map(str::trim).filter(|e| !e.is_empty());
    let authenticated = enabled && live.is_some_and(|p| p["auth"]["status"] == "authenticated");
    let mut parts = Vec::new();
    match email.filter(|_| authenticated) {
        Some(email) => {
            parts.push(format!("Authenticated as {email}"));
            if let Some(l) = live.and_then(|p| p["auth"]["label"].as_str().or(p["auth"]["type"].as_str())) {
                parts.push(l.to_owned());
            }
        }
        None => parts.push(st.headline.clone()),
    }
    parts.extend(detail);
    let mut line = row().w_full().pt(2.0).gap(6.0).items_start();
    if let Some(d) = dot(st.key, t).filter(|_| attention) {
        line = line.child(el().h(16.0).center_y().child(d));
    }
    line = line.child(text(parts.join(" · ")).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().grow(1.0).min_w(0.0));
    col()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(4.0)
        .child(row().min_h(20.0).center_y().child(text("Status").font(theme::SM_MEDIUM).color(t.c.foreground)))
        .child(line)
}

fn small_input(edit: &TextEdit, key: &str, placeholder: &str, t: &T, mono: bool) -> El {
    rune::text_input(edit, if mono { theme::MONO_SM } else { theme::TEXT_SM })
        .key(key.to_owned())
        .placeholder(placeholder, t.c.muted_foreground.alpha_mul(0.72))
        .h(28.0)
        .px(9.0)
        .py(4.0)
        .radius(8.0)
        .border(1.0, t.c.input)
        .fill(t.c.input.alpha_mul(if t.dark { 0.32 } else { 0.0 }))
        .color(t.c.foreground)
        .single_line()
}

fn environment_row(s: &App, t: &T, id: &str, instance: &Value) -> El {
    let ui = &s.ui.settings.providers_ui;
    let variables = instance["environment"].as_array().cloned().unwrap_or_default();
    let line = row()
        .w_full()
        .gap(32.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(row().min_h(20.0).center_y().child(text("Variables").font(theme::SM_MEDIUM).color(t.c.foreground)))
                .child(
                    text("API keys, base URLs, and other per-instance CLI settings.")
                        .font(theme::TEXT_XS.lh(18.0))
                        .color(t.c.muted_foreground)
                        .wrap(),
                ),
        )
        .child(
            kit::Button::new("Add variable")
                .variant("outline")
                .size("sm")
                .icon(&icons::PLUS)
                .on_press(|s: &mut App| s.ui.settings.providers_ui.env_adding = true)
                .build(t),
        );
    if variables.is_empty() && !ui.env_adding {
        return line.px(16.0).py(12.0);
    }
    let mut list = col().w_full().mt(12.0).pb(8.0).gap(8.0);
    for (index, v) in variables.iter().enumerate() {
        let name = v["name"].as_str().unwrap_or("").to_owned();
        let shown = if v["sensitive"] == true || v["valueRedacted"] == true || !v["value"].is_string() {
            "Stored separately".to_owned()
        } else {
            v["value"].as_str().unwrap_or("").to_owned()
        };
        let remove_id = id.to_owned();
        list = list.child(
            row()
                .w_full()
                .gap(6.0)
                .center_y()
                .padding(8.0)
                .radius(theme::radius::LG)
                .border(1.0, t.c.border)
                .child(text(name.clone()).font(theme::MONO_XS).color(t.c.foreground).truncate().w(176.0))
                .child(text(shown).font(theme::MONO_XS).color(t.c.muted_foreground).truncate().grow(1.0).min_w(0.0))
                .child(
                    kit::Button::new(format!("Remove environment variable {}", if name.is_empty() { (index + 1).to_string() } else { name }))
                        .variant("ghost-destructive")
                        .size("icon-micro")
                        .icon(&icons::X)
                        .on_press(move |s: &mut App| {
                            let driver = driver_of(s, &remove_id);
                            let mut next = instance_of(s, &remove_id, &driver);
                            let mut env = next["environment"].as_array().cloned().unwrap_or_default();
                            if index < env.len() {
                                env.remove(index);
                            }
                            if env.is_empty() {
                                if let Some(m) = next.as_object_mut() {
                                    m.remove("environment");
                                }
                            } else {
                                next["environment"] = json!(env);
                            }
                            upsert(s, &remove_id, next, json!({}));
                        })
                        .build(t),
                ),
        );
    }
    if ui.env_adding {
        let commit_id = id.to_owned();
        let commit = move |s: &mut App| {
            let ui = &s.ui.settings.providers_ui;
            let name = ui.env_name.text.trim().to_owned();
            if name.is_empty() {
                return;
            }
            let value = ui.env_value.text.clone();
            let driver = driver_of(s, &commit_id);
            let mut next = instance_of(s, &commit_id, &driver);
            let mut env = next["environment"].as_array().cloned().unwrap_or_default();
            env.push(json!({"name": name, "value": value, "sensitive": false}));
            next["environment"] = json!(env);
            upsert(s, &commit_id, next, json!({}));
            let ui = &mut s.ui.settings.providers_ui;
            ui.env_adding = false;
            ui.env_name.clear();
            ui.env_value.clear();
        };
        let commit_name = commit.clone();
        list = list.child(
            col()
                .w_full()
                .gap(6.0)
                .padding(8.0)
                .radius(theme::radius::LG)
                .border(1.0, t.c.border)
                .child(
                    row()
                        .w_full()
                        .gap(6.0)
                        .center_y()
                        .child(
                            small_input(&ui.env_name, "provider-env-name", "VARIABLE_NAME", t, true)
                                .grow(1.0)
                                .min_w(0.0)
                                .on_edit(|s: &mut App| &mut s.ui.settings.providers_ui.env_name)
                                .on_submit(move |s: &mut App, _| commit_name(s)),
                        )
                        .child(
                            kit::Button::new("Remove environment variable")
                                .variant("ghost-destructive")
                                .size("icon-micro")
                                .icon(&icons::X)
                                .on_press(|s: &mut App| {
                                    let ui = &mut s.ui.settings.providers_ui;
                                    ui.env_adding = false;
                                    ui.env_name.clear();
                                    ui.env_value.clear();
                                })
                                .build(t),
                        ),
                )
                .child(
                    small_input(&ui.env_value, "provider-env-value", "value", t, true)
                        .w_full()
                        .on_edit(|s: &mut App| &mut s.ui.settings.providers_ui.env_value)
                        .on_submit(move |s: &mut App, _| commit(s)),
                ),
        );
    }
    col().w_full().px(16.0).pt(12.0).pb(4.0).child(line).child(list)
}

// --- Models -----------------------------------------------------------------------------------

struct Model {
    slug: String,
    name: String,
    custom: bool,
    caps: Value,
}

fn custom_entries(config: &Value) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for raw in config["customModels"].as_array().into_iter().flatten() {
        let (slug, name) = match raw {
            Value::String(slug) => (slug.trim().to_owned(), None),
            v => (v["slug"].as_str().unwrap_or("").trim().to_owned(), v["name"].as_str().map(str::to_owned)),
        };
        if slug.is_empty() || out.iter().any(|(s, _)| *s == slug) {
            continue;
        }
        out.push((slug.clone(), name.filter(|n| !n.trim().is_empty()).unwrap_or(slug)));
    }
    out
}

/// `deriveProviderModelsForDisplay`: server models, then saved custom ones it doesn't report.
fn models(live: Option<&Value>, instance: &Value) -> Vec<Model> {
    let live_models = live.and_then(|p| p["models"].as_array()).cloned().unwrap_or_default();
    let mut out: Vec<Model> = live_models
        .iter()
        .filter(|m| m["isCustom"] != true)
        .map(|m| Model {
            slug: crate::data::s(m, "slug").to_owned(),
            name: crate::data::s(m, "name").to_owned(),
            custom: false,
            caps: m["capabilities"].clone(),
        })
        .collect();
    if instance["driver"] != "antigravity" {
        for (slug, name) in custom_entries(&instance["config"]) {
            if out.iter().any(|m| m.slug == slug) {
                continue;
            }
            let caps = live_models.iter().find(|m| m["isCustom"] == true && m["slug"] == slug.as_str()).map_or(Value::Null, |m| m["capabilities"].clone());
            out.push(Model { slug, name, custom: true, caps });
        }
    }
    out
}

fn has_reasoning(m: &Model) -> bool {
    m.caps["optionDescriptors"].as_array().into_iter().flatten().any(|d| {
        (d["type"] == "select" && d["role"] == "reasoning")
            || matches!(
                d["id"].as_str(),
                Some("reasoningEffort" | "effort" | "reasoning" | "variant" | "reasoning_effort" | "thinking_level" | "thinking")
            )
    })
}

/// `describeModelCapabilities`.
fn capabilities(m: &Model, flag_missing_reasoning: bool) -> String {
    let descriptors = m.caps["optionDescriptors"].as_array().cloned().unwrap_or_default();
    let mut labels = Vec::new();
    if descriptors.iter().any(|d| {
        d["id"] == "fastMode"
            || (d["id"] == "serviceTier"
                && d["type"] == "select"
                && d["options"].as_array().into_iter().flatten().any(|o| o["id"] == "fast" || o["label"] == "Fast"))
    }) {
        labels.push("Fast mode");
    }
    if flag_missing_reasoning && !m.custom && !m.caps.is_null() && !matches!(m.slug.as_str(), "default" | "auto") && !has_reasoning(m) {
        labels.push("No reasoning");
    }
    labels.join(" · ")
}

fn preferences(s: &App, id: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let settings = server_settings(s);
    let strings = |v: &Value| v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_owned)).collect::<Vec<_>>();
    let prefs = &settings["providerModelPreferences"][id];
    let favorites = settings["favorites"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["provider"] == id)
        .filter_map(|f| f["model"].as_str().map(str::to_owned))
        .collect();
    (favorites, strings(&prefs["hiddenModels"]), strings(&prefs["modelOrder"]))
}

#[derive(Clone, Copy, PartialEq)]
enum Group {
    Favorite,
    Visible,
    Hidden,
}

/// `groupModelsForDisplay`: favorites, then shown models, then hidden ones, each in saved order.
fn grouped(models: Vec<Model>, favorites: &[String], hidden: &[String], order: &[String]) -> Vec<(Model, Group)> {
    let mut ranked: Vec<(usize, Model)> = models.into_iter().enumerate().collect();
    ranked.sort_by_key(|(i, m)| {
        (!favorites.contains(&m.slug), order.iter().position(|o| *o == m.slug).unwrap_or(usize::MAX), *i)
    });
    let group = |m: &Model| {
        if favorites.contains(&m.slug) {
            Group::Favorite
        } else if !m.custom && hidden.contains(&m.slug) {
            Group::Hidden
        } else {
            Group::Visible
        }
    };
    let mut out: Vec<(Model, Group)> = ranked.into_iter().map(|(_, m)| { let g = group(&m); (m, g) }).collect();
    out.sort_by_key(|(_, g)| match g {
        Group::Favorite => 0,
        Group::Visible => 1,
        Group::Hidden => 2,
    });
    out
}

fn preference(s: &mut App, patch: Value) {
    request(s, "server.updateSettings", json!({"patch": patch}));
}

fn add_custom_model(s: &mut App, id: &str) {
    let driver = driver_of(s, id);
    let instance = instance_of(s, id, &driver);
    let live = crate::chat::composer::provider(s, id).cloned();
    let slug = s.ui.settings.providers_ui.custom.text.trim().to_owned();
    let error = if slug.is_empty() {
        Some("Enter a model slug.".to_owned())
    } else if models(live.as_ref(), &instance).iter().any(|m| !m.custom && m.slug == slug) {
        Some("That model is already built in.".to_owned())
    } else if slug.chars().count() > 256 {
        Some("Model slugs must be 256 characters or less.".to_owned())
    } else if custom_entries(&instance["config"]).iter().any(|(s, _)| *s == slug) {
        Some("That custom model is already saved.".to_owned())
    } else {
        None
    };
    if let Some(error) = error {
        s.ui.settings.providers_ui.error = Some(error);
        return;
    }
    let mut next = instance;
    if !next["config"].is_object() {
        next["config"] = json!({});
    }
    let mut saved = next["config"]["customModels"].as_array().cloned().unwrap_or_default();
    saved.push(json!(slug));
    next["config"]["customModels"] = json!(saved);
    upsert(s, id, next, json!({}));
    let ui = &mut s.ui.settings.providers_ui;
    ui.custom.clear();
    ui.filter.clear();
    ui.error = None;
    ui.adding = false;
}

fn remove_custom_model(s: &mut App, id: &str, slug: &str) {
    let driver = driver_of(s, id);
    let mut next = instance_of(s, id, &driver);
    let saved: Vec<Value> = next["config"]["customModels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|raw| raw.as_str().or(raw["slug"].as_str()).map(str::trim) != Some(slug))
        .cloned()
        .collect();
    next["config"]["customModels"] = json!(saved);
    upsert(s, id, next, json!({}));
    let (favorites, _, order) = preferences(s, id);
    if order.iter().any(|o| o == slug) {
        let order: Vec<_> = order.into_iter().filter(|o| o != slug).collect();
        preference(s, json!({"setProviderModelOrder": {"provider": id, "modelOrder": order}}));
    }
    if favorites.iter().any(|f| f == slug) {
        preference(s, json!({"setModelFavorites": [{"provider": id, "model": slug, "favorite": false}]}));
    }
    s.ui.settings.providers_ui.error = None;
}

fn models_section(s: &App, t: &T, id: &str, driver: &str, live: Option<&Value>, instance: &Value) -> El {
    let ui = &s.ui.settings.providers_ui;
    let (favorites, hidden, order) = preferences(s, id);
    let all = models(live, instance);
    let flag_missing_reasoning = all.iter().any(has_reasoning);
    let count = all.len();
    let built_in: Vec<String> = all.iter().filter(|m| !m.custom).map(|m| m.slug.clone()).collect();
    let display = grouped(all, &favorites, &hidden, &order);
    let favorite_count = display.iter().filter(|(_, g)| *g == Group::Favorite).count();
    let hidden_count = display.iter().filter(|(m, _)| !m.custom && hidden.contains(&m.slug)).count();
    let show_filter = count > FILTER_THRESHOLD;
    let query = ui.filter.text.trim().to_lowercase();
    let filtering = show_filter && !query.is_empty();
    let antigravity = driver == "antigravity";

    let mut summary = row().gap(8.0).center_y();
    if !built_in.is_empty() {
        let all_hidden = built_in.iter().all(|m| hidden.contains(m));
        let (owned, slugs) = (id.to_owned(), built_in.clone());
        summary = summary.child(
            kit::Button::new(if all_hidden { "Enable all" } else { "Disable all" })
                .variant("ghost-muted")
                .size("xs")
                .on_press(move |s: &mut App| {
                    let changes: Vec<_> = slugs.iter().map(|m| json!({"provider": owned, "model": m, "hidden": !all_hidden})).collect();
                    preference(s, json!({"setModelsHidden": changes}));
                })
                .build(t),
        );
    }
    let mut copy = format!("{count} model{}", if count == 1 { "" } else { "s" });
    if favorite_count > 0 {
        copy += &format!(" · {favorite_count} favorite{}", if favorite_count == 1 { "" } else { "s" });
    }
    if hidden_count > 0 {
        copy += &format!(" · {hidden_count} hidden");
    }
    summary = summary.child(text(copy).font(theme::TEXT_XS).color(t.c.muted_foreground));
    let add = (!antigravity && !ui.adding).then(|| {
        kit::Button::new("Add custom model")
            .variant("ghost-muted")
            .size("xs")
            .icon(&icons::PLUS)
            .on_press(|s: &mut App| s.ui.settings.providers_ui.adding = true)
            .build(t)
            .mr(-8.0)
    });
    // `flex-wrap justify-between`: the filter and summary share a line; Add wraps under them.
    let mut toolbar = col().w_full().gap(8.0);
    if show_filter {
        toolbar = toolbar.child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    small_input(&ui.filter, "provider-models-filter", "Filter models", t, false)
                        .w(224.0)
                        .label("Filter models")
                        .on_edit(|s: &mut App| &mut s.ui.settings.providers_ui.filter),
                )
                .child(el().grow(1.0))
                .child(summary),
        );
        if let Some(add) = add {
            toolbar = toolbar.child(row().w_full().child(el().grow(1.0)).child(add));
        }
    } else {
        let mut line = row().w_full().gap(8.0).center_y().child(summary).child(el().grow(1.0));
        if let Some(add) = add {
            line = line.child(add);
        }
        toolbar = toolbar.child(line);
    }

    let visible: Vec<usize> = (0..display.len())
        .filter(|&i| {
            let m = &display[i].0;
            !filtering || m.name.to_lowercase().contains(&query) || m.slug.to_lowercase().contains(&query)
        })
        .collect();
    let mut list = col().mt(8.0).mx(-8.0).self_stretch();
    if visible.is_empty() {
        list = list.child(
            text(if filtering { "No models match." } else { "No models reported for this provider yet." })
                .font(theme::TEXT_XS)
                .color(t.c.muted_foreground)
                .px(8.0)
                .py(8.0),
        );
    }
    let slugs: Vec<String> = display.iter().map(|(m, _)| m.slug.clone()).collect();
    for (n, &i) in visible.iter().enumerate() {
        let (m, group) = &display[i];
        let starts = n == 0 || display[visible[n - 1]].1 != *group;
        let label = match group {
            Group::Favorite if favorite_count > 0 => Some("Favorites"),
            Group::Visible if favorite_count > 0 => Some("All"),
            Group::Hidden => Some("Hidden from picker"),
            _ => None,
        };
        if let Some(label) = label.filter(|_| starts) {
            list = list.child(
                text(label).font(theme::TEXT_2XS).color(t.c.muted_foreground).px(8.0).pt(if n == 0 { 4.0 } else { 20.0 }).pb(6.0),
            );
        }
        let is_hidden = *group == Group::Hidden;
        let is_favorite = *group == Group::Favorite;
        let can_up = !filtering && i > 0 && display[i - 1].1 == *group;
        let can_down = !filtering && i + 1 < display.len() && display[i + 1].1 == *group;
        let (fav_id, fav_slug) = (id.to_owned(), m.slug.clone());
        let star = kit::Button::new(format!("{} {} {} favorites", if is_favorite { "Remove" } else { "Add" }, m.name, if is_favorite { "from" } else { "to" }))
            .variant("ghost-muted")
            .size("icon-micro")
            .on_press(move |s: &mut App| {
                preference(s, json!({"setModelFavorites": [{"provider": fav_id, "model": fav_slug, "favorite": !is_favorite}]}));
            })
            .build(t)
            .child(if is_favorite {
                icon(&STAR_FILLED, 12.0).color(t.c.warning)
            } else {
                icon(&icons::STAR, 12.0).color(t.c.muted_foreground)
            });
        let mut name = row()
            .grow(1.0)
            .min_w(0.0)
            .gap(8.0)
            .center_y()
            .child(text(m.name.clone()).font(theme::TEXT_XS).color(if is_hidden { t.c.muted_foreground } else { t.c.foreground.alpha_mul(0.9) }).truncate());
        if m.name != m.slug {
            name = name.child(text(m.slug.clone()).font(theme::mono(theme::TEXT_2XS)).color(t.c.muted_foreground.alpha_mul(0.7)).truncate().shrink().min_w(0.0));
        }
        if m.custom {
            name = name.child(text("custom").font(theme::TEXT_2XS).color(t.c.muted_foreground.alpha_mul(0.7)));
        }
        let mut actions = row().min_w(88.0).gap(2.0).center_y().justify_end();
        if !is_hidden && !filtering {
            for (up, enabled) in [(true, can_up), (false, can_down)] {
                let (owned, order) = (id.to_owned(), slugs.clone());
                actions = actions.child(
                    kit::Button::new(format!("Move {} {}", m.name, if up { "up" } else { "down" }))
                        .variant("ghost-muted")
                        .size("icon-micro")
                        .icon(if up { &icons::ARROW_UP } else { &icons::ARROW_DOWN })
                        .disabled(!enabled)
                        .on_press(move |s: &mut App| {
                            let mut next = order.clone();
                            let j = if up { i - 1 } else { i + 1 };
                            next.swap(i, j);
                            preference(s, json!({"setProviderModelOrder": {"provider": owned, "modelOrder": next}}));
                        })
                        .build(t),
                );
            }
        }
        if m.custom {
            let (owned, slug) = (id.to_owned(), m.slug.clone());
            actions = actions.child(
                kit::Button::new(format!("Remove {}", m.slug))
                    .variant("ghost-muted")
                    .size("icon-micro")
                    .icon(&icons::X)
                    .on_press(move |s: &mut App| remove_custom_model(s, &owned, &slug))
                    .build(t),
            );
        }
        let (show_id, show_slug) = (id.to_owned(), m.slug.clone());
        list = list.child(
            row()
                .key(format!("model-row-{}", m.slug))
                .w_full()
                .h(28.0)
                .px(8.0)
                .gap(8.0)
                .center_y()
                .radius(theme::radius::MD)
                .hover(t.c.muted.alpha_mul(0.3))
                .opacity(if is_hidden { 0.5 } else { 1.0 })
                .child(row().w(24.0).center_y().child(star))
                .child(name)
                .child(text(capabilities(m, flag_missing_reasoning)).font(theme::TEXT_2XS).color(t.c.muted_foreground.alpha_mul(0.7)))
                .child(actions)
                .child(
                    kit::Switch::new(!is_hidden)
                        .label(format!("Show {} in the model picker", m.name))
                        .hide_label()
                        .size("sm")
                        .disabled(m.custom)
                        .on_toggle(move |s: &mut App| {
                            preference(s, json!({"setModelsHidden": [{"provider": show_id, "model": show_slug, "hidden": !is_hidden}]}));
                        })
                        .build(&kit::theme(t)),
                ),
        );
    }

    let mut body = col()
        .w_full()
        .px(16.0)
        .py(12.0)
        .child(
            text("Model preferences and custom models are saved on the selected environment and shared across connected devices.")
                .font(theme::TEXT_XS)
                .color(t.c.muted_foreground)
                .wrap()
                .mb(12.0),
        )
        .child(toolbar)
        .child(list);
    if !antigravity && ui.adding {
        let placeholder = match driver {
            "codex" => "gpt-6.7-codex-ultra-preview",
            "claudeAgent" => "claude-sonnet-5",
            "cursor" => "claude-sonnet-4-6",
            "opencode" => "openai/gpt-5",
            "pi" => "anthropic/claude-sonnet-5",
            _ => "model-slug",
        };
        let (submit, press) = (id.to_owned(), id.to_owned());
        let cancel = |s: &mut App| {
            let ui = &mut s.ui.settings.providers_ui;
            ui.custom.clear();
            ui.error = None;
            ui.adding = false;
        };
        body = body.child(
            row()
                .w_full()
                .mt(12.0)
                .gap(8.0)
                .center_y()
                .child(
                    small_input(&ui.custom, "provider-custom-model", placeholder, t, false)
                        .grow(1.0)
                        .min_w(0.0)
                        .on_edit(|s: &mut App| {
                            s.ui.settings.providers_ui.error = None;
                            &mut s.ui.settings.providers_ui.custom
                        })
                        .on_submit(move |s: &mut App, _| add_custom_model(s, &submit)),
                )
                .child(kit::Button::new("Add").variant("outline").size("sm").on_press(move |s: &mut App| add_custom_model(s, &press)).build(t))
                .child(kit::Button::new("Cancel").variant("ghost").size("sm").on_press(cancel).build(t)),
        );
    }
    if let Some(error) = ui.error.as_ref().filter(|_| !antigravity) {
        body = body.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).mt(8.0));
    }
    body
}

fn editor(s: &App, cx: &rune::Cx<App>, t: &T, r: &Row) -> Vec<El> {
    let instance = instance_of(s, &r.id, &r.driver);
    let live = crate::chat::composer::provider(s, &r.id);
    let enabled = resolve_enabled(&instance);
    let name = display_name(&instance);
    let mut sections = vec![flat_section(
        row()
            .gap(8.0)
            .center_y()
            .child(el().size(20.0, 20.0).center().child(provider_icon(t, &r.driver, 16.0)))
            .child(heading(&name, t)),
        Some(header_action(s, t, r, live, enabled)),
        vec![
            status_row(t, live, enabled),
            text_row(s, cx, t, &r.id, "displayName", "Display name", "", label(&r.driver)),
        ],
        t,
    )];
    if let Some(p) = live.filter(|p| p["setup"]["canAuthenticate"] == true) {
        sections.push(flat_section(heading("Setup", t), None, vec![super::auth::view(s, t, p)], t));
    }
    let fields = runtime_fields(&r.driver, &instance["config"]);
    if !fields.is_empty() || !DRIVERS.iter().any(|(d, _, _)| *d == r.driver) {
        let rows = if fields.is_empty() {
            vec![
                col()
                    .w_full()
                    .px(16.0)
                    .py(12.0)
                    .gap(4.0)
                    .child(text("Driver").font(theme::SM_MEDIUM).color(t.c.foreground))
                    .child(
                        text(format!("This instance uses {}, which is not available in this build. Its configuration is preserved.", r.driver))
                            .font(theme::TEXT_XS.lh(18.0))
                            .color(t.c.muted_foreground)
                            .wrap(),
                    ),
            ]
        } else {
            fields.into_iter().map(|(field, title, description, placeholder)| text_row(s, cx, t, &r.id, field, title, description, placeholder)).collect()
        };
        sections.push(flat_section(heading("Runtime", t), None, rows, t));
    }
    sections.push(flat_section(heading("Environment", t), None, vec![environment_row(s, t, &r.id, &instance)], t));
    if DRIVERS.iter().any(|(d, _, _)| *d == r.driver) {
        sections.push(flat_section(heading("Models", t), None, vec![models_section(s, t, &r.id, &r.driver, live, &instance)], t));
    }
    sections
}

// --- Page -------------------------------------------------------------------------------------

/// `ProviderLastChecked`: the newest `checkedAt` among the environment's providers.
fn last_checked(s: &App) -> Option<String> {
    let newest = live_providers(s).iter().filter_map(|p| crate::data::ts(p, "checkedAt")).max()?;
    let secs = (s.now_ms - newest).max(0) / 1000;
    let (m, h) = (secs / 60, secs / 3600);
    Some(if secs < 60 {
        "Checked just now".into()
    } else if m < 60 {
        format!("Checked {m}m ago")
    } else if h < 24 {
        format!("Checked {h}h ago")
    } else {
        format!("Checked {}d ago", h / 24)
    })
}

/// `providerCardHeightClassName`: `min(44rem, 100dvh - 11rem)`, at least `32rem`.
fn card_height(cx: &rune::Cx<App>) -> f32 {
    let viewport = cx.layout("settings-modal").map_or(900.0, |r| r[3] + 80.0);
    (viewport - 176.0).min(704.0).max(512.0)
}

fn card(height: f32, t: &T) -> El {
    row()
        .w_full()
        .h(height)
        .padding(1.0)
        .radius(theme::radius::XL)
        .border(1.0, t.c.border.alpha_mul(0.6))
        .fill(t.c.card.alpha_mul(0.4))
        .clip_rounded(theme::radius::XL)
}

/// `ProviderSettingsPlaceholder`: an Empty state inside the card.
fn placeholder(cx: &rune::Cx<App>, t: &T, title: &str, description: &str, error: Option<&str>) -> El {
    let media = |rotate: f32, origin: f32| {
        el().size(36.0, 36.0)
            .absolute(0.0, 0.0)
            .radius(theme::radius::MD)
            .border(1.0, t.c.border)
            .fill(t.c.card)
            .origin(origin, 1.0)
            .rotate(rotate)
            .scale(0.84)
    };
    let icon_box = el()
        .size(36.0, 36.0)
        .mb(24.0)
        .child(media(-10f32.to_radians(), 0.0).offset(-2.0, -1.0))
        .child(media(10f32.to_radians(), 1.0).offset(2.0, -1.0))
        .child(
            el().size(36.0, 36.0)
                .absolute(0.0, 0.0)
                .center()
                .radius(theme::radius::MD)
                .border(1.0, t.c.border)
                .fill(t.c.card)
                .shadow(1.0, 2.0, rune::Color::BLACK.alpha(0.05))
                .child(icon(&icons::SERVER, 18.0).color(t.c.foreground)),
        );
    let mut empty = col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .padding(48.0)
        .gap(24.0)
        .center()
        .child(
            col()
                .max_w(384.0)
                .items_center()
                .child(icon_box)
                .child(text(title.to_owned()).font(theme::weight(theme::TEXT_XL, theme::SEMIBOLD)).color(t.c.foreground).align(rune::Align::Center))
                .child(text(description.to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap().align(rune::Align::Center).mt(4.0)),
        );
    if let Some(error) = error {
        empty = empty.child(col().w_full().max_w(576.0).child(text(error.to_owned()).font(theme::MONO_XS.lh(19.5)).color(t.c.muted_foreground).wrap()));
    }
    card(card_height(cx), t).child(empty)
}

fn section(title_action: Option<El>, header_action: Option<El>, body: El, t: &T) -> El {
    let mut left = row().grow(1.0).min_w(0.0).gap(4.0).center_y().child(row().min_h(28.0).center_y().child(heading("Providers", t)));
    if let Some(action) = title_action {
        left = left.child(action);
    }
    let mut header = row().w_full().min_h(28.0).px(16.0).gap(16.0).items_start().child(left);
    if let Some(action) = header_action {
        header = header.child(row().min_h(28.0).min_w(28.0).center_y().justify_end().child(action));
    }
    col().w_full().gap(10.0).child(header).child(body)
}

pub fn view(s: &App, cx: &rune::Cx<App>, t: &T) -> El {
    if s.state().config.is_none() {
        let (title, description, error) = match &s.state().conn {
            s5_net::Conn::Closed(reason) => ("Could not connect to this device", "Failed to connect. Reconnecting...", Some(reason.as_str())),
            s5_net::Conn::Connecting => ("Loading provider settings", "Waiting for this device's configuration.", None),
            _ => ("Provider settings are unavailable", "Connect an environment to set up its providers.", None),
        };
        return section(None, None, placeholder(cx, t, title, description, error), t);
    }
    let rows = rows(s);
    let selected = selected_id(s);
    let refreshing = s.ui.settings.pending.values().any(|m| m == "server.refreshProviders");
    let refresh_label = if refreshing { Some("Refreshing providers".to_owned()) } else { last_checked(s) };
    let mut refresh = kit::Button::new(refresh_label.clone().unwrap_or_default())
        .variant("ghost-muted")
        .size(if refresh_label.is_some() { "xs" } else { "icon-xs" })
        .icon(&icons::REFRESH_CW)
        .disabled(refreshing);
    if !refreshing {
        refresh = refresh.on_press(|s: &mut App| request(s, "server.refreshProviders", json!({"refreshModels": true, "reloadSecrets": true})));
    }
    let add = kit::Button::new("Add provider")
        .variant("ghost-muted")
        .size("icon-xs")
        .icon(&icons::PLUS)
        .on_press(|s: &mut App| s.ui.settings.provider_picker = true)
        .build(t);
    let detail = match rows.iter().find(|r| Some(&r.id) == selected.as_ref()) {
        Some(r) => kit::scroll_area(
            "provider-detail",
            col().w_full().h_full().scroll(rune::ScrollSpec { stick_bottom: s.ui.settings.providers_ui.scene_bottom }).child(col().key(format!("provider-detail-{}", r.id)).w_full().py(16.0).gap(24.0).children(editor(s, cx, t, r))),
            t,
        ),
        None => col().w_full().padding(24.0).child(
            text(if s.ui.settings.provider.is_some() && !rows.is_empty() {
                "This provider instance is no longer available on this device."
            } else {
                "No providers configured."
            })
            .font(theme::TEXT_SM)
            .color(t.c.muted_foreground),
        ),
    };
    let body = card(card_height(cx), t)
        .child(list(s, t, &rows, selected.as_deref()))
        .child(el().w(1.0).h_full().fill(t.c.border.alpha_mul(0.6)))
        .child(col().grow(1.0).min_w(0.0).h_full().child(detail));
    section(Some(add), Some(row().mr(-8.0).child(refresh.build(t))), body, t)
}

/// The remove and reset confirmations (`confirmProviderInstanceRemoval`).
pub fn confirm(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some((id, reset)) = s.ui.settings.providers_ui.confirm.clone() else { return };
    let driver = driver_of(s, &id);
    let instance = instance_of(s, &id, &driver);
    let name = display_name(&instance);
    let (title, verb) = if reset {
        (format!("Reset the {name} provider to its defaults?"), "Reset")
    } else {
        (format!("Remove the {name} provider instance?"), "Remove")
    };
    let turns_off = reset && resolve_enabled(&instance) && !resolve_enabled(&json!({"driver": driver}));
    let description = format!(
        "Its saved settings, including any custom binary path, will be removed.{}",
        if turns_off { " It will also be turned off." } else { "" }
    );
    let close = |s: &mut App| s.ui.settings.providers_ui.confirm = None;
    kit::Dialog::new("provider-confirm", title)
        .open(true)
        .alert()
        .description(description)
        .on_close(close)
        .child(
            row()
                .gap(8.0)
                .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                .child(
                    kit::Button::new(verb)
                        .variant("destructive")
                        .on_press(move |s: &mut App| {
                            remove_instance(s, &id);
                            s.ui.settings.providers_ui.confirm = None;
                        })
                        .build(t),
                ),
        )
        .build(t, layers);
}

pub fn picker(s: &App, t: &T, layers: &mut Vec<El>) {
    if !s.ui.settings.provider_picker {
        return;
    }
    let mut choices = col().w_full().gap(8.0);
    for p in live_providers(s) {
        let id = p["instanceId"].as_str().unwrap_or("").to_owned();
        let enable = p["enabled"] == false;
        choices = choices.child(
            c::button(&format!("{} {}", if enable { "Enable" } else { "Disable" }, p["displayName"].as_str().unwrap_or(&id)), t).on_press(
                move |s: &mut App| {
                    update(s, &id, "enabled", json!(enable));
                    s.ui.settings.provider = enable.then(|| id.clone());
                    s.ui.settings.provider_picker = false;
                },
            ),
        );
    }
    kit::Dialog::new("add-provider", "Add provider")
        .open(true)
        .on_close(|s: &mut App| s.ui.settings.provider_picker = false)
        .child(choices)
        .build(t, layers);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_edit_preserves_driver_environment_and_other_config_fields() {
        let settings = json!({"providerInstances":{"custom":{"driver":"codex","environment":[{"name":"FOO","value":"bar"}],"config":{"homePath":"/custom","future":true}}}});
        let patch = config_patch(&settings, "custom", "codex", "binaryPath", json!("/bin/codex"));
        assert_eq!(patch["providerInstances"]["custom"]["environment"], settings["providerInstances"]["custom"]["environment"]);
        assert_eq!(
            patch["providerInstances"]["custom"]["config"],
            json!({"homePath":"/custom","future":true,"binaryPath":"/bin/codex"})
        );
        // An emptied field is dropped rather than saved as "".
        let cleared = config_patch(&settings, "custom", "codex", "homePath", json!("  "));
        assert_eq!(cleared["providerInstances"]["custom"]["config"], json!({"future":true}));
    }

    #[test]
    fn model_preference_patches_fold_into_the_optimistic_settings() {
        let mut settings = json!({"favorites": [{"provider": "opencode", "model": "a"}], "providerModelPreferences": null});
        let payload = json!({"patch": {
            "setModelFavorites": [{"provider": "opencode", "model": "a", "favorite": false}, {"provider": "opencode", "model": "b", "favorite": true}],
            "setModelsHidden": [{"provider": "opencode", "model": "c", "hidden": true}],
            "setProviderModelOrder": {"provider": "opencode", "modelOrder": ["b", "a"]}
        }});
        apply_extras(&mut settings, &payload);
        assert_eq!(settings["favorites"], json!([{"provider": "opencode", "model": "b"}]));
        assert_eq!(settings["providerModelPreferences"]["opencode"], json!({"hiddenModels": ["c"], "modelOrder": ["b", "a"]}));
        assert!(settings.get("setModelFavorites").is_none());
    }

    #[test]
    fn rows_follow_driver_order_with_unused_default_slots_left_out() {
        let instances = json!({
            "devReplay": {"driver": "acpRegistry"},
            "opencode": {"driver": "opencode", "enabled": true},
            "work": {"driver": "codex"},
            "legacy": {"driver": "retired"}
        });
        let ids = |target| build_rows(&instances, false, target).into_iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(ids(None), ["codex", "work", "claudeAgent", "opencode", "devReplay", "legacy"]);
        // A disabled driver's default slot appears while it is the one being opened.
        assert_eq!(ids(Some("grok")), ["codex", "work", "claudeAgent", "grok", "opencode", "devReplay", "legacy"]);
    }
}
