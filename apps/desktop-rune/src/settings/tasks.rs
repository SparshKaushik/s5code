//! Web `ScheduledTasksSettings`: every environment's tasks in scope, each row's switch and menu,
//! webhook deliveries, and the editor (`tasks/editor.rs`).
mod draft;
mod editor;

use super::connections::{dot, machine_icon, machine_kind};
use crate::data::{Source, Zone};
use crate::kit::{self, Menu, MenuItem, Placement};
use crate::shell::palette::toast_error;
use crate::theme::{self, AlphaMul, T};
use crate::{App, Route, env, icons};
use rune::{El, TextEdit, col, icon, row, text};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

const SUBSCRIBE: &str = "scheduledTasks.subscribe";
const PAGE: &str = "scheduled-tasks";

#[derive(Default)]
pub struct Ui {
    /// Environments with a live `scheduledTasks.subscribe`.
    watching: Vec<String>,
    pending: Vec<Pending>,
    /// Tasks with a switch, run or delete in flight.
    busy: HashSet<String>,
    /// The row menu that is open: environment and task id.
    menu: Option<(String, String)>,
    editor: Option<editor::Editor>,
    deliveries: Option<Deliveries>,
    /// The webhook URL rotation awaiting confirmation: environment and task id.
    rotate: Option<(String, String)>,
    /// Scenes: each environment's tasks, standing in for its server.
    scene: Option<HashMap<String, Vec<Value>>>,
    /// What an edit lands in while no editor is open.
    scratch: TextEdit,
}

struct Pending {
    env: String,
    id: u64,
    kind: Kind,
}

enum Kind {
    Upsert,
    Action(String),
    Rotate,
    Deliveries,
    Delivery,
}

/// Web `WebhookDeliveriesDialog`.
struct Deliveries {
    env: String,
    task: Value,
    list: Option<Result<Vec<Value>, String>>,
    selected: Option<String>,
    detail: Option<Result<Value, String>>,
}

/// One environment's task list as the page shows it.
#[derive(Debug, PartialEq)]
enum List {
    Disconnected,
    Loading,
    Failed(String),
    Tasks(Vec<Value>),
}

fn on_page(s: &App) -> bool {
    matches!(&s.route, Route::Settings(p) if p == PAGE)
}

/// Web `useSettingsScope().environments`: the scoped project's, the one on screen, or all.
fn environments(s: &App) -> Vec<String> {
    if let Some(project) = &s.ui.settings.project {
        return vec![env::owner(s, project)];
    }
    if s.ui.settings.environment_only {
        return vec![s.ui.env.active.clone()];
    }
    env::keys(s)
}

/// Connected with a server config, as web's `phase === "connected" && serverConfig !== null`.
fn connected(s: &App, key: &str) -> bool {
    env::connected(s, key) && config(s, key).is_some()
}

fn config<'a>(s: &'a App, key: &str) -> Option<&'a Value> {
    env::source(s, key)?.state().config.as_ref()
}

/// Web `matchesScheduledTaskScope` for a task already on an environment in scope.
fn in_scope(s: &App, project_id: &Value) -> bool {
    s.ui.settings.project.as_ref().is_none_or(|p| project_id == p)
}

fn list(s: &App, key: &str) -> List {
    if let Some(tasks) = s.ui.settings.tasks.scene.as_ref().and_then(|scene| scene.get(key)) {
        return List::Tasks(tasks.iter().filter(|t| in_scope(s, &t["projectId"])).cloned().collect());
    }
    if !connected(s, key) {
        return List::Disconnected;
    }
    let reply = match env::source(s, key) {
        Some(Source::Live(net)) => net.resource(SUBSCRIBE, json!({})),
        // A fixture has no task store.
        _ => return List::Tasks(Vec::new()),
    };
    match reply {
        None => List::Loading,
        Some(v) if !v["error"].is_null() => List::Failed(failure(&v["error"]["cause"])),
        Some(v) => List::Tasks(v["tasks"].as_array().into_iter().flatten().filter(|t| in_scope(s, &t["projectId"])).cloned().collect()),
    }
}

/// The tasks an environment's server listed, whatever the scope.
fn all_tasks(s: &App, key: &str) -> Option<Vec<Value>> {
    if let Some(tasks) = s.ui.settings.tasks.scene.as_ref().and_then(|scene| scene.get(key)) {
        return Some(tasks.clone());
    }
    match env::source(s, key)? {
        Source::Live(net) => net.resource(SUBSCRIBE, json!({})).filter(|v| v["error"].is_null()).map(|v| v["tasks"].as_array().cloned().unwrap_or_default()),
        _ => Some(Vec::new()),
    }
}

