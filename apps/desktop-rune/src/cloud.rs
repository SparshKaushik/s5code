//! T3 Connect as Electron's desktop app offers it (web `T3ConnectSidebarSignIn`,
//! `CloudEnvironmentConnectRows`, onboarding `ConnectAccountOption`): sign in through the system
//! browser, list the computers linked to the account, and add them as environments reached
//! through the relay. `s5_net::cloud` speaks the protocol; this module keeps the session and the
//! tokens each relay socket mints from.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, LazyLock, Mutex};

use rune::{El, col, icon, row, text};
use s5_net::cloud::Failure;
use s5_net::cloud::clerk::{self, Callback, Clerk, Identity};
use s5_net::cloud::dpop::Key;
use s5_net::cloud::relay::{self, Linked, Relay, Token};
use serde_json::{Value, json};

use crate::theme::{self, T};
use crate::{App, icons, kit};

/// The public cloud config `loadRepoEnv` reads; the process environment overrides it.
const PUBLIC_CONFIG: &str = include_str!("../../../.env.example");

fn public(name: &str) -> Option<String> {
    let set = std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    set.or_else(|| PUBLIC_CONFIG.lines().find_map(|line| line.trim().strip_prefix(name)?.strip_prefix('=').map(|v| v.trim().to_owned()))).filter(|v| !v.is_empty())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub fapi: String,
    pub template: String,
    pub relay: String,
}

/// `hasCloudPublicConfig`: a build without it offers no T3 Connect.
pub fn config() -> Option<Config> {
    Some(Config {
        fapi: clerk::frontend_api(&public("T3CODE_CLERK_PUBLISHABLE_KEY")?)?,
        template: public("T3CODE_CLERK_JWT_TEMPLATE")?,
        relay: relay::relay_origin(&public("T3CODE_RELAY_URL")?)?,
    })
}

/// A browser sign-in in flight: the attempt its deep link names, the Clerk sign-in once started,
/// and the nonce once the browser has come back.
#[derive(Clone, Debug, PartialEq)]
pub struct SignIn {
    pub attempt: String,
    pub sign_in: Option<String>,
    pub nonce: Option<String>,
    pub finishing: bool,
}

#[derive(Default)]
pub struct Ui {
    pub identity: Option<Identity>,
    /// The social providers the instance offers, once loaded.
    pub providers: Vec<(String, String)>,
    pub signing_in: Option<SignIn>,
    /// The account's computers: `None` until listed.
    pub environments: Option<Result<Vec<Linked>, String>>,
    pub loading: bool,
    pub error: Option<String>,
    /// The setup wizard's T3 Connect card, folded away.
    pub wizard_folded: bool,
    events: Option<(Sender<Event>, Receiver<Event>)>,
}

pub enum Event {
    Restored(Result<Option<Identity>, Failure>),
    Providers(Vec<(String, String)>),
    Started { attempt: String, sign_in: String },
    Failed { attempt: String, message: String },
    SignedIn { attempt: String, result: Result<Identity, String> },
    Environments(Result<Vec<Linked>, Failure>),
    SignedOut,
}

/// What background requests and relay sockets share: the device's Clerk client, the session,
/// the DPoP key, and the tokens minted from them.
#[derive(Default)]
struct Shared {
    client_jwt: Option<String>,
    identity: Option<Identity>,
    key: Option<Arc<Key>>,
    relay: Option<Token>,
    environments: HashMap<String, (Linked, Token)>,
}

static SHARED: LazyLock<Mutex<Shared>> = LazyLock::new(Mutex::default);

fn shared() -> std::sync::MutexGuard<'static, Shared> {
    SHARED.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Tests and scenes never touch the person's account or the network.
fn live() -> bool {
    !cfg!(test) && !rune::is_headless()
}

fn path() -> std::path::PathBuf {
    crate::settings::client::sibling_path("t3-connect.json")
}

