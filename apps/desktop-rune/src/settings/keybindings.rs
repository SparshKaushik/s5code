mod when;

use super::{controls as c, request};
use crate::App;
use crate::icons;
use crate::kit::{Button, Input, Menu, MenuItem, Placement, Popover, Select};
use crate::theme::{self, AlphaMul, T};
use rune::{El, TextEdit, col, el, icon, row, text, text_input};
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use when::{Edit, Op, When};

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

const MAC: bool = cfg!(target_os = "macos");
const NEW: &str = "new";
/// `STATIC_KEYBINDING_COMMANDS` entries with no default binding.
const UNBOUND_COMMANDS: [&str; 5] = [
    "rightPanel.toggleMaximized",
    "composer.cycleHost",
    "composer.increaseEffort",
    "composer.decreaseEffort",
    "thread.stop",
];
/// `usageCommandOrder`: `usage.open`, then the metric and period shortcuts.
const USAGE_ORDER: [&str; 8] = [
    "usage.open",
    "usage.cost",
    "usage.tokens",
    "usage.limits",
    "usage.period.day",
    "usage.period.week",
    "usage.period.month",
    "usage.period.quarter",
];
const USAGE_LABELS: [(&str, &str); 7] = [
    ("usage.cost", "Usage: Cost"),
    ("usage.tokens", "Usage: Tokens"),
    ("usage.limits", "Usage: Limits"),
    ("usage.period.day", "Usage: Period: Past 24h"),
    ("usage.period.week", "Usage: Period: 7 days"),
    ("usage.period.month", "Usage: Period: 30 days"),
    ("usage.period.quarter", "Usage: Period: 90 days"),
];
const US_SHIFTED: [(char, char); 21] = [
    ('!', '1'),
    ('@', '2'),
    ('#', '3'),
    ('$', '4'),
    ('%', '5'),
    ('^', '6'),
    ('&', '7'),
    ('*', '8'),
    ('(', '9'),
    (')', '0'),
    ('~', '`'),
    ('_', '-'),
    ('+', '='),
    ('{', '['),
    ('}', ']'),
    ('|', '\\'),
    (':', ';'),
    ('"', '\''),
    ('<', ','),
    ('>', '.'),
    ('?', '/'),
];
const UNKNOWN_HELP: &str = "T3 Code does not recognize this condition yet. It can still be saved, but it may not match unless the runtime provides it.";

/// The page's editing state; each row's draft lives here, keyed by row id ("new" for the add row).
#[derive(Default)]
pub struct Ui {
    drafts: HashMap<String, Draft>,
    recording: Option<String>,
    recorder: TextEdit,
    when_open: Option<String>,
    expression: TextEdit,
    when_select: Option<String>,
    menu: Option<String>,
    adding: bool,
    command: Option<String>,
    command_open: bool,
    saving: Option<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Draft {
    key: String,
    when: Option<When>,
    invalid: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Shortcut {
    key: String,
    cmd: bool,
    meta: bool,
    ctrl: bool,
    alt: bool,
    shift: bool,
}

impl Shortcut {
    fn from_json(v: &Value) -> Option<Shortcut> {
        let flag = |name: &str| v[name].as_bool() == Some(true);
        Some(Shortcut {
            key: v["key"].as_str()?.to_owned(),
            cmd: flag("modKey"),
            meta: flag("metaKey"),
            ctrl: flag("ctrlKey"),
            alt: flag("altKey"),
            shift: flag("shiftKey"),
        })
    }

    /// `parseKeybindingShortcut`.
    fn parse(value: &str) -> Option<Shortcut> {
        let lower = value.to_lowercase();
        let mut tokens: Vec<&str> = lower.split('+').map(str::trim).collect();
        let mut trailing = 0;
        while tokens.last() == Some(&"") {
            trailing += 1;
            tokens.pop();
        }
        if trailing > 0 {
            tokens.push("+");
        }
        if tokens.is_empty() || tokens.iter().any(|t| t.is_empty()) {
            return None;
        }
        let mut out = Shortcut::default();
        let mut key = None;
        for token in tokens {
            match token {
                "cmd" | "meta" => out.meta = true,
                "ctrl" | "control" => out.ctrl = true,
                "shift" => out.shift = true,
                "alt" | "option" => out.alt = true,
                "mod" => out.cmd = true,
                other => {
                    if key.is_some() {
                        return None;
                    }
                    key = Some(match other {
                        "space" => " ",
                        "esc" => "escape",
                        k => k,
                    }
                    .to_owned());
                }
            }
        }
        out.key = key?;
        Some(out)
    }

    /// `shortcutToKeybindingInput`.
    fn input(&self) -> String {
        let mut parts = Vec::new();
        for (on, name) in [
            (self.cmd, "mod"),
            (self.meta, "meta"),
            (self.ctrl, "ctrl"),
            (self.alt, "alt"),
            (self.shift, "shift"),
        ] {
            if on {
                parts.push(name);
            }
        }
        parts.push(match self.key.as_str() {
            " " => "space",
            "escape" => "esc",
            key => key,
        });
        parts.join("+")
    }

    /// `shortcutConflictKey`: `mod` resolves to Meta on macOS and Ctrl elsewhere.
    fn conflict_key(&self, mac: bool) -> String {
        let flag = |on: bool, name: &'static str| if on { name } else { "" };
        [
            self.key.as_str(),
            flag(self.meta || (self.cmd && mac), "meta"),
            flag(self.ctrl || (self.cmd && !mac), "ctrl"),
            flag(self.shift, "shift"),
            flag(self.alt, "alt"),
        ]
        .join("|")
    }
}

#[derive(Clone, Debug)]
struct Binding {
    command: String,
    shortcut: Shortcut,
    when: Option<When>,
}

impl Binding {
    fn from_json(v: &Value) -> Option<Binding> {
        Some(Binding {
            command: v["command"].as_str()?.to_owned(),
            shortcut: Shortcut::from_json(&v["shortcut"])?,
            when: When::from_json(&v["whenAst"]),
        })
    }
    fn same(&self, other: &Binding) -> bool {
        self.command == other.command && self.shortcut == other.shortcut && self.when == other.when
    }
}

fn defaults() -> &'static [Binding] {
    static DEFAULTS: std::sync::OnceLock<Vec<Binding>> = std::sync::OnceLock::new();
    DEFAULTS.get_or_init(|| {
        let raw: Vec<Value> = serde_json::from_str(include_str!("keybinding-defaults.json")).unwrap_or_default();
        raw.iter()
            .filter_map(|b| {
                Some(Binding {
                    command: b["command"].as_str()?.to_owned(),
                    shortcut: Shortcut::parse(b["key"].as_str()?)?,
                    when: b["when"].as_str().and_then(when::parse),
                })
            })
            .collect()
    })
}

#[derive(Clone, Debug, PartialEq)]
struct Row {
    id: String,
    command: String,
    key: String,
    when: String,
    ast: Option<When>,
    source: &'static str,
    default_key: Option<String>,
    default_when: String,
    conflict: String,
}

impl Row {
    fn seed(&self) -> Draft {
        Draft { key: self.key.clone(), when: self.ast.clone(), invalid: false }
    }
    /// `rowKeybindingTarget`: what `replace` and `server.removeKeybinding` name.
    fn target(&self) -> Value {
        let mut target = json!({"command": self.command, "key": self.key});
        if !self.when.trim().is_empty() {
            target["when"] = json!(self.when);
        }
        target
    }
}

/// `buildKeybindingRows` over `mergeWithDefaultKeybindings(config.keybindings)`.
fn build_rows(config: &[Value], mac: bool) -> Vec<Row> {
    let custom: Vec<Binding> = config.iter().filter_map(Binding::from_json).collect();
    let mut merged: Vec<Binding> =
        defaults().iter().filter(|d| !custom.iter().any(|b| b.command == d.command)).cloned().collect();
    merged.extend(custom);
    let mut rows: Vec<Row> = Vec::new();
    for binding in &merged {
        let key = binding.shortcut.input();
        let when = when::expression(binding.when.as_ref());
        let exact = defaults().iter().find(|d| d.same(binding));
        let fallback = exact
            .or_else(|| defaults().iter().find(|d| d.command == binding.command && d.when == binding.when))
            .or_else(|| defaults().iter().find(|d| d.command == binding.command));
        let source = if binding.command.starts_with("script.") {
            "Project"
        } else if exact.is_some() {
            "Default"
        } else {
            "Custom"
        };
        let next = Row {
            id: format!("{}\0{key}\0{when}", binding.command),
            command: binding.command.clone(),
            key,
            when,
            ast: binding.when.clone(),
            source,
            default_key: fallback.map(|d| d.shortcut.input()),
            default_when: when::expression(fallback.and_then(|d| d.when.as_ref())),
            conflict: binding.shortcut.conflict_key(mac),
        };
        match rows.iter_mut().find(|r| r.id == next.id) {
            Some(slot) => *slot = next,
            None => rows.push(next),
        }
    }
    rows.sort_by(|a, b| {
        compare_commands(&a.command, &b.command, str::to_owned).then_with(|| when::locale_order(&a.key, &b.key))
    });
    rows
}

fn compare_commands(left: &str, right: &str, key: impl Fn(&str) -> String) -> Ordering {
    let rank = |command: &str| USAGE_ORDER.iter().position(|u| *u == command);
    match (rank(left), rank(right)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (a, b) => when::locale_order(
            &key(if a.is_none() { left } else { USAGE_ORDER[1] }),
            &key(if b.is_none() { right } else { USAGE_ORDER[1] }),
        ),
    }
}

/// `buildKeybindingCommandOptions`.
fn command_options(rows: &[Row]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut commands: Vec<String> = UNBOUND_COMMANDS
        .iter()
        .map(|c| c.to_string())
        .chain(defaults().iter().map(|d| d.command.clone()))
        .chain(rows.iter().map(|r| r.command.clone()))
        .filter(|c| seen.insert(c.clone()))
        .collect();
    commands.sort_by(|a, b| compare_commands(a, b, label));
    commands
}

/// `filterKeybindingRows`; `query` is already trimmed and lowercased.
fn matches(r: &Row, query: &str) -> bool {
    query.is_empty()
        || [r.command.clone(), label(&r.command), r.key.clone(), r.when.clone(), r.source.to_owned()]
            .iter()
            .any(|field| field.to_lowercase().contains(query))
}

/// `keybindingConflictLabels`.
fn conflict_labels(rows: &[Row], id: &str, key: &str, when: &str, mac: bool) -> Vec<String> {
    if key.trim().is_empty() {
        return vec![];
    }
    let Some(shortcut) = Shortcut::parse(key) else {
        return vec![];
    };
    let target = shortcut.conflict_key(mac);
    let mut labels: Vec<String> = rows
        .iter()
        .filter(|r| r.id != id && r.conflict == target && (r.when.is_empty() || when.is_empty() || r.when == when))
        .map(|r| label(&r.command))
        .collect();
    labels.sort();
    labels.dedup();
    labels
}

fn conflict_text(labels: &[String]) -> String {
    if labels.len() == 1 {
        return format!("Conflicts with {}.", labels[0]);
    }
    let shown: Vec<&str> = labels.iter().take(3).map(String::as_str).collect();
    let more = if labels.len() > 3 { ", and more" } else { "" };
    format!("Conflicts with {}{more}.", shown.join(", "))
}

fn unknown_text(names: &[String]) -> String {
    if names.len() == 1 {
        format!("Unknown condition: {}", names[0])
    } else {
        format!("Unknown conditions: {}", names.join(", "))
    }
}

fn group_for(command: &str) -> &'static str {
    let prefix = command.split('.').next().unwrap_or("");
    GROUPS
        .iter()
        .find(|(_, prefixes)| prefixes.contains(&prefix))
        .map(|(name, _)| *name)
        .unwrap_or("Other")
}