fn failure(cause: &Value) -> String {
    super::failure_message("", "", cause)
}

/// Web `serverEnvironment.upsertScheduledTask.permissionAtom`.
fn can_operate(s: &App, key: &str) -> bool {
    super::permissions::can_on(s, key, "orchestration:operate")
}

fn machine(s: &App, key: &str) -> &'static rune::Icon {
    let config = config(s, key);
    machine_icon(machine_kind(
        config.and_then(|c| c["settings"]["environmentIcon"].as_str()),
        config.and_then(|c| c["environment"]["platform"]["machine"].as_str()),
    ))
}

/// Web `settingsScopeEnvironmentLabels`: a second line only when another environment shares the name.
fn labels(s: &App, key: &str, among: &[String]) -> (String, String) {
    let label = env::label(s, key);
    let address = |k: &str| env::base_url(s, k).map(|b| env::route_host(&b)).filter(|h| !h.is_empty()).unwrap_or_else(|| k.to_owned());
    let duplicates: Vec<&String> = among.iter().filter(|k| *k != key && env::label(s, k) == label).collect();
    if duplicates.is_empty() {
        return (label, String::new());
    }
    let mine = address(key);
    let secondary = if duplicates.iter().any(|k| address(k) == mine) { env::base_url(s, key).unwrap_or_else(|| key.to_owned()) } else { mine };
    (label, secondary)
}

/// Web `environmentTransportLabel`.
fn transport(s: &App, key: &str) -> String {
    if key == env::LOCAL {
        return "This machine".into();
    }
    env::ssh::transport(s, key).or_else(|| env::base_url(s, key)).unwrap_or_default()
}

fn status_color(s: &App, key: &str, t: &T) -> rune::Color {
    match env::short_status(s, key) {
        "Connected" => t.c.success,
        "Connecting…" | "Reconnecting…" => t.c.warning,
        _ => t.c.destructive,
    }
}

fn send(s: &mut App, key: &str, method: &str, payload: Value, kind: Kind) -> bool {
    let Some(id) = env::net_of(s, key).map(|net| net.request(method, payload)) else { return false };
    s.ui.settings.tasks.pending.push(Pending { env: key.to_owned(), id, kind });
    true
}

/// Web `ScheduledTaskRow`'s `act`: the switch, Run now and Delete.
fn act(s: &mut App, key: &str, task: &Value, method: &str) {
    let id = task["id"].as_str().unwrap_or_default().to_owned();
    if s.ui.settings.tasks.busy.contains(&id) || !can_operate(s, key) {
        return;
    }
    let payload = match method {
        "scheduledTasks.setEnabled" => json!({"id": id, "enabled": task["enabled"] != true}),
        _ => json!({"id": id}),
    };
    if send(s, key, method, payload, Kind::Action(id.clone())) {
        s.ui.settings.tasks.busy.insert(id);
    }
}

fn load_deliveries(s: &mut App) {
    let Some(d) = &s.ui.settings.tasks.deliveries else { return };
    let (key, id) = (d.env.clone(), d.task["id"].clone());
    if send(s, &key, "scheduledTasks.listWebhookDeliveries", json!({"id": id, "acceptAnyOutcome": true}), Kind::Deliveries) {
        if let Some(d) = &mut s.ui.settings.tasks.deliveries {
            d.list = None;
        }
    }
}

fn select_delivery(s: &mut App, delivery: &str) {
    let Some(d) = &mut s.ui.settings.tasks.deliveries else { return };
    d.selected = Some(delivery.to_owned());
    d.detail = None;
    let (key, id) = (d.env.clone(), d.task["id"].clone());
    send(s, &key, "scheduledTasks.getWebhookDelivery", json!({"id": id, "deliveryId": delivery, "acceptAnyOutcome": true}), Kind::Delivery);
}

pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    let page = on_page(s);
    if !page && (s.ui.settings.tasks.editor.is_some() || s.ui.settings.tasks.menu.is_some() || s.ui.settings.tasks.deliveries.is_some()) {
        let ui = &mut s.ui.settings.tasks;
        ui.editor = None;
        ui.menu = None;
        ui.deliveries = None;
        ui.rotate = None;
        changed = true;
    }
    let mut wanted: Vec<String> = if page { environments(s).into_iter().filter(|k| connected(s, k)).collect() } else { Vec::new() };
    if let Some(editor) = s.ui.settings.tasks.editor.as_ref().filter(|e| !wanted.contains(&e.env) && connected(s, &e.env)) {
        wanted.push(editor.env.clone());
    }
    if wanted != s.ui.settings.tasks.watching {
        let before = std::mem::take(&mut s.ui.settings.tasks.watching);
        for key in before.iter().filter(|k| !wanted.contains(k)) {
            if let Some(net) = env::net_of(s, key) {
                net.unwatch_resource(SUBSCRIBE, json!({}));
            }
        }
        for key in &wanted {
            if let Some(net) = env::net_of(s, key) {
                net.watch_resource(SUBSCRIBE, json!({}));
            }
        }
        s.ui.settings.tasks.watching = wanted;
    }
    for pending in std::mem::take(&mut s.ui.settings.tasks.pending) {
        let Some(reply) = env::net_of(s, &pending.env).and_then(|net| net.take_reply(pending.id)) else {
            // The environment is gone or still answering; a vanished one never will.
            if env::source(s, &pending.env).is_some() {
                s.ui.settings.tasks.pending.push(pending);
            }
            continue;
        };
        changed = true;
        landed(s, pending, reply);
    }
    changed
}

fn landed(s: &mut App, pending: Pending, reply: Result<Value, Value>) {
    let ui = &mut s.ui.settings.tasks;
    match pending.kind {
        Kind::Upsert => {
            let editor = ui.editor.as_mut().filter(|e| e.env == pending.env);
            match reply {
                Ok(_) => {
                    if editor.is_some() {
                        ui.editor = None;
                    }
                }
                Err(error) => {
                    if let Some(editor) = editor {
                        editor.saving = false;
                    }
                    toast_error(s, "Could not save scheduled task", Some(&failure(&error)));
                }
            }
        }
        Kind::Action(id) => {
            ui.busy.remove(&id);
            if let Err(error) = reply {
                toast_error(s, "Could not update scheduled task", Some(&failure(&error)));
            }
        }
        Kind::Rotate => {
            if let Err(error) = reply {
                toast_error(s, "Could not rotate webhook URL", Some(&failure(&error)));
            }
        }
        Kind::Deliveries => {
            if let Some(d) = ui.deliveries.as_mut().filter(|d| d.env == pending.env) {
                d.list = Some(reply.map(|v| v["deliveries"].as_array().cloned().unwrap_or_default()).map_err(|e| failure(&e)));
            }
        }
        Kind::Delivery => {
            if let Some(d) = ui.deliveries.as_mut().filter(|d| d.env == pending.env && d.selected.is_some()) {
                d.detail = Some(reply.map(|v| v["delivery"].clone()).map_err(|e| failure(&e)));
            }
        }
    }
}

/// Escape closes the innermost open layer.
pub fn escape(s: &mut App) -> bool {
    let ui = &mut s.ui.settings.tasks;
    if ui.rotate.take().is_some() || ui.menu.take().is_some() {
        return true;
    }
    if editor::escape(s) {
        return true;
    }
    let ui = &mut s.ui.settings.tasks;
    if let Some(d) = &mut ui.deliveries {
        if d.selected.take().is_some() {
            d.detail = None;
        } else {
            ui.deliveries = None;
        }
        return true;
    }
    if ui.editor.as_ref().is_some_and(|e| !e.saving) {
        ui.editor = None;
        return true;
    }
    false
}

fn header(t: &T, new_task: El) -> El {
    row()
        .w_full()
        .h(28.0)
        .px(16.0)
        .gap(8.0)
        .center_y()
        .child(text("Scheduled tasks").font(theme::weight(theme::TEXT_BASE, theme::SEMIBOLD)).color(t.c.foreground).grow(1.0))
        .child(new_task)
}