/// The Clerk client token and the DPoP key are credentials: the file is the user's alone.
fn save(state: &Shared) {
    use base64::Engine;
    if !live() {
        return;
    }
    let mut file = json!({"version": 1});
    if let Some(key) = &state.key {
        file["dpopKey"] = json!(base64::engine::general_purpose::STANDARD.encode(key.pkcs8()));
    }
    if let Some(jwt) = &state.client_jwt {
        file["clientJwt"] = json!(jwt);
    }
    if let Some(i) = &state.identity {
        file["identity"] = json!({"session": i.session, "user": i.user, "email": i.email, "name": i.name});
    }
    let path = path();
    let write = || -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temporary = path.with_extension("json.tmp");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        std::io::Write::write_all(&mut options.open(&temporary)?, file.to_string().as_bytes())?;
        std::fs::rename(&temporary, &path)
    };
    if let Err(e) = write() {
        eprintln!("s5code-rune: could not save the T3 Connect session: {e}");
    }
}

fn identity_of(value: &Value) -> Option<Identity> {
    let field = |k: &str| value[k].as_str().unwrap_or_default().to_owned();
    Some(Identity { session: value["session"].as_str().filter(|s| !s.is_empty())?.to_owned(), user: field("user"), email: field("email"), name: field("name") })
}

/// Loads the saved session and checks it with Clerk in the background.
pub fn start(s: &mut App) {
    if !live() {
        return;
    }
    let Some(config) = config() else { return };
    let file: Value = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let identity = identity_of(&file["identity"]);
    {
        use base64::Engine;
        let mut state = shared();
        state.client_jwt = file["clientJwt"].as_str().map(str::to_owned);
        state.identity = identity.clone();
        state.key = file["dpopKey"].as_str().and_then(|k| base64::engine::general_purpose::STANDARD.decode(k).ok()).and_then(|k| Key::from_pkcs8(&k).ok()).map(Arc::new);
    }
    s.ui.cloud.identity = identity.clone();
    let check = config.clone();
    spawn(s, move || {
        let mut clerk = client(&check);
        let providers = clerk.environment().map(|e| clerk::providers(&e)).unwrap_or_default();
        keep(&clerk);
        Event::Providers(providers)
    });
    if identity.is_some() {
        spawn(s, move || {
            let mut clerk = client(&config);
            let restored = clerk.client().map(|c| clerk::identity(&c));
            keep(&clerk);
            Event::Restored(restored)
        });
    }
}

fn spawn(s: &mut App, job: impl FnOnce() -> Event + Send + 'static) {
    if !live() {
        return;
    }
    let tx = s.ui.cloud.events.get_or_insert_with(channel).0.clone();
    let waker = s.ui.panel.waker.clone();
    std::thread::spawn(move || {
        let _ = tx.send(job());
        if let Some(w) = waker {
            w();
        }
    });
}

fn client(config: &Config) -> Clerk {
    Clerk { fapi: config.fapi.clone(), client_jwt: shared().client_jwt.clone() }
}

/// Keeps the client token Clerk rotated in its last reply.
fn keep(clerk: &Clerk) {
    let mut state = shared();
    if clerk.client_jwt.is_some() && state.client_jwt != clerk.client_jwt {
        state.client_jwt = clerk.client_jwt.clone();
        save(&state);
    }
}

fn key() -> Result<Arc<Key>, Failure> {
    let mut state = shared();
    if let Some(key) = &state.key {
        return Ok(key.clone());
    }
    let key = Arc::new(Key::generate().map_err(Failure::new)?);
    state.key = Some(key.clone());
    save(&state);
    Ok(key)
}

fn signed_out() -> Failure {
    Failure { message: "Sign in to T3 Connect to reach this environment.".into(), trace: None, status: Some(401) }
}

/// A fresh session JWT for the relay's audience.
fn session_token(config: &Config) -> Result<String, Failure> {
    let session = shared().identity.as_ref().map(|i| i.session.clone());
    let session = session.ok_or_else(signed_out)?;
    let mut clerk = client(config);
    let token = clerk.session_token(&session, &config.template);
    keep(&clerk);
    token
}

/// The signed-in account's session JWT, for the relay's client endpoints (linking this machine).
/// Blocks on Clerk: call it off the UI thread.
pub fn account_token() -> Result<String, Failure> {
    let config = config().ok_or_else(|| Failure::new("This build has no T3 Connect configuration."))?;
    session_token(&config)
}

