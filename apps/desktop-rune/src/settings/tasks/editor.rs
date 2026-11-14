//! Web `ScheduledTaskEditorDialog`: where a task runs, what it does, and when.
use super::draft::{self, Draft, Filter, Mode, Workspace};
use super::{Kind, all_tasks, can_operate, config, connected, environments, labels, machine, send};
use crate::data::Zone;
use crate::kit;
use crate::shell::palette::toast_error;
use crate::theme::{self, AlphaMul, T};
use crate::{App, env, icons};
use rune::{El, TextEdit, col, el, row, text};
use serde_json::{Value, json};

pub struct Editor {
    pub(super) env: String,
    /// The task being edited, as it opened; `None` for a new one.
    task: Option<Value>,
    pub(super) draft: Draft,
    pub(super) saving: bool,
    /// The select whose list is open.
    open: Option<String>,
}

pub(super) fn open(s: &mut App, key: &str, task: Option<Value>) {
    let draft = match &task {
        Some(task) => Draft::from_task(task, Zone::Local),
        None => Draft::new(default_project(s, key)),
    };
    s.ui.settings.tasks.editor = Some(Editor { env: key.to_owned(), task, draft, saving: false, open: None });
}

/// Escape closes an open list first; the dialog itself closes in the caller.
pub(super) fn escape(s: &mut App) -> bool {
    s.ui.settings.tasks.editor.as_mut().is_some_and(|e| e.open.take().is_some())
}

fn editor_mut(s: &mut App) -> Option<&mut Editor> {
    s.ui.settings.tasks.editor.as_mut()
}

fn draft_text(s: &mut App, pick: fn(&mut Draft) -> &mut TextEdit) -> &mut TextEdit {
    let ui = &mut s.ui.settings.tasks;
    match ui.editor.as_mut() {
        Some(e) => pick(&mut e.draft),
        None => &mut ui.scratch,
    }
}

fn filter_text(s: &mut App, index: usize, path: bool) -> &mut TextEdit {
    let ui = &mut s.ui.settings.tasks;
    match ui.editor.as_mut().and_then(|e| e.draft.filters.get_mut(index)) {
        Some(f) if path => &mut f.path,
        Some(f) => &mut f.value,
        None => &mut ui.scratch,
    }
}

/// The scope's projects on `key`.
fn projects_on(s: &App, key: &str) -> Vec<Value> {
    let Some(src) = env::source(s, key) else { return Vec::new() };
    src.state().projects.iter().filter(|p| s.ui.settings.project.as_ref().is_none_or(|id| p["id"] == *id)).cloned().collect()
}

/// Web `defaultScheduledTaskProjectId`: a pinned open project, then any open one; an archived
/// one only when Settings is scoped to it.
fn default_project(s: &App, key: &str) -> String {
    let scoped = projects_on(s, key);
    let open: Vec<&Value> = scoped.iter().filter(|p| p["archivedAt"].is_null()).collect();
    open.iter()
        .find(|p| !p["pinnedAt"].is_null())
        .or(open.first())
        .copied()
        .or(scoped.first().filter(|_| s.ui.settings.project.is_some()))
        .and_then(|p| p["id"].as_str())
        .unwrap_or_default()
        .to_owned()
}

/// Pinned first; the chosen project stays listed after it is archived.
fn project_choices(s: &App, e: &Editor) -> Vec<Value> {
    let saved = e.task.as_ref().map(|t| t["projectId"].clone()).unwrap_or(Value::Null);
    let mut projects: Vec<Value> =
        projects_on(s, &e.env).into_iter().filter(|p| p["archivedAt"].is_null() || p["id"] == e.draft.project_id.as_str() || p["id"] == saved).collect();
    projects.sort_by_key(|p| p["pinnedAt"].is_null());
    projects
}

fn selected_project(s: &App, e: &Editor) -> Option<Value> {
    let projects = project_choices(s, e);
    if e.draft.project_id.is_empty() {
        return projects.into_iter().next();
    }
    projects.into_iter().find(|p| p["id"] == e.draft.project_id.as_str())
}

fn providers(config: &Value) -> impl Iterator<Item = &Value> {
    config["providers"].as_array().into_iter().flatten()
}

/// Web `scheduledTaskDefaultModel`: the project's then the environment's default when its
/// provider can run it, else the first default model.
fn default_model(config: &Value, project: Option<&str>) -> Option<Value> {
    let available: Vec<&Value> = providers(config)
        .filter(|p| p["enabled"] != false && p["installed"] != false && p["availability"] != "unavailable" && p["auth"]["status"] != "unauthenticated")
        .collect();
    let settings = &config["settings"];
    let configured = project.map(|p| &settings["projectSettingsOverrides"][p]["defaultModelSelection"]);
    for selection in configured.into_iter().chain([&settings["defaultModelSelection"]]) {
        let usable = available.iter().any(|entry| {
            entry["instanceId"] == selection["instanceId"]
                && !entry["models"].as_array().into_iter().flatten().any(|m| m["slug"] == selection["model"] && m["isLegacy"] == true)
        });
        if selection.is_object() && usable {
            return Some(selection.clone());
        }
    }
    let models: Vec<(&Value, &Value)> =
        available.iter().flat_map(|entry| entry["models"].as_array().into_iter().flatten().filter(|m| m["isLegacy"] != true).map(move |m| (*entry, m))).collect();
    let (entry, model) = models.iter().find(|(_, m)| m["isDefault"] == true).or(models.first())?;
    Some(json!({"instanceId": entry["instanceId"], "model": model["slug"]}))
}

