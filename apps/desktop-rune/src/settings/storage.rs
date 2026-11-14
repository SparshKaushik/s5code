//! Web `StorageSettings`: Maintenance's "Artifacts and logs" and "Cleanup" (Delete now and the
//! last run), and the Worktrees page's "Cleanup" rules.
//!
//! Fields are settings keys under `storage::` (`storage::logsAfterDays`, `storage::mode`), so the
//! shared select, field and switch plumbing edits them and `value`/`update` route here.

use super::{controls as c, nested_object, server_settings, update};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{Cx, El, col, el, row, text};
use serde_json::{Value, json};

const REPORT: &str = "server.getStorageCleanupReport";
const WORKTREE_FIELDS: [&str; 6] = [
    "worktreeKeepWhen",
    "worktreeAfterDays",
    "worktreeSettledAfterDays",
    "worktreeOnMerge",
    "worktreeOnDelete",
    "worktreeUnchanged",
];
const KEEP_WHEN: [(&str, &str); 3] = [
    ("any-local-files", "Any local files"),
    ("uncommitted-changes", "Uncommitted changes"),
    ("tracked-changes", "Edited tracked files"),
];

/// Maintenance page state: the manual cleanup run in flight and its reply, and the background
/// activity dialog and policy tooltip.
#[derive(Default)]
pub struct Maintenance {
    run: Option<u64>,
    report: Option<Value>,
    pub(super) dialog: bool,
    pub(super) tip: bool,
}

fn capability(s: &App, name: &str) -> bool {
    s.state().config.as_ref().is_some_and(|c| c["environment"]["capabilities"][name] == true)
}

fn environment_label(s: &App) -> String {
    s.state()
        .config
        .as_ref()
        .and_then(|c| c["environment"]["label"].as_str())
        .unwrap_or("this environment")
        .to_owned()
}

pub(super) fn project_scope(s: &App) -> bool {
    s.ui.settings.project.is_some()
}

/// `resolveWorktreeCleanup`: a custom policy's rules, an off policy's (nothing deleted), or the
/// machine's `storageCleanup` worktree fields.
fn rules(policy: &Value, storage: &Value) -> Value {
    match policy["mode"].as_str() {
        Some("custom") => {
            let mut rules = json!({
                "worktreeKeepWhen": "uncommitted-changes",
                "worktreeAfterDays": null,
                "worktreeSettledAfterDays": null,
                "worktreeOnMerge": false,
                "worktreeOnDelete": false,
                "worktreeUnchanged": false
            });
            for (field, v) in policy["rules"].as_object().into_iter().flatten() {
                rules[field.as_str()] = v.clone();
            }
            rules
        }
        Some("off") => json!({
            "worktreeKeepWhen": storage["worktreeKeepWhen"].as_str().unwrap_or("uncommitted-changes"),
            "worktreeAfterDays": null,
            "worktreeSettledAfterDays": null,
            "worktreeOnMerge": false,
            "worktreeOnDelete": false,
            "worktreeUnchanged": false
        }),
        _ => {
            let mut rules = json!({});
            for field in WORKTREE_FIELDS {
                rules[field] = storage[field].clone();
            }
            rules
        }
    }
}

fn rules_enabled(rules: &Value) -> bool {
    !rules["worktreeAfterDays"].is_null()
        || !rules["worktreeSettledAfterDays"].is_null()
        || rules["worktreeOnMerge"] == true
        || rules["worktreeOnDelete"] == true
        || rules["worktreeUnchanged"] == true
}

/// Whether Delete now has anything to do: a retention rule, or worktree rules on the machine or
/// in any project's custom policy.
fn has_rules(settings: &Value) -> bool {
    let storage = &settings["storageCleanup"];
    !storage["browserArtifactsAfterDays"].is_null()
        || !storage["logsAfterDays"].is_null()
        || rules_enabled(&rules(&settings["worktreeCleanup"], storage))
        || settings["projectSettingsOverrides"].as_object().into_iter().flatten().any(|(_, o)| {
            o["worktreeCleanup"]["mode"] == "custom" && rules_enabled(&o["worktreeCleanup"]["rules"])
        })
}