/// The relay token, exchanged again when it lapses or `stale` says the relay refused it.
fn relay_token(config: &Config, key: &Key, stale: bool) -> Result<Token, Failure> {
    let cached = shared().relay.clone().filter(|t| !stale && t.fresh(relay::now()));
    if let Some(token) = cached {
        return Ok(token);
    }
    let token = Relay { origin: config.relay.clone(), key }.exchange(&session_token(config)?, relay::CONNECT_SCOPE)?;
    shared().relay = Some(token.clone());
    Ok(token)
}

/// `authorizeDpop` for a socket: a cached environment token mints a ticket; otherwise the
/// relay connects the environment again. Runs on the socket's own thread before each attempt.
pub fn mint(environment: &str) -> Result<String, Failure> {
    let config = config().ok_or_else(|| Failure::new("This build has no T3 Connect configuration."))?;
    let cached = shared().environments.get(environment).cloned().filter(|(_, token)| token.fresh(relay::now()));
    if let Some((linked, token)) = cached {
        match relay::socket(&*key()?, &linked, &token) {
            Err(refused) if refused.denied() => {
                shared().environments.remove(environment);
            }
            other => return other,
        }
    }
    if shared().identity.is_none() {
        return Err(signed_out());
    }
    let key = key()?;
    let relay = Relay { origin: config.relay.clone(), key: &key };
    let bootstrap = match relay.connect(&relay_token(&config, &key, false)?, environment) {
        Err(refused) if refused.denied() => relay.connect(&relay_token(&config, &key, true)?, environment)?,
        other => other?,
    };
    let token = relay::environment_token(&key, &bootstrap)?;
    shared().environments.insert(environment.to_owned(), (bootstrap.environment.clone(), token.clone()));
    relay::socket(&key, &bootstrap.environment, &token)
}

/// The socket auth of an environment saved as reached through T3 Connect.
pub fn socket_auth(environment: &str) -> s5_net::Auth {
    let environment = environment.to_owned();
    s5_net::Auth::Socket(Arc::new(move || mint(&environment)))
}

/// The providers to offer: the instance's, else GitHub and Google until they load.
pub fn providers(s: &App) -> Vec<(String, String)> {
    if s.ui.cloud.providers.is_empty() {
        return vec![("oauth_github".into(), "GitHub".into()), ("oauth_google".into(), "Google".into())];
    }
    s.ui.cloud.providers.clone()
}

/// Opens the provider's sign-in page in the browser; the result returns on the deep link.
pub fn sign_in(s: &mut App, strategy: &str) {
    let Some(config) = config() else { return };
    if s.ui.cloud.signing_in.is_some() {
        return;
    }
    let attempt = s5_net::uuid();
    s.ui.cloud.signing_in = Some(SignIn { attempt: attempt.clone(), sign_in: None, nonce: None, finishing: false });
    s.ui.cloud.error = None;
    let strategy = strategy.to_owned();
    spawn(s, move || {
        let mut clerk = client(&config);
        let started = clerk.start_sign_in(&strategy, &attempt);
        keep(&clerk);
        match started {
            Ok((sign_in, page)) if crate::settings::codex_callback::open_browser(&page) => Event::Started { attempt, sign_in },
            Ok(_) => Event::Failed { attempt, message: "Could not open your sign-in browser.".into() },
            Err(failure) => Event::Failed { attempt, message: failure.message },
        }
    });
}

pub fn cancel_sign_in(s: &mut App) {
    s.ui.cloud.signing_in = None;
}

fn failed_message(code: &str) -> String {
    match code {
        "oauth_access_denied" | "access_denied" => "Sign-in was cancelled.".into(),
        code => format!("Sign-in failed ({code})."),
    }
}

/// Records what a deep link says about the sign-in in flight. Links for another attempt are
/// ignored, so a stale tab cannot finish a newer sign-in.
fn accept(ui: &mut Ui, callback: Callback) {
    let Some(pending) = ui.signing_in.as_mut() else { return };
    match callback {
        Callback::Completed { attempt, nonce } if attempt == pending.attempt => pending.nonce = Some(nonce),
        Callback::Failed { attempt, code } if attempt == pending.attempt => {
            ui.signing_in = None;
            ui.error = Some(failed_message(&code));
        }
        _ => {}
    }
}

