//! The browser profiles row (`BrowserProfilesSetting`): the profile list with rename, the
//! default badge and each profile's options menu, the Add profile menu, and the remove and
//! clear confirmations.
//!
//! Profiles are client settings. Rune's browser keeps one Default profile in the T3 home
//! (`userdata/browser/rune-default`), cleared from the preview menu, so the data cleared here is
//! a server browser's (`preview.clearProfile`). "Import from" reads a Chromium browser on this
//! machine (`panel::browser_import`) into that Default profile, the only one Rune's Chrome
//! opens, so the wizard offers no "New profile" target.

use rune::{El, col, icon, row, text};
use serde_json::{Value, json};

use crate::panel::browser_import::{self as import, Profile, Source, Stage};

use super::{controls, request, update, value as setting};
use crate::kit::{self, Menu, MenuItem, Placement};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};

const DEFAULT_ID: &str = "default";
const INCOGNITO_ID: &str = "incognito";
const MAX_COUNT: usize = 24;
const NAME_MAX: usize = 48;

#[derive(Default)]
pub struct Ui {
    /// The open menu: `add`, or a profile id for its options.
    pub menu: Option<String>,
    /// `(id, name)` awaiting the remove or clear confirmation.
    pub remove: Option<(String, String)>,
    pub clear: Option<(String, String)>,
    pub import: Option<Import>,
}

/// Web's `BrowserImportWizard`, minus the steps only Safari needs.
pub struct Import {
    source: &'static Source,
    profiles: Vec<Profile>,
    from: String,
    step: Step,
}

