//! Thread rows (`SidebarThreadRow`): cards for Pinned / Active / Working, slim rows for the
//! Snoozed and Settled shelves, and the list that lays them out and drags them.

use rune::{Color, Cursor, Cx, Drag, El, PointerPress, Phase, ScrollSpec, Tx, col, el, icon, row, spacer, text};

use super::chrome::{self, Tone};
use super::logic::{self, Item, Marker, Section, Status, Thread, Verb};
use super::{Dragging, Model, Shelf, brand};
use crate::icons as i;
use crate::theme::{self, AlphaMul, BOLD, T, mono, radius, tw};
use crate::App;

fn hue(index: usize, dark: bool) -> Color {
    const DARK: [Color; 18] = [
        tw::GRAY_400, tw::RED_400, tw::ORANGE_400, tw::AMBER_400, tw::YELLOW_400, tw::LIME_400, tw::GREEN_400, tw::EMERALD_400, tw::TEAL_400,
        tw::CYAN_400, tw::SKY_400, tw::BLUE_400, tw::INDIGO_400, tw::VIOLET_400, tw::PURPLE_400, tw::FUCHSIA_400, tw::PINK_400, tw::ROSE_400,
    ];
    const LIGHT: [Color; 18] = [
        tw::GRAY_600, tw::RED_600, tw::ORANGE_600, tw::AMBER_600, tw::YELLOW_600, tw::LIME_600, tw::GREEN_600, tw::EMERALD_600, tw::TEAL_600,
        tw::CYAN_600, tw::SKY_600, tw::BLUE_600, tw::INDIGO_600, tw::VIOLET_600, tw::PURPLE_600, tw::FUCHSIA_600, tw::PINK_600, tw::ROSE_600,
    ];
    if dark { DARK[index] } else { LIGHT[index] }
}

/// `ProjectFavicon` without an icon: the name's monogram on a 14% tint of its colour.
pub fn monogram(t: &T, name: &str, dim: bool) -> El {
    monogram_at(t, name, dim, 16.0)
}

/// The favicon's SVG scaled to `size` (its 16-unit viewBox drawn smaller).
fn monogram_at(t: &T, name: &str, dim: bool, size: f32) -> El {
    let k = size / 16.0;
    let glyphs = logic::monogram(name);
    let color = if dim { t.sb.sidebar_muted_foreground } else { hue(logic::project_color(name), t.dark) };
    // The SVG stretches two glyphs to 12px (`textLength`); tracking does the same here.
    let tracking = if glyphs.chars().count() > 1 { 1.05 } else { 0.0 };
    el().size(size, size)
        .shrink()
        .radius(size / 4.0)
        .fill(color.alpha(0.14))
        .center()
        .opacity(if dim { 0.4 } else { 1.0 })
        .child(text(glyphs).font(mono(rune::TextStyle::new(8.25 * k, BOLD)).lh(size).tracking(tracking * k)).color(color))
}

/// The provider mark at 14px, 60% opaque (`ProviderInstanceIcon`).
fn provider_icon(t: &T, model: &Model, th: &Thread, text_color: Color) -> Option<El> {
    let driver = model.drivers.get(&th.provider).map(String::as_str).unwrap_or(th.provider.as_str());
    let mark = match driver {
        "codex" => icon(&brand::OPENAI, 14.0).color(text_color),
        "claudeAgent" => icon(&brand::CLAUDE, 14.0).color(rune::color::hex("#d97757").unwrap_or(text_color)),
        "acpRegistry" => icon(&brand::ACP_REGISTRY, 14.0).color(if t.dark { Color::WHITE } else { Color::BLACK }),
        "opencode" => {
            let (frame, inner) = if t.dark { ("#F1ECEC", "#4B4646") } else { ("#211E1E", "#CFCECD") };
            el().size(14.0, 14.0)
                .child(icon(&brand::OPENCODE, 14.0).color(rune::color::hex(frame).unwrap_or(text_color)).absolute(0.0, 0.0))
                .child(icon(&brand::OPENCODE_INNER, 14.0).color(rune::color::hex(inner).unwrap_or(text_color)).absolute(0.0, 0.0))
        }
        _ => return None,
    };
    Some(mark.shrink().opacity(0.6))
}

