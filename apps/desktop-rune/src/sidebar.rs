//! Sidebar v2: brand chrome, the search and action row, the Pinned and Active card lists you can
//! drag between, the Working and Settled shelves and the footer.

use rune::{Anim, Cursor, Drag, El, Phase, ScrollSpec, Spring, Tx, col, el, icon, row, spacer, text, text_input};
use rune_kit::{Kit, Spinner};

use crate::icons as i;
use crate::theme::{self, P};
use crate::ui::{self, icon_button};
use crate::{App, DragRow, Route, Status, Thread};

pub const WIDTH: f32 = 256.0;
const CARD_H: f32 = 78.0;
const SLIM_H: f32 = 36.0;
const GAP: f32 = 2.0;
const LABEL_H: f32 = 24.0;
const PLACEHOLDER_H: f32 = 36.0;
const SLOP: f32 = 6.0;

pub fn view(s: &App, p: P, k: &Kit) -> El {
    col()
        .key("sidebar")
        .w(WIDTH)
        .h_full()
        .fill(p.sidebar)
        .child(chrome(p))
        .child(header(s, p))
        .child(list(s, p, k))
        .child(shelves(s, p, k))
        .child(footer(s, p))
        .child(el().absolute(WIDTH - 1.0, 0.0).w(1.0).h_full().fill(p.border))
}

fn chrome(p: P) -> El {
    row()
        .key("sidebar-chrome")
        .h(52.0)
        .w_full()
        .pl(14.0)
        .pr(8.0)
        .gap(2.0)
        .center_y()
        .window_drag()
        .child(
            row()
                .h(28.0)
                .gap(5.0)
                .center_y()
                .child(row().size(20.0, 20.0).center().radius(5.0).fill(p.fg).child(text("S5").font(rune::tok::TextStyle::new(10.0, 700.0).lh(12.0)).color(p.bg)))
                .child(text("S5").font(theme::SM_MEDIUM).color(p.fg))
                .child(text("Code").font(theme::SM_MEDIUM).color(p.muted)),
        )
        .child(spacer())
        .child(ui::disabled_icon(p, "go-back", &i::ARROW_LEFT, 28.0))
        .child(ui::disabled_icon(p, "go-forward", &i::ARROW_RIGHT, 28.0))
}

fn header(s: &App, p: P) -> El {
    let field = row()
        .key("sidebar-search")
        .h(32.0)
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .px(8.0)
        .gap(6.0)
        .center_y()
        .radius(6.0)
        .border(1.0, p.input)
        .child(icon(&i::SEARCH, 14.0).color(p.muted))
        .child(
            text_input(&s.search, theme::SM)
                .single_line()
                .key("search-input")
                .placeholder("Search", p.faint)
                .color(p.fg)
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .on_edit(|s: &mut App| &mut s.search),
        );
    row()
        .key("sidebar-header")
        .w_full()
        .px(8.0)
        .pb(8.0)
        .gap(2.0)
        .center_y()
        .child(field)
        .child(icon_button(p, "filter-projects", &i::LIST_FILTER, 28.0, "Filter projects", |_, _| {}))
        .child(icon_button(p, "add-project", &i::FOLDER_PLUS, 28.0, "Add project", |_, _| {}))
        .child(icon_button(p, "new-thread", &i::SQUARE_PEN, 28.0, "New thread", |s, _| s.new_draft(s.current_project())))
}

/// Where each Pinned and Active card sits in the list, top down, and the Active label's top.
struct Slots {
    rows: Vec<(u64, f32)>,
}

impl Slots {
    fn top(&self, id: u64) -> f32 {
        self.rows.iter().find(|r| r.0 == id).map_or(0.0, |r| r.1)
    }
}

fn dragging(s: &App) -> bool {
    s.drag.as_ref().is_some_and(|d| d.started)
}

fn slots(s: &App, labels: bool) -> Slots {
    let mut rows = Vec::new();
    let mut y = 0.0;
    if labels {
        y += LABEL_H + GAP;
        if s.pinned.is_empty() {
            y += PLACEHOLDER_H + GAP;
        }
    }
    for &id in &s.pinned {
        rows.push((id, y));
        y += CARD_H + GAP;
    }
    if labels {
        y += LABEL_H + GAP;
    }
    for id in s.visible_active() {
        rows.push((id, y));
        y += CARD_H + GAP;
    }
    Slots { rows }
}