/// `commandLabel`.
fn label(command: &str) -> String {
    let fixed = match command {
        "workspace.focusNext" => Some("Focus Next Workspace Area"),
        "workspace.focusPrevious" => Some("Focus Previous Workspace Area"),
        "composer.sendAlternate" => Some("Composer: Opposite Queue or Steer Action"),
        "composer.sendBackground" => Some("Composer: Start in Background"),
        "composer.sendAndNewThread" => Some("Composer: Send and Start New Thread"),
        "thread.steerQueuedMessage" => Some("Queue: Send First Queued Message"),
        "thread.editQueuedMessage" => Some("Queue: Edit Last Queued Message"),
        "thread.copyReference" => Some("Pull Request: Copy Link or Thread"),
        "view.reopenClosed" => Some("Reopen Closed Tab"),
        "preview.focusUrl" => Some("Preview: Focus URL"),
        _ => USAGE_LABELS.iter().find(|(c, _)| *c == command).map(|(_, l)| *l),
    };
    if let Some(fixed) = fixed {
        return fixed.to_owned();
    }
    if command.starts_with("script.") && command.ends_with(".run") {
        let id = command.get("script.".len()..command.len().saturating_sub(".run".len())).unwrap_or("");
        return format!("Run Script: {}", title_case(id));
    }
    command.split('.').map(title_case).collect::<Vec<_>>().join(": ")
}

