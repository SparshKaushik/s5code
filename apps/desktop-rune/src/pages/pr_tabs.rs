//! The pull request panel beside the `/pull-requests` list (RightPanelTabs as PullRequestsPage
//! uses it): every opened pull request keeps a tab until it is closed, and the panel's width is
//! dragged from its left edge and remembered.

use rune::{Color, Cursor, El, Phase, Tx, col, el, icon, row, text};
use serde_json::{Value, json};

use super::pull_requests::Ui;
use crate::App;
use crate::data::s as field;
use crate::icons as i;
use crate::kit;
use crate::theme::{self, AlphaMul, T, layout, radius};

/// usePreviewPanelInlineSize's PREVIEW_PANEL_MIN_WIDTH, PREVIEW_PANEL_MAX_WIDTH_FRACTION and
/// SIBLING_COLUMN_MIN_WIDTH.
const MIN_WIDTH: f32 = 360.0;
const MAX_FRACTION: f32 = 0.7;
const SIBLING_MIN: f32 = 360.0;
/// The web's `t3code:pull-request-panel-width`.
const WIDTH_KEY: &str = "pullRequestPanelWidth";
/// SortableTabList's PointerSensor activation distance.
const DRAG_DISTANCE: f32 = 6.0;
const TAB_GAP: f32 = 4.0;
/// closedViewStore keeps the 20 newest closed views.
const CLOSED_CAP: usize = 20;
/// The tab strip's horizontal scroll viewport.
pub(super) const STRIP: &str = "pr-tab-scroll";

#[derive(Default)]
pub struct Tabs {
    /// Each open pull request's entry, environment tag included, in strip order.
    pub open: Vec<Value>,
    /// The tab context menu: its pull request's URL and the point it opened at.
    pub menu: Option<(String, f32, f32)>,
    /// The tab being dragged and how far it has moved.
    drag: Option<(usize, f32)>,
    /// The width while the handle is dragged; it is saved when the drag ends.
    resizing: Option<f32>,
    /// Closed tabs' entries for `view.reopenClosed`, newest first.
    closed: Vec<Value>,
}

fn ui(s: &mut App) -> &mut Ui {
    &mut s.ui.pages.pull_requests
}

fn index(u: &Ui, url: &str) -> Option<usize> {
    u.tabs.open.iter().position(|e| field(e, "url") == url)
}

fn listed<'a>(s: &'a App, url: &str) -> Option<&'a Value> {
    s.ui.pages.pull_requests.result.as_ref()?["entries"].as_array()?.iter().find(|e| field(e, "url") == url)
}

/// openPullRequest (upsertSurface): a pull request opened from a row or a stack layer joins the
/// end of the strip; a tab the list no longer holds shows from the entry it kept, as
/// select_layer does.
pub(super) fn sync(s: &mut App) -> bool {
    let Some(url) = s.ui.pages.pull_requests.selected.clone() else { return false };
    let showing = s.ui.pages.detail.entry.as_ref().is_some_and(|e| field(e, "url") == url);
    match index(&s.ui.pages.pull_requests, &url) {
        None => {
            let Some(entry) = listed(s, &url).or(s.ui.pages.detail.entry.as_ref().filter(|_| showing)).cloned() else { return false };
            s.ui.pages.pull_requests.tabs.open.push(entry);
            true
        }
        Some(_) if showing || listed(s, &url).is_some() => false,
        Some(n) => {
            let entry = s.ui.pages.pull_requests.tabs.open[n].clone();
            let detail = &mut s.ui.pages.detail;
            let tab = detail.tab;
            *detail = Default::default();
            detail.tab = tab;
            detail.entry = Some(entry);
            super::pr_detail::refresh(s);
            true
        }
    }
}

/// activateSurface.
fn activate(u: &mut Ui, url: &str) {
    if index(u, url).is_some() {
        u.selected = Some(url.to_string());
        u.hidden = None;
    }
}

