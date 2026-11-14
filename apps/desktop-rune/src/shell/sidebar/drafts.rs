//! `SidebarDraftBlock`: unsent new-thread drafts above the thread list, each opening its draft,
//! with a discard button and web's draft menu.

use rune::{Cursor, El, PointerPress, Tx, col, el, icon, row, text};

use crate::icons as i;
use crate::kit::{self, MenuEntry, MenuItem};
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route};

#[derive(Default)]
pub struct Ui {
    /// The route's draft when the route last changed, and its preview then if it had content:
    /// the open draft's row neither appears nor leaves while it is being typed.
    frozen: Option<(String, Option<String>)>,
    /// The draft whose menu is open, and where.
    menu: Option<(String, f32, f32)>,
    hover: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct Row {
    pub key: String,
    pub project: String,
    pub label: String,
    pub preview: String,
    pub active: bool,
}

/// The new-thread draft the route shows, if it shows one.
fn route_key(s: &App) -> Option<String> {
    matches!(s.route, Route::Home { .. }).then(|| crate::chat::composer::draft_key(s, &s.route))
}

/// The draft's first prompt line, else its attachment count.
fn preview(d: &crate::chat::composer::Draft) -> String {
    let line = d.edit.text.trim().lines().next().unwrap_or("").to_string();
    if !line.is_empty() {
        return line;
    }
    let n = d.attachments.len() + d.contexts.len();
    format!("{n} attachment{}", if n == 1 { "" } else { "s" })
}

pub fn poll(s: &mut App) {
    let key = route_key(s);
    if s.ui.shell.sidebar.drafts.frozen.as_ref().map(|(k, _)| k) == key.as_ref() {
        return;
    }
    s.ui.shell.sidebar.drafts.frozen = key.map(|k| {
        let shown = s.ui.chat.composer.drafts.get(&k).filter(|d| crate::chat::composer::has_user_content(d)).map(preview);
        (k, shown)
    });
}

/// The rows, honouring the project scope; the open draft's only as it was frozen.
pub fn rows(s: &App) -> Vec<Row> {
    let ui = &s.ui.shell.sidebar;
    let (groups, by_project) = super::folders::group_of(s);
    let route = route_key(s);
    let mut out = Vec::new();
    for (key, d) in &s.ui.chat.composer.drafts {
        let Some(project) = key.strip_prefix("home:") else { continue };
        if !s.state().projects.iter().any(|p| p["id"] == project) {
            continue;
        }
        if !ui.project_scope.is_empty() && !ui.project_scope.iter().any(|p| p == project) {
            continue;
        }
        let active = route.as_ref() == Some(key);
        let preview = if active {
            match ui.drafts.frozen.as_ref().filter(|(k, _)| k == key).and_then(|(_, p)| p.clone()) {
                Some(_) if crate::chat::composer::has_user_content(d) => preview(d),
                Some(frozen) => frozen,
                None => continue,
            }
        } else if crate::chat::composer::has_user_content(d) {
            preview(d)
        } else {
            continue;
        };
        let label = by_project.get(project).map_or_else(|| project.to_string(), |n| groups[*n].label.clone());
        out.push(Row { key: key.clone(), project: project.to_string(), label, preview, active });
    }
    out.sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.key.cmp(&b.key)));
    out
}

/// `discardComposerDraft`: drops the draft, with the footer's undo.
pub fn discard(s: &mut App, key: &str) {
    let Some(stored) = crate::chat::composer::take_draft(s, key) else { return };
    let ui = &mut s.ui.shell.sidebar;
    if let Some((k, shown)) = ui.drafts.frozen.as_mut()
        && k == key
    {
        *shown = None;
    }
    ui.undos.push(super::logic::UndoAction::Discarded, key, vec![super::logic::UndoStep::RestoreDraft(stored)], crate::data::now_ms());
}

/// The block and its divider, or nothing without drafts.
pub fn view(s: &App, t: &T) -> Option<El> {
    let rows = rows(s);
    if rows.is_empty() {
        return None;
    }
    let mut block = col().key("sb-drafts").w_full();
    for r in rows {
        block = block.child(draft_row(s, t, r));
    }
    Some(block.child(el().key("sb-draft-divider").mx(10.0).my(6.0).h(1.0).fill(t.sb.sidebar_border.alpha_mul(0.6))))
}