/// `titleCaseCommandSegment`: camelCase, `-`, `_` and spaces become capitalised words.
fn title_case(segment: &str) -> String {
    let mut spaced = String::new();
    let mut previous: Option<char> = None;
    for c in segment.chars() {
        if c.is_ascii_uppercase() && previous.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit()) {
            spaced.push(' ');
        }
        spaced.push(c);
        previous = Some(c);
    }
    spaced
        .split(|c: char| c == '-' || c == '_' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map(|first| first.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `KeybindingPill`'s parts, in the key string's order.
fn pill_parts(value: &str, mac: bool) -> Vec<String> {
    value
        .split('+')
        .map(|part| match part {
            "mod" => (if mac { "⌘" } else { "Ctrl" }).to_owned(),
            "shift" => "⇧".to_owned(),
            "alt" => (if mac { "⌥" } else { "Alt" }).to_owned(),
            "ctrl" => "⌃".to_owned(),
            p if p.chars().count() == 1 => p.to_uppercase(),
            p => p.to_owned(),
        })
        .collect()
}

/// `keybindingFromKeyboardEvent` with `layoutCharacter`; `base` is `Tx::key_base`.
fn key_from_event(e: &rune::KeyEvent, base: Option<char>, mac: bool) -> Option<String> {
    use rune::Key;
    let typed = match &e.key {
        Key::Text(text) => {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Some(c),
                _ => return None,
            }
        }
        Key::Command(c) => Some(*c),
        _ => None,
    };
    let token = match typed {
        Some(c) => {
            let c = base.unwrap_or(c);
            let c = match US_SHIFTED.iter().find(|(shifted, _)| *shifted == c) {
                Some((_, plain)) if e.mods.shift => *plain,
                _ => c,
            };
            if c == ' ' { "space".to_owned() } else { c.to_lowercase().collect() }
        }
        None => match &e.key {
            Key::Enter => "enter".to_owned(),
            Key::Tab => "tab".to_owned(),
            Key::Backspace => "backspace".to_owned(),
            Key::Delete => "delete".to_owned(),
            Key::Home => "home".to_owned(),
            Key::End => "end".to_owned(),
            Key::PageUp => "pageup".to_owned(),
            Key::PageDown => "pagedown".to_owned(),
            Key::Up => "arrowup".to_owned(),
            Key::Down => "arrowdown".to_owned(),
            Key::Left => "arrowleft".to_owned(),
            Key::Right => "arrowright".to_owned(),
            Key::Function(n) => format!("f{n}"),
            _ => return None,
        },
    };
    let m = e.mods;
    let mut parts = Vec::new();
    if mac {
        if m.cmd || m.logo {
            parts.push("mod");
        }
        if m.ctrl {
            parts.push("ctrl");
        }
    } else {
        if m.ctrl || m.cmd {
            parts.push("mod");
        }
        if m.logo {
            parts.push("meta");
        }
    }
    if m.alt {
        parts.push("alt");
    }
    if m.shift {
        parts.push("shift");
    }
    parts.push(&token);
    Some(parts.join("+"))
}

fn rows(s: &App) -> Vec<Row> {
    let config = s.state().config.as_ref();
    build_rows(config.and_then(|c| c["keybindings"].as_array()).map(Vec::as_slice).unwrap_or(&[]), MAC)
}

/// The command being saved while an upsert or remove is in flight.
fn saving(s: &App) -> Option<&str> {
    let busy = s.ui.settings.pending.values().any(|m| m == "server.upsertKeybinding" || m == "server.removeKeybinding");
    busy.then(|| s.ui.settings.keybindings.saving.as_ref().map(|(command, _)| command.as_str())).flatten()
}

fn element_key(id: &str) -> String {
    id.replace('\0', "~")
}

fn variables() -> Vec<String> {
    when::variable_options(defaults().iter().filter_map(|d| d.when.clone()))
}

pub fn view(s: &App, t: &T) -> El {
    let rows = rows(s);
    let busy = saving(s);
    let query = s.ui.settings.keybinding_search.text.trim().to_lowercase();
    let mut body = col().w_full().gap(32.0).child(c::section("Keybindings", vec![toolbar(s, t)], t));
    if s.ui.settings.keybindings.adding {
        body = body.child(c::card(vec![new_row(s, t, &rows, busy)], t));
    }
    let mut any = false;
    for (group, _) in GROUPS {
        let members: Vec<El> = rows
            .iter()
            .filter(|r| group_for(&r.command) == *group && matches(r, &query))
            .map(|r| binding_row(s, t, r, &rows, busy))
            .collect();
        if !members.is_empty() {
            any = true;
            body = body.child(c::section(group, members, t));
        }
    }
    if !any {
        body = body.child(c::card(
            vec![
                el().w_full().px(16.0).py(48.0).center().child(
                    text("No keybindings match your search.")
                        .font(theme::TEXT_SM)
                        .color(t.c.muted_foreground),
                ),
            ],
            t,
        ));
    }
    body
}

fn toolbar(s: &App, t: &T) -> El {
    let config = s.state().config.as_ref();
    let path = config.and_then(|c| c["keybindingsConfigPath"].as_str()).map(str::to_owned);
    let editor = config
        .and_then(|c| c["availableEditors"].as_array())
        .and_then(|v| v.first())
        .and_then(|e| e.as_str().or_else(|| e["id"].as_str()))
        .map(str::to_owned);
    let unavailable = path.is_none() || editor.is_none();
    let search = text_input(&s.ui.settings.keybinding_search, theme::TEXT_SM)
        .single_line()
        .label("Search keybindings")
        .placeholder("Search keybindings", t.c.contrast_placeholder)
        .grow(1.0)
        .min_w(0.0)
        .h(30.0)
        .pr(11.0)
        .py(5.0)
        .color(t.c.contrast_foreground)
        .caret(t.c.contrast_foreground, t.c.primary.alpha_mul(0.3))
        .on_edit(|s: &mut App| &mut s.ui.settings.keybinding_search)
        .on_input_key(|s: &mut App, e, tx| {
            // Web blurs an empty keybinding search before Escape can leave Settings.
            let search = &mut s.ui.settings.keybinding_search;
            if matches!(e.key, rune::Key::Escape) && search.text.is_empty() {
                tx.blur();
                return true;
            }
            super::clear_search_on_escape(search, e)
        });
    let field = row()
        .grow(1.0)
        .min_w(0.0)
        .h(32.0)
        .pl(11.0)
        .gap(3.0)
        .center_y()
        .radius(8.0)
        .border(1.0, t.c.contrast_input)
        .fill(if t.dark { t.c.input.alpha_mul(0.32) } else { t.c.background })
        .child(icon(&icons::SEARCH, 14.0).color(t.c.muted_foreground))
        .child(search);
    let add = Button::new("Add keybinding")
        .variant("outline")
        .icon(&icons::PLUS)
        .disabled(s.ui.settings.keybindings.adding)
        .on_press(|s: &mut App| {
            let ui = &mut s.ui.settings.keybindings;
            ui.drafts.remove(NEW);
            ui.command = None;
            ui.adding = true;
        })
        .build(t);
    let open = Button::new("Open keybindings.json")
        .variant("outline")
        .size("icon")
        .icon(&icons::FILE_BRACES)
        .disabled(unavailable)
        .on_press(move |s: &mut App| {
            if let (Some(path), Some(editor)) = (&path, &editor) {
                request(s, "shell.openInEditor", json!({"cwd": path, "editor": editor}));
            }
        })
        .build(t)
        .help("Open keybindings.json");
    row().w_full().px(16.0).py(12.0).gap(8.0).center_y().child(field).child(add).child(open)
}

/// `SettingsRow`: title and description beside the controls.
fn settings_row(title: El, description: El, controls: El) -> El {
    row()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(32.0)
        .center_y()
        .hover_group()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(row().min_h(20.0).gap(6.0).center_y().child(title))
                .child(description),
        )
        .child(controls)
}

fn when_line(id: &str, name: &str, draft: &Draft, seed: &Draft, t: &T) -> El {
    let expression = when::expression(draft.when.as_ref());
    let fg = if expression.is_empty() { t.c.muted_foreground } else { t.c.foreground };
    let (owned, seed) = (id.to_owned(), seed.clone());
    let trigger = row()
        .role("button")
        .label(format!("Edit when clause for {name}"))
        .focusable(true)
        .shrink()
        .h(20.0)
        .px(5.0)
        .gap(4.0)
        .radius(4.0)
        .center_y()
        .hover(t.c.accent)
        .focus_ring(2.0, -2.0, t.c.ring)
        .child(
            text(if expression.is_empty() { "Always".to_owned() } else { expression })
                .font(theme::mono(theme::weight(theme::TEXT_2XS, theme::MEDIUM)))
                .color(fg)
                .shrink()
                .truncate(),
        )
        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(fg).opacity(0.6))
        .on_press(move |s: &mut App| open_when(s, &owned, &seed));
    row()
        .h(24.0)
        .min_w(0.0)
        .gap(6.0)
        .center_y()
        .child(text("When").font(theme::TEXT_XS).color(t.c.muted_foreground.alpha_mul(0.7)))
        .child(row().key(format!("kb-when-{}-trigger", element_key(id))).shrink().child(trigger))
}

