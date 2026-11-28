//! Project scripts in the details card (ProjectScriptsControl): the run split button and its
//! menu, the add/edit dialog with shortcut capture, delete, and running in the terminal drawer.
use std::collections::HashMap;

use rune::{Align, Cursor, El, Icon, Side, Key, KeyEvent, TextEdit, col, el, icon, row, text};
use serde_json::{Value, json};

use crate::chat::composer;
use crate::shell::palette;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, data, icons, kit};

const MAX_ID: usize = 24;
static ICONS: [(&str, &str); 6] = [("play", "Play"), ("test", "Test"), ("lint", "Lint"), ("configure", "Configure"), ("build", "Build"), ("debug", "Debug")];

#[derive(Default)]
pub struct State {
    editor: Option<Editor>,
    /// The delete confirm, over the editor.
    confirm_delete: bool,
    /// The last script run per project (`lastInvokedScriptByProjectId`).
    last: HashMap<String, String>,
    pending: Option<Pending>,
    /// `t3.json` scripts by checkout (`useT3ProjectFileState`): empty when missing, None when invalid.
    files: HashMap<(String, String), Option<Vec<Value>>>,
    reading: Option<(u64, (String, String))>,
    /// The project Settings opened the editor for, in place of the open thread's.
    target: Option<String>,
    /// Settings > Projects > Actions' "Import scripts" menu, open for this project.
    import_menu: Option<String>,
}

#[derive(Default)]
struct Editor {
    id: Option<String>,
    name: TextEdit,
    command: TextEdit,
    preview: TextEdit,
    icon: String,
    icons_open: bool,
    setup: bool,
    wait: bool,
    settle: bool,
    auto_preview: bool,
    keybinding: String,
    initial_keybinding: Option<String>,
    capturing: bool,
    error: Option<String>,
}

struct Pending {
    waiting: u64,
    /// Keybinding writes that follow the settings write, one at a time.
    queue: Vec<(String, &'static str, Value)>,
    scoped: bool,
    /// A delete's script name, for its toast.
    deleted: Option<String>,
    /// A Settings import's script, reopened in the editor when the save fails.
    imported: Option<Value>,
}

pub fn icon_for(name: &str) -> &'static Icon {
    match name {
        "test" => &icons::FLASK_CONICAL,
        "lint" => &icons::LIST_CHECKS,
        "configure" => &icons::WRENCH,
        "build" => &icons::HAMMER,
        "debug" => &icons::BUG,
        _ => &icons::PLAY,
    }
}

/// `resolveProjectScripts`.
pub fn resolve(settings: &Value, project: &Value) -> Vec<Value> {
    let id = data::s(project, "id");
    let list = |v: &Value| v.as_array().cloned().unwrap_or_default();
    let defaults = list(&settings["defaultProjectScripts"]);
    let overrides = &settings["projectSettingsOverrides"][id]["defaultProjectScripts"];
    if overrides.is_array() {
        return list(overrides);
    }
    if settings["projectSettingsFolded"] == true {
        return defaults;
    }
    match settings["projectScriptOverrides"].get(id) {
        Some(Value::Null) => defaults,
        Some(legacy) => list(legacy),
        None => Some(list(&project["scripts"])).filter(|v| !v.is_empty()).unwrap_or(defaults),
    }
}

/// `primaryProjectScript`: the first plain script, else the first that isn't the settle script.
pub fn primary(scripts: &[Value]) -> Option<&Value> {
    scripts.iter().find(|v| v["runOnWorktreeCreate"] != true && v["runOnSettle"] != true).or_else(|| scripts.iter().find(|v| v["runOnSettle"] != true))
}

/// `projectScriptMenuLabel`.
pub fn menu_label(script: &Value) -> String {
    let roles: Vec<&str> = [(script["runOnWorktreeCreate"] == true, "setup"), (script["runOnSettle"] == true, "on settle")].into_iter().filter(|r| r.0).map(|r| r.1).collect();
    let name = data::s(script, "name");
    if roles.is_empty() { name.to_string() } else { format!("{name} ({})", roles.join(", ")) }
}

/// `nextProjectScriptId`.
pub fn next_id(name: &str, existing: &[String]) -> String {
    let mut cleaned = String::new();
    for c in name.trim().to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            cleaned.push(c);
        } else if !cleaned.ends_with('-') {
            cleaned.push('-');
        }
    }
    let mut base = cleaned.trim_matches('-').to_string();
    if base.len() > MAX_ID {
        base = base[..MAX_ID].trim_end_matches('-').to_string();
    }
    if base.is_empty() {
        base = "script".into();
    }
    if !existing.contains(&base) {
        return base;
    }
    (2..10_000)
        .map(|n| {
            let candidate = format!("{base}-{n}");
            if candidate.len() <= MAX_ID { candidate } else { format!("{}-{n}", &base[..(MAX_ID - n.to_string().len() - 1).max(1)]) }
        })
        .find(|c| !existing.contains(c))
        .unwrap_or(base)
}

pub fn command_for(id: &str) -> String {
    format!("script.{id}.run")
}

/// `keybindingValueForCommand`: a resolved shortcut back in `mod+shift+k` form.
fn shortcut_value(binding: &Value) -> String {
    let k = &binding["shortcut"];
    let mut parts: Vec<String> = [("modKey", "mod"), ("ctrlKey", "ctrl"), ("metaKey", "meta"), ("altKey", "alt"), ("shiftKey", "shift")]
        .into_iter()
        .filter(|(field, _)| k[*field] == true)
        .map(|(_, name)| name.to_string())
        .collect();
    parts.push(match data::s(k, "key") {
        " " => "space".into(),
        "escape" => "esc".into(),
        key => key.to_string(),
    });
    parts.join("+")
}

fn bindings(s: &App) -> Vec<Value> {
    s.state().config.as_ref().and_then(|c| c["keybindings"].as_array().cloned()).unwrap_or_default()
}

fn keybinding_for(bindings: &[Value], command: &str) -> Option<String> {
    bindings.iter().rev().find(|b| b["command"] == command).map(shortcut_value)
}

/// The keybinding writes a save makes (`persistProjectScripts`): drop stale rules, then upsert
/// the new key replacing the last one; a cleared key keeps rules another project still uses.
fn keybinding_ops(bindings: &[Value], command: &str, next: Option<&str>, retained: bool) -> Vec<(&'static str, Value)> {
    let previous: Vec<Value> = bindings
        .iter()
        .filter(|b| b["command"] == command && b["whenAst"].is_null())
        .map(|b| json!({"key": shortcut_value(b), "command": command}))
        .collect();
    let Some(key) = next else {
        return if retained { vec![] } else { previous.into_iter().map(|rule| ("server.removeKeybinding", rule)).collect() };
    };
    let mut ops: Vec<_> = previous.iter().take(previous.len().saturating_sub(1)).map(|rule| ("server.removeKeybinding", rule.clone())).collect();
    let mut rule = json!({"key": key, "command": command});
    if let Some(last) = previous.last().filter(|last| last["key"] != key) {
        rule["replace"] = last.clone();
    }
    ops.push(("server.upsertKeybinding", rule));
    ops
}

/// The script as saved (`buildProjectScript`).
fn build(id: &str, e: &Editor) -> Value {
    let mut script = json!({"id": id, "name": e.name.text.trim(), "command": e.command.text.trim(), "icon": e.icon, "runOnWorktreeCreate": e.setup});
    if e.setup && e.wait {
        script["async"] = json!(false);
    }
    if e.settle {
        script["runOnSettle"] = json!(true);
    }
    let preview = e.preview.text.trim();
    if !preview.is_empty() {
        script["previewUrl"] = json!(preview);
        script["autoOpenPreview"] = json!(e.auto_preview);
    }
    script
}

