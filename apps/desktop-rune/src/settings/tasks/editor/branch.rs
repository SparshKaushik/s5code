//! Web `WorktreeBaseBranchPicker`: the selected project's refs from `vcs.listRefs`, searched on
//! the server, with "Start from remote" under the list.
use super::{Editor, FULL, editor_mut, selected_project};
use crate::kit::{self, Placement};
use crate::shell::palette::error_message;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, data, env, icons};
use rune::{Cursor, El, TextEdit, col, el, icon, row, text, text_input};
use serde_json::{Value, json};
use std::cell::Cell;
use std::rc::Rc;

pub(super) const ID: &str = "branch";
/// `usePaginatedBranches`'s page.
const SEARCH_LIMIT: u64 = 100;
/// The closed picker's lookup of the chosen ref, for the trigger's "(remote)".
const LOOKUP_LIMIT: u64 = 10;

/// A listing: cwd, query and limit.
type Want = (String, String, u64);

#[derive(Default)]
pub(super) struct Refs {
    query: TextEdit,
    /// The listing `list` answers; `None` until the first lands.
    shown: Option<Want>,
    list: Vec<Value>,
    total: u64,
    /// The next page's cursor.
    next: Option<u64>,
    error: Option<String>,
    /// The `vcs.listRefs` in flight, and whether it is a next page.
    asking: Option<(u64, Want, bool)>,
    /// Set by the list's scroll when it nears the end going down (`shouldLoadNextBranchPageAfterScroll`).
    near_end: Rc<Cell<bool>>,
    last_scroll: Rc<Cell<Option<f32>>>,
    /// The chosen ref's name and `isRemote`, as last listed.
    selected: Option<(String, bool)>,
    active: usize,
}

/// `sanitizeNewRefName`: whitespace runs become `-`.
fn sanitize(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join("-").chars().take(256).collect()
}

/// `resolveBranchTriggerLabel` for a new worktree: only the server knows a local branch's remote.
fn trigger_label(value: &str, remote: Option<bool>, from_origin: bool) -> String {
    match value {
        "" => "Select ref".into(),
        _ if from_origin && remote == Some(false) => format!("From {value} (remote)"),
        _ => format!("From {value}"),
    }
}

/// `BranchPickerRefItem`'s badge.
fn badge(r: &Value, project_cwd: &str) -> Option<&'static str> {
    if r["current"] == true {
        Some("current")
    } else if r["worktreePath"].as_str().is_some_and(|w| !w.is_empty() && w != project_cwd) {
        Some("worktree")
    } else if r["isRemote"] == true {
        Some("remote")
    } else if r["isDefault"] == true {
        Some("default")
    } else {
        None
    }
}

fn cwd(s: &App, e: &Editor) -> Option<String> {
    selected_project(s, e).and_then(|p| p["workspaceRoot"].as_str().filter(|c| !c.is_empty()).map(str::to_owned))
}

/// The open picker searches; the closed one looks up the chosen ref.
fn wanted(s: &App, e: &Editor) -> Option<Want> {
    if e.draft.workspace != super::Workspace::Worktree || !super::connected(s, &e.env) {
        return None;
    }
    let cwd = cwd(s, e)?;
    if e.open.as_deref() == Some(ID) {
        return Some((cwd, sanitize(e.refs.query.text.trim()), SEARCH_LIMIT));
    }
    let value = e.draft.base_ref.text.trim();
    (!value.is_empty()).then(|| (cwd, value.to_owned(), LOOKUP_LIMIT))
}

/// Folds a reply into the picker: a first page replaces the list, a next one appends the refs it
/// doesn't already have; refs by name, first one wins.
fn land(refs: &mut Refs, want: Want, next: bool, reply: Result<Value, Value>, value: &str) {
    refs.asking = None;
    if next && refs.shown.as_ref() != Some(&want) {
        return;
    }
    if !next {
        refs.list.clear();
        refs.total = 0;
        refs.active = 0;
        refs.last_scroll.set(None);
        refs.near_end.set(false);
    }
    match reply {
        Ok(page) => {
            for r in page["refs"].as_array().into_iter().flatten() {
                if !refs.list.iter().any(|l| l["name"] == r["name"]) {
                    refs.list.push(r.clone());
                }
            }
            let total = page["totalCount"].as_u64().unwrap_or(refs.list.len() as u64);
            refs.total = refs.total.max(total);
            refs.next = page["nextCursor"].as_u64();
            refs.error = None;
            if let Some(r) = refs.list.iter().find(|r| r["name"] == value) {
                refs.selected = Some((value.to_owned(), r["isRemote"] == true));
            }
        }
        // A failed next page keeps the refs already listed, as web does.
        Err(error) => {
            refs.next = None;
            refs.error = Some(error_message(&error, "Failed to load refs."));
        }
    }
    refs.shown = Some(want);
}

