use super::{controls as c, request};
use crate::App;
use crate::theme::{self, T};
use rune::{El, col, row, text, text_input};
use serde_json::{Value, json};

const GROUPS: &[(&str, &[&str])] = &[
    (
        "Navigation",
        &[
            "sidebar",
            "rightPanel",
            "commandPalette",
            "filePicker",
            "projectSearch",
            "editor",
        ],
    ),
    ("Threads", &["thread", "chat", "pullRequest"]),
    ("Composer", &["composer", "modelPicker"]),
    ("Terminal", &["terminal"]),
    ("Preview & diff", &["preview", "diff"]),
    ("Appearance", &["theme", "appearance", "themeEditor"]),
    ("Project scripts", &["script"]),
    ("Other", &[]),
];

pub fn view(s: &App, t: &T) -> El {
    let mut body = col().w_full().gap(32.0);
    let search = text_input(&s.ui.settings.keybinding_search, theme::TEXT_SM)
        .single_line()
        .placeholder("Search keybindings", t.c.muted_foreground)
        .w(280.0)
        .h(32.0)
        .px(12.0)
        .radius(6.0)
        .border(1.0, t.c.border)
        .color(t.c.foreground)
        .on_edit(|s: &mut App| &mut s.ui.settings.keybinding_search);
    body = body.child(c::section(
        "Keybindings",
        vec![row().padding(12.0).gap(8.0).child(search)],
        t,
    ));
    let bindings = s
        .state()
        .config
        .as_ref()
        .and_then(|c| c["keybindings"].as_array());
    let query = s.ui.settings.keybinding_search.text.to_lowercase();
    for (group, _) in GROUPS {
        let mut entries: Vec<_> = bindings
            .into_iter()
            .flatten()
            .filter(|binding| {
                let command = binding["command"].as_str().unwrap_or("");
                group_for(command) == *group
                    && (query.is_empty() || label(command).to_lowercase().contains(&query))
            })
            .collect();
        entries.sort_by_key(|b| label(b["command"].as_str().unwrap_or("")));
        if entries.is_empty() {
            continue;
        }
        let rows = entries
            .into_iter()
            .enumerate()
            .map(|(i, binding)| {
                let command = binding["command"].as_str().unwrap_or("");
                let shortcut = &binding["shortcut"];
                let mut target = json!({"command":command,"key":shortcut_input(shortcut)});
                let when = when_expression(&binding["whenAst"]);
                if !when.is_empty() {
                    target["when"] = json!(when);
                }
                let owned = command.to_owned();
                let add_command = owned.clone();
                let remove = target.clone();
                let reset_target = target.clone();
                let default = defaults()
                    .iter()
                    .find(|b| b["command"] == command && b["when"].as_str().unwrap_or("") == when)
                    .cloned();
                let edit_when = when.clone();
                row()
                    .key(format!("binding-{command}-{i}"))
                    .w_full()
                    .px(16.0)
                    .py(12.0)
                    .center_y()
                    .gap(16.0)
                    .child(
                        col()
                            .grow(1.0)
                            .gap(4.0)
                            .child(
                                text(label(command))
                                    .font(theme::SM_MEDIUM)
                                    .color(t.c.foreground),
                            )
                            .child(
                                text(if when.is_empty() {
                                    "Always".to_owned()
                                } else {
                                    when
                                })
                                .font(theme::MONO_XS)
                                .color(t.c.muted_foreground),
                            ),
                    )
                    .child(
                        c::button(&shortcut_label(shortcut), t).on_press(move |s: &mut App| {
                            s.ui.settings.keybinding_editor = Some(owned.clone());
                            s.ui.settings.binding_target = Some(target.clone());
                            s.ui.settings.shortcut = None;
                            s.ui.settings.binding_when = rune::TextEdit::new(edit_when.clone());
                        }),
                    )
                    .child(c::button("Add", t).on_press(move |s: &mut App| {
                        s.ui.settings.keybinding_editor = Some(add_command.clone());
                        s.ui.settings.binding_target = None;
                        s.ui.settings.shortcut = None;
                        s.ui.settings.binding_when = rune::TextEdit::new("");
                    }))
                    .child(c::button("Remove", t).on_press(move |s: &mut App| {
                        request(s, "server.removeKeybinding", remove.clone())
                    }))
                    .child(c::button("Reset", t).disabled(default.is_none()).on_press(
                        move |s: &mut App| {
                            if let Some(default) = &default {
                                let mut payload = default.clone();
                                payload["replace"] = reset_target.clone();
                                request(s, "server.upsertKeybinding", payload);
                            }
                        },
                    ))
            })
            .collect();
        body = body.child(c::section(group, rows, t));
    }

    body
}

