//! Global command overlay. Navigation stays local; mutations go through Source::net.
use crate::{
    App, Appearance, Route, data, icons, kit,
    theme::{self, AlphaMul, T},
};
use rune::{Cx, El, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col, el, icon, row, text, text_input};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

pub(crate) mod actions;
pub(crate) mod add_project;
mod scenes;
pub use scenes::SCENES;

#[derive(Default)]
pub struct Ui {
    pub open: bool,
    pub query: TextEdit,
    pub selected: usize,
    pub page: String,
    pub gallery: Option<&'static str>,
    pub gallery_edit: TextEdit,
    pub gallery_on: bool,
    pub gallery_menu: bool,
    pub gallery_combo: kit::ComboboxState,
    searches: Vec<(u64, String)>,
    acknowledgements: Vec<u64>,
    message_query: String,
    search_due: Option<i64>,
    message_matches: Vec<serde_json::Value>,
    restart: Option<Restart>,
    notice: Option<Notice>,
    /// Pages under the current one, so Escape and Backspace walk back.
    back: Vec<(String, String)>,
    /// Arrow keys picked a row; views without auto-highlight leave Enter to submit until then.
    picked: bool,
    add: add_project::State,
    act: actions::State,
}
struct Notice {
    title: String,
    description: Option<String>,
    until: i64,
    actions: Vec<ToastAction>,
    error: bool,
    /// Actions reading "Copied!" until their deadline, as web updates a toast after a copy.
    copied: Vec<(String, i64)>,
}
/// A toast button: its label, whether it is the primary one, and what it does.
pub(crate) type ToastAction = (String, bool, std::rc::Rc<dyn Fn(&mut App, &mut Tx)>);
struct Restart {
    thread: String,
    remaining: Vec<String>,
    pending: u64,
    refresh: serde_json::Value,
    refreshed: bool,
}
#[derive(Clone)]
enum Action {
    Navigate(Route),
    Page(&'static str),
    Appearance(&'static str),
    Theme(String),
    PinProject(String, bool),
    Restart(String),
    OpenProject(String),
    AddProject,
    NewProject,
    Source(add_project::Source),
    Browse(String),
    TogglePublish,
    Triage,
    NewWithoutProject,
    Find,
    CopyThread,
    LinkPr(String),
    ShowPrs,
    Files,
    ContentSearch,
    OpenFile(String),
    ProjectSettings(String),
}
enum Trailing {
    None,
    Chevron,
    /// Not ready: a "Setup Required" button that explains why.
    Setup(String),
    Check(bool),
    /// The theme or appearance in use: "Current" in muted text.
    Current,
    /// `ProjectPinButton`: a project row's pin, always shown once pinned, else on the highlight.
    Pin(String, bool),
}
struct SearchTerm {
    value: String,
    weight: f32,
    context: bool,
    keyword: bool,
}
impl SearchTerm {
    fn context(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            weight: 0.25,
            context: true,
            keyword: false,
        }
    }
    fn weighted(value: impl Into<String>, weight: f32) -> Self {
        Self {
            value: value.into(),
            weight,
            context: false,
            keyword: false,
        }
    }
    fn keyword(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            weight: 0.6,
            context: false,
            keyword: true,
        }
    }
}
struct Item {
    title: String,
    group: &'static str,
    glyph: &'static rune::Icon,
    shortcut: String,
    action: Action,
    subtitle: Option<String>,
    description: Option<String>,
    disabled: bool,
    trailing: Trailing,
    brand: Option<usize>,
    highlight: Option<(Vec<(usize, usize)>, Vec<(usize, usize)>)>,
    /// Title bytes from here on are semibold ("New thread in **demo**").
    strong_from: Option<usize>,
    timestamp: Option<String>,
    fields: Vec<SearchTerm>,
    pick: (i64, bool),
    references: Vec<String>,
    usage: f32,
}
fn item(
    title: impl Into<String>,
    group: &'static str,
    glyph: &'static rune::Icon,
    shortcut: &str,
    action: Action,
) -> Item {
    Item {
        title: title.into(),
        group,
        glyph,
        shortcut: shortcut.into(),
        action: action.clone(),
        subtitle: None,
        description: None,
        disabled: false,
        trailing: if matches!(action, Action::Page(_) | Action::AddProject | Action::NewProject | Action::Files | Action::ContentSearch) {
            Trailing::Chevron
        } else {
            Trailing::None
        },
        brand: None,
        highlight: None,
        strong_from: None,
        timestamp: None,
        pick: (0, false),
        references: vec![],
        fields: match &action {
            Action::Theme(_) | Action::Appearance(_) => vec![],
            Action::Restart(_) => [
                "restart", "reset", "reload", "agent", "session", "skills", "plugins", "mcp",
            ]
            .into_iter()
            .map(SearchTerm::keyword)
            .collect(),
            Action::Page("projects") => ["new thread", "project", "pick", "choose", "select"]
                .into_iter()
                .map(SearchTerm::keyword)
                .collect(),
            Action::Navigate(Route::Settings(_)) => {
                ["settings", "preferences", "configuration", "keybindings"]
                    .into_iter()
                    .map(SearchTerm::keyword)
                    .collect()
            }
            Action::Navigate(Route::Home { .. }) => ["new thread", "chat", "create", "draft"]
                .into_iter()
                .map(SearchTerm::keyword)
                .collect(),
            _ => vec![],
        },
        usage: 1.,
    }
}

/// "Change theme": every theme in the library, the one the current mode uses marked Current.
fn theme_items(s: &App) -> Vec<Item> {
    // Under System the palette cannot see the OS mode; the halves normally agree.
    let dark = s.appearance != Appearance::Light;
    let current = s.client_settings.values.get(if dark { "themeDark" } else { "themeLight" }).and_then(serde_json::Value::as_str).unwrap_or("t3-code").to_owned();
    crate::settings::themes::library(s)
        .iter()
        .map(|v| {
            let id = data::s(v, "id").to_owned();
            let mut row = item(data::s(v, "label"), "Change theme", &icons::PALETTE, "", Action::Theme(id.clone()));
            row.fields = keywords(&[data::s(v, "label"), "theme", "appearance"]);
            if id == current {
                row.trailing = Trailing::Current;
            }
            row
        })
        .collect()
}
/// "Change appearance": System, Light and Dark, the saved mode marked Current.
fn appearance_items(s: &App) -> Vec<Item> {
    [("system", "System", &icons::MONITOR, Appearance::System), ("light", "Light", &icons::SUN, Appearance::Light), ("dark", "Dark", &icons::MOON, Appearance::Dark)]
        .into_iter()
        .map(|(mode, label, glyph, appearance)| {
            let mut row = item(label, "Change appearance", glyph, "", Action::Appearance(mode));
            row.fields = keywords(&[label, "appearance", "mode"]);
            if s.appearance == appearance {
                row.trailing = Trailing::Current;
            }
            row
        })
        .collect()
}
fn keywords(terms: &[&str]) -> Vec<SearchTerm> {
    terms.iter().map(|v| SearchTerm::keyword(*v)).collect()
}
/// A configured shortcut as the web's `shortcutLabelForCommand` writes it: ⌃⌥⇧⌘ then the key on
/// mac, "Ctrl+Shift+N" elsewhere.
pub fn shortcut_label(s: &App, command: &str) -> String {
    let binding = s.state().config.as_ref().and_then(|v| v["keybindings"].as_array()).and_then(|rows| {
        rows.iter().find(|v| v["command"] == command && v["when"].is_null()).or_else(|| rows.iter().find(|v| v["command"] == command))
    });
    binding.map_or_else(String::new, |v| format_shortcut(&v["shortcut"], cfg!(target_os = "macos")))
}