/// The card's top-right status (`topStatus`): label, icon and colour, or none for a plain time.
fn top_status(t: &T, model: &Model, th: &Thread, status: Status) -> Option<(&'static rune::Icon, String, Color)> {
    let since = |at: Option<i64>| at.map(|a| format!(" {}", logic::duration_label(model.now - a))).unwrap_or_default();
    Some(match status {
        Status::Working => {
            let label = if th.goal_active { "Goal" } else { "Working" };
            (&i::CIRCLE_DASHED, format!("{label}{}", since(th.working_started_at())), t.sb.info_foreground)
        }
        Status::Waiting => (&i::CIRCLE_DOT_DASHED, format!("Waiting{}", since(th.run.as_ref().and_then(|r| r.completed_at))), t.sb.muted_foreground),
        Status::Approval => (&i::SHIELD_QUESTION_MARK, "Approval".into(), t.sb.warning_foreground),
        Status::Input => (&i::MESSAGE_CIRCLE_QUESTION_MARK, "Input".into(), if t.dark { tw::INDIGO_300 } else { tw::INDIGO_600 }),
        Status::Request => (&i::USER_ROUND, "Request".into(), t.sb.guest_foreground),
        Status::Limited => (&i::CIRCLE_ALERT, "Limited".into(), t.sb.warning_foreground),
        Status::Failed => (&i::CIRCLE_ALERT, "Failed".into(), t.sb.error_foreground),
        Status::Ready if th.woke(model.now) => (&i::ALARM_CLOCK, "Woke".into(), t.sb.warning_foreground),
        Status::Ready if th.unread() => (&i::CIRCLE_CHECK, "Done".into(), t.sb.success_foreground),
        _ => return None,
    })
}

fn verb_icon(v: Verb) -> &'static rune::Icon {
    match v {
        Verb::Pin => &i::PIN,
        Verb::Unpin => &i::PIN_OFF,
        Verb::Settle => &i::CIRCLE_CHECK,
        Verb::Unsettle => &i::UNDO_2,
        Verb::Wake => &i::ALARM_CLOCK_OFF,
    }
}

/// The badge that replaces the status slot while a row is dragged across a section.
fn verb_badge(t: &T, v: Verb) -> El {
    row()
        .ml(4.0)
        .h(20.0)
        .gap(4.0)
        .px(6.0)
        .radius(radius::SM)
        .border(1.0, t.sb.primary.alpha_mul(0.4))
        .fill(t.sb.primary.alpha_mul(0.1))
        .center_y()
        .shrink()
        .child(icon(verb_icon(v), 12.0).color(t.sb.primary))
        .child(text(v.label()).font(theme::weight(theme::TEXT_2XS, theme::MEDIUM)).color(t.sb.primary))
}

/// A small text action that appears over the time on hover (Snooze, Settle, Wake, Un-settle).
fn hover_action(t: &T, key: String, glyph: &'static rune::Icon, size: f32, label: Option<&str>, px: f32) -> El {
    let mut b = row()
        .key(key)
        .h_full()
        .px(px)
        .gap(if label.is_some() { 4.0 } else { 2.0 })
        .radius(radius::MD)
        .center_y()
        .hover_alone()
        .hover_group()
        .cursor(Cursor::Pointer)
        .child(icon(glyph, size).color(t.sb.muted_foreground).group_color(t.sb.foreground));
    if let Some(l) = label {
        b = b.child(text(l).font(theme::TEXT_XS).color(t.sb.muted_foreground).group_color(t.sb.foreground));
    }
    b
}

pub struct RowCtx<'a> {
    pub section: Section,
    pub hovered: bool,
    pub dragging: Option<(f32, Option<Verb>)>,
    pub over: Option<&'a str>,
    /// Where the drag projection moves this row, or `None` while it hides it.
    pub shift: Option<f32>,
}

/// Applies the drag: the dragged row follows the pointer above the rest, peers slide to their
/// projected slots (dnd-kit's transforms, without its 200ms transition).
fn place(li: El, rc: &RowCtx) -> El {
    match (rc.dragging, rc.shift) {
        (Some((dy, _)), _) => li.offset(0.0, dy).raise().instant_changes(),
        (None, Some(0.0)) => li,
        (None, Some(dy)) => li.offset(0.0, dy),
        (None, None) => li.opacity(0.0),
    }
}

/// The press/drag/menu wiring every row shares: a click opens (Cmd toggles, Shift extends the
/// selection), a drag past `DRAG_DISTANCE` moves it, a secondary press opens the menu.
fn interactive(e: El, th: &Thread, section: Section, over: Option<String>) -> El {
    let (id, hid, mid) = (th.id.clone(), th.id.clone(), th.id.clone());
    e.key(format!("sb-row-{}", th.id))
        .cursor(Cursor::Pointer)
        .focusable(true)
        .on_hover_change(move |s: &mut App, on, _: &mut Tx| {
            let ui = &mut s.ui.shell.sidebar;
            if on {
                ui.hover = Some(hid.clone());
            } else if ui.hover.as_deref() == Some(hid.as_str()) {
                ui.hover = None;
            }
        })
        .on_secondary_press(move |s: &mut App, at: PointerPress, _: &mut Tx| {
            s.ui.shell.sidebar.menu = Some(super::menu::Menu::new(mid.clone(), at.x, at.y));
        })
        .on_double_press({
            let id = th.id.clone();
            move |s: &mut App| super::start_rename(s, &id)
        })
        .on_key({
            let id = th.id.clone();
            move |s: &mut App, e: &rune::KeyEvent, _: &mut Tx| {
                match e.key {
                    rune::Key::Enter => super::open_thread(s, &id),
                    rune::Key::Function(2) => super::start_rename(s, &id),
                    _ => return false,
                }
                true
            }
        })
        .on_drag(move |s: &mut App, d: &mut Drag, tx: &mut Tx| {
            let far = d.dx.hypot(d.dy) >= super::DRAG_DISTANCE;
            match d.phase {
                Phase::Down => s.ui.shell.sidebar.card = None,
                Phase::Move => {
                    let ui = &mut s.ui.shell.sidebar;
                    if ui.drag.is_some() || far {
                        ui.drag = Some(Dragging { key: id.clone(), section, dy: d.dy });
                        ui.hover = None;
                    }
                }
                Phase::Up => {
                    let was = s.ui.shell.sidebar.drag.take();
                    if d.cancelled() {
                        return;
                    }
                    match (was, &over) {
                        (Some(_), Some(over)) => {
                            let model = Model::build(s);
                            let items = model.items(&s.ui.shell.sidebar);
                            if let Some(target) = logic::drop_target(&items, &id, over) {
                                let plan = model.plan(&id, section, &target);
                                super::apply_plan(s, &id, plan);
                            }
                            tx.instant();
                        }
                        (Some(_), None) => tx.instant(),
                        (None, _) => click(s, &id, tx.mods()),
                    }
                }
            }
        })
}

