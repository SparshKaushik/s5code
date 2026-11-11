//! The row context menu (`useSidebarThreadContextMenu`). Desktop shows it as a native menu, so
//! it is `kit::Menu` in native mode: 22pt rows, inset separators, no icons.

use rune::{Cx, El, Key, KeyEvent, Tx};

use super::Model;
use super::logic::Thread;
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
    /// The multi-selection's menu, opened on a selected row.
    pub bulk: bool,
}

impl Menu {
    pub fn new(thread: String, x: f32, y: f32) -> Menu {
        Menu { thread, at: (x, y), anchor: None, sub: None, snooze_only: false, header: false, bulk: false }
    }

    pub fn bulk(thread: String, x: f32, y: f32) -> Menu {
        Menu { bulk: true, ..Menu::new(thread, x, y) }
    }

    pub fn snooze_for(thread: String) -> Menu {
        let anchor = Some(format!("sb-snooze-{thread}"));
        Menu { thread, at: (0.0, 0.0), anchor, sub: None, snooze_only: true, header: false, bulk: false }
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

fn snooze_entries(ids: &[String], now: i64) -> Vec<MenuEntry> {
    let mut out: Vec<MenuEntry> = snooze_presets(now, Zone::Local)
        .into_iter()
        .map(|(label, until)| {
            let ids = ids.to_vec();
            MenuEntry::item(item(label, move |s, _| {
                close(s);
                super::snooze_many(s, &ids, until);
            }))
        })
        .collect();
    out.push(MenuEntry::Separator);
    let ids = ids.to_vec();
    out.push(MenuEntry::item(item("Custom…", move |s, _| {
        close(s);
        super::custom_snooze(s, ids.clone());
    })));
    out
}

/// The selection, in board order, as `handleMultiSelectContextMenu` reads it.
fn selection(s: &App, model: &Model) -> Vec<String> {
    model.ordered_ids().into_iter().filter(|id| s.ui.shell.sidebar.selected.contains(id)).collect()
}

/// `handleMultiSelectContextMenu`'s items for the selected rows.
fn bulk_entries(s: &App, model: &Model, sub: Option<Sub>) -> Vec<MenuEntry> {
    let ids = selection(s, model);
    let threads: Vec<&Thread> = ids.iter().filter_map(|id| model.thread(id)).collect();
    if sub == Some(Sub::Snooze) {
        return snooze_entries(&ids, model.now);
    }
    let count = ids.len();
    let open = s.ui.shell.sidebar.menu.as_ref().and_then(|m| m.sub);
    let leaf = |i: MenuItem| MenuEntry::item(i.on_hover(|s: &mut App| set_sub(s, None)));
    let mut out = Vec::new();
    let pinned: Vec<String> = threads.iter().filter(|t| t.pinned).map(|t| t.id.clone()).collect();
    if !pinned.is_empty() {
        out.push(leaf(item(format!("Unpin ({})", pinned.len()), move |s, _| {
            close(s);
            for id in &pinned {
                super::unpin(s, id);
            }
            s.ui.shell.sidebar.selected.clear();
        })));
    }
    let settle = format!("Settle ({count})");
    out.push(leaf(if threads.iter().all(|t| t.settled) {
        todo(settle)
    } else {
        let ids = ids.clone();
        item(settle, move |s, _| {
            close(s);
            super::settle_many(s, &ids);
        })
    }));
    if threads.iter().all(|t| t.can_snooze() && !t.snoozed(model.now)) {
        out.push(MenuEntry::item(
            MenuItem::new(format!("Snooze ({count})"))
                .submenu(bulk_entries(s, model, Some(Sub::Snooze)), open == Some(Sub::Snooze))
                .on_submenu_change(|s: &mut App, on| set_sub(s, on.then_some(Sub::Snooze))),
        ));
    }
    let shells = &s.state().threads;
    let regenerating = |id: &str| shells.iter().any(|t| t["id"] == id && !t["titleRegeneration"].is_null());
    let fresh: Vec<String> = ids.iter().filter(|id| !regenerating(id.as_str())).cloned().collect();
    out.push(leaf(if fresh.is_empty() {
        todo(format!("Regenerating… ({count})"))
    } else {
        item(format!("Regenerate titles ({})", fresh.len()), move |s, _| {
            close(s);
            for id in &fresh {
                super::dispatch(s, "thread.metadata.update", id, serde_json::json!({ "regenerateTitle": true }));
            }
            s.ui.shell.sidebar.selected.clear();
        })
    }));
    let unread = ids.clone();
    out.push(leaf(item(format!("Mark unread ({count})"), move |s, _| {
        close(s);
        for id in &unread {
            super::dispatch(s, "thread.mark-unread", id, serde_json::json!({}));
        }
        s.ui.shell.sidebar.selected.clear();
    })));
    out.push(leaf(
        item(format!("Delete ({count})"), move |s, _| {
            close(s);
            super::request_delete_many(s, ids.clone());
        }),
    ));
    out
}

/// A Copy entry: the value on the clipboard, then the web's success toast.
fn copy(label: &str, value: String, toast: &'static str, description: String) -> MenuEntry {
    MenuEntry::item(
        item(label, move |s, tx| {
            tx.copy(&value);
            close(s);
            crate::shell::palette::toast(s, toast, Some(&description));
        }),
    )
}

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
        Some(Sub::Snooze) => return snooze_entries(std::slice::from_ref(&th.id), model.now),
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
                out.push(copy("Path", p.clone(), "Path copied", p));
            }
            if let Some(b) = th.branch.clone() {
                out.push(copy("Branch", b.clone(), "Branch copied", b));
            }
            let link = crate::shell::palette::actions::thread_link(s, &th.id);
            out.push(copy("Thread", link, "Thread copied", th.title.clone()));
            out.push(copy("Thread ID", th.id.clone(), "Thread ID copied", th.id.clone()));
            return out;
        }
        None => {}
    }
    let open = s.ui.shell.sidebar.menu.as_ref().and_then(|m| m.sub);
    // Leaves close a sibling's submenu as the pointer moves onto them, as AppKit menus do.
    let leaf = |i: MenuItem| MenuEntry::item(i.on_hover(|s: &mut App| set_sub(s, None)));
    let sub_item = |label: &str, sub: Sub| {
        MenuEntry::item(
            MenuItem::new(label)
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
        })));
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
        out.push(leaf(item("Unpin thread", with(super::request_unpin))));
    } else {
        out.push(leaf(item("Pin thread", with(|s, id| super::pin(s, id, None)))));
    }
    if th.settled {
        out.push(leaf(item("Un-settle thread", with(super::unsettle))));
    } else {
        out.push(leaf(item("Settle thread", with(super::settle))));
    }
    if th.snoozed(model.now) {
        out.push(leaf(item("Wake thread", with(super::unsnooze))));
    } else if th.can_snooze() {
        out.push(sub_item("Snooze", Sub::Snooze));
    } else {
        out.push(leaf(todo("Snooze")));
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
    out.push(leaf(rename));
    out.push(leaf(
        item("Regenerate title", with(|s, id| super::dispatch(s, "thread.metadata.update", id, serde_json::json!({ "regenerateTitle": true })))),
    ));
    out.push(leaf(item("Mark unread", with(|s, id| super::dispatch(s, "thread.mark-unread", id, serde_json::json!({}))))));
    if super::share::can_share_with_guest(s) {
        out.push(leaf(item("Share with guest", with(super::share::open_guest))));
    }
    let project = th.project_id.clone();
    let scoped = s.ui.shell.sidebar.project_scope.contains(&project);
    let filter = if scoped { "Show all projects".to_string() } else { format!("Filter by {}", model.project(th)) };
    out.push(leaf(
        item(filter, move |s, _| {
            let scope = &mut s.ui.shell.sidebar.project_scope;
            *scope = if scoped { Vec::new() } else { vec![project.clone()] };
            close(s);
        }),
    ));
    if super::share::can_turn_into_project(s, th) {
        let (thread, title) = (th.id.clone(), th.title.clone());
        let turn = item("Turn into project", move |s, _| {
            close(s);
            super::share::turn_into_project(s, &thread, &title);
        });
        out.push(leaf(if th.running() { todo("Turn into project") } else { turn }));
    }
    out.push(sub_item("Auto-settle behavior", Sub::AutoSettle));
    out.push(MenuEntry::Separator);
    out.push(sub_item("Copy", Sub::Copy));
    let settings_project = th.project_id.clone();
    out.push(leaf(
        item("Project settings", move |s, _| {
            close(s);
            s.ui.settings.project = Some(settings_project.clone()).filter(|p| !p.is_empty());
            super::go(s, Route::Settings("projects".into()));
        }),
    ));
    out.push(MenuEntry::Separator);
    let archive = if th.running() { todo("Archive thread") } else { item("Archive thread", with(super::request_archive)) };
    out.push(leaf(archive));
    out.push(leaf(item("Delete", with(super::request_delete))));
    out
}

/// Mounts the menu: at the pointer for a context invocation, under the clock button for snooze.
pub fn view(s: &App, _cx: &mut Cx<App>, model: &Model, th: &Thread, m: &Menu, t: &T) -> Vec<El> {
    let root = if m.snooze_only { Some(Sub::Snooze) } else { None };
    let entries = if m.bulk { bulk_entries(s, model, None) } else { entries(s, model, th, root) };
    let mut menu = kit::Menu::new("sb-menu").native(true).width(198.0).entries(entries).open(true).on_close(close);
    menu = match &m.anchor {
        Some(key) => menu.anchor(key.clone()).placement(kit::Placement::BottomStart).gutter(4.0),
        None => menu.at(m.at.0, m.at.1),
    };
    let mut layers = Vec::new();
    menu.overlay(&kit::theme(t), &mut layers);
    layers
}