/// `releaseClaimedRoles`: one script owns setup and one owns settle.
fn release(mut script: Value, setup: bool, settle: bool) -> Value {
    if setup && script["runOnWorktreeCreate"] == true {
        script["runOnWorktreeCreate"] = json!(false);
    }
    if settle && script["runOnSettle"] == true {
        script["runOnSettle"] = json!(false);
    }
    script
}

/// `parseT3ProjectFile`'s scripts, read as leniently as web (comments, trailing commas). None when
/// any field fails the `T3ProjectFile` schema, which makes web ignore the whole file.
fn parse_file(contents: &str) -> Option<Vec<Value>> {
    let file = crate::settings::client::lenient_json(contents)?;
    let file = file.as_object()?;
    let text = |v: &Value| v.as_str().is_some_and(|v| !v.trim().is_empty());
    let one_of = |key: &str, allowed: &[&str]| file.get(key).is_none_or(|v| v.as_str().is_some_and(|v| allowed.contains(&v)));
    let valid = file.get("$schema").is_none_or(Value::is_string)
        && file.get("iconPath").is_none_or(|v| text(v) && v.as_str().is_some_and(|p| p.trim().chars().count() <= 512))
        && one_of("defaultThreadEnvMode", &["local", "worktree"])
        && one_of("worktreeSubmodules", &["recursive", "top-level", "none"]);
    if !valid {
        return None;
    }
    let Some(scripts) = file.get("scripts") else { return Some(vec![]) };
    let scripts = scripts.as_array().filter(|list| list.len() <= 50)?;
    let script = |v: &Value| {
        v.is_object()
            && text(&v["name"])
            && text(&v["command"])
            && v.get("icon").is_none_or(|i| ICONS.iter().any(|(id, _)| i.as_str() == Some(*id)))
            && v.get("previewUrl").is_none_or(text)
            && ["runOnWorktreeCreate", "runOnSettle", "async", "autoOpenPreview"].iter().all(|k| v.get(*k).is_none_or(Value::is_boolean))
    };
    scripts.iter().all(script).then(|| scripts.clone())
}

/// `importableScripts`: file scripts whose command and name (any case) no script already has.
fn importable(scripts: &[Value], file: &[Value]) -> Vec<Value> {
    let taken = |f: &Value| scripts.iter().any(|v| v["command"] == f["command"] || data::s(v, "name").to_lowercase() == data::s(f, "name").to_lowercase());
    file.iter().filter(|f| !taken(f)).cloned().collect()
}

/// The script an import adds (`importFileScript`), shaped as `build` saves one.
fn imported(id: &str, f: &Value) -> Value {
    let setup = f["runOnWorktreeCreate"] == true;
    let icon = Some(data::s(f, "icon")).filter(|i| !i.is_empty()).unwrap_or("play");
    let mut script = json!({"id": id, "name": data::s(f, "name").trim(), "command": data::s(f, "command").trim(), "icon": icon, "runOnWorktreeCreate": setup});
    if setup && f["async"] == false {
        script["async"] = json!(false);
    }
    if f["runOnSettle"] == true {
        script["runOnSettle"] = json!(true);
    }
    if let Some(url) = f["previewUrl"].as_str().map(str::trim).filter(|u| !u.is_empty()) {
        script["previewUrl"] = json!(url);
        script["autoOpenPreview"] = json!(f["autoOpenPreview"] == true);
    }
    script
}

/// Reads the checkout's `t3.json` once (`force` rereads it, as opening the menu does).
pub fn load_file(s: &mut App, force: bool) {
    let Some(cwd) = super::cwd(s) else { return };
    let key = (s.ui.env.active.clone(), cwd.clone());
    let ui = &s.ui.chat.details.scripts;
    if ui.reading.is_some() || (!force && ui.files.contains_key(&key)) {
        return;
    }
    let Some(net) = s.src.net() else { return };
    let id = net.request("projects.readFile", json!({"cwd": cwd, "relativePath": "t3.json", "allowMissing": true}));
    s.ui.chat.details.scripts.reading = Some((id, key));
}

fn file_scripts(s: &App) -> Vec<Value> {
    super::cwd(s).and_then(|cwd| s.ui.chat.details.scripts.files.get(&(s.ui.env.active.clone(), cwd)).cloned().flatten()).unwrap_or_default()
}

/// Settings > Projects reads the picked project's `t3.json` from its root while the page is open.
fn load_settings_file(s: &mut App) {
    if !matches!(&s.route, crate::Route::Settings(p) if p == "projects") {
        s.ui.chat.details.scripts.import_menu = None;
        return;
    }
    let Some(id) = s.ui.settings.project.clone() else { return };
    let root = s.state().projects.iter().find(|p| p["id"] == id.as_str()).map(|p| data::s(p, "workspaceRoot").to_string());
    let Some(root) = root.filter(|r| !r.is_empty()) else { return };
    let key = (crate::env::owner(s, &id), root.clone());
    let ui = &s.ui.chat.details.scripts;
    if ui.reading.is_some() || ui.files.contains_key(&key) {
        return;
    }
    // Parked environments hand their replies to `App::src`, where `poll_file` takes them.
    let Some(net) = crate::env::net_for(s, &id) else { return };
    let request = net.request("projects.readFile", json!({"cwd": root, "relativePath": "t3.json", "allowMissing": true}));
    s.ui.chat.details.scripts.reading = Some((request, key));
}

/// Settings > Projects > Actions: the picked project's `t3.json` scripts it doesn't have yet, or
/// None while that file is invalid.
pub fn project_imports(s: &App, project: &Value) -> Option<Vec<Value>> {
    let key = (crate::env::of(s, project), data::s(project, "workspaceRoot").to_owned());
    match s.ui.chat.details.scripts.files.get(&key) {
        Some(None) => None,
        Some(Some(file)) => Some(importable(&resolve(crate::settings::server_settings(s), project), file)),
        None => Some(vec![]),
    }
}

pub fn imports_open(s: &App, project: &str) -> bool {
    s.ui.chat.details.scripts.import_menu.as_deref() == Some(project)
}

pub fn toggle_imports(s: &mut App, project: &str) {
    let menu = &mut s.ui.chat.details.scripts.import_menu;
    *menu = if menu.as_deref() == Some(project) { None } else { Some(project.to_string()) };
}

pub fn project_targets<'a>(s: &'a App, project: &'a Value) -> Vec<(String, &'a Value, &'a Value)> {
    let owner = crate::env::of(s, project);
    let grouping = crate::shell::sidebar::folders::Grouping::read(s);
    let logical = grouping.key(project);
    let mut targets: Vec<(String, &'a Value, &'a Value)> = Vec::new();
    for candidate in std::iter::once(project).chain(s.state().projects.iter()) {
        let environment = crate::env::of(s, candidate);
        if (s.ui.settings.machine.is_some() && environment != owner)
            || targets.iter().any(|(key, member, _)| *key == environment && member["id"] == candidate["id"])
            || grouping.key(candidate) != logical
        {
            continue;
        }
        if let Some(settings) = crate::env::source(s, &environment)
            .and_then(|src| src.state().config.as_ref())
            .and_then(|config| config.get("settings"))
        {
            targets.push((environment, candidate, settings));
        }
    }
    targets
}

pub fn project_controls_locked(s: &App, project: &Value) -> bool {
    let targets = project_targets(s, project);
    s.ui.chat.details.scripts.pending.is_some()
        || targets.is_empty()
        || targets.iter().any(|(key, _, _)| !crate::settings::permissions::can_on(s, key, "settings:write"))
}