/// Moves the dragged row to wherever its centre now falls, so the rows around it step aside.
fn retarget(s: &mut App) {
    let Some(d) = s.drag.clone() else { return };
    let pinned: Vec<u64> = s.pinned.iter().copied().filter(|&x| x != d.id).collect();
    let active: Vec<u64> = s.visible_active().into_iter().filter(|&x| x != d.id).collect();
    let center = d.top + CARD_H / 2.0;
    let mut y = LABEL_H + GAP;
    if pinned.is_empty() {
        y += PLACEHOLDER_H + GAP;
    }
    let pinned_centers: Vec<f32> = pinned.iter().enumerate().map(|(n, _)| y + n as f32 * (CARD_H + GAP) + CARD_H / 2.0).collect();
    y += pinned.len() as f32 * (CARD_H + GAP);
    let active_label = y;
    let to_pinned = center < active_label + LABEL_H / 2.0;
    s.pinned.retain(|&x| x != d.id);
    s.active.retain(|&x| x != d.id);
    if to_pinned {
        let at = pinned_centers.iter().filter(|&&c| c < center).count();
        s.pinned.insert(at, d.id);
    } else {
        let base = active_label + LABEL_H + GAP;
        let at = (0..active.len()).filter(|&n| base + n as f32 * (CARD_H + GAP) + CARD_H / 2.0 < center).count();
        match active.get(at).and_then(|next| s.active.iter().position(|x| x == next)) {
            Some(pos) => s.active.insert(pos, d.id),
            None => s.active.push(d.id),
        }
    }
}

fn on_drag(id: u64) -> impl Fn(&mut App, &mut Drag, &mut Tx) + 'static {
    move |s: &mut App, d: &mut Drag, tx: &mut Tx| match d.phase {
        Phase::Down => {
            let top = slots(s, false).top(id);
            s.drag = Some(DragRow { id, from_pinned: s.pinned.contains(&id), press_top: top, top, started: false, before: (s.pinned.clone(), s.active.clone()) });
        }
        Phase::Move => {
            let Some(r) = s.drag.as_mut() else { return };
            if !r.started && d.dy.abs() < SLOP {
                return;
            }
            r.started = true;
            r.top = r.press_top + d.dy;
            tx.animated();
            retarget(s);
        }
        Phase::Up => {
            let Some(r) = s.drag.take() else { return };
            tx.animated();
            if !r.started {
                s.open_thread(id);
            } else if d.cancelled() {
                (s.pinned, s.active) = r.before;
            }
        }
    }
}

fn list(s: &App, p: P, k: &Kit) -> El {
    let query = s.search.text.trim().to_lowercase();
    let mut content = col().key("thread-list").w_full().gap(GAP);
    if !query.is_empty() {
        let hits: Vec<&Thread> = s.threads.iter().filter(|t| t.title.to_lowercase().contains(&query)).collect();
        if hits.is_empty() {
            content = content.child(text("No threads found").font(theme::SM).color(p.muted).px(10.0).py(8.0));
        }
        for t in hits {
            content = content.child(slim_row(s, t, p, true));
        }
        return scroll(content);
    }
    let drag = dragging(s);
    let dragged = s.drag.as_ref().filter(|d| d.started).map(|d| d.id);
    let laid = slots(s, drag);
    if drag {
        let on_pinned = dragged.is_some_and(|id| s.pinned.contains(&id));
        content = content.child(boundary(p, "pinned-label", "Pinned", on_pinned));
        if s.pinned.is_empty() {
            content = content.child(
                row().key("pin-placeholder").w_full().h(PLACEHOLDER_H).center().radius(6.0).border(1.0, p.input).child(text("Drop here to pin").font(theme::XS).color(p.faint)),
            );
        }
    }
    for &id in &s.pinned {
        if let Some(t) = s.thread(id) {
            content = content.child(card(s, t, p, k, true, &laid));
        }
    }
    if drag {
        let on_active = dragged.is_some_and(|id| s.active.contains(&id));
        content = content.child(boundary(p, "active-label", "Active", on_active));
    }
    let active = s.visible_active();
    for &id in &active {
        if let Some(t) = s.thread(id) {
            content = content.child(card(s, t, p, k, false, &laid));
        }
    }
    if s.pinned.is_empty() && active.is_empty() && s.threads.is_empty() {
        let msg = if s.connected == Some(false) { "OpenCode isn't reachable" } else { "No threads yet" };
        content = content.child(text(msg).font(theme::SM).color(p.muted).px(10.0).py(8.0));
    }
    scroll(content)
}