fn format_shortcut(k: &serde_json::Value, mac: bool) -> String {
    let yes = |name: &str| k[name].as_bool().unwrap_or(false);
    let key = k["key"].as_str().unwrap_or("");
    let (ctrl, meta) = (yes("ctrlKey") || (yes("modKey") && !mac), yes("metaKey") || (yes("modKey") && mac));
    if !mac {
        let mut parts: Vec<String> = [(ctrl, "Ctrl"), (yes("altKey"), "Alt"), (yes("shiftKey"), "Shift"), (meta, "Meta")]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, name)| name.to_string())
            .collect();
        parts.push(match key {
            " " | "space" => "Space".into(),
            "escape" => "Esc".into(),
            "arrowup" => "Up".into(),
            "arrowdown" => "Down".into(),
            "arrowleft" => "Left".into(),
            "arrowright" => "Right".into(),
            one if one.chars().count() == 1 => one.to_uppercase(),
            word => word.chars().take(1).flat_map(char::to_uppercase).chain(word.chars().skip(1)).collect(),
        });
        return parts.join("+");
    }
    let mut label = String::new();
    for (on, glyph) in [(ctrl, "⌃"), (yes("altKey"), "⌥"), (yes("shiftKey"), "⇧"), (meta, "⌘")] {
        if on {
            label.push_str(glyph);
        }
    }
    label.push_str(&match key {
        "enter" => "↵".into(),
        "escape" => "Esc".into(),
        "arrowup" => "↑".into(),
        "arrowdown" => "↓".into(),
        "arrowleft" => "←".into(),
        "arrowright" => "→".into(),
        "backspace" => "⌫".into(),
        "space" => "Space".into(),
        other => other.to_uppercase(),
    });
    label
}
pub(crate) fn toast(s: &mut App, title: &str, description: Option<&str>) {
    toast_with(s, title, description, Vec::new());
}
pub(crate) fn toast_with(s: &mut App, title: &str, description: Option<&str>, actions: Vec<ToastAction>) {
    s.ui.shell.palette.notice = Some(Notice {
        title: title.into(),
        description: description.filter(|v| !v.is_empty()).map(str::to_owned),
        until: data::now_ms() + 4000,
        actions,
        error: false,
        copied: Vec::new(),
    });
}
/// The open toast's `label` action reads "Copied!", disabled, for 2 s.
pub(crate) fn toast_copied(s: &mut App, tx: &mut Tx, label: &str) {
    let Some(notice) = s.ui.shell.palette.notice.as_mut() else { return };
    let until = data::now_ms() + 2000;
    notice.copied.retain(|(l, _)| l != label);
    notice.copied.push((label.to_string(), until));
    notice.until = notice.until.max(until);
    tx.wake_after(std::time::Duration::from_millis(2000));
}
/// An error toast that stays until dismissed (`timeout: 0` on web), so a long hook failure can
/// be read.
pub(crate) fn toast_error(s: &mut App, title: &str, description: Option<&str>) {
    s.ui.shell.palette.notice = Some(Notice {
        title: title.into(),
        description: description.filter(|v| !v.is_empty()).map(str::to_owned),
        until: i64::MAX,
        actions: Vec::new(),
        error: true,
        copied: Vec::new(),
    });
}
/// The first `message` in an Effect cause, else the reply's own text.
pub(crate) fn error_message(error: &serde_json::Value, fallback: &str) -> String {
    fn find(v: &serde_json::Value) -> Option<&str> {
        match v {
            serde_json::Value::Object(map) => map
                .get("message")
                .and_then(|m| m.as_str())
                .filter(|m| !m.is_empty())
                .or_else(|| map.values().find_map(find)),
            serde_json::Value::Array(rows) => rows.iter().find_map(find),
            _ => None,
        }
    }
    find(error)
        .map(str::to_owned)
        .or_else(|| error.as_str().map(str::to_owned))
        .unwrap_or_else(|| fallback.into())
}
fn push_page(s: &mut App, page: &str, query: &str) {
    let ui = &mut s.ui.shell.palette;
    let previous = (std::mem::take(&mut ui.page), ui.query.text.clone());
    ui.back.push(previous);
    ui.page = page.into();
    set_query(s, query);
}
fn pop_page(s: &mut App) {
    let (page, query) = s.ui.shell.palette.back.pop().unwrap_or_default();
    if s.ui.shell.palette.page == "add-clone" {
        s.ui.shell.palette.add.clone = None;
    }
    s.ui.shell.palette.page = page;
    set_query(s, &query);
    add_project::sync_browse(s);
}
fn set_query(s: &mut App, query: &str) {
    let ui = &mut s.ui.shell.palette;
    ui.search_due = None;
    ui.message_matches.clear();
    ui.query = TextEdit::new(query);
    ui.query.focused = true;
    ui.selected = 0;
    ui.picked = false;
}
/// Opens the palette on the add-project sources, as the sidebar's Add project button does.
pub fn open_add_project(s: &mut App) {
    reset(s, true);
    add_project::open(s);
}
/// Opens the palette on "New thread in...", as `chat.new` does with several projects.
pub fn open_new_thread_in(s: &mut App) {
    reset(s, true);
    push_page(s, "projects", "");
}
fn reset(s: &mut App, open: bool) {
    let ui = &mut s.ui.shell.palette;
    ui.open = open;
    ui.page.clear();
    ui.back.clear();
    ui.add.clone = None;
    set_query(s, "");
}