fn split_model_key(key: &str) -> Option<(&str, &str)> {
    let (instance, model) = key.split_once(':')?;
    (!instance.is_empty() && !model.is_empty()).then_some((instance, model))
}

/// The model the task saves with: the picked one while its provider is enabled, else the default.
fn active_selection(s: &App, e: &Editor) -> Option<Value> {
    let config = config(s, &e.env)?;
    if let Some((instance, model)) = split_model_key(&e.draft.model_key) {
        if providers(config).any(|p| p["instanceId"] == instance && p["enabled"] != false) {
            return Some(json!({"instanceId": instance, "model": model}));
        }
    }
    let project = selected_project(s, e);
    default_model(config, project.as_ref().and_then(|p| p["id"].as_str()))
}

/// Every enabled provider's models, as `instanceId:model` and a label.
fn model_options(config: &Value) -> Vec<(String, String)> {
    providers(config)
        .filter(|p| p["enabled"] != false)
        .flat_map(|p| {
            let provider = p["displayName"].as_str().or(p["instanceId"].as_str()).unwrap_or_default().to_owned();
            let instance = p["instanceId"].as_str().unwrap_or_default().to_owned();
            p["models"].as_array().into_iter().flatten().filter_map(move |m| {
                let slug = m["slug"].as_str()?;
                Some((format!("{instance}:{slug}"), format!("{provider} · {}", m["name"].as_str().unwrap_or(slug))))
            })
        })
        .collect()
}

/// Contracts `managedCapabilityDenial(policy, "webhookTasks")`.
fn webhook_denial(policy: &Value) -> Option<String> {
    (policy["limits"]["webhookTasks"] == false)
        .then(|| format!("{}'s policy does not allow webhook tasks.", policy["organization"].as_str().unwrap_or("Your organization")))
}

const RUNTIME_MODES: [&str; 4] = ["approval-required", "auto-accept-edits", "auto", "full-access"];

/// Contracts `clampManagedRuntimeMode`: tasks have no mode picker, so a mode above the
/// administrator's ceiling would fail every run.
fn clamp_runtime(policy: &Value, mode: &str) -> String {
    let rank = |m: &str| RUNTIME_MODES.iter().position(|x| *x == m);
    let Some(ceiling) = policy["limits"]["maxRuntimeMode"].as_str().and_then(rank) else { return mode.to_owned() };
    if rank(mode).is_none_or(|r| r <= ceiling) {
        return mode.to_owned();
    }
    RUNTIME_MODES[ceiling].to_owned()
}

/// Client-runtime `webhookAddress`: the address, whether it can be copied, and a note.
fn webhook_address(endpoint: &Value, base: Option<&str>) -> (String, bool, Option<&'static str>) {
    if let Some(url) = endpoint["url"].as_str() {
        return (url.to_owned(), true, None);
    }
    let path = endpoint["path"].as_str().unwrap_or_default();
    let Some(base) = base.filter(|b| b.contains("://")) else {
        return (path.to_owned(), false, Some("Link this environment to T3 Connect for a public URL."));
    };
    let (scheme, rest) = base.split_once("://").unwrap_or_default();
    let authority = rest.split('/').next().unwrap_or_default();
    let address = format!("{scheme}://{authority}{path}");
    let note = if env::route_kind(&address) == "loopback" {
        "Only this computer can call this address. Link T3 Connect for a public URL."
    } else {
        "Works wherever this environment's address is reachable, for example over Tailscale or your own proxy. Link T3 Connect for a public URL."
    };
    (address, true, Some(note))
}

fn base_url(s: &App, key: &str) -> Option<String> {
    match env::source(s, key)? {
        crate::data::Source::Live(net) => Some(net.base_url().to_owned()),
        _ => None,
    }
}

fn offers_once(s: &App, e: &Editor) -> bool {
    config(s, &e.env).is_some_and(|c| c["environment"]["capabilities"]["oneTimeSchedules"] == true)
        || e.task.as_ref().is_some_and(|t| t["schedule"]["type"] == "once")
}

/// The task as its server lists it now; before the list loads, as it opened.
fn live_task(s: &App, e: &Editor) -> Option<Value> {
    let id = e.draft.editing_id.as_deref()?;
    match all_tasks(s, &e.env) {
        Some(tasks) => tasks.into_iter().find(|t| t["id"] == id),
        None => e.task.clone(),
    }
}

fn editing_missing(s: &App, e: &Editor) -> bool {
    e.draft.editing_id.as_deref().is_some_and(|id| all_tasks(s, &e.env).is_some_and(|tasks| !tasks.iter().any(|t| t["id"] == id)))
}

fn has_secret(live: Option<&Value>) -> bool {
    live.is_some_and(|t| t["schedule"]["type"] == "webhook" && t["webhook"]["hasSecret"] == true)
}