/// An environment-wide worktree rule: `storageCleanup`, plus the custom policy web keeps in step
/// when the machine has one.
fn worktree_patch(storage: &Value, policy: &Value, field: &str, next: Value) -> Value {
    let mut patch = json!({"storageCleanup": storage.clone()});
    patch["storageCleanup"][field] = next.clone();
    if !policy.is_null() {
        let mut custom = rules(policy, storage);
        custom[field] = next;
        patch["worktreeCleanup"] = json!({"mode": "custom", "rules": custom});
    }
    patch
}

/// `RetentionControl`'s commit: off, or whole days in range.
fn days(v: &Value, minimum: i64) -> Option<Value> {
    if v.is_null() {
        return Some(Value::Null);
    }
    v.as_f64().map(|d| json!((d.round() as i64).clamp(minimum, 3650)))
}

fn minimum(field: &str) -> i64 {
    if field == "worktreeSettledAfterDays" { 0 } else { 1 }
}

/// The project's own policy mode; a project without one inherits.
fn mode(s: &App) -> &'static str {
    let Some(project) = &s.ui.settings.project else { return "inherit" };
    match server_settings(s)["projectSettingsOverrides"][project]["worktreeCleanup"]["mode"].as_str() {
        Some("off") => "off",
        Some("custom") => "custom",
        _ => "inherit",
    }
}

fn effective_rules(s: &App) -> Value {
    rules(&super::value(s, "worktreeCleanup"), &nested_object(s, "storageCleanup"))
}

pub fn value(s: &App, key: &str) -> Value {
    match key {
        "mode" => json!(mode(s)),
        k if WORKTREE_FIELDS.contains(&k) => effective_rules(s)[k].clone(),
        k => super::value(s, &format!("storageCleanup.{k}")),
    }
}

pub fn set(s: &mut App, key: &str, next: Value) {
    if key == "mode" {
        match next.as_str() {
            Some("inherit") => super::scope::reset(s, "worktreeCleanup"),
            Some("off") => update(s, "worktreeCleanup", json!({"mode": "off"})),
            Some("custom") => {
                let custom = effective_rules(s);
                update(s, "worktreeCleanup", json!({"mode": "custom", "rules": custom}));
            }
            _ => {}
        }
        return;
    }
    let next = if key.ends_with("AfterDays") {
        match days(&next, minimum(key)) {
            Some(v) => v,
            None => return,
        }
    } else {
        next
    };
    if !WORKTREE_FIELDS.contains(&key) {
        return update(s, &format!("storageCleanup.{key}"), next);
    }
    if project_scope(s) {
        let mut custom = effective_rules(s);
        custom[key] = next;
        return update(s, "worktreeCleanup", json!({"mode": "custom", "rules": custom}));
    }
    let patch = worktree_patch(&nested_object(s, "storageCleanup"), &super::value(s, "worktreeCleanup"), key, next);
    super::request(s, "server.updateSettings", json!({"patch": patch}));
}

/// The last-run subscription the Maintenance page watches.
pub fn report_resource(s: &App) -> Option<(String, Value)> {
    capability(s, "storageCleanupRun").then(|| (REPORT.to_owned(), json!({})))
}

