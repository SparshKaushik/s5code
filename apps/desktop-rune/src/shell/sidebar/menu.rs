//! The row context menu (`useSidebarThreadContextMenu`). Desktop shows it as a native menu, so
//! it is `kit::Menu` in native mode: 22pt rows, inset separators, no icons.

use rune::{Cx, El, Key, KeyEvent, Tx};

use super::Model;
use super::logic::Thread;
use crate::icons;
use crate::kit::{self, MenuEntry, MenuItem};
use crate::theme::T;
use crate::data::Zone;
use crate::{App, Route};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sub {
    Snooze,
    AutoSettle,
    Copy,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub thread: String,
    /// Window point the menu opened at, or the element it hangs under.
    pub at: (f32, f32),
    pub anchor: Option<String>,
    pub sub: Option<Sub>,
    /// Just the snooze presets (the row's hover clock button).
    pub snooze_only: bool,
    /// Opened from the chat header (ChatHeader's useThreadActionMenu): adds "Find in thread"
    /// and renames in the header.
    pub header: bool,
}

impl Menu {
    pub fn new(thread: String, x: f32, y: f32) -> Menu {
        Menu { thread, at: (x, y), anchor: None, sub: None, snooze_only: false, header: false }
    }

    pub fn snooze_for(thread: String) -> Menu {
        let anchor = Some(format!("sb-snooze-{thread}"));
        Menu { thread, at: (0.0, 0.0), anchor, sub: None, snooze_only: true, header: false }
    }
}

fn close(s: &mut App) {
    s.ui.shell.sidebar.menu = None;
}

pub fn on_key(s: &mut App, e: &KeyEvent, _tx: &mut Tx) -> bool {
    if e.key == Key::Escape {
        let ui = &mut s.ui.shell.sidebar;
        match ui.menu.as_mut() {
            Some(m) if m.sub.is_some() && !m.snooze_only => m.sub = None,
            _ => close(s),
        }
        return true;
    }
    false
}

fn set_sub(s: &mut App, sub: Option<Sub>) {
    if let Some(m) = s.ui.shell.sidebar.menu.as_mut() {
        m.sub = sub;
    }
}

fn item(label: impl Into<String>, act: impl Fn(&mut App, &mut Tx) + 'static) -> MenuItem {
    MenuItem::new(label).on_select_tx(act)
}

fn todo(label: impl Into<String>) -> MenuItem {
    MenuItem::new(label).disabled(true)
}

/// "9:00 AM" for a wall-clock time, as `toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })`.
fn clock(wall: i64) -> String {
    let mins = wall.rem_euclid(86_400_000) / 60_000;
    let (h, m) = (mins / 60, mins % 60);
    format!("{}:{m:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" })
}

/// `resolveSnoozePresets` over `DEFAULT_SNOOZE_PRESETS`: soonest first, one per wake minute.
pub fn snooze_presets(now: i64, zone: Zone) -> Vec<(String, i64)> {
    const DAY: i64 = 86_400_000;
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    let wall_now = zone.wall(now);
    let today = wall_now - wall_now.rem_euclid(DAY);
    let at = |days: i64, h: i64| zone.instant(today + days * DAY + h * 3_600_000);
    // getDay(): the epoch was a Thursday (4).
    let weekday = (wall_now.div_euclid(DAY) + 4).rem_euclid(7);
    let monday = (1 - weekday + 7).rem_euclid(7);
    let monday = if monday == 0 { 7 } else { monday };
    let clock = |at: i64| clock(zone.wall(at));
    let when = |wake: i64, names_day: bool| {
        let day = zone.wall(wake).div_euclid(DAY);
        if names_day || day == wall_now.div_euclid(DAY) {
            clock(wake)
        } else {
            format!("{} {}", DAYS[day.rem_euclid(7) as usize], clock(wake))
        }
    };
    let mut out: Vec<(String, i64)> = vec![
        (format!("In 1 hour ({})", when(now + 3_600_000, false)), now + 3_600_000),
        (format!("In 3 hours ({})", when(now + 3 * 3_600_000, false)), now + 3 * 3_600_000),
    ];
    if at(0, 18) > now {
        out.push((format!("This evening ({})", clock(at(0, 18))), at(0, 18)));
    }
    out.push((format!("Tomorrow ({})", clock(at(1, 9))), at(1, 9)));
    let label = if monday == 1 { "Tomorrow" } else { "Next week" };
    out.push((format!("{label} ({})", when(at(monday, 9), monday == 1)), at(monday, 9)));
    out.sort_by_key(|(_, w)| w / 60_000);
    out.dedup_by_key(|(_, w)| *w / 60_000);
    out
}