/// Opens the row's hover card after the sidebar's 150ms tooltip delay (no repaint while waiting).
fn card_trigger(li: El, id: &str) -> El {
    let id = id.to_owned();
    li.hover_delays(std::time::Duration::from_millis(150), std::time::Duration::ZERO)
        .on_hover_change(move |s: &mut App, on, _: &mut Tx| {
            let ui = &mut s.ui.shell.sidebar;
            if on {
                ui.card = Some(id.clone());
            } else if ui.card.as_deref() == Some(id.as_str()) {
                ui.card = None;
            }
        })
}

/// `ThreadHoverCard`: title, then project, environment, branch and model, right of the row.
pub fn hover_card(s: &App, model: &Model, t: &T, overlays: &mut Vec<El>) {
    let ui = &s.ui.shell.sidebar;
    let Some(id) = ui.card.as_deref() else { return };
    if ui.menu.is_some() || ui.drag.is_some() || ui.rename.as_ref().is_some_and(|(r, _)| r == id) {
        return;
    }
    let Some(th) = model.threads.iter().find(|th| th.id == id) else { return };
    let k = crate::kit::theme(t);
    let detail = |glyph: El, label: String| {
        row().gap(8.0).center_y().min_w(0.0)
            .child(glyph)
            .child(text(label).font(theme::TEXT_XS).color(t.c.foreground.alpha_mul(0.75)).truncate().shrink())
    };
    let mut rows = col().gap(6.0).pl(2.0).min_w(0.0);
    let project = model.project(th);
    if !project.is_empty() {
        rows = rows.child(detail(monogram_at(t, project, false, 12.0), project.to_string()));
    }
    let env = s.state().config.as_ref().map(|c| crate::data::s(&c["environment"], "label").to_string()).unwrap_or_default();
    if !env.is_empty() {
        rows = rows.child(detail(icon(&i::SERVER, 12.0).color(t.c.muted_foreground).shrink(), env));
    }
    if let Some(branch) = &th.branch {
        rows = rows.child(detail(icon(&i::GIT_BRANCH, 12.0).color(t.c.muted_foreground).shrink(), branch.clone()));
    }
    let raw = s.state().threads.iter().find(|v| crate::data::s(v, "id") == id);
    let slug = raw.map(|v| crate::data::s(&v["modelSelection"], "model").to_string()).unwrap_or_default();
    if let Some(driver) = model.drivers.get(&th.provider).filter(|_| !slug.is_empty()) {
        let label = crate::chat::composer::model_name(s, &th.provider, &slug);
        rows = rows.child(detail(crate::chat::composer::provider_icon(t, driver, 12.0).shrink().opacity(0.6), label));
    }
    let body = col().max_w(346.0).padding(13.0).gap(8.0)
        .child(text(th.title.clone()).font(theme::XS_MEDIUM.lh(15.0)).color(t.c.foreground).truncate())
        .child(rows);
    let surface = crate::kit::menu::glass(body.key("sb-card"), &k, false).radius(6.0);
    overlays.push(rune_kit::popover::anchored(&k, surface, &format!("sb-row-{id}"), rune_kit::Placement::RightStart, 4.0));
}

fn click(s: &mut App, id: &str, mods: rune::Mods) {
    let ui = &mut s.ui.shell.sidebar;
    if mods.cmd {
        if !ui.selected.remove(id) {
            ui.selected.insert(id.to_string());
        }
        ui.anchor = Some(id.to_string());
        return;
    }
    if mods.shift
        && let Some(anchor) = ui.anchor.clone()
    {
        let ids = Model::build(s).visible_ids(&s.ui.shell.sidebar);
        if let (Some(a), Some(b)) = (ids.iter().position(|x| *x == anchor), ids.iter().position(|x| x == id)) {
            s.ui.shell.sidebar.selected = ids[a.min(b)..=a.max(b)].iter().cloned().collect();
            return;
        }
    }
    super::open_thread(s, id);
}

