//! Web's `ThemeEditorPanel` as `ThemeEditorHost` mounts it: a floating panel over the app whose
//! colors preview live. The session lives in the `form.themeEditor` value. Inspect picks the
//! app element under the pointer and maps its keyed region to a role (Rune can't tell which
//! token painted a pixel, as web's CSS probes do); merge-by-name is not ported.
use super::{builtins, color, custom_themes, modes, palette};
use crate::kit::{self, Button};
use crate::settings::vivid;
use crate::theme::{self, AlphaMul, T};
use crate::{App, Route, icons};
use rune::{El, Phase, TextEdit, col, el, row, text};
use serde_json::{Value, json};

const SESSION: &str = "form.themeEditor";
const FILTER: &str = "form.themeEditorFilter";
const SIMPLE: [(&str, &str); 2] = [("canvas", "Background"), ("accent", "Accent")];
const MARGIN: f32 = 8.0;
const MIN_W: f32 = 280.0;
const MIN_H: f32 = 220.0;
const WIDTH: f32 = 416.0;
static NULL: Value = Value::Null;

type Family = (&'static str, &'static str, &'static [&'static str]);

/// `THEME_EDITOR_ROLE_GROUPS`: each family shows one role and an advanced edit moves the rest.
const GROUPS: [(&str, &[Family]); 5] = [
    (
        "Foundation",
        &[
            ("Background", "canvas", &["canvas", "chrome", "toolbar"]),
            ("Surface", "surface", &["surface"]),
            ("Raised surface", "surfaceRaised", &["surfaceRaised"]),
            ("Overlay", "surfaceOverlay", &["surfaceOverlay"]),
            ("Text", "text", &["text", "toolbarForeground", "toolbarControlForeground"]),
            ("Muted text", "mutedForeground", &["textMuted", "mutedForeground", "placeholder", "secondaryLabel", "iconMuted", "sidebarMutedForeground"]),
            ("Border", "border", &["border", "toolbarBorder", "sidebarBorder"]),
            ("Input", "input", &["input"]),
        ],
    ),
    (
        "Brand & content",
        &[
            ("Subtle surface", "secondary", &["secondary", "secondaryForeground", "muted", "toolbarControl"]),
            ("Highlight surface", "accentSurface", &["accentSurface", "accentSurfaceForeground", "toolbarControlHover"]),
            ("Accent", "accent", &["accent", "accentForeground", "focus", "update", "updateForeground", "updateSurface", "terminalCursor"]),
            ("Action", "messageAction", &["messageAction", "messageActionForeground", "messageActionHover"]),
            ("Message surface", "messageSurface", &["messageSurface", "messageForeground"]),
            ("Code surface", "codeBackground", &["codeBackground", "codeForeground"]),
        ],
    ),
    (
        "Search",
        &[
            ("Search matches", "searchMatchBackground", &["searchMatchBackground", "searchMatchForeground"]),
            ("Current match", "searchMatchActiveBackground", &["searchMatchActiveBackground", "searchMatchActiveForeground"]),
        ],
    ),
    (
        "Context",
        &[
            ("Sidebar background", "sidebar", &["sidebar", "sidebarForeground"]),
            ("Sidebar controls", "sidebarControlSurface", &["sidebarControlSurface"]),
            ("Sidebar selection", "sidebarRowSelected", &["sidebarRowHover", "sidebarRowActive", "sidebarRowSelected"]),
            ("Terminal background", "terminalBackground", &["terminalBackground", "terminalForeground", "terminalSelection", "terminalScrollbar", "terminalScrollbarHover"]),
        ],
    ),
    (
        "Status",
        &[("Error", "error", &["error", "errorForeground", "errorSurface"]), ("Warning", "warning", &["warning", "warningForeground", "warningSurface"])],
    ),
];

fn mode(dark: bool) -> &'static str {
    if dark { "dark" } else { "light" }
}

fn stock(dark: bool) -> &'static Value {
    palette(&builtins()[0], dark)
}

fn session(s: &App) -> &Value {
    s.ui.settings.form.get(SESSION).unwrap_or(&NULL)
}

fn session_mut(s: &mut App) -> &mut Value {
    s.ui.settings.form.entry(SESSION.into()).or_insert_with(|| json!({}))
}

/// Every stock role, with the theme's own colors over it.
fn filled(dark: bool, colors: &Value) -> Value {
    let mut out = stock(dark).clone();
    for (role, css) in colors.as_object().into_iter().flatten() {
        out[role.as_str()] = css.clone();
    }
    out
}

fn picked<'a>(colors: &'a Value, dark: bool, role: &str) -> &'a str {
    colors[role].as_str().map(str::trim).filter(|c| color(c).is_some()).unwrap_or_else(|| stock(dark)[role].as_str().unwrap_or(""))
}

/// `getManagedEditorColors`: the whole palette regenerated from the background and accent.
fn managed(dark: bool, colors: &Value) -> Value {
    vivid::colors(stock(dark), dark, picked(colors, dark, "canvas"), picked(colors, dark, "accent"))
}

/// The seeded session and the appearance it opens on. Unmanaged themes open in advanced mode so
/// imported or hand-tuned colors are not regenerated.
fn seed(template: &Value, dark: bool, from_theme: bool) -> (Value, bool) {
    let mut colors = json!({"light": stock(false), "dark": stock(true)});
    let mut active = dark;
    if from_theme {
        if let Some(base) = template["appearance"].as_str().filter(|m| matches!(*m, "light" | "dark")) {
            colors[base] = filled(base == "dark", &template["colors"]);
        }
        for m in ["light", "dark"] {
            if template["variants"][m].is_object() {
                colors[m] = filled(m == "dark", &template["variants"][m]);
            }
        }
        if !modes(template).contains(&mode(dark)) {
            active = template["appearance"] == "dark";
        }
    }
    let unmanaged = from_theme && template["managed"] != true;
    let session = json!({"colors": colors, "dirty": {"light": false, "dark": false}, "regenerate": unmanaged, "advanced": unmanaged, "minimized": false, "pos": null, "error": null});
    (session, active)
}

fn open(s: &mut App, template: Value, dark: bool, from_theme: bool) {
    let (session, active) = seed(&template, dark, from_theme);
    s.ui.settings.theme_name = TextEdit::new(template["label"].as_str().unwrap_or(""));
    s.ui.settings.theme_editor = Some(template);
    s.ui.settings.theme_edit_dark = active;
    s.ui.settings.form.insert(SESSION.into(), session);
    s.ui.settings.form.remove(FILTER);
    s.ui.settings.edits.remove(FILTER);
    sync_fields(s, None);
}

/// Opens the editor seeded from a theme; a builtin opens as a new “… copy”.
pub fn edit(s: &mut App, mut template: Value, dark: bool) {
    if builtins().iter().any(|t| t["id"] == template["id"]) {
        template["id"] = json!(format!("custom-{}", s5_net::uuid()));
        template["label"] = json!(format!("{} copy", template["label"].as_str().unwrap_or("Theme")));
    }
    open(s, template, dark, true);
}

/// Web's “Create theme”: stock colors, no name, the two-color mode.
pub fn create(s: &mut App, dark: bool) {
    let template = json!({"id": format!("custom-{}", s5_net::uuid()), "label": "", "appearance": mode(dark), "colors": stock(dark)});
    open(s, template, dark, false);
}

