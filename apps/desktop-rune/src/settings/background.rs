//! Web `BackgroundActivitySettingsRow` and its Advanced dialog, resolved as
//! `@t3tools/shared/backgroundActivitySettings` does.
//!
//! Fields are settings keys under `bg::` (`bg::profile`, `bg::automaticGitFetchInterval`), so
//! the shared select and field plumbing edits them and `value`/`update` route here.

use super::storage::{setting, stepper, title};
use super::{controls as c, server_settings, update};
use crate::theme::{self, T};
use crate::{App, icons};
use rune::{Cx, El, col, row, text};
use serde_json::{Value, json};

const PROFILES: [(&str, &str, &str); 3] = [
    ("balanced", "Balanced", "Pauses probes for idle clients, locked hosts, or low power mode."),
    ("performance", "Performance", "Allows scoped background probes while any subscribed client remains connected."),
    ("battery-saver", "Battery saver", "Also pauses background probes when the host or client is on battery."),
];
const DURATIONS: [&str; 4] = [
    "automaticGitFetchInterval",
    "providerHealthRefreshInterval",
    "hostPowerMonitorActiveInterval",
    "hostPowerMonitorIdleInterval",
];
const FLAGS: [(&str, &str); 4] = [
    ("pauseWhenHostLocked", "Pause when host is locked"),
    ("pauseWhenHostLowPower", "Pause on host low power"),
    ("pauseWhenClientLowPower", "Pause on client low power"),
    ("pauseWhenOnBattery", "Pause on battery"),
];
const POLICY: &str = "This shared policy gates background work such as Git refreshes and provider health probes after their individual intervals elapse.";
const TIP: &str = "background-policy";

#[derive(Clone, Copy, Debug, PartialEq)]
struct Resolved {
    profile: &'static str,
    /// Milliseconds, in `DURATIONS` order.
    durations: [i64; 4],
    flags: [bool; 4],
}

fn preset(profile: &str) -> Resolved {
    match profile {
        "performance" => Resolved {
            profile: "performance",
            durations: [15_000, 60_000, 30_000, 120_000],
            flags: [true, false, false, false],
        },
        "battery-saver" => Resolved {
            profile: "battery-saver",
            durations: [0, 900_000, 60_000, 600_000],
            flags: [true; 4],
        },
        _ => Resolved {
            profile: "balanced",
            durations: [30_000, 300_000, 30_000, 300_000],
            flags: [true, true, true, false],
        },
    }
}

fn profile(v: &Value) -> Option<&'static str> {
    PROFILES.iter().map(|p| p.0).find(|p| v.as_str() == Some(*p))
}

fn label(profile: &str) -> &'static str {
    PROFILES.iter().find(|p| p.0 == profile).map_or("Balanced", |p| p.1)
}

/// `Schema.DurationFromMillis` sends milliseconds; the bundled defaults hold `Duration`'s JSON.
fn millis(v: &Value) -> Option<i64> {
    v.as_f64().or_else(|| v["millis"].as_f64()).map(|ms| ms.round() as i64)
}

fn base_profile(activity: &Value) -> &'static str {
    let selection = if activity["profile"] == "custom" { &activity["baseProfile"] } else { &activity["profile"] };
    profile(selection).unwrap_or("balanced")
}

fn resolve(activity: &Value) -> Resolved {
    let mut resolved = preset(base_profile(activity));
    if activity["profile"] == "custom" {
        let overrides = &activity["overrides"];
        for (i, key) in DURATIONS.iter().enumerate() {
            if let Some(ms) = millis(&overrides[*key]) {
                resolved.durations[i] = ms;
            }
        }
        for (i, (key, _)) in FLAGS.iter().enumerate() {
            if let Some(on) = overrides[*key].as_bool() {
                resolved.flags[i] = on;
            }
        }
    }
    resolved
}