/// Lands the listing in flight, then asks for the one the picker wants, one at a time.
pub(super) fn poll(s: &mut App) -> bool {
    let Some(e) = s.ui.settings.tasks.editor.as_ref() else { return false };
    let key = e.env.clone();
    let mut changed = false;
    if let Some((id, want, next)) = e.refs.asking.clone() {
        let Some(reply) = env::net_of(s, &key).and_then(|net| net.take_reply(id)) else { return false };
        if let Some(e) = editor_mut(s) {
            let value = e.draft.base_ref.text.trim().to_owned();
            land(&mut e.refs, want, next, reply, &value);
        }
        changed = true;
    }
    let Some(e) = s.ui.settings.tasks.editor.as_ref() else { return changed };
    let Some(want) = wanted(s, e) else { return changed };
    let cursor = match e.refs.shown.as_ref() == Some(&want) {
        false => None,
        // `fetchNextBranchPage`: only the open picker pages, once the list scrolls near its end.
        true if want.2 == SEARCH_LIMIT && e.refs.next.is_some() && e.refs.near_end.replace(false) => e.refs.next,
        true => return changed,
    };
    let mut params = json!({"cwd": want.0, "limit": want.2});
    if !want.1.is_empty() {
        params["query"] = json!(want.1);
    }
    if let Some(cursor) = cursor {
        params["cursor"] = json!(cursor);
    }
    let Some(id) = env::net_of(s, &key).map(|net| net.request("vcs.listRefs", params)) else { return changed };
    if let Some(e) = editor_mut(s) {
        e.refs.asking = Some((id, want, cursor.is_some()));
    }
    // "Loading more refs..." shows while the next page is in flight.
    changed || cursor.is_some()
}

fn toggle(s: &mut App) {
    let Some(e) = editor_mut(s) else { return };
    if e.open.as_deref() == Some(ID) {
        e.open = None;
        return;
    }
    e.open = Some(ID.into());
    e.refs.query = TextEdit::new("");
    e.refs.query.focused = true;
    e.refs.shown = None;
    e.refs.list.clear();
    e.refs.error = None;
    e.refs.next = None;
    e.refs.active = 0;
    e.refs.last_scroll.set(None);
    e.refs.near_end.set(false);
}

fn choose(s: &mut App, r: &Value) {
    let Some(e) = editor_mut(s) else { return };
    let name = data::s(r, "name");
    e.draft.base_ref = TextEdit::new(name);
    e.refs.selected = Some((name.to_owned(), r["isRemote"] == true));
    e.open = None;
}

fn query_mut(s: &mut App) -> &mut TextEdit {
    let ui = &mut s.ui.settings.tasks;
    match ui.editor.as_mut() {
        Some(e) => &mut e.refs.query,
        None => &mut ui.scratch,
    }
}

fn on_key(s: &mut App, ev: &rune::KeyEvent, _: &mut rune::Tx) -> bool {
    let Some(e) = editor_mut(s) else { return false };
    let len = e.refs.list.len();
    match ev.key {
        rune::Key::Down if len > 0 => e.refs.active = (e.refs.active + 1) % len,
        rune::Key::Up if len > 0 => e.refs.active = (e.refs.active + len - 1) % len,
        rune::Key::Enter => {
            if let Some(r) = e.refs.list.get(e.refs.active).cloned() {
                choose(s, &r);
            }
        }
        _ => return false,
    }
    true
}

/// Web's status line: the error, the first load, the next page, or how much of a long list shows.
fn status(refs: &Refs) -> Option<String> {
    if let Some(error) = &refs.error {
        return Some(error.clone());
    }
    if refs.shown.is_none() {
        return Some("Loading refs...".into());
    }
    if matches!(refs.asking, Some((_, _, true))) {
        return Some("Loading more refs...".into());
    }
    let shown = crate::chat::details::thousands(refs.list.len() as u64);
    refs.next.map(|_| format!("Showing {shown} of {} refs", crate::chat::details::thousands(refs.total)))
}