/// Web `submit`: the checks in its order, then `scheduledTasks.upsert`.
fn submit(s: &mut App) {
    let Some(e) = &s.ui.settings.tasks.editor else { return };
    let key = e.env.clone();
    if e.saving || !can_operate(s, &key) || !connected(s, &key) || all_tasks(s, &key).is_none() || editing_missing(s, e) {
        return;
    }
    let draft = &e.draft;
    let project = selected_project(s, e);
    let selection = active_selection(s, e);
    let Some((project, selection)) = project.zip(selection).filter(|_| !draft.title.text.trim().is_empty() && !draft.prompt.text.trim().is_empty()) else {
        return toast_error(s, "Scheduled task is incomplete", Some("Add a title, prompt, project, and model."));
    };
    if draft.mode == Mode::Webhook {
        if let Err(error) = draft::parse_filters(&draft.filters) {
            return toast_error(s, "Invalid filter", Some(&error));
        }
    }
    let Some(schedule) = draft::schedule(draft, Zone::Local) else {
        for (title, description) in draft::errors(draft, Zone::Local) {
            toast_error(s, &title, Some(&description));
        }
        return;
    };
    let signature = &schedule["signature"];
    if signature.is_object()
        && (signature["header"].as_str().is_none_or(str::is_empty) || (signature["secret"].is_null() && !has_secret(live_task(s, e).as_ref())))
    {
        return toast_error(s, "Signing secret is required", Some("Enter the signature header and secret."));
    }
    if draft.mode == Mode::Interval && draft.interval_ms().is_none_or(|ms| ms < draft::MIN_INTERVAL_MS) {
        return toast_error(s, "Invalid interval", Some("Enter an interval of at least one minute."));
    }
    if draft.mode == Mode::Once && !offers_once(s, e) {
        return toast_error(
            s,
            "Once is not available here",
            Some("This environment runs an older T3 Code. Update it to schedule a one-time task, or pick another schedule."),
        );
    }
    if draft.workspace == Workspace::Existing && draft.checkout.text.trim().is_empty() {
        return toast_error(s, "Checkout path is required", Some("Enter the path of the checkout to run in."));
    }
    let model_selection = match &draft.base_model {
        Some(base) if base["instanceId"] == selection["instanceId"] && base["model"] == selection["model"] => base.clone(),
        _ => selection,
    };
    let workspace = match draft.workspace {
        Workspace::Root => json!({"type": "root"}),
        Workspace::Existing => json!({"type": "existing_worktree", "worktreePath": draft.checkout.text.trim()}),
        Workspace::Worktree => {
            let base = draft.base_ref.text.trim();
            json!({"type": "worktree", "baseRef": if base.is_empty() { "main" } else { base }, "startFromOrigin": draft.start_from_origin})
        }
    };
    let policy = config(s, &key).map(|c| c["managedPolicy"].clone()).unwrap_or(Value::Null);
    let mut input = json!({
        "title": draft.title.text.trim(),
        "prompt": draft.prompt.text.trim(),
        "enabled": draft.enabled,
        "schedule": schedule,
        "projectId": project["id"],
        "threadId": draft.thread_id,
        "workspaceStrategy": workspace,
        "modelSelection": model_selection,
        "runtimeMode": clamp_runtime(&policy, &draft.runtime_mode),
        "interactionMode": draft.interaction_mode,
        "creationSource": "web",
    });
    if let Some(id) = &draft.editing_id {
        input["id"] = json!(id);
        input["requireExisting"] = json!(true);
    }
    if send(s, &key, "scheduledTasks.upsert", input, Kind::Upsert) {
        if let Some(e) = editor_mut(s) {
            e.saving = true;
        }
    }
}

fn label(t: &T, value: &str) -> El {
    text(value.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground)
}

