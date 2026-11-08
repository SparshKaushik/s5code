//! Web `ProjectIconPickerDialog`: a Lucide icon, an emoji or a monogram, with a colour, saved as
//! the project's `projectIcon` (and its `faviconPath` cleared). Also the icon preview Settings shows.
use crate::theme::{self, AlphaMul, T, tw};
use crate::{App, icons, kit};
use rune::{Color, Cursor, El, Icon, TextEdit, col, el, icon, row, text};
use serde_json::{Value, json};

const DEFAULT_ICON: &str = "folder-code";
const SCRATCH: (&str, &str) = ("message-square-dashed", "gray");

/// `PROJECT_ICON_COLORS`: value, label, swatch (`bg-*-500`), text light (`-600`), text dark (`-400`).
const COLORS: [(&str, &str, Color, Color, Color); 18] = [
    ("gray", "Gray", tw::GRAY_500, tw::GRAY_600, tw::GRAY_400),
    ("red", "Red", tw::RED_500, tw::RED_600, tw::RED_400),
    ("orange", "Orange", tw::ORANGE_500, tw::ORANGE_600, tw::ORANGE_400),
    ("amber", "Amber", tw::AMBER_500, tw::AMBER_600, tw::AMBER_400),
    ("yellow", "Yellow", tw::YELLOW_500, tw::YELLOW_600, tw::YELLOW_400),
    ("lime", "Lime", tw::LIME_500, tw::LIME_600, tw::LIME_400),
    ("green", "Green", tw::GREEN_500, tw::GREEN_600, tw::GREEN_400),
    ("emerald", "Emerald", tw::EMERALD_500, tw::EMERALD_600, tw::EMERALD_400),
    ("teal", "Teal", tw::TEAL_500, tw::TEAL_600, tw::TEAL_400),
    ("cyan", "Cyan", tw::CYAN_500, tw::CYAN_600, tw::CYAN_400),
    ("sky", "Sky", tw::SKY_500, tw::SKY_600, tw::SKY_400),
    ("blue", "Blue", tw::BLUE_500, tw::BLUE_600, tw::BLUE_400),
    ("indigo", "Indigo", tw::INDIGO_500, tw::INDIGO_600, tw::INDIGO_400),
    ("violet", "Violet", tw::VIOLET_500, tw::VIOLET_600, tw::VIOLET_400),
    ("purple", "Purple", tw::PURPLE_500, tw::PURPLE_600, tw::PURPLE_400),
    ("fuchsia", "Fuchsia", tw::FUCHSIA_500, tw::FUCHSIA_600, tw::FUCHSIA_400),
    ("pink", "Pink", tw::PINK_500, tw::PINK_600, tw::PINK_400),
    ("rose", "Rose", tw::ROSE_500, tw::ROSE_600, tw::ROSE_400),
];

const EMOJIS: [(&str, &str); 30] = [
    ("💻", "Computer"), ("🛠️", "Tools"), ("🚀", "Rocket"), ("🤖", "Robot"), ("✨", "Sparkles"),
    ("⚡", "Lightning"), ("🌐", "Web"), ("📱", "Mobile"), ("🖥️", "Desktop"), ("⌨️", "Keyboard"),
    ("⚙️", "Gear"), ("🗄️", "Database"), ("☁️", "Cloud"), ("📦", "Package"), ("📚", "Books"),
    ("🧪", "Test tube"), ("🔒", "Lock"), ("🎮", "Game"), ("🎵", "Music"), ("🎬", "Movie"),
    ("🖼️", "Picture"), ("🛍️", "Shopping"), ("🔥", "Fire"), ("💡", "Idea"), ("🧩", "Puzzle"),
    ("📊", "Chart"), ("🧠", "Brain"), ("🦄", "Unicorn"), ("🐙", "Octopus"), ("🌱", "Seedling"),
];