fn binding_row(s: &App, t: &T, r: &Row, rows: &[Row], busy: Option<&str>) -> El {
    let ui = &s.ui.settings.keybindings;
    let seed = r.seed();
    let draft = ui.drafts.get(&r.id).unwrap_or(&seed);
    let name = label(&r.command);
    let when_text = when::expression(draft.when.as_ref());
    let dirty = draft.key != r.key || when_text != r.when;
    let is_saving = busy == Some(r.command.as_str());
    let recording = ui.recording.as_deref() == Some(r.id.as_str()) && ui.recorder.focused;
    let conflicts = conflict_labels(rows, &r.id, &draft.key, &when_text, MAC);
    let title = row()
        .gap(8.0)
        .center_y()
        .help(r.command.clone())
        .child(text(name.clone()).font(theme::SM_MEDIUM).color(t.c.foreground))
        .children((r.source != "Default").then(|| crate::kit::badge(r.source, "outline", "sm", t)));
    let mut controls = row()
        .gap(6.0)
        .center_y()
        .children(conflict_warning(&conflicts, t))
        .children(actions(r, &name, is_saving, ui.menu.as_deref() == Some(r.id.as_str()), t));
    if dirty {
        let (id, command, replace) = (r.id.clone(), r.command.clone(), r.target());
        controls = controls.child(
            Button::new(if is_saving { "Saving" } else { "Save" })
                .size("sm")
                .disabled(is_saving || draft.key.trim().is_empty() || draft.invalid)
                .on_press(move |s: &mut App| {
                    let Some(d) = s.ui.settings.keybindings.drafts.get(&id) else {
                        return;
                    };
                    let (key, when) = (d.key.clone(), when::expression(d.when.as_ref()));
                    upsert(s, &id, &command, &key, &when, Some(replace.clone()));
                })
                .build(t),
        );
    }
    controls = if !recording && !dirty && !r.key.is_empty() {
        controls.child(pill(r, &name, t))
    } else {
        controls.child(key_field(s, t, &r.id, &r.key, &draft.key, &seed, &format!("Keybinding for {name}")))
    };
    settings_row(title, when_line(&r.id, &name, draft, &seed, t), controls)
}

fn new_row(s: &App, t: &T, rows: &[Row], busy: Option<&str>) -> El {
    let ui = &s.ui.settings.keybindings;
    let seed = Draft::default();
    let draft = ui.drafts.get(NEW).unwrap_or(&seed);
    let name = ui.command.as_deref().map(label).unwrap_or_else(|| "new keybinding".to_owned());
    let when_text = when::expression(draft.when.as_ref());
    let conflicts = conflict_labels(rows, NEW, &draft.key, &when_text, MAC);
    let can_save = ui.command.is_some() && !draft.key.trim().is_empty() && !draft.invalid;
    let controls = row()
        .gap(8.0)
        .center_y()
        .child(command_select(s, rows).trigger(&crate::kit::theme(t)))
        .children(conflict_warning(&conflicts, t))
        .child(key_field(s, t, NEW, "", &draft.key, &seed, &format!("Keybinding for {name}")))
        .child(
            Button::new(if busy.is_some() { "Saving" } else { "Save" })
                .size("sm")
                .disabled(busy.is_some() || !can_save)
                .on_press(save_new)
                .build(t),
        )
        .child(
            Button::new("Cancel new keybinding")
                .variant("ghost-muted")
                .size("icon-sm")
                .icon(&icons::X)
                .disabled(busy.is_some())
                .on_press(|s: &mut App| close_add(&mut s.ui.settings.keybindings))
                .build(t)
                .help("Cancel"),
        );
    let title = text("New keybinding").font(theme::SM_MEDIUM).color(t.c.foreground);
    settings_row(title, when_line(NEW, &name, draft, &seed, t), controls)
        .radius(11.0)
        .fill(t.c.muted.alpha_mul(0.15))
}

fn command_select(s: &App, rows: &[Row]) -> Select {
    let commands = command_options(rows);
    let labels: Vec<String> = commands.iter().map(|c| label(c)).collect();
    let current = s.ui.settings.keybindings.command.as_deref().map(label);
    Select::new("kb-command", &labels)
        .current(current.as_deref())
        .placeholder("Command")
        .label("Command")
        .size("sm")
        .width(224.0)
        .on_toggle(|s: &mut App| {
            let ui = &mut s.ui.settings.keybindings;
            ui.command_open = !ui.command_open;
        })
        .on_choose(move |s: &mut App, value: &str, _: &mut rune::Tx| {
            let ui = &mut s.ui.settings.keybindings;
            if let Some(command) = commands.iter().find(|c| label(c) == value) {
                ui.command = Some(command.clone());
            }
            ui.command_open = false;
        })
        .on_close(|s: &mut App| s.ui.settings.keybindings.command_open = false)
}

fn pill(r: &Row, name: &str, t: &T) -> El {
    let parts = pill_parts(&r.key, MAC);
    let (id, seed) = (r.id.clone(), r.seed());
    row()
        .role("button")
        .label(format!("Edit shortcut for {name}: {}", parts.concat()))
        .focusable(true)
        .h(28.0)
        .px(6.0)
        .mr(-6.0)
        .gap(4.0)
        .radius(6.0)
        .center_y()
        .hover(t.c.accent)
        .focus_ring(3.0, -3.0, t.c.ring.alpha_mul(0.24))
        .children(parts.iter().map(|p| crate::kit::kbd(p, t)))
        .on_press(move |s: &mut App| start_recording(s, &id, &seed))
}

/// Web's capture `Input`: focused it records the next chord; otherwise it shows the draft.
fn key_field(s: &App, t: &T, id: &str, reset: &str, key: &str, seed: &Draft, aria: &str) -> El {
    let ui = &s.ui.settings.keybindings;
    if ui.recording.as_deref() == Some(id) && ui.recorder.focused {
        let (id, reset) = (id.to_owned(), reset.to_owned());
        return Input::new(&ui.recorder)
            .placeholder("Press shortcut")
            .size("sm")
            .font("mono")
            .on_edit(|s: &mut App| &mut s.ui.settings.keybindings.recorder)
            .build(t)
            .single_line()
            .label(aria.to_owned())
            .w(176.0)
            .on_input_key(move |s: &mut App, e, tx| record(s, &id, &reset, e, tx));
    }
    let (id, seed) = (id.to_owned(), seed.clone());
    el().w(176.0)
        .role("button")
        .label(aria.to_owned())
        .focusable(true)
        .radius(8.0)
        .focus_ring(3.0, -3.0, t.c.ring.alpha_mul(0.24))
        .child(
            Input::new(&TextEdit::new(key))
                .placeholder("Unassigned")
                .size("sm")
                .font("mono")
                .build(t)
                .inert(),
        )
        .on_press(move |s: &mut App| start_recording(s, &id, &seed))
}

fn conflict_warning(labels: &[String], t: &T) -> Option<El> {
    if labels.is_empty() {
        return None;
    }
    let description = conflict_text(labels);
    let help = format!("{description} The most recent matching binding wins when both conditions can apply.");
    Some(warning_icon(description, help, 20.0, t))
}

fn warning_icon(label: String, help: String, size: f32, t: &T) -> El {
    el().size(size, size)
        .center()
        .radius(4.0)
        .focusable(true)
        .label(label)
        .help(help)
        .hover(t.c.warning.alpha_mul(0.1))
        .child(icon(&icons::TRIANGLE_ALERT, 14.0).color(t.c.warning))
}

/// `KeybindingHoverRowMenu`'s trigger: shown on row hover and while its menu is open.
fn actions(r: &Row, name: &str, is_saving: bool, open: bool, t: &T) -> Option<El> {
    let (can_reset, can_remove) = (r.source == "Custom" && r.default_key.is_some(), r.source != "Default");
    if !can_reset && !can_remove {
        return None;
    }
    let id = r.id.clone();
    Some(
        row()
            .key(format!("kb-actions-{}", element_key(&r.id)))
            .center_y()
            .group_opacity(if open { 1.0 } else { 0.0 }, 1.0)
            .child(
                Button::new(format!("Actions for {name}"))
                    .variant("ghost-muted")
                    .size("icon-sm")
                    .icon(&icons::ELLIPSIS)
                    .disabled(is_saving)
                    .on_press(move |s: &mut App| s.ui.settings.keybindings.menu = Some(id.clone()))
                    .build(t),
            ),
    )
}

fn start_recording(s: &mut App, id: &str, seed: &Draft) {
    let ui = &mut s.ui.settings.keybindings;
    ui.drafts.entry(id.to_owned()).or_insert_with(|| seed.clone());
    ui.recording = Some(id.to_owned());
    ui.recorder = TextEdit::new("");
    ui.recorder.focused = true;
}