fn muted(t: &T, value: &str) -> El {
    text(value.to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap()
}

fn hint(t: &T, value: &str) -> El {
    text(value.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()
}

/// Web `Field`: a label, an optional hint at its end, then the control.
fn field(t: &T, name: &str, note: Option<&str>, control: El) -> El {
    let mut head = row().w_full().gap(8.0).child(label(t, name).grow(1.0));
    if let Some(note) = note {
        head = head.child(text(note.to_owned()).font(theme::TEXT_2XS).color(t.c.muted_foreground.alpha_mul(0.8)));
    }
    col().w_full().gap(6.0).child(head).child(control)
}

fn input(t: &T, edit: &TextEdit, placeholder: &str, disabled: bool, pick: fn(&mut Draft) -> &mut TextEdit) -> El {
    kit::Input::new(edit).placeholder(placeholder).disabled(disabled).on_edit(move |s: &mut App| draft_text(s, pick)).build(t).single_line()
}

/// A labelled switch row with an explanation under the label.
fn switch_row(t: &T, name: &str, description: &str, on: bool, flip: fn(&mut Draft)) -> El {
    row()
        .w_full()
        .gap(16.0)
        .center_y()
        .child(col().grow(1.0).min_w(0.0).gap(4.0).child(label(t, name)).child(muted(t, description)))
        .child(
            kit::Switch::new(on)
                .label(name)
                .hide_label()
                .on_toggle(move |s: &mut App| {
                    if let Some(e) = editor_mut(s) {
                        flip(&mut e.draft);
                    }
                })
                .build(&kit::theme(t)),
        )
}

struct Choice<'a> {
    id: &'a str,
    options: Vec<(String, String)>,
    current: Option<String>,
    placeholder: &'a str,
    width: f32,
    disabled: bool,
}

fn select(e: &Editor, t: &T, popups: &mut Vec<El>, choice: Choice, choose: impl Fn(&mut App, &str) + Clone + 'static) -> El {
    let values: Vec<&str> = choice.options.iter().map(|(v, _)| v.as_str()).collect();
    let id = choice.id.to_owned();
    let open = e.open.as_deref() == Some(choice.id);
    let mut select = kit::Select::new(format!("task-{id}"), &values)
        .label(choice.placeholder)
        .placeholder(choice.placeholder)
        .size("sm")
        .width(choice.width)
        .current(choice.current.as_deref())
        .open(open && !choice.disabled)
        .disabled(choice.disabled);
    for (value, name) in &choice.options {
        select = select.option_name(value.clone(), name.clone());
    }
    select
        .on_toggle(move |s: &mut App| {
            if let Some(e) = editor_mut(s) {
                e.open = if open { None } else { Some(id.clone()) };
            }
        })
        .on_close(|s: &mut App| {
            if let Some(e) = editor_mut(s) {
                e.open = None;
            }
        })
        .on_choose(move |s: &mut App, value: &str, _: &mut rune::Tx| {
            if let Some(e) = editor_mut(s) {
                e.open = None;
            }
            choose(s, value);
        })
        .build(&kit::theme(t), popups)
}

const FULL: f32 = 528.0;
const HALF: f32 = 258.0;

pub(super) fn dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(e) = &s.ui.settings.tasks.editor else { return };
    let mut popups = Vec::new();
    let draft = &e.draft;
    let saving = e.saving;
    let is_connected = connected(s, &e.env);
    let config = config(s, &e.env).cloned().unwrap_or(Value::Null);
    let policy = &config["managedPolicy"];
    let denial = webhook_denial(policy);
    let missing = editing_missing(s, e);
    let live = live_task(s, e);
    let mut form = col().w_full().gap(20.0);
    if !is_connected {
        form = form.child(text("Reconnect this environment before saving.").font(theme::TEXT_SM).color(t.c.destructive).wrap());
    }
    // Runs on.
    let envs: Vec<String> = environments(s).into_iter().filter(|k| connected(s, k)).collect();
    let env_options: Vec<(String, String)> = envs.iter().map(|k| (k.clone(), env::label(s, k))).collect();
    let mut runs_on = kit::Select::new("task-env", &envs)
        .label("Runs on")
        .size("sm")
        .width(FULL)
        .current(Some(e.env.as_str()))
        .open(e.open.as_deref() == Some("env") && e.task.is_none() && !saving)
        .disabled(e.task.is_some() || saving)
        .leading(rune::icon(machine(s, &e.env), 16.0).color(t.c.foreground).opacity(0.8), 16.0);
    if !envs.contains(&e.env) {
        runs_on = runs_on.display(if is_connected { env::label(s, &e.env) } else { "Unavailable environment".into() });
    }
    for (key, name) in &env_options {
        runs_on = runs_on.option_name(key.clone(), name.clone());
        // Web adds the address only when two environments share a name.
        let (_, secondary) = labels(s, key, &envs);
        if !secondary.is_empty() {
            runs_on = runs_on.option_detail(key.clone(), machine(s, key), secondary);
        }
    }
    let runs_on = runs_on
        .on_toggle(|s: &mut App| {
            if let Some(e) = editor_mut(s) {
                e.open = if e.open.as_deref() == Some("env") { None } else { Some("env".into()) };
            }
        })
        .on_close(|s: &mut App| {
            if let Some(e) = editor_mut(s) {
                e.open = None;
            }
        })
        .on_choose(|s: &mut App, key: &str, _: &mut rune::Tx| {
            let project = default_project(s, key);
            if let Some(e) = editor_mut(s) {
                e.open = None;
                e.env = key.to_owned();
                e.draft.project_id = project;
                e.draft.model_key.clear();
                e.draft.base_model = None;
                e.draft.base_ref = TextEdit::new("main");
                e.draft.start_from_origin = true;
                e.draft.checkout = TextEdit::new("");
            }
        })
        .build(&kit::theme(t), &mut popups);
    form = form.child(field(t, "Runs on", None, runs_on));
    if let super::List::Failed(error) = super::list(s, &e.env) {
        form = form.child(text(error).font(theme::TEXT_SM).color(t.c.destructive).wrap().role("status"));
    }
    if missing {
        form = form.child(text("This scheduled task no longer exists.").font(theme::TEXT_XS).color(t.c.destructive).role("status"));
    }
    form = form.child(field(t, "Name", None, input(t, &draft.title, "e.g. Check for Sentry issues", saving, |d| &mut d.title)));
    // Destination.
    let destination = match &draft.thread_id {
        None => muted(t, "Each run starts a new thread with the workspace and model below."),
        Some(id) => {
            let title = env::source(s, &e.env)
                .and_then(|src| src.state().threads.iter().find(|th| th["id"] == id.as_str()).and_then(|th| th["title"].as_str()).map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned))
                .unwrap_or_else(|| format!("Thread {id}"));
            col().w_full().gap(4.0).child(text(title).font(theme::TEXT_SM).color(t.c.foreground).wrap()).child(muted(
                t,
                "Each run continues this conversation, in its workspace and with its current model.",
            ))
        }
    };
    form = form.child(field(t, "Destination", None, destination));
    // Project and workspace.
    let projects = project_choices(s, e);
    let every: Vec<&Value> = env::keys(s).iter().filter_map(|k| env::source(s, k)).flat_map(|src| src.state().projects.iter()).collect();
    let selected = selected_project(s, e);
    let mut project_select = kit::Select::new("task-project", &projects.iter().map(|p| crate::data::s(p, "id").to_owned()).collect::<Vec<_>>())
        .label("Project")
        .placeholder("Select a project")
        .size("sm")
        .width(HALF)
        .current(selected.as_ref().map(|p| crate::data::s(p, "id")))
        .open(e.open.as_deref() == Some("project"));
    for p in &projects {
        let (id, title) = (crate::data::s(p, "id"), crate::data::s(p, "title"));
        project_select = project_select.option_name(id, title);
        // Web shows the folder only when another project, anywhere, has the same name.
        if every.iter().filter(|o| crate::data::s(o, "title") == title).count() > 1 {
            project_select = project_select.option_detail(id, &icons::FOLDER, crate::data::s(p, "workspaceRoot"));
        }
    }
    let project_select = project_select
        .on_toggle(|s: &mut App| {
            if let Some(e) = editor_mut(s) {
                e.open = if e.open.as_deref() == Some("project") { None } else { Some("project".into()) };
            }
        })
        .on_close(|s: &mut App| {
            if let Some(e) = editor_mut(s) {
                e.open = None;
            }
        })
        .on_choose(|s: &mut App, id: &str, _: &mut rune::Tx| {
            if let Some(e) = editor_mut(s) {
                e.open = None;
                e.draft.project_id = id.to_owned();
            }
        })
        .build(&kit::theme(t), &mut popups);
    let workspace = select(
        e,
        t,
        &mut popups,
        Choice {
            id: "workspace",
            options: Workspace::ALL.iter().map(|w| (w.value().to_owned(), w.label().to_owned())).collect(),
            current: Some(draft.workspace.value().to_owned()),
            placeholder: "Workspace",
            width: HALF,
            disabled: saving,
        },
        |s, value| {
            if let Some(e) = editor_mut(s) {
                e.draft.workspace = Workspace::ALL.into_iter().find(|w| w.value() == value).unwrap_or(Workspace::Worktree);
            }
        },
    );
    form = form.child(
        row()
            .w_full()
            .gap(12.0)
            .items_start()
            .child(field(t, "Project", None, project_select).w(HALF))
            .child(field(t, "Workspace", None, workspace).w(HALF)),
    );
    match draft.workspace {
        Workspace::Worktree => {
            let origin = row().w_full().gap(12.0).center_y().child(text("Start from remote").font(theme::XS_MEDIUM).color(t.c.muted_foreground).grow(1.0)).child(
                kit::Switch::new(draft.start_from_origin)
                    .label("Start worktree from remote")
                    .hide_label()
                    .size("sm")
                    .disabled(saving || !is_connected)
                    .on_toggle(|s: &mut App| {
                        if let Some(e) = editor_mut(s) {
                            e.draft.start_from_origin ^= true;
                        }
                    })
                    .build(&kit::theme(t)),
            );
            let control = col().w_full().gap(8.0).child(input(t, &draft.base_ref, "main", saving || !is_connected, |d| &mut d.base_ref)).child(origin);
            form = form.child(field(t, "Base branch", None, control));
        }
        Workspace::Existing => {
            form = form.child(field(t, "Checkout path", None, input(t, &draft.checkout, "/path/to/checkout", saving, |d| &mut d.checkout)));
        }
        Workspace::Root => {}
    }
    // Prompt.
    let prompt = rune::text_input(&draft.prompt, theme::TEXT_SM)
        .key("task-prompt")
        .label("Prompt")
        .placeholder("What should the agent do each time this runs?", t.c.muted_foreground.alpha_mul(0.72))
        .w_full()
        .min_h(70.0)
        .max_h(256.0)
        .px(11.0)
        .py(5.0)
        .radius(8.0)
        .border(1.0, t.c.input)
        .fill(t.c.input.alpha_mul(if t.dark { 0.32 } else { 0.0 }))
        .color(t.c.foreground)
        .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
        .focus_ring(3.0, -3.0, t.c.ring.alpha_mul(0.24))
        .on_edit(|s: &mut App| draft_text(s, |d| &mut d.prompt));
    form = form.child(field(t, "Prompt", None, prompt));
    // Model.
    let selection = active_selection(s, e);
    let model = select(
        e,
        t,
        &mut popups,
        Choice {
            id: "model",
            options: model_options(&config),
            current: selection.map(|m| format!("{}:{}", m["instanceId"].as_str().unwrap_or_default(), m["model"].as_str().unwrap_or_default())),
            placeholder: "Model",
            width: FULL,
            disabled: saving || !is_connected,
        },
        |s, value| {
            if let Some(e) = editor_mut(s) {
                e.draft.model_key = value.to_owned();
            }
        },
    );
    form = form.child(field(t, "Model", None, model));
    form = form.child(schedule_section(s, t, e, denial.as_deref(), live.as_ref(), &mut popups));
    form = form.child(switch_row(t, "Enabled", "Disabled tasks stay saved but do not run.", draft.enabled, |d| d.enabled ^= true));
    let can = can_operate(s, &e.env);
    let blocked = draft.enabled && draft.mode == Mode::Webhook && denial.is_some();
    let ready = can && !saving && !missing && is_connected && all_tasks(s, &e.env).is_some() && !blocked;
    let close = |s: &mut App| {
        if s.ui.settings.tasks.editor.as_ref().is_some_and(|e| !e.saving) {
            s.ui.settings.tasks.editor = None;
        }
    };
    let footer = row()
        .w_full()
        .gap(8.0)
        .child(el().grow(1.0))
        .child(kit::Button::new("Cancel").variant("outline").size("sm").disabled(saving).on_press(close).build(t))
        .child(kit::Button::new(if draft.editing_id.is_some() { "Save task" } else { "Create task" }).size("sm").disabled(!ready).on_press(submit).build(t));
    kit::Dialog::new("task-editor", if draft.editing_id.is_some() { "Edit task" } else { "New task" })
        .open(true)
        .width(576.0)
        .scroll_all()
        .description("Run a prompt automatically on an interval, at a fixed time, once at a set date, or when a webhook is called.")
        .on_close(close)
        .child(form)
        .child(footer)
        .build(t, layers);
    layers.extend(popups);
}

