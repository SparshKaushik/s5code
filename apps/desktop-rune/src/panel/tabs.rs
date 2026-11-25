use super::{Surface, closed, device, drawer, files};
use crate::{App, data, icons, kit, theme::{self, T}};
use rune::{Cx, Drag, El, Key, Phase, TextEdit, Tx, row, text, text_input};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

pub(super) const STRIP: &str = "panel-tab-scroll";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Target {
    File(String),
    Attachment(String),
    Terminal(String),
    SideChat(String),
    PullRequest(Value),
    Device { host: String, device: String, name: String, title: Option<String> },
}

impl Target {
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::PullRequest(a), Self::PullRequest(b)) => {
                ["host", "repository", "number", "projectId"].into_iter().all(|key| a[key] == b[key])
                    && a[crate::env::TAG] == b[crate::env::TAG]
            }
            (Self::Device { host: a, device: b, .. }, Self::Device { host: c, device: d, .. }) => a == c && b == d,
            _ => self == other,
        }
    }

    pub(super) fn encode(&self) -> Value {
        match self {
            Self::File(path) => json!({"kind":"file", "path":path}),
            Self::Attachment(id) => json!({"kind":"attachment", "id":id}),
            Self::Terminal(group) => json!({"kind":"terminal", "group":group}),
            Self::SideChat(id) => json!({"kind":"side-chat", "threadId":id}),
            Self::PullRequest(entry) => json!({"kind":"pull-request", "entry":entry}),
            Self::Device { host, device, name, title } => json!({"kind":"device", "host":host, "device":device, "name":name, "title":title}),
        }
    }

    pub(super) fn decode(row: &Value) -> Option<Self> {
        let read = |key: &str| row[key].as_str().filter(|s| !s.trim().is_empty()).map(str::to_owned);
        Some(match data::s(row, "kind") {
            "file" => Self::File(file_path(&read("path")?)),
            "attachment" => Self::Attachment(read("id")?),
            "terminal" => Self::Terminal(read("group")?),
            "side-chat" => Self::SideChat(read("threadId")?),
            "pull-request" => Self::PullRequest(closed::pull_request(&row["entry"])?),
            "device" => Self::Device { host: read("host")?, device: read("device")?, name: read("name").unwrap_or_default(), title: read("title") },
            _ => return None,
        })
    }

    pub(super) fn closed(&self) -> Option<closed::View> {
        Some(match self {
            Self::File(path) => closed::View::Files(path.clone()),
            Self::Attachment(id) => closed::View::Attachment(id.clone()),
            Self::SideChat(id) => closed::View::SideChat(id.clone()),
            Self::PullRequest(entry) => closed::View::PullRequest(entry.clone()),
            Self::Device { host, device, name, title } => closed::View::Device { host: host.clone(), device: device.clone(), name: name.clone(), title: title.clone() },
            Self::Terminal(_) => return None,
        })
    }
}

#[derive(Default)]
pub(super) struct Tabs {
    entries: Vec<(String, Target)>,
    pub(super) drag: Option<(Surface, f32)>,
    scroll: Arc<Mutex<f32>>,
    pub(super) reveal: Option<Surface>,
    pub(super) rename: Option<(usize, TextEdit)>,
}

impl Tabs {
    pub(super) fn get(&self, id: usize) -> Option<&Target> {
        self.entries.get(id).map(|(_, target)| target)
    }

    pub(super) fn get_mut(&mut self, id: usize) -> Option<&mut Target> {
        self.entries.get_mut(id).map(|(_, target)| target)
    }

    pub(super) fn target(&self, surface: Surface) -> Option<&Target> {
        match surface {
            Surface::Tab(id) => self.get(id),
            _ => None,
        }
    }

    pub(super) fn adopt_scope(&mut self, from: &str, to: &str) {
        for (scope, _) in &mut self.entries {
            if scope == from {
                *scope = to.to_owned();
            }
        }
    }

    pub(super) fn intern(&mut self, scope: &str, target: Target) -> usize {
        if let Some(id) = self.entries.iter().position(|(owner, old)| owner == scope && old.same(&target)) {
            if let (Target::PullRequest(entry), Target::PullRequest(mut fresh)) = (&self.entries[id].1, target) {
                if fresh["detailTab"].is_null() && !entry["detailTab"].is_null() {
                    fresh["detailTab"] = entry["detailTab"].clone();
                }
                self.entries[id].1 = Target::PullRequest(fresh);
            }
            return id;
        }
        let id = self.entries.len();
        self.entries.push((scope.to_owned(), target));
        id
    }
}

