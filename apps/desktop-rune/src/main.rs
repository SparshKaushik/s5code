//! S5 Code's desktop client, native on Rune. It speaks to the same server the Electron app does
//! (`s5_net`) and reproduces the web client's UI (apps/web) surface by surface.
//!
//! Each surface module owns its slice of UI state (`App::ui.*`) and its headless scenes, which
//! render recorded server state from `fixtures/` for pixel comparison against the web app.

mod chat;
mod data;
mod desktop;
mod env;
mod host;
mod icons;
mod kit;
mod links;
mod notify;
mod onboarding;
mod pages;
mod panel;
mod search;
mod settings;
mod shell;
mod theme;
mod tokens;
mod wizard;

/// The entry the desktop shell hands `t3code://new?…` prompt links to: `prompt_link::open(app, url)`.
pub(crate) use links as prompt_link;

/// The Linux launcher entry (`<APP_ID>.desktop`) the unread badge counts on.
pub use desktop::APP_ID;

use rune::{App as RuneApp, AppSpec, Cx, El, KeyEvent, Tx, Waker};

use crate::data::Source;
use crate::theme::T;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Appearance {
    System,
    Light,
    Dark,
}

/// The web client's routes: `/` (new thread draft), `/$environmentId/$threadId`, `/settings/*`,
/// `/pull-requests` and `/usage`.
#[derive(Clone, PartialEq, Debug)]
pub enum Route {
    Home { project_id: Option<String> },
    Thread(String),
    Settings(String),
    PullRequests,
    Usage,
}

impl Route {
    /// Settings and usage are route modals: the frame keeps the route they opened over.
    fn is_modal(&self) -> bool {
        matches!(self, Route::Settings(_) | Route::Usage)
    }
}

/// Per-surface UI state; each module owns its struct.
#[derive(Default)]
pub struct Ui {
    pub shell: shell::Ui,
    pub chat: chat::Ui,
    pub panel: panel::Ui,
    pub settings: settings::Ui,
    pub pages: pages::Ui,
    pub notify: notify::Ui,
    pub search: search::Ui,
    pub onboarding: onboarding::Ui,
    pub desktop: desktop::Ui,
    pub env: env::Ui,
    pub links: links::Ui,
    pub wizard: wizard::Ui,
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

    /// The route the window frame shows: under a route modal, the one it opened from.
    pub fn frame_route(&self) -> &Route {
        match (&self.route, &self.ui.settings.return_route) {
            (Route::Settings(_) | Route::Usage, Some(back)) => back,
            (route, _) => route,
        }
    }

    pub fn navigate(&mut self, route: Route) {
        if route.is_modal() && !self.route.is_modal() {
            self.ui.settings.return_route = Some(self.route.clone());
        }
        if matches!(route, Route::Home { .. } | Route::Thread(_)) {
            self.ui.pages.main_route = Some(route.clone());
        }
        env::follow(self, &route);
        if let Route::Thread(id) = &route
            && let Some(net) = self.src.net()
        {
            net.watch_thread(id);
        }
        if matches!(route, Route::Home { .. } | Route::Thread(_)) && route != self.route {
            chat::composer::focus(self, &route);
        }
        self.route = route;
    }
}

fn setup(s: &mut App, waker: Waker) {
    s.ui.panel.waker = Some(waker.clone());
    env::start(s);
    prompt_link::start(s);
    s.host = host::start(move || waker());
}