pub fn actions_differ(lists: impl IntoIterator<Item = Vec<Value>>) -> bool {
    let mut lists = lists.into_iter();
    lists.next().is_some_and(|first| lists.any(|list| list != first))
}

fn scoped_write(
    s: &mut App,
    transform: impl Fn(&[Value]) -> Option<Vec<Value>>,
    binding: Option<(&str, Option<&str>)>,
    imported: Option<Value>,
) {
    let Some(project) = project(s) else { return };
    let targets = project_targets(s, &project);
    if s.ui.chat.details.scripts.pending.is_some() || targets.is_empty() {
        palette::toast_error_brief(s, "Actions not saved", Some("No available machine, or another action change is saving."));
        return;
    }
    if targets.iter().any(|(key, _, _)| !crate::settings::permissions::can_on(s, key, "settings:write")) {
        palette::toast_error_brief(s, "Failed to save project actions", Some("This connection cannot change environment settings."));
        return;
    }

    let mut queue = Vec::new();
    for (environment, project, settings) in targets {
        let current = resolve(settings, project);
        let next = transform(&current);
        let project_id = data::s(project, "id");
        let patch = crate::settings::scope::override_patch(settings, project_id, "defaultProjectScripts", next.clone().map(Value::Array));
        queue.push((environment.clone(), "server.updateSettings", json!({"patch": patch})));
        let keybindings = crate::env::source(s, &environment)
            .and_then(|src| src.state().config.as_ref())
            .and_then(|config| config["keybindings"].as_array())
            .map_or(&[][..], Vec::as_slice);
        let effective = next.as_deref().or_else(|| settings["defaultProjectScripts"].as_array().map(Vec::as_slice)).unwrap_or_default();
        let removed = current.iter()
            .map(|script| data::s(script, "id"))
            .filter(|id| !effective.iter().any(|script| script["id"] == *id));
        let changes = binding.map_or_else(|| removed.map(|id| (id, None)).collect::<Vec<_>>(), |binding| vec![binding]);
        for (id, shortcut) in changes {
            let retained = !effective.iter().any(|script| script["id"] == id)
                && (settings["defaultProjectScripts"].as_array().is_some_and(|list| list.iter().any(|script| script["id"] == id))
                    || s.state().projects.iter().any(|other| {
                        crate::env::of(s, other) == environment && other["id"] != project_id
                            && resolve(settings, other).iter().any(|script| script["id"] == id)
                    }));
            queue.extend(keybinding_ops(keybindings, &command_for(id), shortcut, retained)
                .into_iter().map(|(method, payload)| (environment.clone(), method, payload)));
        }
    }
    let (environment, method, payload) = queue.remove(0);
    let Some(net) = crate::env::net_of(s, &environment) else {
        palette::toast_error_brief(s, "Actions not saved", Some("No available machine, or another action change is saving."));
        return;
    };
    let waiting = net.request(method, payload);
    s.ui.chat.details.scripts.pending = Some(Pending { waiting, queue, deleted: None, imported, scoped: true });
}

pub fn reset_for_project(s: &mut App, project: &str) {
    s.ui.chat.details.scripts.target = Some(project.to_owned());
    scoped_write(s, |_| None, None, None);
    if s.ui.chat.details.scripts.pending.is_none() {
        s.ui.chat.details.scripts.target = None;
    }
}

fn apply_script(current: &[Value], saved: &Value, adding: bool) -> Vec<Value> {
    let mut next: Vec<_> = current.iter().map(|script| {
        if script["id"] == saved["id"] {
            saved.clone()
        } else {
            release(script.clone(), saved["runOnWorktreeCreate"] == true, saved["runOnSettle"] == true)
        }
    }).collect();
    if adding {
        next.push(saved.clone());
    }
    next
}

fn available_ids(s: &App, scripts: &[Value]) -> Vec<String> {
    let mut ids: Vec<_> = scripts.iter().map(|script| data::s(script, "id").to_owned()).collect();
    if s.ui.chat.details.scripts.target.is_some() && let Some(project) = project(s) {
        for (_, candidate, settings) in project_targets(s, &project) {
            for list in [settings["defaultProjectScripts"].as_array(), candidate["scripts"].as_array()]
                .into_iter().chain(settings["projectSettingsOverrides"].as_object().into_iter()
                    .flat_map(|entries| entries.values().map(|entry| entry["defaultProjectScripts"].as_array())))
                .flatten()
            {
                ids.extend(list.iter().map(|script| data::s(script, "id").to_owned()));
            }
        }
        for project in &s.state().projects {
            if let Some(list) = project["scripts"].as_array() {
                ids.extend(list.iter().map(|script| data::s(script, "id").to_owned()));
            }
        }
    }
    ids
}

fn import(s: &mut App, file: &Value) {
    s.ui.chat.details.menu = None;
    s.ui.chat.details.scripts.import_menu = None;
    if s.ui.chat.details.scripts.pending.is_some() {
        return;
    }
    let scripts = scripts(s);
    let id = next_id(data::s(file, "name"), &available_ids(s, &scripts));
    let saved = imported(&id, file);
    if s.ui.chat.details.scripts.target.is_some() {
        scoped_write(s, |current| Some(apply_script(current, &saved, true)), Some((&id, None)), Some(saved.clone()));
    } else {
        persist(s, apply_script(&scripts, &saved, true), command_for(&id), None, None);
    }
}

/// The Import scripts menu's pick: added to the Settings project without opening the editor.
fn import_for_project(s: &mut App, project: &str, file: &Value) {
    if s.ui.chat.details.scripts.pending.is_some() || s.ui.chat.details.scripts.editor.is_some() {
        return;
    }
    s.ui.chat.details.scripts.target = Some(project.to_string());
    import(s, file);
    if s.ui.chat.details.scripts.pending.is_none() {
        s.ui.chat.details.scripts.target = None;
    }
}

fn project(s: &App) -> Option<Value> {
    if let Some(id) = &s.ui.chat.details.scripts.target {
        return s.state().projects.iter().find(|p| p["id"] == id.as_str()).cloned();
    }
    composer::ctx(s).project.cloned()
}

/// Settings > Projects > Actions: the same editor, for a project picked there.
pub fn open_for_project(s: &mut App, project: &str, script: Option<&Value>) {
    s.ui.chat.details.scripts.target = Some(project.to_string());
    open_editor(s, script);
}

/// Whether the editor or the Import scripts menu is open for a project Settings picked (Settings
/// draws them then).
pub fn settings_editor_open(s: &App) -> bool {
    let ui = &s.ui.chat.details.scripts;
    (ui.editor.is_some() && ui.target.is_some()) || ui.import_menu.is_some()
}

/// Escape closes the import menu, the delete confirm, then the editor.
pub fn escape(s: &mut App) -> bool {
    let ui = &mut s.ui.chat.details.scripts;
    if ui.import_menu.take().is_some() {
        return true;
    }
    if ui.confirm_delete {
        ui.confirm_delete = false;
        return true;
    }
    if let Some(editor) = ui.editor.as_mut().filter(|editor| editor.icons_open) {
        editor.icons_open = false;
        return true;
    }
    if ui.editor.is_none() {
        return false;
    }
    close(s);
    true
}

pub fn scripts(s: &App) -> Vec<Value> {
    project(s).map(|p| resolve(crate::settings::server_settings(s), &p)).unwrap_or_default()
}

fn preferred<'a>(s: &App, scripts: &'a [Value]) -> Option<&'a Value> {
    let project = project(s)?;
    let last = s.ui.chat.details.scripts.last.get(data::s(&project, "id"));
    last.and_then(|id| scripts.iter().find(|v| v["id"] == id.as_str())).or_else(|| primary(scripts))
}