/// Scenes: the editor open on a builtin's copy, picking from the app, or moved and resized with
/// the accent field selected.
pub fn scene(s: &mut App, state: &str) {
    let template = builtins().first().cloned().unwrap_or_else(|| json!({}));
    edit(s, template, true);
    let session = session_mut(s);
    match state {
        "inspecting" => {
            session["inspecting"] = json!(true);
            session["pointer"] = json!([180.0, 220.0]);
        }
        "resized" => {
            session["pos"] = json!([420.0, 48.0]);
            session["size"] = json!([560.0, 600.0]);
            session["selected"] = json!("accent");
        }
        _ => {}
    }
    if state == "picker" {
        open_picker(s, "accent", "Accent");
    }
}

fn close(s: &mut App) {
    s.ui.settings.theme_editor = None;
    s.ui.settings.theme_picker = None;
}

/// The modes of the theme being edited, or `None` while creating one.
fn editing_modes(s: &App) -> Option<Vec<&'static str>> {
    let template = s.ui.settings.theme_editor.as_ref()?;
    custom_themes(s).iter().find(|t| t["id"] == template["id"]).map(modes)
}

/// The hex fields follow the session, except the one being typed in.
fn sync_fields(s: &mut App, keep: Option<&str>) {
    let colors = session(s)["colors"][mode(s.ui.settings.theme_edit_dark)].clone();
    let fields = &mut s.ui.settings.theme_colors;
    fields.retain(|role, _| Some(role.as_str()) == keep);
    for (role, css) in colors.as_object().into_iter().flatten() {
        if Some(role.as_str()) != keep {
            let css = css.as_str().unwrap_or("");
            fields.insert(role.clone(), TextEdit::new(&vivid::hex(css).unwrap_or_else(|| css.to_owned())));
        }
    }
}

/// `updateColor`: advanced edits move the role's family, simple ones regenerate the palette.
fn update_color(s: &mut App, role: &str, value: &str) {
    let dark = s.ui.settings.theme_edit_dark;
    let m = mode(dark);
    let session = session_mut(s);
    let current = session["colors"][m].clone();
    if session["advanced"] == true {
        session["colors"][m] = vivid::update_family(dark, &current, role, value);
        session["regenerate"] = json!(true);
    } else {
        let mut next = current;
        next[role] = json!(value);
        if SIMPLE.iter().any(|(r, _)| *r == role) {
            next = managed(dark, &next);
            session["dirty"][m] = json!(true);
        }
        session["colors"][m] = next;
    }
    sync_fields(s, Some(role));
}

/// `handleAdvancedChange`: leaving advanced mode after an edit regenerates the derived roles.
fn set_advanced(s: &mut App, on: bool) {
    let both = editing_modes(s).is_some_and(|m| m.len() > 1);
    let dark = s.ui.settings.theme_edit_dark;
    let session = session_mut(s);
    session["advanced"] = json!(on);
    if on || session["regenerate"] != true {
        return;
    }
    for d in if both { vec![false, true] } else { vec![dark] } {
        let regenerated = managed(d, &session["colors"][mode(d)]);
        session["colors"][mode(d)] = regenerated;
        session["dirty"][mode(d)] = json!(true);
    }
    session["regenerate"] = json!(false);
    sync_fields(s, None);
}

fn lock_reason(s: &App, dark: bool) -> Option<String> {
    let Some(modes) = editing_modes(s) else {
        // A new theme named like an existing one adds the palette that theme lacks.
        let target = name_target(s)?;
        let label = target["label"].as_str().unwrap_or("");
        return super::modes(&target).contains(&mode(dark)).then(|| format!("\u{201c}{label}\u{201d} already has a {} palette.", mode(dark)));
    };
    let label = s.ui.settings.theme_editor.as_ref().and_then(|t| t["label"].as_str()).unwrap_or("");
    (!modes.contains(&mode(dark))).then(|| format!("\u{201c}{label}\u{201d} has no {} palette. Create a theme with the same name to add one.", mode(dark)))
}

/// Web `themeIdFromName` for names with ASCII letters or digits.
fn id_from_name(name: &str) -> String {
    let dashed: String = name.trim().to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let parts: Vec<&str> = dashed.split('-').filter(|p| !p.is_empty()).collect();
    parts.join("-").chars().take(48).collect::<String>().trim_end_matches('-').to_owned()
}

/// Web `mergeTarget`: another custom theme with this name, or with the id the name would get.
fn merge_target<'a>(library: &'a [Value], editing_id: &Value, name: &str) -> Option<&'a Value> {
    let normalized = name.trim().to_lowercase();
    if normalized.is_empty() {
        return None;
    }
    let id = id_from_name(&normalized);
    library.iter().find(|t| t["id"] != *editing_id && ((!id.is_empty() && t["id"] == id.as_str()) || t["label"].as_str().is_some_and(|l| l.trim().to_lowercase() == normalized)))
}

fn name_target(s: &App) -> Option<Value> {
    let template = s.ui.settings.theme_editor.as_ref()?;
    merge_target(&custom_themes(s), &template["id"], &s.ui.settings.theme_name.text).cloned()
}

/// Web's merge branches of `handleSubmit`: the target theme with the edited palettes added as
/// variants, and the appearance web reports as added, or why the name can't take them.
fn merged(target: &Value, edited: Option<&[&str]>, active: &'static str, advanced: bool, colors: impl Fn(&str) -> Value) -> Result<(Value, &'static str), String> {
    let label = target["label"].as_str().unwrap_or("");
    let taken = super::modes(target);
    let added: Vec<&str> = match edited {
        Some(edited) => {
            if let Some(collision) = edited.iter().copied().find(|m| taken.contains(m)) {
                return Err(format!("\u{201c}{label}\u{201d} already has a {collision} palette. Pick another name."));
            }
            edited.to_vec()
        }
        None if taken.contains(&active) => return Err(format!("\u{201c}{label}\u{201d} already has light and dark palettes. Pick another name.")),
        None => vec![active],
    };
    let mut variants = target["variants"].as_object().cloned().unwrap_or_default();
    for m in &added {
        variants.insert((*m).to_owned(), colors(*m));
    }
    let mut theme = json!({"id": target["id"], "label": target["label"], "appearance": target["appearance"], "colors": target["colors"], "variants": variants});
    if target["managed"] == true && !advanced {
        theme["managed"] = json!(true);
    }
    if target["collection"].is_object() {
        theme["collection"] = target["collection"].clone();
    }
    Ok((theme, if added.first() == Some(&"dark") { "dark" } else { "light" }))
}

/// Web switches a new theme off an appearance its name's theme already has.
fn follow_name(s: &mut App) {
    if editing_modes(s).is_some() {
        return;
    }
    let taken = name_target(s).map(|t| super::modes(&t)).unwrap_or_default();
    let dark = s.ui.settings.theme_edit_dark;
    if taken.len() == 1 && taken[0] == mode(dark) {
        s.ui.settings.theme_edit_dark = !dark;
        sync_fields(s, None);
    }
}

fn set_appearance(s: &mut App, dark: bool) {
    if lock_reason(s, dark).is_none() {
        s.ui.settings.theme_edit_dark = dark;
        sync_fields(s, None);
    }
}