/// Every key goes to the recorder; Escape restores `reset` without leaving Settings.
fn record(s: &mut App, id: &str, reset: &str, e: &rune::KeyEvent, tx: &mut rune::Tx) -> bool {
    let next = if matches!(e.key, rune::Key::Escape) {
        Some(reset.to_owned())
    } else {
        key_from_event(e, tx.key_base(), MAC)
    };
    if let Some(next) = next {
        let ui = &mut s.ui.settings.keybindings;
        if let Some(draft) = ui.drafts.get_mut(id) {
            draft.key = next;
        }
        ui.recording = None;
        tx.blur();
    }
    true
}

fn upsert(s: &mut App, id: &str, command: &str, key: &str, when: &str, replace: Option<Value>) {
    let mut payload = json!({"command": command, "key": key.trim()});
    if !when.trim().is_empty() {
        payload["when"] = json!(when.trim());
    }
    if let Some(replace) = replace {
        payload["replace"] = replace;
    }
    s.ui.settings.keybindings.saving = Some((command.to_owned(), id.to_owned()));
    request(s, "server.upsertKeybinding", payload);
}

fn save_new(s: &mut App) {
    let ui = &s.ui.settings.keybindings;
    let (Some(command), Some(draft)) = (ui.command.clone(), ui.drafts.get(NEW)) else {
        return;
    };
    let (key, when) = (draft.key.clone(), when::expression(draft.when.as_ref()));
    upsert(s, NEW, &command, &key, &when, None);
}

fn reset(s: &mut App, r: &Row) {
    if let Some(key) = &r.default_key {
        upsert(s, &r.id, &r.command, key, &r.default_when, Some(r.target()));
    }
}

fn remove(s: &mut App, r: &Row) {
    s.ui.settings.keybindings.saving = Some((r.command.clone(), r.id.clone()));
    request(s, "server.removeKeybinding", r.target());
}

fn close_add(ui: &mut Ui) {
    ui.adding = false;
    ui.command = None;
    ui.command_open = false;
    ui.drafts.remove(NEW);
    if ui.when_open.as_deref() == Some(NEW) {
        ui.when_open = None;
        ui.when_select = None;
    }
    if ui.recording.as_deref() == Some(NEW) {
        ui.recording = None;
    }
}

fn close_menu(s: &mut App) {
    s.ui.settings.keybindings.menu = None;
}

/// A successful upsert or remove: the saved row's draft is spent, and an upsert closes the add row.
pub fn saved(s: &mut App, upsert: bool) {
    let ui = &mut s.ui.settings.keybindings;
    if let Some((_, id)) = ui.saving.take() {
        ui.drafts.remove(&id);
    }
    if upsert {
        close_add(ui);
    }
}

/// Escape closes the innermost keybinding overlay first.
pub fn escape(s: &mut App) -> bool {
    let ui = &mut s.ui.settings.keybindings;
    if ui.when_select.take().is_some() || ui.menu.take().is_some() {
        return true;
    }
    if ui.command_open {
        ui.command_open = false;
        return true;
    }
    if ui.when_open.is_some() {
        close_when(s);
        return true;
    }
    false
}

/// The add row with its condition editor open on `!terminalFocus`.
pub fn scene_editor(s: &mut App) {
    let ui = &mut s.ui.settings.keybindings;
    ui.adding = true;
    ui.command = Some("terminal.toggle".into());
    ui.drafts.insert(NEW.into(), Draft { key: String::new(), when: when::parse("!terminalFocus"), invalid: false });
    open_when(s, NEW, &Draft::default());
}

fn open_when(s: &mut App, id: &str, seed: &Draft) {
    let ui = &mut s.ui.settings.keybindings;
    let draft = ui.drafts.entry(id.to_owned()).or_insert_with(|| seed.clone());
    draft.invalid = false;
    let text = when::expression(draft.when.as_ref());
    ui.expression = TextEdit::new(text);
    ui.when_open = Some(id.to_owned());
    ui.when_select = None;
}

/// Closing keeps the last valid condition and drops an unparsed expression.
fn close_when(s: &mut App) {
    let ui = &mut s.ui.settings.keybindings;
    if let Some(id) = ui.when_open.take()
        && let Some(draft) = ui.drafts.get_mut(&id)
    {
        draft.invalid = false;
    }
    ui.when_select = None;
}

fn set_when(s: &mut App, id: &str, next: Option<When>) {
    let ui = &mut s.ui.settings.keybindings;
    ui.expression.replace_text(when::expression(next.as_ref()));
    ui.when_select = None;
    if let Some(draft) = ui.drafts.get_mut(id) {
        draft.when = next;
        draft.invalid = false;
    }
}

fn apply(s: &mut App, id: &str, path: &[usize], change: &Edit) {
    let Some(root) = s.ui.settings.keybindings.drafts.get(id).and_then(|d| d.when.as_ref()) else {
        return;
    };
    let next = when::edit(root, path, change);
    set_when(s, id, next);
}

fn add_root(s: &mut App, id: &str, as_group: bool) {
    let current = s.ui.settings.keybindings.drafts.get(id).and_then(|d| d.when.clone());
    set_when(s, id, Some(when::add_root(current.as_ref(), as_group)));
}

fn expression_changed(s: &mut App, id: &str) {
    let ui = &mut s.ui.settings.keybindings;
    let parsed = when::parse_draft(&ui.expression.text);
    if let Some(draft) = ui.drafts.get_mut(id) {
        match parsed {
            Ok(next) => {
                draft.when = next;
                draft.invalid = false;
            }
            Err(_) => draft.invalid = true,
        }
    }
}

fn toggle_select(s: &mut App, key: &str) {
    let open = &mut s.ui.settings.keybindings.when_select;
    *open = if open.as_deref() == Some(key) { None } else { Some(key.to_owned()) };
}

/// The When popover, the row actions menu and the new row's command list.
pub fn overlays(s: &App, t: &T, layer: &mut Vec<El>) {
    let ui = &s.ui.settings.keybindings;
    if ui.when_open.is_none() && ui.menu.is_none() && !ui.command_open {
        return;
    }
    let k = crate::kit::theme(t);
    let rows = rows(s);
    if ui.command_open && ui.adding {
        let _ = command_select(s, &rows).open(true).build(&k, layer);
    }
    if let Some(id) = ui.menu.as_deref()
        && let Some(r) = rows.iter().find(|r| r.id == id)
    {
        let is_saving = saving(s) == Some(r.command.as_str());
        let mut menu = Menu::new("kb-row-menu")
            .anchor(format!("kb-actions-{}", element_key(id)))
            .placement(Placement::BottomEnd)
            .gutter(4.0)
            .open(true)
            .on_close(close_menu);
        if r.source == "Custom" && r.default_key.is_some() {
            let target = r.clone();
            menu = menu.item(MenuItem::new("Reset to default").disabled(is_saving).on_select(move |s: &mut App| {
                close_menu(s);
                reset(s, &target);
            }));
        }
        if r.source != "Default" {
            let target = r.clone();
            menu = menu.item(MenuItem::new("Remove").variant("destructive").disabled(is_saving).on_select(
                move |s: &mut App| {
                    close_menu(s);
                    remove(s, &target);
                },
            ));
        }
        menu.overlay(&k, layer);
    }
    if let Some(id) = ui.when_open.as_deref() {
        let (seed, name) = if id == NEW {
            (Draft::default(), ui.command.as_deref().map(label).unwrap_or_else(|| "new keybinding".to_owned()))
        } else if let Some(r) = rows.iter().find(|r| r.id == id) {
            (r.seed(), label(&r.command))
        } else {
            return;
        };
        when_popover(s, t, &k, id, &name, &seed, layer);
    }
}