/// `resolveServerBackgroundActivitySettings`: untouched settings still honour the legacy
/// profile and intervals.
fn resolve_server(settings: &Value) -> Resolved {
    let activity = &settings["backgroundActivity"];
    let untouched = activity["profile"].as_str().unwrap_or("balanced") == "balanced"
        && activity["baseProfile"].is_null()
        && activity["overrides"].as_object().map_or(true, |o| o.is_empty());
    let legacy = profile(&settings["backgroundActivityProfile"]).unwrap_or("balanced");
    let git = millis(&settings["automaticGitFetchInterval"]).unwrap_or(30_000);
    let health = millis(&settings["providerHealthRefreshInterval"]).unwrap_or(300_000);
    if untouched && (legacy != "balanced" || git != 30_000 || health != 300_000) {
        let base = preset(legacy);
        let mut overrides = serde_json::Map::new();
        if git != base.durations[0] {
            overrides.insert(DURATIONS[0].into(), json!(git));
        }
        if health != base.durations[1] {
            overrides.insert(DURATIONS[1].into(), json!(health));
        }
        let selection = if overrides.is_empty() { legacy } else { "custom" };
        return resolve(&json!({"profile": selection, "baseProfile": legacy, "overrides": Value::Object(overrides)}));
    }
    resolve(activity)
}

/// `resolveBackgroundActivityProfileOption`: the first preset the values equal, else advanced.
fn option(resolved: &Resolved) -> &'static str {
    [resolved.profile, "balanced", "performance", "battery-saver"]
        .into_iter()
        .find(|p| {
            let p = preset(p);
            p.durations == resolved.durations && p.flags == resolved.flags
        })
        .unwrap_or("advanced")
}

/// The fields that differ from the base preset (`normalizeBackgroundActivitySettings`).
fn differences(resolved: &Resolved) -> Value {
    let base = preset(resolved.profile);
    let mut overrides = serde_json::Map::new();
    for i in 0..4 {
        if resolved.durations[i] != base.durations[i] {
            overrides.insert(DURATIONS[i].into(), json!(resolved.durations[i]));
        }
        if resolved.flags[i] != base.flags[i] {
            overrides.insert(FLAGS[i].0.into(), json!(resolved.flags[i]));
        }
    }
    Value::Object(overrides)
}

fn profile_settings(profile: &str) -> Value {
    json!({"schemaVersion": 1, "profile": profile, "overrides": {}})
}

/// `backgroundActivitySharedPolicySettings`: a new base policy that keeps custom intervals.
fn shared_policy(settings: &Value, next: &str) -> Value {
    let resolved = resolve_server(settings);
    let overrides = if option(&resolved) == "advanced" { differences(&resolved) } else { json!({}) };
    json!({"schemaVersion": 1, "profile": "custom", "baseProfile": next, "overrides": overrides})
}

/// `backgroundActivityOverrideSettings`: every resolved value pinned, with one changed.
fn with_override(settings: &Value, field: &str, next: Value) -> Value {
    let resolved = resolve_server(settings);
    let mut overrides = serde_json::Map::new();
    for i in 0..4 {
        overrides.insert(DURATIONS[i].into(), json!(resolved.durations[i]));
        overrides.insert(FLAGS[i].0.into(), json!(resolved.flags[i]));
    }
    overrides.insert(field.into(), next);
    json!({
        "schemaVersion": 1,
        "profile": "custom",
        "baseProfile": base_profile(&settings["backgroundActivity"]),
        "overrides": Value::Object(overrides)
    })
}

/// The shared policy's preset value for `field`, in seconds: what a reset goes back to.
pub(super) fn preset_seconds(settings: &Value, field: &str) -> i64 {
    let resolved = resolve_server(settings);
    DURATIONS.iter().position(|d| *d == field).map_or(0, |i| preset(resolved.profile).durations[i] / 1000)
}

/// `backgroundActivityOverrideSettings` with `field: undefined`: every other value stays pinned
/// while `field` falls back to the shared policy's preset.
pub(super) fn clear_override(s: &mut App, field: &str) {
    let mut next = with_override(server_settings(s), field, Value::Null);
    if let Some(overrides) = next["overrides"].as_object_mut() {
        overrides.remove(field);
    }
    update(s, "backgroundActivity", next);
}