/// closedViewStore.remember for the tabs a close removed: the shown tab goes last, so it
/// reopens first, and a tab closed again replaces its older entry.
fn remember(u: &mut Ui, removed: Vec<Value>) {
    let active = u.selected.clone().or_else(|| u.hidden.clone());
    let (shown, rest): (Vec<Value>, Vec<Value>) = removed.into_iter().partition(|e| active.as_deref() == Some(field(e, "url")));
    for e in rest.into_iter().chain(shown) {
        let url = field(&e, "url").to_string();
        u.tabs.closed.retain(|c| field(c, "url") != url);
        u.tabs.closed.insert(0, e);
    }
    u.tabs.closed.truncate(CLOSED_CAP);
}

/// ReopenClosedViewShortcut (planNextReopen, openPullRequest): the newest closed tab that isn't
/// already showing opens at the end of the strip and shows; the key of its tab, to reveal.
pub(super) fn reopen(u: &mut Ui) -> Option<String> {
    while !u.tabs.closed.is_empty() {
        let e = u.tabs.closed.remove(0);
        let url = field(&e, "url").to_string();
        let open = index(u, &url).is_some();
        if open && u.selected.is_some() {
            continue;
        }
        let key = tab_key(&e);
        if !open {
            u.tabs.open.push(e);
        }
        u.selected = Some(url);
        u.hidden = None;
        return Some(key);
    }
    None
}

/// closeSurface: closing the shown tab shows the one that takes its place, else the one before;
/// closing the last empties the panel.
pub(super) fn close(u: &mut Ui, url: &str) {
    let Some(at) = index(u, url) else { return };
    let removed = u.tabs.open.remove(at);
    remember(u, vec![removed]);
    let next = u.tabs.open.get(at.min(u.tabs.open.len().saturating_sub(1))).map(|e| field(e, "url").to_string());
    if u.selected.as_deref() == Some(url) {
        u.selected = next;
    } else if u.hidden.as_deref() == Some(url) {
        u.hidden = next;
    }
}

/// closeOtherSurfaces: the kept tab shows.
fn close_others(u: &mut Ui, url: &str) {
    let Some(at) = index(u, url) else { return };
    let kept = u.tabs.open.remove(at);
    let removed = std::mem::replace(&mut u.tabs.open, vec![kept]);
    remember(u, removed);
    activate(u, url);
}

/// closeSurfacesToRight: the shown tab stays unless it was closed, when this one shows.
fn close_right(u: &mut Ui, url: &str) {
    let Some(at) = index(u, url) else { return };
    let removed = u.tabs.open.split_off(at + 1);
    remember(u, removed);
    if u.selected.as_deref().is_some_and(|a| index(u, a).is_none()) {
        u.selected = Some(url.to_string());
    }
    if u.hidden.as_deref().is_some_and(|a| index(u, a).is_none()) {
        u.hidden = Some(url.to_string());
    }
}

/// closeAllSurfaces.
fn close_all(u: &mut Ui) {
    let removed = std::mem::take(&mut u.tabs.open);
    remember(u, removed);
    u.selected = None;
    u.hidden = None;
}

/// moveSurface.
fn move_tab(u: &mut Ui, from: usize, to: usize) {
    if from == to || from >= u.tabs.open.len() || to >= u.tabs.open.len() {
        return;
    }
    let tab = u.tabs.open.remove(from);
    u.tabs.open.insert(to, tab);
}

/// Each tab's x along the strip and its width, from the widths alone so scrolling the strip
/// doesn't re-run the view.
fn spans(widths: &[f32]) -> Vec<(f32, f32)> {
    let mut x = 0.0;
    widths
        .iter()
        .map(|&w| {
            let at = x;
            x += w + TAB_GAP;
            (at, w)
        })
        .collect()
}

