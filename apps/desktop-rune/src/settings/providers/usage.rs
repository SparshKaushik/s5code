//! The Providers page around its card: web's read-only notice, `UsageProviderSettings` with
//! `AddUsageLimitSourceDialog`, and the Advanced section (health check interval,
//! `CompactIdleThreadsRow`).

use super::super::{background, controls as c, permissions, request, server_settings, storage, update};
use crate::kit;
use crate::theme::{self, T};
use crate::{App, icons};
use rune::{El, TextEdit, col, row, text};
use serde_json::{Value, json};

/// The provider id in `provider::{id}::{field}` keys that addresses page-wide settings.
pub const PAGE: &str = "*";
pub const IDLE_KEY: &str = "provider::*::idleCompaction";
const HEALTH_KEY: &str = "bg::providerHealthRefreshInterval";
const HEALTH_FIELD: &str = "providerHealthRefreshInterval";
const TIP: &str = "provider-health-policy";
const POLICY: &str = "This interval is configured here, then the shared Background activity policy decides whether provider probes may run when the timer fires. Custom intervals appear as Advanced in Maintenance settings.";
const CURSOR_USAGE: &str = "Read your existing Cursor CLI login from macOS Keychain to show account history and monthly limits. macOS may ask you to allow access.";
const REMOVE_COPY: &str = "The hub's management key is deleted from this server. Its accounts leave the Limits view; the hub itself is untouched. Add it again with the URL and key to bring them back.";

/// Contracts `IDLE_COMPACTION_MODES`.
const MODES: [(&str, &str); 3] = [("onReturn", "When you return"), ("beforeCacheExpires", "Before the cache expires (beta)"), ("off", "Never")];

#[derive(Default)]
pub struct Ui {
    adding: bool,
    url: TextEdit,
    key: TextEdit,
    label: TextEdit,
    /// The hub whose removal awaits confirmation.
    removing: Option<String>,
    /// The Cursor account usage switch's write is in flight; providers refresh once it lands.
    cursor_updating: bool,
    tip: bool,
}

/// Web `classifyProviderEnvironmentAccess` "read-only": the session answered without
/// `providers:manage`. Fixtures have no session and stay editable.
pub fn read_only(s: &App) -> bool {
    permissions::reply(s).is_some() && !permissions::can(s, "providers:manage")
}

fn environment_label(s: &App) -> String {
    crate::env::label(s, &permissions::target(s))
}

/// A `SettingsRow` with plain copy and an optional control.
fn plain_row(t: &T, title: &str, description: Option<String>, control: Option<El>) -> El {
    let mut copy = col().grow(1.0).min_w(0.0).gap(4.0).child(text(title.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground).wrap());
    if let Some(description) = description {
        copy = copy.child(text(description).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap());
    }
    let mut line = row().w_full().px(16.0).py(12.0).gap(32.0).center_y().child(copy);
    if let Some(control) = control {
        line = line.child(control);
    }
    line
}

/// The "Limited permissions" card above the providers while the session can't manage them.
pub fn limited(s: &App, t: &T) -> Option<El> {
    read_only(s).then(|| {
        let description = format!("This session can view {}'s providers but can't change their settings.", environment_label(s));
        c::card(vec![plain_row(t, "Limited permissions", Some(description), None)], t)
    })
}

// --- Usage providers --------------------------------------------------------------------------

fn sources(settings: &Value) -> Vec<(String, &Value)> {
    settings["usageLimitSources"].as_object().into_iter().flatten().filter(|(_, v)| v.is_object()).map(|(id, v)| (id.clone(), v)).collect()
}

/// A hub row's title and description: its label (else URL), "CLI Proxy", disabled, the URL.
fn describe(source: &Value) -> (String, String) {
    let url = source["url"].as_str().unwrap_or_default();
    let label = source["label"].as_str().map(str::trim).filter(|l| !l.is_empty()).unwrap_or(url).to_owned();
    let mut description = "CLI Proxy".to_owned();
    if source["enabled"] == false {
        description += " · Disabled";
    }
    if label != url {
        description += &format!(" · {url}");
    }
    (label, description)
}

/// `new URL(url).host`, or the whole string when it isn't a URL.
fn host(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://").filter(|(scheme, _)| !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))) else {
        return url.to_owned();
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h).to_ascii_lowercase();
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "http" | "ws" => Some(":80"),
        "https" | "wss" => Some(":443"),
        _ => None,
    };
    if let Some(bare) = default_port.and_then(|p| host.strip_suffix(p)) {
        return bare.to_owned();
    }
    host
}