/// `runProjectScript`: types the command into an idle drawer terminal and remembers it.
/// `hasTerminalWriteAccess`: running types into a terminal.
fn can_run(s: &App) -> bool {
    composer::ctx(s).thread.is_some() && super::grants(s, "terminal:operate")
}

/// Why this connection can't save the editor's change, as the editor's validation error.
fn edit_denied(s: &App, changes_keybinding: bool) -> Option<&'static str> {
    if s.ui.chat.details.scripts.target.is_some() {
        return project(s).filter(|project| !project_controls_locked(s, project)).is_none()
            .then_some("This connection cannot change environment settings.");
    }
    if !super::grants(s, "orchestration:operate") {
        Some("This connection cannot change project actions.")
    } else if changes_keybinding && !super::grants(s, "settings:write") {
        Some("This connection cannot change keyboard shortcuts.")
    } else {
        None
    }
}

/// A chat code block's "Run in terminal" (runShellCommand): the project-script terminal, the
/// command kept in shell history and not remembered as the last action.
pub fn run_command(s: &mut App, command: &str) {
    if !can_run(s) {
        return;
    }
    let worktree = composer::ctx(s).thread.map(|t| data::s(t, "worktreePath").to_string()).filter(|p| !p.is_empty());
    let Some(cwd) = super::cwd(s) else { return };
    crate::panel::drawer::run(s, &cwd, worktree.as_deref(), format!("{command}\r"));
}

pub fn run(s: &mut App, script: &Value) {
    if !can_run(s) {
        return;
    }
    let c = composer::ctx(s);
    let (Some(thread), Some(project)) = (c.thread, c.project) else { return };
    let worktree = Some(data::s(thread, "worktreePath").to_string()).filter(|p| !p.is_empty());
    let project = data::s(project, "id").to_string();
    let Some(cwd) = super::cwd(s) else { return };
    s.ui.chat.details.scripts.last.insert(project, data::s(script, "id").to_string());
    s.ui.chat.details.menu = None;
    crate::panel::drawer::run(s, &cwd, worktree.as_deref(), format!(" {}\r", data::s(script, "command")));
    if script["autoOpenPreview"] == true
        && let Some(url) = script["previewUrl"].as_str().filter(|u| !u.is_empty())
    {
        crate::panel::open_preview(s, url);
    }
}

fn open_editor(s: &mut App, script: Option<&Value>) {
    s.ui.chat.details.menu = None;
    let mut e = Editor { icon: "play".into(), ..Editor::default() };
    if let Some(v) = script {
        let id = data::s(v, "id").to_string();
        e.initial_keybinding = keybinding_for(&bindings(s), &command_for(&id));
        e.keybinding = e.initial_keybinding.clone().unwrap_or_default();
        e.name = TextEdit::new(data::s(v, "name"));
        e.command = TextEdit::new(data::s(v, "command"));
        e.preview = TextEdit::new(data::s(v, "previewUrl"));
        e.icon = Some(data::s(v, "icon")).filter(|i| !i.is_empty()).unwrap_or("play").to_string();
        e.setup = v["runOnWorktreeCreate"] == true;
        e.wait = e.setup && v["async"] == false;
        e.settle = v["runOnSettle"] == true;
        e.auto_preview = v["autoOpenPreview"] == true;
        e.id = Some(id);
    }
    e.name.focused = true;
    s.ui.chat.details.scripts.editor = Some(e);
}

/// Scene: the action editor on a connection that can read the project but not change it.
pub fn scene_locked(s: &mut App) {
    (crate::shell::SCENES[1].1)(s);
    open_editor(s, Some(&serde_json::json!({"id": "test", "name": "Test", "command": "bun run test", "icon": "test"})));
    super::set_session(s, Some(Ok(serde_json::json!({"authenticated": true, "scopes": ["orchestration:read", "terminal:operate"], "auth": {"serverUpdateScope": "environment:maintain"}}))));
}

/// Writes the project's next script list, then its keybinding changes (`persistProjectScripts`).
fn persist(s: &mut App, next: Vec<Value>, command: String, keybinding: Option<Option<String>>, deleted: Option<String>) {
    let Some(project) = project(s) else { return };
    let id = data::s(&project, "id").to_string();
    let settings = crate::settings::server_settings(s);
    let folded = s.state().config.as_ref().is_some_and(|c| c["environment"]["capabilities"]["projectSettingsOverrides"] == true);
    let patch = if folded {
        let mut entry = settings["projectSettingsOverrides"][&id].clone();
        if !entry.is_object() {
            entry = json!({});
        }
        entry["defaultProjectScripts"] = json!(next);
        json!({"projectSettingsOverrides": {&id: entry}})
    } else {
        json!({"projectScriptOverrides": {&id: next}})
    };
    let bindings = bindings(s);
    let script_id = command.trim_start_matches("script.").trim_end_matches(".run").to_string();
    let changes = keybinding.as_ref().is_some_and(|k| k.as_deref() != keybinding_for(&bindings, &command).as_deref());
    let queue = match keybinding.filter(|_| changes) {
        Some(key) => {
            let in_next = next.iter().any(|v| v["id"] == script_id.as_str());
            let retained = !in_next
                && (settings["defaultProjectScripts"].as_array().is_some_and(|v| v.iter().any(|v| v["id"] == script_id.as_str()))
                    || s.state().projects.iter().filter(|p| data::s(p, "id") != id).any(|p| resolve(settings, p).iter().any(|v| v["id"] == script_id.as_str())));
            keybinding_ops(&bindings, &command, key.as_deref(), retained)
        }
        None => vec![],
    };
    let environment = s.ui.env.active.clone();
    let queue = queue.into_iter().map(|(method, payload)| (environment.clone(), method, payload)).collect();
    let Some(net) = s.src.net() else { return };
    let waiting = net.request("server.updateSettings", json!({"patch": patch}));
    s.ui.chat.details.scripts.pending = Some(Pending { waiting, queue, deleted, imported: None, scoped: false });
}

fn save(s: &mut App) {
    let scripts = scripts(s);
    let ids = available_ids(s, &scripts);
    let changes_keybinding = s.ui.chat.details.scripts.editor.as_ref().is_some_and(|e| Some(e.keybinding.trim()).filter(|k| !k.is_empty()) != e.initial_keybinding.as_deref());
    let denied = edit_denied(s, changes_keybinding);
    let ui = &mut s.ui.chat.details.scripts;
    if ui.pending.is_some() {
        return;
    }
    let Some(e) = ui.editor.as_mut() else { return };
    e.error = if let Some(denied) = denied {
        Some(denied.into())
    } else if e.name.text.trim().is_empty() {
        Some("Name is required.".into())
    } else if e.command.text.trim().is_empty() {
        Some("Command is required.".into())
    } else {
        None
    };
    if e.error.is_some() {
        return;
    }
    let editing = e.id.clone();
    let id = editing.clone().unwrap_or_else(|| next_id(e.name.text.trim(), &ids));
    let saved = build(&id, e);
    let key = Some(e.keybinding.trim().to_string()).filter(|k| !k.is_empty());
    if ui.target.is_some() {
        scoped_write(s, |current| Some(apply_script(current, &saved, editing.is_none())), Some((&id, key.as_deref())), None);
    } else {
        let keybinding = (editing.is_none() || key != e.initial_keybinding).then_some(key);
        persist(s, apply_script(&scripts, &saved, editing.is_none()), command_for(&id), keybinding, None);
    }
}

