//! The row context menu (`useSidebarThreadContextMenu`). Desktop shows it as a native menu, so
//! this draws the same items with native menu metrics: 22pt rows, inset separators, no icons.

use rune::{Color, Cursor, Cx, El, Key, KeyEvent, Tx, col, el, icon, row, spacer, text};

use super::Model;
use super::logic::Thread;
use crate::icons as i;
use crate::theme::{self, T};
use crate::{App, Route};

const ITEM_H: f32 = 22.0;
const PAD_X: f32 = 26.0;
const WIDTH: f32 = 198.0;

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
    pub confirm_delete: bool,
}

impl Menu {
    pub fn new(thread: String, x: f32, y: f32) -> Menu {
        Menu { thread, at: (x, y), anchor: None, sub: None, snooze_only: false, confirm_delete: false }
    }

    pub fn snooze_for(thread: String) -> Menu {
        let anchor = Some(format!("sb-snooze-{thread}"));
        Menu { thread, at: (0.0, 0.0), anchor, sub: None, snooze_only: true, confirm_delete: false }
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

enum Entry {
    Item { label: String, sub: Option<Sub>, checked: bool, act: Option<Box<dyn Fn(&mut App, &mut Tx)>> },
    Separator,
}

fn item(label: impl Into<String>, act: impl Fn(&mut App, &mut Tx) + 'static) -> Entry {
    Entry::Item { label: label.into(), sub: None, checked: false, act: Some(Box::new(act)) }
}

fn submenu(label: &str, sub: Sub) -> Entry {
    Entry::Item { label: label.into(), sub: Some(sub), checked: false, act: None }
}

fn todo(label: impl Into<String>) -> Entry {
    Entry::Item { label: label.into(), sub: None, checked: false, act: None }
}

struct Look {
    fill: Color,
    border: Color,
    separator: Color,
    text: Color,
    disabled: Color,
    highlight: Color,
}

fn look(t: &T) -> Look {
    let rgb = |r: u8, g: u8, b: u8| rune::color::hex(&format!("#{r:02x}{g:02x}{b:02x}")).unwrap_or(Color::BLACK);
    if t.dark {
        Look {
            fill: rgb(38, 38, 40),
            border: rgb(55, 55, 56),
            separator: rgb(60, 60, 61),
            text: rgb(222, 222, 223),
            disabled: rgb(222, 222, 223).alpha(0.4),
            highlight: rgb(255, 255, 255).alpha(0.1),
        }
    } else {
        Look {
            fill: rgb(246, 246, 246),
            border: rgb(214, 214, 214),
            separator: rgb(222, 222, 222),
            text: rgb(28, 28, 30),
            disabled: rgb(28, 28, 30).alpha(0.4),
            highlight: rgb(0, 0, 0).alpha(0.08),
        }
    }
}

/// "9:00 AM", as `toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })`.
fn clock(ms: i64) -> String {
    let mins = ms.rem_euclid(86_400_000) / 60_000;
    let (h, m) = (mins / 60, mins % 60);
    format!("{}:{m:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" })
}

/// `resolveSnoozePresets` over `DEFAULT_SNOOZE_PRESETS`: soonest first, one per wake minute.
pub fn snooze_presets(now: i64) -> Vec<(String, i64)> {
    const DAY: i64 = 86_400_000;
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    let today = now - now.rem_euclid(DAY);
    let at = |days: i64, h: i64| today + days * DAY + h * 3_600_000;
    // getDay(): the epoch was a Thursday (4).
    let weekday = (now.div_euclid(DAY) + 4).rem_euclid(7);
    let monday = (1 - weekday + 7).rem_euclid(7);
    let monday = if monday == 0 { 7 } else { monday };
    let when = |wake: i64, names_day: bool| {
        if names_day || wake.div_euclid(DAY) == now.div_euclid(DAY) {
            clock(wake)
        } else {
            format!("{} {}", DAYS[wake.div_euclid(DAY).rem_euclid(7) as usize], clock(wake))
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

fn snooze_entries(id: &str, now: i64) -> Vec<Entry> {
    let mut out: Vec<Entry> = snooze_presets(now)
        .into_iter()
        .map(|(label, until)| {
            let id = id.to_string();
            item(label, move |s, _| {
                super::snooze(s, &id, until);
                close(s);
            })
        })
        .collect();
    out.push(Entry::Separator);
    out.push(todo("Custom…"));
    out
}

fn copy(label: &str, value: String) -> Entry {
    item(label, move |s, tx| {
        tx.copy(&value);
        close(s);
    })
}

fn entries(s: &App, model: &Model, th: &Thread, m: &Menu, sub: Option<Sub>) -> Vec<Entry> {
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
                    let mut e = item(label, move |s, _| {
                        super::dispatch(s, "thread.auto-settle.set", &id, serde_json::json!({ "enabled": on }));
                        close(s);
                    });
                    if let Entry::Item { checked, .. } = &mut e {
                        *checked = on == enabled;
                    }
                    e
                })
                .collect();
        }
        Some(Sub::Copy) => {
            let mut out = Vec::new();
            let path = th.worktree.clone().or_else(|| {
                s.state().projects.iter().find(|p| p["id"] == th.project_id.as_str()).and_then(|p| p["workspaceRoot"].as_str().map(str::to_string))
            });
            if let Some(p) = path {
                out.push(copy("Path", p));
            }
            if let Some(b) = th.branch.clone() {
                out.push(copy("Branch", b));
            }
            out.push(copy("Thread", th.title.clone()));
            out.push(copy("Thread ID", th.id.clone()));
            return out;
        }
        None => {}
    }
    let mut out = Vec::new();
    if th.pinned {
        out.push(item("Unpin thread", with(super::unpin)));
    } else {
        out.push(item("Pin thread", with(|s, id| super::pin(s, id, None))));
    }
    if th.settled {
        out.push(item("Un-settle thread", with(super::unsettle)));
    } else {
        out.push(item("Settle thread", with(super::settle)));
    }
    if th.snoozed(model.now) {
        out.push(item("Wake thread", with(super::unsnooze)));
    } else if th.can_snooze() {
        out.push(submenu("Snooze", Sub::Snooze));
    } else {
        out.push(todo("Snooze"));
    }
    out.push(Entry::Separator);
    // Renaming edits the title in place, which the row doesn't do yet.
    out.push(todo("Rename thread"));
    out.push(item("Regenerate title", with(|s, id| super::dispatch(s, "thread.metadata.update", id, serde_json::json!({ "regenerateTitle": true })))));
    out.push(item("Mark unread", with(|s, id| super::dispatch(s, "thread.mark-unread", id, serde_json::json!({})))));
    // The sidebar has no project filter yet.
    out.push(todo(format!("Filter by {}", model.project(th))));
    out.push(submenu("Auto-settle behavior", Sub::AutoSettle));
    out.push(Entry::Separator);
    out.push(submenu("Copy", Sub::Copy));
    out.push(item("Project settings", |s, _| {
        close(s);
        super::go(s, Route::Settings("general".into()));
    }));
    out.push(Entry::Separator);
    let archive = if th.running() { todo("Archive thread") } else { item("Archive thread", with(super::archive)) };
    out.push(archive);
    // The web confirms in a dialog; here a second press confirms.
    let id = th.id.clone();
    let delete = if m.confirm_delete {
        item("Delete permanently?", move |s, _| {
            super::dispatch(s, "thread.delete", &id, serde_json::json!({}));
            super::patch(s, &id, |p| p.archived = true);
            close(s);
        })
    } else {
        item("Delete", |s, _| {
            if let Some(m) = s.ui.shell.sidebar.menu.as_mut() {
                m.confirm_delete = true;
            }
        })
    };
    out.push(delete);
    out
}