/// The sign-in to finish, once it has both started and come back.
fn ready(ui: &mut Ui) -> Option<(String, String, String)> {
    let pending = ui.signing_in.as_mut().filter(|p| !p.finishing)?;
    let job = (pending.attempt.clone(), pending.sign_in.clone()?, pending.nonce.clone()?);
    pending.finishing = true;
    Some(job)
}

fn finish(s: &mut App) {
    let Some((attempt, sign_in, nonce)) = ready(&mut s.ui.cloud) else { return };
    let Some(config) = config() else { return };
    spawn(s, move || {
        let mut clerk = client(&config);
        let result = clerk.finish_sign_in(&sign_in, &nonce).map_err(|f| f.message);
        if let Ok(identity) = &result {
            let mut state = shared();
            state.client_jwt = clerk.client_jwt.clone().or(state.client_jwt.take());
            state.identity = Some(identity.clone());
            state.relay = None;
            state.environments.clear();
            save(&state);
        } else {
            keep(&clerk);
        }
        Event::SignedIn { attempt, result }
    });
}

/// `s5code://app/?__clerk_oauth_attempt=…`, handed over by the OS.
pub fn returned(s: &mut App, callback: Callback) {
    accept(&mut s.ui.cloud, callback);
    finish(s);
}

pub fn refresh(s: &mut App) {
    let Some(config) = config() else { return };
    if s.ui.cloud.identity.is_none() || s.ui.cloud.loading {
        return;
    }
    s.ui.cloud.loading = true;
    spawn(s, move || Event::Environments(session_token(&config).and_then(|jwt| relay::environments(&config.relay, &jwt))));
}

pub fn sign_out(s: &mut App) {
    let session = {
        let mut state = shared();
        let session = state.identity.take().map(|i| i.session);
        state.relay = None;
        state.environments.clear();
        save(&state);
        session
    };
    let ui = &mut s.ui.cloud;
    ui.identity = None;
    ui.environments = None;
    ui.signing_in = None;
    ui.error = None;
    if let (Some(session), Some(config)) = (session, config()) {
        spawn(s, move || {
            let mut clerk = client(&config);
            let _ = clerk.sign_out(&session);
            keep(&clerk);
            Event::SignedOut
        });
    }
}

/// Web `connectRelayEnvironment`: a new environment over T3 Connect, or a fallback route for
/// one saved over another address.
pub fn add(s: &mut App, linked: &Linked) {
    let label = if linked.label.is_empty() { linked.id.clone() } else { linked.label.clone() };
    match crate::env::add_relay(s, linked) {
        Some(true) => crate::shell::palette::toast(s, "Environment added", Some(&format!("Connecting to {label} through T3 Connect."))),
        Some(false) => crate::shell::palette::toast(s, "T3 Connect route added", Some(&format!("{label} falls back to T3 Connect when its other routes are unreachable."))),
        None => {}
    }
}

fn land(s: &mut App, event: Event) {
    let ui = &mut s.ui.cloud;
    match event {
        Event::Restored(Ok(identity)) => {
            if identity.is_none() {
                shared().identity = None;
            }
            ui.identity = identity;
        }
        // Offline at launch: keep the saved session and let sockets retry.
        Event::Restored(Err(_)) | Event::SignedOut => {}
        Event::Providers(providers) => ui.providers = providers,
        Event::Started { attempt, sign_in } => {
            if let Some(pending) = ui.signing_in.as_mut().filter(|p| p.attempt == attempt) {
                pending.sign_in = Some(sign_in);
            }
            finish(s);
        }
        Event::Failed { attempt, message } => {
            if ui.signing_in.as_ref().is_some_and(|p| p.attempt == attempt) {
                ui.signing_in = None;
                ui.error = Some(message);
            }
        }
        Event::SignedIn { attempt, result } => {
            if ui.signing_in.as_ref().is_none_or(|p| p.attempt != attempt) {
                return;
            }
            ui.signing_in = None;
            match result {
                Ok(identity) => {
                    ui.identity = Some(identity);
                    ui.environments = None;
                    ui.error = None;
                    crate::shell::palette::toast(s, "Signed in to T3 Connect", None);
                    reconnect_relay_environments(s);
                }
                Err(message) => ui.error = Some(message),
            }
        }
        Event::Environments(result) => {
            ui.loading = false;
            ui.environments = Some(result.map_err(|f| f.message));
        }
    }
}