fn is_default(activity: &Value) -> bool {
    activity.is_null()
        || (activity["schemaVersion"].as_i64().unwrap_or(1) == 1
            && activity["profile"].as_str().unwrap_or("balanced") == "balanced"
            && activity["baseProfile"].is_null()
            && activity["overrides"].as_object().map_or(true, |o| o.is_empty()))
}

fn description(resolved: &Resolved) -> String {
    if option(resolved) == "advanced" {
        return format!("Uses custom intervals. Shared policy: {}.", label(resolved.profile));
    }
    PROFILES.iter().find(|p| p.0 == resolved.profile).map_or("", |p| p.2).to_owned()
}

/// Web's `isEnvironmentScope`: one environment selected, or the only one there is.
fn environment_scope(s: &App) -> bool {
    s.ui.settings.environment_only || s.ui.env.saved.len() <= 1
}

/// The interval minimum and the field's step, in seconds.
fn interval(i: usize) -> (i64, i64) {
    match i {
        0 => (0, 5),
        1 => (0, 30),
        2 => (5, 5),
        _ => (5, 30),
    }
}

pub fn value(s: &App, key: &str) -> Value {
    let resolved = resolve_server(server_settings(s));
    match key {
        "profile" => json!(option(&resolved)),
        "shared" => json!(resolved.profile),
        key => match DURATIONS.iter().position(|d| *d == key) {
            Some(i) => json!((resolved.durations[i] as f64 / 1000.0).round() as i64),
            None => FLAGS.iter().position(|f| f.0 == key).map_or(Value::Null, |i| json!(resolved.flags[i])),
        },
    }
}

pub fn set(s: &mut App, key: &str, next: Value) {
    let write = match key {
        "profile" if next == "advanced" => {
            s.ui.settings.maintenance.dialog = environment_scope(s);
            return;
        }
        "profile" => match profile(&next) {
            Some(p) => profile_settings(p),
            None => return,
        },
        "shared" => match profile(&next) {
            Some(p) => shared_policy(server_settings(s), p),
            None => return,
        },
        key => {
            if let Some(i) = DURATIONS.iter().position(|d| *d == key) {
                let (min, _) = interval(i);
                let Some(seconds) = next.as_f64().map(|v| (v.round() as i64).max(min)) else { return };
                if json!(seconds) == value(s, key) {
                    return;
                }
                with_override(server_settings(s), key, json!(seconds * 1000))
            } else if FLAGS.iter().any(|f| f.0 == key) && next.is_boolean() {
                with_override(server_settings(s), key, next)
            } else {
                return;
            }
        }
    };
    update(s, "backgroundActivity", write);
}

pub fn escape(s: &mut App) -> bool {
    let open = std::mem::take(&mut s.ui.settings.maintenance.dialog);
    if open {
        super::commit_edits(s, true);
    }
    open
}

fn close(s: &mut App) {
    super::commit_edits(s, true);
    s.ui.settings.maintenance.dialog = false;
}

fn options(advanced: Option<&str>) -> Vec<(Value, String)> {
    PROFILES
        .iter()
        .map(|(v, l, _)| (json!(v), (*l).to_owned()))
        .chain(advanced.map(|l| (json!("advanced"), l.to_owned())))
        .collect()
}