const WEEKDAY_ORDER: [u8; 7] = [1, 2, 3, 4, 5, 6, 0];
const WEEKDAY_SHORT: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

/// Web's schedule block: the type toggle, then the type's own fields.
fn schedule_section(s: &App, t: &T, e: &Editor, denial: Option<&str>, live: Option<&Value>, popups: &mut Vec<El>) -> El {
    let draft = &e.draft;
    let mut block = col().w_full().gap(12.0);
    if let Some(task) = &e.task {
        if task["schedule"]["type"] == "interval" && task["schedule"]["everyMs"].as_i64().is_some_and(|ms| ms < draft::MIN_INTERVAL_MS) {
            block = block.child(muted(t, "This task uses a legacy interval below one minute. Saving updates it to at least one minute.").role("status"));
        }
    }
    let mut modes = vec![(Mode::Fixed, "At a time")];
    if offers_once(s, e) || draft.mode == Mode::Once {
        modes.push((Mode::Once, "Once"));
    }
    modes.push((Mode::Interval, "Every interval"));
    modes.push((Mode::Webhook, "On webhook"));
    let names: Vec<&str> = modes.iter().map(|(_, name)| *name).collect();
    let picks: Vec<Mode> = modes.iter().map(|(mode, _)| *mode).collect();
    let selected: Vec<usize> = picks.iter().position(|m| *m == draft.mode).into_iter().collect();
    let mut group = kit::ToggleGroup::new(&names, &selected).size("sm").on_pick(move |s: &mut App, index, _| {
        let now = s.now_ms;
        if let (Some(mode), Some(e)) = (picks.get(index).copied(), editor_mut(s)) {
            e.draft.set_mode(mode, now, Zone::Local);
        }
    });
    if denial.is_some() && draft.mode != Mode::Webhook {
        group = group.item_state(modes.len() - 1, rune_kit::State::Disabled);
    }
    block = block.child(row().w_full().gap(8.0).center_y().flex_wrap().child(label(t, "Schedule").grow(1.0)).child(group.build(&kit::theme(t))));
    if let Some(denial) = denial {
        let notice = if draft.mode == Mode::Webhook {
            format!("{denial} Switch this task to a time or an interval, or turn it off, to save it.")
        } else {
            denial.to_owned()
        };
        block = block.child(
            row()
                .w_full()
                .gap(8.0)
                .padding(10.0)
                .radius(8.0)
                .border(1.0, t.c.border)
                .fill(t.c.muted.alpha_mul(0.4))
                .child(rune::icon(&icons::SHIELD, 14.0).color(t.c.muted_foreground))
                .child(hint(t, &notice).grow(1.0)),
        );
    }
    // A disabled task still saves, so its webhook fields stay reachable for fixing.
    let fields = match draft.mode {
        Mode::Webhook if denial.is_some() && draft.enabled => None,
        Mode::Webhook => Some(webhook_fields(s, t, e, live, popups)),
        Mode::Once => Some(
            row().gap(8.0).center_y().child(label(t, "Run on")).child(
                input(t, &draft.run_at, "YYYY-MM-DDTHH:MM", e.saving, |d| &mut d.run_at).w(224.0),
            ),
        ),
        Mode::Fixed => {
            let chosen: Vec<usize> = WEEKDAY_ORDER.iter().enumerate().filter(|(_, d)| draft.weekdays.contains(d)).map(|(i, _)| i).collect();
            let days: Vec<&str> = WEEKDAY_ORDER.iter().map(|d| WEEKDAY_SHORT[*d as usize]).collect();
            let weekdays = kit::ToggleGroup::new(&days, &chosen).variant("outline").size("sm").on_pick(|s: &mut App, index, _| {
                let Some(e) = editor_mut(s) else { return };
                let day = WEEKDAY_ORDER[index];
                let days = &mut e.draft.weekdays;
                match days.iter().position(|d| *d == day) {
                    // Web's group never empties.
                    Some(at) if days.len() > 1 => {
                        days.remove(at);
                    }
                    Some(_) => {}
                    None => days.push(day),
                }
            });
            Some(
                row()
                    .w_full()
                    .gap(12.0)
                    .center_y()
                    .flex_wrap()
                    .child(
                        row()
                            .gap(8.0)
                            .center_y()
                            .child(label(t, "Run at"))
                            .child(input(t, &draft.time_of_day, "09:00", e.saving, |d| &mut d.time_of_day).w(128.0))
                            .child(text("on").font(theme::TEXT_XS).color(t.c.muted_foreground)),
                    )
                    .child(weekdays.build(&kit::theme(t))),
            )
        }
        Mode::Interval => Some(
            row()
                .gap(8.0)
                .center_y()
                .child(label(t, "Run every"))
                .child(input(t, &draft.interval_minutes, "15", e.saving, |d| &mut d.interval_minutes).w(96.0))
                .child(text("minutes").font(theme::TEXT_XS).color(t.c.muted_foreground)),
        ),
    };
    if let Some(fields) = fields {
        block = block.child(fields);
    }
    block
}