/// `colorsForSave`: guided mode regenerates the palettes it changed.
fn save_colors(session: &Value, m: &str) -> Value {
    let current = &session["colors"][m];
    if session["advanced"] != true && session["dirty"][m] == true { managed(m == "dark", current) } else { current.clone() }
}

/// `handleSubmit` without the merge-by-name path: an edit keeps the theme's appearances and
/// collection, a new theme takes the appearance being edited.
fn saved_theme(template: &Value, session: &Value, name: &str, dark: bool, editing: bool) -> Value {
    let advanced = session["advanced"] == true;
    let colors = |m: &str| save_colors(session, m);
    let mut theme = if editing {
        let base = if template["appearance"] == "dark" { "dark" } else { "light" };
        let mut theme = json!({"id": template["id"], "label": name, "appearance": base, "colors": colors(base)});
        if modes(template).len() > 1 {
            let other = if base == "light" { "dark" } else { "light" };
            theme["variants"] = Value::Object(serde_json::Map::from_iter([(other.to_owned(), colors(other))]));
        }
        if template["collection"].is_object() {
            theme["collection"] = template["collection"].clone();
        }
        theme
    } else {
        json!({"id": template["id"], "label": name, "appearance": mode(dark), "colors": colors(mode(dark))})
    };
    if !advanced {
        theme["managed"] = json!(true);
    }
    theme
}

fn save(s: &mut App) {
    let name = s.ui.settings.theme_name.text.trim().to_owned();
    if name.is_empty() {
        session_mut(s)["error"] = json!("Name your theme first.");
        return;
    }
    let Some(template) = s.ui.settings.theme_editor.clone() else { return };
    let dark = s.ui.settings.theme_edit_dark;
    let mut library = custom_themes(s);
    let editing = library.iter().any(|t| t["id"] == template["id"]);
    if let Some(target) = merge_target(&library, &template["id"], &name).cloned() {
        let edited = library.iter().find(|t| t["id"] == template["id"]).map(modes);
        let session = session(s).clone();
        match merged(&target, edited.as_deref(), mode(dark), session["advanced"] == true, |m| save_colors(&session, m)) {
            Err(error) => session_mut(s)["error"] = json!(error),
            Ok((theme, appearance)) => save_merge(s, library, &template["id"], theme, appearance),
        }
        return;
    }
    let theme = saved_theme(&template, session(s), &name, dark, editing);
    let id = theme["id"].clone();
    match library.iter_mut().find(|t| t["id"] == id) {
        Some(existing) => *existing = theme,
        None => library.push(theme),
    }
    crate::settings::update(s, "customThemes", json!(library));
    if editing {
        let active = ["themeLight", "themeDark"].iter().any(|k| s.client_settings.values.get(*k) == Some(&id));
        let description = if active { "Your changes are now active." } else { "Your changes are saved." };
        crate::shell::palette::toast(s, &format!("{name} saved"), Some(description));
    } else {
        crate::settings::update(s, if dark { "themeDark" } else { "themeLight" }, id);
        crate::shell::palette::toast(s, &format!("{name} created"), Some("It\u{2019}s now active."));
    }
    close(s);
}

/// Saves a merge, retires the edited theme it absorbed, and makes the result active like web's
/// `setTheme`.
fn save_merge(s: &mut App, mut library: Vec<Value>, retired: &Value, theme: Value, appearance: &str) {
    let id = theme["id"].clone();
    let label = theme["label"].as_str().unwrap_or("").to_owned();
    library.retain(|t| t["id"] != *retired);
    if let Some(existing) = library.iter_mut().find(|t| t["id"] == id) {
        *existing = theme;
    }
    crate::settings::update(s, "customThemes", json!(library));
    for key in ["themeLight", "themeDark"] {
        if s.client_settings.values.get(key) == Some(retired) {
            crate::settings::update(s, key, id.clone());
        }
    }
    crate::settings::update(s, if appearance == "dark" { "themeDark" } else { "themeLight" }, id);
    if s.client_settings.error.is_some() {
        crate::shell::palette::toast_error(s, "Could not save your theme", Some("Browser storage is unavailable, so the change was not kept."));
        return;
    }
    crate::shell::palette::toast(s, &format!("{label} updated"), Some(&format!("Its {appearance} palette was added.")));
    close(s);
}

fn pair(v: &Value) -> Option<[f32; 2]> {
    Some([v[0].as_f64()? as f32, v[1].as_f64()? as f32])
}

/// `clampPanelPosition`: the header stays reachable inside the window.
fn clamp_pos([x, y]: [f32; 2], width: f32, [vw, vh]: [f32; 2]) -> [f32; 2] {
    [x.clamp(MARGIN, (vw - width - MARGIN).max(MARGIN)), y.clamp(MARGIN, (vh - 48.0).max(MARGIN))]
}

/// The resize handle's bounds: the minimum size, and the window's edge past the panel's corner.
/// Web's `max-h-[min(42rem,calc(100dvh-6rem))]`, never below the resize minimum.
fn default_height(window: Option<[f32; 2]>) -> f32 {
    window.map_or(672.0, |[_, vh]| (vh - 96.0).clamp(MIN_H, 672.0))
}

fn clamp_size([w, h]: [f32; 2], [x, y]: [f32; 2], [vw, vh]: [f32; 2]) -> [f32; 2] {
    [w.clamp(MIN_W, (vw - MARGIN - x).max(MIN_W)), h.clamp(MIN_H, (vh - MARGIN - y).max(MIN_H))]
}

/// Where the panel sits: dragged once, it keeps its place inside the window.
fn placed(s: &App) -> Option<[f32; 2]> {
    let session = session(s);
    let pos = pair(&session["pos"])?;
    let width = pair(&session["size"]).map_or(WIDTH, |[w, _]| w);
    Some(s.ui.desktop.window_size().map_or(pos, |window| clamp_pos(pos, width, window)))
}

/// A gesture starts from where the panel is now, pinned or placed.
fn anchor(s: &mut App) {
    let from = placed(s).map_or_else(|| json!([session(s)["rect"][0], session(s)["rect"][1]]), |p| json!(p));
    let session = session_mut(s);
    session["pos"] = from.clone();
    session["grab"] = from;
}

fn drag(s: &mut App, d: &rune::Drag) {
    if d.phase == Phase::Down {
        anchor(s);
    }
    if let Some([x, y]) = pair(&session(s)["grab"]) {
        let width = pair(&session(s)["size"]).map_or(WIDTH, |[w, _]| w);
        let next = [x + d.dx, y + d.dy];
        let next = s.ui.desktop.window_size().map_or([next[0].max(MARGIN), next[1].max(MARGIN)], |window| clamp_pos(next, width, window));
        session_mut(s)["pos"] = json!(next);
    }
}

fn resize(s: &mut App, d: &rune::Drag) {
    if d.phase == Phase::Down {
        anchor(s);
        let rect = &session(s)["rect"];
        let start = json!([rect[2], rect[3]]);
        session_mut(s)["grow"] = start;
    }
    let (Some([w, h]), Some(pos)) = (pair(&session(s)["grow"]), pair(&session(s)["pos"])) else { return };
    let next = [w + d.dx, h + d.dy];
    let next = s.ui.desktop.window_size().map_or([next[0].max(MIN_W), next[1].max(MIN_H)], |window| clamp_size(next, pos, window));
    session_mut(s)["size"] = json!(next);
}