fn delete(s: &mut App) {
    let denied = edit_denied(s, false);
    let ui = &mut s.ui.chat.details.scripts;
    ui.confirm_delete = false;
    if let (Some(denied), Some(e)) = (denied, ui.editor.as_mut()) {
        e.error = Some(denied.into());
        return;
    }
    if ui.target.is_some() {
        let Some(id) = ui.editor.as_ref().and_then(|e| e.id.clone()) else { return };
        scoped_write(s, |current| Some(current.iter().filter(|script| script["id"] != id.as_str()).cloned().collect()), Some((&id, None)), None);
        close_editor(s);
        return;
    }
    let Some(id) = ui.editor.take().and_then(|e| e.id) else { return };
    let scripts = scripts(s);
    let name = scripts.iter().find(|v| v["id"] == id.as_str()).map_or("Unknown", |v| data::s(v, "name")).to_string();
    let next = scripts.into_iter().filter(|v| v["id"] != id.as_str()).collect();
    persist(s, next, command_for(&id), Some(None), Some(name));
    s.ui.chat.details.scripts.target = None;
}

pub fn poll(s: &mut App) -> bool {
    load_settings_file(s);
    poll_file(s) | poll_save(s)
}

fn poll_file(s: &mut App) -> bool {
    let Some(id) = s.ui.chat.details.scripts.reading.as_ref().map(|r| r.0) else { return false };
    let Some(reply) = s.src.net().and_then(|net| net.take_reply(id)) else { return false };
    let Some((_, cwd)) = s.ui.chat.details.scripts.reading.take() else { return false };
    // Missing, truncated or unreadable reads as no file, as web's query does.
    let list = match reply {
        Ok(v) if v["exists"] != false && v["truncated"] != true => parse_file(data::s(&v, "contents")),
        _ => Some(vec![]),
    };
    s.ui.chat.details.scripts.files.insert(cwd, list);
    true
}

fn poll_save(s: &mut App) -> bool {
    let Some(id) = s.ui.chat.details.scripts.pending.as_ref().map(|p| p.waiting) else { return false };
    let Some(reply) = s.src.net().and_then(|net| net.take_reply(id)) else { return false };
    let Some(mut pending) = s.ui.chat.details.scripts.pending.take() else { return false };
    if let Err(e) = reply {
        let message = palette::error_message(&e, "Failed to save action.");
        if pending.scoped {
            palette::toast_error_brief(s, "Failed to save project actions", Some(&message));
        }
        if pending.deleted.is_some() {
            palette::toast(s, "Could not delete action", Some(&message));
        } else if let Some(editor) = s.ui.chat.details.scripts.editor.as_mut() {
            editor.error = Some(message);
        } else if let Some(script) = pending.imported {
            // Web's Settings import reopens the failed action as a new one in the editor.
            open_editor(s, Some(&script));
            if let Some(e) = s.ui.chat.details.scripts.editor.as_mut() {
                (e.id, e.initial_keybinding, e.error) = (None, None, Some(message));
                e.keybinding.clear();
            }
        } else if !pending.scoped {
            palette::toast(s, "Failed to import action.", Some(&message));
        } else {
            s.ui.chat.details.scripts.target = None;
        }
        return true;
    }
    if !pending.queue.is_empty() {
        let (environment, method, payload) = pending.queue.remove(0);
        if let Some(net) = crate::env::net_of(s, &environment) {
            pending.waiting = net.request(method, payload);
            s.ui.chat.details.scripts.pending = Some(pending);
        } else {
            palette::toast_error_brief(s, "Failed to save project actions", Some("An environment became unavailable."));
        }
        return true;
    }
    match pending.deleted {
        Some(name) => palette::toast(s, &format!("Deleted action \"{name}\""), None),
        None => {
            s.ui.chat.details.scripts.editor = None;
            s.ui.chat.details.scripts.target = None;
        }
    }
    true
}

/// The keybinding field records the next chord; Backspace or Delete clears it.
fn capture(e: &KeyEvent) -> Option<Option<String>> {
    let key = match &e.key {
        Key::Backspace | Key::Delete => return Some(None),
        Key::Text(v) if v == " " => "space".into(),
        Key::Text(v) => v.to_lowercase(),
        Key::Command(c) => c.to_string(),
        Key::Function(n) => format!("f{n}"),
        Key::Enter => "enter".into(),
        Key::Up => "up".into(),
        Key::Down => "down".into(),
        Key::Left => "left".into(),
        Key::Right => "right".into(),
        _ => return None,
    };
    let mut parts = vec![];
    if e.mods.cmd {
        parts.push("mod".to_string());
    }
    if e.mods.ctrl && !e.mods.cmd {
        parts.push("ctrl".into());
    }
    if e.mods.alt {
        parts.push("alt".into());
    }
    if e.mods.shift {
        parts.push("shift".into());
    }
    parts.push(key);
    Some(Some(parts.join("+")))
}

/// The editor's shortcut capture, then `script.{id}.run` shortcuts for the open thread.
pub fn on_key(s: &mut App, e: &KeyEvent) -> bool {
    if let Some(editor) = s.ui.chat.details.scripts.editor.as_mut() {
        let typing = editor.name.focused || editor.command.focused || editor.preview.focused;
        if editor.capturing && !typing && !s.ui.chat.details.scripts.confirm_delete
            && let Some(next) = capture(e)
        {
            editor.keybinding = next.unwrap_or_default();
            return true;
        }
        return false;
    }
    if composer::ctx(s).thread.is_none() || s.ui.chat.details.scripts.import_menu.is_some() {
        return false;
    }
    let scripts = scripts(s);
    let hit = scripts.iter().find(|v| crate::panel::shortcut(s, e, &command_for(data::s(v, "id")))).cloned();
    hit.map(|script| run(s, &script)).is_some()
}

/// The scripts control: a run split button, or "Add project script" when there are none.
pub fn view(s: &App, t: &T) -> El {
    let fg = t.c.foreground.alpha_mul(0.8);
    let muted = t.c.muted_foreground;
    let scripts = scripts(s);
    let Some(script) = preferred(s, &scripts) else {
        if !importable(&scripts, &file_scripts(s)).is_empty() {
            let main = super::action(t, "details-script-actions", icon(&icons::WRENCH, 16.0).color(muted), "Actions", fg).label("Project actions").on_press(|s: &mut App| super::toggle_menu(s, super::Menu::Scripts));
            return super::split(t, "details-scripts-menu", main, "Choose project action", super::Menu::Scripts).role("group").label("Project actions");
        }
        return super::action(t, "details-script", icon(&icons::PLUS, 14.0).color(fg), "Add project script", fg).gap(12.0).w_full().on_press(|s: &mut App| open_editor(s, None));
    };
    let name = data::s(script, "name");
    let owned = script.clone();
    let can_run = can_run(s);
    let main = super::action(t, "details-script-run", icon(icon_for(data::s(script, "icon")), 16.0).color(muted), name, fg).label(format!("Run {name}")).help(run_help(s, name));
    let main = if can_run { main.on_press(move |s: &mut App| run(s, &owned)) } else { main.opacity(0.64) };
    super::split(t, "details-scripts-menu", main, "Script actions", super::Menu::Scripts).role("group").label("Project scripts")
}

/// The scripts menu's rows: each script with its shortcut and edit button, then "Add".
/// The run button's tooltip: a connection that can't type into terminals is told to pair again.
fn run_help(s: &App, name: &str) -> String {
    if super::grants(s, "terminal:operate") { format!("Run {name}") } else { "Pair this client again with permission to run terminal commands.".into() }
}