fn snooze_entries(id: &str, now: i64) -> Vec<MenuEntry> {
    let mut out: Vec<MenuEntry> = snooze_presets(now, Zone::Local)
        .into_iter()
        .map(|(label, until)| {
            let id = id.to_string();
            MenuEntry::item(item(label, move |s, _| {
                super::snooze(s, &id, until);
                close(s);
            }))
        })
        .collect();
    out.push(MenuEntry::Separator);
    out.push(MenuEntry::item(todo("Custom…")));
    out
}

/// A Copy entry: the value on the clipboard, then the web's success toast.
fn copy(label: &str, glyph: &'static rune::Icon, value: String, toast: &'static str, description: String) -> MenuEntry {
    MenuEntry::item(
        item(label, move |s, tx| {
            tx.copy(&value);
            close(s);
            crate::shell::palette::toast(s, toast, Some(&description));
        })
        .icon(glyph),
    )
}

// threadActionMenu's icons that apps/web draws from contextMenuFallback, not lucide-react.
static MAIL_OPEN: rune::Icon = rune::Icon { name: "mail-open", view: 24.0, body: "<path d=\"M21.2 8.4c.5.38.8.97.8 1.6v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V10a2 2 0 0 1 .8-1.6l8-6a2 2 0 0 1 2.4 0l8 6Z\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\"/><path d=\"m22 10-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 10\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\"/>" };
static TIMER: rune::Icon = rune::Icon { name: "timer", view: 24.0, body: "<line x1=\"10\" x2=\"14\" y1=\"2\" y2=\"2\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\"/><line x1=\"12\" x2=\"15\" y1=\"14\" y2=\"11\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\"/><circle cx=\"12\" cy=\"14\" r=\"8\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\"/>" };
static HASH: rune::Icon = rune::Icon { name: "hash", view: 24.0, body: "<line x1=\"4\" x2=\"20\" y1=\"9\" y2=\"9\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\"/><line x1=\"4\" x2=\"20\" y1=\"15\" y2=\"15\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\"/><line x1=\"10\" x2=\"8\" y1=\"3\" y2=\"21\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\"/><line x1=\"16\" x2=\"14\" y1=\"3\" y2=\"21\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\"/>" };