/// The role an inspected element shows, from the keyed regions on its path, innermost first.
fn inspected_role(path: &str) -> &'static str {
    for segment in path.rsplit('/') {
        let role = match segment {
            k if k.starts_with("terminal") => "terminalBackground",
            k if k.starts_with("sb-row") => "sidebarRowSelected",
            k if k == "sidebar" || k.starts_with("sb-") => "sidebar",
            k if k.starts_with("composer") => "input",
            k if k.starts_with("topbar") => "toolbar",
            "code" | "file-editor" => "codeBackground",
            k if k.starts_with("toast") => "surfaceOverlay",
            "settings-modal" => "surface",
            "main" | "shell" => "canvas",
            _ => continue,
        };
        return role;
    }
    "canvas"
}

/// `getThemeRoleLabel`: a few roles read differently; the rest split their camelCase.
fn role_label(role: &str) -> String {
    const LABELS: [(&str, &str); 14] = [
        ("canvas", "Background"),
        ("toolbar", "Toolbar background"),
        ("toolbarForeground", "Toolbar text"),
        ("toolbarBorder", "Toolbar border"),
        ("toolbarControl", "Toolbar control"),
        ("toolbarControlForeground", "Toolbar control text"),
        ("toolbarControlHover", "Toolbar control hover"),
        ("accent", "Accent color"),
        ("errorForeground", "Error text"),
        ("errorSurface", "Error background"),
        ("warningForeground", "Warning text"),
        ("warningSurface", "Warning background"),
        ("updateForeground", "Update text"),
        ("updateSurface", "Update background"),
    ];
    if let Some((_, label)) = LABELS.iter().find(|(r, _)| *r == role) {
        return (*label).to_owned();
    }
    let mut out = String::new();
    for (i, c) in role.chars().enumerate() {
        if i == 0 {
            out.extend(c.to_uppercase());
        } else if c.is_ascii_uppercase() {
            out.push(' ');
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out
}

/// The family filter: its label and its roles' labels, as web's `renderColorFields`.
fn family_matches(label: &str, roles: &[&str], query: &str) -> bool {
    query.is_empty() || std::iter::once(label.to_owned()).chain(roles.iter().map(|r| role_label(r))).collect::<Vec<_>>().join(" ").to_lowercase().contains(query)
}

fn family_label(role: &str) -> &'static str {
    GROUPS.iter().flat_map(|(_, families)| families.iter()).find(|(_, shown, _)| *shown == role).map_or("Background", |&(label, _, _)| label)
}

/// The family whose field shows `role`.
fn family_role(role: &str) -> &'static str {
    GROUPS.iter().flat_map(|(_, families)| families.iter()).find(|(_, _, roles)| roles.contains(&role)).map_or("canvas", |&(_, shown, _)| shown)
}

/// `selectThemeRole` with reveal: a role outside the simple pair opens advanced, unfiltered.
fn select_role(s: &mut App, role: &str, tx: &mut rune::Tx) {
    let shown = family_role(role);
    let session = session_mut(s);
    session["selected"] = json!(shown);
    session["inspecting"] = json!(false);
    if !SIMPLE.iter().any(|(r, _)| *r == shown) {
        set_advanced(s, true);
        s.ui.settings.edits.insert(FILTER.to_owned(), TextEdit::new(""));
    }
    tx.scroll_into_view(format!("theme-color-{shown}"));
}

fn inspect(s: &mut App, on: bool) {
    let session = session_mut(s);
    session["inspecting"] = json!(on);
    if !on {
        session["selected"] = Value::Null;
    }
}

/// Settings' Escape: cancels inspecting, else closes an open editor.
pub fn escape(s: &mut App) -> bool {
    let open = s.ui.settings.theme_editor.is_some();
    if open {
        dismiss(s);
    }
    open
}

/// Escape cancels inspecting before it closes the editor.
fn dismiss(s: &mut App) {
    if s.ui.settings.theme_picker.is_some() {
        cancel_pick(s);
    } else if session(s)["inspecting"] == true {
        inspect(s, false);
    } else {
        close(s);
    }
}

/// While inspecting, a press over the app picks its region instead of reaching it.
fn inspector(s: &App) -> El {
    let scope = if matches!(s.route, Route::Settings(_)) { "settings-modal" } else { "shell" };
    el().key("theme-inspect")
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .cursor(rune::Cursor::Crosshair)
        .on_pointer_move_local(|s: &mut App, e: rune::PointerEvent, _: &mut rune::Tx| session_mut(s)["pointer"] = json!([e.window_x, e.window_y]))
        .on_drag(move |s: &mut App, d: &mut rune::Drag, tx: &mut rune::Tx| {
            if d.phase != Phase::Down {
                return;
            }
            let Some([x, y]) = pair(&session(s)["pointer"]) else { return };
            tx.element_at_in(scope, x, y, |s: &mut App, picked: Option<rune::Picked>, tx: &mut rune::Tx| {
                if let Some(picked) = picked {
                    select_role(s, inspected_role(picked.path()), tx);
                }
            });
        })
}

/// The session's colors for the appearance being edited, which `apply` paints over the app.
pub(super) fn preview(s: &App) -> Option<(bool, &Value)> {
    s.ui.settings.theme_editor.as_ref()?;
    let dark = s.ui.settings.theme_edit_dark;
    let colors = &session(s)["colors"][mode(dark)];
    colors.is_object().then_some((dark, colors))
}

fn filter_text(s: &App) -> String {
    s.ui.settings.edits.get(FILTER).map(|e| e.text.clone()).unwrap_or_else(|| crate::settings::value(s, FILTER).as_str().unwrap_or("").to_owned())
}

fn field(s: &App, t: &T, role: &'static str, label: &str) -> El {
    let css = session(s)["colors"][mode(s.ui.settings.theme_edit_dark)][role].as_str().unwrap_or("");
    let edit = s.ui.settings.theme_colors.get(role).cloned().unwrap_or_else(|| TextEdit::new(css));
    let input = kit::Input::new(&edit)
        .placeholder(label)
        .size("sm")
        .font("mono")
        .invalid(color(edit.text.trim()).is_none())
        .on_edit(move |s: &mut App| s.ui.settings.theme_colors.entry(role.to_owned()).or_default())
        .on_change(move |s: &mut App, _| {
            let value = s.ui.settings.theme_colors.get(role).map(|e| e.text.trim().to_owned()).unwrap_or_default();
            if color(&value).is_some() {
                update_color(s, role, &value);
            }
        })
        .build(t)
        .single_line()
        .w(128.0);
    let selected = session(s)["selected"].as_str() == Some(role);
    let out = row()
        .key(format!("theme-color-{role}"))
        .w_full()
        .min_h(32.0)
        .gap(10.0)
        .center_y()
        .radius(6.0)
        .child(text(label.to_owned()).font(theme::TEXT_SM).color(t.c.foreground).grow(1.0).min_w(0.0).truncate())
        .child(swatch(s, t, role, label, css))
        .child(input);
    if selected { out.outline(2.0, 2.0, t.c.ring) } else { out }
}

/// Web `ThemeColorPicker`'s open popover: the role it edits, its colour as HSV, the text drafts,
/// the alpha the swatch keeps, and the colours Escape restores.
pub struct Picker {
    role: &'static str,
    label: String,
    hsv: (f32, f32, f32),
    hex: TextEdit,
    rgb: TextEdit,
    alpha: String,
    before: Value,
}

