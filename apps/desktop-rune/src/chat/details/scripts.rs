//! Project scripts in the details card (ProjectScriptsControl): the run split button and its
//! menu, the add/edit dialog with shortcut capture, delete, and running in the terminal drawer.
use std::collections::HashMap;

use rune::{Cursor, El, Icon, Key, KeyEvent, TextEdit, col, el, icon, row, text};
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
    queue: Vec<(&'static str, Value)>,
    /// A delete's script name, for its toast.
    deleted: Option<String>,
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

fn command_for(id: &str) -> String {
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

fn project(s: &App) -> Option<Value> {
    composer::ctx(s).project.cloned()
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
pub fn run(s: &mut App, script: &Value) {
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
    let Some(net) = s.src.net() else { return };
    let waiting = net.request("server.updateSettings", json!({"patch": patch}));
    s.ui.chat.details.scripts.pending = Some(Pending { waiting, queue, deleted });
}

fn save(s: &mut App) {
    let scripts = scripts(s);
    let ui = &mut s.ui.chat.details.scripts;
    if ui.pending.is_some() {
        return;
    }
    let Some(e) = ui.editor.as_mut() else { return };
    e.error = if e.name.text.trim().is_empty() {
        Some("Name is required.".into())
    } else if e.command.text.trim().is_empty() {
        Some("Command is required.".into())
    } else {
        None
    };
    if e.error.is_some() {
        return;
    }
    let ids: Vec<String> = scripts.iter().map(|v| data::s(v, "id").to_string()).collect();
    let editing = e.id.clone();
    let id = editing.clone().unwrap_or_else(|| next_id(e.name.text.trim(), &ids));
    let saved = build(&id, e);
    let (setup, settle) = (e.setup, e.settle);
    let key = Some(e.keybinding.trim().to_string()).filter(|k| !k.is_empty());
    let keybinding = (editing.is_none() || key != e.initial_keybinding).then_some(key);
    let mut next: Vec<Value> = scripts.into_iter().map(|v| if editing.as_deref() == Some(data::s(&v, "id")) { saved.clone() } else { release(v, setup, settle) }).collect();
    if editing.is_none() {
        next.push(saved);
    }
    persist(s, next, command_for(&id), keybinding, None);
}

fn delete(s: &mut App) {
    let ui = &mut s.ui.chat.details.scripts;
    ui.confirm_delete = false;
    let Some(id) = ui.editor.take().and_then(|e| e.id) else { return };
    let scripts = scripts(s);
    let name = scripts.iter().find(|v| v["id"] == id.as_str()).map_or("Unknown", |v| data::s(v, "name")).to_string();
    let next = scripts.into_iter().filter(|v| v["id"] != id.as_str()).collect();
    persist(s, next, command_for(&id), Some(None), Some(name));
}

pub fn poll(s: &mut App) -> bool {
    let Some(id) = s.ui.chat.details.scripts.pending.as_ref().map(|p| p.waiting) else { return false };
    let Some(reply) = s.src.net().and_then(|net| net.take_reply(id)) else { return false };
    let Some(mut pending) = s.ui.chat.details.scripts.pending.take() else { return false };
    if let Err(e) = reply {
        let message = palette::error_message(&e, "Failed to save action.");
        if pending.deleted.is_some() {
            palette::toast(s, "Could not delete action", Some(&message));
        } else if let Some(editor) = s.ui.chat.details.scripts.editor.as_mut() {
            editor.error = Some(message);
        }
        return true;
    }
    if !pending.queue.is_empty() {
        let (method, payload) = pending.queue.remove(0);
        if let Some(net) = s.src.net() {
            pending.waiting = net.request(method, payload);
            s.ui.chat.details.scripts.pending = Some(pending);
        }
        return true;
    }
    match pending.deleted {
        Some(name) => palette::toast(s, &format!("Deleted action \"{name}\""), None),
        None => s.ui.chat.details.scripts.editor = None,
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
    if composer::ctx(s).thread.is_none() {
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
        return super::action(t, "details-script", icon(&icons::PLUS, 14.0).color(fg), "Add project script", fg).gap(12.0).w_full().on_press(|s: &mut App| open_editor(s, None));
    };
    let name = data::s(script, "name");
    let owned = script.clone();
    let can_run = composer::ctx(s).thread.is_some();
    let main = super::action(t, "details-script-run", icon(icon_for(data::s(script, "icon")), 16.0).color(muted), name, fg).help(format!("Run {name}"));
    let main = if can_run { main.on_press(move |s: &mut App| run(s, &owned)) } else { main.opacity(0.64) };
    super::split(t, "details-scripts-menu", main, "Project scripts", super::Menu::Scripts)
}

/// The scripts menu's rows: each script with its shortcut and edit button, then "Add".
pub fn menu_rows(s: &App, t: &T) -> Vec<El> {
    let can_run = composer::ctx(s).thread.is_some();
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

pub fn overlays(s: &App, t: &T, out: &mut Vec<El>) {
    let ui = &s.ui.chat.details.scripts;
    let Some(e) = &ui.editor else { return };
    let k = kit::theme(t);
    let saving = ui.pending.is_some();
    let field_label = |v: &str| text(v.to_string()).font(theme::SM_MEDIUM).color(t.c.foreground);
    let help = |v: &str| text(v.to_string()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap();
    let picker = kit::Button::new("Choose icon")
        .variant("outline")
        .size("icon-lg")
        .icon(icon_for(&e.icon))
        .on_press(|s: &mut App| {
            let e = editor_mut(s);
            e.icons_open = !e.icons_open;
        })
        .build(t)
        .key("script-icon");
    let name = kit::Input::new(&e.name).placeholder("Test").disabled(saving).on_edit(|s: &mut App| &mut editor_mut(s).name).on_submit(|s: &mut App, _| save(s)).build(t).single_line().grow(1.0).shrink().min_w(0.0);
    let mut fields = col().w_full().gap(16.0).child(col().w_full().gap(6.0).child(field_label("Name")).child(row().w_full().gap(8.0).center_y().child(picker).child(name)));
    if e.icons_open {
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
        fields = fields.child(el().padding(8.0).radius(radius::LG).fill(t.c.popover).border(1.0, t.c.border).shadow(8.0, 24.0, rune::Color::BLACK.alpha(0.2)).child(grid));
    }
    let capturing = e.capturing && !(e.name.focused || e.command.focused || e.preview.focused);
    let keybinding = row()
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
        .child(text(if e.keybinding.is_empty() { "Press shortcut".to_string() } else { e.keybinding.clone() }).font(theme::TEXT_SM).color(if e.keybinding.is_empty() { t.c.contrast_placeholder } else { t.c.foreground }))
        .on_press(|s: &mut App| {
            let e = editor_mut(s);
            (e.name.focused, e.command.focused, e.preview.focused, e.capturing) = (false, false, false, true);
        });
    fields = fields
        .child(col().w_full().gap(6.0).child(field_label("Keybinding")).child(keybinding).child(help(
            "Press a shortcut. Use Backspace to clear. Shortcuts are environment-wide. Projects using the same action share its shortcut.",
        )))
        .child(col().w_full().gap(6.0).child(field_label("Command")).child(kit::TextArea::new(&e.command).placeholder("bun test").rows(3).on_edit(|s: &mut App| &mut editor_mut(s).command).build(&k)))
        .child(
            col()
                .w_full()
                .gap(6.0)
                .child(field_label("Preview URL (optional)"))
                .child(kit::Input::new(&e.preview).placeholder("http://localhost:5173").on_edit(|s: &mut App| &mut editor_mut(s).preview).build(t).single_line())
                .child(help("Open this URL in the in-app preview when this action runs.")),
        )
        .child(switch_row(t, "Run automatically on worktree creation", e.setup, saving, |e| e.setup = !e.setup))
        .child(switch_row(t, "Wait for it to finish before the agent starts", e.wait, saving || !e.setup, |e| e.wait = !e.wait))
        .child(switch_row(t, "Run in the thread's worktree when the thread settles", e.settle, saving, |e| e.settle = !e.settle))
        .child(switch_row(t, "Open preview automatically when this action runs", e.auto_preview, saving || e.preview.text.trim().is_empty(), |e| e.auto_preview = !e.auto_preview));
    if let Some(error) = &e.error {
        fields = fields.child(text(error.clone()).font(theme::TEXT_SM).color(t.c.destructive).wrap());
    }
    let editing = e.id.is_some();
    let mut footer = row().w_full().gap(8.0).center_y();
    if editing {
        footer = footer.child(kit::Button::new("Delete").variant("destructive-outline").disabled(saving).on_press(|s: &mut App| s.ui.chat.details.scripts.confirm_delete = true).build(t));
    }
    footer = footer
        .child(el().grow(1.0))
        .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
        .child(kit::Button::new(if saving { "Saving…" } else if editing { "Save changes" } else { "Save action" }).disabled(saving).on_press(save).build(t));
    kit::Dialog::new("script-editor", if editing { "Edit Action" } else { "Add Action" })
        .open(true)
        .description("Actions are project-scoped commands you can run from the top bar or keybindings.")
        .child(fields)
        .child(footer)
        .on_close(close)
        .build(t, out);
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

fn close(s: &mut App) {
    let ui = &mut s.ui.chat.details.scripts;
    if ui.pending.is_none() {
        ui.editor = None;
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
    fn saving_releases_claimed_roles() {
        let other = json!({"id": "a", "runOnWorktreeCreate": true, "runOnSettle": true});
        assert_eq!(release(other.clone(), true, false)["runOnWorktreeCreate"], false);
        assert_eq!(release(other, false, true)["runOnSettle"], false);
        let e = Editor { name: TextEdit::new("Dev"), command: TextEdit::new("bun dev"), icon: "play".into(), setup: true, wait: true, ..Editor::default() };
        assert_eq!(build("dev", &e), json!({"id": "dev", "name": "Dev", "command": "bun dev", "icon": "play", "runOnWorktreeCreate": true, "async": false}));
    }
}