/// Maintenance's "Background activity" section.
pub fn view(s: &App, cx: &Cx<App>, t: &T) -> El {
    let settings = server_settings(s);
    let resolved = resolve_server(settings);
    let scoped = environment_scope(s);
    let tip = crate::kit::Button::new("Background policy details")
        .variant("ghost-muted")
        .size("icon-micro")
        .icon(&icons::INFO)
        .build(t)
        .hover_delays(std::time::Duration::from_millis(200), std::time::Duration::ZERO)
        .on_hover_change(|s: &mut App, inside, _| s.ui.settings.maintenance.tip = inside);
    // The tooltip's popup mounts with the settings overlays, anchored to this key.
    let mut heading = title("Background activity", t).child(row().key(format!("{TIP}-trigger")).child(tip));
    if !is_default(&settings["backgroundActivity"]) {
        heading = heading.child(
            crate::kit::Button::new("Reset background activity")
                .variant("ghost-muted")
                .size("icon-micro")
                .icon(&icons::UNDO_2)
                .on_press(|s: &mut App| update(s, "backgroundActivity", profile_settings("balanced")))
                .build(t),
        );
    }
    let advanced = if scoped { "Advanced" } else { "Advanced (one environment)" };
    let mut control = row()
        .gap(8.0)
        .center_y()
        .child(c::select(s, t, "bg::profile", "Background activity profile", &options(Some(advanced)), c::select_width("bg::profile")));
    if scoped && option(&resolved) == "advanced" {
        control = control.child(
            crate::kit::Button::new("Configure advanced background activity")
                .variant("outline")
                .size("icon-sm")
                .icon(&icons::SETTINGS)
                .on_press(|s: &mut App| s.ui.settings.maintenance.dialog = true)
                .build(t),
        );
    }
    // Environment-wide, like the page's other server settings under a project scope.
    if s.ui.settings.project.is_some() {
        control = control.disabled(true).opacity(0.5);
    }
    c::section(
        "Background activity",
        vec![setting(cx, t, "background-activity", heading, &description(&resolved), control)],
        t,
    )
}