/// closestCenter: the tab whose centre lies nearest the dragged tab's, from each tab's x and width.
fn drop_index(spans: &[(f32, f32)], from: usize, dx: f32) -> Option<usize> {
    let (x, w) = *spans.get(from)?;
    let center = x + w / 2.0 + dx;
    let gap = |&(x, w): &(f32, f32)| (x + w / 2.0 - center).abs();
    spans.iter().enumerate().min_by(|a, b| gap(a.1).total_cmp(&gap(b.1))).map(|(n, _)| n)
}

/// getPreviewPanelMaxWidth: at most 70% of the window, leaving the list its minimum, but never
/// under the panel's own minimum.
fn max_width(window: f32, row: Option<f32>) -> f32 {
    let room = row.map_or(f32::INFINITY, |r| r.floor() - SIBLING_MIN);
    (window * MAX_FRACTION).floor().min(room).max(MIN_WIDTH)
}

/// useResizableWidth's clamp: an unusable width falls back to the default.
fn clamp_width(width: f32, default: f32, max: f32) -> f32 {
    let width = if width.is_finite() { width } else { default };
    width.clamp(MIN_WIDTH, max)
}

/// The panel's width and its cap: the width being dragged, else the saved one, else half the
/// window (PullRequestsPage's defaultWidth).
fn width(s: &App, cx: &mut rune::Cx<App>) -> (f32, f32) {
    let window = cx.layout_with("shell", |r| r[2]).unwrap_or(1280.0);
    let max = max_width(window, cx.layout_with("main", |r| r[2]));
    let saved = s.client_settings.values.get(WIDTH_KEY).and_then(Value::as_f64).map(|w| w as f32);
    let default = (window / 2.0).floor();
    (clamp_width(s.ui.pages.pull_requests.tabs.resizing.or(saved).unwrap_or(default), default, max), max)
}

/// RightPanelResizeHandle: the handle is the panel's left edge, so moving left widens it; the
/// width is saved once a drag that moved ends.
fn resize(s: &mut App, d: &mut rune::Drag, start: f32, max: f32) {
    if d.cancelled() {
        ui(s).tabs.resizing = None;
        return;
    }
    if d.phase == Phase::Down {
        d.data = start;
    }
    let dx = crate::desktop::from_window(d.dx, 0.0).0;
    let next = (d.data - dx).clamp(MIN_WIDTH, max);
    match d.phase {
        Phase::Down => {}
        Phase::Move => ui(s).tabs.resizing = Some(next),
        Phase::Up => {
            ui(s).tabs.resizing = None;
            if dx != 0.0 {
                s.client_settings.values.insert(WIDTH_KEY.into(), json!(next.round()));
                s.client_settings.save();
            }
        }
    }
}

/// The open tabs, and the shown pull request before `sync` has given it one.
fn shown(s: &App) -> Vec<&Value> {
    let u = &s.ui.pages.pull_requests;
    let mut out: Vec<&Value> = u.tabs.open.iter().collect();
    if let Some(url) = u.selected.as_deref().filter(|url| index(u, url).is_none())
        && let Some(e) = listed(s, url).or(s.ui.pages.detail.entry.as_ref().filter(|e| field(e, "url") == url))
    {
        out.push(e);
    }
    out
}

/// The tab's freshest state: the loaded detail, else the list's row, else what it opened with.
fn fresh<'a>(s: &'a App, stored: &'a Value) -> &'a Value {
    let url = field(stored, "url");
    let d = &s.ui.pages.detail;
    let detail = d.detail.as_ref().filter(|_| d.entry.as_ref().is_some_and(|e| field(e, "url") == url));
    detail.or_else(|| listed(s, url)).unwrap_or(stored)
}

pub(super) fn tab_key(e: &Value) -> String {
    format!("pr-tab-{}", super::pull_requests::key(e))
}