/// Web `WebhookEndpointField`.
fn endpoint_field(s: &App, t: &T, e: &Editor, live: Option<&Value>) -> El {
    let Some(task) = live.filter(|task| task["webhook"].is_object()) else { return muted(t, "The URL appears after you save.") };
    let (address, copyable, note) = webhook_address(&task["webhook"], base_url(s, &e.env).as_deref());
    let id = task["id"].as_str().unwrap_or_default().to_owned();
    let env_key = e.env.clone();
    let copy = address.clone();
    let rotating = s.ui.settings.tasks.rotate.is_some();
    let copy_button = kit::Button::new("Copy").variant("outline").size("sm").icon(&icons::COPY).disabled(!copyable).build(t);
    // A path without an origin is not callable as-is, so it does not copy.
    let copy_button = if copyable {
        copy_button.on_press_tx(move |s: &mut App, tx| {
            tx.copy(copy.clone());
            crate::shell::palette::toast(s, "Webhook URL copied", None);
        })
    } else {
        copy_button
    };
    let line = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            row()
                .grow(1.0)
                .min_w(0.0)
                .h(30.0)
                .px(11.0)
                .center_y()
                .radius(8.0)
                .border(1.0, t.c.input)
                .role("textbox")
                .label("Webhook URL")
                .child(text(address).font(theme::TEXT_SM).color(t.c.foreground).truncate()),
        )
        .child(copy_button)
        .child(
            kit::Button::new("Rotate")
                .variant("outline")
                .size("sm")
                .disabled(rotating || !can_operate(s, &e.env))
                .on_press(move |s: &mut App| s.ui.settings.tasks.rotate = Some((env_key.clone(), id.clone())))
                .build(t),
        );
    let mut out = col().w_full().gap(6.0).child(line);
    if let Some(note) = note {
        out = out.child(hint(t, note));
    }
    out
}