fn draft_row(s: &App, t: &T, r: Row) -> El {
    let hovered = s.ui.shell.sidebar.drafts.hover.as_deref() == Some(r.key.as_str());
    let discard_key = r.key.clone();
    let discard = el()
        .key(format!("sb-draft-discard-{}", r.key))
        .px(4.0)
        .h(20.0)
        .center()
        .radius(radius::MD)
        .cursor(Cursor::Pointer)
        .role("button")
        .label("Discard draft")
        .help("Discard draft")
        .opacity(if hovered { 1.0 } else { 0.0 })
        .child(icon(&i::X, 12.0).color(t.sb.muted_foreground))
        .on_press(move |s: &mut App| discard(s, &discard_key));
    let line1 = row()
        .h(20.0)
        .gap(6.0)
        .center_y()
        .child(icon(&i::SQUARE_PEN, 12.0).color(t.sb.warning_foreground).shrink())
        .child(super::favicon::mark(s, t, &r.project, &r.label, false, 16.0))
        .child(text(r.label.clone()).font(theme::XS_MEDIUM).color(t.sb.secondary_label).truncate().grow(1.0).shrink())
        .child(row().ml(4.0).h(20.0).min_w(20.0).justify_end().center_y().shrink().child(discard));
    let body = col()
        .w_full()
        .h(78.0)
        .px(10.0)
        .py(8.0)
        .child(line1)
        .child(text(r.preview.clone()).font(theme::SM_MEDIUM).color(t.sb.foreground.alpha_mul(0.9)).truncate().mt(2.0));
    let surface = if r.active {
        body.fill(t.sb.sidebar_row_active)
    } else {
        body.fill(t.sb.warning.alpha_mul(if hovered { 0.08 } else { 0.04 }))
    };
    let (open, hover, menu) = (r.project.clone(), r.key.clone(), r.key.clone());
    el().key(format!("sb-draft-{}", r.key)).w_full().py(2.0).child(
        surface
            .radius(radius::MD)
            .clip_rounded(radius::MD)
            .cursor(Cursor::Pointer)
            .focusable(true)
            .focus_ring(2.0, -2.0, t.sb.ring)
            .role("button")
            .label(format!("{}, Unsent draft, {}", r.preview, r.label))
            .on_hover_change(move |s: &mut App, on, _: &mut Tx| {
                let ui = &mut s.ui.shell.sidebar.drafts;
                if on {
                    ui.hover = Some(hover.clone());
                } else if ui.hover.as_deref() == Some(hover.as_str()) {
                    ui.hover = None;
                }
            })
            .on_press(move |s: &mut App| super::go(s, Route::Home { project_id: Some(open.clone()) }))
            .on_secondary_press(move |s: &mut App, at: PointerPress, _: &mut Tx| s.ui.shell.sidebar.drafts.menu = Some((menu.clone(), at.x, at.y))),
    )
}

/// `buildDraftActionMenuItems`: Copy › Path / Branch, Project settings, Discard draft.
pub fn entries(s: &App, key: &str) -> Vec<MenuEntry> {
    let close = |s: &mut App| s.ui.shell.sidebar.drafts.menu = None;
    let project = key.strip_prefix("home:").unwrap_or("").to_string();
    // The branch and worktree this draft was started on (`start_on_branch`).
    let strategy = s.ui.chat.home.workspace.as_ref().filter(|(p, _)| *p == project).map(|(_, w)| w);
    let branch = strategy.and_then(|w| w["branch"].as_str()).filter(|b| !b.is_empty()).map(str::to_owned);
    let root = s.state().projects.iter().find(|p| p["id"] == project.as_str()).and_then(|p| p["workspaceRoot"].as_str());
    let path = strategy.and_then(|w| w["worktreePath"].as_str()).or(root).filter(|p| !p.is_empty()).map(str::to_owned);
    let copy = |label: &str, value: String, toast: &'static str| {
        MenuEntry::item(MenuItem::new(label).on_select_tx(move |s: &mut App, tx: &mut Tx| {
            tx.copy(value.clone());
            close(s);
            crate::shell::palette::toast(s, toast, Some(&value));
        }))
    };
    let mut copies = Vec::new();
    if let Some(path) = path.clone() {
        copies.push(copy("Path", path, "Path copied"));
    }
    if let Some(branch) = branch.clone() {
        copies.push(copy("Branch", branch, "Branch copied"));
    }
    let open = s.ui.shell.sidebar.drafts.menu.is_some();
    let mut out = vec![MenuEntry::item(MenuItem::new("Copy").icon(&i::COPY).disabled(path.is_none() && branch.is_none()).submenu(copies, open))];
    let (_, by_project) = super::folders::group_of(s);
    if by_project.contains_key(&project) {
        out.push(MenuEntry::item(MenuItem::new("Project settings").icon(&i::SETTINGS).on_select(move |s: &mut App| {
            close(s);
            s.ui.settings.project = Some(project.clone());
            super::go(s, Route::Settings("projects".into()));
        })));
    }
    out.push(MenuEntry::Separator);
    let key = key.to_string();
    out.push(MenuEntry::item(MenuItem::new("Discard draft").icon(&i::TRASH_2).variant("destructive").on_select(move |s: &mut App| {
        close(s);
        discard(s, &key);
    })));
    out
}