/// SortableTabList: a press shows the tab; past the activation distance the tab follows the
/// pointer and drops where its centre lands.
fn drag_tab(s: &mut App, d: &mut rune::Drag, tx: &mut Tx, n: usize, url: &str, spans: &[(f32, f32)]) {
    // handleTabAuxClick: a middle click released over the tab closes it.
    if tx.button() == rune::PointerButton::Middle {
        let over = (0.0..=d.w).contains(&d.x) && (0.0..=d.h).contains(&d.y);
        if d.phase == Phase::Up && !d.cancelled() && over {
            close(ui(s), url);
        }
        return;
    }
    let dx = crate::desktop::from_window(d.dx, 0.0).0;
    let tabs = &mut ui(s).tabs;
    match d.phase {
        Phase::Down => tabs.drag = None,
        Phase::Move => {
            if tabs.drag.is_some() || dx.abs() >= DRAG_DISTANCE {
                tabs.drag = Some((n, dx));
            }
        }
        Phase::Up => {
            let was = tabs.drag.take();
            if d.cancelled() {
                return;
            }
            match was {
                Some((from, dx)) => {
                    if let Some(to) = drop_index(spans, from, dx) {
                        move_tab(ui(s), from, to);
                    }
                    tx.instant();
                }
                None => activate(ui(s), url),
            }
        }
    }
}

/// RightPanelTab for a pull request: its state glyph, which turns into the close button on hover,
/// then `#number`.
fn tab(s: &App, t: &T, n: usize, stored: &Value, spans: &[(f32, f32)]) -> El {
    let u = &s.ui.pages.pull_requests;
    let url = field(stored, "url").to_string();
    let active = u.selected.as_deref() == Some(url.as_str());
    let title = format!("#{}", stored["number"]);
    let fg = if active { t.c.foreground } else { t.c.muted_foreground };
    let (glyph, tone) = super::pull_requests::state_glyph(t, fresh(s, stored));
    let close_url = url.clone();
    let close_button = el()
        .size(16.0, 16.0)
        .role("button")
        .label(format!("Close {title}"))
        .on_press(move |s: &mut App| close(ui(s), &close_url))
        .child(el().absolute(2.0, 2.0).size(12.0, 12.0).group_opacity(1.0, 0.0).child(icon(glyph, 12.0).size(12.0, 12.0).color(tone)))
        .child(el().absolute(0.0, 0.0).size(16.0, 16.0).radius(4.0).center().hover(t.c.muted).group_opacity(0.0, 1.0).child(icon(&i::X, 12.0).size(12.0, 12.0).color(fg)));
    let (drag_url, key_url, menu_url) = (url.clone(), url.clone(), url);
    let spans = spans.to_vec();
    let tab = row()
        .key(tab_key(stored))
        .h(24.0)
        .max_w(144.0)
        .pl(6.0)
        .pr(8.0)
        .gap(2.0)
        .radius(radius::MD)
        .center_y()
        .fill(if active { t.c.accent } else { Color::TRANSPARENT })
        .hover(if active { t.c.accent } else { t.c.accent.alpha_mul(0.6) })
        .hover_group()
        .cursor(Cursor::Pointer)
        .role("tab")
        .label(title.clone())
        .help(title.clone())
        .focusable(true)
        .pointer_buttons([rune::PointerButton::Middle])
        .on_drag(move |s: &mut App, d: &mut rune::Drag, tx: &mut Tx| drag_tab(s, d, tx, n, &drag_url, &spans))
        .on_key(move |s: &mut App, e: &rune::KeyEvent, _: &mut Tx| {
            let press = matches!(e.key, rune::Key::Enter) || matches!(e.key, rune::Key::Text(ref k) if k == " ");
            if press {
                activate(ui(s), &key_url);
            }
            press
        })
        .on_secondary_press(move |s: &mut App, at: rune::PointerPress, _: &mut Tx| ui(s).tabs.menu = Some((menu_url.clone(), at.x, at.y)))
        .child(close_button)
        .child(text(title).font(theme::TEXT_XS).color(fg).group_color(t.c.foreground).truncate().shrink());
    match u.tabs.drag {
        Some((from, dx)) if from == n => tab.offset(dx, 0.0).raise().instant_changes(),
        _ => tab,
    }
}