fn dialog_row(t: &T, name: &str, copy: &str, control: El) -> El {
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
                .child(text(name.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(text(copy.to_owned()).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap()),
        )
        .child(control)
}

fn seconds(s: &App, t: &T, i: usize, name: &str) -> El {
    let (_, step) = interval(i);
    row()
        .gap(8.0)
        .center_y()
        .child(stepper(s, t, &format!("bg::{}", DURATIONS[i]), name, None, step))
        .child(text("seconds").font(theme::TEXT_XS).color(t.c.muted_foreground))
}

fn flag(s: &App, t: &T, i: usize) -> El {
    let (key, name) = FLAGS[i];
    let on = value(s, key) == true;
    row()
        .grow(1.0)
        .min_w(0.0)
        .px(16.0)
        .py(12.0)
        .gap(12.0)
        .center_y()
        .child(text(name).font(theme::SM_MEDIUM).color(t.c.foreground).grow(1.0).min_w(0.0))
        .child(
            crate::kit::Switch::new(on)
                .label(name)
                .hide_label()
                .on_toggle(move |s: &mut App| set(s, key, json!(!on)))
                .build(&crate::kit::theme(t)),
        )
}

/// The policy tooltip and the Advanced dialog.
pub fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    if !matches!(&s.route, crate::Route::Settings(p) if p == "maintenance" || p == "storage") {
        return;
    }
    if s.ui.settings.maintenance.tip {
        let _ = crate::kit::Tooltip::new(TIP).label(POLICY).open(true).build(&crate::kit::theme(t), layers);
    }
    if !(s.ui.settings.maintenance.dialog && environment_scope(s)) {
        return;
    }
    let rows = vec![
        dialog_row(
            t,
            "Shared policy",
            "Controls whether background work may run after a subscribed interval fires.",
            c::select(s, t, "bg::shared", "Shared background policy", &options(None), c::select_width("bg::shared")),
        ),
        dialog_row(t, "Git fetch interval", "Refresh remote branch status in the background.", seconds(s, t, 0, "Git fetch interval")),
        dialog_row(
            t,
            "Provider health interval",
            "Refresh provider availability, versions, and auth state. Model catalog updates follow Provider update checks.",
            seconds(s, t, 1, "Provider health interval"),
        ),
        dialog_row(t, "Host power monitor", "Poll host power state while clients are active.", seconds(s, t, 2, "Active host power interval")),
        dialog_row(
            t,
            "Idle host monitor",
            "Poll host power state when no foreground client is active.",
            seconds(s, t, 3, "Idle host power interval"),
        ),
        row().w_full().child(flag(s, t, 0)).child(flag(s, t, 1)),
        row().w_full().child(flag(s, t, 2)).child(flag(s, t, 3)),
    ];
    let footer = row()
        .w_full()
        .gap(8.0)
        .child(rune::spacer())
        .child(
            crate::kit::Button::new("Reset all")
                .variant("outline")
                .on_press(|s: &mut App| update(s, "backgroundActivity", profile_settings("balanced")))
                .build(t),
        )
        .child(crate::kit::Button::new("Done").on_press(close).build(t));
    crate::kit::Dialog::new("background-activity", "Background Activity")
        .open(true)
        .width(576.0)
        .description("Tune the shared power policy and the background intervals that feed it.")
        .on_close(close)
        .child(col().w_full().shrink().min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(c::card(rows, t)))
        .child(footer)
        .build(t, layers);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_and_overrides_resolve() {
        let custom = json!({"profile": "custom", "baseProfile": "performance", "overrides": {
            "automaticGitFetchInterval": 45_000, "pauseWhenOnBattery": true
        }});
        let resolved = resolve(&custom);
        assert_eq!(resolved.profile, "performance");
        assert_eq!(resolved.durations, [45_000, 60_000, 30_000, 120_000]);
        assert_eq!(resolved.flags, [true, false, false, true]);
        // A preset ignores stray overrides.
        assert_eq!(resolve(&json!({"profile": "battery-saver", "overrides": {"pauseWhenOnBattery": false}})), preset("battery-saver"));
        assert_eq!(resolve(&Value::Null), preset("balanced"));
        assert_eq!(millis(&json!({"_id": "Duration", "_tag": "Millis", "millis": 30000})), Some(30_000));
    }

    #[test]
    fn untouched_settings_honour_legacy_fields() {
        let legacy = json!({"backgroundActivity": {"schemaVersion": 1, "profile": "balanced", "overrides": {}},
            "backgroundActivityProfile": "performance", "automaticGitFetchInterval": 15_000, "providerHealthRefreshInterval": 60_000});
        assert_eq!(resolve_server(&legacy), preset("performance"));
        let tuned = json!({"backgroundActivity": {}, "backgroundActivityProfile": "balanced", "automaticGitFetchInterval": 10_000});
        let resolved = resolve_server(&tuned);
        assert_eq!((resolved.profile, resolved.durations[0]), ("balanced", 10_000));
        assert_eq!(option(&resolved), "advanced");
        // Once the new setting is written, legacy fields stop counting.
        let written = json!({"backgroundActivity": {"profile": "battery-saver"}, "backgroundActivityProfile": "performance"});
        assert_eq!(resolve_server(&written), preset("battery-saver"));
    }

    #[test]
    fn the_option_is_the_preset_the_values_match() {
        assert_eq!(option(&preset("battery-saver")), "battery-saver");
        let mut matches_performance = preset("performance");
        matches_performance.profile = "balanced";
        assert_eq!(option(&matches_performance), "performance");
        let mut custom = preset("balanced");
        custom.flags[3] = true;
        assert_eq!(option(&custom), "advanced");
        assert_eq!(description(&custom), "Uses custom intervals. Shared policy: Balanced.");
        assert_eq!(description(&preset("performance")), PROFILES[1].2);
    }

    #[test]
    fn writes_keep_custom_intervals() {
        let settings = json!({"backgroundActivity": {"profile": "custom", "baseProfile": "balanced", "overrides": {"automaticGitFetchInterval": 45_000}}});
        assert_eq!(
            shared_policy(&settings, "battery-saver"),
            json!({"schemaVersion": 1, "profile": "custom", "baseProfile": "battery-saver", "overrides": {"automaticGitFetchInterval": 45_000}})
        );
        let plain = json!({"backgroundActivity": {"profile": "performance"}});
        assert_eq!(shared_policy(&plain, "balanced")["overrides"], json!({}));
        let next = with_override(&settings, "pauseWhenOnBattery", json!(true));
        assert_eq!(next["baseProfile"], "balanced");
        assert_eq!(next["overrides"]["automaticGitFetchInterval"], 45_000);
        assert_eq!(next["overrides"]["pauseWhenOnBattery"], true);
        assert_eq!(next["overrides"]["hostPowerMonitorIdleInterval"], 300_000);
        assert!(is_default(&json!({"schemaVersion": 1, "profile": "balanced", "overrides": {}})));
        assert!(!is_default(&next));
    }
}