/// `WhenExpressionBuilder` in its popover; select menus inside it go above it.
fn when_popover(s: &App, t: &T, k: &rune_kit::Kit, id: &str, name: &str, seed: &Draft, layer: &mut Vec<El>) {
    let ui = &s.ui.settings.keybindings;
    let draft = ui.drafts.get(id).unwrap_or(seed);
    let variables = variables();
    let parsed = when::parse_draft(&ui.expression.text);
    let unknown = match &parsed {
        Ok(value) => when::unknown(value.as_ref(), &variables),
        Err(_) => vec![],
    };
    let add = |label: &str, variant: &str, as_group: bool| {
        let owned = id.to_owned();
        Button::new(label)
            .variant(variant)
            .size("compact")
            .icon(&icons::PLUS)
            .on_press(move |s: &mut App| add_root(s, &owned, as_group))
            .build(t)
    };
    let owned = id.to_owned();
    let input = Input::new(&ui.expression)
        .placeholder("Always")
        .size("compact")
        .font("mono")
        .invalid(parsed.is_err())
        .on_edit(|s: &mut App| &mut s.ui.settings.keybindings.expression)
        .on_change(move |s: &mut App, _: &mut rune::Tx| expression_changed(s, &owned))
        .build(t)
        .single_line()
        .label("When expression");
    let field = if unknown.is_empty() {
        col().w_full().child(input)
    } else {
        col().w_full().child(input.pr(30.0)).child(
            warning_icon(unknown_text(&unknown), UNKNOWN_HELP.to_owned(), 18.0, t).pin(None, Some(5.0), Some(6.0), None),
        )
    };
    let error = parsed.is_err().then(|| {
        row()
            .gap(6.0)
            .center_y()
            .child(icon(&icons::CIRCLE_X, 14.0).color(t.c.destructive))
            .child(text(when::PARSE_ERROR).font(theme::TEXT_2XS).color(t.c.destructive))
    });
    let mut builder = Builder { t, k, id: id.to_owned(), variables: &variables, open: ui.when_select.as_deref(), sub: vec![] };
    let tree = match draft.when.as_ref() {
        Some(root) => builder.node(root, vec![], 0, true, 544.0),
        None => col()
            .w_full()
            .padding(12.0)
            .radius(6.0)
            .border(1.0, t.c.border.alpha_mul(0.8))
            .border_dash(4.0, 3.0)
            .fill(t.c.muted.alpha_mul(0.15))
            .child(row().gap(8.0).flex_wrap().child(add("Condition", "default", false)).child(add("Group", "outline", true))),
    };
    let mut area = col().w_full().child(tree);
    if parsed.is_err() {
        area = area.child(
            col()
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .center()
                .padding(16.0)
                .radius(8.0)
                .border(1.0, t.c.destructive.alpha_mul(0.3))
                .fill(t.c.background.alpha_mul(0.75))
                .inert()
                .child(
                    text("Fix the expression above to continue editing visually.")
                        .font(theme::TEXT_XS)
                        .color(t.c.destructive)
                        .align(rune::Align::Center)
                        .wrap(),
                ),
        );
    }
    let content = col()
        .w(544.0)
        .gap(12.0)
        .child(
            row()
                .w_full()
                .gap(12.0)
                .items_start()
                .child(text("When").font(theme::SM_MEDIUM).color(t.c.foreground).grow(1.0))
                .child(row().gap(8.0).child(add("Condition", "outline", false)).child(add("Group", "outline", true))),
        )
        .child(col().w_full().gap(6.0).child(field).children(error))
        .child(area);
    let sub = std::mem::take(&mut builder.sub);
    let _ = Popover::new(format!("kb-when-{}", element_key(id)))
        .open(true)
        .placement(Placement::BottomStart)
        .gutter(6.0)
        .padding(16.0)
        .label(format!("Edit when clause for {name}"))
        .child(content)
        .on_close(close_when)
        .build(k, layer);
    layer.extend(sub);
}

/// `WhenExpressionNodeEditor`; `path` addresses the node for `when::edit`, `avail` is its width.
struct Builder<'a> {
    t: &'a T,
    k: &'a rune_kit::Kit,
    id: String,
    variables: &'a [String],
    open: Option<&'a str>,
    sub: Vec<El>,
}

impl Builder<'_> {
    fn node(&mut self, node: &When, path: Vec<usize>, depth: usize, removable: bool, avail: f32) -> El {
        let t = self.t;
        if let Some((name, negated)) = when::condition(node) {
            let unknown = !self.variables.iter().any(|v| v == name);
            let inner = avail - 2.0 - 16.0;
            let width = inner - 48.0 - (if removable { 36.0 } else { 0.0 }) - (if unknown { 26.0 } else { 0.0 });
            let mut options = self.variables.to_vec();
            if unknown {
                options.insert(0, name.to_owned());
            }
            let key = format!("kb-var-{}", path_key(&path));
            let (toggle_key, choose_id, choose_path) = (key.clone(), self.id.clone(), path.clone());
            let select = Select::new(key.clone(), &options)
                .current(Some(name))
                .placeholder("Condition")
                .label("Condition")
                .size("compact")
                .width(width.max(80.0))
                .open(self.open == Some(key.as_str()))
                .on_toggle(move |s: &mut App| toggle_select(s, &toggle_key))
                .on_choose(move |s: &mut App, value: &str, _: &mut rune::Tx| {
                    apply(s, &choose_id, &choose_path, &Edit::Variable(value.to_owned()))
                })
                .on_close(|s: &mut App| s.ui.settings.keybindings.when_select = None)
                .build(self.k, &mut self.sub);
            let (id, negate_path) = (self.id.clone(), path.clone());
            return row()
                .w_full()
                .gap(8.0)
                .center_y()
                .px(8.0)
                .py(8.0)
                .radius(6.0)
                .border(1.0, t.c.border.alpha_mul(0.7))
                .fill(t.c.background.alpha_mul(0.6))
                .child(
                    not_toggle(negated, format!("Negate {name}"), t)
                        .on_press(move |s: &mut App| apply(s, &id, &negate_path, &Edit::Negate(!negated))),
                )
                .child(select)
                .children(unknown.then(|| warning_icon(unknown_text(&[name.to_owned()]), UNKNOWN_HELP.to_owned(), 18.0, t)))
                .children(removable.then(|| self.remove(node, &path, depth)));
        }
        let child_avail = avail - 2.0 - 16.0 - 16.0;
        if let When::Not(inner) = node {
            let (fill, border) = if depth == 0 {
                (t.c.muted.alpha_mul(0.2), t.c.border.alpha_mul(0.7))
            } else {
                (t.c.background.alpha_mul(0.5), t.c.border.alpha_mul(0.5))
            };
            let mut body_path = path.clone();
            body_path.push(0);
            let body = self.node(inner, body_path, depth + 1, false, child_avail);
            let (id, toggle_path) = (self.id.clone(), path.clone());
            let header = row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    not_toggle(true, "Negate group".to_owned(), t)
                        .on_press(move |s: &mut App| apply(s, &id, &toggle_path, &Edit::Negate(false))),
                )
                .child(el().grow(1.0))
                .children(removable.then(|| self.remove(node, &path, depth)));
            return col()
                .w_full()
                .gap(8.0)
                .padding(8.0)
                .radius(8.0)
                .border(1.0, border)
                .fill(fill)
                .child(header)
                .child(indent(body, t.c.border.alpha_mul(0.7)));
        }
        let op = node.op().unwrap_or(Op::And);
        let (fill, border, line) = if depth == 0 {
            (t.c.muted.alpha_mul(0.1), t.c.border.alpha_mul(0.6), t.c.border)
        } else {
            (t.c.background.alpha_mul(0.55), t.c.border.alpha_mul(0.7), t.c.border.alpha_mul(0.7))
        };
        let key = format!("kb-op-{}", path_key(&path));
        let (toggle_key, choose_id, choose_path) = (key.clone(), self.id.clone(), path.clone());
        let operator = Select::new(key.clone(), &["and", "or"])
            .current(Some(op.name()))
            .label("Operator")
            .size("compact")
            .width(96.0)
            .open(self.open == Some(key.as_str()))
            .on_toggle(move |s: &mut App| toggle_select(s, &toggle_key))
            .on_choose(move |s: &mut App, value: &str, _: &mut rune::Tx| {
                let next = if value == "or" { Op::Or } else { Op::And };
                apply(s, &choose_id, &choose_path, &Edit::Operator(next))
            })
            .on_close(|s: &mut App| s.ui.settings.keybindings.when_select = None)
            .build(self.k, &mut self.sub);
        let add = |label: &str, change: Edit| {
            let (id, add_path) = (self.id.clone(), path.clone());
            Button::new(label)
                .variant("outline")
                .size("compact")
                .icon(&icons::PLUS)
                .on_press(move |s: &mut App| apply(s, &id, &add_path, &change))
                .build(t)
        };
        let header = row()
            .w_full()
            .gap(8.0)
            .center_y()
            .flex_wrap()
            .child(operator)
            .child(add("Condition", Edit::AddCondition))
            .child(add("Group", Edit::AddGroup))
            .child(el().grow(1.0))
            .children(removable.then(|| self.remove(node, &path, depth)));
        let mut children = col().w_full().gap(8.0);
        for (i, child) in when::flatten(node, op).iter().enumerate() {
            let mut child_path = path.clone();
            child_path.push(i);
            let editor = self.node(child, child_path, depth + 1, true, child_avail);
            children = children.child(indent(editor, line));
        }
        col()
            .w_full()
            .gap(8.0)
            .padding(8.0)
            .radius(8.0)
            .border(1.0, border)
            .fill(fill)
            .child(header)
            .child(children)
    }

    fn remove(&self, node: &When, path: &[usize], depth: usize) -> El {
        let label = when::remove_label(node, depth);
        let (id, path) = (self.id.clone(), path.to_vec());
        Button::new(label)
            .variant("ghost")
            .size("icon-sm")
            .icon(&icons::MINUS)
            .on_press(move |s: &mut App| apply(s, &id, &path, &Edit::Remove))
            .build(self.t)
            .help(label)
    }
}