/// Web's webhook fields, in its order.
fn webhook_fields(s: &App, t: &T, e: &Editor, live: Option<&Value>, popups: &mut Vec<El>) -> El {
    let draft = &e.draft;
    let saving = e.saving;
    let mut out = col()
        .w_full()
        .gap(16.0)
        .child(endpoint_field(s, t, e, live))
        .child(hint(
            t,
            "Each request runs the prompt. Use {{body.path}}, {{headers.name}}, {{query.name}}, {{body}} or {{request}} in the prompt; only what it names reaches the agent.",
        ))
        .child(field(t, "Skip requests older than", Some("minutes, optional"), input(t, &draft.max_age, "Run every request", saving, |d| &mut d.max_age)))
        .child(filters_field(t, e, popups))
        .child(field(t, "Duplicate request key", Some("template, optional"), input(t, &draft.replay_key, "{{headers.delivery-id}}", saving, |d| &mut d.replay_key)))
        .child(hint(t, "Requests with the same key run only once within 7 days."))
        .child(field(t, "Coalesce key", Some("template, optional"), input(t, &draft.coalesce_key, "{{body.id}}", saving, |d| &mut d.coalesce_key)))
        .child(field(t, "Coalesce window", Some("seconds, 10–3600"), input(t, &draft.coalesce_window, "120", saving, |d| &mut d.coalesce_window)))
        .child(hint(t, "Coalescing runs the latest payload once when the window from the first delivery closes. Empty keys run normally."))
        .child(switch_row(
            t,
            "Require signature",
            "Reject requests without a valid HMAC-SHA256 signature of the body.",
            draft.signature_enabled,
            |d| d.signature_enabled ^= true,
        ));
    if draft.signature_enabled {
        let encoding = select(
            e,
            t,
            popups,
            Choice {
                id: "encoding",
                options: vec![("hex".into(), "hex".into()), ("base64".into(), "base64".into())],
                current: Some(draft.signature_encoding.to_owned()),
                placeholder: "Encoding",
                width: HALF,
                disabled: saving,
            },
            |s, value| {
                if let Some(e) = editor_mut(s) {
                    e.draft.signature_encoding = if value == "base64" { "base64" } else { "hex" };
                }
            },
        );
        let secret_placeholder = if has_secret(live) { "Unchanged" } else { "Shared secret" };
        let pair = |a: El, b: El| row().w_full().gap(12.0).items_start().child(a.w(HALF)).child(b.w(HALF));
        out = out
            .child(pair(
                field(t, "Header", None, input(t, &draft.signature_header, "", saving, |d| &mut d.signature_header).label("Signature header")),
                field(t, "Prefix", None, input(t, &draft.signature_prefix, "None", saving, |d| &mut d.signature_prefix)),
            ))
            .child(pair(
                field(t, "Encoding", None, encoding),
                // The secret is write-only; it shows as dots while typed.
                field(
                    t,
                    "Secret",
                    None,
                    kit::Input::new(&draft.signature_secret)
                        .placeholder(secret_placeholder)
                        .disabled(saving)
                        .on_edit(|s: &mut App| draft_text(s, |d| &mut d.signature_secret))
                        .build(t)
                        .single_line(),
                ),
            ));
    }
    out
}