fn entries(s: &App, model: &Model, th: &Thread, sub: Option<Sub>) -> Vec<MenuEntry> {
    let id = th.id.clone();
    let with = |f: fn(&mut App, &str)| {
        let id = id.clone();
        move |s: &mut App, _: &mut Tx| {
            f(s, &id);
            close(s);
        }
    };
    let shell = s.state().threads.iter().find(|t| t["id"] == th.id.as_str());
    match sub {
        Some(Sub::Snooze) => return snooze_entries(&th.id, model.now),
        Some(Sub::AutoSettle) => {
            let enabled = shell.is_none_or(|t| t["autoSettleDisabledAt"].is_null());
            return [("Enabled", true), ("Disabled", false)]
                .into_iter()
                .map(|(label, on)| {
                    let id = th.id.clone();
                    MenuEntry::item(
                        item(label, move |s, _| {
                            super::dispatch(s, "thread.auto-settle.set", &id, serde_json::json!({ "enabled": on }));
                            close(s);
                        })
                        .checked(on == enabled),
                    )
                })
                .collect();
        }
        Some(Sub::Copy) => {
            let mut out = Vec::new();
            let path = th.worktree.clone().or_else(|| {
                s.state().projects.iter().find(|p| p["id"] == th.project_id.as_str()).and_then(|p| p["workspaceRoot"].as_str().map(str::to_string))
            });
            if let Some(p) = path {
                out.push(copy("Path", &icons::FOLDER, p.clone(), "Path copied", p));
            }
            if let Some(b) = th.branch.clone() {
                out.push(copy("Branch", &icons::GIT_BRANCH, b.clone(), "Branch copied", b));
            }
            let link = crate::shell::palette::actions::thread_link(s, &th.id);
            out.push(copy("Thread", &icons::MESSAGE_SQUARE, link, "Thread copied", th.title.clone()));
            out.push(copy("Thread ID", &HASH, th.id.clone(), "Thread ID copied", th.id.clone()));
            return out;
        }
        None => {}
    }
    let open = s.ui.shell.sidebar.menu.as_ref().and_then(|m| m.sub);
    // Leaves close a sibling's submenu as the pointer moves onto them, as AppKit menus do.
    let leaf = |i: MenuItem| MenuEntry::item(i.on_hover(|s: &mut App| set_sub(s, None)));
    let sub_item = |label: &str, glyph: &'static rune::Icon, sub: Sub| {
        MenuEntry::item(
            MenuItem::new(label)
                .icon(glyph)
                .submenu(entries(s, model, th, Some(sub)), open == Some(sub))
                .on_submenu_change(move |s: &mut App, on| set_sub(s, on.then_some(sub))),
        )
    };
    let mut out = Vec::new();
    let header = s.ui.shell.sidebar.menu.as_ref().is_some_and(|m| m.header);
    if header {
        out.push(leaf(item("Find in thread", |s, _| {
            close(s);
            crate::chat::timeline::find::open(s);
        })
        .icon(&icons::SEARCH)));
        out.push(MenuEntry::Separator);
    }
    if let Some(branch) = th.branch.clone() {
        let (project, worktree) = (th.project_id.clone(), th.worktree.clone());
        out.push(leaf(item(format!("New thread on {branch}"), move |s, _| {
            close(s);
            crate::chat::home::start_on_branch(s, &project, &branch, worktree.as_deref());
        })));
    }
    if th.pinned {
        out.push(leaf(item("Unpin thread", with(super::unpin)).icon(&icons::PIN_OFF)));
    } else {
        out.push(leaf(item("Pin thread", with(|s, id| super::pin(s, id, None))).icon(&icons::PIN)));
    }
    if th.settled {
        out.push(leaf(item("Un-settle thread", with(super::unsettle)).icon(&icons::CIRCLE_CHECK)));
    } else {
        out.push(leaf(item("Settle thread", with(super::settle)).icon(&icons::CIRCLE_CHECK)));
    }
    if th.snoozed(model.now) {
        out.push(leaf(item("Wake thread", with(super::unsnooze)).icon(&icons::CLOCK)));
    } else if th.can_snooze() {
        out.push(sub_item("Snooze", &icons::CLOCK, Sub::Snooze));
    } else {
        out.push(leaf(todo("Snooze").icon(&icons::CLOCK)));
    }
    out.push(MenuEntry::Separator);
    let rename = if header {
        item("Rename thread", |s, _| {
            close(s);
            crate::chat::topbar::start_rename(s);
        })
    } else {
        item("Rename thread", with(super::start_rename))
    };
    out.push(leaf(rename.icon(&icons::PENCIL)));
    out.push(leaf(
        item("Regenerate title", with(|s, id| super::dispatch(s, "thread.metadata.update", id, serde_json::json!({ "regenerateTitle": true })))).icon(&icons::REFRESH_CW),
    ));
    out.push(leaf(item("Mark unread", with(|s, id| super::dispatch(s, "thread.mark-unread", id, serde_json::json!({})))).icon(&MAIL_OPEN)));
    let project = th.project_id.clone();
    let scoped = s.ui.shell.sidebar.project_scope.contains(&project);
    let filter = if scoped { "Show all projects".to_string() } else { format!("Filter by {}", model.project(th)) };
    out.push(leaf(
        item(filter, move |s, _| {
            let scope = &mut s.ui.shell.sidebar.project_scope;
            *scope = if scoped { Vec::new() } else { vec![project.clone()] };
            close(s);
        })
        .icon(&icons::FOLDER_TREE),
    ));
    out.push(sub_item("Auto-settle behavior", &TIMER, Sub::AutoSettle));
    out.push(MenuEntry::Separator);
    out.push(sub_item("Copy", &icons::COPY, Sub::Copy));
    let settings_project = th.project_id.clone();
    out.push(leaf(
        item("Project settings", move |s, _| {
            close(s);
            s.ui.settings.project = Some(settings_project.clone()).filter(|p| !p.is_empty());
            super::go(s, Route::Settings("projects".into()));
        })
        .icon(&icons::SETTINGS),
    ));
    out.push(MenuEntry::Separator);
    let archive = if th.running() { todo("Archive thread") } else { item("Archive thread", with(super::archive)) };
    out.push(leaf(archive.icon(&icons::ARCHIVE)));
    out.push(leaf(item("Delete", with(super::request_delete)).icon(&icons::TRASH_2).variant("destructive")));
    out
}

/// Mounts the menu: at the pointer for a context invocation, under the clock button for snooze.
pub fn view(s: &App, _cx: &mut Cx<App>, model: &Model, th: &Thread, m: &Menu, t: &T) -> Vec<El> {
    let root = if m.snooze_only { Some(Sub::Snooze) } else { None };
    let mut menu = kit::Menu::new("sb-menu").native(true).width(198.0).entries(entries(s, model, th, root)).open(true).on_close(close);
    menu = match &m.anchor {
        Some(key) => menu.anchor(key.clone()).placement(kit::Placement::BottomStart).gutter(4.0),
        None => menu.at(m.at.0, m.at.1),
    };
    let mut layers = Vec::new();
    menu.overlay(&kit::theme(t), &mut layers);
    layers
}