const fn lucide(name: &'static str, body: &'static str) -> Icon {
    Icon { name, view: 24.0, body }
}
static ROCKET: Icon = lucide("rocket", r#"<path d="M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09z" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="m12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="M9 12H4s.55-3.03 2-4c1.62-1.08 5 0 5 0" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#);
static GAMEPAD_2: Icon = lucide("gamepad-2", r#"<line x1="6" x2="10" y1="11" y2="11" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><line x1="8" x2="8" y1="9" y2="13" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><line x1="15" x2="15.01" y1="12" y2="12" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><line x1="18" x2="18.01" y1="10" y2="10" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="M17.32 5H6.68a4 4 0 0 0-3.978 3.59c-.006.052-.01.101-.017.152C2.604 9.416 2 14.456 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.414-1.414A2 2 0 0 1 9.828 16h4.344a2 2 0 0 1 1.414.586L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.545-.604-6.584-.685-7.258-.007-.05-.011-.1-.017-.151A4 4 0 0 0 17.32 5z" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#);
static MUSIC: Icon = lucide("music", r#"<path d="M9 18V5l12-2v13" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><circle cx="6" cy="18" r="3" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><circle cx="18" cy="16" r="3" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#);
static SHOPPING_BAG: Icon = lucide("shopping-bag", r#"<path d="M6 2 3 6v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V6l-3-4Z" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="M3 6h18" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="M16 10a4 4 0 0 1-8 0" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#);
static CLOUD_COG: Icon = lucide("cloud-cog", r#"<circle cx="12" cy="17" r="3" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="M4.2 15.1A7 7 0 1 1 15.71 8h1.79a4.5 4.5 0 0 1 2.5 8.2" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="m15.7 18.4-.9-.3" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="m9.2 15.9-.9-.3" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="m10.6 20.7.3-.9" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="m13.1 14.2.3-.9" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="m13.6 20.7-.4-1" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="m10.8 14.3-.4-1" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="m8.3 18.6 1-.4" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/><path d="m14.7 15.8 1-.4" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#);

/// `POPULAR_PROJECT_ICONS`, as the names web stores; `code-2`, `globe-2` and `layers-3` are
/// lucide aliases of `code-xml`, `earth` and `layers`.
static POPULAR: [(&str, &Icon); 24] = [
    ("folder-code", &icons::FOLDER_CODE), ("code-2", &icons::CODE_XML), ("terminal", &icons::TERMINAL),
    ("globe-2", &icons::GLOBE), ("server", &icons::SERVER), ("database", &icons::DATABASE),
    ("bot", &icons::BOT), ("sparkles", &icons::SPARKLES), ("smartphone", &icons::SMARTPHONE),
    ("monitor", &icons::MONITOR), ("cloud-cog", &CLOUD_COG), ("package", &icons::PACKAGE),
    ("book-open", &icons::BOOK_OPEN), ("flask-conical", &icons::FLASK_CONICAL), ("shield-check", &icons::SHIELD_CHECK),
    ("rocket", &ROCKET), ("gamepad-2", &GAMEPAD_2), ("music", &MUSIC), ("image", &icons::IMAGE),
    ("shopping-bag", &SHOPPING_BAG), ("git-branch", &icons::GIT_BRANCH), ("workflow", &icons::WORKFLOW),
    ("wrench", &icons::WRENCH), ("layers-3", &icons::LAYERS),
];

/// A stored icon name drawn with the closest glyph this build has.
pub fn glyph(name: &str) -> &'static Icon {
    POPULAR
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, i)| *i)
        .or_else(|| super::icon_index::ALL.iter().find(|i| i.name == name).copied())
        .unwrap_or(&icons::FOLDER_CODE)
}

/// `filterProjectIconNames`: the popular set, else every icon whose name holds the query.
fn filter(query: &str) -> Vec<(&'static str, &'static Icon)> {
    let query = query.split_whitespace().collect::<Vec<_>>().join("-").to_lowercase();
    if query.is_empty() {
        return POPULAR.to_vec();
    }
    let mut out: Vec<(&str, &Icon)> = POPULAR.iter().copied().filter(|(n, _)| n.contains(&query)).collect();
    for i in super::icon_index::ALL.iter().copied() {
        if i.name.contains(&query) && !out.iter().any(|(n, o)| *n == i.name || std::ptr::eq(*o, i)) {
            out.push((i.name, i));
        }
    }
    out.truncate(60);
    out
}

fn ink(t: &T, color: &str) -> Color {
    let c = COLORS.iter().find(|c| c.0 == color).unwrap_or(&COLORS[0]);
    if t.dark { c.4 } else { c.3 }
}

/// `ProjectMonogram`: letters on a 14% wash of their colour.
fn monogram(t: &T, letters: &str, color: &str, size: f32) -> El {
    let ink = ink(t, color);
    el().size(size, size)
        .center()
        .radius(size * 0.25)
        .fill(ink.alpha_mul(0.14))
        .child(text(letters.to_owned()).font(theme::mono(rune::TextStyle::new(8.25 * size / 16.0, theme::BOLD).lh(size))).color(ink))
}

/// `ProjectFavicon` for a stored override; image favicons and no override fall back to the
/// name's monogram.
pub fn preview(t: &T, project: &Value, size: f32) -> El {
    let stored = &project["projectIcon"];
    let title = crate::data::s(project, "title");
    match stored["kind"].as_str() {
        Some("lucide") => el().size(size, size).center().child(icon(glyph(crate::data::s(stored, "name")), size * 0.75).color(ink(t, crate::data::s(stored, "color")))),
        Some("monogram") => monogram(t, crate::data::s(stored, "text"), crate::data::s(stored, "color"), size),
        Some("emoji") => el().size(size, size).center().child(text(crate::data::s(stored, "emoji").to_owned()).font(rune::TextStyle::new(size * 0.75, 400.0).lh(size))),
        _ => crate::chat::topbar::monogram(t, title, size),
    }
}

/// The settings row's description for the stored icon.
pub fn describe(project: &Value) -> String {
    let stored = &project["projectIcon"];
    let s = |k: &str| crate::data::s(stored, k).to_owned();
    match stored["kind"].as_str() {
        Some("lucide") => format!("{} · {}", s("name"), s("color")),
        Some("monogram") => format!("{} · {}", s("text"), s("color")),
        Some("emoji") => s("emoji"),
        _ => project["faviconPath"].as_str().unwrap_or("Automatic").to_owned(),
    }
}

/// `defaultProjectIcon`: what Reset writes, and whether the project already has it.
pub fn default_icon(project: &Value) -> Value {
    if project["isScratch"] == true { json!({"kind": "lucide", "name": SCRATCH.0, "color": SCRATCH.1}) } else { Value::Null }
}
pub fn has_default(project: &Value) -> bool {
    if !project["faviconPath"].is_null() {
        return false;
    }
    let stored = &project["projectIcon"];
    if project["isScratch"] != true {
        return stored.is_null();
    }
    stored["kind"] == "lucide" && stored["name"] == SCRATCH.0 && stored["color"] == SCRATCH.1
}

pub fn set(s: &mut App, project: &str, favicon: Value, icon: Value) {
    super::request(
        s,
        "projects.mutate",
        json!({"type":"project.update","commandId":s5_net::uuid(),"projectId":project,"faviconPath":favicon,"projectIcon":icon}),
    );
}

const IMAGE_EXTS: [&str; 9] = ["avif", "gif", "icns", "ico", "jpg", "jpeg", "png", "svg", "webp"];

/// Web's "Project icon" settings row: the current icon, the picker, a workspace image, and Reset.
pub fn settings_row(t: &T, project: &Value) -> El {
    let id = crate::data::s(project, "id").to_owned();
    let mut heading = row().gap(6.0).center_y().child(text("Project icon").font(theme::SM_MEDIUM).color(t.c.foreground));
    if !has_default(project) {
        let (id, icon) = (id.clone(), default_icon(project));
        heading = heading.child(
            kit::Button::new("Reset project icon")
                .variant("ghost-muted")
                .size("icon-micro")
                .icon(&icons::UNDO_2)
                .on_press(move |s: &mut App| set(s, &id, Value::Null, icon.clone()))
                .build(t),
        );
    }
    let chosen = project.clone();
    let root = crate::data::s(project, "workspaceRoot").to_owned();
    row()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(32.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(heading)
                .child(text(describe(project)).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap()),
        )
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(preview(t, project, 24.0))
                .child(
                    kit::Button::new("Choose icon")
                        .variant("outline")
                        .size("sm")
                        .on_press(move |s: &mut App| open(s, &chosen))
                        .build(t)
                        .label("Choose a project icon"),
                )
                .child(
                    kit::Button::new("Choose file")
                        .variant("outline")
                        .size("sm")
                        .build(t)
                        .label("Choose a project icon file")
                        .on_press_tx(move |_: &mut App, tx: &mut rune::Tx| choose_file(tx, id.clone(), root.clone())),
                ),
        )
}

/// Web lists the workspace's images; here the native dialog opens in the workspace and the pick
/// is kept as a workspace-relative path, the only kind the server resolves.
fn choose_file(tx: &mut rune::Tx, project: String, root: String) {
    let options = rune::FileDialogOptions {
        title: "Choose project icon".into(),
        directory: Some(root.clone().into()),
        ..Default::default()
    }
    .filter("Images", &IMAGE_EXTS);
    tx.open_file(options, move |s: &mut App, picked: rune::FileDialogResult, _| {
        let Ok(Some(path)) = picked else { return };
        match workspace_relative(&root, &path) {
            Some(relative) => set(s, &project, json!(relative), Value::Null),
            None => crate::shell::palette::toast(s, "Choose an image inside the project folder.", None),
        }
    });
}

fn workspace_relative(root: &str, path: &std::path::Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = relative.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[derive(Default)]
pub struct Draft {
    project: String,
    mode: usize,
    icon: String,
    color: String,
    letters: TextEdit,
    emoji: String,
    query: TextEdit,
    custom: TextEdit,
}

const MODES: [&str; 3] = ["lucide", "emoji", "monogram"];

pub fn open(s: &mut App, project: &Value) {
    let stored = &project["projectIcon"];
    let (automatic, index) = crate::chat::topbar::project_identity(crate::data::s(project, "title"));
    let kind = stored["kind"].as_str().unwrap_or("lucide");
    let field = |k: &str| stored[k].as_str().filter(|v| !v.is_empty()).map(str::to_owned);
    s.ui.settings.icon_picker = Some(Draft {
        project: crate::data::s(project, "id").to_owned(),
        mode: MODES.iter().position(|m| *m == kind).unwrap_or(0),
        icon: (if kind == "lucide" { field("name") } else { None }).unwrap_or_else(|| DEFAULT_ICON.into()),
        color: (if kind != "emoji" { field("color") } else { None }).unwrap_or_else(|| COLORS[index].0.into()),
        letters: TextEdit::new(if kind == "monogram" { field("text").unwrap_or(automatic) } else { automatic }),
        emoji: (if kind == "emoji" { field("emoji") } else { None }).unwrap_or_else(|| "💻".into()),
        ..Draft::default()
    });
}

/// `ProjectMonogramText` as the picker accepts it: one or two letters or numbers, uppercased.
fn monogram_text(letters: &str) -> Option<String> {
    let text = letters.trim().to_uppercase();
    let n = text.chars().count();
    ((1..=2).contains(&n) && text.chars().all(char::is_alphanumeric)).then_some(text)
}

/// `firstEmoji`: the first grapheme when it is a pictograph (a non-ASCII symbol and the
/// joiners, variation selectors and skin tones that belong to it).
fn first_emoji(value: &str) -> Option<String> {
    let mut chars = value.trim().chars().peekable();
    let first = chars.next()?;
    if first.is_ascii() || first.is_alphabetic() {
        return None;
    }
    let mut out = String::from(first);
    while let Some(&c) = chars.peek() {
        let joiner = out.ends_with('\u{200D}');
        if joiner || c == '\u{200D}' || c == '\u{FE0F}' || c == '\u{20E3}' || ('\u{1F3FB}'..='\u{1F3FF}').contains(&c) {
            out.push(c);
            chars.next();
        } else {
            break;
        }
    }
    Some(out)
}

fn draft(s: &mut App) -> &mut Draft {
    s.ui.settings.icon_picker.get_or_insert_with(Draft::default)
}

fn save(s: &mut App) {
    let Some(d) = s.ui.settings.icon_picker.as_ref() else { return };
    let icon = match MODES[d.mode] {
        "monogram" => match monogram_text(&d.letters.text) {
            Some(text) => json!({"kind": "monogram", "text": text, "color": d.color}),
            None => return,
        },
        "lucide" => json!({"kind": "lucide", "name": d.icon, "color": d.color}),
        _ => json!({"kind": "emoji", "emoji": d.emoji}),
    };
    let project = d.project.clone();
    s.ui.settings.icon_picker = None;
    set(s, &project, Value::Null, icon);
}

fn cell(t: &T, key: String, label: &str, selected: bool, size: f32, content: El) -> El {
    el().key(key)
        .size(size, size)
        .center()
        .radius(6.0)
        .border(1.0, if selected { t.c.border } else { Color::TRANSPARENT })
        .fill(if selected { t.c.accent } else { Color::TRANSPARENT })
        .hover(t.c.accent)
        .cursor(Cursor::Pointer)
        .focusable(true)
        .role("button")
        .label(label.to_owned())
        .checked(selected)
        .child(content)
}

fn grid(cells: Vec<El>, t: &T) -> El {
    let mut rows = col().w_full().gap(4.0).padding(2.0);
    let mut cells = cells.into_iter().peekable();
    while cells.peek().is_some() {
        rows = rows.child(row().gap(4.0).children(cells.by_ref().take(10)));
    }
    kit::scroll_area("project-icon-grid", col().w_full().max_h(256.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(rows), t)
}

pub fn dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(d) = &s.ui.settings.icon_picker else { return };
    let mode = MODES[d.mode];
    let size = ((464.0 - 4.0 - 9.0 * 4.0) / 10.0_f32).floor();
    let mut body = col().w_full().gap(16.0).child(
        kit::SegmentedControl::new(&["Icons", "Emoji", "Monogram"], Some(d.mode))
            .label("Icon type")
            .on_pick(|s: &mut App, i, _| draft(s).mode = i)
            .build(&kit::theme(t)),
    );
    if mode != "emoji" {
        let mut swatches = row().gap(6.0).flex_wrap().role("group").label("Icon color");
        for (value, label, swatch, ..) in COLORS {
            let on = d.color == value;
            swatches = swatches.child(
                el().size(24.0, 24.0)
                    .center()
                    .radius(12.0)
                    .border(1.0, if on { t.c.foreground.alpha_mul(0.64) } else { Color::TRANSPARENT })
                    .cursor(Cursor::Pointer)
                    .focusable(true)
                    .role("button")
                    .label(label)
                    .checked(on)
                    .on_press(move |s: &mut App| draft(s).color = value.into())
                    .child(el().size(16.0, 16.0).radius(8.0).fill(swatch)),
            );
        }
        body = body.child(
            col().w_full().gap(8.0).child(text("Color").font(theme::XS_MEDIUM).color(t.c.muted_foreground)).child(swatches),
        );
    }
    match mode {
        "lucide" => {
            let icons = filter(&d.query.text);
            let ink = ink(t, &d.color);
            body = body.child(
                kit::Input::new(&d.query).placeholder("Search all Lucide icons").on_edit(|s: &mut App| &mut draft(s).query).build(t).single_line(),
            );
            if icons.is_empty() {
                body = body.child(el().w_full().py(32.0).center().child(text("No icons found.").font(theme::TEXT_SM).color(t.c.muted_foreground)));
            } else {
                let cells = icons
                    .into_iter()
                    .map(|(name, glyph)| {
                        let label: String = name.split('-').map(|p| p[..1].to_uppercase() + &p[1..]).collect::<Vec<_>>().join(" ");
                        cell(t, format!("project-icon-{name}"), &label, d.icon == name, size, icon(glyph, 20.0).color(ink))
                            .on_press(move |s: &mut App| draft(s).icon = name.into())
                    })
                    .collect();
                body = body.child(grid(cells, t));
            }
        }
        "monogram" => {
            let valid = monogram_text(&d.letters.text);
            let (automatic, _) = crate::chat::topbar::project_identity(
                s.state().projects.iter().find(|p| p["id"] == d.project.as_str()).map_or("", |p| crate::data::s(p, "title")),
            );
            body = body.child(
                row()
                    .w_full()
                    .gap(16.0)
                    .py(8.0)
                    .center_y()
                    .child(monogram(t, valid.as_deref().unwrap_or(&automatic), &d.color, 48.0))
                    .child(
                        col()
                            .grow(1.0)
                            .min_w(0.0)
                            .gap(8.0)
                            .child(text("Letters").font(theme::SM_MEDIUM).color(t.c.foreground))
                            .child(kit::Input::new(&d.letters).invalid(valid.is_none()).on_edit(|s: &mut App| &mut draft(s).letters).build(t).single_line())
                            .child(text("One or two letters or numbers.").font(theme::TEXT_XS).color(t.c.muted_foreground)),
                    ),
            );
        }
        _ => {
            let cells = EMOJIS
                .iter()
                .map(|&(emoji, label)| {
                    cell(t, format!("project-emoji-{label}"), label, d.emoji == emoji, size, text(emoji).font(theme::TEXT_XL))
                        .on_press(move |s: &mut App| draft(s).emoji = emoji.into())
                })
                .collect();
            body = body.child(grid(cells, t)).child(
                col()
                    .w_full()
                    .gap(8.0)
                    .child(text("Or paste any emoji").font(theme::XS_MEDIUM).color(t.c.muted_foreground))
                    .child(
                        kit::Input::new(&d.custom)
                            .placeholder("Paste an emoji")
                            .on_edit(|s: &mut App| &mut draft(s).custom)
                            .on_change(|s: &mut App, _: &mut rune::Tx| {
                                let d = draft(s);
                                if let Some(emoji) = first_emoji(&d.custom.text) {
                                    d.emoji = emoji;
                                }
                            })
                            .build(t)
                            .single_line(),
                    ),
            );
        }
    }
    let close = |s: &mut App| s.ui.settings.icon_picker = None;
    let can_save = mode != "monogram" || monogram_text(&d.letters.text).is_some();
    kit::Dialog::new("project-icon", "Choose project icon")
        .open(true)
        .description("Choose an icon, emoji, or monogram.")
        .child(body)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                .child(kit::Button::new("Save icon").disabled(!can_save).on_press(save).build(t)),
        )
        .on_close(close)
        .build(t, layers);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_values_follow_web_validation() {
        assert_eq!(monogram_text(" ab "), Some("AB".into()));
        assert_eq!(monogram_text("x9"), Some("X9".into()));
        assert_eq!(monogram_text("abc"), None);
        assert_eq!(monogram_text("a!"), None);
        assert_eq!(monogram_text("  "), None);
        assert_eq!(first_emoji(" 🚀 rocket").as_deref(), Some("🚀"));
        assert_eq!(first_emoji("👩‍💻!").as_deref(), Some("👩‍💻"));
        assert_eq!(first_emoji("⚙️").as_deref(), Some("⚙️"));
        assert_eq!(first_emoji("abc"), None);
        assert_eq!(filter("").len(), 24);
        assert!(filter("git branch").iter().any(|(n, _)| *n == "git-branch"));
        assert!(filter("zzzz").is_empty());
        assert!(std::ptr::eq(glyph("code-2"), &icons::CODE_XML));
        let root = "/work/app";
        assert_eq!(workspace_relative(root, std::path::Path::new("/work/app/public/icon.png")).as_deref(), Some("public/icon.png"));
        assert_eq!(workspace_relative(root, std::path::Path::new("/work/application/icon.png")), None);
        assert_eq!(workspace_relative(root, std::path::Path::new("/work/app")), None);
    }

    #[test]
    fn reset_matches_default_project_icon() {
        let plain = json!({"faviconPath": null, "projectIcon": null});
        assert!(has_default(&plain));
        assert_eq!(default_icon(&plain), Value::Null);
        assert!(!has_default(&json!({"faviconPath": "icon.png", "projectIcon": null})));
        let scratch = json!({"isScratch": true, "projectIcon": {"kind": "lucide", "name": "message-square-dashed", "color": "gray"}});
        assert!(has_default(&scratch));
        assert!(!has_default(&json!({"isScratch": true, "projectIcon": null})));
        assert_eq!(default_icon(&scratch)["name"], "message-square-dashed");
    }
}
