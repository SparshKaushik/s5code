//! Web's `ThemeEditorPanel` as `ThemeEditorHost` mounts it: a floating panel over the app whose
//! colors preview live. The session lives in the `form.themeEditor` value; web's inspect mode,
//! resize handle and merge-by-name are not ported.
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

fn close(s: &mut App) {
    s.ui.settings.theme_editor = None;
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
    let modes = editing_modes(s)?;
    let label = s.ui.settings.theme_editor.as_ref().and_then(|t| t["label"].as_str()).unwrap_or("");
    (!modes.contains(&mode(dark))).then(|| format!("\u{201c}{label}\u{201d} has no {} palette. Create a theme with the same name to add one.", mode(dark)))
}

fn set_appearance(s: &mut App, dark: bool) {
    if lock_reason(s, dark).is_none() {
        s.ui.settings.theme_edit_dark = dark;
        sync_fields(s, None);
    }
}

/// `handleSubmit` without the merge-by-name path: an edit keeps the theme's appearances and
/// collection, a new theme takes the appearance being edited.
fn saved_theme(template: &Value, session: &Value, name: &str, dark: bool, editing: bool) -> Value {
    let advanced = session["advanced"] == true;
    let colors = |m: &str| {
        let current = &session["colors"][m];
        if !advanced && session["dirty"][m] == true { managed(m == "dark", current) } else { current.clone() }
    };
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

fn drag(s: &mut App, d: &rune::Drag) {
    let session = session_mut(s);
    if d.phase == Phase::Down {
        let from = if session["pos"].is_array() { session["pos"].clone() } else { json!([session["rect"][0], session["rect"][1]]) };
        session["grab"] = from;
    }
    if let (Some(x), Some(y)) = (session["grab"][0].as_f64(), session["grab"][1].as_f64()) {
        session["pos"] = json!([(x as f32 + d.dx).max(MARGIN), (y as f32 + d.dy).max(MARGIN)]);
    }
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
    row()
        .key(format!("theme-color-{role}"))
        .w_full()
        .min_h(32.0)
        .gap(10.0)
        .center_y()
        .child(el().size(20.0, 20.0).radius(6.0).border(1.0, t.c.border).fill(color(css).unwrap_or(rune::Color::TRANSPARENT)))
        .child(text(label.to_owned()).font(theme::TEXT_SM).color(t.c.foreground).grow(1.0).min_w(0.0).truncate())
        .child(input)
}

fn fields(s: &App, t: &T, advanced: bool) -> El {
    if !advanced {
        return col().w_full().gap(4.0).children(SIMPLE.iter().map(|&(role, label)| field(s, t, role, label)));
    }
    let query = filter_text(s).trim().to_lowercase();
    let mut out = col().w_full().gap(20.0);
    let mut any = false;
    for (title, families) in GROUPS {
        let shown: Vec<&Family> = families.iter().filter(|(label, _, roles)| query.is_empty() || format!("{label} {}", roles.join(" ")).to_lowercase().contains(&query)).collect();
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
            .on_change(|s: &mut App, _| session_mut(s)["error"] = Value::Null)
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
    out.child(row().w_full().gap(12.0).center_y().child(text("Appearance").font(theme::SM_MEDIUM).color(t.c.foreground).grow(1.0)).child(appearance.build(&k)))
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
                .on_drag(|s: &mut App, d, _| drag(s, d))
                .child(text(title).font(theme::SM_MEDIUM).color(t.c.foreground).truncate()),
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
    let mut panel = col()
        .key("theme-editor")
        .w(416.0)
        .max_h(672.0)
        .radius(12.0)
        .clip_rounded(12.0)
        .border(1.0, t.c.border)
        .fill(t.c.popover)
        .shadow(16.0, 40.0, rune::Color::BLACK.alpha(0.3))
        .role("dialog")
        .label(title)
        .on_layout(|s: &mut App, rect| session_mut(s)["rect"] = json!(rect))
        .child(header);
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
            );
    }
    panel = match (state["pos"][0].as_f64(), state["pos"][1].as_f64()) {
        (Some(x), Some(y)) => panel.absolute(x as f32, y as f32),
        _ => panel.pin(None, None, Some(16.0), Some(16.0)),
    };
    panel = if matches!(s.route, Route::Settings(_)) {
        panel.modal(false, close)
    } else {
        panel.on_key(|s: &mut App, e: &rune::KeyEvent, _: &mut rune::Tx| {
            let escape = e.key == rune::Key::Escape;
            if escape {
                close(s);
            }
            escape
        })
    };
    layers.push(panel);
}

#[cfg(test)]
mod tests {
    use super::*;

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