fn scroll(content: El) -> El {
    col().key("thread-scroll").w_full().grow(1.0).min_h(0.0).px(8.0).child(content).scroll(ScrollSpec { stick_bottom: false }).scroll_edge_fade(12.0)
}

fn boundary(p: P, key: &str, label: &str, target: bool) -> El {
    let tone = if target { p.primary } else { p.muted };
    row()
        .key(key.to_string())
        .w_full()
        .h(LABEL_H)
        .px(4.0)
        .gap(8.0)
        .center_y()
        .child(text(label.to_string()).font(theme::XS).color(tone))
        .child(el().grow(1.0).h(1.0).fill(if target { p.primary } else { p.border }))
}

fn card(s: &App, t: &Thread, p: P, k: &Kit, pinned: bool, laid: &Slots) -> El {
    let selected = s.route == Route::Thread(t.id);
    let drag = s.drag.as_ref().filter(|d| d.started && d.id == t.id);
    let project = &s.projects[t.project];
    let recede = t.status == Status::Idle && !selected;
    let mut line1 = row().key("line1").w_full().h(20.0).gap(6.0).center_y().child(ui::favicon(&project.name, project.color, 16.0));
    if pinned {
        line1 = line1.child(icon(&i::PIN, 12.0).color(p.secondary));
    }
    line1 = line1.child(text(project.name.clone()).font(theme::XS).color(p.secondary).truncate().shrink()).child(spacer());
    let verb = drag.and_then(|d| match (d.from_pinned, pinned) {
        (false, true) => Some(("Pin", &i::PIN)),
        (true, false) => Some(("Unpin", &i::PIN_OFF)),
        _ => None,
    });
    line1 = match verb {
        Some((v, glyph)) => line1.child(
            row()
                .key("verb")
                .h(20.0)
                .px(6.0)
                .gap(4.0)
                .center_y()
                .radius(4.0)
                .border(1.0, p.primary.alpha(0.4))
                .fill(p.primary.alpha(0.1))
                .child(icon(glyph, 11.0).color(p.primary))
                .child(text(v).font(theme::XS2).color(p.primary)),
        ),
        None => line1.child(status_slot(s, t, p, k)),
    };
    let title = text(t.title.clone())
        .key("title")
        .font(if recede { theme::SM } else { theme::SM_MEDIUM })
        .color(if recede { p.fg.alpha(0.75) } else { p.fg })
        .truncate()
        .w_full()
        .mt(4.0);
    let mut meta = row().key("meta").w_full().gap(6.0).mt(4.0).center_y();
    if let Some(b) = &t.branch {
        meta = meta.child(icon(&i::GIT_BRANCH, 12.0).color(p.faint)).child(text(b.clone()).font(theme::XS).color(p.faint).truncate_middle().shrink());
    }
    if let Some((add, del)) = t.diff {
        meta = meta.child(text(format!("+{add}")).font(theme::MONO_XS).color(p.success)).child(text(format!("−{del}")).font(theme::MONO_XS).color(p.error));
    }
    meta = meta.child(spacer()).child(provider_mark(p));
    let mut r = col()
        .key(format!("thread-{}", t.id))
        .role("button")
        .label(t.title.clone())
        .w_full()
        .h(CARD_H)
        .px(10.0)
        .py(8.0)
        .radius(6.0)
        .hover(p.row_hover)
        .cursor(Cursor::Pointer)
        .hover_group()
        .on_drag(on_drag(t.id))
        .child(line1)
        .child(title)
        .child(meta);
    if selected {
        r = r.fill(p.row_selected);
    }
    if let Some(d) = drag {
        let at = laid.top(t.id);
        r = r
            .fill(p.card)
            .border(1.0, p.input)
            .motion(Spring::NONE)
            .offset(0.0, d.top - at)
            .offset_motion(Anim::INSTANT)
            .raise()
            .shadow(8.0, 24.0, rune::Color::BLACK.alpha(0.45))
            .cursor(Cursor::Grabbing);
    }
    r
}

fn status_slot(s: &App, t: &Thread, p: P, k: &Kit) -> El {
    let labelled = |glyph: El, label: String, color: rune::Color| row().key("status").gap(4.0).center_y().child(glyph).child(text(label).font(theme::XS).color(color));
    match t.status {
        Status::Working(since) => labelled(
            Spinner::new().size(12.0).color(p.info).build(k),
            format!("Working {}", ui::duration(s.now_ms, since)),
            p.info,
        ),
        Status::Done => labelled(icon(&i::CIRCLE_CHECK, 14.0).color(p.success), "Done".into(), p.success),
        Status::Failed => labelled(icon(&i::INFO, 14.0).color(p.error), "Failed".into(), p.error),
        Status::Idle => text(ui::relative(s.now_ms, t.updated_ms)).key("time").font(theme::XS).color(p.faint),
    }
}