/// Sockets waiting on a sign-in try again at once instead of after their backoff.
fn reconnect_relay_environments(s: &mut App) {
    let ids: Vec<String> = s.ui.env.saved.iter().filter(|e| crate::env::reached_by_relay(s, &e.id)).map(|e| e.id.clone()).collect();
    for id in ids {
        if let Some(net) = crate::env::net_of(s, &id) {
            net.reconnect();
        }
    }
}

pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    while let Some(event) = s.ui.cloud.events.as_ref().and_then(|(_, rx)| rx.try_recv().ok()) {
        land(s, event);
        changed = true;
    }
    let showing = matches!(&s.route, crate::Route::Settings(page) if page == "connections") || s.ui.wizard.open;
    if showing && s.ui.cloud.identity.is_some() && s.ui.cloud.environments.is_none() && !s.ui.cloud.loading {
        refresh(s);
    }
    changed
}

/// Web `CloudEnvironmentConnectRows`' status text and its button.
fn standing(s: &App, linked: &Linked) -> (String, Option<&'static str>) {
    let saved = s.ui.env.saved.iter().any(|e| e.id == linked.id);
    if saved && crate::env::reached_by_relay(s, &linked.id) {
        return (crate::env::status(s, &linked.id), None);
    }
    if saved {
        return ("T3 Connect · Saved without T3 Connect".into(), Some("Add route"));
    }
    ("T3 Connect · Not added".into(), Some("Add"))
}

fn muted(copy: impl Into<String>, t: &T) -> El {
    let copy: String = copy.into();
    text(copy).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap()
}

pub(crate) fn provider_buttons(s: &App, t: &T, busy: bool) -> El {
    let mut buttons = row().gap(8.0).flex_wrap();
    for (strategy, name) in providers(s) {
        buttons = buttons.child(kit::Button::new(format!("Continue with {name}")).variant("outline").size("sm").disabled(busy).on_press(move |s: &mut App| sign_in(s, &strategy)).build(t));
    }
    buttons
}

/// The account row: sign in, the sign-in in flight, or who is signed in.
fn account(s: &App, t: &T) -> El {
    let ui = &s.ui.cloud;
    let (title, description, trailing): (String, &str, El) = match (&ui.identity, &ui.signing_in) {
        (_, Some(pending)) => (
            if pending.finishing { "Signing in…".into() } else { "Finish signing in in your browser".into() },
            "T3 Code opens again when the browser hands the sign-in back.",
            kit::Button::new("Cancel").variant("ghost").size("sm").disabled(pending.finishing).on_press(cancel_sign_in).build(t),
        ),
        (Some(identity), None) => (
            format!("Signed in as {}", [identity.email.as_str(), identity.name.as_str()].into_iter().find(|v| !v.is_empty()).unwrap_or("your T3 Connect account")),
            "Computers linked to this account can be added from anywhere.",
            row()
                .gap(8.0)
                .child(kit::Button::new("Refresh").variant("ghost-muted").size("sm").icon(&icons::REFRESH_CW).disabled(ui.loading).on_press(refresh).build(t))
                .child(kit::Button::new("Sign out").variant("outline").size("sm").on_press(sign_out).build(t)),
        ),
        (None, None) => ("Sign in to T3 Connect".into(), "Reach the computers linked to your account from anywhere, without pairing each one.", provider_buttons(s, t, false)),
    };
    let mut copy = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(text(title.clone()).key(crate::settings::title_key(&title)).font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(muted(description, t));
    if let Some(error) = &ui.error {
        copy = copy.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap());
    }
    row().w_full().px(16.0).py(12.0).gap(24.0).center_y().flex_wrap().child(copy).child(trailing)
}

/// How to link a computer to the account (web onboarding's `npx t3 connect` block).
pub(crate) fn link_hint(t: &T) -> El {
    col().w_full().gap(8.0).child(muted("Run this on each computer you want to connect.", t)).child(crate::wizard::command_block("npx t3 connect", t))
}

