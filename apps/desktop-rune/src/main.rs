//! S5 Code's desktop client, native on Rune. It speaks to the same server the Electron app does
//! (`s5_net`) and reproduces the web client's UI (apps/web) surface by surface.
//!
//! Each surface module owns its slice of UI state (`App::ui.*`) and its headless scenes, which
//! render recorded server state from `fixtures/` for pixel comparison against the web app.

mod chat;
mod data;
mod host;
mod icons;
mod kit;
mod panel;
mod settings;
mod shell;
mod theme;
mod tokens;

use rune::{App as RuneApp, AppSpec, Cx, El, KeyEvent, Tx, Waker};

use crate::data::Source;
use crate::theme::T;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Appearance {
    System,
    Light,
    Dark,
}

/// The web client's routes: `/` (new thread draft), `/$environmentId/$threadId`, `/settings/*`.
#[derive(Clone, PartialEq, Debug)]
pub enum Route {
    Home { project_id: Option<String> },
    Thread(String),
    Settings(String),
}

/// Per-surface UI state; each module owns its struct.
#[derive(Default)]
pub struct Ui {
    pub shell: shell::Ui,
    pub chat: chat::Ui,
    pub panel: panel::Ui,
    pub settings: settings::Ui,
}

pub struct App {
    pub src: Source,
    pub appearance: Appearance,
    pub client_settings: settings::client::ClientSettings,
    pub route: Route,
    /// Wall clock for relative times; scenes pin it so snapshots are stable.
    pub now_ms: i64,
    pub ui: Ui,
}

impl App {
    fn new() -> App {
        let src = Source::Fixture(if host::server().is_some() { data::empty_state() } else { data::fixture_state() });
        let client_settings = settings::client::ClientSettings::load();
        let appearance = settings::appearance(&client_settings);
        App { src, appearance, client_settings, route: Route::Home { project_id: None }, now_ms: data::now_ms(), ui: Ui::default() }
    }

    pub fn state(&self) -> &s5_net::State {
        self.src.state()
    }

    pub fn navigate(&mut self, route: Route) {
        if matches!(route, Route::Settings(_)) && !matches!(self.route, Route::Settings(_)) {
            self.ui.settings.return_route = Some(self.route.clone());
        }
        if let Route::Thread(id) = &route
            && let Some(net) = self.src.net()
        {
            net.watch_thread(id);
        }
        self.route = route;
    }
}

fn setup(s: &mut App, waker: Waker) {
    if let Some((url, token)) = host::server() {
        s.src = Source::Live(s5_net::Net::connect(&url, &token, move || waker()));
    }
}

fn poll(s: &mut App, _: f64, tx: &mut Tx) -> bool {
    let mut changed = s.src.net().is_some_and(|net| net.drain());
    changed |= settings::poll(s);
    changed |= chat::timeline::poll(s);
    changed |= shell::palette::poll(s);
    let now = data::now_ms();
    if matches!(s.src, Source::Live(_)) && now / 1000 != s.now_ms / 1000 {
        s.now_ms = now;
        changed = true;
    }
    tx.wake_after(std::time::Duration::from_millis(1000));
    changed
}

fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    settings::on_key(s, e, tx) || shell::on_key(s, e, tx)
}

pub fn is_dark(s: &App, cx: &Cx<App>) -> bool {
    match s.appearance {
        Appearance::System => cx.system_dark(),
        Appearance::Light => false,
        Appearance::Dark => true,
    }
}

fn view(s: &App, cx: &mut Cx<App>) -> El {
    let t: T = settings::apply_theme(s, theme::theme(is_dark(s, cx)));
    shell::view(s, cx, &t)
}

/// Every module's scenes, gathered once.
fn scenes() -> &'static [(&'static str, fn(&mut App))] {
    let all: Vec<(&'static str, fn(&mut App))> =
        [shell::SCENES, shell::sidebar::SCENES, chat::timeline::SCENES, chat::composer::SCENES, panel::SCENES, settings::SCENES, kit::SCENES].concat();
    Box::leak(all.into_boxed_slice())
}

/// Pins the clock for scenes: a minute after the newest thread update in the fixtures.
pub fn scene_clock(s: &mut App) {
    let newest = s.state().threads.iter().filter_map(|t| data::ts(t, "updatedAt")).max().unwrap_or(0);
    s.now_ms = newest + 60_000;
}

fn main() {
    let spec = AppSpec {
        min_size: Some((640.0, 480.0)),
        families: (theme::SANS, "ui-monospace"),
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        scenes: scenes(),
        ..AppSpec::new("S5 Code", (1440.0, 900.0), App::new, view)
    };
    RuneApp::new(spec).run();
    panel::browser::shutdown();
    host::shutdown();
}