/// handleTabContextMenu's close items.
fn tab_menu(s: &App, t: &T, url: &str, x: f32, y: f32, overlays: &mut Vec<El>) {
    let u = &s.ui.pages.pull_requests;
    let Some(at) = index(u, url) else { return };
    let count = u.tabs.open.len();
    let pick = |act: fn(&mut Ui, &str)| {
        let url = url.to_string();
        move |s: &mut App| {
            let u = ui(s);
            u.tabs.menu = None;
            act(u, &url);
        }
    };
    kit::Menu::new("pr-tab-menu")
        .native(true)
        .width(198.0)
        .open(true)
        .at(x, y)
        .on_close(|s: &mut App| ui(s).tabs.menu = None)
        .item(kit::MenuItem::new("Close").on_select(pick(close)))
        .item(kit::MenuItem::new("Close others").disabled(count <= 1).on_select(pick(close_others)))
        .item(kit::MenuItem::new("Close to the right").disabled(at + 1 >= count).on_select(pick(close_right)))
        .item(kit::MenuItem::new("Close all").on_select(pick(|u: &mut Ui, _: &str| close_all(u))))
        .overlay(&kit::theme(t), overlays);
}

/// The tab strip, scrolling sideways once the tabs overflow (RightPanelTabs' ScrollArea), then
/// the maximize and panel toggles (PullRequestsPage's openPanelControls).
fn strip(s: &App, cx: &mut rune::Cx<App>, t: &T) -> El {
    let shown = shown(s);
    let widths: Vec<f32> = shown.iter().map(|e| cx.layout_with(tab_key(e).as_str(), |r| r[2]).unwrap_or_default()).collect();
    let spans = spans(&widths);
    let mut list = row().h_full().gap(TAB_GAP).center_y().role("tablist");
    for (n, e) in shown.iter().enumerate() {
        list = list.child(tab(s, t, n, e, &spans));
    }
    let tabs = row().key(STRIP).grow(1.0).min_w(0.0).h_full().scroll_x().scroll_edge_fade(24.0).child(list);
    row()
        .key("pr-panel-tabs")
        .w_full()
        .h(layout::TOPBAR_H)
        .shrink()
        .pl(8.0)
        .pr(12.0)
        .gap(4.0)
        .center_y()
        .window_drag()
        .child(tabs)
        .child(super::pull_requests::maximize_control(s, t))
        .child(super::pull_requests::panel_control(s, t))
}

/// The panel beside the list (PreviewPanelShell): its resize handle, the tab strip, then the
/// shown pull request; maximized, it fills the page and does not resize.
pub(super) fn panel(s: &App, cx: &mut rune::Cx<App>, t: &T, overlays: &mut Vec<El>) -> El {
    let u = &s.ui.pages.pull_requests;
    if let Some((url, x, y)) = &u.tabs.menu {
        tab_menu(s, t, url, *x, *y, overlays);
    }
    let body = col().grow(1.0).min_w(0.0).h_full().fill(t.c.background).child(strip(s, cx, t)).child(super::pr_detail::view(s, t, overlays));
    if u.maximized {
        return body.key("pr-panel");
    }
    let (width, max) = width(s, cx);
    row()
        .key("pr-panel")
        .w(width)
        .h_full()
        .child(el().w(1.0).h_full().fill(t.c.border))
        .child(body)
        .child(
            el().absolute(0.0, 0.0)
                .w(6.0)
                .h_full()
                .cursor(Cursor::ResizeColumn)
                .role("separator")
                .label("Resize panel")
                .on_drag(move |s: &mut App, d: &mut rune::Drag, _: &mut Tx| resize(s, d, width, max)),
        )
}