/// A `SettingsRow` with no control.
fn info(t: &T, title: &str, description: Option<&str>) -> El {
    let mut c = col().w_full().px(16.0).py(12.0).gap(4.0).child(text(title.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground));
    if let Some(description) = description {
        c = c.child(text(description.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap());
    }
    c
}

pub fn view(s: &App, t: &T) -> El {
    let envs = environments(s);
    // Web: the active environment when permitted, else the first permitted connected one.
    let usable: Vec<&String> = envs.iter().filter(|k| connected(s, k) && can_operate(s, k)).collect();
    let creation = usable.iter().find(|k| ***k == s.ui.env.active).or(usable.first()).map(|k| (*k).clone());
    let new_task = kit::Button::new("New task")
        .variant("ghost-muted")
        .size("xs")
        .icon(&icons::PLUS)
        .disabled(creation.is_none())
        .on_press(move |s: &mut App| {
            if let Some(key) = &creation {
                editor::open(s, key, None);
            }
        })
        .build(t);
    let page = col().w_full().gap(10.0).child(header(t, new_task));
    if envs.is_empty() {
        return page.child(
            col()
                .w_full()
                .center()
                .gap(8.0)
                .child(icon(&icons::CLOCK_3, 18.0).color(t.c.muted_foreground))
                .child(kit::empty("No environments available", "Connect an environment to manage scheduled tasks.", t)),
        );
    }
    if envs.len() > 1 {
        return page.child(super::controls::card(environment_list(s, t, &envs), t));
    }
    page.child(super::controls::card(environment_section(s, t, &envs[0]), t))
}

/// Web `ScheduledTaskEnvironmentList`: environments with tasks, then a line per kind of gap.
fn environment_list(s: &App, t: &T, envs: &[String]) -> Vec<El> {
    let states: Vec<(String, List)> = envs.iter().map(|k| (k.clone(), list(s, k))).collect();
    let loaded = states.iter().filter(|(_, l)| matches!(l, List::Tasks(_))).count();
    let populated = states.iter().filter(|(_, l)| matches!(l, List::Tasks(t) if !t.is_empty())).count();
    let mut rows = Vec::new();
    for (key, state) in &states {
        let List::Tasks(tasks) = state else { continue };
        if tasks.is_empty() {
            continue;
        }
        let (primary, secondary) = labels(s, key, envs);
        let transport = transport(s, key);
        let mut subtitle = col().gap(0.0);
        if !secondary.is_empty() && secondary != transport {
            subtitle = subtitle.child(text(secondary).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate());
        }
        subtitle = subtitle.child(text(transport).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate());
        let status = env::short_status(s, key);
        rows.push(
            row()
                .w_full()
                .px(16.0)
                .py(10.0)
                .gap(12.0)
                .center_y()
                .child(icon(machine(s, key), 16.0).color(t.c.muted_foreground))
                .child(col().grow(1.0).min_w(0.0).child(text(primary).font(theme::SM_MEDIUM).color(t.c.foreground).truncate()).child(subtitle))
                .child(dot(status_color(s, key, t)).help(status))
                .child(text(status).font(theme::TEXT_XS).color(t.c.muted_foreground)),
        );
        rows.extend(tasks.iter().map(|task| task_row(s, t, key, task)));
    }
    if populated == 0 && loaded > 0 {
        rows.push(info(t, "No scheduled tasks", Some("No tasks match this environment and project selection.")));
    }
    for line in summaries(s, envs, &states, populated) {
        rows.push(text(line.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().help(line).role("status").px(16.0).py(10.0));
    }
    rows
}

/// Web's summary lines under the list, in its order.
fn summaries(s: &App, envs: &[String], states: &[(String, List)], populated: usize) -> Vec<String> {
    let mut groups: [(&str, Vec<String>); 4] =
        [("Not connected:", vec![]), ("Loading scheduled tasks on", vec![]), ("Could not load scheduled tasks:", vec![]), ("No tasks on", vec![])];
    for (key, state) in states {
        let (primary, secondary) = labels(s, key, envs);
        let label = if secondary.is_empty() { primary } else { format!("{primary} · {secondary}") };
        match state {
            List::Disconnected => groups[0].1.push(format!("{label} ({})", env::short_status(s, key))),
            List::Failed(error) => groups[2].1.push(format!("{label}: {error}")),
            List::Loading => groups[1].1.push(label),
            List::Tasks(tasks) if tasks.is_empty() && populated > 0 => groups[3].1.push(label),
            List::Tasks(_) => {}
        }
    }
    groups.into_iter().filter(|(_, entries)| !entries.is_empty()).map(|(label, entries)| format!("{label} {}", entries.join(", "))).collect()
}

/// Web `ScheduledTaskEnvironmentSection`.
fn environment_section(s: &App, t: &T, key: &str) -> Vec<El> {
    match list(s, key) {
        List::Disconnected => {
            vec![info(t, "Environment disconnected", Some(&format!("Reconnect {} to view its scheduled tasks.", env::label(s, key))))]
        }
        List::Failed(error) => vec![info(t, "Could not load scheduled tasks", Some(&error))],
        List::Loading => vec![info(t, "Loading scheduled tasks…", None).role("status")],
        List::Tasks(tasks) if tasks.is_empty() => vec![info(t, "No scheduled tasks", Some("No tasks match this environment and project selection."))],
        List::Tasks(tasks) => tasks.iter().map(|task| task_row(s, t, key, task)).collect(),
    }
}

fn status_variant(status: &str) -> &'static str {
    match status {
        "failed" => "error",
        "succeeded" => "success",
        "running" => "info",
        _ => "outline",
    }
}

/// Web `ScheduledTaskRow`'s status line.
fn status_line(task: &Value, now: i64) -> String {
    let schedule = draft::schedule_label(&task["schedule"], Zone::Local);
    let state = if let Some(blocked) = task["blockedByPolicy"].as_str() {
        blocked.to_owned()
    } else if task["enabled"] != true {
        "Paused".into()
    } else if task["schedule"]["type"] == "webhook" {
        "Listening".into()
    } else if let Some(next) = task["nextRunAt"].as_str() {
        format!("Next run {}", draft::relative_label(Some(next), now))
    } else {
        "Not scheduled".into()
    };
    format!("{schedule} · {state}")
}

fn task_row(s: &App, t: &T, key: &str, task: &Value) -> El {
    let id = task["id"].as_str().unwrap_or_default().to_owned();
    let title = task["title"].as_str().unwrap_or("Task").to_owned();
    let enabled = task["enabled"] == true;
    let usable = !s.ui.settings.tasks.busy.contains(&id) && can_operate(s, key);
    let mut status = row().gap(8.0).center_y().flex_wrap().child(text(status_line(task, s.now_ms)).font(theme::TEXT_XS).color(t.c.muted_foreground));
    let last = task["lastRunStatus"].as_str().unwrap_or("never");
    if last != "never" {
        status = status.child(kit::badge(last, status_variant(last), "default", t));
    }
    if let Some(error) = task["lastRunError"].as_str() {
        status = status.child(text(error.to_owned()).font(theme::TEXT_XS).color(t.c.destructive).wrap());
    }
    let (toggle_key, toggle_task) = (key.to_owned(), task.clone());
    let (menu_key, menu_id) = (key.to_owned(), id.clone());
    let switch = kit::Switch::new(enabled)
        .label(format!("Enable {title}"))
        .hide_label()
        .disabled(!usable)
        .on_toggle(move |s: &mut App| act(s, &toggle_key, &toggle_task, "scheduledTasks.setEnabled"))
        .build(&kit::theme(t));
    let more = row().key(format!("task-more-{id}")).child(
        kit::icon_button(&icons::ELLIPSIS, &format!("Actions for {title}"))
            .size("icon-sm")
            .disabled(!usable)
            .on_press(move |s: &mut App| s.ui.settings.tasks.menu = Some((menu_key.clone(), menu_id.clone())))
            .build(t),
    );
    row()
        .key(format!("task-{id}"))
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(16.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(text(title).font(theme::SM_MEDIUM).color(t.c.foreground).wrap())
                .child(text(task["prompt"].as_str().unwrap_or_default().to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap())
                .child(status),
        )
        .child(row().gap(8.0).center_y().child(switch).child(more))
}

fn task_on(s: &App, key: &str, id: &str) -> Option<Value> {
    all_tasks(s, key)?.into_iter().find(|t| t["id"] == id)
}

/// Overlays: the row menu, the editor, deliveries and the rotation confirmation.
pub fn editor(s: &App, t: &T, layers: &mut Vec<El>) {
    row_menu(s, t, layers);
    editor::dialog(s, t, layers);
    deliveries_dialog(s, t, layers);
    rotate_dialog(s, t, layers);
}

/// Web `ScheduledTaskRow`'s menu: Edit, Deliveries or Run now, then Delete.
fn row_menu(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some((key, id)) = &s.ui.settings.tasks.menu else { return };
    let Some(task) = task_on(s, key, id) else { return };
    let close = |s: &mut App| s.ui.settings.tasks.menu = None;
    let (edit_key, edit_task) = (key.clone(), task.clone());
    let mut menu = Menu::new("task-menu")
        .anchor(format!("task-more-{id}"))
        .placement(Placement::BottomEnd)
        .gutter(4.0)
        .open(true)
        .on_close(close)
        .item(MenuItem::new("Edit").icon(&icons::PENCIL).on_select(move |s: &mut App| {
            close(s);
            editor::open(s, &edit_key, Some(edit_task.clone()));
        }));
    let (action_key, action_task) = (key.clone(), task.clone());
    menu = if task["schedule"]["type"] == "webhook" {
        menu.item(MenuItem::new("Deliveries").icon(&icons::INBOX).on_select(move |s: &mut App| {
            close(s);
            s.ui.settings.tasks.deliveries = Some(Deliveries { env: action_key.clone(), task: action_task.clone(), list: None, selected: None, detail: None });
            load_deliveries(s);
        }))
    } else {
        menu.item(MenuItem::new("Run now").icon(&icons::PLAY).on_select(move |s: &mut App| {
            close(s);
            act(s, &action_key, &action_task, "scheduledTasks.runNow");
        }))
    };
    let (delete_key, delete_task) = (key.clone(), task);
    menu.separator()
        .item(MenuItem::new("Delete").icon(&icons::TRASH_2).on_select(move |s: &mut App| {
            close(s);
            act(s, &delete_key, &delete_task, "scheduledTasks.delete");
        }))
        .overlay(&kit::theme(t), layers);
}

fn block(t: &T, title: &str, body: String) -> El {
    col()
        .w_full()
        .gap(4.0)
        .child(text(title.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(
            col()
                .w_full()
                .max_h(256.0)
                .scroll(rune::ScrollSpec { stick_bottom: false })
                .radius(6.0)
                .fill(t.c.muted)
                .padding(8.0)
                .child(text(body).font(theme::mono(theme::TEXT_XS)).color(t.c.foreground).wrap()),
        )
}

/// Web `WebhookDeliveriesDialog`.
fn deliveries_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(d) = &s.ui.settings.tasks.deliveries else { return };
    let muted = |line: &str| text(line.to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap();
    let mut body = col().w_full().gap(16.0);
    let error = match (&d.selected, &d.list, &d.detail) {
        (None, Some(Err(e)), _) | (Some(_), _, Some(Err(e))) => Some(e.clone()),
        _ => None,
    };
    if let Some(error) = error {
        body = body.child(text(error).font(theme::TEXT_SM).color(t.c.destructive).wrap());
    }
    match (&d.selected, &d.detail, &d.list) {
        (Some(_), None, _) => body = body.child(muted("Loading delivery…").role("status")),
        (Some(_), Some(Ok(selected)), _) => {
            let mut head = row()
                .gap(8.0)
                .center_y()
                .flex_wrap()
                .child(kit::badge(&draft::delivery_label(selected), draft::delivery_variant(selected), "default", t))
                .child(
                    text(format!(
                        "{} · {}",
                        selected["method"].as_str().unwrap_or_default(),
                        draft::relative_label(selected["receivedAt"].as_str(), s.now_ms)
                    ))
                    .font(theme::TEXT_SM)
                    .color(t.c.foreground),
                );
            if selected["signatureVerified"] == true {
                head = head.child(muted("Signature verified"));
            }
            body = body.child(head);
            if let Some(error) = selected["error"].as_str() {
                let color = if selected["outcome"] == "filtered" { t.c.muted_foreground } else { t.c.destructive };
                body = body.child(text(error.to_owned()).font(theme::TEXT_SM).color(color).wrap());
            }
            let pending_window = selected["pendingWindow"] == true;
            let prompt = selected["renderedPrompt"].as_str().map(str::to_owned).unwrap_or_else(|| {
                if pending_window { "Run scheduled when the window closes; no run started yet." } else { "No run was started for this request." }.into()
            });
            body = body.child(block(t, if pending_window { "Scheduled run" } else { "Prompt sent to the agent" }, prompt));
            let missing: Vec<&str> = selected["missingFields"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            if !missing.is_empty() {
                body = body.child(muted(&format!("Empty placeholders: {}", missing.join(", "))));
            }
            let headers = selected["headers"].as_object().map(|h| h.iter().map(|(k, v)| format!("{k}: {}", v.as_str().unwrap_or_default())).collect::<Vec<_>>().join("\n")).unwrap_or_default();
            body = body.child(block(t, "Headers", headers));
            if let Some(query) = selected["query"].as_str().filter(|q| !q.is_empty()) {
                body = body.child(block(t, "Query", query.to_owned()));
            }
            let raw = selected["body"].as_str().filter(|b| !b.is_empty()).unwrap_or("(empty)").to_owned();
            body = body.child(block(t, if selected["bodyTruncated"] == true { "Body (truncated)" } else { "Body" }, raw));
        }
        (Some(_), Some(Err(_)), _) => {}
        (None, _, None) => body = body.child(muted("Loading deliveries…").role("status")),
        (None, _, Some(Err(_))) => {}
        (None, _, Some(Ok(list))) if list.is_empty() => body = body.child(muted("No requests yet.")),
        (None, _, Some(Ok(list))) => {
            let mut rows = col().w_full();
            for (i, delivery) in list.iter().enumerate() {
                if i > 0 {
                    rows = rows.child(rune::el().w_full().h(1.0).fill(t.c.border));
                }
                let mut line = row()
                    .w_full()
                    .py(8.0)
                    .gap(8.0)
                    .center_y()
                    .flex_wrap()
                    .role("button")
                    .focusable(true)
                    .hover(t.c.accent.alpha_mul(0.4))
                    .child(kit::badge(&draft::delivery_label(delivery), draft::delivery_variant(delivery), "default", t))
                    .child(text(delivery["method"].as_str().unwrap_or_default().to_owned()).font(theme::TEXT_SM).color(t.c.foreground))
                    .child(muted(&draft::relative_label(delivery["receivedAt"].as_str(), s.now_ms)));
                if let Some(error) = delivery["error"].as_str().filter(|_| delivery["outcome"] == "filtered") {
                    line = line.child(muted(error));
                }
                let missing = delivery["missingFields"].as_array().map_or(0, Vec::len);
                if missing > 0 {
                    line = line.child(muted(&format!("{missing} empty placeholder{}", if missing == 1 { "" } else { "s" })));
                }
                let id = delivery["id"].as_str().unwrap_or_default().to_owned();
                rows = rows.child(line.on_press(move |s: &mut App| select_delivery(s, &id)));
            }
            body = body.child(rows);
        }
    }
    let back = d.selected.is_some();
    let loading = d.list.is_none();
    let close = |s: &mut App| s.ui.settings.tasks.deliveries = None;
    let footer = row()
        .w_full()
        .gap(8.0)
        .child(rune::el().grow(1.0))
        .child(if back {
            kit::Button::new("Back")
                .variant("outline")
                .size("sm")
                .on_press(|s: &mut App| {
                    if let Some(d) = &mut s.ui.settings.tasks.deliveries {
                        d.selected = None;
                        d.detail = None;
                    }
                })
                .build(t)
        } else {
            kit::Button::new("Refresh").variant("outline").size("sm").disabled(loading).on_press(load_deliveries).build(t)
        })
        .child(kit::Button::new("Done").size("sm").on_press(close).build(t));
    kit::Dialog::new("task-deliveries", format!("Deliveries · {}", d.task["title"].as_str().unwrap_or_default()))
        .open(true)
        .width(672.0)
        .scroll_all()
        .description("Recent requests to this task's webhook URL.")
        .on_close(close)
        .child(body)
        .child(footer)
        .build(t, layers);
}

/// Web's confirmation before `rotateScheduledTaskWebhookToken`.
fn rotate_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some((key, id)) = s.ui.settings.tasks.rotate.clone() else { return };
    let close = |s: &mut App| s.ui.settings.tasks.rotate = None;
    kit::Dialog::new("rotate-webhook", "Rotate this webhook URL?")
        .open(true)
        .alert()
        .description("The current URL stops working.")
        .on_close(close)
        .child(
            row()
                .gap(8.0)
                .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                .child(
                    kit::Button::new("Confirm")
                        .variant("destructive")
                        .on_press(move |s: &mut App| {
                            s.ui.settings.tasks.rotate = None;
                            send(s, &key, "scheduledTasks.rotateWebhookToken", json!({"id": id}), Kind::Rotate);
                        })
                        .build(t),
                ),
        )
        .build(t, layers);
}

/// Scenes: tasks on this machine's fixture, and which overlay is open.
pub(super) fn scene(s: &mut App, open: &str) {
    let (project, now) = (s_project(s), s.now_ms);
    let fixture = |id: &str, title: &str, schedule: Value, enabled: bool, status: &str| {
        let webhook = if schedule_is_webhook(&schedule) { json!({"path": "/api/webhooks/tasks/abc123", "url": null, "hasSecret": true}) } else { Value::Null };
        json!({
            "id": id, "title": title, "prompt": "Check Sentry for new issues and open a thread for each regression.",
            "enabled": enabled, "schedule": schedule, "projectId": project, "threadId": null,
            "workspaceStrategy": {"type": "worktree", "baseRef": "main", "startFromOrigin": true},
            "modelSelection": {"instanceId": "codex", "model": "gpt-5.5"}, "runtimeMode": "full-access", "interactionMode": "default",
            "nextRunAt": draft::iso(now + 3 * 3_600_000), "lastRunAt": null,
            "lastRunStatus": status, "lastRunError": null, "runCount": 3,
            "webhook": webhook,
        })
    };
    let tasks = vec![
        fixture("task-sentry", "Check for Sentry issues", json!({"type": "fixed_time", "timeOfDay": "09:00", "weekdays": [1, 2, 3, 4, 5]}), true, "succeeded"),
        fixture("task-deps", "Update dependencies", json!({"type": "interval", "everyMs": 3_600_000}), false, "failed"),
        fixture("task-hook", "Triage new issues", json!({"type": "webhook", "signature": null, "maxDeliveryAgeMinutes": null, "dedupe": {}}), true, "never"),
    ];
    let key = s.ui.env.active.clone();
    s.ui.settings.tasks.scene = Some(HashMap::from([(key.clone(), tasks.clone())]));
    match open {
        "menu" => s.ui.settings.tasks.menu = Some((key, "task-sentry".into())),
        "editor" => editor::open(s, &key, None),
        "webhook" => editor::open(s, &key, tasks.into_iter().find(|t| t["id"] == "task-hook")),
        "deliveries" => {
            let now = s.now_ms;
            let delivery = |id: &str, outcome: &str, minutes: i64| {
                json!({"id": id, "method": "POST", "outcome": outcome, "receivedAt": draft::iso(now - minutes * 60_000), "missingFields": [], "error": null, "signatureVerified": false})
            };
            let list = vec![delivery("d1", "accepted", 4), delivery("d2", "duplicate", 30), delivery("d3", "rejected_signature", 180)];
            s.ui.settings.tasks.deliveries = Some(Deliveries { env: key, task: tasks[2].clone(), list: Some(Ok(list)), selected: None, detail: None });
        }
        _ => {}
    }
}

fn s_project(s: &App) -> Value {
    s.state().projects.first().map(|p| p["id"].clone()).unwrap_or(Value::Null)
}

fn schedule_is_webhook(schedule: &Value) -> bool {
    schedule["type"] == "webhook"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_group_environments_like_web() {
        let s = App::new();
        let envs = vec!["a".to_owned(), "b".to_owned(), "c".to_owned(), "d".to_owned()];
        let states = vec![
            ("a".to_owned(), List::Disconnected),
            ("b".to_owned(), List::Loading),
            ("c".to_owned(), List::Failed("boom".into())),
            ("d".to_owned(), List::Tasks(vec![])),
        ];
        let lines = summaries(&s, &envs, &states, 1);
        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("Not connected: "));
        assert!(lines[1].starts_with("Loading scheduled tasks on "));
        assert!(lines[2].starts_with("Could not load scheduled tasks: ") && lines[2].ends_with(": boom"));
        assert!(lines[3].starts_with("No tasks on "));
        // With nothing populated the empty row speaks instead of "No tasks on".
        assert_eq!(summaries(&s, &envs[3..], &states[3..], 0), Vec::<String>::new());
    }

    #[test]
    fn a_task_row_states_its_schedule_like_web() {
        let now = crate::data::iso_ms("2026-11-02T12:00:00Z").unwrap();
        let task = |extra: Value| {
            let mut task = json!({"enabled": true, "schedule": {"type": "interval", "everyMs": 900_000}, "nextRunAt": "2026-11-02T12:45:00Z"});
            for (k, v) in extra.as_object().unwrap() {
                task[k] = v.clone();
            }
            task
        };
        assert_eq!(status_line(&task(json!({})), now), "Every 15 min · Next run in 45m");
        assert_eq!(status_line(&task(json!({"enabled": false})), now), "Every 15 min · Paused");
        assert_eq!(status_line(&task(json!({"blockedByPolicy": "Acme's policy does not allow webhook tasks."})), now), "Every 15 min · Acme's policy does not allow webhook tasks.");
        assert_eq!(status_line(&task(json!({"schedule": {"type": "webhook"}})), now), "On webhook · Listening");
        assert_eq!(status_line(&task(json!({"nextRunAt": null})), now), "Every 15 min · Not scheduled");
    }

    #[test]
    fn the_scenes_survive_the_first_poll() {
        let run = |open: &str| {
            let mut s = App::new();
            crate::scene_clock(&mut s);
            s.route = Route::Settings(PAGE.into());
            scene(&mut s, open);
            poll(&mut s);
            s
        };
        let s = run("menu");
        assert!(s.ui.settings.tasks.menu.is_some());
        assert_eq!(list(&s, &s.ui.env.active).ne(&List::Loading), true);
        assert!(run("editor").ui.settings.tasks.editor.is_some());
        assert!(run("deliveries").ui.settings.tasks.deliveries.is_some());
    }
}