pub(super) fn file_path(path: &str) -> String {
    if path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) && path.as_bytes().get(1..3) == Some(b":/") && path[3..].bytes().all(|byte| byte == b'/') {
        return format!("{}/", &path[..2]);
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() { path.to_owned() } else { trimmed.to_owned() }
}

pub(super) fn open(app: &mut App, target: Target) -> usize {
    if matches!(app.src, crate::data::Source::Live(_)) {
        super::switch_thread(app);
    }
    let scope = app.ui.panel.stash_key.clone();
    let id = app.ui.panel.tabs.intern(&scope, target);
    let surface = Surface::Tab(id);
    super::activate(app, surface);
    app.ui.panel.tabs.reveal = Some(surface);
    id
}

pub(super) fn activate(app: &mut App, id: usize, switched: bool) {
    let Some(target) = app.ui.panel.tabs.get(id).cloned() else { return };
    match target {
        Target::File(path) if switched && files::selected(app) != path => files::open(app, path, None),
        Target::Terminal(group) => drawer::activate_group(app, &group),
        Target::SideChat(side) => {
            if let Some(net) = app.src.net() { net.watch_thread(&side); }
            if switched { crate::chat::side::focus(app, &side, true); }
        }
        Target::Device { host, device, .. } => device::select_tab(app, &host, &device),
        _ => {}
    }
}

pub(super) fn closed(app: &mut App, id: usize) {
    let Some(target) = app.ui.panel.tabs.get(id).cloned() else { return };
    match target {
        Target::File(path) if files::selected(app) == path => files::back_to_tree(app),
        Target::SideChat(side) => crate::chat::side::close_tab(app, &side),
        _ => {}
    }
}

pub(super) fn is_device(app: &App, surface: Surface) -> bool {
    surface == Surface::Device || matches!(app.ui.panel.tabs.target(surface), Some(Target::Device { .. }))
}

pub(super) fn is_terminal(app: &App, surface: Surface) -> bool {
    surface == Surface::Terminal || matches!(app.ui.panel.tabs.target(surface), Some(Target::Terminal(_)))
}

pub(super) fn title(app: &App, id: usize) -> String {
    match app.ui.panel.tabs.get(id) {
        Some(Target::File(path)) => path.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(path).into(),
        Some(Target::Attachment(id)) => crate::chat::timeline::attachments::panel_name(app, id).unwrap_or("Attachment").into(),
        Some(Target::SideChat(id)) => app.state().threads.iter().find(|t| t["id"] == id.as_str())
            .map(|t| data::s(t, "title")).filter(|s| !s.is_empty()).unwrap_or("Side chat").into(),
        Some(Target::PullRequest(entry)) => format!("#{}", entry["number"]),
        Some(Target::Terminal(group)) => drawer::group_title(app, group),
        Some(Target::Device { name, title, device, .. }) => title.clone().unwrap_or_else(|| if name.is_empty() { device.clone() } else { name.clone() }),
        None => String::new(),
    }
}

pub(super) fn glyph(app: &App, surface: Surface) -> &'static rune::Icon {
    match app.ui.panel.tabs.target(surface) {
        Some(Target::Terminal(_)) => &icons::SQUARE_TERMINAL,
        Some(Target::Device { .. }) => &icons::SMARTPHONE,
        Some(Target::PullRequest(_)) => &icons::GIT_PULL_REQUEST_ARROW,
        Some(Target::Attachment(_)) => &icons::PAPERCLIP,
        Some(Target::SideChat(_)) => &icons::MESSAGE_SQUARE,
        _ => surface.glyph(),
    }
}

pub(super) fn view(app: &App, cx: &mut Cx<App>, t: &T, id: usize, width: f32, overlays: &mut Vec<El>) -> El {
    match app.ui.panel.tabs.get(id) {
        Some(Target::File(_)) => files::view(app, t, cx.layout_with("file-editor", |r| r[2])),
        Some(Target::Terminal(_)) => drawer::surface(app, t, drawer::Owner::Panel, width - 1.),
        Some(Target::SideChat(side)) => crate::chat::side::panel(app, cx, t, side),
        Some(Target::Device { .. }) => device::view(app, t, cx.layout_with("device-stage", |r| (r[2], r[3])), overlays),
        Some(Target::PullRequest(_)) => crate::pages::pr_detail::view(app, t, overlays),
        Some(Target::Attachment(id)) => crate::chat::timeline::attachments::panel_view(app, id, t),
        None => rune::col(),
    }
}

pub(super) fn key(surface: Surface) -> String {
    format!("panel-tab-{surface:?}")
}

pub(super) fn remove(surfaces: &mut Vec<Surface>, surface: Surface) -> Option<Surface> {
    let index = surfaces.iter().position(|&s| s == surface)?;
    surfaces.remove(index);
    surfaces.get(index).or_else(|| surfaces.last()).copied()
}