fn environment_rows(s: &App, t: &T) -> Vec<El> {
    let pad = |inner: El| row().w_full().px(16.0).py(12.0).child(inner);
    match &s.ui.cloud.environments {
        None => vec![pad(muted("Loading computers…", t))],
        Some(Err(message)) => vec![pad(
            col()
                .w_full()
                .gap(4.0)
                .child(text("Could not load T3 Connect environments").font(theme::SM_MEDIUM).color(t.c.destructive))
                .child(muted(message.clone(), t))
                .child(row().mt(8.0).child(kit::Button::new("Retry").variant("outline").size("sm").disabled(s.ui.cloud.loading).on_press(refresh).build(t))),
        )],
        Some(Ok(list)) if list.is_empty() => vec![pad(col().w_full().gap(12.0).child(text("No computers linked yet.").font(theme::TEXT_SM).color(t.c.muted_foreground)).child(link_hint(t)))],
        Some(Ok(list)) => list
            .iter()
            .map(|linked| {
                let (status, action) = standing(s, linked);
                let button = match action {
                    Some(label) => {
                        let target = linked.clone();
                        kit::Button::new(label).size("sm").on_press(move |s: &mut App| add(s, &target)).build(t)
                    }
                    None => kit::Button::new(crate::env::short_status(s, &linked.id)).variant("outline").size("sm").disabled(true).build(t),
                };
                row()
                    .key(format!("t3-connect-env-{}", linked.id))
                    .w_full()
                    .px(16.0)
                    .py(12.0)
                    .gap(12.0)
                    .center_y()
                    .child(icon(&icons::CLOUD, 16.0).color(t.c.muted_foreground))
                    .child(
                        col()
                            .grow(1.0)
                            .min_w(0.0)
                            .gap(2.0)
                            .child(text(if linked.label.is_empty() { linked.id.clone() } else { linked.label.clone() }).font(theme::SM_MEDIUM).color(t.c.foreground).truncate())
                            .child(text(status).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate()),
                    )
                    .child(button)
            })
            .collect(),
    }
}

/// Settings → Connections: the account, then its computers.
pub fn section(s: &App, t: &T) -> Option<El> {
    config()?;
    let mut rows = vec![account(s, t)];
    if s.ui.cloud.identity.is_some() && s.ui.cloud.signing_in.is_none() {
        rows.extend(environment_rows(s, t));
    }
    Some(crate::settings::controls::section("T3 Connect", rows, t))
}

/// The account's computers the wizard can still set up: those not saved yet.
fn unsaved(s: &App) -> Vec<Linked> {
    match &s.ui.cloud.environments {
        Some(Ok(list)) => list.iter().filter(|l| !s.ui.env.saved.iter().any(|e| e.id == l.id)).cloned().collect(),
        _ => vec![],
    }
}