fn provider_mark(p: P) -> El {
    row().size(16.0, 16.0).center().radius(4.0).fill(p.fg.alpha(0.12)).child(text("oc").font(rune::tok::TextStyle::new(8.0, 700.0).lh(10.0)).color(p.muted))
}

fn slim_row(s: &App, t: &Thread, p: P, opaque: bool) -> El {
    let project = &s.projects[t.project];
    let id = t.id;
    let selected = s.route == Route::Thread(id);
    let mut r = row()
        .key(format!("slim-{id}"))
        .role("button")
        .label(t.title.clone())
        .w_full()
        .h(SLIM_H)
        .px(10.0)
        .gap(10.0)
        .center_y()
        .radius(6.0)
        .hover(p.row_hover)
        .cursor(Cursor::Pointer)
        .hover_group()
        .on_press(move |s: &mut App| s.open_thread(id))
        .child(ui::favicon(&project.name, project.color, 16.0).group_opacity(if opaque { 1.0 } else { 0.4 }, 1.0))
        .child(text(t.title.clone()).font(theme::SM).color(p.muted).truncate().grow(1.0).shrink().min_w(0.0))
        .child(text(ui::relative(s.now_ms, t.updated_ms)).font(theme::XS).color(p.faint));
    if selected {
        r = r.fill(p.row_selected);
    }
    r
}

fn shelf_header(p: P, key: &str, label: &str, count: usize, open: bool, toggle: fn(&mut App)) -> El {
    row()
        .key(key.to_string())
        .w_full()
        .h(32.0)
        .px(10.0)
        .gap(6.0)
        .center_y()
        .radius(6.0)
        .hover(p.row_hover)
        .cursor(Cursor::Pointer)
        .on_press(toggle)
        .child(text(label.to_string()).font(theme::XS_MEDIUM).color(p.muted))
        .child(text(count.to_string()).font(theme::XS).color(p.faint))
        .child(spacer())
        .child(icon(if open { &i::CHEVRON_DOWN } else { &i::CHEVRON_RIGHT }, 14.0).color(p.faint))
}

fn shelves(s: &App, p: P, k: &Kit) -> El {
    let mut c = col().key("shelves").w_full().px(8.0).pt(4.0).gap(GAP);
    let working = s.working();
    if !working.is_empty() {
        c = c.child(shelf_header(p, "working-shelf", "Working", working.len(), s.working_open, |s| s.working_open = !s.working_open));
        if s.working_open {
            let laid = Slots { rows: Vec::new() };
            for id in working {
                if let Some(t) = s.thread(id) {
                    c = c.child(card(s, t, p, k, false, &laid));
                }
            }
        }
    }
    c = c.child(shelf_header(p, "settled-shelf", "Settled", s.settled.len(), s.settled_open, |s| s.settled_open = !s.settled_open));
    if s.settled_open {
        for &id in s.settled.iter().take(5) {
            if let Some(t) = s.thread(id) {
                c = c.child(slim_row(s, t, p, false));
            }
        }
        if s.settled.len() > 5 {
            c = c.child(row().h(SLIM_H).px(10.0).center_y().child(text(format!("Show {} more", s.settled.len() - 5)).font(theme::SM).color(p.muted)));
        }
    }
    c
}

fn footer(s: &App, p: P) -> El {
    let (dot, label) = match s.connected {
        Some(true) => (p.success, "OpenCode"),
        Some(false) => (p.error, "OpenCode offline"),
        None => (p.faint, "Connecting…"),
    };
    row()
        .key("sidebar-footer")
        .w_full()
        .h(48.0)
        .px(8.0)
        .gap(2.0)
        .center_y()
        .child(icon_button(p, "open-settings", &i::SETTINGS, 28.0, "Settings", |s, _| s.settings = Some(0)))
        .child(icon_button(p, "pull-requests", &i::PULL_REQUEST, 28.0, "Pull requests", |_, _| {}))
        .child(icon_button(p, "usage", &i::GAUGE, 28.0, "Usage", |_, _| {}))
        .child(spacer())
        .child(row().key("connection").gap(6.0).pr(6.0).center_y().child(el().size(6.0, 6.0).radius(3.0).fill(dot)).child(text(label).font(theme::XS).color(p.muted)))
}