/// Web `sourceIdFromUrl`: the saved id for that URL, else a new `cliproxy-<host>` that never
/// replaces another hub.
fn source_id(url: &str, sources: &Value) -> String {
    if let Some((id, _)) = sources.as_object().into_iter().flatten().find(|(_, v)| v["url"] == url) {
        return id.clone();
    }
    // `replace(/[^a-z0-9.-]+/g, "-")`: each run of other characters becomes one dash.
    let mut slug = String::new();
    let mut run = false;
    for ch in host(url).to_lowercase().chars() {
        let kept = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '.' || ch == '-';
        if kept {
            slug.push(ch);
        } else if !run {
            slug.push('-');
        }
        run = !kept;
    }
    let slug = slug.trim_matches('-');
    let base = format!("cliproxy-{}", if slug.is_empty() { "hub" } else { slug });
    let mut id = base.clone();
    let mut suffix = 2;
    while sources.get(&id).is_some() {
        id = format!("{base}-{suffix}");
        suffix += 1;
    }
    id
}

fn set_cursor_usage(s: &mut App, enabled: bool) {
    update(s, "cursorKeychainUsageEnabled", json!(enabled));
    s.ui.settings.providers_ui.usage.cursor_updating = true;
}

/// Once the Cursor account usage write lands, refresh providers so their limits follow it.
pub fn poll(s: &mut App) -> bool {
    if !s.ui.settings.providers_ui.usage.cursor_updating || s.ui.settings.pending.values().any(|m| m == "server.updateSettings") {
        return false;
    }
    s.ui.settings.providers_ui.usage.cursor_updating = false;
    if s.ui.settings.error.is_none() {
        request(s, "server.refreshProviders", json!({}));
    }
    true
}

/// Web `UsageProviderSettings`.
pub fn section(s: &App, t: &T) -> El {
    let read_only = read_only(s);
    let ui = &s.ui.settings.providers_ui.usage;
    let settings = server_settings(s);
    let mut rows = Vec::new();
    if s.state().config.as_ref().is_some_and(|c| c["environment"]["platform"]["os"] == "darwin") {
        let on = settings["cursorKeychainUsageEnabled"] == true;
        let switch = kit::Switch::new(on)
            .label("Cursor account usage")
            .hide_label()
            .disabled(read_only || ui.cursor_updating)
            .on_toggle(move |s: &mut App| set_cursor_usage(s, !on))
            .build(&kit::theme(t));
        rows.push(plain_row(t, "Cursor account usage", Some(CURSOR_USAGE.into()), Some(switch)));
    }
    let hubs = sources(settings);
    if hubs.is_empty() {
        rows.push(plain_row(t, "No hubs configured.", None, None));
    }
    for (id, source) in hubs {
        let (label, description) = describe(source);
        let remove = (!read_only).then(|| {
            kit::Button::new("Remove")
                .variant("ghost")
                .size("xs")
                .on_press(move |s: &mut App| s.ui.settings.providers_ui.usage.removing = Some(id.clone()))
                .build(t)
        });
        rows.push(plain_row(t, &label, Some(description), remove));
    }
    let actions = if read_only {
        vec![]
    } else {
        vec![kit::Button::new("Add hub").variant("outline").size("xs").icon(&icons::PLUS).on_press(open_add).build(t)]
    };
    c::section_with_actions("Usage providers", actions, rows, t)
}

pub fn open_add(s: &mut App) {
    let ui = &mut s.ui.settings.providers_ui.usage;
    ui.adding = true;
    ui.url = TextEdit::default();
    ui.key = TextEdit::new(String::new()).secure();
    ui.label = TextEdit::default();
}

fn close_add(s: &mut App) {
    let ui = &mut s.ui.settings.providers_ui.usage;
    ui.adding = false;
    // The management key never outlives the dialog.
    ui.key = TextEdit::default();
}

fn can_save(ui: &Ui) -> bool {
    !ui.url.text.trim().is_empty() && !ui.key.text.trim().is_empty()
}

/// The `usageLimitSources` patch the Add hub dialog sends; the key goes to server secret storage.
fn hub_patch(sources: &Value, url: &str, key: &str, label: &str) -> Value {
    let mut entry = json!({"kind": "cliproxy", "url": url, "managementKey": key, "enabled": true});
    if !label.is_empty() {
        entry["label"] = json!(label);
    }
    let id = source_id(url, sources);
    json!({ id: entry })
}

