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

fn snooze_entries(s: &App, ids: &[String], now: i64) -> Vec<MenuEntry> {
    let mut out: Vec<MenuEntry> = crate::settings::snooze_menu(s, now, Zone::Local)
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
        return snooze_entries(s, &ids, model.now);
    }
    let count = ids.len();
    let open = s.ui.shell.sidebar.menu.as_ref().and_then(|m| m.sub);
    let leaf = |i: MenuItem| MenuEntry::item(i.on_hover(|s: &mut App| set_sub(s, None)));
    // `canOperateThreads` over the threads each item acts on.
    let operate = |of: &[&Thread]| super::can_operate(s, of);
    let mut out = Vec::new();
    // Pin and title regeneration count only rows whose server config is in.
    let loaded: Vec<&Thread> = threads.iter().copied().filter(|t| super::config_of(s, t).is_some()).collect();
    let pinned_rows: Vec<&Thread> = loaded.iter().copied().filter(|t| t.pinned).collect();
    let pinned: Vec<String> = pinned_rows.iter().map(|t| t.id.clone()).collect();
    if !pinned.is_empty() {
        out.push(leaf(item(format!("Unpin ({})", pinned.len()), move |s, _| {
            close(s);
            super::request_unpin_many(s, pinned.clone());
            s.ui.shell.sidebar.selected.clear();
        })
        .disabled(!operate(&pinned_rows))));
    }
    let settle = format!("Settle ({count})");
    let settling: Vec<&Thread> = threads.iter().copied().filter(|t| !t.settled).collect();
    out.push(leaf(if settling.is_empty() || !operate(&settling) {
        todo(settle)
    } else {
        let ids = ids.clone();
        item(settle, move |s, _| {
            close(s);
            super::settle_many(s, &ids);
        })
    }));
    // `canSnoozeSelection`: every row's config is in and each can snooze, snoozed or not.
    if threads.iter().all(|t| super::config_of(s, t).is_some() && t.can_snooze(model.now)) {
        out.push(MenuEntry::item(
            MenuItem::new(format!("Snooze ({count})"))
                .submenu(bulk_entries(s, model, Some(Sub::Snooze)), open == Some(Sub::Snooze))
                .on_submenu_change(|s: &mut App, on| set_sub(s, on.then_some(Sub::Snooze)))
                .disabled(!operate(&threads)),
        ));
    }
    let shells = &s.state().threads;
    let regenerating = |id: &str| shells.iter().any(|t| t["id"] == id && !t["titleRegeneration"].is_null());
    let fresh: Vec<String> = loaded.iter().map(|t| t.id.clone()).filter(|id| !regenerating(id.as_str())).collect();
    let fresh_rows: Vec<&Thread> = threads.iter().copied().filter(|t| fresh.contains(&t.id)).collect();
    if loaded.is_empty() {
    } else if fresh.is_empty() {
        out.push(leaf(todo(format!("Regenerating… ({})", loaded.len()))));
    } else if !operate(&fresh_rows) {
        out.push(leaf(todo(format!("Regenerate titles ({})", fresh.len()))));
    } else {
        out.push(leaf(item(format!("Regenerate titles ({})", fresh.len()), move |s, _| {
            close(s);
            for id in &fresh {
                super::dispatch(s, "thread.metadata.update", id, serde_json::json!({ "regenerateTitle": true }));
            }
            s.ui.shell.sidebar.selected.clear();
        })));
    }
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
        })
        .variant("destructive")
        .disabled(!operate(&threads)),
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
        Some(Sub::Snooze) => return snooze_entries(s, std::slice::from_ref(&th.id), model.now),
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
            match path {
                Some(p) => out.push(copy("Path", p.clone(), "Path copied", p)),
                None => out.push(MenuEntry::item(item("Path", |s, _| {
                    close(s);
                    crate::shell::palette::toast_error_brief(s, "Path unavailable", Some("This thread does not have a workspace path to copy."));
                }))),
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
    // Without `orchestration:operate` the actions that change the thread are disabled; navigation,
    // read markers, sharing and copying stay (`threadActionRequiresOperate`).
    let operate = super::can_operate(s, &[th]);
    let op = |i: MenuItem| i.disabled(!operate);
    let sub_item = |label: &str, sub: Sub| match sub_item(label, sub) {
        MenuEntry::Item(i) if !operate && sub != Sub::Copy => MenuEntry::item(i.disabled(true)),
        other => other,
    };
    // Pin, settle, snooze and title regeneration wait for the row's server config.
    let config = super::config_of(s, th);
    let loaded = config.is_some();
    let mut out = Vec::new();
    let header = s.ui.shell.sidebar.menu.as_ref().is_some_and(|m| m.header);
    if header {
        out.push(leaf(item("Find in thread", |s, _| {
            close(s);
            crate::chat::timeline::find::open(s);
        })));
    }
    if let Some(branch) = th.branch.clone() {
        let (project, worktree) = (th.project_id.clone(), th.worktree.clone());
        out.push(leaf(item(format!("New thread on {branch}"), move |s, _| {
            close(s);
            crate::chat::home::start_on_branch(s, &project, &branch, worktree.as_deref());
        })));
    }
    if loaded && th.pinned {
        out.push(leaf(op(item("Unpin thread", with(super::request_unpin)))));
    } else if loaded {
        out.push(leaf(op(item("Pin thread", with(|s, id| super::pin(s, id, None))))));
    }
    if loaded && th.settled {
        out.push(leaf(op(item("Un-settle thread", with(super::unsettle)))));
    } else if loaded {
        out.push(leaf(op(item("Settle thread", with(super::settle)))));
    }
    if !loaded {
    } else if th.snoozed(model.now) {
        out.push(leaf(op(item("Wake thread", with(super::unsnooze)))));
    } else if th.can_snooze(model.now) {
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
    out.push(leaf(op(rename)));
    if loaded && th.title_regenerating {
        out.push(leaf(todo("Regenerating…")));
    } else if loaded {
        out.push(leaf(op(item("Regenerate title", with(|s, id| super::dispatch(s, "thread.metadata.update", id, serde_json::json!({ "regenerateTitle": true })))))));
    }
    out.push(leaf(item("Mark unread", with(|s, id| super::dispatch(s, "thread.mark-unread", id, serde_json::json!({}))))));
    if super::share::can_share_with_guest(s) {
        out.push(leaf(item("Share with guest", with(super::share::open_guest))));
    }
    // An archived group is not a filter choice, and the chat header's menu offers none.
    let (groups, by_project) = super::folders::group_of(s);
    if let Some(group) = by_project.get(&th.project_id).map(|n| &groups[*n]).filter(|g| !g.archived && !header) {
        let members = group.members.clone();
        let scoped = members.iter().all(|m| s.ui.shell.sidebar.project_scope.contains(m));
        let filter = if scoped { "Show all projects".to_string() } else { format!("Filter by {}", group.label) };
        out.push(leaf(op(item(filter, move |s, _| {
            s.ui.shell.sidebar.project_scope = if scoped { Vec::new() } else { members.clone() };
            close(s);
        }))));
    }
    if super::share::can_turn_into_project(s, th) {
        let (thread, title) = (th.id.clone(), th.title.clone());
        let turn = item("Turn into project", move |s, _| {
            close(s);
            super::share::turn_into_project(s, &thread, &title);
        });
        out.push(leaf(if th.running() { todo("Turn into project") } else { op(turn) }));
    }
    if config.is_some_and(|c| c["environment"]["capabilities"]["threadAutoSettleOptOut"] == true) {
        out.push(sub_item("Auto-settle behavior", Sub::AutoSettle));
    }
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
    let archive = if th.running() { todo("Archive thread") } else { op(item("Archive thread", with(super::request_archive))) };
    out.push(leaf(archive));
    out.push(leaf(op(item("Delete", with(super::request_delete)).variant("destructive"))));
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn labels(entries: &[MenuEntry]) -> Vec<String> {
        entries
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item(i) => Some(i.label_text().to_owned()),
                _ => None,
            })
            .collect()
    }

    fn state(s: &mut App) -> &mut s5_net::State {
        match &mut s.src {
            crate::data::Source::Fixture(state) => state,
            _ => unreachable!("tests run on the fixture"),
        }
    }

    #[test]
    fn the_thread_menu_gates_its_items_like_the_web() {
        let mut s = App::new();
        let model = Model::build(&s);
        let (groups, by_project) = super::super::folders::group_of(&s);
        let mut th = model.threads.iter().find(|t| by_project.contains_key(&t.project_id)).expect("a thread in a project").clone();
        let group = groups[by_project[&th.project_id]].clone();
        let filter = format!("Filter by {}", group.label);

        state(&mut s).config.get_or_insert_with(|| json!({}))["environment"]["capabilities"]["threadAutoSettleOptOut"] = json!(false);
        let shown = labels(&entries(&s, &model, &th, None));
        assert!(!shown.iter().any(|l| l == "Auto-settle behavior"), "FAILED: auto-settle needs the capability");
        assert!(shown.contains(&filter), "FAILED: the filter names the thread's group: {shown:?}");

        state(&mut s).config.as_mut().unwrap()["environment"]["capabilities"]["threadAutoSettleOptOut"] = json!(true);
        for p in state(&mut s).projects.iter_mut().filter(|p| group.members.iter().any(|m| p["id"] == m.as_str())) {
            p["archivedAt"] = json!("2026-01-01T00:00:00Z");
            p["workspaceRoot"] = serde_json::Value::Null;
        }
        let shown = labels(&entries(&s, &model, &th, None));
        assert!(shown.iter().any(|l| l == "Auto-settle behavior"));
        assert!(!shown.iter().any(|l| l.starts_with("Filter by")), "FAILED: an archived group is no filter choice");

        th.worktree = None;
        assert_eq!(labels(&entries(&s, &model, &th, Some(Sub::Copy)))[0], "Path", "FAILED: Copy keeps Path without one, as web toasts");

        th.title_regenerating = true;
        let shown = entries(&s, &model, &th, None);
        let regen = shown.iter().find_map(|e| match e { MenuEntry::Item(i) if i.label_text() == "Regenerating…" => Some(i), _ => None });
        assert!(regen.is_some_and(|i| i.is_disabled()), "FAILED: a regenerating title shows a disabled Regenerating…: {:?}", labels(&shown));

        let config = state(&mut s).config.take();
        let shown = labels(&entries(&s, &model, &th, None));
        for gated in ["Pin thread", "Unpin thread", "Settle thread", "Un-settle thread", "Snooze", "Wake thread", "Regenerating…", "Regenerate title"] {
            assert!(!shown.iter().any(|l| l == gated), "FAILED: {gated} waits for the server config: {shown:?}");
        }
        state(&mut s).config = config;

        // A turn about to start can't snooze (`hasQueuedTurnStart`): a message newer than the run.
        th.snoozed_until = None;
        th.run = None;
        th.latest_user_message_at = Some(model.now);
        let shown = entries(&s, &model, &th, None);
        assert!(shown.iter().any(|e| matches!(e, MenuEntry::Item(i) if i.label_text() == "Snooze" && i.is_disabled())), "FAILED: a queued turn disables Snooze: {:?}", labels(&shown));
        th.latest_user_message_at = Some(model.now - 3 * 60 * 1000);
        assert!(th.can_snooze(model.now), "FAILED: an old message outside the grace window doesn't hold it");

        // The chat header's menu (useThreadActionMenu): Find first, no separator after it, no filter.
        let mut header = Menu::new(th.id.clone(), 0.0, 0.0);
        header.header = true;
        s.ui.shell.sidebar.menu = Some(header);
        let shown = entries(&s, &model, &th, None);
        assert!(matches!(&shown[0], MenuEntry::Item(i) if i.label_text() == "Find in thread"));
        assert!(!matches!(shown[1], MenuEntry::Separator), "FAILED: web has no separator after Find");
        assert!(!labels(&shown).iter().any(|l| l.starts_with("Filter by") || l == "Show all projects"), "FAILED: the header menu has no project filter");
        s.ui.shell.sidebar.menu = None;

        // A session without `orchestration:operate` keeps the menu but disables what changes threads.
        crate::chat::details::set_session(&mut s, Some(Ok(json!({"authenticated": true, "permissions": ["orchestration:read"]}))));
        let shown = entries(&s, &model, &th, None);
        let disabled = |label: &str| shown.iter().any(|e| matches!(e, MenuEntry::Item(i) if i.label_text().starts_with(label) && i.is_disabled()));
        for label in ["Rename", "Archive", "Delete"] {
            assert!(disabled(label), "FAILED: {label} needs operate: {:?}", labels(&shown));
        }
    }
}