enum Step {
    Configure,
    Quit,
    Blocked(&'static str),
    Running(import::Job),
}

/// `initialWizardStep` for a freshly read source.
fn wizard(source: &'static Source, from: Option<String>) -> Import {
    let profiles = import::profiles(source);
    let from = from.filter(|f| profiles.iter().any(|p| p.directory == *f)).or_else(|| profiles.first().map(|p| p.directory.clone())).unwrap_or_default();
    let step = if import::running(source) {
        Step::Quit
    } else if profiles.is_empty() {
        Step::Blocked("unknownSourceProfile")
    } else {
        Step::Configure
    };
    Import { source, profiles, from, step }
}

fn run_import(s: &mut App, scale: f32) {
    let Some(w) = s.ui.settings.profiles.import.as_ref() else { return };
    let (source, from) = (w.source, w.from.clone());
    if let Some(denied) = crate::desktop::policy::current().denies_cookie_import() {
        crate::shell::palette::toast(s, "Could not import cookies", Some(&denied));
        return;
    }
    let step = match crate::panel::preview::import(s, scale, source, from) {
        Some(job) => Step::Running(job),
        None => Step::Blocked("sessionUnavailable"),
    };
    if let Some(w) = s.ui.settings.profiles.import.as_mut() {
        w.step = step;
    }
}

fn user_profiles(s: &App) -> Vec<Value> {
    setting(s, "browserProfiles").as_array().cloned().unwrap_or_default()
}

/// `resolveBrowserProfiles` without Incognito, which the list leaves out.
fn listed(s: &App) -> Vec<(String, String, bool)> {
    let mut out = vec![(DEFAULT_ID.to_owned(), "Default".to_owned(), true)];
    for p in user_profiles(s) {
        let id = crate::data::s(&p, "id").to_owned();
        if id.is_empty() || id == INCOGNITO_ID || out.iter().any(|(i, _, _)| *i == id) {
            continue;
        }
        out.push((id, crate::data::s(&p, "name").to_owned(), false));
    }
    out
}

fn default_id(s: &App) -> String {
    let id = setting(s, "browserDefaultProfileId").as_str().unwrap_or(DEFAULT_ID).to_owned();
    if listed(s).iter().any(|(i, _, _)| *i == id) { id } else { DEFAULT_ID.to_owned() }
}

/// Clearing needs somewhere to clear: a connected environment with a server browser.
fn removal_available(s: &App) -> bool {
    matches!(s.src, crate::data::Source::Live(_))
        && s.state().config.as_ref().is_some_and(|c| c["environment"]["capabilities"]["serverBrowser"] == true)
}

fn create(s: &mut App) {
    let mut profiles = user_profiles(s);
    if profiles.len() >= MAX_COUNT {
        return;
    }
    let taken: Vec<_> = listed(s).into_iter().map(|(_, n, _)| n).chain(["Incognito".to_owned()]).collect();
    let mut name = "New profile".to_owned();
    let mut n = 2;
    while taken.contains(&name) {
        name = format!("New profile {n}");
        n += 1;
    }
    profiles.push(json!({"id": format!("profile-{}", s5_net::uuid()), "name": name, "kind": "persistent"}));
    update(s, "browserProfiles", json!(profiles));
}

/// The rename field's value, behind the `profile::<id>` key.
pub fn value(s: &App, id: &str) -> Value {
    listed(s).into_iter().find(|(i, _, _)| i == id).map_or(Value::Null, |(_, n, _)| json!(n))
}

pub fn rename(s: &mut App, id: &str, next: Value) {
    let name: String = next.as_str().unwrap_or("").trim().chars().take(NAME_MAX).collect();
    if name.is_empty() {
        return;
    }
    let profiles: Vec<_> = user_profiles(s)
        .into_iter()
        .map(|mut p| {
            if p["id"] == id {
                p["name"] = json!(name);
            }
            p
        })
        .collect();
    update(s, "browserProfiles", json!(profiles));
}

fn clear_data(s: &mut App, id: &str) {
    request(s, "preview.clearProfile", json!({"profileId": id}));
}

fn remove(s: &mut App, id: &str) {
    clear_data(s, id);
    let profiles: Vec<_> = user_profiles(s).into_iter().filter(|p| p["id"] != id).collect();
    update(s, "browserProfiles", json!(profiles));
    if setting(s, "browserDefaultProfileId") == id {
        update(s, "browserDefaultProfileId", json!(DEFAULT_ID));
    }
    s.ui.settings.profiles.remove = None;
}

/// Kit menus truncate labels to a fixed width; the web's popup grows to its longest item.
fn fit(longest: &str) -> f32 {
    (longest.chars().count() as f32 * 7.0 + 28.0).ceil().max(160.0)
}

fn close_menu(s: &mut App) {
    s.ui.settings.profiles.menu = None;
}

/// The Add profile trigger and the list under the row's copy.
pub fn parts(s: &App, t: &T) -> (El, Option<El>) {
    let open = s.ui.settings.profiles.menu.as_deref() == Some("add");
    let add = row().key("profiles-add").child(
        kit::Button::new("Add profile")
            .variant("outline")
            .size("sm")
            .icon(&icons::PLUS)
            .on_press(move |s: &mut App| s.ui.settings.profiles.menu = if open { None } else { Some("add".into()) })
            .build(t)
            .expanded(open),
    );
    let default = default_id(s);
    let line = t.c.border.alpha_mul(0.6);
    let mut list = col().w_full().mt(8.0).mb(8.0).radius(theme::radius::LG).border(1.0, line).clip_rounded(theme::radius::LG);
    for (index, (id, name, built_in)) in listed(s).into_iter().enumerate() {
        if index > 0 {
            list = list.child(rune::el().w_full().h(1.0).fill(line));
        }
        let mut label = row().grow(1.0).min_w(0.0).gap(8.0).center_y();
        label = if built_in {
            label.child(text(name.clone()).font(theme::TEXT_SM).color(t.c.foreground).truncate())
        } else {
            label.child(controls::input(s, t, &format!("profile::{id}"), 224.0, "", false).label(format!("Rename {name}")))
        };
        if id == default {
            label = label.child(kit::badge("Default", "default", "default", t));
        }
        let menu_open = s.ui.settings.profiles.menu.as_deref() == Some(id.as_str());
        let owned = id.clone();
        list = list.child(
            row()
                .key(format!("profile-row-{id}"))
                .w_full()
                .px(12.0)
                .py(8.0)
                .gap(12.0)
                .center_y()
                .child(label)
                .child(
                    row().key(format!("profile-options-{id}")).child(
                        kit::Button::new(format!("{name} options"))
                            .variant("ghost-muted")
                            .size("icon-xs")
                            .icon(&icons::ELLIPSIS_VERTICAL)
                            .on_press(move |s: &mut App| {
                                s.ui.settings.profiles.menu = if menu_open { None } else { Some(owned.clone()) };
                            })
                            .build(t)
                            .expanded(menu_open),
                    ),
                ),
        );
    }
    (add, Some(list))
}

/// The open menu and any confirmation dialog.
pub fn overlays(s: &App, t: &T, scale: f32, layers: &mut Vec<El>) {
    let k = kit::theme(t);
    match s.ui.settings.profiles.menu.as_deref() {
        Some("add") => {
            let full = user_profiles(s).len() >= MAX_COUNT;
            let sources = import::installed();
            let longest = if full {
                "You’ve reached the profile limit"
            } else if sources.is_empty() {
                "No supported browsers found"
            } else {
                sources.iter().map(|s| s.name).max_by_key(|n| n.len()).unwrap_or_default()
            };
            let mut menu = Menu::new("profiles-add-menu")
                .anchor("profiles-add")
                .width(fit(longest))
                .placement(Placement::BottomEnd)
                .gutter(4.0)
                .open(true)
                .on_close(close_menu)
                .item(MenuItem::new("Blank profile").disabled(full).on_select(|s: &mut App| {
                    create(s);
                    close_menu(s);
                }));
            if full {
                menu = menu.item(MenuItem::new("You’ve reached the profile limit").disabled(true));
            }
            menu = menu.separator().group_label("Import from");
            if sources.is_empty() {
                menu = menu.item(MenuItem::new("No supported browsers found").disabled(true));
            }
            for source in sources {
                menu = menu.item(MenuItem::new(source.name).on_select(move |s: &mut App| {
                    s.ui.settings.profiles.import = Some(wizard(source, None));
                    close_menu(s);
                }));
            }
            menu.overlay(&k, layers);
        }
        Some(id) => {
            if let Some((id, name, built_in)) = listed(s).into_iter().find(|(i, _, _)| i == id) {
                let available = removal_available(s);
                let is_default = default_id(s) == id;
                let (set_id, clear, remove) = (id.clone(), (id.clone(), name.clone()), (id.clone(), name.clone()));
                let longest = if available { "Remove profile and data" } else { "Connect to an environment to clear profile data" };
                let mut menu = Menu::new("profile-options-menu")
                    .anchor(format!("profile-options-{id}"))
                    .width(fit(longest))
                    .placement(Placement::BottomEnd)
                    .gutter(4.0)
                    .open(true)
                    .on_close(close_menu)
                    .item(MenuItem::new("Set as default").disabled(is_default).on_select(move |s: &mut App| {
                        update(s, "browserDefaultProfileId", json!(set_id));
                        close_menu(s);
                    }))
                    .item(MenuItem::new("Clear cookies and cache").disabled(!available).on_select(move |s: &mut App| {
                        s.ui.settings.profiles.clear = Some(clear.clone());
                        close_menu(s);
                    }));
                if !built_in {
                    menu = menu.item(
                        MenuItem::new("Remove profile and data").variant("destructive").disabled(!available).on_select(
                            move |s: &mut App| {
                                s.ui.settings.profiles.remove = Some(remove.clone());
                                close_menu(s);
                            },
                        ),
                    );
                }
                if !available {
                    menu = menu.separator().item(MenuItem::new("Connect to an environment to clear profile data").disabled(true));
                }
                menu.overlay(&k, layers);
            }
        }
        None => {}
    }
    let footer = |confirm: &str, on_confirm: Box<dyn Fn(&mut App)>, cancel: fn(&mut App)| {
        row()
            .gap(8.0)
            .child(kit::Button::new("Cancel").variant("outline").on_press(cancel).build(t))
            .child(kit::Button::new(confirm.to_owned()).variant("destructive").on_press(move |s: &mut App| on_confirm(s)).build(t))
    };
    if let Some((id, name)) = s.ui.settings.profiles.remove.clone() {
        kit::Dialog::new("remove-profile", format!("Remove “{name}”?"))
            .open(true)
            .alert()
            .description(
                "Its cookies and logins are deleted. Desktop tabs already open in this profile stay open until you close them; server browser tabs close now.",
            )
            .on_close(|s: &mut App| s.ui.settings.profiles.remove = None)
            .child(footer("Remove profile", Box::new(move |s: &mut App| remove(s, &id)), |s| s.ui.settings.profiles.remove = None))
            .build(t, layers);
    }
    if let Some((id, name)) = s.ui.settings.profiles.clear.clone() {
        kit::Dialog::new("clear-profile", format!("Clear “{name}”’s cookies and cache?"))
            .open(true)
            .alert()
            .description("You are signed out of its sites. Server browser tabs open in this profile close now.")
            .on_close(|s: &mut App| s.ui.settings.profiles.clear = None)
            .child(footer(
                "Clear data",
                Box::new(move |s: &mut App| {
                    clear_data(s, &id);
                    s.ui.settings.profiles.clear = None;
                }),
                |s| s.ui.settings.profiles.clear = None,
            ))
            .build(t, layers);
    }
    if let Some(w) = &s.ui.settings.profiles.import {
        import_dialog(s, t, w, scale, layers);
    }
}

fn cookie_count(n: usize) -> String {
    format!("{} {}", thousands(n), if n == 1 { "cookie" } else { "cookies" })
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn close_import(s: &mut App) {
    s.ui.settings.profiles.import = None;
}

/// Web's `SelectableTile`.
fn tile(t: &T, key: String, selected: bool, title: &str, subtitle: Option<String>, on_select: impl Fn(&mut App) + Clone + 'static) -> El {
    let mut tile = row()
        .key(key)
        .w_full()
        .gap(12.0)
        .px(12.0)
        .py(8.0)
        .center_y()
        .radius(theme::radius::LG)
        .border(1.0, if selected { t.c.primary } else { t.c.border.alpha_mul(0.6) })
        .focusable(true)
        .role("button")
        .label(title.to_owned())
        .on_press(on_select);
    tile = if selected { tile.fill(t.c.primary.alpha_mul(0.08)) } else { tile.hover(t.c.muted.alpha_mul(0.4)) };
    tile.child(
        col()
            .grow(1.0)
            .min_w(0.0)
            .child(text(title.to_owned()).font(theme::weight(theme::TEXT_SM, theme::MEDIUM)).color(t.c.foreground).truncate())
            .children(subtitle.map(|sub| text(sub).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate())),
    )
    .child(
        row()
            .w(16.0)
            .h(16.0)
            .radius(8.0)
            .center()
            .border(1.0, if selected { t.c.primary } else { t.c.input })
            .fill(if selected { t.c.primary } else { rune::Color::TRANSPARENT })
            .children(selected.then(|| icon(&icons::CHECK, 10.0).color(t.c.primary_foreground))),
    )
}

fn section(t: &T, label: &str, tiles: Vec<El>) -> El {
    col()
        .grow(1.0)
        .min_w(0.0)
        .gap(8.0)
        .child(text(label.to_uppercase()).font(theme::weight(theme::TEXT_XS, theme::MEDIUM)).color(t.c.muted_foreground))
        .children(tiles)
}

fn busy(t: &T, label: &str) -> El {
    row()
        .gap(12.0)
        .py(8.0)
        .center_y()
        .child(kit::Spinner::new().size("md").tone("muted").build(&kit::theme(t)))
        .child(text(label.to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground))
}

fn footer(buttons: Vec<El>) -> El {
    row().w_full().gap(8.0).justify_end().children(buttons)
}

const ENVIRONMENT: &str = "This device";

fn import_dialog(s: &App, t: &T, w: &Import, scale: f32, layers: &mut Vec<El>) {
    let name = w.source.name;
    let cancel = || kit::Button::new("Cancel").variant("outline").on_press(close_import).build(t);
    let stage = match &w.step {
        Step::Running(job) => Some(job.lock().unwrap().clone()),
        _ => None,
    };
    let dialog = |title: String, description: String| kit::Dialog::new("browser-import", title).open(true).alert().description(description).on_close(close_import);
    let blocked = |reason: &'static str| {
        let mut buttons = vec![kit::Button::new("Close").variant("outline").on_press(close_import).build(t)];
        if import::retryable(reason) {
            buttons.push(kit::Button::new("Try again").on_press(move |s: &mut App| run_import(s, scale)).build(t));
        }
        dialog(format!("Couldn’t import from {name}"), import::failure_copy(reason).to_owned()).child(footer(buttons))
    };
    let quit = || {
        let source = w.source;
        let from = w.from.clone();
        dialog(format!("Quit {name} to import"), format!("{name} is open, so its cookies can’t be read yet. Quit it, then continue.")).child(footer(vec![
            cancel(),
            kit::Button::new("I’ve quit it")
                .on_press(move |s: &mut App| s.ui.settings.profiles.import = Some(wizard(source, Some(from.clone()))))
                .build(t),
        ]))
    };
    let built = match (&w.step, stage) {
        (Step::Configure, _) => {
            let from: Vec<El> = w
                .profiles
                .iter()
                .map(|p| {
                    let directory = p.directory.clone();
                    let subtitle = p.cookies.map(|n| if n == 0 { "no cookies".to_owned() } else { cookie_count(n) });
                    tile(t, format!("import-from-{}", p.directory), p.directory == w.from, &p.name, subtitle, move |s: &mut App| {
                        if let Some(w) = s.ui.settings.profiles.import.as_mut() {
                            w.from = directory.clone();
                        }
                    })
                })
                .collect();
            let into = vec![tile(t, "import-into-default".into(), true, "Default", Some("Existing profile".into()), |_: &mut App| {})];
            let body = row()
                .w_full()
                .gap(16.0)
                .center_y()
                .child(section(t, "From", from))
                .child(icon(&icons::ARROW_RIGHT, 16.0).color(t.c.muted_foreground))
                .child(section(t, "Into", into));
            dialog(format!("Import from {name}"), format!("Choose which cookies to import for {ENVIRONMENT}.")).child(body).child(footer(vec![
                cancel(),
                kit::Button::new("Import").disabled(w.from.is_empty()).on_press(move |s: &mut App| run_import(s, scale)).build(t),
            ]))
        }
        (Step::Quit, _) | (_, Some(Stage::Failed("browserRunning"))) => quit(),
        (Step::Blocked(reason), _) => blocked(*reason),
        (_, Some(Stage::Failed(reason))) => blocked(reason),
        (_, Some(Stage::Writing { .. })) if crate::panel::preview::browser_failed(s) => blocked("sessionUnavailable"),
        (_, Some(Stage::Done { imported, skipped, domains })) => {
            let title = if imported > 0 {
                format!("Imported {}", cookie_count(imported))
            } else if skipped > 0 {
                format!("Skipped {}", cookie_count(skipped))
            } else {
                "No cookies found".to_owned()
            };
            let description = if imported > 0 {
                let tail = if skipped > 0 { format!(" {} skipped.", cookie_count(skipped)) } else { String::new() };
                format!("Added to Default for {ENVIRONMENT}.{tail}")
            } else if skipped > 0 {
                format!("No cookies were imported for {ENVIRONMENT}.")
            } else {
                format!("There were no cookies to import for {ENVIRONMENT}.")
            };
            let mut d = dialog(title, description);
            if !domains.is_empty() {
                d = d.child(
                    col()
                        .gap(4.0)
                        .child(text("SKIPPED").font(theme::weight(theme::TEXT_XS, theme::MEDIUM)).color(t.c.muted_foreground))
                        .child(text(import::skipped_domains(&domains)).font(theme::TEXT_SM).color(t.c.foreground).wrap()),
                );
            }
            d.child(footer(vec![kit::Button::new("Done").on_press(close_import).build(t)]))
        }
        _ => dialog("Importing cookies".into(), "This may take a moment.".into()).child(busy(t, "Importing…")),
    };
    built.build(t, layers);
}

/// Escape closes the innermost profile overlay first.
pub fn escape(s: &mut App) -> bool {
    let p = &mut s.ui.settings.profiles;
    p.menu.take().is_some() || p.remove.take().is_some() || p.clear.take().is_some() || p.import.take().is_some()
}