fn save(s: &mut App) {
    let ui = &s.ui.settings.providers_ui.usage;
    if !can_save(ui) {
        return;
    }
    let patch = hub_patch(&server_settings(s)["usageLimitSources"], ui.url.text.trim(), ui.key.text.trim(), ui.label.text.trim());
    update(s, "usageLimitSources", patch);
    close_add(s);
}

fn remove(s: &mut App) {
    if let Some(id) = s.ui.settings.providers_ui.usage.removing.take() {
        update(s, "usageLimitSources", json!({ id: null }));
    }
}

/// Escape closes the Add hub or Remove hub dialog first.
pub fn escape(s: &mut App) -> bool {
    let ui = &s.ui.settings.providers_ui.usage;
    if ui.adding {
        close_add(s);
        return true;
    }
    s.ui.settings.providers_ui.usage.removing.take().is_some()
}

fn field(label: &str, input: El, t: &T) -> El {
    col().w_full().gap(6.0).child(text(label.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground)).child(input)
}

/// The Add hub and Remove hub dialogs, and the health interval's policy tooltip.
pub fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    let ui = &s.ui.settings.providers_ui.usage;
    if ui.tip {
        let _ = kit::Tooltip::new(TIP).label(POLICY).open(true).build(&kit::theme(t), layers);
    }
    if ui.adding && !read_only(s) {
        let url = kit::Input::new(&ui.url)
            .placeholder("https://hub.example.ts.net:8318")
            .on_edit(|s: &mut App| &mut s.ui.settings.providers_ui.usage.url)
            .on_submit(|s: &mut App, _| save(s))
            .build(t)
            .single_line();
        let key = kit::Input::new(&ui.key)
            .placeholder("Management key")
            .on_edit(|s: &mut App| &mut s.ui.settings.providers_ui.usage.key)
            .on_submit(|s: &mut App, _| save(s))
            .build(t)
            .single_line()
            .secure();
        let label = kit::Input::new(&ui.label)
            .placeholder("Defaults to the hub's host name")
            .on_edit(|s: &mut App| &mut s.ui.settings.providers_ui.usage.label)
            .on_submit(|s: &mut App, _| save(s))
            .build(t)
            .single_line();
        let form = col().w_full().gap(16.0).child(field("Hub URL", url, t)).child(field("Management key", key, t)).child(field("Label (optional)", label, t));
        let footer = row()
            .w_full()
            .gap(8.0)
            .child(rune::spacer())
            .child(kit::Button::new("Cancel").variant("outline").on_press(close_add).build(t))
            .child(kit::Button::new("Add hub").disabled(!can_save(ui)).on_press(save).build(t));
        kit::Dialog::new("usage-add-hub", "Add a CLIProxyAPI hub")
            .open(true)
            .width(448.0)
            .description(format!(
                "Show the quota of every account the hub pools, next to the providers on {}. The key stays on that server.",
                environment_label(s)
            ))
            .on_close(close_add)
            .child(form)
            .child(footer)
            .build(t, layers);
    }
    if let Some(id) = &ui.removing {
        let settings = server_settings(s);
        let label = sources(settings).into_iter().find(|(k, _)| k == id).map_or_else(|| id.clone(), |(_, v)| describe(v).0);
        let close = |s: &mut App| s.ui.settings.providers_ui.usage.removing = None;
        kit::Dialog::new("usage-remove-hub", format!("Remove {label}?"))
            .open(true)
            .alert()
            .description(REMOVE_COPY)
            .on_close(close)
            .child(
                row()
                    .gap(8.0)
                    .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                    .child(kit::Button::new("Remove hub").variant("destructive").on_press(remove).build(t)),
            )
            .build(t, layers);
    }
}

// --- Advanced ---------------------------------------------------------------------------------

/// Contracts `globalIdleCompaction`: the mode, else the legacy field's mode or switch.
fn idle_mode(settings: &Value) -> &str {
    if let Some(mode) = settings["idleCompaction"].as_str() {
        return mode;
    }
    match &settings["compactIdleThreads"] {
        Value::String(mode) => mode.as_str(),
        Value::Bool(true) => "beforeCacheExpires",
        _ => "onReturn",
    }
}

/// Contracts `readsIdleCompactionModes`: a server from before modes only has the switch.
fn reads_modes(settings: &Value) -> bool {
    settings.get("idleCompaction").is_some_and(|v| !v.is_null()) || settings["compactIdleThreads"].is_string()
}