/// Web `WebhookFiltersField`.
fn filters_field(t: &T, e: &Editor, popups: &mut Vec<El>) -> El {
    let filters = &e.draft.filters;
    let mut out = col().w_full().gap(8.0).child(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(col().grow(1.0).min_w(0.0).gap(4.0).child(label(t, "Filters")).child(muted(
                t,
                "Run only when every filter matches. Other requests are logged as filtered. Separate “is one of” values with commas; a value can't contain a comma.",
            )))
            .child(
                kit::Button::new("Add filter")
                    .variant("outline")
                    .size("sm")
                    .icon(&icons::PLUS)
                    .disabled(filters.len() >= draft::MAX_FILTERS)
                    .on_press(|s: &mut App| {
                        if let Some(e) = editor_mut(s) {
                            e.draft.filters.push(Filter::empty());
                        }
                    })
                    .build(t),
            ),
    );
    for (index, filter) in filters.iter().enumerate() {
        let n = index + 1;
        let path = kit::Input::new(&filter.path)
            .placeholder("body.action")
            .on_edit(move |s: &mut App| filter_text(s, index, true))
            .build(t)
            .single_line()
            .label(format!("Filter {n} path"));
        let remove = kit::icon_button(&icons::TRASH_2, &format!("Remove filter {n}"))
            .size("icon-sm")
            .on_press(move |s: &mut App| {
                if let Some(e) = editor_mut(s) {
                    if index < e.draft.filters.len() {
                        e.draft.filters.remove(index);
                    }
                    e.open = None;
                }
            })
            .build(t);
        let op = select(
            e,
            t,
            popups,
            Choice {
                id: &format!("op-{index}"),
                options: draft::FILTER_OPS.iter().map(|(op, name)| ((*op).to_owned(), (*name).to_owned())).collect(),
                current: Some(filter.op.to_owned()),
                placeholder: &format!("Filter {n} test"),
                width: 176.0,
                disabled: false,
            },
            move |s, value| {
                if let (Some(op), Some(e)) = (draft::FILTER_OPS.iter().map(|(op, _)| *op).find(|op| *op == value), editor_mut(s)) {
                    if let Some(filter) = e.draft.filters.get_mut(index) {
                        filter.op = op;
                    }
                }
            },
        );
        let mut second = row().w_full().gap(8.0).center_y().child(op);
        if filter.op != "exists" {
            second = second.child(
                kit::Input::new(&filter.value)
                    .placeholder(if filter.op == "in" { "opened, reopened" } else { "opened" })
                    .on_edit(move |s: &mut App| filter_text(s, index, false))
                    .build(t)
                    .single_line()
                    .label(format!("Filter {n} value"))
                    .grow(1.0)
                    .min_w(0.0),
            );
        }
        out = out.child(col().w_full().gap(8.0).pt(4.0).child(row().w_full().gap(8.0).center_y().child(path.grow(1.0).min_w(0.0)).child(remove)).child(second));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_model_follows_web_scheduled_task_default_model() {
        let config = json!({
            "providers": [
                {"instanceId": "off", "enabled": false, "models": [{"slug": "x", "isDefault": true}]},
                {"instanceId": "codex", "models": [{"slug": "old", "isLegacy": true}, {"slug": "gpt"}, {"slug": "gpt-mini", "isDefault": true}]},
            ],
            "settings": {
                "defaultModelSelection": {"instanceId": "codex", "model": "gpt"},
                "projectSettingsOverrides": {"p1": {"defaultModelSelection": {"instanceId": "codex", "model": "old"}}},
            },
        });
        // The project's default is a legacy model, so the environment's wins.
        assert_eq!(default_model(&config, Some("p1")), Some(json!({"instanceId": "codex", "model": "gpt"})));
        let mut bare = config.clone();
        bare["settings"] = json!({});
        assert_eq!(default_model(&bare, None), Some(json!({"instanceId": "codex", "model": "gpt-mini"})), "the provider's default model");
        assert_eq!(split_model_key("codex:gpt:5"), Some(("codex", "gpt:5")));
        assert_eq!(split_model_key(":gpt"), None);
    }

    #[test]
    fn webhook_addresses_and_policy_limits_read_like_web() {
        let hosted = json!({"path": "/hooks/abc", "url": "https://relay.t3.codes/hooks/abc"});
        assert_eq!(webhook_address(&hosted, None), ("https://relay.t3.codes/hooks/abc".into(), true, None));
        let local = json!({"path": "/hooks/abc", "url": null});
        assert_eq!(webhook_address(&local, None).1, false);
        let (address, copyable, note) = webhook_address(&local, Some("http://127.0.0.1:3773/"));
        assert_eq!((address.as_str(), copyable), ("http://127.0.0.1:3773/hooks/abc", true));
        assert!(note.unwrap().starts_with("Only this computer"));
        assert!(webhook_address(&local, Some("https://box.tail1234.ts.net")).2.unwrap().starts_with("Works wherever"));
        let policy = json!({"organization": "Acme", "limits": {"webhookTasks": false, "maxRuntimeMode": "auto-accept-edits"}});
        assert_eq!(webhook_denial(&policy).as_deref(), Some("Acme's policy does not allow webhook tasks."));
        assert_eq!(webhook_denial(&json!({})), None);
        assert_eq!(clamp_runtime(&policy, "full-access"), "auto-accept-edits");
        assert_eq!(clamp_runtime(&policy, "approval-required"), "approval-required");
        assert_eq!(clamp_runtime(&Value::Null, "full-access"), "full-access");
    }
}