fn path_key(path: &[usize]) -> String {
    path.iter().fold("r".to_owned(), |key, i| format!("{key}-{i}"))
}

/// The tree's connector: a rail down the left and a stub into the child.
fn indent(body: El, line: rune::Color) -> El {
    col()
        .w_full()
        .pl(16.0)
        .child(el().pin(Some(6.0), Some(0.0), None, Some(0.0)).w(1.0).fill(line))
        .child(el().absolute(6.0, 16.0).w(10.0).h(1.0).fill(line))
        .child(body)
}

/// Web's outline compact `Toggle` labelled "Not".
fn not_toggle(pressed: bool, label: String, t: &T) -> El {
    let fill = match (pressed, t.dark) {
        (true, true) => t.c.input,
        (true, false) => t.c.input.alpha_mul(0.64),
        (false, true) => t.c.input.alpha_mul(0.32),
        (false, false) => t.c.background,
    };
    row()
        .role("button")
        .label(label)
        .checked(pressed)
        .focusable(true)
        .h(28.0)
        .min_w(40.0)
        .px(3.0)
        .center()
        .radius(6.0)
        .border(1.0, t.c.input)
        .fill(fill)
        .hover(if t.dark { t.c.input.alpha_mul(0.64) } else { t.c.accent })
        .focus_ring(2.0, -2.0, t.c.ring)
        .child(text("Not").font(theme::XS_MEDIUM).color(t.c.foreground))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Key, KeyEvent, Mods};

    fn ast(expression: &str) -> When {
        when::parse(expression).unwrap()
    }

    #[test]
    fn preserves_nested_when_conditions_while_editing_a_shortcut() {
        let value = json!({"type":"and","left":{"type":"not","node":{"type":"identifier","name":"terminalFocus"}},"right":{"type":"or","left":{"type":"identifier","name":"isDesktop"},"right":{"type":"identifier","name":"isWeb"}}});
        let node = When::from_json(&value);
        assert_eq!(when::expression(node.as_ref()), "!terminalFocus && (isDesktop || isWeb)");
        let negated_group = json!({"type":"not","node":{"type":"and","left":{"type":"identifier","name":"a"},"right":{"type":"identifier","name":"b"}}});
        assert_eq!(when::expression(When::from_json(&negated_group).as_ref()), "!(a && b)");
    }

    #[test]
    fn parses_when_expressions_with_precedence() {
        assert_eq!(when::expression(Some(&ast("a || b && !c"))), "a || (b && !c)");
        assert_eq!(when::expression(Some(&ast("!(a || b)"))), "!(a || b)");
        assert_eq!(when::expression(Some(&ast("  rightPanel.open && !x-y_z "))), "rightPanel.open && !x-y_z");
        for invalid in ["", "a &&", "(a", "a b", "a & b", "a)", "!", "1a"] {
            assert_eq!(when::parse(invalid), None, "{invalid}");
        }
        assert_eq!(when::parse_draft("   "), Ok(None));
        assert_eq!(when::parse_draft("a ||"), Err(when::PARSE_ERROR));
        assert_eq!(when::parse_draft(" a "), Ok(Some(When::id("a"))));
    }

    #[test]
    fn edits_the_condition_tree_like_the_web_builder() {
        let root = ast("a && !b && (c || d)");
        let edit = |path: &[usize], change: Edit| when::edit(&root, path, &change);
        assert_eq!(edit(&[1], Edit::Negate(false)), Some(ast("a && b && (c || d)")));
        assert_eq!(edit(&[0], Edit::Negate(true)), Some(ast("!a && !b && (c || d)")));
        assert_eq!(edit(&[0], Edit::Variable("x".into())), Some(ast("x && !b && (c || d)")));
        assert_eq!(edit(&[1], Edit::Variable("x".into())), Some(ast("a && !x && (c || d)")));
        assert_eq!(edit(&[2], Edit::Operator(Op::And)), Some(ast("a && !b && (c && d)")));
        assert_eq!(edit(&[], Edit::AddCondition), Some(ast("a && !b && (c || d) && terminalFocus")));
        assert_eq!(
            edit(&[2], Edit::AddGroup),
            Some(ast("a && !b && (c || d || (terminalFocus && !terminalFocus))"))
        );
        assert_eq!(edit(&[1], Edit::Remove), Some(ast("a && (c || d)")));
        assert_eq!(edit(&[2, 0], Edit::Remove), Some(ast("a && !b && d")));
        assert_eq!(edit(&[], Edit::Remove), None);
        assert_eq!(when::edit(&ast("a && b"), &[0], &Edit::Remove), Some(ast("b")));

        let negated = ast("!(a || b)");
        assert_eq!(when::edit(&negated, &[], &Edit::Negate(false)), Some(ast("a || b")));
        assert_eq!(when::edit(&negated, &[0, 1], &Edit::Variable("z".into())), Some(ast("!(a || z)")));
        assert_eq!(when::edit(&ast("a || b"), &[], &Edit::Negate(true)), Some(ast("!(a || b)")));
    }

    #[test]
    fn adds_root_conditions_and_labels_removals() {
        assert_eq!(when::add_root(None, false), ast("terminalFocus"));
        assert_eq!(when::add_root(None, true), ast("terminalFocus || !terminalFocus"));
        assert_eq!(when::add_root(Some(&ast("a")), true), ast("a && (terminalFocus || !terminalFocus)"));
        assert_eq!(when::remove_label(&ast("a && b"), 0), "Clear all conditions");
        assert_eq!(when::remove_label(&ast("!a"), 1), "Remove condition");
        assert_eq!(when::remove_label(&ast("!(a || b)"), 1), "Remove group and its conditions");
    }

    #[test]
    fn orders_variables_and_reports_unknown_ones() {
        let options = when::variable_options([ast("zeta && Alpha && terminalFocus")]);
        assert_eq!(
            options,
            ["terminalFocus", "terminalOpen", "isWeb", "isDesktop", "true", "false", "Alpha", "zeta"]
        );
        assert_eq!(when::unknown(Some(&ast("b && a && isWeb")), &options), ["a", "b"]);
        assert_eq!(unknown_text(&["a".into()]), "Unknown condition: a");
        assert_eq!(unknown_text(&["a".into(), "b".into()]), "Unknown conditions: a, b");
    }

    #[test]
    fn parses_and_formats_shortcuts() {
        let input = |value: &str| Shortcut::parse(value).map(|s| s.input());
        assert_eq!(input("shift+Mod+K").as_deref(), Some("mod+shift+k"));
        assert_eq!(input("ctrl++").as_deref(), Some("ctrl++"));
        assert_eq!(input("space").as_deref(), Some("space"));
        assert_eq!(input("esc").as_deref(), Some("esc"));
        assert_eq!(input("mod+a+b"), None);
        assert_eq!(input("mod++a"), None);
        let conflict = |value: &str, mac: bool| Shortcut::parse(value).unwrap().conflict_key(mac);
        assert_eq!(conflict("mod+k", true), conflict("meta+k", true));
        assert_ne!(conflict("mod+k", true), conflict("ctrl+k", true));
        assert_eq!(conflict("mod+k", false), conflict("ctrl+k", false));
        assert_eq!(pill_parts("mod+shift+f", true), ["⌘", "⇧", "F"]);
        assert_eq!(pill_parts("alt+ctrl+arrowup", false), ["Alt", "⌃", "arrowup"]);
        assert_eq!(pill_parts("mod+k", false), ["Ctrl", "K"]);
    }

    #[test]
    fn records_chords_in_web_order() {
        let event = |key: Key, mods: Mods| KeyEvent { key, mods };
        let cmd_shift = Mods { cmd: true, shift: true, ..Default::default() };
        assert_eq!(key_from_event(&event(Key::Command('k'), cmd_shift), None, true).as_deref(), Some("mod+shift+k"));
        let ctrl = Mods { cmd: true, ctrl: true, ..Default::default() };
        assert_eq!(key_from_event(&event(Key::Command('k'), ctrl), None, false).as_deref(), Some("mod+k"));
        assert_eq!(key_from_event(&event(Key::Command('k'), ctrl), None, true).as_deref(), Some("mod+ctrl+k"));
        let logo = Mods { logo: true, alt: true, ..Default::default() };
        assert_eq!(key_from_event(&event(Key::Text("x".into()), logo), None, false).as_deref(), Some("meta+alt+x"));
        let shift = Mods { shift: true, ..Default::default() };
        assert_eq!(key_from_event(&event(Key::Text("!".into()), shift), None, false).as_deref(), Some("shift+1"));
        let alt = Mods { alt: true, ..Default::default() };
        assert_eq!(key_from_event(&event(Key::Text("å".into()), alt), Some('a'), true).as_deref(), Some("alt+a"));
        let none = Mods::default();
        assert_eq!(key_from_event(&event(Key::Up, none), None, true).as_deref(), Some("arrowup"));
        assert_eq!(key_from_event(&event(Key::Function(5), none), None, true).as_deref(), Some("f5"));
        assert_eq!(key_from_event(&event(Key::Text(" ".into()), none), None, true).as_deref(), Some("space"));
        assert_eq!(key_from_event(&event(Key::Text("ab".into()), none), None, true), None);
    }

    #[test]
    fn labels_commands_like_web() {
        assert_eq!(label("usage.cost"), "Usage: Cost");
        assert_eq!(label("usage.period.week"), "Usage: Period: 7 days");
        assert_eq!(label("usage.open"), "Usage: Open");
        assert_eq!(label("script.build-app.run"), "Run Script: Build App");
        assert_eq!(label("script.run"), "Run Script: ");
        assert_eq!(label("rightPanel.toggleMaximized"), "Right Panel: Toggle Maximized");
        assert_eq!(label("thread.jump.1"), "Thread: Jump: 1");
        assert_eq!(label("preview.focusUrl"), "Preview: Focus URL");
        assert_eq!(title_case("openAI2Go"), "Open AI2 Go");
    }

    #[test]
    fn builds_sorted_rows_with_sources_and_conflicts() {
        let binding = |command: &str, key: Value, when: Option<&str>| {
            let mut rule = json!({"command": command, "shortcut": key});
            if let Some(when) = when {
                rule["whenAst"] = json!({"type": "identifier", "name": when});
            }
            rule
        };
        let shortcut = |key: &str, cmd: bool, shift: bool| {
            json!({"key": key, "modKey": cmd, "metaKey": false, "ctrlKey": false, "altKey": false, "shiftKey": shift})
        };
        let config = [
            binding("usage.period.day", shortcut("d", true, false), None),
            binding("sidebar.toggle", shortcut("b", true, true), None),
            binding("script.test.run", shortcut("f19", true, false), Some("terminalFocus")),
            binding("usage.open", shortcut("u", true, false), None),
            binding("chat.new", shortcut("f19", true, false), None),
        ];
        let rows = build_rows(&config, true);
        let commands: Vec<&str> = rows.iter().map(|r| r.command.as_str()).collect();
        let position = |command: &str| commands.iter().position(|c| *c == command).unwrap();
        assert!(position("chat.new") < position("usage.open"));
        assert!(position("usage.open") < position("usage.period.day"));
        assert!(position("script.test.run") < position("sidebar.toggle"));
        let row_for = |command: &str| rows.iter().find(|r| r.command == command).unwrap();
        let sidebar = row_for("sidebar.toggle");
        assert_eq!((sidebar.source, sidebar.default_key.as_deref()), ("Custom", Some("mod+b")));
        assert_eq!(rows.iter().filter(|r| r.command == "sidebar.toggle").count(), 1);
        assert_eq!(row_for("script.test.run").source, "Project");
        assert!(rows.iter().any(|r| r.command == "workspace.focusNext" && r.source == "Default"));

        let chat = row_for("chat.new");
        assert_eq!(conflict_labels(&rows, &chat.id, "mod+f19", "", true), ["Run Script: Test"]);
        assert!(conflict_labels(&rows, &chat.id, "mod+f19", "isWeb", true).is_empty());
        assert_eq!(conflict_labels(&rows, "other", "mod+f19", "isWeb", true), ["Chat: New"]);
        assert_eq!(conflict_labels(&rows, "other", "meta+f19", "terminalFocus", true), ["Chat: New", "Run Script: Test"]);
        assert!(conflict_labels(&rows, "other", "ctrl+f19", "", true).is_empty());
        assert!(conflict_labels(&rows, &chat.id, "", "", true).is_empty());
        assert!(matches(sidebar, "custom") && matches(sidebar, "mod+shift+b") && !matches(sidebar, "zzz"));
        assert_eq!(sidebar.target(), json!({"command": "sidebar.toggle", "key": "mod+shift+b"}));
        assert_eq!(row_for("script.test.run").target()["when"], "terminalFocus");
    }

    #[test]
    fn describes_conflicts() {
        let labels = |n: usize| (0..n).map(|i| format!("L{i}")).collect::<Vec<_>>();
        assert_eq!(conflict_text(&labels(1)), "Conflicts with L0.");
        assert_eq!(conflict_text(&labels(3)), "Conflicts with L0, L1, L2.");
        assert_eq!(conflict_text(&labels(4)), "Conflicts with L0, L1, L2, and more.");
    }
}