fn group_for(command: &str) -> &'static str {
    let prefix = command.split('.').next().unwrap_or("");
    GROUPS
        .iter()
        .find(|(_, prefixes)| prefixes.contains(&prefix))
        .map(|(name, _)| *name)
        .unwrap_or("Other")
}
fn label(command: &str) -> String {
    match command {
        "workspace.focusNext" => return "Focus Next Workspace Area".into(),
        "workspace.focusPrevious" => return "Focus Previous Workspace Area".into(),
        "composer.sendAlternate" => return "Composer: Opposite Queue or Steer Action".into(),
        "composer.sendBackground" => return "Composer: Start in Background".into(),
        "composer.sendAndNewThread" => return "Composer: Send and Start New Thread".into(),
        "thread.steerQueuedMessage" => return "Queue: Send First Queued Message".into(),
        "thread.editQueuedMessage" => return "Queue: Edit Last Queued Message".into(),
        "thread.copyReference" => return "Pull Request: Copy Link or Thread".into(),
        "view.reopenClosed" => return "Reopen Closed Tab".into(),
        "preview.focusUrl" => return "Preview: Focus URL".into(),
        _ => {}
    }
    command
        .split('.')
        .map(|part| {
            let mut out = String::new();
            let mut previous_lower = false;
            for c in part.chars() {
                if c.is_uppercase() && previous_lower {
                    out.push(' ');
                }
                if c == '_' || c == '-' {
                    out.push(' ');
                } else if out.is_empty() || out.ends_with(' ') {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
                previous_lower = c.is_lowercase();
            }
            out
        })
        .collect::<Vec<_>>()
        .join(": ")
}
fn shortcut_input(shortcut: &Value) -> String {
    let mut parts = Vec::new();
    for (field, name) in [
        ("modKey", "mod"),
        ("metaKey", "meta"),
        ("ctrlKey", "ctrl"),
        ("altKey", "alt"),
        ("shiftKey", "shift"),
    ] {
        if shortcut[field].as_bool() == Some(true) {
            parts.push(name.to_owned());
        }
    }
    let key = shortcut["key"].as_str().unwrap_or("");
    parts.push(if key == " " { "space" } else { key }.to_owned());
    parts.join("+")
}
fn shortcut_label(shortcut: &Value) -> String {
    let mut out = String::new();
    for (field, symbol) in [
        ("ctrlKey", "⌃"),
        ("altKey", "⌥"),
        ("shiftKey", "⇧"),
        ("metaKey", "⌘"),
        ("modKey", "⌘"),
    ] {
        if shortcut[field].as_bool() == Some(true) {
            out.push_str(symbol);
        }
    }
    out.push_str(&shortcut["key"].as_str().unwrap_or("").to_uppercase());
    out
}

fn defaults() -> &'static Vec<Value> {
    static DEFAULTS: std::sync::OnceLock<Vec<Value>> = std::sync::OnceLock::new();
    DEFAULTS.get_or_init(|| serde_json::from_str(include_str!("keybinding-defaults.json")).unwrap())
}

pub fn editor(s: &App, t: &T) -> Option<El> {
    if let Some(command) = &s.ui.settings.keybinding_editor {
        return Some(
            col()
                .w(480.0)
                .padding(24.0)
                .gap(16.0)
                .radius(12.0)
                .border(1.0, t.c.border)
                .fill(t.c.popover)
                .child(
                    text(format!("Edit shortcut: {}", label(command)))
                        .font(theme::SM_MEDIUM)
                        .color(t.c.foreground),
                )
                .child(
                    text(
                        s.ui.settings
                            .shortcut
                            .as_deref()
                            .unwrap_or("Press a key combination")
                            .to_owned(),
                    )
                    .font(theme::TEXT_SM)
                    .color(t.c.muted_foreground),
                )
                .child(text("When").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(crate::kit::Input::new(&s.ui.settings.binding_when).placeholder("Always (e.g. !terminalFocus)")
                    .on_edit(|s: &mut App| &mut s.ui.settings.binding_when).build(t).single_line())
                .child(text(if conflicts(s,command) { "This shortcut is already assigned. Check that the When conditions do not overlap." } else { "" })
                    .font(theme::TEXT_XS).color(t.c.destructive).wrap())
                .child(
                    row()
                        .gap(8.0)
                        .child(
                            c::button("Save", t)
                                .disabled(s.ui.settings.shortcut.is_none())
                                .on_press(|s: &mut App| {
                                    if let (Some(command), Some(key)) = (
                                        s.ui.settings.keybinding_editor.clone(),
                                        s.ui.settings.shortcut.clone(),
                                    ) {
                                        let mut payload = json!({"command":command,"key":key});
                                        if let Some(target) = &s.ui.settings.binding_target {
                                            payload["replace"] = target.clone();
                                        }
                                        let when=s.ui.settings.binding_when.text.trim();
                                        if !when.is_empty() { payload["when"]=json!(when); }
                                        request(s, "server.upsertKeybinding", payload);
                                        s.ui.settings.keybinding_editor = None;
                                        s.ui.settings.shortcut = None;
                                    }
                                }),
                        )
                        .child(
                            c::button("Cancel", t)
                                .on_press(|s: &mut App| s.ui.settings.keybinding_editor = None),
                        ),
                )
                .modal(true, |s: &mut App| s.ui.settings.keybinding_editor = None),
        );
    }
    None
}

fn conflicts(s: &App, command: &str) -> bool {
    let Some(key) = &s.ui.settings.shortcut else {
        return false;
    };
    s.state()
        .config
        .as_ref()
        .and_then(|c| c["keybindings"].as_array())
        .is_some_and(|bindings| {
            bindings
                .iter()
                .any(|b| shortcut_input(&b["shortcut"]) == *key && b["command"] != command)
        })
}

fn when_expression(ast: &Value) -> String {
    match ast["type"].as_str() {
        Some("identifier") => ast["name"].as_str().unwrap_or("").to_owned(),
        Some("not") => format!("!({})", when_expression(&ast["node"])),
        Some("and") => format!(
            "({}) && ({})",
            when_expression(&ast["left"]),
            when_expression(&ast["right"])
        ),
        Some("or") => format!(
            "({}) || ({})",
            when_expression(&ast["left"]),
            when_expression(&ast["right"])
        ),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_nested_when_conditions_while_editing_a_shortcut() {
        let ast = json!({"type":"and","left":{"type":"not","node":{"type":"identifier","name":"terminalFocus"}},"right":{"type":"or","left":{"type":"identifier","name":"isDesktop"},"right":{"type":"identifier","name":"isWeb"}}});
        assert_eq!(
            when_expression(&ast),
            "(!(terminalFocus)) && ((isDesktop) || (isWeb))"
        );
    }
}