/// Scenes: the first `count` listed pull requests open as tabs, the panel at `width`.
pub(super) fn scene(s: &mut App, count: usize, width: Option<f32>) {
    let rows: Vec<Value> = s.ui.pages.pull_requests.result.as_ref().and_then(|r| r["entries"].as_array()).map(|a| a.iter().take(count).cloned().collect()).unwrap_or_default();
    let u = ui(s);
    u.tabs.open = rows;
    u.tabs.resizing = width;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(n: u64) -> Value {
        json!({"url": format!("https://github.com/acme/app/pull/{n}"), "host": "github.com", "repository": "acme/app", "number": n, "state": "open"})
    }

    fn url(n: u64) -> String {
        format!("https://github.com/acme/app/pull/{n}")
    }

    fn opened(numbers: &[u64], shown: u64) -> Ui {
        let mut u = Ui::default();
        u.tabs.open = numbers.iter().map(|n| pr(*n)).collect();
        u.selected = Some(url(shown));
        u
    }

    fn numbers(u: &Ui) -> Vec<u64> {
        u.tabs.open.iter().filter_map(|e| e["number"].as_u64()).collect()
    }

    #[test]
    fn opening_rows_adds_tabs_once_and_an_unlisted_tab_shows_from_its_own_entry() {
        let mut s = App::new();
        let mut tagged = pr(1);
        tagged[crate::env::TAG] = json!("remote");
        s.ui.pages.pull_requests.result = Some(json!({"entries": [tagged, pr(2)]}));
        s.ui.pages.pull_requests.selected = Some(url(1));
        assert!(sync(&mut s));
        s.ui.pages.pull_requests.selected = Some(url(2));
        assert!(sync(&mut s));
        s.ui.pages.pull_requests.selected = Some(url(1));
        sync(&mut s);
        assert_eq!(numbers(&s.ui.pages.pull_requests), [1, 2], "a pull request already open is shown, not added again");
        assert_eq!(s.ui.pages.pull_requests.tabs.open[0][crate::env::TAG], "remote", "the tab keeps the environment that listed it");

        s.ui.pages.pull_requests.tabs.open.push(pr(9));
        s.ui.pages.pull_requests.selected = Some(url(9));
        assert!(sync(&mut s));
        assert_eq!(s.ui.pages.detail.entry.as_ref().map(|e| e["number"].clone()), Some(json!(9)));
        assert!(!sync(&mut s), "once shown it is left alone");
    }

    #[test]
    fn closing_shows_the_tab_that_takes_its_place_and_the_last_close_empties_the_panel() {
        let mut u = opened(&[1, 2, 3], 2);
        close(&mut u, &url(2));
        assert_eq!((numbers(&u), u.selected.clone()), (vec![1, 3], Some(url(3))));
        close(&mut u, &url(1));
        assert_eq!((numbers(&u), u.selected.clone()), (vec![3], Some(url(3))), "closing another tab keeps the shown one");
        u.tabs.open.push(pr(4));
        u.selected = Some(url(4));
        close(&mut u, &url(4));
        assert_eq!(u.selected, Some(url(3)), "the last tab falls back to the one before");

        u.hidden = u.selected.take();
        close(&mut u, &url(3));
        assert_eq!((u.tabs.open.len(), u.selected.clone(), u.hidden.clone()), (0, None, None), "a hidden panel's last tab leaves nothing to reopen");
    }

    #[test]
    fn close_others_and_close_to_the_right_keep_or_take_the_shown_tab() {
        let mut u = opened(&[1, 2, 3, 4], 4);
        close_right(&mut u, &url(2));
        assert_eq!((numbers(&u), u.selected.clone()), (vec![1, 2], Some(url(2))), "the shown tab closed, so this one shows");
        let mut u = opened(&[1, 2, 3, 4], 1);
        close_right(&mut u, &url(2));
        assert_eq!((numbers(&u), u.selected.clone()), (vec![1, 2], Some(url(1))));

        let mut u = opened(&[1, 2, 3], 1);
        u.hidden = u.selected.take();
        close_others(&mut u, &url(3));
        assert_eq!((numbers(&u), u.selected.clone(), u.hidden.clone()), (vec![3], Some(url(3)), None), "the kept tab shows");
        close_all(&mut u);
        assert_eq!((u.tabs.open.len(), u.selected.clone()), (0, None));
    }

    #[test]
    fn a_dragged_tab_drops_where_its_centre_is_nearest() {
        let spans = [(0.0, 40.0), (44.0, 40.0), (88.0, 60.0)];
        assert_eq!(super::spans(&[40.0, 40.0, 60.0]), spans, "positions follow from the widths and the gap");
        assert_eq!(drop_index(&spans, 0, 5.0), Some(0));
        assert_eq!(drop_index(&spans, 0, 50.0), Some(1));
        assert_eq!(drop_index(&spans, 0, 80.0), Some(2));
        assert_eq!(drop_index(&spans, 2, -80.0), Some(0));
        assert_eq!(drop_index(&spans, 5, 0.0), None);

        let mut u = opened(&[1, 2, 3], 1);
        move_tab(&mut u, 0, 2);
        assert_eq!(numbers(&u), [2, 3, 1]);
        move_tab(&mut u, 2, 0);
        assert_eq!((numbers(&u), u.selected.clone()), (vec![1, 2, 3], Some(url(1))), "moving never changes the shown tab");
    }

    #[test]
    fn reopening_brings_back_the_newest_closed_tab_and_skips_ones_already_showing() {
        let reopened = |u: &mut Ui| reopen(u).and(u.selected.clone());
        let mut u = opened(&[1, 2, 3, 4], 2);
        close_all(&mut u);
        assert_eq!(reopened(&mut u), Some(url(2)), "the shown tab is remembered last, so it reopens first");
        assert_eq!(reopened(&mut u), Some(url(4)));
        assert_eq!(numbers(&u), [2, 4], "reopened tabs join the end of the strip");
        close(&mut u, &url(4));
        assert_eq!(reopened(&mut u), Some(url(4)), "a tab closed again moves to the front");
        u.tabs.open.push(pr(3));
        assert_eq!(reopened(&mut u), Some(url(1)), "a tab already showing is dropped");
        assert_eq!((reopen(&mut u), numbers(&u)), (None, vec![2, 4, 3, 1]));

        let mut u = opened(&[1, 2, 3], 1);
        close_right(&mut u, &url(1));
        assert_eq!(reopened(&mut u), Some(url(3)), "of tabs closed together the rightmost reopens first");

        let mut u = opened(&[1, 2], 2);
        close(&mut u, &url(2));
        u.tabs.open.push(pr(2));
        u.hidden = u.selected.take();
        assert_eq!(reopened(&mut u), Some(url(2)), "behind a hidden panel an open tab shows again");
        assert_eq!((numbers(&u), u.hidden.clone()), (vec![1, 2], None));

        let mut u = opened(&(1..=25).collect::<Vec<u64>>(), 1);
        close_all(&mut u);
        assert_eq!(u.tabs.closed.len(), CLOSED_CAP);
        assert_eq!(reopened(&mut u), Some(url(1)));
    }

    #[test]
    fn the_width_stays_between_the_panel_minimum_and_the_lists_room() {
        assert_eq!(max_width(1000.0, None), 700.0, "70% of the window");
        assert_eq!(max_width(1600.0, Some(1300.5)), 940.0, "the list keeps 360");
        assert_eq!(max_width(800.0, Some(600.0)), MIN_WIDTH, "never under the panel's minimum");
        assert_eq!(clamp_width(2000.0, 640.0, 940.0), 940.0);
        assert_eq!(clamp_width(100.0, 640.0, 940.0), MIN_WIDTH);
        assert_eq!(clamp_width(f32::NAN, 640.0, 940.0), 640.0);
        assert_eq!(clamp_width(500.0, 640.0, 940.0), 500.0);
    }
}