/// Contracts `idleCompactionPatch`: the key and value in the form this server reads.
fn idle_patch(settings: &Value, mode: &str) -> (&'static str, Value) {
    if settings.get("idleCompaction").is_some_and(|v| !v.is_null()) {
        ("idleCompaction", json!(mode))
    } else if reads_modes(settings) {
        ("compactIdleThreads", json!(mode))
    } else {
        ("compactIdleThreads", json!(mode == "beforeCacheExpires"))
    }
}

/// `provider::*::{field}` reads.
pub fn value(s: &App, field: &str) -> Value {
    match field {
        "idleCompaction" => json!(idle_mode(server_settings(s))),
        _ => Value::Null,
    }
}

/// `provider::*::{field}` writes.
pub fn set(s: &mut App, field: &str, next: Value) {
    if field != "idleCompaction" {
        return;
    }
    let Some(mode) = next.as_str().filter(|m| MODES.iter().any(|(v, _)| v == m)) else { return };
    let (key, value) = idle_patch(server_settings(s), mode);
    update(s, key, value);
}

/// "Never" needs a server that stores modes.
pub fn never_unavailable(s: &App) -> bool {
    !reads_modes(server_settings(s))
}

fn reset_button(key: &str, label: &str, lock: Option<String>, t: &T, f: impl Fn(&mut App) + 'static) -> El {
    let button = kit::Button::new(format!("Reset {label}")).variant("ghost-muted").size("icon-micro").icon(&icons::UNDO_2).on_press(f).build(t);
    permissions::gate(key, button, lock)
}

/// Web's Providers "Advanced" section.
pub fn advanced(s: &App, cx: &rune::Cx<App>, t: &T) -> El {
    let settings = server_settings(s);
    let health_lock = permissions::row_lock(s, "backgroundActivity");
    let current = super::super::value(s, HEALTH_KEY).as_i64().unwrap_or(0);
    let tip = kit::Button::new("Background policy details")
        .variant("ghost-muted")
        .size("icon-micro")
        .icon(&icons::INFO)
        .build(t)
        .hover_delays(std::time::Duration::from_millis(200), std::time::Duration::ZERO)
        .on_hover_change(|s: &mut App, inside, _| s.ui.settings.providers_ui.usage.tip = inside);
    let mut heading = storage::title("Health check interval", t).child(row().key(format!("{TIP}-trigger")).child(tip));
    if current != background::preset_seconds(settings, HEALTH_FIELD) {
        heading = heading.child(reset_button("provider-health-reset", "provider health check interval", health_lock.clone(), t, |s| {
            background::clear_override(s, HEALTH_FIELD)
        }));
    }
    let interval = row()
        .gap(8.0)
        .center_y()
        .child(storage::stepper(s, t, HEALTH_KEY, "Provider health check interval", None, 30))
        .child(text("seconds").font(theme::TEXT_XS).color(t.c.muted_foreground));
    let health = storage::setting(
        cx,
        t,
        "provider-health-check-interval",
        heading,
        "Refresh provider status, versions, and models in the background. Set to 0 to disable.",
        permissions::gate("provider-health", interval, health_lock),
    );
    let idle_lock = permissions::row_lock(s, "idleCompaction");
    let mut heading = storage::title("Compact idle threads", t);
    if idle_mode(settings) != "onReturn" {
        heading = heading.child(reset_button("compact-idle-reset", "compact idle threads", idle_lock.clone(), t, |s| set(s, "idleCompaction", json!("onReturn"))));
    }
    let options: Vec<(Value, String)> = MODES.iter().map(|(v, l)| (json!(v), (*l).to_owned())).collect();
    let select = c::select(s, t, IDLE_KEY, "Compact idle threads", &options, c::select_width(IDLE_KEY));
    let idle = storage::setting(
        cx,
        t,
        "compact-idle-threads",
        heading,
        "When to compact an idle Claude thread over 200,000 tokens: after you return, just before its prompt cache expires, or never. Each Claude provider can override this.",
        permissions::gate("compact-idle", select, idle_lock),
    );
    c::section("Advanced", vec![health, idle], t)
}

/// Claude's own "Compact idle threads" in its Runtime section (contracts `ClaudeSettings`).
pub fn instance_idle_row(s: &App, cx: &rune::Cx<App>, t: &T, id: &str) -> El {
    let mut options = vec![json!(["default", "Use default"])];
    options.extend(MODES.iter().map(|(v, l)| json!([v, l])));
    c::setting_row(
        s,
        cx,
        t,
        &json!({
            "key": super::field_key(id, "idleCompaction"),
            "title": "Compact idle threads",
            "description": "When to compact an idle thread over 200,000 tokens: after you return, just before its prompt cache expires, or never. Overrides the default in Providers > Advanced.",
            "kind": "select",
            "options": options,
        }),
    )
}