/// Source-over in sRGB, as the browser composites (colours are stored linear).
fn over_srgb(top: Color, bottom: Color) -> Color {
    let enc = |v: f32| if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    let dec = |v: f32| if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    let mix = |a: f32, b: f32| dec(enc(a) * top.a + enc(b) * (1.0 - top.a));
    Color { r: mix(top.r, bottom.r), g: mix(top.g, bottom.g), b: mix(top.b, bottom.b), a: 1.0 }
}

/// The row surface: route row filled, selected rows tinted, hover lifts the rest.
fn surface(e: El, t: &T, active: bool, selected: bool, hovered: bool, dragging: bool) -> El {
    let e = e.radius(radius::MD).clip_rounded(radius::MD).press(t.sb.sidebar_row_active.alpha_mul(0.5));
    if dragging {
        // `bg-sidebar` under the row tint: the lifted card must hide what it passes over.
        let fill = over_srgb(t.sb.sidebar_row_active, t.sb.sidebar);
        e.fill(fill).shadow(10.0, 15.0, Color::BLACK.alpha(0.1)).shadow(4.0, 6.0, Color::BLACK.alpha(0.1))
    } else if active {
        let e = e.fill(t.sb.sidebar_row_active);
        if t.dark { e } else { e.border(1.0, t.sb.sidebar_border.alpha_mul(0.7)) }
    } else if selected {
        e.fill(t.sb.sidebar_row_selected)
    } else if hovered {
        e.fill(t.sb.sidebar_row_hover)
    } else {
        e
    }
}

/// The row title, or ThreadRenameInput while it is being renamed: Enter commits, Escape cancels.
fn title(s: &App, th: &Thread, t: &T, font: rune::TextStyle, color: rune::Color) -> El {
    let Some((_, edit)) = s.ui.shell.sidebar.rename.as_ref().filter(|(id, _)| *id == th.id) else {
        return text(th.title.clone()).font(font).color(color).truncate().grow(1.0).shrink();
    };
    rune::text_input(edit, theme::SM_MEDIUM)
        .key("sb-rename")
        .label("Thread title")
        .single_line()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .px(4.0)
        .radius(theme::radius::SM)
        .fill(t.c.card)
        .border(1.0, t.c.foreground)
        .color(t.c.card_foreground)
        .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
        .on_edit(|s: &mut App| &mut s.ui.shell.sidebar.rename.get_or_insert_with(|| (String::new(), rune::TextEdit::default())).1)
        .on_submit(|s: &mut App, _| super::commit_rename(s))
        .on_input_key(|s: &mut App, e, _| {
            if e.key == rune::Key::Escape {
                s.ui.shell.sidebar.rename = None;
                return true;
            }
            false
        })
}

fn card(s: &App, model: &Model, th: &Thread, t: &T, rc: &RowCtx) -> El {
    let ui = &s.ui.shell.sidebar;
    let active = model.route.as_deref() == Some(th.id.as_str());
    let selected = ui.selected.contains(&th.id);
    let hovered = rc.hovered && rc.dragging.is_none();
    let status = logic::status(th);
    let (unread, woke) = (th.unread(), th.woke(model.now));
    let receded = logic::recedes(status, unread, woke, active, selected);
    let row_text = if receded && !hovered { t.sb.sidebar_muted_foreground.alpha_mul(0.75) } else { t.sb.sidebar_foreground };
    let title_color = if receded && !hovered {
        t.sb.foreground.alpha_mul(0.75)
    } else if status == Status::Failed && !unread && !woke {
        t.sb.foreground.alpha_mul(0.95)
    } else {
        t.sb.foreground
    };

    let mut line1 = row().h(20.0).gap(6.0).center_y().child(monogram(t, model.project(th), false));
    if th.pinned {
        line1 = line1.child(icon(if hovered { &i::PIN_OFF } else { &i::PIN }, 12.0).color(t.sb.muted_foreground.alpha_mul(0.65)).shrink());
    }
    line1 = line1.child(text(model.project(th).to_string()).font(theme::TEXT_XS).color(t.sb.secondary_label).truncate().grow(1.0).shrink());
    if let Some((_, verb)) = rc.dragging {
        if let Some(v) = verb {
            line1 = line1.child(verb_badge(t, v));
        }
    } else {
        let rest = match top_status(t, model, th, status) {
            Some((glyph, label, color)) => {
                row().gap(4.0).center_y().child(icon(glyph, 16.0).color(color)).child(text(label).font(theme::XS_MEDIUM).color(color))
            }
            None => row().child(text(logic::relative_label(th.card_time(), model.now)).font(theme::TEXT_XS.features("'tnum' 1")).color(t.sb.secondary_label)),
        };
        let mut slot = row().ml(4.0).h(20.0).min_w(32.0).justify_end().center_y().shrink().child(rest.opacity(if hovered { 0.0 } else { 1.0 }));
        if hovered {
            let mut actions = row().h_full();
            if th.can_snooze() {
                let id = th.id.clone();
                actions = actions.child(hover_action(t, format!("sb-snooze-{}", th.id), &i::CLOCK, 12.0, None, 6.0).on_press_tx(move |s: &mut App, tx: &mut Tx| {
                    let _ = tx;
                    if let Some(m) = Model::build(s).thread(&id).map(|_| super::menu::Menu::snooze_for(id.clone())) {
                        s.ui.shell.sidebar.menu = Some(m);
                    }
                }));
            }
            let id = th.id.clone();
            actions = actions.child(hover_action(t, format!("sb-settle-{}", th.id), &i::CHECK, 14.0, Some("Settle"), 6.0).on_press(move |s: &mut App| super::settle(s, &id)));
            slot = slot.child(actions.pin(None, Some(0.0), Some(-4.0), Some(0.0)));
        }
        line1 = line1.child(slot);
    }

    let provider = provider_icon(t, model, th, row_text);
    let mut line2 = row().mt(4.0).gap(6.0).center_y().child(title(s, th, t, if receded { theme::TEXT_SM } else { theme::SM_MEDIUM }, title_color));
    let mut body = col().px(10.0).py(8.0).child(line1);
    match &th.branch {
        Some(branch) => {
            body = body.child(line2);
            let mut meta = row().mt(2.0).gap(6.0).center_y().child(text(branch.clone()).font(theme::TEXT_XS).color(t.sb.secondary_label).truncate().grow(1.0).shrink());
            if let Some(p) = provider {
                meta = meta.child(p);
            }
            body = body.child(meta);
        }
        None => {
            if let Some(p) = provider {
                line2 = line2.child(p);
            }
            body = body.child(line2);
        }
    }
    let mut e = surface(body.w_full(), t, active, selected, hovered, rc.dragging.is_some());
    if receded && status == Status::Working && !hovered {
        e = e.opacity(0.7);
    }
    let over = rc.over.map(str::to_string);
    place(card_trigger(el().key(format!("sb-li-{}", th.id)).w_full().py(2.0), &th.id).child(interactive(e, th, rc.section, over)), rc)
}

