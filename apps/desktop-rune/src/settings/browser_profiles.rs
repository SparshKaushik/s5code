//! The browser profiles row (`BrowserProfilesSetting`): the profile list with rename, the
//! default badge and each profile's options menu, the Add profile menu, and the remove and
//! clear confirmations.
//!
//! Profiles are client settings. Rune's browser runs on a throwaway per-process profile, so the
//! only profile data to clear is a server browser's (`preview.clearProfile`). Importing cookies
//! reads another browser on this machine through the Electron preview bridge; Rune has no such
//! bridge, so "Import from" lists no browsers.

use rune::{El, col, row, text};
use serde_json::{Value, json};

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
pub fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    let k = kit::theme(t);
    match s.ui.settings.profiles.menu.as_deref() {
        Some("add") => {
            let full = user_profiles(s).len() >= MAX_COUNT;
            let longest = if full { "You’ve reached the profile limit" } else { "No supported browsers found" };
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
            menu.separator()
                .group_label("Import from")
                .item(MenuItem::new("No supported browsers found").disabled(true))
                .overlay(&k, layers);
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
}

/// Escape closes the innermost profile overlay first.
pub fn escape(s: &mut App) -> bool {
    let p = &mut s.ui.settings.profiles;
    p.menu.take().is_some() || p.remove.take().is_some() || p.clear.take().is_some()
}
