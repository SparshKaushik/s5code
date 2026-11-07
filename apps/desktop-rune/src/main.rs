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
    /// Server lifecycle from `host`: the bundled server becoming ready, or failing for good.
    host: Option<std::sync::mpsc::Receiver<host::Event>>,
}

impl App {
    fn new() -> App {
        let src = Source::Fixture(if host::live() { data::empty_state() } else { data::fixture_state() });
        let client_settings = settings::client::ClientSettings::load();
        let appearance = settings::appearance(&client_settings);
        App { src, appearance, client_settings, route: Route::Home { project_id: None }, now_ms: data::now_ms(), ui: Ui::default(), host: None }
    }

    pub fn state(&self) -> &s5_net::State {
        self.src.state()
    }

    /// The route the window frame shows: under the settings modal, the one settings opened from.
    pub fn frame_route(&self) -> &Route {
        match (&self.route, &self.ui.settings.return_route) {
            (Route::Settings(_), Some(back)) => back,
            (route, _) => route,
        }
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
    s.ui.panel.waker = Some(waker.clone());
    s.host = host::start(move || waker());
}

fn poll_host(s: &mut App) -> bool {
    let Some(event) = s.host.as_ref().and_then(|rx| rx.try_recv().ok()) else { return false };
    match event {
        host::Event::Ready { url, token } => {
            let waker = s.ui.panel.waker.clone().expect("setup stores the waker");
            s.src = Source::Live(s5_net::Net::connect(&url, &token, move || waker()));
            s.navigate(s.route.clone());
        }
        host::Event::Failed(reason) => {
            eprintln!("s5code-rune: {reason}");
            if let Source::Fixture(state) = &mut s.src {
                state.conn = s5_net::Conn::Closed(reason);
            }
        }
    }
    true
}

fn poll(s: &mut App, _: f64, tx: &mut Tx) -> bool {
    let mut changed = poll_host(s);
    changed |= s.src.net().is_some_and(|net| net.drain());
    changed |= panel::poll(s);
    changed |= settings::poll(s);
    changed |= chat::timeline::poll(s);
    changed |= chat::composer::poll(s);
    changed |= shell::palette::poll(s);
    changed |= shell::sidebar::poll(s);
    chat::details::watch(s);
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

/// Pins the clock for scenes where the references were captured: 28 minutes after the newest
/// thread update in the fixtures ("Replay showcase" reads 29m).
pub fn scene_clock(s: &mut App) {
    let newest = s.state().threads.iter().filter_map(|t| data::ts(t, "updatedAt")).max().unwrap_or(0);
    s.now_ms = newest + 28 * 60_000;
}

fn main() {
    if std::env::args().any(|a| a == "--check-server") {
        std::process::exit(host::check());
    }
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