fn wake_label(at: i64, now: i64) -> String {
    let day = |ms: i64| ms.div_euclid(86_400_000);
    let mins = at.rem_euclid(86_400_000) / 60_000;
    let (h, m) = (mins / 60, mins % 60);
    let clock = match (h % 12, m) {
        (0, 0) => format!("12 {}", if h < 12 { "AM" } else { "PM" }),
        (hh, 0) => format!("{hh} {}", if h < 12 { "AM" } else { "PM" }),
        (hh, mm) => format!("{}:{mm:02} {}", if hh == 0 { 12 } else { hh }, if h < 12 { "AM" } else { "PM" }),
    };
    if day(at) == day(now) {
        clock
    } else {
        const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
        format!("{} {clock}", DAYS[day(at).rem_euclid(7) as usize])
    }
}

fn slim(s: &App, model: &Model, th: &Thread, t: &T, rc: &RowCtx) -> El {
    let ui = &s.ui.shell.sidebar;
    let active = model.route.as_deref() == Some(th.id.as_str());
    let selected = ui.selected.contains(&th.id);
    let hovered = rc.hovered && rc.dragging.is_none();
    let status = logic::status(th);
    let (unread, woke) = (th.unread(), th.woke(model.now));
    let receded = logic::recedes(status, unread, woke, active, selected);
    let settled = rc.section == Section::Settled;
    let title_color = if hovered || active || woke || matches!(status, Status::Input | Status::Request) {
        t.sb.foreground
    } else if settled || receded {
        t.sb.secondary_label
    } else if unread {
        t.sb.muted_foreground
    } else {
        t.sb.secondary_label.alpha_mul(0.7)
    };
    let mut r = row()
        .h(36.0)
        .px(10.0)
        .gap(10.0)
        .center_y()
        .child(monogram(t, model.project(th), !(hovered || (active && !settled))))
        .child(title(s, th, t, theme::TEXT_SM, title_color));
    if th.pinned {
        r = r.child(icon(&i::PIN, 12.0).color(t.sb.muted_foreground.alpha_mul(0.65)).shrink());
    }
    if let Some((_, verb)) = rc.dragging {
        if let Some(v) = verb {
            r = r.child(verb_badge(t, v));
        }
    } else {
        let rest = if rc.section == Section::Snoozed {
            let at = th.snoozed_until.unwrap_or(model.now);
            row().gap(4.0).center_y().child(icon(&i::ALARM_CLOCK, 12.0).color(t.sb.info_foreground)).child(text(wake_label(at, model.now)).font(theme::TEXT_XS).color(t.sb.info_foreground))
        } else {
            let at = if settled { th.settled_time() } else { th.card_time() };
            row().child(text(logic::relative_label(at, model.now)).font(theme::TEXT_XS.features("'tnum' 1")).color(t.sb.secondary_label))
        };
        let mut slot = row().ml(4.0).h(24.0).min_w(32.0).justify_end().center_y().shrink().child(rest.opacity(if hovered && !woke { 0.0 } else { 1.0 }));
        if hovered {
            let id = th.id.clone();
            let action = match rc.section {
                Section::Snoozed => hover_action(t, format!("sb-wake-{}", th.id), &i::ALARM_CLOCK_OFF, 12.0, None, 6.0).on_press(move |s: &mut App| super::unsnooze(s, &id)),
                Section::Settled => hover_action(t, format!("sb-unsettle-{}", th.id), &i::UNDO_2, 14.0, None, 6.0).on_press(move |s: &mut App| super::unsettle(s, &id)),
                _ => hover_action(t, format!("sb-settle-{}", th.id), &i::CHECK, 12.0, None, 8.0).on_press(move |s: &mut App| super::settle(s, &id)),
            };
            slot = slot.child(action.pin(None, Some(0.0), Some(-4.0), Some(0.0)));
        }
        r = r.child(slot);
    }
    let e = surface(r.w_full(), t, active, selected, hovered, rc.dragging.is_some());
    let over = rc.over.map(str::to_string);
    place(card_trigger(el().key(format!("sb-li-{}", th.id)).w_full(), &th.id).child(interactive(e, th, rc.section, over)), rc)
}