pub(super) fn picker(s: &App, t: &T, e: &Editor, disabled: bool, popups: &mut Vec<El>) -> El {
    let k = kit::theme(t);
    let refs = &e.refs;
    let project_cwd = cwd(s, e);
    let disabled = disabled || project_cwd.is_none();
    let open = e.open.as_deref() == Some(ID) && !disabled;
    let value = e.draft.base_ref.text.trim();
    let remote = refs.selected.as_ref().filter(|(name, _)| name == value).map(|(_, remote)| *remote);
    let muted = t.c.muted_foreground;
    let mut trigger = kit::Select::new::<&str>("task-base-ref", &[])
        .label("Base branch")
        .display(trigger_label(value, remote, e.draft.start_from_origin))
        .size("sm")
        .width(FULL)
        .leading(icon(&icons::GIT_BRANCH, 14.0).color(muted), 14.0)
        .disabled(disabled)
        .trigger(&k);
    if !disabled {
        trigger = trigger.on_press(toggle);
    }
    let popover = kit::Popover::new("task-base-ref-picker")
        .trigger(trigger)
        .label("Base branch")
        .placement(Placement::BottomStart)
        .padding(0.0)
        .open(open)
        .on_close(|s: &mut App| {
            if let Some(e) = editor_mut(s) {
                e.open = None;
            }
        });
    if !open {
        return popover.build(&k, popups);
    }
    let project_cwd = project_cwd.unwrap_or_default();
    let search = row().w_full().h(32.0).px(8.0).gap(8.0).center_y().child(icon(&icons::SEARCH, 16.0).color(muted.alpha_mul(0.8))).child(
        text_input(&refs.query, theme::TEXT_SM)
            .key("task-base-ref-search")
            .label("Search refs")
            .grow(1.0)
            .min_w(0.0)
            .font(theme::TEXT_SM)
            .color(t.c.foreground)
            .placeholder("Search refs...", muted)
            .on_edit(query_mut)
            .on_input_change(|s: &mut App, _| {
                if let Some(e) = editor_mut(s) {
                    e.refs.active = 0;
                }
            })
            .on_input_key(on_key),
    );
    let divider = || el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.6));
    let (near_end, last) = (refs.near_end.clone(), refs.last_scroll.clone());
    let waker = s.ui.panel.waker.clone();
    let mut rows = col().w_full().padding(4.0).role("listbox").label("Refs");
    if refs.shown.is_some() && refs.error.is_none() && refs.list.is_empty() {
        rows = rows.child(row().h(28.0).px(8.0).center_y().child(text("No refs found.").font(theme::TEXT_SM).color(muted)));
    }
    for (i, r) in refs.list.iter().enumerate() {
        let name = data::s(r, "name").to_owned();
        let picked = r.clone();
        let mut line = row()
            .key(format!("task-base-ref-item-{i}"))
            .w_full()
            .h(28.0)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(radius::MD)
            .cursor(Cursor::Pointer)
            .role("option")
            .label(name.clone())
            .checked(name == value)
            .child(text(name).font(theme::TEXT_SM).color(t.c.foreground).single_line().truncate_middle().grow(1.0).shrink().min_w(0.0))
            .on_hover(move |s: &mut App| {
                if let Some(e) = editor_mut(s) {
                    e.refs.active = i;
                }
            })
            .on_press(move |s: &mut App| choose(s, &picked));
        if let Some(badge) = badge(r, &project_cwd) {
            line = line.child(text(badge).font(theme::TEXT_3XS).color(muted.alpha_mul(0.45)));
        }
        if i == refs.active {
            line = line.fill(t.c.accent);
        }
        rows = rows.child(line);
    }
    let origin = row()
        .w_full()
        .px(12.0)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .child(row().grow(1.0).min_w(0.0).gap(6.0).center_y().child(icon(&icons::REFRESH_CW, 12.0).color(muted)).child(text("Start from remote").font(theme::XS_MEDIUM).color(muted).truncate()))
        .child(
            kit::Switch::new(e.draft.start_from_origin)
                .label("Start worktree from remote")
                .hide_label()
                .size("sm")
                .on_toggle(|s: &mut App| {
                    if let Some(e) = editor_mut(s) {
                        e.draft.start_from_origin ^= true;
                    }
                })
                .build(&k),
        )
        .help("Uses the latest commit of the base branch’s tracked remote branch, even if its name differs. Otherwise uses the same-named branch on origin. Uses your local branch if that remote branch is missing.");
    let mut body = col()
        .key("task-base-ref-popup")
        .w(320.0)
        .min_h(0.0)
        .child(col().w_full().px(4.0).pt(4.0).child(search))
        .child(divider())
        .child(rows.max_h(224.0).scroll(rune::ScrollSpec { stick_bottom: false }).on_scroll_offset(move |at: rune::ScrollPos| {
            if crate::chat::details::next_page_after_scroll(last.replace(Some(at.offset[1])), at.offset[1], at.max[1]) && !near_end.replace(true) {
                if let Some(w) = &waker {
                    w();
                }
            }
        }))
        .child(divider())
        .child(origin);
    if let Some(line) = status(refs) {
        body = body.child(row().w_full().px(12.0).py(6.0).child(text(line).font(theme::TEXT_XS).color(muted).wrap().role("status")));
    }
    popover.child(body).build(&k, popups)
}