fn items(s: &App) -> Vec<Item> {
    if let Some(rows) = add_project::raw_items(s) {
        return rows;
    }
    let ui = &s.ui.shell.palette;
    if ui.page == "files" {
        return actions::file_items(s);
    }
    let mut out = vec![];
    if ui.page == "add-project" {
        out = add_project::source_items(s);
    }
    if ui.page == "themes" {
        out.extend(theme_items(s));
    }
    if ui.page == "appearance" {
        out.extend(appearance_items(s));
    }
    let (actions_only, typed) = actions_filter(ui);
    if ui.page.is_empty() {
        let project = match &s.route {
            Route::Thread(id) => s
                .state()
                .threads
                .iter()
                .find(|v| data::s(v, "id") == id)
                .map(|v| data::s(v, "projectId")),
            Route::Home { project_id } => project_id.as_deref(),
            _ => None,
        };
        // The open project, else the most recent one, as the web's preferred picker entry.
        if let Some(p) = project
            .and_then(|id| s.state().projects.iter().find(|v| data::s(v, "id") == id))
            .or_else(|| crate::chat::home::draft_project(s))
        {
            let mut row = item(
                format!("New thread in {}", data::s(p, "title")),
                "Actions",
                &icons::SQUARE_PEN,
                &shortcut_label(s, "chat.newLocal"),
                Action::Navigate(Route::Home {
                    project_id: Some(data::s(p, "id").into()),
                }),
            );
            row.strong_from = Some("New thread in ".len());
            out.push(row);
        }
        out.push(item(
            "New thread in...",
            "Actions",
            &icons::SQUARE_PEN,
            "",
            Action::Page("projects"),
        ));
        if actions::scratch_available(s) {
            out.push(item(
                "New thread without a project",
                "Actions",
                &icons::MESSAGE_SQUARE_DASHED,
                &shortcut_label(s, "chat.newWithoutProject"),
                Action::NewWithoutProject,
            ));
        }
        out.extend(actions::thread_items(s));
        if let Route::Thread(id) = &s.route {
            out.push(item(
                "Restart agent session",
                "Actions",
                &icons::ROTATE_CCW,
                "",
                Action::Restart(id.clone()),
            ));
        }
        out.extend(actions::tail_items(s));
        let mut themes = item("Change theme", "Actions", &icons::PALETTE, &shortcut_label(s, "theme.select"), Action::Page("themes"));
        themes.fields = keywords(&["change theme", "appearance", "colors", "palette"]);
        out.push(themes);
        let mut appearance = item("Change appearance", "Actions", &icons::MONITOR, &shortcut_label(s, "appearance.cycle"), Action::Page("appearance"));
        appearance.fields = keywords(&["change appearance", "light", "dark", "system", "mode", "toggle"]);
        out.push(appearance);
        if crate::pages::pull_requests::supported(s) {
            let mut prs = item("Open pull requests", "Actions", &icons::GIT_PULL_REQUEST_ARROW, "", Action::Navigate(Route::PullRequests));
            prs.fields = keywords(&["pull requests", "prs", "review", "merge", "branch"]);
            out.push(prs);
        }
        let mut usage = item("Open usage", "Actions", &icons::CHART_NO_AXES_COLUMN, &shortcut_label(s, "usage.open"), Action::Navigate(Route::Usage));
        usage.fields = keywords(&["usage", "use", "tokens", "cost", "spend", "limits", "stats", "analytics"]);
        out.push(usage);
        out.push(item(
            "Open settings",
            "Actions",
            &icons::SETTINGS,
            "",
            Action::Navigate(Route::Settings("general".into())),
        ));
        if let Some(p) = project.and_then(|id| s.state().projects.iter().find(|v| data::s(v, "id") == id))
            && actions::can_organize_projects(s)
        {
            let pinned = !p["pinnedAt"].is_null();
            let mut pin = item(
                if pinned { "Unpin project" } else { "Pin project" },
                "Actions",
                if pinned { &icons::PIN_OFF } else { &icons::PIN },
                "",
                Action::PinProject(data::s(p, "id").into(), !pinned),
            );
            pin.description = Some(data::s(p, "title").into());
            pin.fields = keywords(&["pin", "unpin", "project", "favorite"]);
            out.push(pin);
        }
        // The current project, else the first, as the web's contextual project group.
        if let Some(p) = project
            .and_then(|id| s.state().projects.iter().find(|v| data::s(v, "id") == id))
            .or_else(|| s.state().projects.first())
        {
            let mut settings = item("Project settings", "Actions", &icons::FOLDER, "", Action::ProjectSettings(data::s(p, "id").into()));
            settings.description = Some(data::s(p, "title").into());
            settings.fields = keywords(&["project", "settings", "name", "icon", "scripts", "workspace", "checkout", "remove"]);
            out.push(settings);
        }
        if !actions_only && !typed.trim().is_empty() {
            out.extend(suggestions(s, &out, &normalize_query(typed)));
        }
        let browsing = typed.trim().is_empty();
        let open_thread = match &s.route {
            Route::Thread(id) => Some(id.as_str()),
            _ => None,
        };
        // RECENT_THREAD_LIMIT; the open thread is not a place to go back to.
        let mut threads: Vec<_> = s
            .state()
            .threads
            .iter()
            .filter(|v| v["archivedAt"].is_null() && !(browsing && open_thread == Some(data::s(v, "id"))))
            .collect();
        threads.sort_by_key(|v| std::cmp::Reverse(recent_use(v)));

        let limit = if browsing {
            12
        } else {
            threads.len()
        };
        for v in threads.into_iter().take(limit) {
            out.push(item(
                data::s(v, "title"),
                if browsing {
                    "Recent Threads"
                } else {
                    "Threads"
                },
                &icons::MESSAGE_SQUARE,
                "",
                Action::Navigate(Route::Thread(data::s(v, "id").into())),
            ));
            if let Some(row) = out.last_mut() {
                let project = s
                    .state()
                    .projects
                    .iter()
                    .find(|p| data::s(p, "id") == data::s(v, "projectId"))
                    .map(|p| data::s(p, "title"))
                    .unwrap_or("");
                row.subtitle = Some(format!("{project} · Local · ∞"));
                row.fields.push(SearchTerm::weighted(
                    data::s(v, "branch")
                        .trim_start_matches("t3/")
                        .trim_start_matches("t3code/"),
                    0.6,
                ));
                row.fields.push(SearchTerm::context(project));
                row.references.push(data::s(v, "id").into());
                for pr in v["pullRequests"].as_array().into_iter().flatten() {
                    row.fields
                        .push(SearchTerm::weighted(format!("#{}", pr["number"]), 1.));
                    row.fields
                        .push(SearchTerm::weighted(data::s(pr, "title"), 0.5));
                    row.references.push(data::s(pr, "url").into());
                }
                row.pick = active_pick(v, &normalize_query(typed), s.now_ms);
                row.usage = usage_boost(v, s.now_ms);

                let minutes =
                    ((s.now_ms - data::ts(v, "updatedAt").unwrap_or(s.now_ms)) / 60_000).max(0);
                row.timestamp = Some(if minutes == 0 {
                    "now".into()
                } else if minutes < 60 {
                    format!("{minutes}m ago")
                } else if minutes < 1440 {
                    format!("{}h ago", minutes / 60)
                } else {
                    format!("{}d ago", minutes / 1440)
                });
            }
        }
    }
    if ui.page == "projects" || (!typed.trim().is_empty() && ui.page.is_empty()) {
        let can_pin = actions::can_organize_projects(s);
        for p in &s.state().projects {
            out.push(item(
                data::s(p, "title"),
                "Projects",
                &icons::FOLDER_GIT_2,
                "",
                if ui.page == "projects" {
                    Action::Navigate(Route::Home {
                        project_id: Some(data::s(p, "id").into()),
                    })
                } else {
                    Action::OpenProject(data::s(p, "id").into())
                },
            ));
            if can_pin && let Some(row) = out.last_mut() {
                row.trailing = Trailing::Pin(data::s(p, "id").into(), !p["pinnedAt"].is_null());
            }
        }
    }
    if !typed.trim().is_empty() && ui.page.is_empty() {
        for (label, page) in [
            ("General", "general"),
            ("Appearance", "appearance"),
            ("Keybindings", "keybindings"),
            ("Providers", "providers"),
            ("Connections", "connections"),
        ] {
            out.push(item(
                label,
                "Settings",
                &icons::SETTINGS,
                "",
                Action::Navigate(Route::Settings(page.into())),
            ));
        }
    }
    if actions_only {
        out.retain(|v| v.group == "Actions");
    }
    let query = normalize_query(typed);
    if !query.is_empty() {
        let rank = |v: &Item, typos: bool| -> Option<(f32, bool, f32)> {
            let thread = matches!(v.action, Action::Navigate(Route::Thread(_)));
            let reference = reference_match(&v.references, &query);
            let normalized = if thread {
                thread_query(&query)
            } else {
                query.clone()
            };
            let query = normalized.as_str();
            let meaningful: Vec<_> = query.split_whitespace().filter(|v| !filler(v)).collect();
            let short = meaningful.join(" ").len() < 4;
            let mut total = 0.;
            let mut count = 0;
            let mut strong = true;
            let mut primary_matches = false;
            let primary = field(&v.title);
            for token in query.split_whitespace() {
                let needle: Vec<_> = token.chars().collect();
                let primary_score = match_token(&primary, &needle, typos, true, thread);
                let mut best = primary_score;
                let mut at_word_start = primary_score >= 0.7;
                primary_matches |= primary_score > 0.;
                for extra in &v.fields {
                    let score = match_token(
                        &field(&extra.value),
                        &needle,
                        typos && !extra.keyword,
                        false,
                        false,
                    );
                    if extra.keyword && score < if short { 1. } else { 0.7 } {
                        continue;
                    }
                    if score >= 0.7 && !extra.context {
                        at_word_start = true;
                    }
                    best = best.max(
                        score
                            * if extra.keyword && short {
                                0.8
                            } else {
                                extra.weight
                            },
                    );
                }
                if best == 0. {
                    if !meaningful.is_empty() && filler(token) {
                        continue;
                    }
                    return reference.then_some((1., true, 1.));
                }
                strong &= at_word_start;
                total += best;
                count += 1;
            }
            if count == 0 {
                return None;
            }
            let name = fold(&v.title);
            let quality = total / count as f32
                + if name == query {
                    0.5
                } else if query.contains(' ') && name.contains(query) {
                    0.25
                } else {
                    0.
                };
            let group_quality = if thread {
                quality.min(1.49)
            } else if !primary_matches {
                quality.min(0.65)
            } else {
                quality
            };
            Some((quality, strong, group_quality))
        };
        let typos = !out
            .iter()
            .any(|v| rank(v, false).is_some_and(|(_, strong, _)| strong));
        let mut groups: Vec<(&str, Vec<(Item, f32, f32)>)> = vec![];
        for v in out {
            if let Some((quality, _, group_quality)) = rank(&v, typos) {
                if let Some((_, rows)) = groups.iter_mut().find(|(group, _)| *group == v.group) {
                    rows.push((v, quality, group_quality));
                } else {
                    groups.push((v.group, vec![(v, quality, group_quality)]));
                }
            }
        }
        let class = |q: f32| {
            if q >= 1.5 {
                3
            } else if q >= 0.7 {
                2
            } else if q >= 0.4 {
                1
            } else {
                0
            }
        };
        groups.sort_by_key(|(_, rows)| {
            std::cmp::Reverse(
                rows.iter()
                    .map(|(_, _, group_quality)| class(*group_quality))
                    .max()
                    .unwrap_or(0),
            )
        });
        out = groups
            .into_iter()
            .flat_map(|(_, mut rows)| {
                rows.sort_by(|(a, qa, _), (b, qb, _)| {
                    class(*qb)
                        .min(2)
                        .cmp(&class(*qa).min(2))
                        .then_with(|| {
                            (if b.pick.1 { b.pick.0 } else { 0 })
                                .cmp(&(if a.pick.1 { a.pick.0 } else { 0 }))
                        })
                        .then_with(|| class(*qb).cmp(&class(*qa)))
                        .then_with(|| b.pick.0.cmp(&a.pick.0))
                        .then_with(|| (qb * b.usage).total_cmp(&(qa * a.usage)))
                });
                let limit = if !ui.page.is_empty() {
                    usize::MAX
                } else if rows.first().is_some_and(|(v, _, _)| v.group == "Threads") {
                    20
                } else {
                    3
                };
                rows.into_iter().take(limit).map(|(v, _, _)| v)
            })
            .collect();
        if ui.message_query == query && ui.page.is_empty() && !actions_only {
            let mut hits: Vec<_> = ui.message_matches.iter().collect();
            let subject = |hit: &serde_json::Value| {
                s.state()
                    .threads
                    .iter()
                    .find(|v| v["id"] == hit["threadId"])
            };
            let key = |hit: &serde_json::Value| {
                let quality = hit["quality"].as_f64().unwrap_or(1.) as f32;
                let pick = subject(hit)
                    .map(|v| active_pick(v, &query, s.now_ms).0)
                    .unwrap_or(0);
                let usage = subject(hit).map(|v| usage_boost(v, s.now_ms)).unwrap_or(1.);
                let score = (1. + (1. + hit["hits"].as_f64().unwrap_or(1.) as f32).log2()) * usage;
                (quality, pick, score)
            };
            hits.sort_by(|a, b| {
                let (qa, pa, sa) = key(a);
                let (qb, pb, sb) = key(b);
                (qb >= 0.8)
                    .cmp(&(qa >= 0.8))
                    .then_with(|| pb.cmp(&pa))
                    .then_with(|| qb.total_cmp(&qa))
                    .then_with(|| sb.total_cmp(&sa))
                    .then_with(|| {
                        data::s(b, "messageCreatedAt").cmp(data::s(a, "messageCreatedAt"))
                    })
            });
            for hit in hits.into_iter().take(10) {
                let id = data::s(hit, "threadId");
                if out.iter().any(
                    |v| matches!(&v.action,Action::Navigate(Route::Thread(other)) if other == id),
                ) {
                    continue;
                }
                let title = s
                    .state()
                    .threads
                    .iter()
                    .find(|v| data::s(v, "id") == id)
                    .map(|v| data::s(v, "title"))
                    .unwrap_or("Thread");
                let mut row = item(
                    title,
                    "Messages",
                    &icons::MESSAGE_SQUARE,
                    "",
                    Action::Navigate(Route::Thread(id.into())),
                );
                row.subtitle = Some(data::s(hit, "snippet").into());
                out.push(row);
            }
        }
    }
    out
}
/// A root query starting with ">" searches actions only, by the text after it.
fn actions_filter(ui: &Ui) -> (bool, &str) {
    match ui.query.text.strip_prefix('>') {
        Some(rest) if ui.page.is_empty() => (true, rest),
        _ => (false, ui.query.text.as_str()),
    }
}
/// `buildNestedSearchItems`: submenu options join the root search as "New thread in demo" or
/// "Change theme: Dracula", unless the query only names the submenu.
fn suggestions(s: &App, top: &[Item], query: &str) -> Vec<Item> {
    let mut out = vec![];
    for parent in top {
        let Action::Page(page) = &parent.action else { continue };
        let children = match *page {
            "projects" => s
                .state()
                .projects
                .iter()
                .map(|p| item(data::s(p, "title"), "", &icons::FOLDER_GIT_2, "", Action::Navigate(Route::Home { project_id: Some(data::s(p, "id").into()) })))
                .collect(),
            "themes" => theme_items(s),
            "appearance" => appearance_items(s),
            _ => continue,
        };
        let Some(prefix) = suggestion_prefix(&parent.title, query) else { continue };
        for mut child in children {
            let title = format!("{prefix}{}", child.title);
            if top.iter().any(|v| v.title == title) {
                continue;
            }
            child.title = title;
            child.group = "Suggestions";
            child.fields = vec![];
            out.push(child);
        }
    }
    out
}
/// "New thread in..." prefixes "New thread in ", "Change theme" prefixes "Change theme: ";
/// none when the query only names the submenu.
fn suggestion_prefix(title: &str, query: &str) -> Option<String> {
    let stem = title.strip_suffix("...").or_else(|| title.strip_suffix('…')).unwrap_or(title);
    if fold(stem).starts_with(query) {
        return None;
    }
    Some(if stem == title { format!("{stem}: ") } else { format!("{} ", stem.trim_end()) })
}
fn filler(token: &str) -> bool {
    [
        "a", "an", "and", "for", "in", "of", "on", "the", "to", "with",
    ]
    .contains(&token)
}
fn fold(source: &str) -> String {
    source
        .chars()
        .map(|c| {
            let normalized: String = c
                .to_string()
                .nfkd()
                .filter(|c| !is_combining_mark(*c))
                .flat_map(char::to_lowercase)
                .map(|c| match c {
                    'ς' => 'σ',
                    '‘' | '’' => '\'',
                    _ => c,
                })
                .collect();
            if normalized.chars().count() == 1 {
                normalized
            } else {
                c.to_lowercase().collect()
            }
        })
        .collect()
}
fn normalize_query(query: &str) -> String {
    fold(
        &query
            .trim_matches(|c: char| {
                c.is_whitespace() || ['"', '\'', '`', '“', '”', '‘', '’'].contains(&c)
            })
            .nfc()
            .collect::<String>(),
    )
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
}
fn limit_utf16(query: &str, limit: usize) -> String {
    let mut result = String::new();
    let mut units = 0;
    for c in query.chars() {
        if units + c.len_utf16() > limit {
            break;
        }
        units += c.len_utf16();
        result.push(c);
    }
    result
}
fn thread_query(query: &str) -> String {
    let mut query = query;
    for prefix in ["pull request", "pull", "pr"] {
        if let Some(rest) = query.strip_prefix(prefix) {
            let rest = rest.trim_start().trim_start_matches('#');
            if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                query = rest;
                break;
            }
        }
    }
    query = query.trim_start_matches('#').trim_start();
    for prefix in ["t3/", "t3code/"] {
        if let Some(rest) = query.strip_prefix(prefix)
            && !rest.is_empty()
        {
            query = rest;
            break;
        }
    }
    query.into()
}
fn reference_match(references: &[String], query: &str) -> bool {
    if query.chars().count() < 8 {
        return false;
    }
    let anywhere = query.chars().any(|c| c.is_ascii_digit());
    let not_cut = |s: &str, at: usize| {
        !s.get(at..)
            .and_then(|s| s.chars().next())
            .is_some_and(|c| c.is_ascii_digit())
    };
    references
        .iter()
        .filter(|v| !v.is_empty())
        .any(|reference| {
            let reference = reference.to_lowercase();
            if anywhere {
                reference
                    .find(query)
                    .is_some_and(|at| not_cut(&reference, at + query.len()))
                    || (query.starts_with(&reference) && not_cut(query, reference.len()))
            } else {
                reference.starts_with(query)
            }
        })
}
fn active_pick(thread: &serde_json::Value, query: &str, now: i64) -> (i64, bool) {
    let mut cut = String::new();
    let mut units = 0;
    for c in query.chars() {
        if units + c.len_utf16() > 32 {
            break;
        }
        units += c.len_utf16();
        cut.push(c);
    }
    let query = cut.trim();
    let prefix = query.encode_utf16().count() >= 3;
    let mut latest = (0, false);
    let mut exact = 0;
    for pick in thread["opens"]["picks"].as_array().into_iter().flatten() {
        let at = pick["at"].as_i64().unwrap_or(0);
        let text = data::s(pick, "query");
        if now - at > 21 * 86_400_000 {
            continue;
        }
        if text == query {
            exact = exact.max(at);
        }
        if (text == query || (prefix && text.starts_with(query))) && at > latest.0 {
            latest = (at, false);
        }
    }
    if exact > 0 { (exact, true) } else { latest }
}
fn activity_at(thread: &serde_json::Value) -> Option<i64> {
    if thread.get("latestUserAuthoredMessageAt").is_none() {
        data::ts(thread, "latestUserMessageAt")
            .or_else(|| data::ts(thread, "updatedAt"))
            .or_else(|| data::ts(thread, "createdAt"))
    } else {
        data::ts(thread, "latestUserAuthoredMessageAt").or_else(|| data::ts(thread, "createdAt"))
    }
}
fn usage_boost(thread: &serde_json::Value, now: i64) -> f32 {
    let decay = |at: i64| 2_f32.powf(-((now - at).max(0) as f32) / (14. * 86_400_000.));
    let recency = activity_at(thread).map(decay).unwrap_or(0.);
    let opens = &thread["opens"];
    let open_score = opens["score"].as_f64().unwrap_or(0.) as f32
        * decay(opens["openedAt"].as_i64().unwrap_or(0));
    (1. + recency + 0.5 * (1. + open_score).log2()).min(2.5)
}
fn recent_use(thread: &serde_json::Value) -> i64 {
    activity_at(thread)
        .unwrap_or(0)
        .max(thread["opens"]["openedAt"].as_i64().unwrap_or(0))
}
fn token_score(source: &str, token: &str, typos: bool, mid_word: bool) -> f32 {
    score_options(source, token, typos, mid_word)
        .map(|score| score - if fold(source) == token { 0.5 } else { 0. })
        .unwrap_or(0.)
}