fn marker_key(m: Marker) -> String {
    format!("sb-marker-{}", m.name())
}

fn item_key(it: &Item) -> String {
    match it {
        Item::Thread { key, .. } => format!("sb-li-{key}"),
        Item::Marker(m) => marker_key(*m),
    }
}

/// dnd-kit's `closestCenter` over last frame's layout: the item whose centre is nearest the
/// dragged row's centre, moved by the pointer.
fn drag_over(cx: &Cx<App>, items: &[Item], drag: &Dragging) -> Option<String> {
    let rect = cx.layout(&format!("sb-li-{}", drag.key))?;
    let center = rect[1] + rect[3] / 2.0 + drag.dy;
    items
        .iter()
        .filter_map(|it| cx.layout(&item_key(it)).map(|r| (it.id(), (r[1] + r[3] / 2.0 - center).abs())))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// Height the Pinned / Active boundary labels open while dragging (`SIDEBAR_DRAG_LABEL_HEIGHT`).
const LABEL_H: f32 = 24.0;

fn is_shelf_header(it: Option<&Item>) -> bool {
    matches!(it, Some(Item::Marker(Marker::WorkingHeader | Marker::SnoozedHeader | Marker::SettledHeader)))
}

pub struct Projection<'a> {
    pub items: &'a [Item],
    /// Each item's layout rect (`[x, y, w, h]`), as dnd-kit measures droppables.
    pub rects: &'a [Option<[f32; 4]>],
    pub active: usize,
    pub target: &'a logic::DropTarget,
    pub settled_order: &'a [String],
    pub active_order: Option<&'a [String]>,
    pub settled_expanded: bool,
    pub route: Option<&'a str>,
    pub snoozed_count: usize,
}