// --- Scenes -----------------------------------------------------------------------------------

fn stage(s: &mut App, patch: Value) {
    let mut current = server_settings(s).clone();
    super::super::apply_patch(&mut current, &patch);
    s.ui.settings.optimistic_settings = Some(current);
}

/// Scenes: a saved hub on the Providers page, and the dialog that is open.
pub fn scene(s: &mut App, open: &str) {
    stage(s, json!({"usageLimitSources": {"cliproxy-hub.example.ts.net-8318": {
        "kind": "cliproxy", "label": "Team hub", "url": "https://hub.example.ts.net:8318", "managementKey": "", "enabled": true
    }}}));
    match open {
        "add" => {
            open_add(s);
            s.ui.settings.providers_ui.usage.url = TextEdit::new("https://hub.example.ts.net:8318".to_owned());
        }
        "remove" => s.ui.settings.providers_ui.usage.removing = Some("cliproxy-hub.example.ts.net-8318".into()),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hub_ids_follow_web_source_id_from_url() {
        let none = json!({});
        assert_eq!(source_id("https://hub.example.ts.net:8318", &none), "cliproxy-hub.example.ts.net-8318");
        assert_eq!(source_id("https://Hub.Example.com/", &none), "cliproxy-hub.example.com");
        assert_eq!(source_id("https://hub.example.com:443/x", &none), "cliproxy-hub.example.com");
        assert_eq!(source_id("not a url", &none), "cliproxy-not-a-url");
        assert_eq!(source_id("", &none), "cliproxy-hub");
        // The saved id for a URL, and a fresh suffix for another hub on the same host.
        let saved = json!({"cliproxy-hub.example.com": {"url": "https://hub.example.com/a"}});
        assert_eq!(source_id("https://hub.example.com/a", &saved), "cliproxy-hub.example.com");
        assert_eq!(source_id("https://hub.example.com/b", &saved), "cliproxy-hub.example.com-2");
        let patch = hub_patch(&none, "https://h.ts.net", "k", "");
        assert_eq!(patch, json!({"cliproxy-h.ts.net": {"kind": "cliproxy", "url": "https://h.ts.net", "managementKey": "k", "enabled": true}}));
        assert_eq!(hub_patch(&none, "https://h.ts.net", "k", "Mine")["cliproxy-h.ts.net"]["label"], "Mine");
    }

    #[test]
    fn hub_rows_read_like_web() {
        assert_eq!(describe(&json!({"url": "https://h", "enabled": true})), ("https://h".into(), "CLI Proxy".into()));
        assert_eq!(
            describe(&json!({"url": "https://h", "label": " Team ", "enabled": false})),
            ("Team".into(), "CLI Proxy · Disabled · https://h".into())
        );
    }

    #[test]
    fn idle_compaction_reads_and_writes_the_form_the_server_knows() {
        assert_eq!(idle_mode(&json!({"compactIdleThreads": false})), "onReturn");
        assert_eq!(idle_mode(&json!({"compactIdleThreads": true})), "beforeCacheExpires");
        assert_eq!(idle_mode(&json!({"compactIdleThreads": "off"})), "off");
        assert_eq!(idle_mode(&json!({"idleCompaction": "off", "compactIdleThreads": false})), "off");
        assert_eq!(idle_patch(&json!({"idleCompaction": "onReturn"}), "off"), ("idleCompaction", json!("off")));
        assert_eq!(idle_patch(&json!({"compactIdleThreads": "onReturn"}), "off"), ("compactIdleThreads", json!("off")));
        // A server from before modes only stores the switch, so "Never" is unavailable there.
        assert_eq!(idle_patch(&json!({"compactIdleThreads": false}), "beforeCacheExpires"), ("compactIdleThreads", json!(true)));
        assert!(!reads_modes(&json!({"compactIdleThreads": false})));
    }

    #[test]
    fn the_health_interval_resets_to_the_shared_policy_preset() {
        assert_eq!(background::preset_seconds(&json!({}), HEALTH_FIELD), 300);
        assert_eq!(background::preset_seconds(&json!({"backgroundActivity": {"profile": "performance"}}), HEALTH_FIELD), 60);
    }
}