/// Request only queries that can search message content; replies are applied after Net drains.
pub fn search_changed(s: &mut App, tx: &mut Tx) {
    let query = normalize_query(&s.ui.shell.palette.query.text);
    s.ui.shell.palette.picked = false;
    add_project::sync_browse(s);
    actions::files_changed(s, tx);
    s.ui.shell.palette.selected = 0;
    s.ui.shell.palette.search_due = None;
    s.ui.shell.palette.message_matches.clear();
    s.ui.shell.palette.message_query = query.clone();
    if !s.ui.shell.palette.page.is_empty()
        || s.ui.shell.palette.query.text.starts_with('>')
        || !query.split_whitespace().any(|v| {
            v.chars().count() >= 3
                || (v.chars().count() == 2
                    && v.chars().any(
                        |c| matches!(c as u32,0x3040..=0x30ff|0x3400..=0x9fff|0xac00..=0xd7af),
                    ))
        })
    {
        return;
    }
    s.ui.shell.palette.search_due = Some(data::now_ms() + 200);
    tx.wake_after(std::time::Duration::from_millis(200));
}
pub fn poll(s: &mut App) -> bool {
    let mut changed = add_project::poll(s);
    changed |= actions::poll(s);
    let acknowledgements = std::mem::take(&mut s.ui.shell.palette.acknowledgements);
    for id in acknowledgements {
        if s.src.net().and_then(|net| net.take_reply(id)).is_none() {
            s.ui.shell.palette.acknowledgements.push(id);
        }
    }
    if s.ui
        .shell
        .palette
        .notice
        .as_ref()
        .is_some_and(|v| data::now_ms() >= v.until)
    {
        s.ui.shell.palette.notice = None;
        changed = true;
    }
    if let Some(notice) = s.ui.shell.palette.notice.as_mut() {
        let (now, before) = (data::now_ms(), notice.copied.len());
        notice.copied.retain(|(_, until)| now < *until);
        changed |= notice.copied.len() != before;
    }
    if let Some(mut restart) = s.ui.shell.palette.restart.take() {
        match s.src.net().and_then(|net| net.take_reply(restart.pending)) {
            None => s.ui.shell.palette.restart = Some(restart),
            Some(Err(_)) => {
                toast(s, "Unable to restart agent session", None);
                changed = true;
            }
            Some(Ok(_)) => {
                if restart.refreshed {
                    toast(s, "Agent session will restart on your next message", None);
                    changed = true;
                } else if let Some(net) = s.src.net() {
                    restart.pending = if let Some(session) = restart.remaining.pop() {
                        net.dispatch(serde_json::json!({"type":"provider-session.detach","threadId":restart.thread,"providerSessionId":session,"reason":"client-requested"}))
                    } else {
                        restart.refreshed = true;
                        net.request("server.refreshProviders", restart.refresh.clone())
                    };
                    s.ui.shell.palette.restart = Some(restart);
                }
            }
        }
    }
    if s.ui
        .shell
        .palette
        .search_due
        .is_some_and(|due| data::now_ms() >= due)
    {
        s.ui.shell.palette.search_due = None;
        let query = s.ui.shell.palette.message_query.clone();
        let raw =
            s.ui.shell
                .palette
                .query
                .text
                .trim_matches(|c: char| {
                    c.is_whitespace() || ['"', '\'', '`', '“', '”', '‘', '’'].contains(&c)
                })
                .to_owned();
        if let Some(net) = s.src.net() {
            let id = net.request(
                "orchestration.searchThreads",
                serde_json::json!({"query":limit_utf16(&raw,200),"limit":10}),
            );
            s.ui.shell.palette.searches.push((id, query));
        }
    }
    let pending = std::mem::take(&mut s.ui.shell.palette.searches);
    for (id, query) in pending {
        let reply = s.src.net().and_then(|net| net.take_reply(id));
        match reply {
            Some(reply) => {
                if query == s.ui.shell.palette.message_query {
                    s.ui.shell.palette.message_matches = reply
                        .ok()
                        .and_then(|v| v["matches"].as_array().cloned())
                        .unwrap_or_default();
                    changed = true;
                }
            }
            None => s.ui.shell.palette.searches.push((id, query)),
        }
    }
    changed
}
/// Word starts, camel-case boundaries, prefixes, joined words and one-edit typo matching.
/// Mirrors the primary-name pass in packages/shared/src/paletteSearch.ts.
fn score(source: &str, query: &str) -> Option<f32> {
    score_options(source, query, true, true)
}
struct SearchField {
    text: Vec<char>,
    starts: Vec<bool>,
}
fn field(source: &str) -> std::rc::Rc<SearchField> {
    use std::{
        cell::RefCell,
        collections::{HashMap, VecDeque},
        rc::Rc,
    };
    struct Cache {
        values: HashMap<String, Rc<SearchField>>,
        order: VecDeque<String>,
    }
    thread_local! {static CACHE:RefCell<Cache>=RefCell::new(Cache {values:HashMap::new(),order:VecDeque::new()});}
    CACHE.with(|cache|{
        let mut cache=cache.borrow_mut();
        if let Some(field)=cache.values.get(source) {return field.clone();}
        let chars:Vec<_>=source.chars().collect();
        let starts=chars.iter().enumerate().map(|(i,c)| {
            let prev=i.checked_sub(1).map(|j|chars[j]);
            let apostrophe=matches!(prev,Some('\''|'’')) && i>=2 && chars[i-2].is_alphanumeric();
            !apostrophe && (c.is_alphanumeric() || !c.is_ascii()) && (i==0 || !chars[i-1].is_alphanumeric()
                || (chars[i-1].is_lowercase() && c.is_uppercase())
                || (i+1<chars.len() && chars[i-1].is_uppercase() && c.is_uppercase() && chars[i+1].is_lowercase())
                || chars[i-1].is_numeric()!=c.is_numeric() || matches!(*c as u32,0x3040..=0x30ff|0x3400..=0x4dbf|0x4e00..=0x9fff|0xf900..=0xfaff))
        }).collect();
        let field=Rc::new(SearchField {text:fold(source).chars().collect(),starts});
        if cache.order.len()>=100_000 && let Some(oldest)=cache.order.pop_front() {cache.values.remove(&oldest);}
        cache.order.push_back(source.into());cache.values.insert(source.into(),field.clone());field
    })
}
fn match_token(
    field: &SearchField,
    token: &[char],
    typos: bool,
    primary: bool,
    mid_word: bool,
) -> f32 {
    if token.is_empty() {
        return 0.;
    }
    let text = &field.text;
    let mut best: f32 = 0.;
    for i in 0..text.len() {
        if !text[i..].starts_with(token) {
            continue;
        }
        if field.starts[i] || !token[0].is_alphanumeric() {
            let end = i + token.len();
            let whole = end == text.len() || field.starts[end] || !text[end].is_alphanumeric();
            let mut run_end = end;
            while run_end < text.len() && text[run_end].is_alphanumeric() {
                run_end += 1;
            }
            best = best.max(if whole {
                1.
            } else {
                0.7 + 0.2 * token.len() as f32 / (run_end - i) as f32
            });
            if best == 1. {
                return best;
            }
        } else if mid_word && token.len() >= 4 {
            best = best.max(0.3);
        }
    }
    if best <= 0.3 && primary && joined_words(text, &field.starts, token) {
        return 0.6;
    }
    if best > 0.3 || !typos || token.len() < 3 || !token.iter().any(|c| c.is_alphabetic()) {
        return best;
    }
    for i in 0..text.len() {
        if !field.starts[i]
            || !(text[i] == token[0]
                || (token.len() > 3 && text[i] == token[1] && text.get(i + 1) == Some(&token[0])))
        {
            continue;
        }
        let mut run_end = i + 1;
        while run_end < text.len() && text[run_end].is_alphanumeric() {
            run_end += 1;
        }
        let short = token.len() <= 4;
        let lengths = if short {
            vec![token.len(), token.len() + 1]
        } else {
            vec![token.len(), token.len() - 1, token.len() + 1]
        };
        for len in lengths {
            if i + len <= run_end && one_edit(token, &text[i..i + len], short && len == token.len())
            {
                return 0.45;
            }
        }
    }
    best
}
fn score_options(source: &str, query: &str, typos: bool, mid_word: bool) -> Option<f32> {
    let prepared = field(source);
    let mut total = 0.;
    let mut count = 0;
    let meaningful = query.split_whitespace().any(|v| !filler(v));
    for token in query.split_whitespace() {
        let best = match_token(
            &prepared,
            &token.chars().collect::<Vec<_>>(),
            typos,
            true,
            mid_word,
        );
        if best == 0. {
            if meaningful && filler(token) {
                continue;
            }
            return None;
        }
        total += best;
        count += 1;
    }
    if count == 0 {
        return None;
    }
    let source = fold(source);
    Some(
        total / count as f32
            + if source == query {
                0.5
            } else if query.contains(' ') && source.contains(query) {
                0.25
            } else {
                0.
            },
    )
}
fn joined_words(field: &[char], starts: &[bool], token: &[char]) -> bool {
    let end = |at: usize| {
        let mut end = at + 1;
        while end < field.len() && !starts[end] && field[end].is_alphanumeric() {
            end += 1;
        }
        end
    };
    for start in 0..field.len() {
        if !starts[start] || token.first() != field.get(start) {
            continue;
        }
        let mut at = start;
        let mut used = 0;
        let mut runs = Vec::new();
        while used < token.len() && at < field.len() {
            let word_end = end(at);
            let length = (token.len() - used).min(word_end - at);
            if field[at..at + length] != token[used..used + length] {
                break;
            }
            runs.push((at, at + length));
            used += length;
            if used == token.len() {
                break;
            }
            at = word_end;
            while at < field.len() && !starts[at] {
                at += 1;
            }
        }
        if used != token.len() || runs.len() < 2 {
            continue;
        }
        let (last_start, last_end) = *runs.last().unwrap();
        if last_end == end(last_start)
            || last_end - last_start >= 3
            || Some(start) == starts.iter().position(|v| *v)
            || runs[..runs.len() - 1].iter().all(|(a, b)| b - a >= 3)
        {
            return true;
        }
    }
    false
}
fn one_edit(a: &[char], b: &[char], swap_only: bool) -> bool {
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let i = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[i..], &b[i..]);
    let swap = a.len() > 1 && b.len() > 1 && a[0] == b[1] && a[1] == b[0] && a[2..] == b[2..];
    swap || (!swap_only
        && (a == b
            || (a.len() > 0 && a[1..] == *b)
            || (b.len() > 0 && *a == b[1..])
            || (a.len() > 0 && b.len() > 0 && a[1..] == b[1..])))
}
fn execute(s: &mut App, action: Action, tx: &mut Tx) {
    match action {
        Action::Page(page) => return push_page(s, page, ""),
        Action::AddProject => return add_project::open(s),
        Action::NewProject => return add_project::choose(s, add_project::Source::New),
        Action::Source(source) => return add_project::choose(s, source),
        Action::Browse(to) => {
            set_query(s, &to);
            return add_project::sync_browse(s);
        }
        Action::TogglePublish => return add_project::toggle_publish(s),
        Action::Files => {
            push_page(s, "files", "");
            actions::open_files(s);
            return tx.wake_after(std::time::Duration::from_millis(1));
        }
        Action::Triage => actions::triage(s),
        Action::NewWithoutProject => actions::new_without_project(s),
        Action::Find => actions::find(s),
        Action::ContentSearch => crate::search::open(s),
        Action::CopyThread => {
            actions::copy_thread(s, tx);
        }
        Action::LinkPr(thread) => actions::open_link(s, thread),
        Action::ShowPrs => actions::show_pull_requests(s),
        Action::OpenFile(path) => crate::panel::open_file(s, path),
        Action::Navigate(route) => {
            if let Route::Thread(id) = &route {
                let visited = s
                    .state()
                    .threads
                    .iter()
                    .find(|v| v["id"] == *id)
                    .map(|v| data::s(v, "updatedAt").to_owned());
                let query = limit_utf16(&normalize_query(&s.ui.shell.palette.query.text), 200);
                if let Some(visited) = visited {
                    if let Some(at) = data::iso_ms(&visited) {
                        s.ui.shell.sidebar.visits.claim_open(id, at);
                    }
                    if let Some(net) = s.src.net() {
                        let ack=net.dispatch(serde_json::json!({"type":"thread.visit","threadId":id,"visitedAt":visited,"opened":true,"searchQuery":query}));
                        s.ui.shell.palette.acknowledgements.push(ack);
                    }
                }
            }
            crate::shell::sidebar::go(s, route)
        }
        Action::ProjectSettings(project) => {
            s.ui.settings.project = Some(project);
            crate::shell::sidebar::go(s, Route::Settings("projects".into()))
        }
        Action::Appearance(mode) => crate::settings::update(s, "appearance", serde_json::json!(mode)),
        Action::Theme(id) => {
            crate::settings::update(s, "themeLight", serde_json::json!(id));
            crate::settings::update(s, "themeDark", serde_json::json!(id));
        }
        Action::PinProject(project, pinned) => actions::pin_project(s, &project, pinned),
        Action::OpenProject(project) => {
            let thread = s
                .state()
                .threads
                .iter()
                .filter(|v| {
                    v["projectId"] == project
                        && v["archivedAt"].is_null()
                        && v["lineage"]["parentThreadId"].is_null()
                })
                .max_by_key(|v| recent_use(v))
                .map(|v| data::s(v, "id").to_owned());
            let route = thread.map(Route::Thread).unwrap_or(Route::Home {
                project_id: Some(project),
            });
            execute(s, Action::Navigate(route), tx);
        }
        Action::Restart(id) => {
            if s.ui.shell.palette.restart.is_some() {
                return;
            }
            let sessions = s
                .state()
                .thread_views
                .get(&id)
                .and_then(|v| v.projection["providerSessions"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|v| v["id"].as_str().map(str::to_owned))
                .collect::<Vec<_>>();
            let thread = s.state().threads.iter().find(|v| data::s(v, "id") == id);
            let project = thread.and_then(|v| {
                s.state()
                    .projects
                    .iter()
                    .find(|p| p["id"] == v["projectId"])
            });
            let refresh = serde_json::json!({
                "instanceId":thread.map(|v|v["modelSelection"]["instanceId"].clone()).unwrap_or(serde_json::Value::Null),
                "cwd":thread.and_then(|v|v["worktreePath"].as_str()).or_else(||project.and_then(|v|v["workspaceRoot"].as_str())),
                "fresh":true
            });
            let refresh: serde_json::Value = refresh
                .as_object()
                .unwrap()
                .iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<serde_json::Map<_, _>>()
                .into();
            if let Some(net) = s.src.net() {
                let mut remaining = sessions;
                remaining.reverse();
                let refreshed = remaining.is_empty();
                let pending = if let Some(session) = remaining.pop() {
                    net.dispatch(serde_json::json!({"type":"provider-session.detach","threadId":id,"providerSessionId":session,"reason":"client-requested"}))
                } else {
                    net.request("server.refreshProviders", refresh.clone())
                };
                s.ui.shell.palette.restart = Some(Restart {
                    thread: id,
                    remaining,
                    pending,
                    refresh,
                    refreshed,
                });
            }
        }
    }
    s.ui.shell.palette.open = false;
}
fn toggle_key(s: &App, e: &KeyEvent) -> bool {
    if let Some(bindings) = s
        .state()
        .config
        .as_ref()
        .and_then(|v| v["keybindings"].as_array())
    {
        return bindings
            .iter()
            .filter(|v| v["command"] == "commandPalette.toggle")
            .any(|v| {
                let k = &v["shortcut"];
                let yes = |name: &str| k[name].as_bool().unwrap_or(false);
                let key = k["key"].as_str().unwrap_or("");
                let matches = match &e.key {
                    Key::Command(c) => key == c.to_string(),
                    Key::Text(c) => key == c,
                    _ => false,
                };
                matches
                    && e.mods.cmd == (yes("modKey") || yes("metaKey"))
                    && e.mods.shift == yes("shiftKey")
                    && e.mods.alt == yes("altKey")
                    && (!yes("ctrlKey") || e.mods.ctrl)
            });
    }
    e.is_command('k') && !e.mods.shift && !e.mods.alt
}
/// Shortcuts the palette owns while closed, which the web also binds outside the terminal.
fn global_shortcut(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if crate::panel::terminal_active(s) {
        return false;
    }
    if crate::panel::shortcut(s, e, "filePicker.toggle") {
        if s.ui.shell.palette.open && s.ui.shell.palette.page == "files" {
            s.ui.shell.palette.open = false;
        } else {
            reset(s, true);
            execute(s, Action::Files, tx);
        }
        return true;
    }
    if crate::panel::shortcut(s, e, "theme.select") {
        reset(s, true);
        push_page(s, "themes", "");
        return true;
    }
    if crate::panel::shortcut(s, e, "appearance.cycle") {
        let (mode, label) = match s.appearance {
            Appearance::System => ("light", "Light"),
            Appearance::Light => ("dark", "Dark"),
            Appearance::Dark => ("system", "System"),
        };
        crate::settings::update(s, "appearance", serde_json::json!(mode));
        toast(s, &format!("Appearance: {label}"), None);
        return true;
    }
    if crate::panel::shortcut(s, e, "thread.copyReference") && actions::copy_thread(s, tx) {
        s.ui.shell.palette.open = false;
        return true;
    }
    false
}
pub fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if toggle_key(s, e) {
        let open = !s.ui.shell.palette.open;
        reset(s, open);
        return true;
    }
    if global_shortcut(s, e, tx) {
        return true;
    }
    if !s.ui.shell.palette.open {
        return false;
    }
    let rows = items(s);
    let raw = add_project::raw(s);
    match &e.key {
        Key::Escape => {
            if s.ui.shell.palette.page.is_empty() {
                s.ui.shell.palette.open = false;
            } else {
                pop_page(s);
            }
        }
        Key::Backspace if s.ui.shell.palette.query.text.is_empty() && !s.ui.shell.palette.page.is_empty() => pop_page(s),
        key @ (Key::Down | Key::Up) => {
            let ui = &mut s.ui.shell.palette;
            let n = rows.len();
            let down = matches!(key, Key::Down);
            ui.selected = if n == 0 {
                0
            } else if raw && !ui.picked {
                if down { 0 } else { n - 1 }
            } else if down {
                (ui.selected + 1) % n
            } else {
                (ui.selected + n - 1) % n
            };
            ui.picked = n > 0;
        }
        Key::Tab if raw => {
            let ui = &s.ui.shell.palette;
            let highlighted = if ui.picked { rows.get(ui.selected).map(|v| v.action.clone()) } else { None };
            add_project::complete(s, highlighted);
            return true;
        }
        Key::Enter => {
            let ui = &s.ui.shell.palette;
            let row = rows.get(ui.selected).filter(|v| !v.disabled);
            if raw && (!ui.picked || e.mods.cmd || e.mods.ctrl || row.is_none()) {
                add_project::submit(s);
            } else if let Some(row) = row {
                execute(s, row.action.clone(), tx);
            }
            return true;
        }
        _ => return false,
    }
    tx.scroll_into_view(format!("palette-item-{}", s.ui.shell.palette.selected));
    true
}