/// `createSidebarSortingStrategy`: lays the list out as it would be after the drop, opening the
/// boundary labels and placeholders, and returns each item's offset (`None` hides it).
pub fn project(p: &Projection) -> Vec<Option<f32>> {
    let (items, rects) = (p.items, p.rects);
    let mut result = vec![None; items.len()];
    let (Item::Thread { key: active_key, section: active_section }, Some(first)) = (&items[p.active], rects.first().copied().flatten()) else {
        return vec![Some(0.0); items.len()];
    };
    let mut groups: [Vec<String>; 5] = Default::default();
    let gi = |s: Section| s as usize;
    let (mut card_h, mut slim_h, mut header_scale) = (None, None, None);
    for (n, it) in items.iter().enumerate() {
        match it {
            Item::Marker(_) => {
                if is_shelf_header(Some(it)) && let Some(r) = rects[n] {
                    header_scale.get_or_insert(r[3] / 32.0);
                }
            }
            Item::Thread { key, section } => {
                let h = rects[n].map(|r| r[3]);
                if matches!(section, Section::Pinned | Section::Active | Section::Working) {
                    card_h = card_h.or(h);
                } else {
                    slim_h = slim_h.or(h);
                }
                if key != active_key {
                    groups[gi(*section)].push(key.clone());
                }
            }
        }
    }
    let scale = slim_h.map(|h| h / 36.0).or(header_scale).unwrap_or(card_h.unwrap_or(82.0) / 82.0);
    let card_h = card_h.unwrap_or(82.0 * scale);
    let slim_h = slim_h.unwrap_or(36.0 * scale);
    let label_h = LABEL_H * scale;
    let section = p.target.section;
    let order: &[String] = match section {
        Section::Pinned => &p.target.pinned_order,
        Section::Settled => p.settled_order,
        _ => p.active_order.unwrap_or(&p.target.active_order),
    };
    let rank = |k: &str| order.iter().position(|x| x == k).unwrap_or(usize::MAX);
    let r = rank(active_key);
    let group = &mut groups[gi(section)];
    let at = group.iter().position(|k| rank(k) > r).unwrap_or(group.len());
    group.insert(at, active_key.clone());
    let mut settled: Vec<String> = if p.settled_order.is_empty() { groups[gi(Section::Settled)].clone() } else { p.settled_order.to_vec() };
    settled.retain(|k| k != active_key || section == Section::Settled);
    let mut visible = if p.settled_expanded { settled.clone() } else { Vec::new() };
    if let Some(route) = p.route
        && settled.iter().any(|k| k == route)
        && !visible.iter().any(|k| k == route)
    {
        visible.push(route.to_string());
    }
    groups[gi(Section::Settled)] = visible;

    let has = |m: Marker| items.iter().any(|it| *it == Item::Marker(m));
    let mut projected: Vec<Item> = vec![Item::Marker(Marker::PinnedHeader)];
    let rows = |keys: &[String], section| keys.iter().map(move |k| Item::Thread { key: k.clone(), section }).collect::<Vec<_>>();
    projected.extend(rows(&groups[gi(Section::Pinned)], Section::Pinned));
    projected.push(Item::Marker(Marker::PinnedDivider));
    if groups[gi(Section::Active)].is_empty() {
        projected.push(Item::Marker(Marker::ActivePlaceholder));
    } else {
        projected.extend(rows(&groups[gi(Section::Active)], Section::Active));
    }
    if has(Marker::WorkingHeader) {
        projected.push(Item::Marker(Marker::WorkingHeader));
        projected.extend(rows(&groups[gi(Section::Working)], Section::Working));
    }
    if !groups[gi(Section::Snoozed)].is_empty() || ((*active_section != Section::Snoozed || p.snoozed_count > 1) && has(Marker::SnoozedHeader)) {
        projected.push(Item::Marker(Marker::SnoozedHeader));
        projected.extend(rows(&groups[gi(Section::Snoozed)], Section::Snoozed));
    }
    projected.push(Item::Marker(Marker::SettledHeader));
    if groups[gi(Section::Settled)].is_empty() {
        projected.push(Item::Marker(Marker::SettledPlaceholder));
    } else {
        projected.extend(rows(&groups[gi(Section::Settled)], Section::Settled));
    }

    let index = |it: &Item| items.iter().position(|x| x.id() == it.id());
    let heights: Vec<f32> = projected
        .iter()
        .map(|it| {
            let rect = index(it).and_then(|i| rects[i]);
            let fallback = match it {
                Item::Thread { section: Section::Pinned | Section::Active | Section::Working, .. } => card_h,
                _ => slim_h,
            };
            match it {
                Item::Marker(Marker::PinnedHeader | Marker::PinnedDivider) => label_h,
                Item::Marker(Marker::ActivePlaceholder | Marker::SettledPlaceholder) => slim_h,
                Item::Thread { key, .. } if key == active_key => fallback,
                _ => rect.map_or(fallback, |r| r[3]),
            }
        })
        .collect();
    let first_shelf = items.iter().position(|it| is_shelf_header(Some(it)));
    let shelf_space = match first_shelf.filter(|i| *i > 0).map(|i| (rects[i], rects[i - 1])) {
        Some((Some(shelf), Some(before))) if shelf[1] > before[1] + before[3] + 1.0 => {
            let last = rects.iter().rev().flatten().next().map_or(0.0, |r| r[1] + r[3]);
            (last - first[1] - heights.iter().map(|h| h + 1.0).sum::<f32>() + 1.0).max(0.0)
        }
        _ => 0.0,
    };
    let mut space = shelf_space;
    let mut top = first[1];
    for (n, it) in projected.iter().enumerate() {
        if is_shelf_header(Some(it)) {
            top += space;
            space = 0.0;
        }
        if let Some(i) = index(it)
            && let Some(r) = rects[i]
        {
            result[i] = Some(top - r[1]);
        }
        top += heights[n] + 1.0;
    }
    result[p.active] = Some(0.0);
    result
}