/// `normalizeThemePickerColor` and `themePickerAlphaSuffix`: the opaque `#rrggbb`, and a
/// non-opaque alpha byte to keep on every pick.
fn picker_color(css: &str) -> (String, String) {
    let alpha = css.trim().strip_prefix('#').filter(|d| d.len() == 8 && d.chars().all(|c| c.is_ascii_hexdigit())).map(|d| d[6..].to_lowercase()).filter(|a| a != "ff");
    (vivid::hex(css).unwrap_or_else(|| "#000000".into()), alpha.unwrap_or_default())
}

fn rgb_of(hex: &str) -> String {
    let n = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0);
    format!("{}, {}, {}", n >> 16, (n >> 8) & 255, n & 255)
}

/// `themeRgbToHex`: three whole channels from 0 to 255, bare or inside `rgb()`.
fn rgb_to_hex(value: &str) -> Option<String> {
    let mut v = value.trim();
    if v.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("rgb(")) {
        v = v[4..].trim_start();
    }
    if let Some(inner) = v.strip_suffix(')') {
        v = inner.trim_end();
    }
    let channels: Vec<u8> = v.split(|c: char| c == ',' || c.is_whitespace()).filter(|c| !c.is_empty()).map(|c| c.parse().ok()).collect::<Option<_>>()?;
    let [r, g, b] = channels[..] else { return None };
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}

/// The swatch beside a role's hex field, which opens its picker.
fn swatch(s: &App, t: &T, role: &'static str, label: &str, css: &str) -> El {
    let open = s.ui.settings.theme_picker.as_ref().is_some_and(|p| p.role == role);
    let name = format!("Choose {label} color");
    let label = label.to_owned();
    row().key(format!("theme-picker-{role}-trigger")).child(
        el().size(24.0, 24.0)
            .radius(12.0)
            .border(1.0, t.c.foreground.alpha_mul(0.3))
            .fill(color(css).unwrap_or(rune::Color::BLACK))
            .focusable(true)
            .focus_ring(2.0, -2.0, t.c.ring)
            .role("button")
            .label(name.clone())
            .help(name)
            .expanded(open)
            .on_press(move |s: &mut App| {
                if !s.ui.settings.theme_picker.take().is_some_and(|p| p.role == role) {
                    open_picker(s, role, &label);
                }
            }),
    )
}

fn open_picker(s: &mut App, role: &'static str, label: &str) {
    let colors = session(s)["colors"][mode(s.ui.settings.theme_edit_dark)].clone();
    let (hex, alpha) = picker_color(colors[role].as_str().unwrap_or(""));
    s.ui.settings.theme_picker = Some(Picker {
        role,
        label: label.to_owned(),
        hsv: crate::settings::accent::hex_to_hsv(&hex),
        rgb: TextEdit::new(rgb_of(&hex)),
        hex: TextEdit::new(hex),
        alpha,
        before: colors,
    });
}

/// A pick from the plane, the hue slider, or a text field (`edited`, whose draft keeps its keys).
fn pick(s: &mut App, hsv: (f32, f32, f32), edited: Option<&str>) {
    let Some(p) = s.ui.settings.theme_picker.as_mut() else { return };
    p.hsv = hsv;
    let hex = crate::settings::accent::hsv_to_hex(hsv.0, hsv.1, hsv.2);
    if edited != Some("hex") {
        p.hex = TextEdit::new(hex.clone());
    }
    if edited != Some("rgb") {
        p.rgb = TextEdit::new(rgb_of(&hex));
    }
    let (role, value) = (p.role, format!("{hex}{}", p.alpha));
    update_color(s, role, &value);
    s.ui.settings.theme_colors.insert(role.to_owned(), TextEdit::new(value));
}

/// Escape reverts the role to its colours before the picker opened (web's `onEditCancel`).
fn cancel_pick(s: &mut App) {
    let Some(p) = s.ui.settings.theme_picker.take() else { return };
    let m = mode(s.ui.settings.theme_edit_dark);
    session_mut(s)["colors"][m] = p.before;
    sync_fields(s, None);
}

const PICKER_W: f32 = 264.0;

fn picker_panel(t: &T, p: &Picker) -> El {
    let (h, sat, v) = p.hsv;
    let current = crate::settings::accent::hsv_to_hex(h, sat, v);
    let swatch_color = color(&current).unwrap_or(rune::Color::BLACK);
    let (white, black) = (rune::Color::WHITE, rune::Color::BLACK);
    let thumb = |size: f32| el().size(size, size).radius(size / 2.0).border(2.0, white).shadow(0.0, 1.0, black.alpha(0.4));
    let header = row()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(12.0)
        .center_y()
        .child(
            col().grow(1.0).min_w(0.0)
                .child(text(p.label.clone()).font(theme::weight(theme::TEXT_XS, theme::SEMIBOLD)).color(t.c.foreground).truncate())
                .child(text("Choose a color").font(theme::TEXT_2XS).color(t.c.muted_foreground)),
        )
        .child(el().size(28.0, 28.0).radius(14.0).fill(swatch_color).shadow(0.0, 1.0, black.alpha(0.2)));
    let plane = el()
        .key("theme-picker-plane")
        .role("slider")
        .label(p.label.clone())
        .size(PICKER_W, 144.0)
        .radius(8.0)
        .clip_rounded(8.0)
        .cursor(rune::Cursor::Crosshair)
        .fill(color(&crate::settings::accent::hsv_to_hex(h, 1.0, 1.0)).unwrap_or(black))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).linear_gradient(std::f32::consts::FRAC_PI_2, [(0.0, white), (1.0, white.alpha(0.0))]))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).gradient(black.alpha(0.0), black))
        .child(thumb(12.0).absolute(sat * PICKER_W - 6.0, (1.0 - v) * 144.0 - 6.0))
        .on_drag(|s: &mut App, d: &mut rune::Drag, _| {
            if d.cancelled() {
                return;
            }
            let hue = s.ui.settings.theme_picker.as_ref().map_or(0.0, |p| p.hsv.0);
            pick(s, (hue, (d.x / d.w).clamp(0.0, 1.0), 1.0 - (d.y / d.h).clamp(0.0, 1.0)), None);
        });
    let stops: Vec<(f32, rune::Color)> = (0..=6).map(|i| (i as f32 / 6.0, color(&crate::settings::accent::hsv_to_hex(i as f32 * 60.0, 1.0, 1.0)).unwrap_or(black))).collect();
    let hue = row()
        .key("theme-picker-hue")
        .role("slider")
        .label(format!("{} hue", p.label))
        .range_value(h as f64, 0.0, 360.0)
        .w_full()
        .h(24.0)
        .center_y()
        .cursor(rune::Cursor::Pointer)
        .child(el().w_full().h(10.0).radius(5.0).linear_gradient(std::f32::consts::FRAC_PI_2, stops).border(1.0, black.alpha(0.12)))
        .child(thumb(16.0).absolute(h / 360.0 * PICKER_W - 8.0, 4.0))
        .on_drag(|s: &mut App, d: &mut rune::Drag, _| {
            if d.cancelled() {
                return;
            }
            let Some((_, sat, v)) = s.ui.settings.theme_picker.as_ref().map(|p| p.hsv) else { return };
            pick(s, ((d.x / d.w).clamp(0.0, 1.0) * 360.0, sat, v), None);
        });
    let caption = |name: &str| text(name.to_owned()).font(theme::weight(theme::TEXT_3XS, theme::SEMIBOLD)).color(t.c.muted_foreground).px(4.0);
    let hex = kit::Input::new(&p.hex)
        .size("compact")
        .font("mono")
        .on_edit(|s: &mut App| &mut s.ui.settings.theme_picker.get_or_insert_with(Picker::blank).hex)
        .on_change(|s: &mut App, _: &mut rune::Tx| {
            let Some(text) = s.ui.settings.theme_picker.as_ref().map(|p| p.hex.text.clone()) else { return };
            if text.len() == 7 && crate::settings::accent::normalize(&text).is_some() {
                pick(s, crate::settings::accent::hex_to_hsv(&text.to_lowercase()), Some("hex"));
            }
        })
        .build(t)
        .label(format!("{} picker hex value", p.label))
        .single_line()
        .grow(1.0)
        .min_w(0.0);
    let rgb = kit::Input::new(&p.rgb)
        .size("compact")
        .font("mono")
        .on_edit(|s: &mut App| &mut s.ui.settings.theme_picker.get_or_insert_with(Picker::blank).rgb)
        .on_change(|s: &mut App, _: &mut rune::Tx| {
            let Some(text) = s.ui.settings.theme_picker.as_ref().map(|p| p.rgb.text.clone()) else { return };
            if let Some(hex) = rgb_to_hex(&text) {
                pick(s, crate::settings::accent::hex_to_hsv(&hex), Some("rgb"));
            }
        })
        .build(t)
        .label(format!("{} picker RGB value", p.label))
        .single_line()
        .w_full();
    let texts = row()
        .w_full()
        .gap(8.0)
        .child(
            col().grow(1.0).min_w(0.0).gap(4.0).child(caption("HEX")).child(
                row().w_full().gap(6.0).center_y().child(el().size(14.0, 14.0).radius(7.0).fill(swatch_color)).child(hex),
            ),
        )
        .child(col().grow(1.2).min_w(0.0).gap(4.0).child(caption("RGB")).child(rgb));
    col()
        .w(PICKER_W + 24.0)
        .child(header)
        .child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.7)))
        .child(col().w_full().px(12.0).py(12.0).gap(12.0).child(plane).child(hue).child(texts))
}