/// Web onboarding `ConnectAccountOption`: sign in, then pick the account's computers.
pub fn wizard_option(s: &App, t: &T) -> Option<El> {
    config()?;
    let ui = &s.ui.cloud;
    let signed_in = ui.identity.is_some();
    let open = !ui.wizard_folded;
    let trailing = match (signed_in, &ui.signing_in, &ui.environments) {
        (_, Some(_), _) => "Waiting for your browser…",
        (false, None, _) => "Sign in",
        (true, None, None) => "Loading computers…",
        _ => "",
    };
    let mut card = col().w_full().radius(8.0).border(1.0, t.c.border).fill(t.c.background).child(
        row()
            .key("wizard-t3-connect")
            .w_full()
            .h(56.0)
            .px(12.0)
            .gap(8.0)
            .center_y()
            .role("button")
            .label("T3 Connect")
            .expanded(open)
            .cursor(rune::Cursor::Pointer)
            .child(icon(&icons::CLOUD, 16.0).color(t.c.muted_foreground))
            .child(text("T3 Connect").font(theme::SM_MEDIUM).color(t.c.foreground).grow(1.0))
            .child(text(trailing).font(theme::TEXT_XS).color(t.c.muted_foreground))
            .child(icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 16.0).color(t.c.muted_foreground))
            .on_press(|s: &mut App| s.ui.cloud.wizard_folded = !s.ui.cloud.wizard_folded),
    );
    if !open {
        return Some(card);
    }
    let mut body = col().w_full().px(12.0).pb(12.0).gap(12.0);
    if !signed_in {
        body = body.child(muted("Sign in to reach the computers linked to your account.", t)).child(provider_buttons(s, t, ui.signing_in.is_some()));
        if let Some(error) = &ui.error {
            body = body.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap());
        }
    } else {
        let computers = unsaved(s);
        if matches!(ui.environments, Some(Ok(_))) && computers.is_empty() {
            body = body.child(text("No computers linked yet.").font(theme::TEXT_SM).color(t.c.muted_foreground));
        }
        if let Some(Err(message)) = &ui.environments {
            body = body.child(text(message.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap());
        }
        for linked in computers {
            let target = linked.clone();
            let label = if linked.label.is_empty() { linked.id.clone() } else { linked.label.clone() };
            body = body.child(
                row()
                    .w_full()
                    .gap(12.0)
                    .px(12.0)
                    .py(10.0)
                    .center_y()
                    .radius(8.0)
                    .border(1.0, t.c.border)
                    .cursor(rune::Cursor::Pointer)
                    .child(kit::Checkbox::new(false).hide_label().label(label.clone()).build(&kit::theme(t)))
                    .child(icon(&icons::MONITOR, 16.0).color(t.c.muted_foreground))
                    .child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground).truncate().grow(1.0))
                    .child(text("Available").font(theme::TEXT_XS).color(t.c.muted_foreground))
                    // The wizard ticks a computer the first time it appears.
                    .on_press(move |s: &mut App| add(s, &target)),
            );
        }
    }
    body = body.child(link_hint(t)).child(muted("Keep T3 Code running. Select the computers you want to set up above.", t));
    card = card.child(body);
    Some(card)
}

/// The account computer a saved environment is, for "Add T3 Connect" on its routes.
pub fn linked(s: &App, id: &str) -> Option<Linked> {
    match &s.ui.cloud.environments {
        Some(Ok(list)) => list.iter().find(|l| l.id == id).cloned(),
        _ => None,
    }
}

/// Scenes: a signed-in account with one computer added through T3 Connect and one not yet.
pub(crate) fn scene_signed_in(s: &mut App) {
    s.ui.cloud.identity = Some(Identity { session: "sess_1".into(), user: "user_1".into(), email: "ada@example.com".into(), name: "Ada Lovelace".into() });
    s.ui.cloud.environments = Some(Ok(vec![
        Linked { id: "env-box".into(), label: "Hetzner box".into(), http: "https://box.tunnel.example.com/".into(), ws: "wss://box.tunnel.example.com/".into() },
        Linked { id: "env-studio".into(), label: "Studio Mac".into(), http: "https://studio.tunnel.example.com/".into(), ws: "wss://studio.tunnel.example.com/".into() },
    ]));
}

