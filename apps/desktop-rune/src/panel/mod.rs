//! Workspace surfaces; wire payloads stay identical to the web client's RPCs.
pub mod browser;
mod diff;
mod files;
mod terminal;

use crate::{
    App, Route, data, icons, kit,
    theme::{self, AlphaMul, T},
};
use rune::{Cx, El, Key, KeyEvent, Phase, TextEdit, Tx, Waker, col, el, icon, row, text};
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Browser,
    Terminal,
    Files,
    Diff,
}
impl Surface {
    fn title(self) -> &'static str {
        match self {
            Self::Browser => "Browser",
            Self::Terminal => "Terminal",
            Self::Files => "Files",
            Self::Diff => "Diff",
        }
    }
    fn glyph(self) -> &'static rune::Icon {
        match self {
            Self::Browser => &icons::GLOBE,
            Self::Terminal => &icons::SQUARE_TERMINAL,
            Self::Files => &icons::FILES,
            Self::Diff => &icons::FILE_DIFF,
        }
    }
}
pub struct Ui {
    width: f32,
    surfaces: Vec<Surface>,
    active: Option<Surface>,
    pub diff: diff::Ui,
    files: files::Ui,
    terminal: terminal::Ui,
    browser: browser::Browser,
    url: TextEdit,
    browser_started: bool,
    thread: String,
    pub waker: Option<Waker>,
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            width: 540.,
            surfaces: vec![],
            active: None,
            diff: Default::default(),
            files: Default::default(),
            terminal: Default::default(),
            browser: Default::default(),
            url: TextEdit::new(""),
            browser_started: false,
            thread: String::new(),
            waker: None,
        }
    }
}
fn context(app: &App) -> Option<(String, String)> {
    let Route::Thread(id) = &app.route else {
        return None;
    };
    let thread = app.state().threads.iter().find(|v| v["id"] == *id)?;
    let project = app
        .state()
        .projects
        .iter()
        .find(|v| v["id"] == thread["projectId"])?;
    let cwd = thread["worktreePath"]
        .as_str()
        .filter(|v| !v.is_empty())
        .unwrap_or(data::s(project, "workspaceRoot"));
    Some((id.clone(), cwd.to_string()))
}
fn activate(app: &mut App, surface: Surface) {
    app.ui.shell.panel_open = true;
    app.ui.chat.topbar.terminal_open = surface == Surface::Terminal;
    if !app.ui.panel.surfaces.contains(&surface) {
        app.ui.panel.surfaces.push(surface);
    }
    app.ui.panel.active = Some(surface);
    match surface {
        Surface::Diff => diff::load(app),
        Surface::Files => files::load(app, "".into()),
        Surface::Terminal => terminal::attach(app),
        _ => {}
    }
}
fn close(app: &mut App, surface: Surface) {
    if surface == Surface::Terminal {
        terminal::close(app);
    }
    app.ui.panel.surfaces.retain(|v| *v != surface);
    if app.ui.panel.active == Some(surface) {
        app.ui.panel.active = app.ui.panel.surfaces.last().copied();
    }
    app.ui.chat.topbar.terminal_open = app.ui.panel.active == Some(Surface::Terminal);
}
pub fn open_diff(app: &mut App, checkpoint: Value) {
    app.ui.chat.timeline.diff_target = Some(checkpoint.clone());
    let checkpoint = if checkpoint["appRunOrdinal"].is_null() {
        if let Route::Thread(id) = &app.route {
            app.state()
                .thread_views
                .get(id)
                .and_then(|v| v.projection["checkpoints"].as_array())
                .and_then(|rows| rows.iter().find(|v| v["id"] == checkpoint["checkpointId"]))
                .cloned()
                .unwrap_or(checkpoint)
        } else {
            checkpoint
        }
    } else {
        checkpoint
    };
    app.ui.panel.diff.to = checkpoint["checkpointTurnCount"]
        .as_u64()
        .or(checkpoint["appRunOrdinal"].as_u64())
        .unwrap_or(1);
    app.ui.panel.diff.from = app.ui.panel.diff.to.saturating_sub(1);
    diff::select_checkpoint(app);
    activate(app, Surface::Diff);
}
pub fn poll(app: &mut App) -> bool {
    let mut context_changed = false;
    if matches!(app.src, crate::data::Source::Live(_)) {
        let thread = context(app).map(|(id, _)| id).unwrap_or_default();
        if app.ui.panel.thread != thread {
            app.ui.panel.thread = thread;
            app.ui.panel.diff = diff::Ui::default();
            app.ui.panel.files = files::Ui::default();
            if app.ui.shell.panel_open {
                match app.ui.panel.active {
                    Some(Surface::Diff) => diff::load(app),
                    Some(Surface::Files) => files::load(app, String::new()),
                    _ => {}
                }
            }
            context_changed = true;
        }
        if app.ui.panel.active == Some(Surface::Terminal) {
            terminal::attach(app);
        }
    }

    let mut changed = diff::poll(app) | files::poll(app) | terminal::poll(app);
    if app.ui.chat.topbar.terminal_open && app.ui.panel.active != Some(Surface::Terminal) {
        activate(app, Surface::Terminal);
        changed = true;
    }
    changed | context_changed
}
fn shortcut(app: &App, e: &KeyEvent, command: &str) -> bool {
    app.state()
        .config
        .as_ref()
        .and_then(|v| v["keybindings"].as_array())
        .is_some_and(|bindings| {
            bindings.iter().any(|v| {
                let k = &v["shortcut"];
                let yes = |name: &str| k[name].as_bool().unwrap_or(false);
                let key = match &e.key {
                    Key::Command(c) => c.to_string(),
                    Key::Text(v) => v.clone(),
                    _ => String::new(),
                };
                v["command"] == command
                    && k["key"]
                        .as_str()
                        .is_some_and(|v| v.eq_ignore_ascii_case(&key))
                    && e.mods.cmd
                        == (yes("modKey")
                            || yes("metaKey")
                            || (!cfg!(target_os = "macos") && yes("ctrlKey")))
                    && e.mods.ctrl
                        == (yes("ctrlKey") || (!cfg!(target_os = "macos") && yes("modKey")))
                    && e.mods.shift == yes("shiftKey")
                    && e.mods.alt == yes("altKey")
            })
        })
}
pub fn on_key(app: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if shortcut(app, e, "rightPanel.toggle") {
        app.ui.shell.panel_open = !app.ui.shell.panel_open;
        return true;
    }
    if app.ui.shell.panel_open && shortcut(app, e, "rightPanel.new") {
        app.ui.panel.active = None;
        app.ui.chat.topbar.terminal_open = false;
        return true;
    }
    if app.ui.shell.panel_open && shortcut(app, e, "rightPanel.close") {
        if let Some(surface) = app.ui.panel.active {
            close(app, surface);
        } else {
            app.ui.shell.panel_open = false;
        }
        return true;
    }
    if shortcut(app, e, "terminal.toggle") {
        toggle_terminal(app);
        return true;
    }
    if app.ui.shell.panel_open
        && app.ui.panel.active.is_none()
        && tx
            .focused()
            .is_none_or(|key| key.starts_with('#') || key.contains("panel-launcher"))
        && !e.mods.cmd
        && !e.mods.ctrl
        && !e.mods.alt
    {
        if let Key::Text(v) = &e.key {
            let surface = match v.to_lowercase().as_str() {
                "b" => Some(Surface::Browser),
                "t" => Some(Surface::Terminal),
                "f" => Some(Surface::Files),
                "d" => Some(Surface::Diff),
                _ => None,
            };
            if let Some(surface) = surface {
                activate(app, surface);
                return true;
            }
        }
    }
    false
}
pub fn toggle_terminal(app: &mut App) {
    if app.ui.shell.panel_open && app.ui.panel.active == Some(Surface::Terminal) {
        app.ui.shell.panel_open = false;
        app.ui.chat.topbar.terminal_open = false;
    } else {
        app.ui.chat.topbar.terminal_open = true;
        activate(app, Surface::Terminal);
    }
}
fn action(t: &T, label: &str, glyph: &'static rune::Icon) -> kit::Button {
    let _ = t;
    kit::Button::new(label)
        .icon(glyph)
        .variant("ghost-muted")
        .size("icon-xs")
}
fn picker(app: &App, t: &T) -> El {
    let mut list = col().w(320.).gap(2.).child(
        text("Open a surface")
            .font(theme::SM_MEDIUM)
            .align(rune::Align::Center)
            .color(t.c.contrast_foreground)
            .mb(10.),
    );
    for (surface, key) in [
        (Surface::Browser, "B"),
        (Surface::Terminal, "T"),
        (Surface::Files, "F"),
        (Surface::Diff, "D"),
    ] {
        let available = surface == Surface::Browser || context(app).is_some();
        list = list.child(
            row()
                .h(32.)
                .px(10.)
                .gap(10.)
                .center_y()
                .radius(8.)
                .hover(t.c.accent.alpha_mul(0.6))
                .focusable(true)
                .role("button")
                .label(surface.title())
                .disabled(!available)
                .child(
                    icon(surface.glyph(), 16.)
                        .size(16., 16.)
                        .color(t.c.contrast_foreground),
                )
                .child(
                    text(surface.title())
                        .font(theme::TEXT_SM)
                        .color(t.c.contrast_foreground)
                        .grow(1.),
                )
                .child(text(key).font(theme::MONO_XS).color(t.c.muted_foreground))
                .on_press(move |s: &mut App| activate(s, surface)),
        );
    }
    for (title, glyph, key) in [
        ("Pull request", &icons::GIT_PULL_REQUEST_ARROW, "P"),
        ("Linked pull requests", &icons::LINK, "L"),
        ("Device", &icons::SMARTPHONE, "M"),
    ] {
        list = list.child(
            row()
                .h(32.)
                .px(10.)
                .gap(10.)
                .center_y()
                .opacity(0.5)
                .role("button")
                .label(title)
                .disabled(true)
                .child(
                    icon(glyph, 16.)
                        .size(16., 16.)
                        .color(t.c.contrast_foreground),
                )
                .child(
                    text(title)
                        .font(theme::TEXT_SM)
                        .color(t.c.contrast_foreground)
                        .grow(1.),
                )
                .child(text(key).font(theme::MONO_XS).color(t.c.muted_foreground)),
        );
    }
    col()
        .key("panel-launcher")
        .focus_group()
        .grow(1.)
        .min_h(0.)
        .center()
        .px(24.)
        .pb(52.)
        .child(list)
}
pub fn view(app: &App, cx: &mut Cx<App>, t: &T) -> El {
    let width = app
        .ui
        .panel
        .width
        .min(cx.layout_with("shell", |r| r[2] * 0.7).unwrap_or(1000.));
    let mut tabs = row().h(51.).gap(2.).px(8.).center_y().role("tablist");
    for &surface in &app.ui.panel.surfaces {
        tabs = tabs.child(
            row()
                .h(32.)
                .px(8.)
                .gap(6.)
                .radius(6.)
                .fill(if app.ui.panel.active == Some(surface) {
                    t.c.accent.alpha_mul(0.4)
                } else {
                    rune::Color::TRANSPARENT
                })
                .child(
                    row()
                        .gap(6.)
                        .center_y()
                        .role("tab")
                        .label(surface.title())
                        .focusable(true)
                        .on_press(move |s: &mut App| {
                            s.ui.panel.active = Some(surface);
                            s.ui.chat.topbar.terminal_open = surface == Surface::Terminal;
                        })
                        .child(
                            icon(surface.glyph(), 16.)
                                .size(14., 14.)
                                .color(t.c.muted_foreground),
                        )
                        .child(
                            text(surface.title())
                                .font(theme::TEXT_XS)
                                .color(t.c.contrast_foreground),
                        ),
                )
                .child(
                    action(t, "Close surface", &icons::X)
                        .on_press(move |s: &mut App| close(s, surface))
                        .build(t),
                ),
        );
    }
    tabs = tabs
        .child(
            action(t, "Add panel surface", &icons::PLUS)
                .on_press(|s: &mut App| s.ui.panel.active = None)
                .build(t),
        )
        .child(el().grow(1.))
        .child(
            action(t, "Close right panel", &icons::PANEL_RIGHT)
                .on_press(|s: &mut App| {
                    s.ui.shell.panel_open = false;
                    s.ui.chat.topbar.terminal_open = false;
                })
                .build(t),
        );
    let content = match app.ui.panel.active {
        None => picker(app, t),
        Some(Surface::Diff) => diff::view(app, t),
        Some(Surface::Files) => files::view(app, t),
        Some(Surface::Terminal) => terminal::view(app, t),
        Some(Surface::Browser) => browser_view(app, t),
    };
    row()
        .key("right-panel")
        .w(width)
        .h_full()
        .fill(t.c.background)
        .child(
            el().w(1.)
                .h_full()
                .fill(t.c.border)
                .cursor(rune::Cursor::ResizeColumn)
                .on_drag(|s: &mut App, d, _| {
                    if d.phase == Phase::Down {
                        d.data = s.ui.panel.width;
                    }
                    if !d.cancelled() {
                        s.ui.panel.width = (d.data - d.dx).clamp(320., 1100.);
                    }
                }),
        )
        .child(
            col()
                .grow(1.)
                .min_w(0.)
                .h_full()
                .child(tabs)
                .child(el().h(1.).w_full().fill(t.c.border))
                .child(content),
        )
}
fn navigate(app: &mut App) {
    let input = app.ui.panel.url.text.trim().to_string();
    if input.is_empty() {
        return;
    }
    let url = if input.contains("://") {
        input
    } else {
        format!("http://{input}")
    };
    if !app.ui.panel.browser_started {
        let Some(waker) = app.ui.panel.waker.clone() else {
            return;
        };
        app.ui.panel.browser.start(waker);
        app.ui.panel.browser_started = true;
    }
    app.ui.panel.browser.send(browser::Cmd::Navigate(url));
    app.ui.panel.browser.send(browser::Cmd::Resize {
        w: (app.ui.panel.width - 1.) as u32,
        h: 800,
        scale: 1.,
    });
}
fn browser_view(app: &App, t: &T) -> El {
    let status = app.ui.panel.browser.status.lock().unwrap().clone();
    let chrome = row()
        .h(28.)
        .px(8.)
        .gap(2.)
        .center_y()
        .child(
            action(t, "Back", &icons::ARROW_LEFT)
                .disabled(!status.can_back)
                .on_press(|s: &mut App| s.ui.panel.browser.send(browser::Cmd::Back))
                .build(t),
        )
        .child(
            action(t, "Forward", &icons::ARROW_RIGHT)
                .disabled(!status.can_forward)
                .on_press(|s: &mut App| s.ui.panel.browser.send(browser::Cmd::Forward))
                .build(t),
        )
        .child(
            action(t, "Refresh", &icons::REFRESH_CW)
                .disabled(!app.ui.panel.browser_started)
                .on_press(|s: &mut App| s.ui.panel.browser.send(browser::Cmd::Reload))
                .build(t),
        )
        .child(
            el().grow(1.).min_w(0.).child(
                kit::Input::new(&app.ui.panel.url)
                    .placeholder("Search or enter URL")
                    .size("compact")
                    .on_edit(|s: &mut App| &mut s.ui.panel.url)
                    .on_submit(|s: &mut App, _| navigate(s))
                    .build(t),
            ),
        );
    let chrome = chrome
        .child(
            action(t, "Annotate preview", &icons::MOUSE_POINTER_CLICK)
                .disabled(true)
                .build(t),
        )
        .child(
            action(t, "Capture screenshot", &icons::CAMERA)
                .disabled(true)
                .build(t),
        )
        .child(
            action(t, "Float preview over chat", &icons::PICTURE_IN_PICTURE_2)
                .disabled(true)
                .build(t),
        )
        .child(
            action(t, "Preview menu", &icons::ELLIPSIS_VERTICAL)
                .disabled(true)
                .build(t),
        );
    let content = if status.frames == 0 {
        col().w_full().px(48.).pt(60.).gap(0.)
            .child(el().size(36.,36.).radius(6.).fill(t.c.card).border(1.,t.c.border).center().child(icon(&icons::GLOBE,18.).size(18.,18.).color(t.c.muted_foreground)))
            .child(text("No preview yet").font(theme::weight(theme::TEXT_XL,600.)).color(t.c.contrast_foreground).mt(42.))
            .child(text(status.error.as_deref().unwrap_or("Type a URL above, or run a dev script. Local servers this thread starts will show up here automatically.")).font(theme::TEXT_SM).color(t.c.muted_foreground).w_full().wrap().align(rune::Align::Center).mt(28.)).center_y()
    } else {
        el().grow(1.)
            .min_h(0.)
            .w_full()
            .pixels(&app.ui.panel.browser.pixels)
            .focusable(true)
            .key("browser-page")
            .on_layout(|s: &mut App, r| {
                s.ui.panel.browser.send(browser::Cmd::Resize {
                    w: r[2].max(1.) as u32,
                    h: r[3].max(1.) as u32,
                    scale: 1.,
                })
            })
            .on_drag(|s: &mut App, d, _| {
                s.ui.panel.browser.send(browser::Cmd::Mouse {
                    kind: match d.phase {
                        Phase::Down => "mousePressed",
                        Phase::Up => "mouseReleased",
                        _ => "mouseMoved",
                    },
                    x: d.x,
                    y: d.y,
                    clicks: d.clicks,
                })
            })
            .on_pointer_move_local(|s: &mut App, p, _| {
                s.ui.panel.browser.send(browser::Cmd::Mouse {
                    kind: "mouseMoved",
                    x: p.x,
                    y: p.y,
                    clicks: 0,
                })
            })
            .on_wheel(|s: &mut App, w, _| {
                s.ui.panel.browser.send(browser::Cmd::Wheel {
                    x: w.x,
                    y: w.y,
                    dx: w.dx,
                    dy: w.dy,
                });
                true
            })
            .on_key(|s: &mut App, e, _| {
                let cmd = match &e.key {
                    Key::Text(v) => browser::Cmd::Text(v.clone()),
                    Key::Enter => browser::Cmd::Key("Enter"),
                    Key::Backspace => browser::Cmd::Key("Backspace"),
                    Key::Tab => browser::Cmd::Key("Tab"),
                    Key::Left => browser::Cmd::Key("ArrowLeft"),
                    Key::Right => browser::Cmd::Key("ArrowRight"),
                    Key::Up => browser::Cmd::Key("ArrowUp"),
                    Key::Down => browser::Cmd::Key("ArrowDown"),
                    Key::Escape => browser::Cmd::Key("Escape"),
                    _ => return false,
                };
                s.ui.panel.browser.send(cmd);
                true
            })
    };
    col().grow(1.).min_h(0.).child(chrome).child(content)
}
fn scene(app: &mut App, surface: Option<Surface>) {
    (crate::shell::SCENES[1].1)(app);
    app.ui.shell.panel_open = true;
    if let Some(surface) = surface {
        activate(app, surface);
    }
}
pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("panel-picker", |s| scene(s, None)),
    ("panel-browser", |s| scene(s, Some(Surface::Browser))),
    ("panel-diff-empty", |s| scene(s, Some(Surface::Diff))),
    ("panel-diff", |s| {
        scene(s, Some(Surface::Diff));
        diff::fixture(s);
    }),
    ("panel-diff-split", |s| {
        scene(s, Some(Surface::Diff));
        diff::fixture(s);
        s.ui.panel
            .diff
            .state
            .set_mode(rune_kit::diff::DiffMode::Split);
    }),
    ("panel-files", |s| {
        scene(s, Some(Surface::Files));
        files::fixture(s);
    }),
    ("panel-terminal", |s| {
        scene(s, Some(Surface::Terminal));
        terminal::fixture(s);
    }),
];

pub(super) fn notice(t: &T, message: &str) -> El {
    col().grow(1.).min_h(0.).center().px(20.).child(
        text(message)
            .font(theme::TEXT_XS)
            .color(t.c.muted_foreground.alpha_mul(0.7)),
    )
}

pub fn overlays(app: &App, t: &T) -> Vec<El> {
    if app.ui.shell.panel_open && app.ui.panel.active == Some(Surface::Diff) {
        diff::overlays(app, t)
    } else {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configured_mod_key_uses_the_platform_primary_modifier() {
        let mut app = App::new();
        let mut state = data::empty_state();
        state.config = Some(
            serde_json::json!({"keybindings":[{"command":"terminal.toggle","shortcut":{"key":"j","modKey":true}}]}),
        );
        app.src = data::Source::Fixture(state);
        let mut event = KeyEvent::from(Key::Command('j'));
        event.mods.cmd = true;
        event.mods.ctrl = !cfg!(target_os = "macos");
        assert!(shortcut(&app, &event, "terminal.toggle"));
        event.mods.shift = true;
        assert!(!shortcut(&app, &event, "terminal.toggle"));
    }
}