impl Picker {
    fn blank() -> Self {
        Picker { role: "", label: String::new(), hsv: (0.0, 0.0, 0.0), hex: TextEdit::new(""), rgb: TextEdit::new(""), alpha: String::new(), before: Value::Null }
    }
}

/// The open picker under its swatch. A press outside keeps the colour; Escape reverts it.
fn picker(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(p) = &s.ui.settings.theme_picker else { return };
    layers.push(rune_kit::scrim("theme-picker-scrim".to_owned(), |s: &mut App| s.ui.settings.theme_picker = None));
    let _trigger = kit::Popover::new(format!("theme-picker-{}", p.role))
        .open(true)
        .padding(0.0)
        .gutter(10.0)
        .placement(kit::Placement::BottomEnd)
        .label(format!("Choose {} color", p.label))
        .child(picker_panel(t, p).modal(true, cancel_pick))
        .build(&kit::theme(t), layers);
}

fn fields(s: &App, t: &T, advanced: bool) -> El {
    if !advanced {
        return col().w_full().gap(4.0).children(SIMPLE.iter().map(|&(role, label)| field(s, t, role, label)));
    }
    let query = filter_text(s).trim().to_lowercase();
    let mut out = col().w_full().gap(20.0);
    let mut any = false;
    for (title, families) in GROUPS {
        let shown: Vec<&Family> = families.iter().filter(|(label, _, roles)| family_matches(label, roles, &query)).collect();
        if shown.is_empty() {
            continue;
        }
        any = true;
        let mut group = col().w_full().gap(4.0).child(text(title).font(theme::SM_MEDIUM).color(t.c.foreground).mb(4.0));
        for &(label, role, _) in shown {
            group = group.child(field(s, t, role, label));
        }
        out = out.child(group);
    }
    if !any {
        out = out.child(text("No matches.").font(theme::TEXT_XS).color(t.c.muted_foreground));
    }
    out
}