pub(crate) fn scene_signing_in(s: &mut App) {
    s.ui.cloud.signing_in = Some(SignIn { attempt: "a-1".into(), sign_in: Some("sia_1".into()), nonce: None, finishing: false });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(sign_in: Option<&str>) -> Ui {
        Ui { signing_in: Some(SignIn { attempt: "a-1".into(), sign_in: sign_in.map(str::to_owned), nonce: None, finishing: false }), ..Ui::default() }
    }

    #[test]
    fn the_build_carries_the_public_cloud_config() {
        let config = config().expect("the repo's .env.example configures T3 Connect");
        assert_eq!(config.fapi, "https://clerk.s5code.touchtech.club");
        assert_eq!(config.template, "s5-relay");
        assert_eq!(config.relay, "https://relay.s5code.dpdns.org");
    }

    #[test]
    fn a_sign_in_finishes_once_started_and_returned_in_either_order() {
        let mut ui = pending(Some("sia_1"));
        assert_eq!(ready(&mut ui), None, "not back from the browser yet");
        accept(&mut ui, Callback::Completed { attempt: "a-0".into(), nonce: "stale".into() });
        assert_eq!(ready(&mut ui), None, "another attempt's link is ignored");
        accept(&mut ui, Callback::Completed { attempt: "a-1".into(), nonce: "n-1".into() });
        assert_eq!(ready(&mut ui), Some(("a-1".into(), "sia_1".into(), "n-1".into())));
        assert_eq!(ready(&mut ui), None, "finishing runs once");

        let mut early = pending(None);
        accept(&mut early, Callback::Completed { attempt: "a-1".into(), nonce: "n-1".into() });
        assert_eq!(ready(&mut early), None, "the browser beat the started reply");
        early.signing_in.as_mut().unwrap().sign_in = Some("sia_1".into());
        assert!(ready(&mut early).is_some());

        let mut denied = pending(Some("sia_1"));
        accept(&mut denied, Callback::Failed { attempt: "a-1".into(), code: "oauth_access_denied".into() });
        assert_eq!((denied.signing_in, denied.error.as_deref()), (None, Some("Sign-in was cancelled.")));
    }

    #[test]
    fn landed_sign_ins_and_lists_update_the_account() {
        let mut s = App::new();
        s.ui.cloud = pending(Some("sia_1"));
        let identity = Identity { session: "sess_1".into(), email: "ada@example.com".into(), ..Identity::default() };
        land(&mut s, Event::SignedIn { attempt: "a-0".into(), result: Ok(identity.clone()) });
        assert_eq!(s.ui.cloud.identity, None, "a stale attempt does not sign in");
        land(&mut s, Event::SignedIn { attempt: "a-1".into(), result: Ok(identity.clone()) });
        assert_eq!((s.ui.cloud.identity.as_ref(), s.ui.cloud.signing_in.as_ref()), (Some(&identity), None));
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Signed in to T3 Connect"));
        s.ui.cloud.loading = true;
        land(&mut s, Event::Environments(Err(Failure::new("Could not list T3 Connect environments: HTTP 502"))));
        assert!(!s.ui.cloud.loading);
        assert_eq!(s.ui.cloud.environments, Some(Err("Could not list T3 Connect environments: HTTP 502".into())));
    }

    #[test]
    fn account_computers_add_as_relay_environments_or_fallback_routes() {
        // SAFETY: tests that save environments all point here; keeps them off the real config dir.
        unsafe { std::env::set_var("S5_ENVIRONMENTS", std::env::temp_dir().join(format!("s5-env-{}.json", std::process::id()))) };
        let mut s = App::new();
        scene_signed_in(&mut s);
        let box_ = linked(&s, "env-box").unwrap();
        assert_eq!(standing(&s, &box_), ("T3 Connect · Not added".into(), Some("Add")));
        add(&mut s, &box_);
        let saved = s.ui.env.saved.iter().find(|e| e.id == "env-box").unwrap();
        assert_eq!((saved.base.as_str(), saved.bearer.as_str()), ("https://box.tunnel.example.com", crate::env::T3_CONNECT));
        assert_eq!(crate::shell::palette::toast_description(&s), Some("Connecting to Hetzner box through T3 Connect."));
        assert_eq!(standing(&s, &box_).1, None, "added: the row shows its connection");
        assert!(unsaved(&s).iter().all(|l| l.id != "env-box"), "the wizard offers only computers not saved yet");

        s.ui.env.saved.push(crate::env::Saved { id: "env-studio".into(), label: "Studio Mac".into(), base: "http://192.168.1.4:3773".into(), bearer: "token".into() });
        let studio = linked(&s, "env-studio").unwrap();
        assert_eq!(standing(&s, &studio), ("T3 Connect · Saved without T3 Connect".into(), Some("Add route")));
        add(&mut s, &studio);
        let routes = crate::env::routes(&s, "env-studio");
        assert_eq!(routes.len(), 2, "T3 Connect joins as a fallback after the saved address");
        assert_eq!((routes[0].bearer.as_str(), routes[1].bearer.as_str()), ("token", crate::env::T3_CONNECT));
        assert_eq!(crate::shell::palette::toast_title(&s), Some("T3 Connect route added"));
        add(&mut s, &studio);
        assert_eq!(crate::env::routes(&s, "env-studio").len(), 2, "adding twice keeps one T3 Connect route");
    }
}