fn poll_host(s: &mut App) -> bool {
    let Some(event) = s.host.as_ref().and_then(|rx| rx.try_recv().ok()) else { return false };
    match event {
        host::Event::Ready { url, token } => {
            let waker = s.ui.panel.waker.clone().expect("setup stores the waker");
            s.ui.desktop.server = Some((url.clone(), token.clone()));
            s.ui.desktop.startup_failure = None;
            s.ui.desktop.local_off = false;
            *env::local_mut(s) = Source::Live(s5_net::Net::connect(&url, &token, move || waker()));
            if s.ui.env.active == env::LOCAL {
                s.navigate(s.route.clone());
            }
        }
        // Electron keeps the first failure's details while later attempts retry.
        host::Event::Failing(failure) => {
            s.ui.desktop.startup_failure.get_or_insert(failure);
        }
        // Web's `readPrimaryEnvironmentTarget` is null: an empty, settled local environment, so
        // remote environments still merge in and the window never waits for a server.
        host::Event::Off => {
            s.ui.desktop.server = None;
            s.ui.desktop.startup_failure = None;
            s.ui.desktop.local_off = true;
            let mut state = data::empty_state();
            state.conn = s5_net::Conn::Closed(host::LOCAL_OFF.into());
            state.shell_ready = true;
            *env::local_mut(s) = Source::Fixture(state);
        }
        host::Event::Failed(reason) => {
            eprintln!("s5code-rune: {reason}");
            if let Source::Fixture(state) = env::local_mut(s) {
                state.conn = s5_net::Conn::Closed(reason);
            }
        }
    }
    true
}

fn poll(s: &mut App, _: f64, tx: &mut Tx) -> bool {
    let mut changed = poll_host(s);
    changed |= env::drain(s);
    changed |= panel::poll(s, tx);
    changed |= settings::poll(s);
    settings::fonts::follow(s, tx);
    changed |= chat::timeline::poll(s);
    changed |= chat::composer::poll(s);
    changed |= chat::remember_route(s);
    changed |= shell::palette::poll(s);
    changed |= shell::sidebar::poll(s);
    changed |= pages::poll(s);
    changed |= shell::quit::poll(s);
    changed |= chat::details::poll(s);
    changed |= notify::poll(s, tx);
    changed |= onboarding::poll(s, tx);
    changed |= env::poll(s);
    changed |= links::poll(s);
    changed |= wizard::poll(s);
    changed |= search::poll(s);
    changed |= desktop::poll(s, tx);
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
    // Lone modifiers only drive the pull request rows' Shift actions; the other handlers predate
    // them (the sidebar would hand mod+z back to text fields on the Cmd press alone).
    if matches!(e.key, rune::Key::Modifier { .. }) {
        return pages::on_modifier(s, e);
    }
    panel::preview::note_focus(s, tx);
    shell::quit::on_key(s, e, tx)
        || search::on_key(s, e, tx)
        || settings::on_key(s, e, tx)
        || pages::on_key(s, e, tx)
        || shell::on_key(s, e, tx)
        || chat::on_key(s, e, tx)
        || desktop::on_key(s, e, tx)
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
    let app = shell::view(s, cx, &t);
    desktop::frame(s, cx, app)
}

/// Every module's scenes, gathered once.
fn scenes() -> &'static [(&'static str, fn(&mut App))] {
    let all: Vec<(&'static str, fn(&mut App))> =
        [shell::SCENES, shell::palette::SCENES, shell::sidebar::SCENES, chat::timeline::SCENES, chat::composer::SCENES, panel::SCENES, settings::SCENES, pages::SCENES, kit::SCENES, env::SCENES].concat();
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
    // A second launch focuses the running window and exits.
    let Some(instance) = desktop::single::claim() else { return };
    let (fonts, families) = settings::fonts::launch();
    let spec = AppSpec {
        min_size: Some(desktop::MIN_SIZE),
        fonts,
        families,
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        scenes: scenes(),
        ..AppSpec::new("S5 Code", desktop::initial_size(), App::new, view)
    };
    let mut app = desktop::configure(RuneApp::new(spec).modifier_keys()).on_open_url(links::on_open_url);
    panel::popout::register(&mut app);
    notify::set_window(app.handle());
    instance.serve(app.handle());
    #[cfg(unix)]
    desktop::activation::serve(app.handle());
    #[cfg(unix)]
    desktop::signals::install(app.handle());
    app.run();
    panel::browser::shutdown();
    host::shutdown();
    env::ssh::shutdown();
    drop(instance);
}