fn body(s: &App, t: &T, editing: Option<&[&'static str]>) -> El {
    let state = session(s);
    let advanced = state["advanced"] == true;
    let k = kit::theme(t);
    let name = col().w_full().gap(6.0).child(text("Theme name").font(theme::SM_MEDIUM).color(t.c.foreground)).child(
        kit::Input::new(&s.ui.settings.theme_name)
            .placeholder(if editing.is_some() { "Theme name" } else { "e.g. Aurora" })
            .size("sm")
            .on_edit(|s: &mut App| &mut s.ui.settings.theme_name)
            .on_change(|s: &mut App, _| {
                session_mut(s)["error"] = Value::Null;
                follow_name(s);
            })
            .build(t)
            .single_line(),
    );
    let dark = s.ui.settings.theme_edit_dark;
    let reasons = [lock_reason(s, false), lock_reason(s, true)];
    let mut appearance = kit::ToggleGroup::new(&["Light", "Dark"], &[usize::from(dark)])
        .variant("segmented")
        .on_pick(|s: &mut App, index: usize, _: &mut rune::Tx| set_appearance(s, index == 1));
    if reasons.iter().any(Option::is_some) {
        appearance = appearance.tips(&reasons.clone().map(Option::unwrap_or_default));
    }
    for (index, reason) in reasons.iter().enumerate() {
        if reason.is_some() {
            appearance = appearance.item_state(index, kit::State::Disabled);
        }
    }
    let mut heading = col().grow(1.0).min_w(0.0).child(text("Colors").font(theme::SM_MEDIUM).color(t.c.foreground));
    if !advanced {
        heading = heading.child(text("Two colors, rest derived").font(theme::TEXT_XS).color(t.c.muted_foreground));
    }
    let mut controls = row().gap(12.0).center_y();
    if advanced {
        controls = controls.child(crate::settings::controls::input(s, t, FILTER, 150.0, "Filter colors", false));
    }
    controls = controls.child(
        row().gap(8.0).center_y().child(text("Advanced").font(theme::SM_MEDIUM).color(t.c.foreground)).child(
            kit::Switch::new(advanced)
                .label("Use advanced theme colors")
                .hide_label()
                .on_toggle(|s: &mut App| {
                    let on = session(s)["advanced"] != true;
                    set_advanced(s, on);
                })
                .build(&k),
        ),
    );
    let mut out = col().w_full().gap(12.0).child(name);
    if let Some(error) = state["error"].as_str() {
        out = out.child(text(error.to_owned()).role("status").font(theme::TEXT_SM).color(t.c.destructive).wrap());
    }
    out.child(row().w_full().gap(12.0).center_y().child(text("Appearance").font(theme::SM_MEDIUM).color(t.c.foreground).grow(1.0)).child(appearance.build(&k).label("Theme appearance")))
        .child(row().w_full().gap(12.0).items_start().child(heading).child(controls))
        .child(fields(s, t, advanced))
}

/// The floating editor. Inside Settings it is modal, since the settings sheet already traps
/// focus; over the app it stays non-modal so the preview can be used behind it.
pub fn editor(s: &App, t: &T, layers: &mut Vec<El>) {
    if s.ui.settings.theme_editor.is_none() {
        return;
    }
    let state = session(s);
    let editing = editing_modes(s);
    let title = if editing.is_some() { "Edit theme" } else { "Create theme" };
    let minimized = state["minimized"] == true;
    let inspecting = state["inspecting"] == true;
    let mut heading = row().gap(8.0).min_w(0.0).center_y().child(text(title).font(theme::SM_MEDIUM).color(t.c.foreground));
    if !minimized {
        let hint = if inspecting { "Select an element \u{b7} Esc to cancel" } else { "Select a color below" };
        let hint = state["selected"].as_str().filter(|_| !inspecting).map_or_else(|| hint.to_owned(), |role| family_label(role).to_owned());
        heading = heading.child(text(hint).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().shrink());
    }
    let header = row()
        .w_full()
        .h(44.0)
        .pl(12.0)
        .pr(8.0)
        .gap(4.0)
        .center_y()
        .child(
            row()
                .grow(1.0)
                .min_w(0.0)
                .h_full()
                .center_y()
                .cursor(rune::Cursor::Grab)
                .on_drag(|s: &mut App, d, _| drag(s, d))
                .child(heading),
        )
        .child(
            Button::new(if inspecting { "Cancel" } else { "Inspect" })
                .icon(&icons::MOUSE_POINTER_2)
                .size("xs")
                .variant(if inspecting { "secondary" } else { "ghost" })
                .on_press(move |s: &mut App| inspect(s, !inspecting))
                .build(t)
                .label(if inspecting { "Cancel inspecting app colors" } else { "Inspect app colors" })
                .help(if inspecting { "Cancel and clear the selection" } else { "Pick a color from the app" }),
        )
        .child(
            Button::new(if minimized { "Expand the theme editor" } else { "Minimize the theme editor" })
                .icon(if minimized { &icons::CHEVRON_UP } else { &icons::CHEVRON_DOWN })
                .size("icon-xs")
                .variant("ghost")
                .on_press(move |s: &mut App| session_mut(s)["minimized"] = json!(!minimized))
                .build(t),
        )
        .child(Button::new("Close the theme editor").icon(&icons::X).size("icon-xs").variant("ghost").on_press(close).build(t));
    let size = pair(&state["size"]);
    let mut panel = col()
        .key("theme-editor")
        .w(size.map_or(WIDTH, |[w, _]| w))
        .radius(12.0)
        .clip_rounded(12.0)
        .border(1.0, t.c.border)
        .fill(t.c.popover)
        .shadow(16.0, 40.0, rune::Color::BLACK.alpha(0.3))
        .role("dialog")
        .label(title)
        .on_layout(|s: &mut App, rect| session_mut(s)["rect"] = json!(rect))
        .child(header);
    panel = match size {
        Some([_, h]) if !minimized => panel.h(h),
        // The scrolling body has no intrinsic height, so the open panel takes web's cap.
        _ if !minimized => panel.h(default_height(s.ui.desktop.window_size())),
        _ => panel,
    };
    if !minimized {
        let save_button = Button::new(if editing.is_some() { "Save changes" } else { "Create theme" }).size("sm").disabled(s.ui.settings.theme_name.text.trim().is_empty()).on_press(save);
        let save_button = if editing.is_some() { save_button } else { save_button.icon(&icons::PAINTBRUSH) };
        panel = panel
            .child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.7)))
            .child(col().w_full().grow(1.0).min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).py(12.0).pl(12.0).pr(6.0).child(body(s, t, editing.as_deref())))
            .child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.7)))
            .child(
                row()
                    .w_full()
                    .px(12.0)
                    .py(8.0)
                    .gap(8.0)
                    .justify_end()
                    .child(Button::new("Cancel").size("sm").variant("ghost").on_press(close).build(t))
                    .child(save_button.build(t)),
            )
            .child(
                el().key("theme-editor-resize")
                    .pin(None, None, Some(0.0), Some(0.0))
                    .size(20.0, 20.0)
                    .cursor(rune::Cursor::Move)
                    .label("Resize the theme editor")
                    .on_drag(|s: &mut App, d, _| resize(s, d)),
            );
    }
    panel = match placed(s) {
        Some([x, y]) => panel.absolute(x, y),
        _ => panel.pin(None, None, Some(16.0), Some(16.0)),
    };
    // Modal inside Settings traps focus, but inspecting needs the presses below it.
    panel = if matches!(s.route, Route::Settings(_)) && !inspecting {
        panel.modal(false, dismiss)
    } else {
        panel.on_key(|s: &mut App, e: &rune::KeyEvent, _: &mut rune::Tx| {
            let escape = e.key == rune::Key::Escape;
            if escape {
                dismiss(s);
            }
            escape
        })
    };
    if inspecting {
        layers.push(inspector(s));
    }
    layers.push(panel);
    picker(s, t, layers);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_same_named_theme_gains_the_new_palette() {
        let night = json!({"id": "night", "label": "Night", "appearance": "dark", "colors": {"background": "#000"}, "managed": true});
        let library = vec![night.clone(), json!({"id": "dawn", "label": "Dawn", "appearance": "light", "colors": {}})];
        assert_eq!(id_from_name("  Night Owl!! "), "night-owl");
        assert_eq!(merge_target(&library, &json!("new"), " night ").map(|t| &t["id"]), Some(&json!("night")));
        assert_eq!(merge_target(&library, &json!("night"), "Night"), None, "a theme never merges into itself");
        let colors = |m: &str| json!({"background": if m == "light" { "#fff" } else { "#111" }});
        let (theme, added) = merged(&night, None, "light", false, colors).unwrap();
        assert_eq!(added, "light");
        assert_eq!(theme["variants"]["light"], json!({"background": "#fff"}));
        assert_eq!((&theme["appearance"], &theme["colors"], &theme["managed"]), (&json!("dark"), &json!({"background": "#000"}), &json!(true)));
        assert_eq!(merged(&night, None, "dark", false, colors).unwrap_err(), "\u{201c}Night\u{201d} already has light and dark palettes. Pick another name.");
        // Renaming an edited dark theme onto it collides with its dark palette.
        assert_eq!(merged(&night, Some(&["dark"]), "dark", true, colors).unwrap_err(), "\u{201c}Night\u{201d} already has a dark palette. Pick another name.");
        let (theme, _) = merged(&night, Some(&["light"]), "light", true, colors).unwrap();
        assert!(theme.get("managed").is_none(), "an advanced edit leaves the theme unmanaged");
    }

    #[test]
    fn the_color_picker_reads_rgb_like_web_and_escape_reverts_its_edit() {
        assert_eq!(rgb_to_hex("12, 34, 255").as_deref(), Some("#0c22ff"));
        assert_eq!(rgb_to_hex("rgb( 1 2 3 )").as_deref(), Some("#010203"));
        assert_eq!(rgb_to_hex("RGB(1,2,3)").as_deref(), Some("#010203"));
        for bad in ["256, 0, 0", "1.5, 0, 0", "1, 2", "-1, 0, 0", ""] {
            assert_eq!(rgb_to_hex(bad), None, "{bad}");
        }
        assert_eq!(rgb_of("#0c22ff"), "12, 34, 255");
        assert_eq!(picker_color("#11223380").1, "80", "a translucent role keeps its alpha");
        assert_eq!(picker_color("#112233ff").1, "", "an opaque one has none to keep");

        let mut s = App::new();
        create(&mut s, true);
        let before = session(&s)["colors"]["dark"].clone();
        open_picker(&mut s, "accent", "Accent color");
        pick(&mut s, (0.0, 1.0, 1.0), None);
        assert_eq!(s.ui.settings.theme_colors["accent"].text, "#ff0000", "the role's hex field follows the picker");
        assert_eq!(s.ui.settings.theme_picker.as_ref().map(|p| p.rgb.text.as_str()), Some("255, 0, 0"));
        assert_ne!(session(&s)["colors"]["dark"], before);
        dismiss(&mut s);
        assert!(s.ui.settings.theme_picker.is_none());
        assert!(s.ui.settings.theme_editor.is_some(), "FAILED: Escape closes the picker before the editor");
        assert_eq!(session(&s)["colors"]["dark"], before, "FAILED: Escape restores the colors from before the picker opened");
    }

    #[test]
    fn the_color_filter_reads_role_labels_like_web() {
        assert_eq!(role_label("toolbarForeground"), "Toolbar text");
        assert_eq!(role_label("sidebarBorder"), "Sidebar Border", "others split their camelCase");
        let text: Vec<&str> = vec!["text", "toolbarForeground", "toolbarControlForeground"];
        assert!(family_matches("Text", &text, "toolbar text"), "a role's label matches");
        assert!(!family_matches("Text", &text, "toolbarforeground"), "its id doesn't");
        assert!(family_matches("Text", &text, ""));
    }

    #[test]
    fn the_panel_stays_inside_the_window_and_resizes_within_it() {
        assert_eq!(default_height(Some([1200.0, 900.0])), 672.0, "42rem on a tall window");
        assert_eq!(default_height(Some([1200.0, 600.0])), 504.0, "the viewport less 6rem");
        assert_eq!(default_height(Some([800.0, 260.0])), MIN_H);
        let window = [1200.0, 800.0];
        assert_eq!(clamp_pos([-40.0, -5.0], WIDTH, window), [MARGIN, MARGIN]);
        assert_eq!(clamp_pos([1100.0, 900.0], WIDTH, window), [1200.0 - WIDTH - MARGIN, 800.0 - 48.0]);
        assert_eq!(clamp_pos([10.0, 10.0], WIDTH, [300.0, 40.0]), [MARGIN, MARGIN], "a window narrower than the panel pins it to the margin");
        assert_eq!(clamp_size([100.0, 100.0], [20.0, 20.0], window), [MIN_W, MIN_H]);
        assert_eq!(clamp_size([2000.0, 2000.0], [200.0, 100.0], window), [1200.0 - MARGIN - 200.0, 800.0 - MARGIN - 100.0]);
    }

    #[test]
    fn inspecting_maps_the_innermost_region_to_its_family() {
        let role = |path: &str| family_role(inspected_role(path));
        assert_eq!(role("shell/sidebar/sb-scroll/sb-list/sb-row-t1/#0"), "sidebarRowSelected");
        assert_eq!(role("shell/sidebar/#2"), "sidebar");
        assert_eq!(role("shell/main/terminal-drawer/#1"), "terminalBackground");
        assert_eq!(role("shell/main/topbar/topbar-title"), "canvas", "the toolbar is the background's family");
        assert_eq!(role("shell/main/#3/composer-editor"), "input");
        assert_eq!(role("#0/#1"), "canvas");
        assert_eq!(family_label("terminalBackground"), "Terminal background");
    }

    fn two_modes() -> Value {
        let mut theme = builtins().iter().find(|t| t["id"] == "grove").unwrap().clone();
        theme["id"] = json!("custom-grove");
        theme["collection"] = json!({"id": "open-vsx:x", "label": "X"});
        theme
    }

    #[test]
    fn a_session_opens_on_a_mode_the_theme_has_and_unmanaged_themes_open_advanced() {
        let dark_only = json!({"id": "custom-d", "label": "Dusk", "appearance": "dark", "colors": {"canvas": "#101010"}});
        let (session, active) = seed(&dark_only, false, true);
        assert!(active, "a dark-only theme opens on dark");
        assert_eq!(session["colors"]["dark"]["canvas"], "#101010");
        assert_eq!(session["colors"]["dark"]["accent"], stock(true)["accent"], "missing roles fill from the stock palette");
        assert_eq!((session["advanced"].as_bool(), session["regenerate"].as_bool()), (Some(true), Some(true)));
        let (fresh, active) = seed(&dark_only, false, false);
        assert!(!active);
        assert_eq!(fresh["advanced"], false, "a new theme starts with two colors");
    }

    #[test]
    fn saving_keeps_an_edited_themes_modes_and_a_new_theme_takes_one() {
        let theme = two_modes();
        let (mut session, _) = seed(&theme, true, true);
        session["colors"]["dark"]["text"] = json!("#eeeeee");
        let saved = saved_theme(&theme, &session, "Grove 2", true, true);
        assert_eq!((saved["label"].as_str(), saved["appearance"].clone()), (Some("Grove 2"), theme["appearance"].clone()));
        assert!(saved["variants"].is_object() && saved.get("managed").is_none(), "advanced saves keep every edited role");
        assert_eq!(saved["collection"]["id"], "open-vsx:x");
        let dark = if theme["appearance"] == "dark" { &saved["colors"] } else { &saved["variants"]["dark"] };
        assert_eq!(dark["text"], "#eeeeee");

        let (mut simple, _) = seed(&theme, false, false);
        simple["colors"]["light"]["accent"] = json!("#3366ff");
        simple["dirty"]["light"] = json!(true);
        let created = saved_theme(&json!({"id": "custom-new"}), &simple, "Aurora", false, false);
        assert_eq!((created["appearance"].as_str(), created["managed"].as_bool()), (Some("light"), Some(true)));
        assert!(created.get("variants").is_none());
        assert_eq!(created["colors"], managed(false, &simple["colors"]["light"]), "dirty simple colors regenerate on save");
    }

    #[test]
    fn simple_edits_regenerate_and_advanced_edits_stay_in_the_family() {
        let mut s = App::new();
        create(&mut s, false);
        let before = session(&s)["colors"]["light"].clone();
        update_color(&mut s, "accent", "#3366ff");
        assert_eq!(session(&s)["dirty"]["light"], true);
        assert_ne!(session(&s)["colors"]["light"]["focus"], before["focus"], "derived roles follow the accent");
        assert_eq!(preview(&s).map(|(dark, colors)| (dark, colors["accent"].clone())), Some((false, session(&s)["colors"]["light"]["accent"].clone())));

        set_advanced(&mut s, true);
        let before = session(&s)["colors"]["light"].clone();
        update_color(&mut s, "error", "#cc0000");
        let after = &session(&s)["colors"]["light"];
        assert_eq!(after["canvas"], before["canvas"], "other families are untouched");
        assert_eq!(session(&s)["regenerate"], true);
        set_advanced(&mut s, false);
        assert_eq!(session(&s)["regenerate"], false, "leaving advanced mode regenerates once");
        close(&mut s);
        assert!(preview(&s).is_none(), "closing ends the preview");
    }
}