fn reorder(surfaces: &mut Vec<Surface>, surface: Surface, to: usize) -> bool {
    let Some(from) = surfaces.iter().position(|&s| s == surface) else { return false };
    if from == to || to >= surfaces.len() { return false; }
    surfaces.remove(from);
    surfaces.insert(to, surface);
    true
}

pub(super) const GAP: f32 = 4.;

pub(super) fn tab_width(title: &str, audio: bool, renaming: bool) -> f32 {
    let label = if renaming { 96. } else { rune::text::advance(title, theme::TEXT_XS).ceil() };
    (32. + label + if audio { 18. } else { 0. }).min(144.)
}

pub(super) fn spans(widths: impl IntoIterator<Item = f32>) -> Vec<(f32, f32)> {
    let mut x = 0.;
    widths.into_iter().map(|w| {
        let mid = x + w / 2.;
        x += w + GAP;
        (mid, w)
    }).collect()
}

pub(super) fn strip_width(spans: &[(f32, f32)], available: f32) -> (f32, f32) {
    let content = spans.last().map_or(0., |(center, width)| center + width / 2.);
    if content <= available {
        (content, 0.)
    } else {
        // Two 24pt buttons, the gap between them, and the header's gap beside the pair.
        let viewport = (available - 50. - GAP).max(0.);
        (viewport, content - viewport)
    }
}

fn destination(spans: &[(f32, f32)], from: usize, dx: f32) -> Option<usize> {
    let (start, _) = *spans.get(from)?;
    spans.iter().enumerate().min_by(|(_, a), (_, b)| (a.0 - start - dx).abs().total_cmp(&(b.0 - start - dx).abs())).map(|(index, _)| index)
}

pub(super) fn drag(app: &mut App, surface: Surface, d: &mut Drag, tx: &mut Tx, spans: &[(f32, f32)]) {
    if tx.button() == rune::PointerButton::Middle {
        if d.phase == Phase::Up && !d.cancelled() && (0.0..=d.w).contains(&d.x) && (0.0..=d.h).contains(&d.y) {
            super::close(app, surface);
        }
        return;
    }
    let dx = crate::desktop::from_window(d.dx, 0.).0;
    match d.phase {
        Phase::Down => app.ui.panel.tabs.drag = None,
        Phase::Move if app.ui.panel.tabs.drag.is_some() || dx.abs() >= 6. => app.ui.panel.tabs.drag = Some((surface, dx)),
        Phase::Move => {}
        Phase::Up => {
            let moved = app.ui.panel.tabs.drag.take();
            if d.cancelled() { return; }
            if moved.is_some() {
                let surfaces = &mut app.ui.panel.surfaces;
                if let Some(from) = surfaces.iter().position(|&s| s == surface)
                    && let Some(to) = destination(spans, from, dx)
                    && reorder(surfaces, surface, to)
                {
                    tx.instant();
                }
            } else {
                super::activate(app, surface);
            }
        }
    }
}

pub(super) fn dragged(app: &App, surface: Surface, tab: El) -> El {
    match app.ui.panel.tabs.drag {
        Some((drag, dx)) if drag == surface => tab.offset(dx, 0.).raise().instant_changes(),
        _ => tab,
    }
}

pub(super) fn strip(app: &App, tabs: El, width: f32) -> El {
    let scroll = app.ui.panel.tabs.scroll.clone();
    let wake = app.ui.panel.waker.clone();
    row().key(STRIP).w(width).min_w(0.).h_full().scroll_x().scroll_edge_fade(24.)
        .on_scroll_offset(move |at: rune::ScrollPos| {
            let mut prior = scroll.lock().unwrap_or_else(|error| error.into_inner());
            let next = at.offset[0];
            if *prior != next {
                *prior = next;
                if let Some(waker) = &wake {
                    waker();
                }
            }
        })
        .child(tabs)
}

pub(super) fn overflow(app: &App, t: &T, max: f32) -> Option<El> {
    if max <= 0. { return None; }
    let offset = *app.ui.panel.tabs.scroll.lock().unwrap_or_else(|error| error.into_inner());
    let offset = offset.clamp(0., max);
    let arrow = |key: &str, label: &str, glyph: &'static rune::Icon, enabled: bool, target: f32| {
        kit::Button::new(label).icon(glyph).variant("ghost").size("icon-xs").disabled(!enabled).build(t)
            .key(key).help(label).on_press_tx(move |_: &mut App, tx: &mut Tx| tx.scroll_to(STRIP, target, 0.))
    };
    Some(row().gap(2.).center_y()
        .child(arrow("panel-tabs-scroll-left", "Scroll tabs left", &icons::CHEVRON_LEFT, offset > 0.5, (offset - 160.).max(0.)))
        .child(arrow("panel-tabs-scroll-right", "Scroll tabs right", &icons::CHEVRON_RIGHT, offset < max - 0.5, (offset + 160.).min(max))))
}