pub fn menu_rows(s: &App, t: &T) -> Vec<El> {
    let can_run = can_run(s);
    let mut rows = vec![];
    for script in scripts(s) {
        let id = data::s(&script, "id").to_string();
        let glyph = icon(icon_for(data::s(&script, "icon")), 16.0).color(t.c.muted_foreground);
        let shortcut = Some(palette::shortcut_label(s, &command_for(&id))).filter(|k| !k.is_empty());
        let edit = script.clone();
        let gear = el()
            .key(format!("details-script-edit-{id}"))
            .size(24.0, 24.0)
            .center()
            .radius(radius::MD)
            .hover(t.c.accent)
            .cursor(Cursor::Pointer)
            .label(format!("Edit {}", data::s(&script, "name")))
            .help(format!("Edit {}", data::s(&script, "name")))
            .child(icon(&icons::SETTINGS, 14.0).color(t.c.muted_foreground))
            .on_press(move |s: &mut App| open_editor(s, Some(&edit)));
        let mut trailing = row().gap(6.0).center_y();
        if let Some(label) = shortcut {
            trailing = trailing.child(text(label).font(theme::TEXT_XS).color(t.c.muted_foreground));
        }
        let run_script = script.clone();
        let row = super::menu_row(t, Some(glyph), &menu_label(&script), can_run.then_some(move |s: &mut App| run(s, &run_script)), None);
        rows.push(row.child(trailing.child(gear)));
    }
    let imports = importable(&scripts(s), &file_scripts(s));
    if !imports.is_empty() {
        if !rows.is_empty() {
            rows.push(el().w_full().h(1.0).my(4.0).fill(t.c.border));
        }
        rows.push(super::group_label(t, "From t3.json"));
        for file in imports {
            let glyph = icon(icon_for(data::s(&file, "icon")), 16.0).color(t.c.muted_foreground);
            let name = data::s(&file, "name").to_string();
            let row = super::menu_row(t, Some(glyph), &name, Some(move |s: &mut App| import(s, &file)), None);
            rows.push(row.child(icon(&icons::DOWNLOAD, 14.0).color(t.c.muted_foreground).label("Import")));
        }
    }
    rows.push(super::menu_row(t, Some(icon(&icons::PLUS, 16.0).color(t.c.muted_foreground)), "Add project script", Some(|s: &mut App| open_editor(s, None)), None));
    rows
}

fn editor_mut(s: &mut App) -> &mut Editor {
    s.ui.chat.details.scripts.editor.get_or_insert_with(Editor::default)
}

fn switch_row(t: &T, label: &str, on: bool, disabled: bool, toggle: fn(&mut Editor)) -> El {
    let k = kit::theme(t);
    row()
        .w_full()
        .gap(12.0)
        .center_y()
        .px(12.0)
        .py(8.0)
        .radius(radius::MD)
        .fill(if t.dark { rune::Color::WHITE.alpha(0.035) } else { rune::Color::TRANSPARENT })
        .border(1.0, if t.dark { rune::Color::TRANSPARENT } else { t.c.border.alpha_mul(0.7) })
        .opacity(if disabled { 0.6 } else { 1.0 })
        .child(text(label.to_string()).font(theme::TEXT_SM).color(t.c.foreground).grow(1.0).shrink().min_w(0.0))
        .child(kit::Switch::new(on).label(label).hide_label().disabled(disabled).on_toggle(move |s: &mut App| toggle(editor_mut(s))).build(&k))
}

/// The key Settings gives the Import scripts trigger, which the menu hangs from.
pub const IMPORT_ANCHOR: &str = "project-import-scripts";

/// Settings' Import scripts menu (`ProjectActionsSettings`): each `t3.json` action not yet added.
fn import_menu(s: &App, t: &T, out: &mut Vec<El>) {
    let Some(id) = s.ui.chat.details.scripts.import_menu.clone() else { return };
    let Some(project) = s.state().projects.iter().find(|p| p["id"] == id.as_str()) else { return };
    if !matches!(&s.route, crate::Route::Settings(_)) {
        return;
    }
    let note = text("Add actions declared by this checkout without editing them first.").font(theme::TEXT_SM).color(t.c.muted_foreground).wrap().w(240.0).px(8.0).pb(8.0);
    let mut menu = kit::Menu::new("project-import-scripts-menu")
        .anchor(IMPORT_ANCHOR)
        .placement(kit::Placement::BottomEnd)
        .min_width(256.0)
        .open(true)
        .on_close(|s: &mut App| s.ui.chat.details.scripts.import_menu = None)
        .group_label("Import from t3.json")
        .row(note)
        .separator();
    for file in project_imports(s, project).unwrap_or_default() {
        let item = kit::MenuItem::new(data::s(&file, "name")).icon(icon_for(data::s(&file, "icon"))).description(data::s(&file, "command"));
        let project = id.clone();
        menu = menu.item(item.on_select(move |s: &mut App| import_for_project(s, &project, &file)));
    }
    menu.overlay(&kit::theme(t), out);
}