fn height(entries: &[Entry]) -> f32 {
    10.0 + 2.0 + entries.iter().map(|e| if matches!(e, Entry::Separator) { 11.0 } else { ITEM_H }).sum::<f32>()
}

fn panel(key: &str, t: &T, entries: Vec<Entry>, open: Option<Sub>, set_sub: bool) -> El {
    let l = look(t);
    let mut p = col().key(key.to_string()).role("menu").w(WIDTH).py(5.0).fill(l.fill).border(1.0, l.border).radius(theme::radius::MD).shadow(8.0, 24.0, Color::BLACK.alpha(0.35));
    for (n, e) in entries.into_iter().enumerate() {
        p = p.child(match e {
            Entry::Separator => el().w_full().h(11.0).px(13.0).center_y().child(el().w_full().h(1.0).fill(l.separator)),
            Entry::Item { label, sub, checked, act } => {
                let enabled = act.is_some() || sub.is_some();
                let color = if enabled { l.text } else { l.disabled };
                let mut r = row()
                    .key(format!("{key}-{n}"))
                    .role("menuitem")
                    .w_full()
                    .h(ITEM_H)
                    .pl(PAD_X)
                    .pr(14.0)
                    .center_y()
                    .child(if checked { icon(&i::CHECK, 12.0).color(color).absolute(9.0, 5.0) } else { el() })
                    .child(text(label).font(rune::TextStyle::new(13.0, 400.0)).color(color).single_line())
                    .child(spacer());
                if let Some(sb) = sub {
                    r = r.child(icon(&i::CHEVRON_RIGHT, 12.0).color(color));
                    if open == Some(sb) {
                        r = r.fill(l.highlight);
                    }
                    if set_sub {
                        r = r.on_hover_change(move |s: &mut App, on, _: &mut Tx| {
                            if on && let Some(m) = s.ui.shell.sidebar.menu.as_mut() {
                                m.sub = Some(sb);
                            }
                        });
                        r = r.on_press(move |s: &mut App| {
                            if let Some(m) = s.ui.shell.sidebar.menu.as_mut() {
                                m.sub = Some(sb);
                            }
                        });
                    }
                } else if let Some(act) = act {
                    r = r.hover(l.highlight).cursor(Cursor::Default).on_press_tx(move |s: &mut App, tx: &mut Tx| act(s, tx));
                    if set_sub {
                        r = r.on_hover_change(|s: &mut App, on, _: &mut Tx| {
                            if on && let Some(m) = s.ui.shell.sidebar.menu.as_mut() {
                                m.sub = None;
                            }
                        });
                    }
                } else if set_sub {
                    r = r.on_hover_change(|s: &mut App, on, _: &mut Tx| {
                        if on && let Some(m) = s.ui.shell.sidebar.menu.as_mut() {
                            m.sub = None;
                        }
                    });
                }
                r
            }
        });
    }
    p
}