/// Scenes: a project's refs with the picker open.
pub(super) fn scene(e: &mut Editor, list: Vec<Value>, total: u64, next: Option<u64>) {
    e.open = Some(ID.into());
    e.refs.total = total;
    e.refs.next = next;
    e.refs.shown = Some(Default::default());
    e.refs.list = list;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trigger_reads_like_web() {
        assert_eq!(trigger_label("", None, true), "Select ref");
        assert_eq!(trigger_label("main", None, true), "From main");
        assert_eq!(trigger_label("main", Some(false), true), "From main (remote)");
        assert_eq!(trigger_label("main", Some(false), false), "From main");
        assert_eq!(trigger_label("origin/main", Some(true), true), "From origin/main");
        assert_eq!(sanitize("feature  my branch"), "feature-my-branch");
    }

    #[test]
    fn a_listing_lands_deduplicated_and_remembers_the_chosen_ref() {
        let mut refs = Refs::default();
        let want: Want = ("/repo".into(), "main".into(), LOOKUP_LIMIT);
        let page = json!({"refs": [{"name": "main", "isRemote": false}, {"name": "main", "isRemote": true}, {"name": "origin/main", "isRemote": true}], "totalCount": 240, "nextCursor": 100});
        land(&mut refs, want.clone(), false, Ok(page), "main");
        assert_eq!(refs.list.len(), 2);
        assert_eq!(refs.selected, Some(("main".to_owned(), false)));
        assert_eq!(refs.shown, Some(want.clone()));
        assert_eq!(status(&refs).as_deref(), Some("Showing 2 of 240 refs"));
        land(&mut refs, want, false, Err(json!({"message": "not a git repository"})), "main");
        assert!(refs.list.is_empty() && refs.error.is_some());
        // The chosen ref keeps its last known remote state through a failed lookup.
        assert_eq!(refs.selected, Some(("main".to_owned(), false)));
    }

    #[test]
    fn next_pages_append_new_refs_until_the_cursor_runs_out() {
        let mut refs = Refs::default();
        let want: Want = ("/repo".into(), String::new(), SEARCH_LIMIT);
        let page = |names: &[&str], total: u64, next: Option<u64>| json!({"refs": names.iter().map(|n| json!({"name": n})).collect::<Vec<_>>(), "totalCount": total, "nextCursor": next});
        land(&mut refs, want.clone(), false, Ok(page(&["main", "dev"], 5, Some(2))), "");
        assert_eq!((refs.next, status(&refs).as_deref()), (Some(2), Some("Showing 2 of 5 refs")));
        refs.asking = Some((7, want.clone(), true));
        assert_eq!(status(&refs).as_deref(), Some("Loading more refs..."));
        land(&mut refs, want.clone(), true, Ok(page(&["dev", "feat/a"], 4, Some(4))), "");
        let names = |refs: &Refs| refs.list.iter().map(|r| data::s(r, "name").to_owned()).collect::<Vec<_>>();
        assert_eq!(names(&refs), ["main", "dev", "feat/a"], "a ref a page repeats stays once");
        assert_eq!(refs.total, 5, "the largest total wins, as web's Math.max");
        land(&mut refs, want.clone(), true, Ok(page(&["feat/b"], 5, None)), "");
        assert_eq!((refs.list.len(), refs.next, status(&refs)), (4, None, None));
        // A page for a search the picker has moved on from is dropped.
        let stale: Want = ("/repo".into(), "old".into(), SEARCH_LIMIT);
        land(&mut refs, stale, true, Ok(page(&["old/x"], 1, None)), "");
        assert_eq!(refs.list.len(), 4);
        // A failed next page keeps what is listed and stops paging.
        refs.next = Some(4);
        land(&mut refs, want.clone(), true, Err(json!({"message": "timed out"})), "");
        assert_eq!((refs.list.len(), refs.next, status(&refs).as_deref()), (4, None, Some("timed out")));
        // A new first page replaces the list.
        land(&mut refs, want, false, Ok(page(&["main"], 1, None)), "");
        assert_eq!(names(&refs), ["main"]);
    }

    #[test]
    fn badges_follow_branch_picker_ref_item() {
        assert_eq!(badge(&json!({"current": true, "isRemote": true}), "/repo"), Some("current"));
        assert_eq!(badge(&json!({"worktreePath": "/repo"}), "/repo"), None);
        assert_eq!(badge(&json!({"worktreePath": "/wt"}), "/repo"), Some("worktree"));
        assert_eq!(badge(&json!({"isRemote": true, "isDefault": true}), "/repo"), Some("remote"));
        assert_eq!(badge(&json!({"isDefault": true}), "/repo"), Some("default"));
    }
}