pub fn menu(s: &App, t: &T, overlays: &mut Vec<El>) {
    let Some((key, x, y)) = s.ui.shell.sidebar.drafts.menu.clone() else { return };
    let close = |s: &mut App| s.ui.shell.sidebar.drafts.menu = None;
    kit::Menu::new("sb-draft-menu").native(true).width(198.0).entries(entries(s, &key)).open(true).on_close(close).at(x, y).overlay(&kit::theme(t), overlays);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_into(s: &mut App, key: &str, prompt: &str) {
        s.ui.chat.composer.drafts.entry(key.to_string()).or_default().edit = rune::TextEdit::new(prompt);
    }

    fn labels(entries: &[MenuEntry]) -> Vec<(String, bool)> {
        entries
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item(i) => Some((i.label_text().to_owned(), i.is_disabled())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn drafts_with_content_list_and_the_open_one_freezes_at_navigation() {
        let mut s = App::new();
        let projects: Vec<String> = s.state().projects.iter().map(|p| crate::data::s(p, "id").to_string()).collect();
        let (a, b) = (projects[0].clone(), projects[1].clone());
        let (ka, kb) = (crate::chat::composer::home_key(&a), crate::chat::composer::home_key(&b));
        s.route = Route::Home { project_id: Some(a.clone()) };
        poll(&mut s);
        type_into(&mut s, &ka, "typed after opening");
        type_into(&mut s, &kb, "first line\nsecond");
        s.ui.chat.composer.drafts.entry("thread-1".into()).or_default().edit = rune::TextEdit::new("a thread draft");
        let shown = rows(&s);
        assert_eq!(shown.iter().map(|r| (r.key.as_str(), r.preview.as_str())).collect::<Vec<_>>(), vec![(kb.as_str(), "first line")], "FAILED: the open draft was empty when opened, and thread drafts aren't rows");

        s.route = Route::Home { project_id: Some(b.clone()) };
        poll(&mut s);
        let shown = rows(&s);
        assert_eq!(shown.len(), 2, "FAILED: both drafts with content list: {shown:?}");
        assert!(shown.iter().any(|r| r.key == kb && r.active), "FAILED: the open draft keeps its row");

        s.ui.shell.sidebar.project_scope = vec![a.clone()];
        assert_eq!(rows(&s).iter().map(|r| r.key.clone()).collect::<Vec<_>>(), vec![ka.clone()], "FAILED: the project scope filters drafts");
    }

    #[test]
    fn discarding_a_draft_removes_its_row_and_undo_brings_it_back() {
        let mut s = App::new();
        let project = crate::data::s(&s.state().projects[0], "id").to_string();
        let key = crate::chat::composer::home_key(&project);
        type_into(&mut s, &key, "keep me");
        discard(&mut s, &key);
        assert!(rows(&s).is_empty(), "FAILED: the discarded draft leaves");
        assert!(super::super::undo(&mut s));
        assert_eq!(rows(&s).first().map(|r| r.preview.as_str()), Some("keep me"), "FAILED: undo restores the draft");

        discard(&mut s, &key);
        type_into(&mut s, &key, "newer");
        super::super::undo(&mut s);
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Failed to restore draft"), "FAILED: undo never overwrites a newer draft");
    }

    #[test]
    fn the_draft_menu_matches_the_web() {
        let mut s = App::new();
        let project = crate::data::s(&s.state().projects[0], "id").to_string();
        let key = crate::chat::composer::home_key(&project);
        let shown = labels(&entries(&s, &key));
        assert_eq!(shown, vec![("Copy".into(), false), ("Project settings".into(), false), ("Discard draft".into(), false)]);
        let missing = labels(&entries(&s, "home:no-such-project"));
        assert_eq!(missing, vec![("Copy".into(), true), ("Discard draft".into(), false)], "FAILED: without a path or branch Copy disables");
    }
}