pub fn overlays(s: &App, t: &T, out: &mut Vec<El>) {
    import_menu(s, t, out);
    let ui = &s.ui.chat.details.scripts;
    let Some(e) = &ui.editor else { return };
    let k = kit::theme(t);
    let saving = ui.pending.is_some();
    // Web's fieldset: everything but Cancel waits on orchestration access, the shortcut on settings.
    let locked = saving || edit_denied(s, false).is_some();
    let shortcuts = !locked && edit_denied(s, true).is_none();
    let field_label = |v: &str| text(v.to_string()).font(theme::SM_MEDIUM).color(t.c.foreground);
    let help = |v: &str| text(v.to_string()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap();
    let picker = kit::Button::new("Choose icon")
        .variant("outline")
        .size("icon-lg")
        .icon(icon_for(&e.icon))
        .disabled(locked)
        .on_press(|s: &mut App| {
            let e = editor_mut(s);
            e.icons_open = !e.icons_open;
        })
        .build(t)
        .key("script-icon");
    let name = kit::Input::new(&e.name).placeholder("Test").disabled(locked).on_edit(|s: &mut App| &mut editor_mut(s).name).on_submit(|s: &mut App, _| save(s)).build(t).single_line().grow(1.0).shrink().min_w(0.0);
    let mut fields = col().w_full().gap(16.0).child(col().w_full().gap(6.0).child(field_label("Name")).child(row().w_full().gap(8.0).center_y().child(picker).child(name)));
    let icon_menu = e.icons_open.then(|| {
        let mut grid = col().gap(8.0);
        for pair in ICONS.chunks(3) {
            let mut line = row().gap(8.0);
            for &(id, label) in pair {
                let selected = e.icon == id;
                line = line.child(
                    col()
                        .w(88.0)
                        .gap(8.0)
                        .items_center()
                        .px(8.0)
                        .py(8.0)
                        .radius(radius::MD)
                        .fill(if selected { t.c.primary.alpha_mul(0.1) } else if t.dark { rune::Color::WHITE.alpha(0.035) } else { rune::Color::TRANSPARENT })
                        .border(1.0, if selected { t.c.primary.alpha_mul(0.7) } else { t.c.border.alpha_mul(0.7) })
                        .hover(t.c.accent.alpha_mul(0.6))
                        .cursor(Cursor::Pointer)
                        .role("button")
                        .label(label.to_string())
                        .child(icon(icon_for(id), 16.0).color(t.c.foreground))
                        .child(text(label.to_string()).font(theme::TEXT_XS).color(t.c.foreground))
                        .on_press(move |s: &mut App| {
                            let e = editor_mut(s);
                            (e.icon, e.icons_open) = (id.to_string(), false);
                        }),
                );
            }
            grid = grid.child(line);
        }
        el().key("script-icons").padding(8.0).radius(radius::LG).fill(t.c.popover).border(1.0, t.c.border).shadow(8.0, 24.0, rune::Color::BLACK.alpha(0.2)).child(grid)
            .anchored("script-icon", Side::Bottom, Align::Start, 4.0)
            .modal(true, |s: &mut App| editor_mut(s).icons_open = false)
    });
    let capturing = e.capturing && !(e.name.focused || e.command.focused || e.preview.focused);
    let mut keybinding = row()
        .key("script-keybinding")
        .w_full()
        .h(30.0)
        .px(11.0)
        .center_y()
        .radius(radius::MD)
        .fill(t.c.contrast_input)
        .border(if capturing { 2.0 } else { 1.0 }, if capturing { t.c.ring } else { t.c.input })
        .cursor(Cursor::Text)
        .role("textbox")
        .label("Keybinding")
        .child(text(if e.keybinding.is_empty() { "Press shortcut".to_string() } else { e.keybinding.clone() }).font(theme::TEXT_SM).color(if e.keybinding.is_empty() { t.c.contrast_placeholder } else { t.c.foreground }));
    keybinding = if shortcuts {
        keybinding.on_press(|s: &mut App| {
            let e = editor_mut(s);
            (e.name.focused, e.command.focused, e.preview.focused, e.capturing) = (false, false, false, true);
        })
    } else {
        keybinding.opacity(0.64).cursor(Cursor::Default)
    };
    fields = fields
        .child(col().w_full().gap(6.0).child(field_label("Keybinding")).child(keybinding).child(help(
            "Press a shortcut. Use Backspace to clear. Shortcuts are environment-wide. Projects using the same action share its shortcut.",
        )))
        .child(col().w_full().gap(6.0).child(field_label("Command")).child({
            let command = kit::TextArea::new(&e.command).placeholder("bun test").rows(3);
            // The fieldset is disabled as a whole; without its edit hook the command can't change.
            if locked { command.build(&k).w_full().opacity(0.64) } else { command.on_edit(|s: &mut App| &mut editor_mut(s).command).build(&k).w_full() }
        }))
        .child(
            col()
                .w_full()
                .gap(6.0)
                .child(field_label("Preview URL (optional)"))
                .child(kit::Input::new(&e.preview).placeholder("http://localhost:5173").disabled(locked).on_edit(|s: &mut App| &mut editor_mut(s).preview).on_submit(|s: &mut App, _| save(s)).build(t).single_line())
                .child(help("Open this URL in the in-app preview when this action runs.")),
        )
        .child(switch_row(t, "Run automatically on worktree creation", e.setup, locked, |e| e.setup = !e.setup))
        .child(switch_row(t, "Wait for it to finish before the agent starts", e.wait, locked || !e.setup, |e| e.wait = !e.wait))
        .child(switch_row(t, "Run in the thread's worktree when the thread settles", e.settle, locked, |e| e.settle = !e.settle))
        .child(switch_row(t, "Open preview automatically when this action runs", e.auto_preview, locked || e.preview.text.trim().is_empty(), |e| e.auto_preview = !e.auto_preview));
    if let Some(error) = &e.error {
        fields = fields.child(text(error.clone()).font(theme::TEXT_SM).color(t.c.destructive).wrap());
    }
    let editing = e.id.is_some();
    let mut footer = row().w_full().gap(8.0).center_y();
    if editing {
        footer = footer.child(kit::Button::new("Delete").variant("destructive-outline").disabled(locked).on_press(|s: &mut App| s.ui.chat.details.scripts.confirm_delete = true).build(t));
    }
    footer = footer
        .child(el().grow(1.0))
        .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
        .child(kit::Button::new(if saving { "Saving…" } else if editing { "Save changes" } else { "Save action" }).disabled(locked).on_press(save).build(t));
    kit::Dialog::new("script-editor", if editing { "Edit Action" } else { "Add Action" })
        .open(true)
        .description("Actions are project-scoped commands you can run from the top bar or keybindings.")
        .child(fields)
        .child(footer)
        .on_close(close)
        .build(t, out);
    out.extend(icon_menu);
    if ui.confirm_delete {
        let name = e.name.text.trim().to_string();
        kit::Dialog::new("script-delete", format!("Delete action \"{name}\"?"))
            .open(true)
            .alert()
            .width(448.0)
            .description("This action cannot be undone.")
            .child(
                row()
                    .w_full()
                    .gap(8.0)
                    .justify_end()
                    .child(kit::Button::new("Cancel").variant("outline").on_press(|s: &mut App| s.ui.chat.details.scripts.confirm_delete = false).build(t))
                    .child(kit::Button::new("Delete action").variant("destructive").on_press(delete).build(t)),
            )
            .on_close(|s: &mut App| s.ui.chat.details.scripts.confirm_delete = false)
            .build(t, out);
    }
}

fn close_editor(s: &mut App) {
    let ui = &mut s.ui.chat.details.scripts;
    ui.editor = None;
    ui.target = None;
    ui.confirm_delete = false;
}

fn close(s: &mut App) {
    if s.ui.chat.details.scripts.pending.is_none() {
        close_editor(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_ids_and_labels_follow_web() {
        assert_eq!(next_id(" Run Tests! ", &[]), "run-tests");
        assert_eq!(next_id("", &[]), "script");
        assert_eq!(next_id("test", &["test".into(), "test-2".into()]), "test-3");
        assert_eq!(next_id("a very long script name that keeps going", &[]), "a-very-long-script-name");
        assert_eq!(menu_label(&json!({"name": "Setup", "runOnWorktreeCreate": true, "runOnSettle": true})), "Setup (setup, on settle)");
        let scripts = vec![json!({"id": "setup", "runOnWorktreeCreate": true}), json!({"id": "dev"})];
        assert_eq!(primary(&scripts).unwrap()["id"], "dev");
    }

    #[test]
    fn resolve_prefers_overrides_then_project_scripts() {
        let project = json!({"id": "p", "scripts": [{"id": "own"}]});
        let settings = json!({"defaultProjectScripts": [{"id": "default"}], "projectSettingsOverrides": {}, "projectScriptOverrides": {}});
        assert_eq!(resolve(&settings, &project)[0]["id"], "own");
        let legacy = json!({"defaultProjectScripts": [{"id": "default"}], "projectScriptOverrides": {"p": null}});
        assert_eq!(resolve(&legacy, &project)[0]["id"], "default");
        let folded = json!({"projectSettingsOverrides": {"p": {"defaultProjectScripts": [{"id": "new"}]}}});
        assert_eq!(resolve(&folded, &project)[0]["id"], "new");
    }

    #[test]
    fn keybinding_writes_replace_the_previous_rule() {
        let bindings = vec![json!({"command": "script.dev.run", "shortcut": {"key": "d", "modKey": true, "shiftKey": true}})];
        let ops = keybinding_ops(&bindings, "script.dev.run", Some("mod+r"), false);
        assert_eq!(ops, vec![("server.upsertKeybinding", json!({"key": "mod+r", "command": "script.dev.run", "replace": {"key": "mod+shift+d", "command": "script.dev.run"}}))]);
        assert_eq!(keybinding_ops(&bindings, "script.dev.run", None, false), vec![("server.removeKeybinding", json!({"key": "mod+shift+d", "command": "script.dev.run"}))]);
        assert!(keybinding_ops(&bindings, "script.dev.run", None, true).is_empty());
        assert_eq!(keybinding_ops(&[], "script.x.run", Some("mod+x"), false), vec![("server.upsertKeybinding", json!({"key": "mod+x", "command": "script.x.run"}))]);
    }

    #[test]
    fn adding_to_mixed_actions_preserves_each_checkout_list() {
        let left = vec![json!({"id": "dev", "runOnWorktreeCreate": true})];
        let right = vec![json!({"id": "build", "runOnWorktreeCreate": true})];
        assert!(actions_differ([left.clone(), right.clone()]));
        assert!(!actions_differ([left.clone(), left.clone()]));
        let setup = json!({"id": "setup", "runOnWorktreeCreate": true});
        assert_eq!(apply_script(&left, &setup, true), vec![json!({"id": "dev", "runOnWorktreeCreate": false}), setup.clone()]);
        assert_eq!(apply_script(&right, &setup, true), vec![json!({"id": "build", "runOnWorktreeCreate": false}), setup]);
    }

    #[test]
    fn scoped_write_targets_each_environment_with_its_own_actions() {
        use crate::{Route, data::Source, env, settings::permissions};

        let checkout = |id: &str, script: &str, environment: Option<&str>| {
            let mut project = json!({
                "id": id,
                "workspaceRoot": "/workspace/repo",
                "repositoryIdentity": {"canonicalKey": "github.com/team/repo"},
                "scripts": [{"id": script}]
            });
            if let Some(environment) = environment {
                project[env::TAG] = json!(environment);
            }
            project
        };
        let mut app = App::new();
        app.client_settings = crate::settings::client::ClientSettings::default();
        let (mut local, local_sent) = s5_net::Net::in_memory_sent("http://local.test");
        let (mut remote, remote_sent) = s5_net::Net::in_memory_sent("http://remote.test");
        local.state_mut().config = Some(json!({"settings": {}}));
        remote.state_mut().config = Some(json!({"settings": {}}));
        local.state_mut().projects = vec![checkout("local-project", "dev", None), checkout("remote-project", "build", Some("remote"))];
        app.src = Source::Live(local);
        app.route = Route::Settings("projects".into());
        app.ui.settings.project = Some("local-project".into());
        app.ui.chat.details.scripts.target = app.ui.settings.project.clone();
        env::park(&mut app, "remote", Source::Live(remote));
        let active = app.ui.env.active.clone();
        for key in [active.as_str(), "remote"] {
            permissions::set_session(&mut app, key, json!({"authenticated": true, "permissions": ["settings:write"]}));
        }

        let added = json!({"id": "lint"});
        scoped_write(&mut app, |current| Some(apply_script(current, &added, true)), Some(("lint", None)), None);
        let (id, method, request) = local_sent.requests().pop().unwrap();
        assert_eq!(method, "server.updateSettings");
        assert_eq!(request["patch"]["projectSettingsOverrides"]["local-project"]["defaultProjectScripts"], json!([{"id": "dev"}, {"id": "lint"}]));
        assert!(remote_sent.requests().is_empty());

        app.src.net().unwrap().put_reply(id, Ok(Value::Null));
        assert!(poll_save(&mut app));
        let (id, method, request) = remote_sent.requests().pop().unwrap();
        assert_eq!(method, "server.updateSettings");
        assert_eq!(request["patch"]["projectSettingsOverrides"]["remote-project"]["defaultProjectScripts"], json!([{"id": "build"}, {"id": "lint"}]));
        app.src.net().unwrap().put_reply(id, Ok(Value::Null));
        assert!(poll_save(&mut app));
        assert!(app.ui.chat.details.scripts.pending.is_none());
    }

    #[test]
    fn saving_releases_claimed_roles() {
        let other = json!({"id": "a", "runOnWorktreeCreate": true, "runOnSettle": true});
        assert_eq!(release(other.clone(), true, false)["runOnWorktreeCreate"], false);
        assert_eq!(release(other, false, true)["runOnSettle"], false);
        let e = Editor { name: TextEdit::new("Dev"), command: TextEdit::new("bun dev"), icon: "play".into(), setup: true, wait: true, ..Editor::default() };
        assert_eq!(build("dev", &e), json!({"id": "dev", "name": "Dev", "command": "bun dev", "icon": "play", "runOnWorktreeCreate": true, "async": false}));
    }

    #[test]
    fn t3_json_offers_only_new_scripts_for_import() {
        let file = parse_file(
            r#"{
                // Comments and trailing commas parse, as web's lenient JSON does.
                "scripts": [{"name": "Dev", "command": "bun dev"}, {"name": "test", "command": "bun test"}, {"name": "Lint", "command": "bun lint",},],
            }"#,
        )
        .expect("valid file");
        assert_eq!(file.len(), 3);
        assert_eq!(parse_file("{not json"), None);
        assert_eq!(parse_file(r#"{"iconPath": "logo.svg"}"#), Some(vec![]), "no scripts is valid");
        for invalid in [
            r#"{"scripts": [{"name": "Dev", "command": "bun dev"}, {"name": " ", "command": "x"}]}"#,
            r#"{"scripts": [{"name": "Dev", "command": "bun dev", "icon": "rocket"}]}"#,
            r#"{"scripts": [{"name": "Dev", "command": "bun dev", "async": "no"}]}"#,
            r#"{"scripts": {"name": "Dev"}}"#,
            r#"{"defaultThreadEnvMode": "cloud"}"#,
            r#"[]"#,
        ] {
            assert_eq!(parse_file(invalid), None, "one bad field ignores the whole file: {invalid}");
        }
        let scripts = vec![json!({"id": "t", "name": "Test", "command": "vitest"}), json!({"id": "l", "name": "Check", "command": "bun lint"})];
        let offered = importable(&scripts, &file);
        let names: Vec<&str> = offered.iter().map(|v| data::s(v, "name")).collect();
        assert_eq!(names, vec!["Dev"], "same name in any case, or same command, is already there");
        let setup = json!({"name": "Setup", "command": "bun i", "runOnWorktreeCreate": true, "async": false, "previewUrl": "http://localhost:3000"});
        assert_eq!(
            imported("setup", &setup),
            json!({"id": "setup", "name": "Setup", "command": "bun i", "icon": "play", "runOnWorktreeCreate": true, "async": false, "previewUrl": "http://localhost:3000", "autoOpenPreview": false})
        );
    }

    #[test]
    fn a_connection_without_the_scopes_can_neither_run_nor_save_actions() {
        let mut s = App::new();
        // A modern session: a legacy one (no `auth`) lets orchestration:operate stand in for settings:write.
        let session = |scopes: &[&str]| Some(Ok(json!({"authenticated": true, "scopes": scopes, "auth": {"serverUpdateScope": "environment:maintain"}})));
        open_editor(&mut s, None);
        editor_mut(&mut s).name = TextEdit::new("Test");
        editor_mut(&mut s).command = TextEdit::new("bun test");
        editor_mut(&mut s).keybinding = "mod+shift+t".into();

        super::super::set_session(&mut s, session(&["orchestration:read"]));
        assert!(!can_run(&s), "FAILED: running needs terminal:operate");
        assert_eq!(run_help(&s, "Test"), "Pair this client again with permission to run terminal commands.");
        save(&mut s);
        assert_eq!(s.ui.chat.details.scripts.editor.as_ref().and_then(|e| e.error.as_deref()), Some("This connection cannot change project actions."));

        super::super::set_session(&mut s, session(&["orchestration:operate"]));
        save(&mut s);
        assert_eq!(s.ui.chat.details.scripts.editor.as_ref().and_then(|e| e.error.as_deref()), Some("This connection cannot change keyboard shortcuts."), "FAILED: a new shortcut needs settings:write");
        assert!(s.ui.chat.details.scripts.pending.is_none(), "FAILED: nothing was written");

        editor_mut(&mut s).keybinding.clear();
        assert_eq!(edit_denied(&s, false), None, "FAILED: without a shortcut, orchestration access is enough");
    }
}