/// The thread list (`SidebarThreadList`): markers, rows and shelves, sliding rows aside under a
/// drag as dnd-kit's vertical sorting strategy does.
pub fn list(s: &App, cx: &mut Cx<App>, model: &Model, t: &T) -> El {
    let ui = &s.ui.shell.sidebar;
    let items = model.items(ui);
    let drag = ui.drag.as_ref();
    let over = drag.and_then(|d| drag_over(cx, &items, d));
    let target = drag.zip(over.as_deref()).and_then(|(d, o)| logic::drop_target(&items, &d.key, o));
    let target_section = target.as_ref().map(|t| t.section);
    let verb = drag.and_then(|d| logic::drop_verb(d.section, target_section));
    let s_ = &model.sections;
    let shifts = match (drag, target.as_ref()) {
        (Some(d), Some(target)) => {
            let rects: Vec<_> = items.iter().map(|it| cx.layout(&item_key(it))).collect();
            let ids = |rows: &[usize]| rows.iter().map(|i| model.threads[*i].id.clone()).collect::<Vec<String>>();
            // The dropped row takes "now" as its settle / inbox-return time, so it leads.
            let lead = |rows: &[usize], from: Section| {
                let mut ids = ids(rows);
                if d.section != from {
                    ids.retain(|k| *k != d.key);
                    ids.insert(0, d.key.clone());
                }
                ids
            };
            let settled_order = lead(&s_.settled, Section::Settled);
            let active_order = model.working_shelf.then(|| lead(&s_.active, Section::Active));
            items.iter().position(|it| it.id() == d.key).map(|active| {
                project(&Projection {
                    items: &items,
                    rects: &rects,
                    active,
                    target,
                    settled_order: &settled_order,
                    active_order: active_order.as_deref(),
                    settled_expanded: ui.expanded.contains(&Shelf::Settled),
                    route: model.route.as_deref(),
                    snoozed_count: s_.snoozed.len(),
                })
            })
        }
        _ => None,
    };
    // `restrictBelowSidebarLabel`: the row stops under the Pinned label.
    let min_dy = drag.and_then(|d| Some(cx.layout(&marker_key(Marker::PinnedHeader))?[1] + LABEL_H - cx.layout(&format!("sb-li-{}", d.key))?[1]));
    let viewport = cx.layout("sb-scroll").map_or(0.0, |r| r[3]);

    if items.iter().all(|it| matches!(it, Item::Marker(_))) && model.sections.settled.is_empty() {
        return el().key("sb-scroll").w_full().grow(1.0).shrink().child(
            el().w_full().py(24.0).center().child(text("No threads yet").font(theme::TEXT_XS).color(t.sb.muted_foreground.alpha_mul(0.6))),
        );
    }

    let mut content = col().key("sb-list").w_full().min_h(viewport).px(8.0).py(8.0);
    let mut first = true;
    for (n, it) in items.iter().enumerate() {
        let shift = shifts.as_ref().map_or(Some(0.0), |v| v[n]);
        let marker_shift = shift.unwrap_or(0.0);
        let child = match it {
            Item::Thread { key, section } => {
                let Some(th) = model.thread(key) else { continue };
                let dragging = drag.filter(|d| &d.key == key).map(|d| (min_dy.map_or(d.dy, |m| d.dy.max(m)), verb));
                let rc = RowCtx { section: *section, hovered: ui.hover.as_deref() == Some(key.as_str()), dragging, over: over.as_deref(), shift };
                let e = if matches!(section, Section::Snoozed | Section::Settled) { slim(s, model, th, t, &rc) } else { card(s, model, th, t, &rc) };
                let e = if first { e } else { e.mt(1.0) };
                first = false;
                e
            }
            Item::Marker(m) => {
                let key = marker_key(*m);
                let base = |h: f32| el().key(key.clone()).w_full().h(h).offset(0.0, marker_shift);
                let dragging = drag.is_some();
                match m {
                    Marker::PinnedHeader | Marker::PinnedDivider => {
                        let (label, section) = if *m == Marker::PinnedHeader { ("Pinned", Section::Pinned) } else { ("Active", Section::Active) };
                        let e = base(0.0);
                        if dragging { e.child(chrome::boundary(t, label, target_section == Some(section))) } else { e }
                    }
                    Marker::ActivePlaceholder | Marker::SettledPlaceholder => {
                        let (label, section, rows) =
                            if *m == Marker::ActivePlaceholder { ("Active", Section::Active, &s_.active) } else { ("Settled", Section::Settled, &s_.settled) };
                        let from_here = drag.is_some_and(|d| d.section == section);
                        let hint = dragging && (rows.is_empty() || (from_here && rows.len() == 1 && target_section.is_some_and(|ts| ts != section)));
                        let e = base(0.0);
                        if hint { e.child(chrome::placeholder(t, label, target_section == Some(section))) } else { e }
                    }
                    Marker::WorkingHeader | Marker::SnoozedHeader | Marker::SettledHeader => {
                        let (shelf, count) = match m {
                            Marker::WorkingHeader => (Shelf::Working, s_.working.len()),
                            Marker::SnoozedHeader => (Shelf::Snoozed, s_.snoozed.len()),
                            _ => (Shelf::Settled, s_.settled.len()),
                        };
                        let auto = match shelf {
                            Shelf::Working => true,
                            Shelf::Snoozed => s_.working.is_empty(),
                            Shelf::Settled => s_.working.is_empty() && s_.snoozed.is_empty(),
                        };
                        if auto {
                            content = content.child(spacer().w_full());
                        }
                        let tone = if shelf == Shelf::Settled && target_section == Some(Section::Settled) {
                            Tone::Accent
                        } else if shelf == Shelf::Settled && dragging {
                            Tone::Emphasized
                        } else {
                            Tone::Muted
                        };
                        let e = el().key(key.clone()).w_full().offset(0.0, marker_shift).child(chrome::shelf_header(t, shelf, count, ui.expanded.contains(&shelf), tone).grow(1.0));
                        let e = if first { e } else { e.mt(1.0) };
                        first = false;
                        e
                    }
                }
            }
        };
        content = content.child(child);
    }
    el().key("sb-scroll").w_full().grow(1.0).shrink().scroll(ScrollSpec { stick_bottom: false }).child(content)
}