/// The manual run's reply: its failures and errors toast like web's.
pub fn poll(s: &mut App) -> bool {
    let Some(id) = s.ui.settings.maintenance.run else { return false };
    let Some(reply) = s.src.net().and_then(|n| n.take_reply(id)) else { return false };
    s.ui.settings.maintenance.run = None;
    let environment = environment_label(s);
    match reply {
        Ok(report) => {
            let failure = report["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|e| e["outcome"] == "failed")
                .map(|e| e["reason"].as_str().unwrap_or("").to_owned());
            if let Some(reason) = failure {
                crate::shell::palette::toast_error(s, &format!("Cleanup failed on {environment}"), Some(&reason));
            }
            s.ui.settings.maintenance.report = Some(report);
        }
        Err(error) => {
            let detail = crate::shell::palette::error_message(&error, "The request failed.");
            crate::shell::palette::toast_error(s, &format!("Could not run cleanup on {environment}"), Some(&detail));
        }
    }
    true
}

fn run(s: &mut App) {
    if s.ui.settings.maintenance.run.is_some() {
        return;
    }
    let id = s.src.net().map(|n| n.request("server.runStorageCleanup", json!({})));
    s.ui.settings.maintenance.run = id;
}

/// `formatBytes`.
fn format_bytes(value: f64) -> String {
    if value.round() < 1024.0 {
        return format!("{} B", value.round() as i64);
    }
    let scaled = |v: f64| format!("{:.*}", if v >= 100.0 { 0 } else if v >= 10.0 { 1 } else { 2 }, v);
    let units = ["KB", "MB", "GB", "TB"];
    let (mut next, mut unit) = (value / 1024.0, 0);
    while scaled(next).parse::<f64>().unwrap_or(0.0) >= 1024.0 && unit < units.len() - 1 {
        next /= 1024.0;
        unit += 1;
    }
    format!("{} {}", scaled(next), units[unit])
}

/// `toLocaleString()` in en-US.
fn grouped(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// `formatRelativeTimeLabel`.
fn relative(iso: &str, now: i64) -> String {
    let Some(at) = crate::data::iso_ms(iso) else { return String::new() };
    let minutes = (now - at).max(0) / 60_000;
    match minutes {
        0 => "just now".into(),
        m if m < 60 => format!("{m}m ago"),
        m if m < 24 * 60 => format!("{}h ago", m / 60),
        m => format!("{}d ago", m / (24 * 60)),
    }
}

fn count(v: &Value) -> i64 {
    v.as_f64().unwrap_or(0.0) as i64
}

fn counts(report: &Value) -> String {
    let n = &report["counts"];
    let failed = count(&n["failed"]);
    format!(
        "{} removed, {} kept{}",
        count(&n["removed"]),
        count(&n["kept"]),
        if failed > 0 { format!(", {failed} failed") } else { String::new() }
    )
}

fn last_run(report: &Value, now: i64) -> String {
    let failed = count(&report["counts"]["failed"]);
    format!(
        "Last run {}. {} recovered.{}",
        relative(report["finishedAt"].as_str().unwrap_or(""), now),
        format_bytes(report["bytesFreed"].as_f64().unwrap_or(0.0)),
        if failed > 0 {
            format!(" {failed} {} couldn't be removed.", if failed == 1 { "item" } else { "items" })
        } else {
            String::new()
        }
    )
}

/// A report entry's title (`CleanupResults`).
fn entry_name(entry: &Value) -> String {
    match entry["kind"].as_str() {
        Some("worktree") => {
            let path = entry["path"].as_str();
            let name = if entry["threadId"].is_null() {
                path
            } else {
                path.and_then(|p| p.split(['/', '\\']).filter(|part| !part.is_empty()).last())
            };
            name.unwrap_or("Worktrees").to_owned()
        }
        Some("logs") => "Rotated logs".into(),
        _ => "Browser artifacts".into(),
    }
}

fn entry_size(entry: &Value) -> Option<String> {
    let outcome = entry["outcome"].as_str();
    if !(outcome == Some("removed") || (outcome == Some("kept") && !entry["bytes"].is_null())) {
        return None;
    }
    let mut parts = Vec::new();
    if entry["kind"] != "worktree" && let Some(files) = entry["files"].as_f64() {
        let files = files as i64;
        parts.push(format!("{} {}", grouped(files), if files == 1 { "file" } else { "files" }));
    }
    if let Some(bytes) = entry["bytes"].as_f64() {
        parts.push(format_bytes(bytes));
    }
    Some(parts.join(" · "))
}

/// A `SettingsRow`: title, balanced description, and the control at the end.
pub(super) fn setting(cx: &Cx<App>, t: &T, key: &str, heading: El, description: &str, control: El) -> El {
    let id = format!("copy-{key}");
    let width = cx.layout(&id).map(|r| r[2]).unwrap_or(558.0).min(672.0);
    let mut copy = col().key(id).grow(1.0).min_w(0.0).gap(4.0).child(heading);
    if !description.is_empty() {
        copy = copy.child(
            text(super::balance::description(description, width))
                .font(theme::TEXT_XS.lh(18.0))
                .color(t.c.muted_foreground)
                .wrap(),
        );
    }
    row().key(format!("row-{key}")).w_full().px(16.0).py(12.0).gap(32.0).center_y().child(copy).child(control)
}

pub(super) fn title(label: &str, t: &T) -> El {
    row().gap(6.0).center_y().child(text(label.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground))
}

/// Web `NumberField` size sm: −, the amount and its unit, +, in one bordered group. Steps go
/// through `update`, which clamps them like the field's commit.
pub(super) fn stepper(s: &App, t: &T, key: &str, label: &str, unit: Option<&str>, step: i64) -> El {
    let current = super::value(s, key).as_i64().unwrap_or(0);
    let button = |delta: i64, glyph: &'static rune::Icon, verb: &str| {
        let k = key.to_owned();
        crate::kit::Button::new(format!("{verb} {}", label.to_lowercase()))
            .variant("ghost")
            .size("icon-sm")
            .icon(glyph)
            .on_press(move |s: &mut App| {
                s.ui.settings.edits.remove(&k);
                update(s, &k, json!(current + delta));
            })
            .build(t)
            .w(28.0)
            .h(26.0)
    };
    let mut group = row()
        .h(28.0)
        .center_y()
        .radius(theme::layout::CONTROL_RADIUS)
        .border(1.0, t.c.input)
        .fill(if t.dark { t.c.input.alpha_mul(0.32) } else { t.c.background })
        .clip_rounded(theme::layout::CONTROL_RADIUS)
        .child(button(-step, &icons::MINUS, "Decrease"))
        .child(c::input(s, t, key, if unit.is_some() { 44.0 } else { 64.0 }, "", false).h(26.0).border(0.0, t.c.input).fill(rune::Color::TRANSPARENT));
    if let Some(unit) = unit {
        group = group.child(text(unit).font(theme::TEXT_XS).color(t.c.foreground).pr(8.0));
    }
    group.child(button(step, &icons::PLUS, "Increase"))
}

/// `RetentionControl`: the day count while on, else "Off", then the switch (on starts at 8 days).
fn retention(s: &App, t: &T, field: &str, label: &str) -> El {
    let key = format!("storage::{field}");
    let on = !value(s, field).is_null();
    let control = if on {
        stepper(s, t, &key, label, Some("days"), 1)
    } else {
        text("Off").font(theme::TEXT_XS).color(t.c.muted_foreground)
    };
    row().gap(12.0).center_y().child(control).child(
        crate::kit::Switch::new(on)
            .label(label)
            .hide_label()
            .on_toggle(move |s: &mut App| {
                s.ui.settings.edits.remove(&key);
                update(s, &key, if on { Value::Null } else { json!(8) });
            })
            .build(&crate::kit::theme(t)),
    )
}

fn switch(s: &App, t: &T, field: &str, label: &str) -> El {
    row().w(160.0).child(el().grow(1.0)).child(c::switch(s, t, &format!("storage::{field}"), label))
}

/// A section that only explains why its settings are unavailable (`variant="plain"`).
fn notice(title: &str, copy: &str, t: &T) -> El {
    col()
        .w_full()
        .gap(10.0)
        .child(
            row()
                .w_full()
                .h(28.0)
                .px(16.0)
                .center_y()
                .child(text(title.to_owned()).font(theme::weight(theme::TEXT_BASE, theme::SEMIBOLD)).color(t.c.foreground)),
        )
        .child(text(copy.to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap().px(16.0))
}

/// Maintenance's "Artifacts and logs" and "Cleanup"; environment-wide, so hidden for a project.
pub fn maintenance(s: &App, cx: &Cx<App>, t: &T) -> Vec<El> {
    if project_scope(s) {
        return vec![];
    }
    if !capability(s, "storageCleanup") {
        return vec![notice(
            "Artifacts and logs",
            "Update the selected environments to use storage cleanup, or choose a machine that supports it.",
            t,
        )];
    }
    let artifacts = setting(
        cx,
        t,
        "storage-browser-artifacts",
        title("Delete old browser artifacts", t),
        "Delete saved browser captures after this many days. Older capture links will no longer open.",
        retention(s, t, "browserArtifactsAfterDays", "Delete old browser artifacts"),
    );
    let logs = setting(
        cx,
        t,
        "storage-logs",
        title("Delete old rotated logs", t),
        "Delete inactive rotated log files after this many days. Current logs are kept.",
        retention(s, t, "logsAfterDays", "Delete old rotated logs"),
    );
    let mut out = vec![c::section("Artifacts and logs", vec![artifacts, logs], t)];
    if capability(s, "storageCleanupRun") {
        out.push(cleanup(s, cx, t));
    }
    out
}

fn cleanup(s: &App, cx: &Cx<App>, t: &T) -> El {
    let ui = &s.ui.settings.maintenance;
    let pending = ui.run.is_some();
    let rules = has_rules(server_settings(s));
    let streamed = super::resource(s, REPORT, json!({}));
    let failed = streamed.is_some_and(|v| v.get("error").is_some());
    let report = streamed.filter(|v| v.is_object() && v.get("error").is_none()).or(ui.report.as_ref());
    let description = match report {
        _ if !rules => "Turn on a cleanup rule to use this.".to_owned(),
        Some(report) => last_run(report, s.now_ms),
        None if failed => "Could not load the last run.".to_owned(),
        None => "Cleanup hasn't run yet.".to_owned(),
    };
    let delete = crate::kit::Button::new(if pending { "Deleting…" } else { "Delete now" })
        .variant("outline")
        .size("sm")
        .disabled(pending || !rules || !super::permissions::allowed(s, "server.runStorageCleanup"))
        .on_press(run)
        .build(t);
    let mut section = c::section("Cleanup", vec![setting(cx, t, "storage-delete-now", title("Delete now", t), &description, delete)], t);
    if let Some(report) = report {
        section = section.child(results(s, t, report));
    }
    section
}

/// The "Last run" fold: its counts, then one row per cleaned item.
fn results(s: &App, t: &T, report: &Value) -> El {
    let fold = "maintenance/last-run".to_owned();
    let open = s.ui.settings.advanced.contains(&fold);
    let header = row()
        .w_full()
        .min_h(44.0)
        .px(16.0)
        .py(8.0)
        .gap(8.0)
        .center_y()
        .focusable(true)
        .role("button")
        .label("Last run")
        .expanded(open)
        .child(rune::icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 16.0).color(t.c.muted_foreground))
        .child(text("Last run").font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(text(counts(report)).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().shrink().min_w(0.0))
        .on_press(move |s: &mut App| {
            if !s.ui.settings.advanced.remove(&fold) {
                s.ui.settings.advanced.insert(fold.clone());
            }
        });
    let mut children = vec![header];
    if open {
        let environment = environment_label(s);
        let entries = report["entries"].as_array().map(Vec::as_slice).unwrap_or_default();
        for entry in entries {
            children.push(entry_row(t, entry, &environment));
        }
        let omitted = count(&report["omittedCount"]);
        let note = if omitted > 0 {
            Some(format!("{environment}: {} more results.", grouped(omitted)))
        } else if entries.is_empty() {
            Some(format!("{environment}: Nothing needed cleanup."))
        } else {
            None
        };
        if let Some(note) = note {
            children.push(text(note).font(theme::TEXT_XS).color(t.c.muted_foreground).px(16.0).py(10.0));
        }
    }
    c::card(children, t)
}

fn entry_row(t: &T, entry: &Value, environment: &str) -> El {
    let detail = |label: &str, v: &str| {
        row()
            .gap(12.0)
            .child(text(label.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).w(56.0))
            .child(text(v.to_owned()).font(theme::TEXT_XS).color(t.c.foreground.alpha_mul(0.8)).wrap().selectable(true).grow(1.0).min_w(0.0))
    };
    let mut details = col().grow(1.0).min_w(0.0).gap(4.0).child(detail("Machine", environment));
    for (label, field) in [("Project", "projectPath"), ("Folder", "path"), ("Thread", "threadTitle")] {
        if let Some(v) = entry[field].as_str() {
            details = details.child(detail(label, v));
        }
    }
    let details = row().w_full().gap(12.0).items_stretch().child(el().w(1.0).self_stretch().fill(t.c.border.alpha_mul(0.6))).child(details);
    let reason = text(entry["reason"].as_str().unwrap_or("").to_owned())
        .font(theme::TEXT_XS)
        .color(if entry["outcome"] == "failed" { t.c.destructive } else { t.c.muted_foreground })
        .wrap()
        .selectable(true);
    let copy = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(text(entry_name(entry)).font(theme::SM_MEDIUM).color(t.c.foreground).wrap().selectable(true))
        .child(col().gap(8.0).child(reason).child(details));
    row()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(32.0)
        .center_y()
        .child(copy)
        .children(entry_size(entry).map(|size| text(size).font(theme::TEXT_XS).color(t.c.muted_foreground)))
}

/// Web `WorktreeCleanupSection`, at the end of the Worktrees page.
pub fn worktree_cleanup(s: &App, cx: &Cx<App>, t: &T) -> El {
    let project = project_scope(s);
    if project && !capability(s, "projectWorktreeCleanup") {
        return notice("Cleanup", "Update the selected machines to configure project worktree cleanup.", t);
    }
    if !capability(s, "storageCleanup") {
        return notice(
            "Cleanup",
            "Update the selected environments to use storage cleanup, or choose a machine that supports it.",
            t,
        );
    }
    let mut rows = Vec::new();
    let mode = mode(s);
    if project {
        let options: Vec<_> = [("inherit", "Inherit"), ("off", "Off"), ("custom", "Custom")]
            .into_iter()
            .map(|(v, l)| (json!(v), l.to_owned()))
            .collect();
        rows.push(setting(
            cx,
            t,
            "storage-worktree-mode",
            title("Automatic worktree cleanup", t),
            match mode {
                "off" => "Keep this project's worktrees until you delete them manually.",
                "custom" => "Use these rules for this project.",
                _ => "Use each machine's worktree cleanup settings.",
            },
            c::select(s, t, "storage::mode", "Automatic worktree cleanup", &options, c::select_width("storage::mode")),
        ));
    }
    if !project || mode == "custom" {
        rows.push(setting(
            cx,
            t,
            "storage-worktree-on-delete",
            title("Delete worktrees with deleted threads", t),
            "Remove unused worktrees when active or archived threads are deleted. Worktrees with local changes are kept.",
            switch(s, t, "worktreeOnDelete", "Delete worktrees with deleted threads"),
        ));
        rows.push(setting(
            cx,
            t,
            "storage-worktree-on-merge",
            title("Delete merged worktrees", t),
            "Remove worktrees whose pull request is merged and whose commits are included in the default branch.",
            switch(s, t, "worktreeOnMerge", "Delete merged worktrees"),
        ));
        rows.push(setting(
            cx,
            t,
            "storage-worktree-after-days",
            title("Delete inactive worktrees", t),
            "Remove worktrees after their threads have been inactive for this many days. Branches and thread history are kept.",
            retention(s, t, "worktreeAfterDays", "Delete inactive worktrees"),
        ));
        let fold = "worktrees/Cleanup".to_owned();
        let open = s.ui.settings.advanced.contains(&fold);
        rows.push(super::pages::advanced_trigger(fold, open, t));
        if open {
            if capability(s, "settledWorktreeCleanup") {
                rows.push(setting(
                    cx,
                    t,
                    "storage-worktree-settled",
                    title("Delete settled worktrees", t),
                    "Remove worktrees this many days after their thread settles. Set 0 days to remove them at the next cleanup. Branches and thread history are kept.",
                    retention(s, t, "worktreeSettledAfterDays", "Delete settled worktrees"),
                ));
            }
            rows.push(setting(
                cx,
                t,
                "storage-worktree-unchanged",
                title("Delete unchanged worktrees", t),
                "Remove worktrees with no commits beyond the default branch.",
                switch(s, t, "worktreeUnchanged", "Delete unchanged worktrees"),
            ));
            if capability(s, "storageCleanupRun") {
                let options: Vec<_> = KEEP_WHEN.iter().map(|(v, l)| (json!(v), (*l).to_owned())).collect();
                let description = match value(s, "worktreeKeepWhen").as_str() {
                    Some("any-local-files") => "Includes ignored files, except node_modules.",
                    Some("tracked-changes") => "Deletes untracked and ignored files, including .env.",
                    _ => "Keeps edits and untracked files. Deletes ignored files, including .env.",
                };
                rows.push(setting(
                    cx,
                    t,
                    "storage-worktree-keep-when",
                    title("Keep worktrees with local changes", t),
                    description,
                    c::select(
                        s,
                        t,
                        "storage::worktreeKeepWhen",
                        "Keep worktrees with local changes",
                        &options,
                        c::select_width("storage::worktreeKeepWhen"),
                    ),
                ));
            }
        }
    }
    c::section("Cleanup", rows, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_rules_follow_the_policy() {
        let storage = json!({"worktreeKeepWhen": "tracked-changes", "worktreeAfterDays": 5, "worktreeSettledAfterDays": null,
            "worktreeOnMerge": true, "worktreeOnDelete": false, "worktreeUnchanged": false, "logsAfterDays": 3});
        assert_eq!(rules(&Value::Null, &storage)["worktreeAfterDays"], 5);
        assert_eq!(rules(&Value::Null, &storage).get("logsAfterDays"), None);
        let off = rules(&json!({"mode": "off"}), &storage);
        assert_eq!((off["worktreeKeepWhen"].as_str(), off["worktreeOnMerge"].as_bool()), (Some("tracked-changes"), Some(false)));
        let custom = rules(&json!({"mode": "custom", "rules": {"worktreeOnDelete": true}}), &storage);
        assert_eq!(custom["worktreeOnDelete"], true);
        assert_eq!(custom["worktreeOnMerge"], false, "custom rules don't fall back to the machine's");
        assert_eq!(custom["worktreeKeepWhen"], "uncommitted-changes");
    }

    #[test]
    fn delete_now_needs_a_rule_somewhere() {
        assert!(!has_rules(&json!({"storageCleanup": {"worktreeKeepWhen": "any-local-files"}})));
        assert!(has_rules(&json!({"storageCleanup": {"logsAfterDays": 8}})));
        assert!(has_rules(&json!({"storageCleanup": {"worktreeOnMerge": true}})));
        assert!(!has_rules(&json!({"storageCleanup": {"worktreeOnMerge": true}, "worktreeCleanup": {"mode": "off"}})));
        assert!(has_rules(&json!({"storageCleanup": {}, "projectSettingsOverrides": {
            "p": {"worktreeCleanup": {"mode": "custom", "rules": {"worktreeSettledAfterDays": 0}}}
        }})));
        assert!(!has_rules(&json!({"storageCleanup": {}, "projectSettingsOverrides": {
            "p": {"worktreeCleanup": {"mode": "off"}}
        }})));
    }

    #[test]
    fn a_machine_policy_moves_with_its_worktree_rules() {
        let storage = json!({"worktreeOnMerge": false, "logsAfterDays": null});
        assert_eq!(worktree_patch(&storage, &Value::Null, "worktreeOnMerge", json!(true)), json!({"storageCleanup": {"worktreeOnMerge": true, "logsAfterDays": null}}));
        let patch = worktree_patch(&storage, &json!({"mode": "custom", "rules": {"worktreeUnchanged": true}}), "worktreeOnMerge", json!(true));
        assert_eq!(patch["worktreeCleanup"]["rules"]["worktreeUnchanged"], true);
        assert_eq!(patch["worktreeCleanup"]["rules"]["worktreeOnMerge"], true);
        assert_eq!(patch["worktreeCleanup"]["mode"], "custom");
    }

    #[test]
    fn day_counts_round_and_clamp() {
        assert_eq!(days(&json!(2.6), 1), Some(json!(3)));
        assert_eq!(days(&json!(0), 1), Some(json!(1)));
        assert_eq!(days(&json!(0), 0), Some(json!(0)));
        assert_eq!(days(&json!(9999), 1), Some(json!(3650)));
        assert_eq!(days(&Value::Null, 1), Some(Value::Null));
        assert_eq!(days(&json!("x"), 1), None);
    }

    #[test]
    fn last_run_reads_like_web() {
        assert_eq!(format_bytes(1023.4), "1023 B");
        assert_eq!(format_bytes(1536.0), "1.50 KB");
        assert_eq!(format_bytes(1024.0 * 1023.9), "1.00 MB");
        assert_eq!(format_bytes(150.0 * 1024.0 * 1024.0), "150 MB");
        assert_eq!(grouped(1234567), "1,234,567");
        assert_eq!(grouped(999), "999");
        let at = crate::data::iso_ms("2026-11-06T09:00:00.000Z").unwrap();
        assert_eq!(relative("2026-11-06T09:00:00.000Z", at + 59_000), "just now");
        assert_eq!(relative("2026-11-06T09:00:00.000Z", at + 5 * 60_000), "5m ago");
        assert_eq!(relative("2026-11-06T09:00:00.000Z", at + 50 * 3_600_000), "2d ago");
        let report = json!({"finishedAt": "2026-11-06T09:00:00.000Z", "bytesFreed": 2048, "counts": {"removed": 2, "kept": 1, "failed": 1}});
        assert_eq!(last_run(&report, at + 2 * 3_600_000), "Last run 2h ago. 2.00 KB recovered. 1 item couldn't be removed.");
        assert_eq!(counts(&report), "2 removed, 1 kept, 1 failed");
    }

    #[test]
    fn entries_name_their_item() {
        assert_eq!(entry_name(&json!({"kind": "worktree", "threadId": "t", "path": "/w/repo/feature/"})), "feature");
        assert_eq!(entry_name(&json!({"kind": "worktree", "threadId": null, "path": "/w/repo"})), "/w/repo");
        assert_eq!(entry_name(&json!({"kind": "worktree", "threadId": null, "path": null})), "Worktrees");
        assert_eq!(entry_name(&json!({"kind": "logs"})), "Rotated logs");
        assert_eq!(entry_size(&json!({"kind": "logs", "outcome": "removed", "files": 1, "bytes": 10})).as_deref(), Some("1 file · 10 B"));
        assert_eq!(entry_size(&json!({"kind": "worktree", "outcome": "kept", "files": 3, "bytes": null})), None);
        assert_eq!(entry_size(&json!({"kind": "worktree", "outcome": "removed", "files": 3, "bytes": null})).as_deref(), Some(""));
    }
}