pub(super) fn rename(app: &mut App, surface: Surface) {
    let Surface::Tab(id) = surface else { return };
    if !matches!(app.ui.panel.tabs.get(id), Some(Target::Device { .. })) { return; }
    let name = title(app, id);
    let mut edit = TextEdit::new(name.clone());
    edit.focused = true;
    edit.set_selection(0, name.len());
    app.ui.panel.tabs.rename = Some((id, edit));
}

pub(super) fn finish_rename(app: &mut App, save: bool) {
    let Some((id, edit)) = app.ui.panel.tabs.rename.take() else { return };
    if save && let Some((_, Target::Device { title, .. })) = app.ui.panel.tabs.entries.get_mut(id) {
        let name = edit.text.trim();
        *title = (!name.is_empty()).then(|| name.to_owned());
    }
}

pub(super) fn label(app: &App, t: &T, surface: Surface, title: String, fg: rune::Color) -> El {
    if let Surface::Tab(id) = surface
        && let Some((owner, edit)) = &app.ui.panel.tabs.rename
        && *owner == id
    {
        return text_input(edit, theme::TEXT_XS).w(96.).min_w(0.).single_line().label("Device tab name")
            .color(t.c.foreground).on_edit(|s: &mut App| &mut s.ui.panel.tabs.rename.as_mut().expect("visible rename").1)
            .on_submit(|s: &mut App, _: &mut Tx| finish_rename(s, true))
            .on_input_key(|s: &mut App, e: &rune::KeyEvent, _: &mut Tx| {
                if e.key != Key::Escape { return false; }
                finish_rename(s, false);
                true
            });
    }
    text(title).font(theme::TEXT_XS).color(fg).group_color(t.c.contrast_foreground).truncate().shrink()
}

pub(super) fn poll_rename(app: &mut App) {
    if app.ui.panel.tabs.rename.as_ref().is_some_and(|(_, edit)| !edit.focused) {
        finish_rename(app, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_round_trip_and_only_same_scope_reuses_a_tab() {
        let mut tabs = Tabs::default();
        for target in [
            Target::File("src/lib.rs".into()),
            Target::Attachment("attachment-1".into()),
            Target::Terminal("group-term-2".into()),
            Target::SideChat("child".into()),
            Target::PullRequest(json!({"number":7,"url":"https://github.com/owner/repo/pull/7","repository":"owner/repo"})),
            Target::Device { host: "host-a".into(), device: "id".into(), name: "Phone".into(), title: Some("My phone".into()) },
        ] {
            let restored = Target::decode(&target.encode()).unwrap();
            assert_eq!(restored, target);
            let first = tabs.intern("thread-a", target.clone());
            assert_eq!(tabs.intern("thread-a", restored), first);
            assert_ne!(tabs.intern("thread-b", target), first);
        }
        assert_ne!(tabs.intern("thread-a", Target::Device { host: "host-b".into(), device: "id".into(), name: "Phone".into(), title: None }),
                   tabs.intern("thread-a", Target::Device { host: "host-a".into(), device: "id".into(), name: "Phone".into(), title: None }));
        for (input, normalized) in [
            ("src/lib.rs/", "src/lib.rs"), ("src/", "src"), ("/", "/"),
            ("C://", "C:/"), ("./", "."), ("file with spaces.rs", "file with spaces.rs"),
        ] {
            assert_eq!(file_path(input), normalized);
        }
        for malformed in [
            json!({"kind":"file","path":""}),
            json!({"kind":"device","device":"phone"}),
            json!({"kind":"terminal"}),
            json!({"kind":"pull-request","entry":{"number":0}}),
        ] {
            assert!(Target::decode(&malformed).is_none());
        }
    }

    #[test]
    fn drag_order_and_close_pick_the_neighbor() {
        let mut tabs = vec![Surface::Files, Surface::Diff, Surface::Tab(0)];
        let spans = [(40., 80.), (100., 40.), (150., 60.)];
        assert_eq!(destination(&spans, 0, 105.), Some(2));
        assert!(reorder(&mut tabs, Surface::Files, 2));
        assert_eq!(tabs, [Surface::Diff, Surface::Tab(0), Surface::Files]);
        assert_eq!(remove(&mut tabs, Surface::Tab(0)), Some(Surface::Files));
        assert_eq!(remove(&mut tabs, Surface::Files), Some(Surface::Diff));
    }
}