fn sub_offset(entries: &[Entry], sub: Sub) -> f32 {
    let mut y = 6.0;
    for e in entries {
        match e {
            Entry::Separator => y += 11.0,
            Entry::Item { sub: Some(s), .. } if *s == sub => return y,
            Entry::Item { .. } => y += ITEM_H,
        }
    }
    y
}

/// The menu as window overlays: the panel, and the open submenu beside it. The panel is modal, so
/// Escape or a click outside closes it.
pub fn view(s: &App, cx: &mut Cx<App>, model: &Model, th: &Thread, m: &Menu, t: &T) -> Vec<El> {
    let window = cx.layout("kit-root").map_or([0.0, 0.0, 1280.0, 900.0], |r| r);
    let (mut x, mut y) = match m.anchor.as_ref().and_then(|k| cx.layout(k)) {
        Some(r) => (r[0], r[1] + r[3] + 4.0),
        None => m.at,
    };
    let root = if m.snooze_only { Some(Sub::Snooze) } else { None };
    let main = entries(s, model, th, m, root);
    let h = height(&main);
    x = x.min(window[2] - WIDTH - 4.0).max(4.0);
    y = y.min(window[3] - h - 4.0).max(4.0);
    let mut layer = col().key("sb-menu-layer").absolute(0.0, 0.0).w_full().h_full().modal(true, close);
    if let Some(sub) = m.sub.filter(|_| !m.snooze_only) {
        let top = y + sub_offset(&main, sub) - 6.0;
        let subs = entries(s, model, th, m, Some(sub));
        let sh = height(&subs);
        let sx = if x + WIDTH * 2.0 - 4.0 > window[2] { x - WIDTH + 4.0 } else { x + WIDTH - 4.0 };
        let sy = top.min(window[3] - sh - 4.0).max(4.0);
        layer = layer.child(panel("sb-menu", t, main, Some(sub), true).absolute(x, y));
        layer = layer.child(panel("sb-submenu", t, subs, None, false).absolute(sx, sy));
    } else {
        layer = layer.child(panel("sb-menu", t, main, None, !m.snooze_only).absolute(x, y));
    }
    vec![layer]
}