fn placeholder(s: &App) -> String {
    let ui = &s.ui.shell.palette;
    if ui.page == "files" {
        "Search files…".into()
    } else if ui.page == "add-new" {
        "Project name".into()
    } else if add_project::repository_step(s) {
        match ui.add.clone.as_ref().and_then(|c| c.provider) {
            None => "Enter Git clone URL".into(),
            Some(i) => format!("Enter {} repository ({})", add_project::PROVIDERS[i].label, add_project::PROVIDERS[i].hint),
        }
    } else if add_project::browsing(s) {
        "Enter path (e.g. ~/projects/my-app)".into()
    } else if !ui.page.is_empty() {
        "Search...".into()
    } else {
        "Search commands, projects, and threads...".into()
    }
}
fn submit_modifier() -> &'static str {
    if cfg!(target_os = "macos") { "⌘ Enter" } else { "Ctrl Enter" }
}
/// The add-project submit button that sits at the input's end.
fn submit_button(s: &App, t: &T) -> Option<El> {
    let ui = &s.ui.shell.palette;
    let busy = add_project::busy(s);
    let picked_key = if ui.picked { submit_modifier() } else { "Enter" };
    let (label, key, disabled) = if ui.page == "add-new" {
        (if busy { "Creating" } else { "Create" }, picked_key, ui.query.text.trim().is_empty() || busy)
    } else if add_project::repository_step(s) {
        let label = if busy {
            "Working"
        } else if ui.add.clone.as_ref().is_some_and(|c| c.provider.is_none()) {
            "Continue"
        } else {
            "Lookup"
        };
        (label, "Enter", ui.query.text.trim().is_empty() || busy)
    } else if add_project::browsing(s) {
        let confirm = ui.add.clone.as_ref().is_some_and(|c| c.confirm.is_some());
        let create = add_project::will_create(s);
        let label = match (confirm, busy, create) {
            (true, true, _) => "Cloning",
            (true, false, true) => "Create & Clone",
            (true, false, false) => "Clone",
            (false, _, true) => "Create & Add",
            (false, _, false) => "Add",
        };
        (label, picked_key, add_project::view(s).needs_project || (confirm && busy))
    } else {
        return None;
    };
    Some(
        kit::Button::new(label)
            .variant("outline")
            .size("xs")
            .disabled(disabled)
            .on_press(|s: &mut App| {
                add_project::submit(s);
            })
            .build(t)
            .child(kit::kbd(key, t).mr(-2.))
            .key("palette-submit"),
    )
}
fn summary_row(glyph: El, title: String, description: String, t: &T) -> El {
    row()
        .w_full()
        .min_h(32.)
        .px(8.)
        .py(6.)
        .gap(8.)
        .center_y()
        .child(glyph)
        .child(
            col()
                .grow(1.)
                .shrink()
                .child(text(title).font(theme::TEXT_SM).color(t.c.contrast_foreground).truncate())
                .child(
                    text(description)
                        .font(theme::TEXT_XS)
                        .color(t.c.contrast_muted_foreground.alpha_mul(0.85))
                        .truncate(),
                ),
        )
}
/// The repository being cloned, or the project being created, above the rows.
fn summary(s: &App, t: &T) -> Option<El> {
    let ui = &s.ui.shell.palette;
    if ui.page == "add-new" {
        let root = add_project::new_projects_root(s).unwrap_or_default();
        let name = ui.query.text.trim();
        let (title, description) = if name.is_empty() {
            ("New project".to_string(), format!("Goes in {root}"))
        } else {
            (name.to_string(), format!("Creates {}", add_project::clone_destination(&root, &add_project::folder_name(name))))
        };
        let glyph = icon(&icons::FOLDER_GIT_2, 16.).color(t.c.contrast_icon_muted);
        return Some(col().w_full().padding(8.).pb(0.).child(summary_row(glyph, title, description, t)));
    }
    let flow = ui.add.clone.as_ref().filter(|_| ui.page == "add-clone")?;
    let confirm = flow.confirm.as_ref()?;
    let glyph = flow
        .provider
        .and_then(|i| add_project::brand(i, 16., t.c.contrast_icon_muted))
        .unwrap_or_else(|| icon(&icons::GIT_PULL_REQUEST_ARROW, 16.).color(t.c.contrast_icon_muted));
    let title = confirm.name_with_owner.clone().unwrap_or_else(|| confirm.input.clone());
    Some(
        col()
            .w_full()
            .padding(8.)
            .pb(0.)
            .child(
                row().px(8.).py(6.).child(text("Repository").font(theme::XS_MEDIUM).color(t.c.contrast_muted_foreground)),
            )
            .child(summary_row(glyph, title, confirm.url.clone(), t)),
    )
}
/// What an empty raw view says instead of rows.
fn empty_text(s: &App) -> Option<String> {
    let ui = &s.ui.shell.palette;
    if ui.page == "files" {
        return Some(ui.act.files.error.clone().unwrap_or_else(|| actions::files_empty(s).into()));
    }
    if add_project::repository_step(s) {
        let url = ui.add.clone.as_ref().is_some_and(|c| c.provider.is_none());
        return Some(if url {
            "Enter a Git clone URL and press Enter to continue."
        } else {
            "Enter a repository path and press Enter to look it up."
        }
        .into());
    }
    if add_project::browsing(s) {
        let confirm = ui.add.clone.as_ref().is_some_and(|c| c.confirm.is_some());
        return Some(if confirm {
            "Choose a destination path and press Enter to clone."
        } else if add_project::will_create(s) {
            "Press Enter to create this folder and add it as a project."
        } else {
            "No matching commands, projects, or threads."
        }
        .into());
    }
    if actions_filter(ui).0 {
        return Some("No matching actions.".into());
    }
    (ui.page.is_empty() || add_project::page_is_flow(&ui.page)).then(|| "No matching commands, projects, or threads.".into())
}
fn note(label: String, t: &T) -> El {
    row().w_full().px(16.).py(12.).child(text(label).font(theme::TEXT_SM).color(t.c.contrast_muted_foreground))
}
fn footer(s: &App, rows: &[Item], t: &T) -> El {
    let ui = &s.ui.shell.palette;
    let picked_row = if ui.picked { rows.get(ui.selected) } else { None };
    let action = if ui.page == "files" {
        Some("Open file")
    } else if ui.page == "add-new" {
        Some(match picked_row.map(|v| &v.action) {
            None => "Create",
            Some(Action::TogglePublish) => "Toggle",
            Some(_) => "Select",
        })
    } else if add_project::repository_step(s) {
        Some(if ui.add.clone.as_ref().is_some_and(|c| c.provider.is_none()) { "Continue" } else { "Lookup" })
    } else if add_project::browsing(s) {
        let can_submit = !ui.query.text.trim().is_empty() && !add_project::view(s).needs_project;
        (!can_submit || picked_row.is_some()).then_some("Select")
    } else {
        Some("Select")
    };
    let group = |key: El, label: &str| row().gap(4.).center_y().child(key).child(footer_label(label, t));
    let mut hints = row().gap(12.).center_y().child(
        row()
            .gap(4.)
            .center_y()
            .child(footer_key(Some(&icons::ARROW_UP), "", t))
            .child(footer_key(Some(&icons::ARROW_DOWN), "", t))
            .child(footer_label("Navigate", t)),
    );
    if let Some(action) = action {
        hints = hints.child(group(footer_key(None, "Enter", t), action));
    }
    if !ui.page.is_empty() {
        hints = hints.child(group(footer_key(None, "Backspace", t), "Back"));
    }
    hints = hints.child(group(footer_key(None, "Esc", t), if ui.page.is_empty() { "Close" } else { "Back" }));
    let mut bar = row()
        .w_full()
        .h(41.)
        .pl(17.)
        .pr(16.)
        .gap(12.)
        .center_y()
        .fill(t.c.contrast_foreground.alpha_mul(0.025))
        .border(1., t.c.contrast_foreground.alpha_mul(0.07))
        .child(hints)
        .child(el().grow(1.));
    if add_project::browsing(s) {
        let manager = if cfg!(target_os = "macos") {
            "Finder"
        } else if cfg!(target_os = "windows") {
            "File Explorer"
        } else {
            "Files"
        };
        bar = bar.child(
            row()
                .key("palette-open-in-files")
                .role("button")
                .label(format!("Open in {manager}"))
                .px(8.)
                .radius(8.)
                .center_y()
                .cursor(rune::Cursor::Pointer)
                .on_press_tx(add_project::pick_folder)
                .child(text(format!("Open in {manager}")).font(theme::XS_MEDIUM).color(t.c.contrast_muted_foreground)),
        );
    }
    bar
}
fn row_view(s: &App, i: usize, v: &Item, raw: bool, t: &T) -> El {
    let ui = &s.ui.shell.palette;
    let muted = t.c.contrast_muted_foreground;
    let title = match &v.highlight {
        Some((ranges, _)) => actions::highlighted(&v.title, ranges, theme::TEXT_SM, muted, t.c.contrast_foreground),
        None => match v.strong_from {
            Some(at) => actions::highlighted(&v.title, &[(at, v.title.len())], theme::TEXT_SM, t.c.contrast_foreground, t.c.contrast_foreground),
            None => text(v.title.clone()).font(theme::TEXT_SM).color(t.c.contrast_foreground).truncate(),
        },
    };
    let mut label = col().grow(1.).shrink().child(title);
    if let Some(subtitle) = &v.subtitle {
        label = label.child(text(subtitle).font(theme::TEXT_XS).color(muted).truncate());
    } else if let Some(description) = &v.description {
        let color = muted.alpha_mul(0.7);
        label = label.child(match &v.highlight {
            Some((_, ranges)) => actions::highlighted(description, ranges, theme::TEXT_XS, color, t.c.contrast_foreground),
            None => text(description).font(theme::TEXT_XS).color(color).truncate(),
        });
    }
    let glyph = v
        .brand
        .and_then(|b| add_project::brand(b, 16., t.c.contrast_icon_muted))
        .unwrap_or_else(|| icon(v.glyph, 16.).color(t.c.contrast_icon_muted));
    let selected = ui.selected == i && (!raw || ui.picked);
    let mut entry = row()
        .key(format!("palette-item-{i}"))
        .role("option")
        .label(v.title.clone())
        .w_full()
        .h(if v.subtitle.is_some() {
            50.
        } else if v.description.is_some() {
            48.
        } else {
            32.
        })
        .px(8.)
        .gap(8.)
        .center_y()
        .radius(6.)
        .fill(if selected { t.c.accent } else { rune::Color::TRANSPARENT })
        .child(glyph)
        .child(label);
    if v.disabled {
        entry = entry.opacity(0.64);
    } else {
        let action = v.action.clone();
        entry = entry.hover(t.c.accent).on_press_tx(move |s: &mut App, tx| execute(s, action.clone(), tx));
    }
    if let Some(timestamp) = &v.timestamp {
        entry = entry.child(text(timestamp).font(theme::TEXT_XS).color(muted));
    }
    if !v.shortcut.is_empty() {
        entry = entry.child(text(v.shortcut.clone()).font(theme::XS_MEDIUM).color(t.c.contrast_secondary_label));
    }
    match &v.trailing {
        Trailing::None => {}
        Trailing::Chevron => entry = entry.child(icon(&icons::CHEVRON_RIGHT, 14.).color(t.c.contrast_icon_muted)),
        Trailing::Setup(hint) => {
            entry = entry.child(
                kit::Button::new("Setup Required")
                    .variant("warning-outline")
                    .size("micro")
                    .on_press(|s: &mut App| {
                        s.ui.shell.palette.open = false;
                        crate::shell::sidebar::go(s, Route::Settings("source-control".into()));
                    })
                    .build(t)
                    .help(hint.clone()),
            )
        }
        Trailing::Check(on) => entry = entry.child(kit::Checkbox::new(*on).hide_label().build(&kit::theme(t))),
        Trailing::Current => entry = entry.child(text("Current").font(theme::TEXT_XS).color(t.c.contrast_muted_foreground.alpha_mul(0.7))),
        Trailing::Pin(project, pinned) if *pinned || selected => {
            let (project, pinned) = (project.clone(), *pinned);
            let label = format!("{} {}", if pinned { "Unpin" } else { "Pin" }, v.title);
            entry = entry.child(
                kit::Button::new(label.clone())
                    .icon(&icons::PIN)
                    .size("icon-xs")
                    .variant("ghost-muted")
                    .on_press(move |s: &mut App| actions::pin_project(s, &project, !pinned))
                    .build(t)
                    .key(format!("palette-pin-{i}"))
                    .help(label),
            );
        }
        Trailing::Pin(..) => {}
    }
    entry
}
pub fn overlay(s: &App, _cx: &mut Cx<App>, t: &T) -> Vec<El> {
    let mut out = vec![];
    if s.ui.shell.palette.open {
        out.extend(palette(s, t));
    }
    actions::link_dialog(s, t, &mut out);
    if let Some(notice) = &s.ui.shell.palette.notice {
        let mut toast = kit::Toast::new("palette-notice", notice.title.clone());
        if let Some(description) = &notice.description {
            toast = toast.description(description.clone());
        }
        for (label, primary, run) in &notice.actions {
            let run = run.clone();
            let copied = notice.copied.iter().any(|(l, _)| l == label);
            let button = kit::Button::new(if copied { "Copied!".to_string() } else { label.clone() })
                .variant(if *primary { "default" } else { "outline" })
                .size("xs")
                .disabled(copied)
                .build(t);
            toast = toast.button(*primary, if copied { button } else { button.on_press_tx(move |s: &mut App, tx: &mut Tx| run(s, tx)) });
        }
        if notice.error {
            toast = toast
                .lead(icon(&icons::CIRCLE_ALERT, 16.).color(t.c.destructive))
                .on_close(|s: &mut App| s.ui.shell.palette.notice = None);
        }
        out.push(kit::ToastRegion::new().toast(toast.build(&kit::theme(t))).build(&kit::theme(t)));
    }
    out
}
fn palette(s: &App, t: &T) -> Vec<El> {
    let ui = &s.ui.shell.palette;
    let rows = items(s);
    let raw = add_project::raw(s);
    let input = text_input(&ui.query, theme::TEXT_SM)
        .key("palette-query")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .h(34.)
        .py(7.)
        .font(theme::TEXT_SM)
        .color(t.c.contrast_foreground)
        .placeholder(placeholder(s), t.c.contrast_placeholder)
        .on_input_key(on_key)
        .on_edit(|s: &mut App| &mut s.ui.shell.palette.query)
        .on_input_change(search_changed);
    let lead = if !ui.page.is_empty() {
        row()
            .key("palette-back")
            .role("button")
            .label("Back")
            .center()
            .cursor(rune::Cursor::Pointer)
            .on_press(pop_page)
            .child(icon(&icons::ARROW_LEFT, 16.).color(t.c.contrast_icon_muted))
    } else if add_project::browsing(s) {
        icon(&icons::FOLDER_PLUS, 16.).color(t.c.contrast_icon_muted)
    } else {
        icon(&icons::SEARCH, 16.).color(t.c.contrast_icon_muted)
    };
    let submit = submit_button(s, t);
    let mut head = row()
        .w_full()
        .h(48.)
        .pl(17.)
        .pr(if submit.is_some() { 10. } else { 17. })
        .gap(12.)
        .center_y()
        .child(lead)
        .child(input);
    if let Some(submit) = submit {
        head = head.child(submit);
    }
    let mut list = col().w_full().padding(8.);
    let mut group = "";
    for (i, v) in rows.iter().enumerate() {
        if group != v.group {
            group = v.group;
            let heading = if group == actions::FILES_GROUP { actions::heading(s) } else { group.to_string() };
            if !heading.is_empty() {
                list = list.child(
                    row()
                        .w_full()
                        .h(28.)
                        .px(8.)
                        .center_y()
                        .child(text(heading).font(theme::XS_MEDIUM).color(t.c.contrast_muted_foreground)),
                );
            }
        }
        list = list.child(row_view(s, i, v, raw, t));
    }
    let browse_error = add_project::browsing(s).then(|| add_project::browse_error(s)).flatten();
    if let Some(error) = browse_error {
        list = list.child(note(error, t).role("alert"));
    } else if add_project::browsing(s) && add_project::browse_pending(s) {
        list = list.child(note("Loading folders...".into(), t));
    } else if rows.is_empty() {
        list = list.child(match empty_text(s) {
            Some(label) => col()
                .w_full()
                .py(40.)
                .center()
                .child(text(label).font(theme::TEXT_SM).color(t.c.contrast_muted_foreground)),
            None => kit::empty("No results found.", "", t),
        });
    }
    let tall = ui.page == "files";
    let list = kit::scroll_area(
        "palette-results",
        list.max_h(if tall { 400. } else { 330. }).scroll(ScrollSpec { stick_bottom: false }),
        t,
    );
    let mut panel = col()
        .key("command-palette")
        .role("dialog")
        .label("Command palette")
        .w(576.)
        .max_h(if tall { 490. } else { 420. })
        .radius(16.)
        .clip_rounded(16.)
        .fill(t.c.background.alpha_mul(0.8))
        .backdrop_blur(16.)
        .border(1., t.c.contrast_foreground.alpha_mul(0.1))
        .child(head);
    if let Some(summary) = summary(s, t) {
        panel = panel.child(summary);
    }
    let panel = panel
        .child(list)
        .child(footer(s, &rows, t))
        .modal(true, |s: &mut App| s.ui.shell.palette.open = false);
    vec![
        kit::motion::scrim(t).key("palette-backdrop"),
        col()
            .key("palette-layer")
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .pt(90.)
            .items_center()
            .transition(rune::Transition::none())
            .child(kit::motion::popup(panel)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn submenu_suggestions_name_the_submenu_unless_the_query_only_names_it() {
        assert_eq!(suggestion_prefix("New thread in...", "demo").as_deref(), Some("New thread in "));
        assert_eq!(suggestion_prefix("Change theme", "dracula").as_deref(), Some("Change theme: "));
        assert_eq!(suggestion_prefix("New thread in…", "new thread"), None);
        assert_eq!(suggestion_prefix("Change theme", "change"), None);
    }

    #[test]
    fn shortcut_labels_spell_modifiers_out_off_mac() {
        let k = serde_json::json!({ "key": "n", "modKey": true, "shiftKey": true });
        assert_eq!(format_shortcut(&k, false), "Ctrl+Shift+N");
        assert_eq!(format_shortcut(&k, true), "⇧⌘N");
        let k = serde_json::json!({ "key": "arrowleft", "metaKey": true, "altKey": true });
        assert_eq!(format_shortcut(&k, false), "Alt+Meta+Left");
        assert_eq!(format_shortcut(&serde_json::json!({ "key": "enter", "ctrlKey": true }), false), "Ctrl+Enter");
    }
    #[test]
    fn names_match_word_starts_joined_words_and_restricted_typos() {
        for (name, query, expected) in [
            ("GitHub", "githbu", true),
            ("Open settings", "settings", true),
            ("VS Code", "vscode", true),
            ("JSONSchema", "schema", true),
            ("Search", "arch", true),
            ("Model", "del", false),
            ("PR 123", "124", false),
            ("Thread", "xyz", false),
            ("Café déjà", "cafe deja", true),
            ("Julius’s project", "julius's", true),
            ("配置更新", "更新", true),
            ("Open VS Code", "vscode", true),
            ("Fix in thread", "inth", false),
        ] {
            assert_eq!(score(name, query).is_some(), expected, "{name} / {query}");
        }
        assert!(
            score("Settings", "settings").unwrap() > score("Open settings", "settings").unwrap()
        );
    }
    #[test]
    fn context_matches_do_not_outrank_titles_and_commands_reject_mid_word() {
        assert_eq!(token_score("Search", "arch", false, false), 0.);
        assert!(token_score("Search", "arch", false, true) > 0.);
        assert_eq!(token_score("PR 12345", "12346", true, true), 0.);
        assert_eq!(normalize_query(" “Open   settings” "), "open settings");
        let mut s = App::new();
        s.ui.shell.palette.query = TextEdit::new(data::s(&s.state().projects[0], "title"));
        let rows = items(&s);
        assert!(rows.iter().any(|v| v.group == "Threads"));
        assert!(
            rows.iter()
                .filter(|v| v.group == "Threads")
                .all(|v| v.usage >= 1. && v.usage <= 2.5)
        );
    }
    #[test]
    fn picks_expire_and_exact_picks_beat_newer_prefix_picks() {
        let now = 30 * 86_400_000;
        let thread = serde_json::json!({"opens":{"picks":[
            {"query":"settings","at":now-1000},
            {"query":"settings theme","at":now},
            {"query":"expired","at":0}
        ]}});
        assert_eq!(active_pick(&thread, "settings", now), (now - 1000, true));
        assert_eq!(active_pick(&thread, "set", now), (now, false));
        assert_eq!(active_pick(&thread, "se", now), (0, false));
        assert_eq!(active_pick(&thread, "expired", now), (0, false));
        assert_eq!(thread_query("pull request #123"), "123");
        assert_eq!(thread_query("t3/fix-menu"), "fix-menu");
        let mut app = App::new();
        app.ui.shell.palette.page = "appearance".into();
        app.ui.shell.palette.query = TextEdit::new("dark");
        let rows = items(&app);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Dark");
    }
    #[test]
    fn stale_search_results_are_hidden_after_query_changes() {
        let mut s = App::new();
        s.ui.shell.palette.query = TextEdit::new("needle");
        s.ui.shell.palette.message_query = "old".into();
        s.ui.shell.palette.message_matches =
            vec![serde_json::json!({"threadId":"other","snippet":"old needle"})];
        assert!(!items(&s).iter().any(|v| v.group == "Messages"));
        s.ui.shell.palette.message_query = "needle".into();
        assert!(items(&s).iter().any(|v| v.group == "Messages"));
    }
    #[test]
    fn configured_shortcut_replaces_default_and_requires_exact_modifiers() {
        let mut s = App::new();
        s.src = crate::data::Source::Fixture({
            let mut state = crate::data::fixture_state();
            state.config = Some(
                serde_json::json!({"keybindings":[{"command":"commandPalette.toggle","shortcut":{"key":"p","modKey":true,"shiftKey":true}}]}),
            );
            state
        });
        let mut e = KeyEvent {
            key: Key::Command('p'),
            mods: rune::Mods {
                cmd: true,
                shift: true,
                ..Default::default()
            },
        };
        assert!(toggle_key(&s, &e));
        e.mods.shift = false;
        assert!(!toggle_key(&s, &e));
        e.key = Key::Command('k');
        assert!(!toggle_key(&s, &e));
    }
}

fn footer_key(glyph: Option<&'static rune::Icon>, label: &str, t: &T) -> El {
    let key = row()
        .role("note")
        .center()
        .h(20.)
        .min_w(20.)
        .px(4.)
        .radius(4.)
        .fill(t.c.contrast_foreground.alpha_mul(0.08));
    match glyph {
        Some(glyph) => key.child(icon(glyph, 12.).color(t.c.contrast_foreground)),
        None => key.child(text(label).font(theme::XS_MEDIUM).color(t.c.contrast_foreground)),
    }
}

fn footer_label(label: &str, t: &T) -> El {
    text(label)
        .font(theme::SM_MEDIUM)
        .color(t.c.contrast_muted_foreground)
}
