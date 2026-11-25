//! The setup wizard (components/onboarding/WelcomeWizard.tsx): Connect, Agents, Projects. It
//! opens once, on a fresh workspace whose client settings have no `onboardingCompletedAt`, and
//! closing it or finishing records that, as `completeOnboarding` does. Like the web it sets up
//! every computer ticked on the Connect step that is connected.

use std::collections::HashSet;

use rune::{El, col, icon, row, text};
use serde_json::{Value, json};

use crate::theme::{self, AlphaMul, T};
use crate::{App, Route, data, icons, kit};

const STAGES: [&str; 3] = ["Connect", "Agents", "Projects"];
/// `PRIMARY_AGENT_DRIVERS`.
const AGENTS: [(&str, &str); 2] = [("codex", "Codex"), ("claudeAgent", "Claude Code")];
const PRIVACY_POLICY_URL: &str = "https://t3.codes/privacy-policy";
/// `RECENT_PROJECT_WINDOW_MS` and `DEFAULT_SELECTION_MIN_THREADS` (onboarding/projectImport.logic.ts).
const RECENT_WINDOW_MS: i64 = 30 * 24 * 60 * 60 * 1000;
const DEFAULT_SELECTION_MIN_THREADS: u64 = 3;
/// `AgentSessionSource.literals`, each with its `IMPORT_SOURCE_NAMES` label.
const SOURCES: [(&str, &str); 3] = [("claudeAgent", "Claude Code"), ("codex", "Codex"), ("pi", "Pi")];
const SCAN_LIMIT_MESSAGE: &str = "Scan limit reached. Some projects or conversations may be missing.";
const IMPORT_PERMISSION_MESSAGE: &str = "This connection cannot import projects or thread history.";
const OPERATE: &str = "orchestration:operate";
/// Fields a scanned candidate gains: the computer it was found on and its `onboardingProjectKey`.
const ENV: &str = "_environment";
const KEY: &str = "_key";

#[derive(Default, Clone, Copy, PartialEq, Debug)]
enum Step {
    #[default]
    Connect,
    Agents,
    Projects,
}

#[derive(Default)]
pub struct Ui {
    decided: bool,
    pub open: bool,
    step: Step,
    /// Computers ticked on the Connect step, and every computer listed so far: each is ticked
    /// once when it first appears and is on (`autoSelectedComputers`), so unticking one sticks.
    chosen: HashSet<String>,
    seen: HashSet<String>,
    /// The connected computers Continue set up (`setupIds`), in list order.
    setup: Vec<String>,
    pairing_open: bool,
    /// The pairing link field, whether this wizard started the pairing in flight (so its
    /// result and error are the wizard's), and the "Need a pairing link?" help.
    link: rune::TextEdit,
    paired_here: bool,
    /// Show the pairing error under the field: the wizard's last pairing failed.
    pairing_failed: bool,
    pairing_help: bool,
    /// One project scan per computer set up (`useProjectScans`).
    scans: Vec<Scan>,
    /// Candidate keys to import.
    selected: HashSet<String>,
    /// Import sections opened or closed away from their default (groups open, other folders closed).
    toggled: HashSet<String>,
    import: Option<Import>,
    terminal: Option<Setup>,
}

/// `AGENT_ONBOARDING_THREAD_ID`: setup terminals belong to no real thread.
const SETUP_THREAD: &str = "onboarding-agent-setup";

/// `AgentInstallTerminal`: an agent's login or install command typed into a terminal on its
/// computer, for the person to read and run. Terminals live on the focused connection, so a
/// computer not in focus is brought up for it and `back` is restored on close.
struct Setup {
    env: String,
    driver: &'static str,
    instance: String,
    id: String,
    cwd: String,
    command: String,
    back: Option<String>,
}

struct Scan {
    env: String,
    pending: Option<u64>,
    candidates: Option<Vec<Value>>,
    truncated: bool,
    error: Option<String>,
}

impl Scan {
    fn loading(&self) -> bool {
        self.candidates.is_none() && self.error.is_none()
    }
}

/// One candidate at a time: create its project when it has none, then import its history.
struct Import {
    queue: Vec<Item>,
    stage: Option<Stage>,
    /// `resolveOnboardingLandingProject`: the first selected project that gained history, else
    /// the first imported in full, with its computer.
    with_history: Option<(String, String)>,
    completed: Option<(String, String)>,
    imported: u64,
    skipped: u64,
    failed: u64,
}

struct Item {
    env: String,
    candidate: Value,
    project: Option<String>,
}

/// Each holds its request, the computer, and the project.
enum Stage {
    Create(u64, String, String, String),
    History(u64, String, String),
}

/// A connection the way `resolveOnboardingSetup` weighs it.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Connected,
    Connecting,
    /// Off, failed, or retrying: setup skips it rather than wait.
    Other,
}

fn completed(s: &App) -> bool {
    s.client_settings.values.get("onboardingCompletedAt").is_some_and(|v| !v.is_null())
}

/// `isFreshFirstRunWorkspace`, without the welcome's bootstrap ids: at most this computer's
/// project at the server's cwd, and one untouched thread in it.
fn fresh(st: &s5_net::State) -> bool {
    if st.projects.len() > 1 || st.threads.len() > 1 {
        return false;
    }
    let same = |a: &str, b: &str| a.trim_end_matches(['/', '\\']) == b.trim_end_matches(['/', '\\']);
    let cwd = st.config.as_ref().and_then(|c| c["cwd"].as_str());
    let project = st.projects.first();
    if let Some(p) = project
        && (p.get(crate::env::TAG).is_some() || !cwd.is_some_and(|cwd| same(data::s(p, "workspaceRoot"), cwd)))
    {
        return false;
    }
    match st.threads.first() {
        None => true,
        Some(t) => {
            project.is_some_and(|p| data::s(t, "projectId") == data::s(p, "id"))
                && t["latestRun"].is_null()
                && t["latestUserMessageAt"].is_null()
                && t["runtime"].is_null()
        }
    }
}

fn complete(s: &mut App) {
    s.client_settings.values.insert("onboardingCompletedAt".into(), Value::String(crate::shell::palette::add_project::iso_now()));
    s.client_settings.save();
}

/// `getOnboardingProviderState`.
fn provider_state(p: Option<&Value>) -> &'static str {
    let Some(p) = p else { return "checking" };
    if p["enabled"] == false || p["status"] == "disabled" {
        "disabled"
    } else if p["installed"] == false && p["status"] == "warning" && p["auth"]["status"] == "unknown" {
        "checking"
    } else if p["installed"] == false {
        "install"
    } else if p["auth"]["status"] == "unauthenticated" {
        "signIn"
    } else if p["status"] == "ready" {
        "ready"
    } else {
        "attention"
    }
}

/// `quoteProviderBinary`.
fn quote_binary(path: &str, fallback: &str, os: &str) -> String {
    let safe = !path.is_empty() && path.chars().all(|c| c.is_ascii_alphanumeric() || "_./:\\-".contains(c));
    if safe && (os == "windows" || !path.contains('\\')) {
        return path.to_owned();
    }
    match os {
        "windows" => format!("& '{}'", path.replace('\'', "''")),
        "darwin" | "linux" => match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
            Some(rest) => format!("~/'{}'", rest.replace('\'', "'\"'\"'")),
            None => format!("'{}'", path.replace('\'', "'\"'\"'")),
        },
        _ => fallback.to_owned(),
    }
}

/// `resolveOnboardingProviderLoginCommand` for an installed agent, else
/// `resolveOnboardingProviderInstallCommand`.
fn setup_command(provider: &Value, config: &Value) -> String {
    let os = data::s(&config["environment"]["platform"], "os");
    let driver = data::s(provider, "driver");
    if provider["installed"] == false {
        let (windows, posix) = if driver == "claudeAgent" {
            ("irm https://claude.ai/install.ps1 | iex", "curl -fsSL https://claude.ai/install.sh | bash")
        } else {
            ("irm https://chatgpt.com/codex/install.ps1 | iex", "curl -fsSL https://chatgpt.com/codex/install.sh | sh")
        };
        return if os == "windows" { windows } else { posix }.to_owned();
    }
    let (fallback, verb) = match driver {
        "claudeAgent" => ("claude", "auth login"),
        "codex" => ("codex", "login"),
        other => return other.to_owned(),
    };
    let instance = &config["settings"]["providerInstances"][data::s(provider, "instanceId")]["config"];
    let binary = instance["binaryPath"].as_str().map(str::trim).filter(|p| !p.is_empty()).unwrap_or(fallback);
    format!("{} {verb}", quote_binary(binary, fallback, os))
}

fn can_operate_terminal(s: &App, env: &str) -> bool {
    crate::settings::permissions::can_on(s, env, "terminal:operate")
}

fn open_setup(s: &mut App, env: &str, driver: &'static str, instance: &str) {
    if !can_operate_terminal(s, env) {
        return;
    }
    let Some(config) = config(s, env).cloned() else { return };
    let Some(provider) = config["providers"].as_array().and_then(|list| list.iter().find(|p| data::s(p, "instanceId") == instance)) else { return };
    let cwd = data::s(&config, "cwd").to_owned();
    if cwd.is_empty() {
        return;
    }
    let command = setup_command(provider, &config);
    close_setup(s);
    let back = (s.ui.env.active != env).then(|| s.ui.env.active.clone());
    if back.is_some() {
        crate::env::switch(s, env);
    }
    let setup = Setup { env: env.to_owned(), driver, instance: instance.to_owned(), id: setup_id(driver), cwd, command, back };
    start_setup(s, &setup);
    s.ui.wizard.terminal = Some(setup);
}

fn setup_id(driver: &str) -> String {
    format!("onboarding-{driver}-{}", s5_net::uuid())
}

/// Opens the terminal and types the command without Enter, so nothing runs unread.
fn start_setup(s: &mut App, setup: &Setup) {
    use crate::panel::terminal;
    let payload = json!({"threadId": SETUP_THREAD, "terminalId": setup.id, "cwd": setup.cwd, "providerInstanceId": setup.instance});
    terminal::attach(s, SETUP_THREAD, &setup.id, payload);
    terminal::run(s, SETUP_THREAD, &setup.id, &terminal::Launch { cwd: &setup.cwd, worktree: None, root: None, size: None }, setup.command.clone());
    terminal::focus(s, SETUP_THREAD, &setup.id);
}

fn retry_setup(s: &mut App) {
    let Some(mut setup) = s.ui.wizard.terminal.take() else { return };
    if !can_operate_terminal(s, &setup.env) {
        s.ui.wizard.terminal = Some(setup);
        return;
    }
    crate::panel::terminal::close(s, SETUP_THREAD, &setup.id);
    setup.id = setup_id(setup.driver);
    start_setup(s, &setup);
    s.ui.wizard.terminal = Some(setup);
}

/// Closes the terminal (left running when access was withdrawn), asks the computer for its
/// agents again, and puts back the computer that was in focus.
fn close_setup(s: &mut App) {
    let Some(setup) = s.ui.wizard.terminal.take() else { return };
    let here = s.ui.env.active == setup.env;
    if here && can_operate_terminal(s, &setup.env) {
        crate::panel::terminal::close(s, SETUP_THREAD, &setup.id);
    }
    if let Some(net) = crate::env::net_of(s, &setup.env) {
        net.request("server.refreshProviders", json!({}));
    }
    if let Some(back) = setup.back.filter(|_| here) {
        crate::env::switch(s, &back);
    }
}

/// The setup terminal's header line, and whether opening it failed (offering Retry).
fn setup_status(s: &App, setup: &Setup) -> (String, bool) {
    let session = crate::panel::terminal::session(s, SETUP_THREAD, &setup.id);
    match session {
        _ if !can_operate_terminal(s, &setup.env) => ("This connection cannot control terminals.".into(), false),
        Some(session) if session.attached() => ("Review the command, then press Enter to run it.".into(), false),
        Some(session) if !session.status.is_empty() => ("Could not open the setup terminal.".into(), true),
        _ => ("Preparing command...".into(), false),
    }
}

fn setup_terminal(s: &App, setup: &Setup, t: &T) -> El {
    let (status, failed) = setup_status(s, setup);
    let mut buttons = row().gap(4.).center_y();
    if failed {
        buttons = buttons.child(kit::Button::new("Retry").variant("ghost").size("xs").disabled(!can_operate_terminal(s, &setup.env)).on_press(retry_setup).build(t));
    }
    buttons = buttons.child(kit::Button::new("Close").variant("ghost-muted").size("xs").on_press(close_setup).build(t));
    let header = row()
        .w_full()
        .px(12.)
        .py(6.)
        .gap(8.)
        .center_y()
        .child(text(status).font(theme::XS_MEDIUM).color(t.c.muted_foreground).grow(1.).shrink().min_w(0.).wrap())
        .child(buttons);
    let ready = crate::panel::terminal::session(s, SETUP_THREAD, &setup.id).is_some_and(|session| session.attached());
    let mut body = col().key("wizard-setup-terminal").w_full().h(256.);
    if ready {
        body = body.child(crate::panel::terminal::viewport(s, t, SETUP_THREAD, &setup.id));
    }
    col().w_full().mt(16.).radius(8.).clip_rounded(8.).border(1., t.c.border.alpha_mul(0.7)).fill(t.c.background).child(header).child(rune::el().w_full().h(1.).fill(t.c.border.alpha_mul(0.6))).child(body)
}

/// `getProviderSummary`: a headline, and its detail when there is one.
fn summary(p: Option<&Value>) -> (String, Option<String>) {
    let Some(p) = p else {
        return ("Checking provider status".into(), Some("Waiting for the server to report installation and authentication details.".into()));
    };
    let message = p["message"].as_str().map(str::to_owned);
    let or = |fallback: &str| Some(p["message"].as_str().unwrap_or(fallback).to_owned());
    let auth_label = p["auth"]["label"].as_str().or(p["auth"]["type"].as_str()).filter(|l| !l.is_empty());
    let labelled = |headline: &str| auth_label.map_or_else(|| headline.to_owned(), |label| format!("{headline} · {label}"));
    if p["enabled"] == false || p["status"] == "disabled" {
        ("Disabled".into(), or("This provider is installed but disabled for new sessions in T3 Code."))
    } else if p["installed"] == false {
        ("Not found".into(), or("CLI not detected on PATH."))
    } else if p["auth"]["status"] == "unauthenticated" {
        (labelled("Not authenticated"), message)
    } else if p["status"] == "warning" {
        ("Needs attention".into(), or("The provider is installed, but the server could not fully verify it."))
    } else if p["status"] == "error" {
        ("Unavailable".into(), or("The provider failed its startup checks."))
    } else if p["auth"]["status"] == "authenticated" {
        (labelled("Authenticated"), message)
    } else {
        ("Available".into(), message)
    }
}

/// `AgentCard`'s line under a provider that is not ready: the headline, then its detail.
fn summary_line(p: Option<&Value>) -> String {
    let (headline, detail) = summary(p);
    match detail.filter(|d| !d.is_empty()) {
        Some(detail) => format!("{headline} · {detail}"),
        None => headline,
    }
}

fn rank(p: &Value) -> usize {
    ["checking", "disabled", "install", "attention", "signIn", "ready"].iter().position(|k| *k == provider_state(Some(p))).unwrap_or(0)
}

/// `selectOnboardingProvidersByDriver`: the most ready instance of each driver, the first on a tie.
fn best<'a>(providers: &'a [Value], driver: &str) -> Option<&'a Value> {
    providers.iter().filter(|p| data::s(p, "driver") == driver).rev().max_by_key(|p| rank(p))
}

/// `ConnectedAgentsStep`'s cards: every Codex instance (or its placeholder) and the best Claude
/// one, for the drivers the managed policy allows (`isManagedDriverAllowed`).
fn agent_cards<'a>(providers: &'a [Value], policy: &Value) -> Vec<(&'static str, &'static str, Option<&'a Value>)> {
    let allowed = |driver: &str| policy["limits"]["providers"]["allowedDrivers"].as_array().is_none_or(|list| list.iter().any(|d| d == driver));
    let mut cards = vec![];
    for (driver, name) in AGENTS {
        if !allowed(driver) {
            continue;
        }
        let instances: Vec<&Value> = if driver == "codex" { providers.iter().filter(|p| data::s(p, "driver") == driver).collect() } else { vec![] };
        if instances.is_empty() {
            cards.push((driver, name, best(providers, driver)));
        } else {
            cards.extend(instances.into_iter().map(|p| (driver, name, Some(p))));
        }
    }
    cards
}

fn config<'a>(s: &'a App, key: &str) -> Option<&'a Value> {
    crate::env::source(s, key)?.state().config.as_ref()
}

fn providers_on<'a>(s: &'a App, key: &str) -> &'a [Value] {
    config(s, key).and_then(|c| c["providers"].as_array()).map_or(&[], |v| v.as_slice())
}

/// `managedTelemetryOffMessage` for the primary environment, this computer's.
fn telemetry_off(s: &App) -> Option<String> {
    let policy = &config(s, crate::env::LOCAL)?["managedPolicy"];
    (policy["limits"]["telemetry"] == false).then(|| format!("{} turned off anonymous usage data.", policy["organization"].as_str().unwrap_or_default()))
}

/// The Connect step's privacy line.
fn privacy_copy(telemetry_off: Option<&str>) -> String {
    match telemetry_off {
        Some(off) => format!("{off} To read more about how your data is used, see our privacy policy."),
        None => "T3 Code collects anonymous usage data to help us improve it. To read more about how your data is used and how to opt out, see our privacy policy.".into(),
    }
}

fn phase(s: &App, key: &str) -> Phase {
    let Some(src) = crate::env::source(s, key) else { return Phase::Other };
    let st = src.state();
    match &st.conn {
        s5_net::Conn::Open if st.shell_ready => Phase::Connected,
        s5_net::Conn::Open | s5_net::Conn::Connecting => Phase::Connecting,
        s5_net::Conn::Closed(_) => Phase::Other,
    }
}

/// `resolveOnboardingSetup`: the ticked computers that are connected, and those skipped. It waits
/// only while one is still making its first connection.
fn resolve_setup(envs: &[(String, Phase)], chosen: &HashSet<String>) -> (bool, Vec<String>, Vec<String>) {
    let picked: Vec<&(String, Phase)> = envs.iter().filter(|(key, _)| chosen.contains(key)).collect();
    let keys = |want: Phase| picked.iter().filter(|(_, p)| *p == want).map(|(key, _)| key.clone()).collect::<Vec<_>>();
    let ready = keys(Phase::Connected);
    let settling = picked.iter().any(|(_, p)| *p == Phase::Connecting);
    (!ready.is_empty() && !settling, ready, keys(Phase::Other))
}

fn setup_of(s: &App) -> (bool, Vec<String>, Vec<String>) {
    let envs: Vec<(String, Phase)> = crate::env::keys(s).into_iter().map(|key| (key.clone(), phase(s, &key))).collect();
    resolve_setup(&envs, &s.ui.wizard.chosen)
}

/// The note under the computers when some ticked ones are not connected.
fn skipped_note(labels: &[String]) -> Option<String> {
    (!labels.is_empty()).then(|| format!("Not connected, so setup skips {}. You can set {} up later from Settings.", labels.join(", "), if labels.len() == 1 { "it" } else { "them" }))
}

/// `onboardingProjectKey`.
fn key(env: &str, path: &str) -> String {
    json!([env, path]).to_string()
}

/// Each candidate with the computer it was found on and its key.
fn tag_candidates(env: &str, list: Vec<Value>) -> Vec<Value> {
    list.into_iter()
        .map(|mut c| {
            let k = key(env, data::s(&c, "path"));
            c[ENV] = Value::String(env.to_owned());
            c[KEY] = Value::String(k);
            c
        })
        .collect()
}

fn selected_candidates(ui: &Ui) -> impl Iterator<Item = &Value> + '_ {
    ui.scans.iter().flat_map(|scan| scan.candidates.iter().flatten()).filter(|c| ui.selected.contains(data::s(c, KEY)))
}

/// Web's `canImport`: every computer holding a selected project lets this connection operate.
fn can_import(s: &App) -> bool {
    selected_candidates(&s.ui.wizard).all(|c| crate::settings::permissions::can_on(s, data::s(c, ENV), OPERATE))
}

/// The warning toast after an import that left something behind.
fn import_warning(imported: u64, skipped: u64, failed: u64) -> Option<String> {
    if skipped == 0 && failed == 0 {
        return None;
    }
    let threads = |n: u64| if n == 1 { "thread" } else { "threads" };
    Some(if imported > 0 && skipped > 0 {
        format!("Imported {imported} {}. {skipped} {} could not be imported.", threads(imported), threads(skipped))
    } else if skipped > 0 {
        format!("{skipped} {} not be imported.", if skipped == 1 { "thread could" } else { "threads could" })
    } else if imported > 0 {
        format!("Imported {imported} {}. Some thread history could not be imported.", threads(imported))
    } else {
        "Could not import thread history.".into()
    })
}

pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    if !s.ui.wizard.decided && s.state().shell_ready {
        s.ui.wizard.decided = true;
        if !completed(s) {
            if fresh(s.state()) {
                s.ui.wizard.open = true;
                // `expandPairingInitially`: with no local backend, pairing is the only way on.
                s.ui.wizard.pairing_open = s.ui.desktop.local_off;
                changed = true;
            } else {
                complete(s);
            }
        }
    }
    if !s.ui.wizard.open {
        return changed;
    }
    // `onSessionExited`: the login or install finished.
    if let Some(setup) = &s.ui.wizard.terminal
        && crate::panel::terminal::session(s, SETUP_THREAD, &setup.id).is_some_and(|session| session.ended)
    {
        close_setup(s);
        changed = true;
    }
    for key in crate::env::keys(s) {
        if s.ui.wizard.seen.insert(key.clone()) && !s.ui.env.off.contains(&key) {
            s.ui.wizard.chosen.insert(key);
            changed = true;
        }
    }
    // `selectNewComputers`: an account computer is ticked, and so added, once its status is known.
    for linked in crate::cloud::new_computers(s) {
        if s.ui.wizard.seen.insert(linked.id.clone()) {
            choose(s, &linked, true);
            changed = true;
        }
    }
    // `onPaired`: the new computer is ticked, and the form folds away.
    if s.ui.wizard.paired_here && !crate::env::pairing(s) {
        s.ui.wizard.paired_here = false;
        match s.ui.env.last_paired.take() {
            Some(id) => {
                s.ui.wizard.seen.insert(id.clone());
                s.ui.wizard.chosen.insert(id);
                s.ui.wizard.pairing_open = false;
                s.ui.wizard.link.clear();
            }
            None => s.ui.wizard.pairing_failed = true,
        }
        changed = true;
    }
    for i in 0..s.ui.wizard.scans.len() {
        let Some(id) = s.ui.wizard.scans[i].pending else { continue };
        let Some(reply) = s.src.net().and_then(|n| n.take_reply(id)) else { continue };
        let now = data::now_ms();
        let ui = &mut s.ui.wizard;
        let scan = &mut ui.scans[i];
        scan.pending = None;
        match reply {
            Ok(v) => {
                let candidates = tag_candidates(&scan.env, v["candidates"].as_array().cloned().unwrap_or_default());
                ui.selected.extend(candidates.iter().filter(|c| recent(c, now)).map(|c| data::s(c, KEY).to_owned()));
                scan.truncated = v["truncated"] == true;
                scan.candidates = Some(candidates);
            }
            Err(e) => scan.error = Some(crate::shell::palette::error_message(&e, "Could not look for projects.")),
        }
        changed = true;
    }
    changed | poll_import(s)
}

fn poll_import(s: &mut App) -> bool {
    let Some(mut import) = s.ui.wizard.import.take() else { return false };
    let mut changed = false;
    let reply = |s: &mut App, id: u64| s.src.net().and_then(|n| n.take_reply(id));
    let send = |s: &mut App, env: &str, method: &str, params: Value| crate::env::net_of(s, env).map(|n| n.request(method, params));
    let history = |s: &mut App, env: &str, project: &str, path: &str| send(s, env, "agentSessions.import", json!({"projectId": project, "expectedWorkspaceRoot": path}));
    loop {
        match import.stage.take() {
            Some(Stage::Create(id, env, project, path)) => match reply(s, id) {
                None => {
                    import.stage = Some(Stage::Create(id, env, project, path));
                    break;
                }
                Some(Ok(_)) => match history(s, &env, &project, &path) {
                    Some(id) => import.stage = Some(Stage::History(id, env, project)),
                    None => import.failed += 1,
                },
                Some(Err(_)) => import.failed += 1,
            },
            Some(Stage::History(id, env, project)) => match reply(s, id) {
                None => {
                    import.stage = Some(Stage::History(id, env, project));
                    break;
                }
                Some(reply) => {
                    changed = true;
                    match reply {
                        Ok(v) => {
                            let (imported, skipped) = (v["importedCount"].as_u64().unwrap_or(0), v["skippedCount"].as_u64().unwrap_or(0));
                            import.imported += imported;
                            import.skipped += skipped;
                            if imported > 0 && import.with_history.is_none() {
                                import.with_history = Some((env.clone(), project.clone()));
                            }
                            if skipped == 0 && import.completed.is_none() {
                                import.completed = Some((env, project));
                            }
                        }
                        Err(_) => import.failed += 1,
                    }
                }
            },
            None => {
                let Some(Item { env, candidate, project }) = import.queue.pop() else { break };
                // `stopForDeniedAccess`: access withdrawn mid-import stops it, and the step says why.
                if !crate::settings::permissions::can_on(s, &env, OPERATE) {
                    return true;
                }
                let path = data::s(&candidate, "path").to_owned();
                let stage = match project {
                    Some(project) => history(s, &env, &project, &path).map(|id| Stage::History(id, env.clone(), project)),
                    None => {
                        let project = s5_net::uuid();
                        let command = json!({
                            "type": "project.create",
                            "commandId": format!("onboarding:project:create:{project}"),
                            "projectId": project,
                            "title": data::s(&candidate, "title"),
                            "workspaceRoot": path,
                            "createWorkspaceRootIfMissing": false,
                            "defaultModelSelection": null,
                        });
                        crate::env::adopt(s, &project, &env);
                        send(s, &env, "projects.mutate", command).map(|id| Stage::Create(id, env.clone(), project, path))
                    }
                };
                match stage {
                    Some(stage) => import.stage = Some(stage),
                    None => import.failed += 1,
                }
            }
        }
    }
    if import.stage.is_none() && import.queue.is_empty() {
        let Import { with_history, completed, imported, skipped, failed, .. } = import;
        let landing = with_history.or(completed);
        if let Some((env, project)) = &landing {
            crate::env::adopt(s, project, env);
        }
        let warning = import_warning(imported, skipped, failed);
        finish(s, landing.map(|(_, project)| project));
        match warning {
            Some(warning) => crate::shell::palette::toast_sticky(s, "Some history was not imported", Some(&warning)),
            None if imported > 0 => crate::shell::palette::toast(s, &format!("Imported {imported} {}", if imported == 1 { "thread" } else { "threads" }), None),
            None => {}
        }
        return true;
    }
    s.ui.wizard.import = Some(import);
    changed
}

fn finish(s: &mut App, project: Option<String>) {
    complete(s);
    // Web keeps the wizard open on this failure; here an unreadable settings file would trap
    // the person in it, so it closes and says the choice wasn't saved.
    if s.client_settings.error.is_some() {
        crate::shell::palette::toast_error_brief(s, "Could not finish setup", Some("Your settings could not be saved. Try again."));
    }
    close_setup(s);
    s.ui.wizard.open = false;
    s.ui.wizard.import = None;
    if project.is_some() {
        s.navigate(Route::Home { project_id: project });
    }
}

fn close(s: &mut App) {
    if s.ui.wizard.import.is_none() {
        finish(s, None);
    }
}

fn go(s: &mut App, step: Step) {
    // The terminal belongs to the Agents step, as web's unmounts with it.
    close_setup(s);
    s.ui.wizard.step = step;
    match step {
        // `refreshProviders` on mount, so a CLI installed a moment ago shows.
        Step::Agents => {
            for env in s.ui.wizard.setup.clone() {
                if let Some(net) = crate::env::net_of(s, &env) {
                    net.request("server.refreshProviders", json!({}));
                }
            }
        }
        Step::Projects => sync_scans(s),
        Step::Connect => {}
    }
}

/// One scan per computer set up, kept across visits; a computer added later is scanned then.
fn sync_scans(s: &mut App) {
    let setup = s.ui.wizard.setup.clone();
    s.ui.wizard.scans.retain(|scan| setup.contains(&scan.env));
    for env in &setup {
        if !s.ui.wizard.scans.iter().any(|scan| scan.env == *env) {
            s.ui.wizard.scans.push(Scan { env: env.clone(), pending: None, candidates: None, truncated: false, error: None });
            scan(s, env);
        }
    }
    s.ui.wizard.scans.sort_by_key(|scan| setup.iter().position(|env| *env == scan.env));
}

fn scan(s: &mut App, env: &str) {
    let id = crate::env::net_of(s, env).map(|n| n.request("agentSessions.scan", json!({})));
    if let Some(scan) = s.ui.wizard.scans.iter_mut().find(|scan| scan.env == env) {
        scan.error = None;
        scan.pending = id;
    }
}

/// `partitionOnboardingProjects`' `recent`, which starts selected: a git repository with a few
/// threads, active in the last month. A server predating git scans sends no `git` at all.
fn recent(c: &Value, now: i64) -> bool {
    !c.get("git").is_some_and(Value::is_null)
        && c["threadCount"].as_u64().unwrap_or(0) >= DEFAULT_SELECTION_MIN_THREADS
        && data::ts(c, "lastActiveAt").is_some_and(|at| at >= now - RECENT_WINDOW_MS && at <= now)
}

#[derive(Debug, PartialEq)]
struct Group {
    key: String,
    label: String,
    /// Indexes into the candidates.
    members: Vec<usize>,
    threads: u64,
    last_active: Option<i64>,
}

/// `groupOnboardingProjects`: candidates by repository, most recently active first, and the
/// folders outside git.
fn groups(candidates: &[Value]) -> (Vec<Group>, Vec<usize>) {
    let mut groups: Vec<Group> = vec![];
    let mut other = vec![];
    for (i, c) in candidates.iter().enumerate() {
        let git = c.get("git");
        if git.is_some_and(Value::is_null) {
            other.push(i);
            continue;
        }
        let key = match git.and_then(|g| g["remoteKey"].as_str()) {
            Some(remote) => format!("remote:{remote}"),
            None => format!("path:{}", data::s(c, "path")),
        };
        let (threads, active) = (c["threadCount"].as_u64().unwrap_or(0), data::ts(c, "lastActiveAt"));
        match groups.iter_mut().find(|g| g.key == key) {
            Some(g) => {
                g.members.push(i);
                g.threads += threads;
                g.last_active = g.last_active.max(active);
            }
            None => groups.push(Group {
                key,
                label: git.and_then(|g| g["repository"].as_str()).unwrap_or(data::s(c, "title")).to_owned(),
                members: vec![i],
                threads,
                last_active: active,
            }),
        }
    }
    groups.sort_by(|a, b| match (a.last_active, b.last_active) {
        (x, y) if x == y => a.label.cmp(&b.label),
        (None, _) => std::cmp::Ordering::Greater,
        (_, None) => std::cmp::Ordering::Less,
        (Some(x), Some(y)) => y.cmp(&x),
    });
    (groups, other)
}

/// `splitOnboardingProjectPath`: the folder name and the path before it. A drive root or a
/// network share is a name of its own.
fn split_path(path: &str) -> (&str, &str) {
    let b = path.as_bytes();
    let sep = |c: u8| c == b'/' || c == b'\\';
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2..].iter().all(|&c| sep(c));
    let share = b.len() > 2 && sep(b[0]) && sep(b[1]) && {
        let parts: Vec<&str> = path[2..].trim_end_matches(['/', '\\']).split(['/', '\\']).collect();
        parts.len() == 2 && parts.iter().all(|p| !p.is_empty())
    };
    let trimmed = path.trim_end_matches(['/', '\\']);
    if drive || share || trimmed.is_empty() {
        return ("", path);
    }
    match trimmed.rfind(['/', '\\']) {
        Some(i) => (&trimmed[..=i], &trimmed[i + 1..]),
        None => ("", trimmed),
    }
}

fn set_selected(s: &mut App, keys: &[String], on: bool) {
    if s.ui.wizard.import.is_some() {
        return;
    }
    for k in keys {
        if on {
            s.ui.wizard.selected.insert(k.clone());
        } else {
            s.ui.wizard.selected.remove(k);
        }
    }
}

fn toggle_section(s: &mut App, key: &str) {
    if !s.ui.wizard.toggled.remove(key) {
        s.ui.wizard.toggled.insert(key.to_owned());
    }
}

/// `ImportRowMeta`: which agents left history, how many threads, and how long ago.
fn row_meta(sources: Option<&Value>, threads: u64, last_active: Option<i64>, t: &T) -> El {
    let muted = t.c.muted_foreground;
    let mut out = row().gap(4.).center_y();
    for (source, name) in SOURCES {
        let has = sources.and_then(Value::as_array).is_some_and(|list| list.iter().any(|v| v == source));
        out = out.child(row().size(16., 16.).center().children(has.then(|| crate::chat::composer::provider_icon(t, source, 12.).label(name))));
    }
    let age = last_active.map(|at| crate::shell::sidebar::logic::relative_label(at, data::now_ms())).unwrap_or_default();
    out.child(row().w(40.).justify_end().child(text(threads.to_string()).font(theme::TEXT_XS).color(muted)))
        .child(row().w(36.).justify_end().child(text(age).font(theme::TEXT_XS).color(muted)))
}

/// `ImportCandidateRow`: a top-level row names its repository; a nested one shows only its path.
fn candidate_row(c: &Value, label: Option<&str>, nested: bool, ui: &Ui, t: &T) -> El {
    let path = data::s(c, "path").to_owned();
    let (parent, name) = split_path(&path);
    let (font, ink) = if label.is_some() { (theme::mono(theme::TEXT_2XS), t.c.muted_foreground) } else { (theme::mono(theme::TEXT_XS), t.c.foreground) };
    let mut info = row().grow(1.).shrink().min_w(0.).gap(8.).center_y().help(path.clone());
    if let Some(label) = label {
        info = info.child(text(label.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate().shrink().min_w(0.));
    }
    info = info.child(row().shrink().min_w(0.).child(text(parent.to_owned()).font(font).color(ink).truncate().shrink().min_w(0.)).child(text(name.to_owned()).font(font).color(ink).truncate()));
    let checked = ui.selected.contains(data::s(c, KEY));
    let toggle = data::s(c, KEY).to_owned();
    row()
        .w_full()
        .gap(10.)
        .pr(8.)
        .pl(if nested { 32. } else { 8. })
        .py(6.)
        .radius(6.)
        .center_y()
        .hover(t.c.muted.alpha_mul(0.4))
        .child(kit::Checkbox::new(checked).hide_label().label(label.unwrap_or(&path)).disabled(ui.import.is_some()).build(&kit::theme(t)))
        .child(info)
        .child(row_meta((!nested).then(|| &c["sources"]), c["threadCount"].as_u64().unwrap_or(0), data::ts(c, "lastActiveAt"), t))
        .on_press(move |s: &mut App| set_selected(s, std::slice::from_ref(&toggle), !checked))
}

/// A collapsible section's header: its checkbox selects every member, the rest opens or closes it.
fn section_header(section: &str, open: bool, keys: Vec<String>, ui: &Ui, title: El, meta: El, t: &T) -> El {
    let chosen = keys.iter().filter(|k| ui.selected.contains(*k)).count();
    let all = chosen == keys.len();
    let section = section.to_owned();
    row()
        .w_full()
        .gap(10.)
        .px(8.)
        .py(6.)
        .radius(6.)
        .center_y()
        .hover(t.c.muted.alpha_mul(0.4))
        .child(
            kit::Checkbox::new(all)
                .indeterminate(chosen > 0 && !all)
                .hide_label()
                .label("Select section")
                .disabled(ui.import.is_some())
                .on_toggle(move |s: &mut App| set_selected(s, &keys, !all))
                .build(&kit::theme(t)),
        )
        .child(
            row()
                .grow(1.)
                .shrink()
                .min_w(0.)
                .gap(6.)
                .center_y()
                .role("button")
                .cursor(rune::Cursor::Pointer)
                .child(icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 14.).color(t.c.muted_foreground))
                .child(title.shrink().min_w(0.))
                .child(rune::el().grow(1.))
                .child(meta)
                .on_press(move |s: &mut App| toggle_section(s, &section)),
        )
}

/// `ImportCandidateList` for one computer's candidates; its sections' keys start with `env`.
fn import_list(env: &str, candidates: &[Value], ui: &Ui, t: &T) -> El {
    let (repositories, other) = groups(candidates);
    let keys = |members: &[usize]| members.iter().map(|&i| data::s(&candidates[i], KEY).to_owned()).collect::<Vec<_>>();
    let mut list = col().w_full().gap(2.);
    for group in &repositories {
        if let [only] = group.members[..] {
            list = list.child(candidate_row(&candidates[only], Some(&group.label), false, ui, t));
            continue;
        }
        let section = format!("{env}\n{}", group.key);
        let open = !ui.toggled.contains(&section);
        let mut sources: Vec<Value> = vec![];
        for &i in &group.members {
            for source in candidates[i]["sources"].as_array().into_iter().flatten() {
                if !sources.contains(source) {
                    sources.push(source.clone());
                }
            }
        }
        let title = text(group.label.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate();
        list = list.child(section_header(&section, open, keys(&group.members), ui, title, row_meta(Some(&Value::Array(sources)), group.threads, group.last_active, t), t));
        if open {
            for &i in &group.members {
                list = list.child(candidate_row(&candidates[i], None, true, ui, t));
            }
        }
    }
    if !other.is_empty() {
        let section = format!("{env}\nother");
        let open = ui.toggled.contains(&section);
        let count = format!("{} {}", other.len(), if other.len() == 1 { "folder" } else { "folders" });
        let title = text("Other folders").font(theme::TEXT_SM).color(t.c.muted_foreground).truncate();
        list = list.child(section_header(&section, open, keys(&other), ui, title, text(count).font(theme::TEXT_XS).color(t.c.muted_foreground), t));
        if open {
            for &i in &other {
                list = list.child(candidate_row(&candidates[i], None, true, ui, t));
            }
        }
    }
    list
}

fn start_import(s: &mut App) {
    if s.ui.wizard.import.is_some() || !can_import(s) {
        return;
    }
    let selected: Vec<Value> = selected_candidates(&s.ui.wizard).cloned().collect();
    if selected.is_empty() {
        return finish(s, None);
    }
    let queue = selected
        .into_iter()
        .rev()
        .map(|candidate| {
            let env = data::s(&candidate, ENV).to_owned();
            // `resolveOnboardingProjectId`: the server's match first, then this computer's projects.
            let project = candidate["projectId"].as_str().map(str::to_owned).or_else(|| {
                s.state()
                    .projects
                    .iter()
                    .find(|p| data::s(p, "workspaceRoot") == data::s(&candidate, "path") && p["archivedAt"].is_null() && crate::env::owner(s, data::s(p, "id")) == env)
                    .map(|p| data::s(p, "id").to_owned())
            });
            Item { env, candidate, project }
        })
        .collect();
    s.ui.wizard.import = Some(Import { queue, stage: None, with_history: None, completed: None, imported: 0, skipped: 0, failed: 0 });
}

fn heading(title: &str, description: Option<&str>, t: &T) -> El {
    let mut out = col().w_full().gap(10.).child(text(title.to_owned()).font(theme::weight(theme::TEXT_2XL, 600.)).color(t.c.foreground).wrap());
    if let Some(description) = description {
        out = out.child(text(description.to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap());
    }
    out
}

fn card(t: &T) -> El {
    row().w_full().gap(12.).px(12.).py(12.).center_y().radius(8.).border(1., t.c.border).fill(t.c.background)
}

fn continue_button(t: &T, label: &str, disabled: bool, f: impl Fn(&mut App) + 'static) -> El {
    kit::Button::new(label).icon(&icons::ARROW_RIGHT).disabled(disabled).on_press(f).build(t)
}

/// The T3 Connect card's computers share the Connect step's selection.
pub(crate) fn chosen(s: &App, id: &str) -> bool {
    s.ui.wizard.chosen.contains(id)
}

/// Web `onCheckedChange` for an account computer: ticking one not saved yet adds it through
/// T3 Connect so it is connecting by Continue; unticking leaves it saved but out of setup.
pub(crate) fn choose(s: &mut App, linked: &s5_net::cloud::relay::Linked, on: bool) {
    if !on {
        s.ui.wizard.chosen.remove(&linked.id);
        return;
    }
    s.ui.wizard.seen.insert(linked.id.clone());
    s.ui.wizard.chosen.insert(linked.id.clone());
    if !s.ui.env.saved.iter().any(|e| e.id == linked.id) {
        crate::cloud::add(s, linked);
    }
}

fn toggle_computer(s: &mut App, env: &str) {
    if !s.ui.wizard.chosen.remove(env) {
        s.ui.wizard.chosen.insert(env.to_owned());
    }
}

fn connect(s: &App, t: &T) -> El {
    let mut body = col().w_full().gap(8.).child(heading("Connect your computers", Some("Choose one or more computers. We’ll set up agents and projects on each."), t)).child(rune::el().h(12.));
    // `directEnvironments`: computers reached through T3 Connect are listed in its card.
    let relay_card = crate::cloud::config().is_some();
    for env in crate::env::keys(s).into_iter().filter(|env| !(relay_card && crate::env::reached_by_relay(s, env))) {
        let checked = s.ui.wizard.chosen.contains(&env);
        let label = crate::env::label(s, &env);
        let state = if s.ui.env.off.contains(&env) { "Off" } else { crate::env::short_status(s, &env) };
        let toggle = env.clone();
        body = body.child(
            card(t)
                .cursor(rune::Cursor::Pointer)
                .child(kit::Checkbox::new(checked).hide_label().label(label.clone()).build(&kit::theme(t)))
                .child(icon(&icons::MONITOR, 16.).color(t.c.muted_foreground))
                .child(
                    col()
                        .grow(1.)
                        .shrink()
                        .min_w(0.)
                        .gap(2.)
                        .child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground).truncate())
                        .children(crate::env::base_url(s, &env).map(|url| text(url).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate())),
                )
                .child(text(state).font(theme::TEXT_XS).color(t.c.muted_foreground).help(crate::env::status(s, &env)))
                .on_press(move |s: &mut App| toggle_computer(s, &toggle)),
        );
    }
    let open = s.ui.wizard.pairing_open;
    let mut pairing = col().w_full().radius(8.).border(1., t.c.border).fill(t.c.background).child(
        row()
            .key("wizard-add-computer")
            .w_full()
            .h(56.)
            .px(12.)
            .gap(8.)
            .center_y()
            .role("button")
            .label("Add a computer")
            .cursor(rune::Cursor::Pointer)
            .child(icon(&icons::LINK, 16.).color(t.c.muted_foreground))
            .child(text("Add a computer").font(theme::SM_MEDIUM).color(t.c.foreground).grow(1.))
            .child(icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 16.).color(t.c.muted_foreground))
            .on_press(|s: &mut App| {
                if !crate::env::pairing(s) {
                    s.ui.wizard.pairing_open = !s.ui.wizard.pairing_open;
                }
            }),
    );
    if open {
        pairing = pairing.child(pairing_form(s, t));
    }
    let (ready, _, skipped) = setup_of(s);
    let labels: Vec<String> = skipped.iter().map(|env| crate::env::label(s, env)).collect();
    if let Some(cloud) = crate::cloud::wizard_option(s, t) {
        body = body.child(rune::el().h(8.)).child(cloud);
    }
    body = body.child(rune::el().h(8.)).child(pairing);
    if let Some(note) = skipped_note(&labels) {
        body = body.child(text(note).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().mt(8.));
    }
    let off = telemetry_off(s);
    body.child(rune::el().h(16.)).child(
        row()
            .w_full()
            .gap(24.)
            .center_y()
            .child(
                text(privacy_copy(off.as_deref()))
                    .font(theme::TEXT_XS)
                    .color(t.c.muted_foreground)
                    .wrap()
                    .grow(1.)
                    .shrink()
                    .min_w(0.)
                    .role("link")
                    .cursor(rune::Cursor::Pointer)
                    .on_press_tx(|_: &mut App, tx: &mut rune::Tx| {
                        let _ = tx.open_url(PRIVACY_POLICY_URL);
                    }),
            )
            .child(continue_button(t, "Continue", !ready || crate::env::pairing(s), |s: &mut App| {
                let (ready, setup, _) = setup_of(s);
                if ready && !crate::env::pairing(s) {
                    s.ui.wizard.setup = setup;
                    go(s, Step::Agents);
                }
            })),
    )
}

fn submit_link(s: &mut App) {
    let link = s.ui.wizard.link.text.clone();
    if link.trim().is_empty() || crate::env::pairing(s) {
        return;
    }
    s.ui.wizard.pairing_failed = false;
    s.ui.wizard.paired_here = true;
    crate::env::pair_link(s, &link);
    if !crate::env::pairing(s) {
        // The link didn't parse; its error shows under the field.
        s.ui.wizard.paired_here = false;
        s.ui.wizard.pairing_failed = true;
    }
}

/// Web's onboarding `PairingForm`: one pairing link, Pair, and how to get a link.
fn pairing_form(s: &App, t: &T) -> El {
    let ui = &s.ui.wizard;
    let pairing = crate::env::pairing(s);
    let error = s.ui.env.error.as_deref().filter(|_| ui.pairing_failed && !pairing);
    let mut form = col()
        .key("wizard-pairing")
        .w_full()
        .px(12.)
        .pb(12.)
        .gap(12.)
        .child(
            col().w_full().gap(8.).child(text("Pairing link").font(theme::TEXT_SM).color(t.c.muted_foreground)).child(
                kit::Input::new(&ui.link)
                    .size("lg")
                    .placeholder("https://your-server:5230/pair#token=…")
                    .invalid(error.is_some())
                    .disabled(pairing)
                    .on_edit(|s: &mut App| &mut s.ui.wizard.link)
                    .on_submit(|s: &mut App, _| submit_link(s))
                    .build(t)
                    .single_line(),
            ),
        );
    if let Some(error) = error {
        form = form.child(
            row()
                .w_full()
                .px(12.)
                .py(10.)
                .radius(8.)
                .border(1., t.c.destructive.alpha_mul(0.32))
                .fill(t.c.destructive.alpha_mul(0.04))
                .child(text(error.to_owned()).font(theme::TEXT_SM).color(t.c.destructive_foreground).wrap().grow(1.).shrink().min_w(0.)),
        );
    }
    let help = ui.pairing_help;
    form = form.child(
        row()
            .w_full()
            .gap(12.)
            .center_y()
            .child(
                row()
                    .gap(4.)
                    .center_y()
                    .role("button")
                    .label("Need a pairing link?")
                    .cursor(rune::Cursor::Pointer)
                    .child(icon(if help { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 14.).color(t.c.muted_foreground))
                    .child(text("Need a pairing link?").font(theme::TEXT_SM).color(t.c.muted_foreground))
                    .on_press(|s: &mut App| s.ui.wizard.pairing_help = !s.ui.wizard.pairing_help),
            )
            .child(rune::el().grow(1.))
            .child(kit::Button::new(if pairing { "Pairing..." } else { "Pair" }).disabled(pairing || ui.link.text.trim().is_empty()).on_press(submit_link).build(t)),
    );
    if help {
        let code = |c: &str| text(c.to_owned()).font(theme::mono(theme::TEXT_SM)).color(t.c.muted_foreground);
        form = form
            .child(
                row()
                    .w_full()
                    .flex_wrap()
                    .child(text("On the computer with your code, start T3 Code or run ").font(theme::TEXT_SM).color(t.c.muted_foreground))
                    .child(code("npx t3 serve"))
                    .child(text(", then run:").font(theme::TEXT_SM).color(t.c.muted_foreground)),
            )
            .child(command_block("npx t3 pair", t))
            .child(
                row()
                    .w_full()
                    .flex_wrap()
                    .child(text("On a tailnet, run ").font(theme::TEXT_XS).color(t.c.muted_foreground))
                    .child(text("npx t3 pair --tailscale").font(theme::mono(theme::TEXT_XS)).color(t.c.muted_foreground))
                    .child(text(" instead.").font(theme::TEXT_XS).color(t.c.muted_foreground)),
            );
    }
    form
}

/// Web `CommandBlock`: the command after a muted `$`, with a copy button.
pub(crate) fn command_block(command: &'static str, t: &T) -> El {
    row()
        .w_full()
        .px(12.)
        .py(10.)
        .gap(12.)
        .center_y()
        .radius(8.)
        .border(1., t.c.border)
        .fill(t.c.muted.alpha_mul(0.5))
        .child(
            row()
                .grow(1.)
                .shrink()
                .min_w(0.)
                .child(text("$ ").font(theme::mono(theme::TEXT_SM)).color(t.c.muted_foreground))
                .child(text(command).font(theme::mono(theme::TEXT_SM)).color(t.c.foreground).truncate()),
        )
        .child(
            row()
                .size(24., 24.)
                .center()
                .radius(6.)
                .hover(t.c.accent)
                .role("button")
                .label("Copy command")
                .help("Copy command")
                .cursor(rune::Cursor::Pointer)
                .child(icon(&icons::COPY, 14.).color(t.c.muted_foreground))
                .on_press_tx(move |_: &mut App, tx: &mut rune::Tx| tx.copy(command.to_owned())),
        )
}

/// `ConnectedAgentsStep`: one computer's agents.
fn agent_section(s: &App, env: &str, t: &T) -> El {
    let label = Some(crate::env::label(s, env)).filter(|l| !l.is_empty()).unwrap_or_else(|| "Computer".into());
    let mut list = col().w_full().gap(6.).child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground).mb(2.));
    let can_operate = can_operate_terminal(s, env);
    if !can_operate {
        list = list.child(text("This connection cannot control terminals.").font(theme::TEXT_SM).color(t.c.muted_foreground).mb(2.));
    }
    let open = s.ui.wizard.terminal.as_ref().filter(|setup| setup.env == env).map(|setup| setup.driver);
    let policy = config(s, env).map_or(&Value::Null, |c| &c["managedPolicy"]);
    for (driver, name, p) in agent_cards(providers_on(s, env), policy) {
        let state = provider_state(p);
        let display = p.map(|p| data::s(p, "displayName")).filter(|n| !n.is_empty()).unwrap_or(name).to_owned();
        let status: El = match state {
            "ready" => row().gap(6.).center_y().child(icon(&icons::CHECK, 14.).color(t.c.success_foreground)).child(text("Ready").font(theme::XS_MEDIUM).color(t.c.success_foreground)),
            "checking" => text("Checking...").font(theme::TEXT_XS).color(t.c.muted_foreground),
            "disabled" => text("Disabled").font(theme::TEXT_XS).color(t.c.muted_foreground),
            "attention" => text(summary(p).0).font(theme::TEXT_XS).color(t.c.muted_foreground),
            _ => {
                let (env, instance) = (env.to_owned(), p.map(|p| data::s(p, "instanceId").to_owned()).unwrap_or_default());
                kit::Button::new(if state == "signIn" { "Sign in" } else { "Install" })
                    .icon(&icons::TERMINAL)
                    .variant("ghost")
                    .size("xs")
                    .disabled(open == Some(driver) || config(s, &env).is_none() || !can_operate)
                    .on_press(move |s: &mut App| open_setup(s, &env, driver, &instance))
                    .build(t)
            }
        };
        list = list.child(
            card(t)
                .px(16.)
                .py(16.)
                .child(crate::chat::composer::provider_icon(t, driver, 20.))
                .child(
                    col()
                        .grow(1.)
                        .shrink()
                        .min_w(0.)
                        .gap(2.)
                        .child(text(display).font(theme::SM_MEDIUM).color(t.c.foreground))
                        .child(text(if state == "ready" { "Ready to code.".to_owned() } else { summary_line(p) }).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()),
                )
                .child(status),
        );
    }
    if let Some(setup) = s.ui.wizard.terminal.as_ref().filter(|setup| setup.env == env) {
        list = list.child(setup_terminal(s, setup, t));
    }
    if providers_on(s, env).iter().any(|p| data::s(p, "driver") == "codex" && provider_state(Some(p)) == "ready") {
        let env = env.to_owned();
        list = list.child(
            row().mt(4.).child(
                kit::Button::new("Connect another ChatGPT account")
                    .variant("ghost-muted")
                    .size("xs")
                    .on_press(move |s: &mut App| another_chatgpt_account(s, &env))
                    .build(t),
            ),
        );
    }
    list
}

/// Web `AddManagedCodexAccountDialog`: Settings' "Add ChatGPT account" dialog, on the computer
/// whose agents these are (the dialog works on the active environment).
fn another_chatgpt_account(s: &mut App, env: &str) {
    if s.ui.env.active != env {
        crate::env::switch(s, env);
    }
    crate::settings::add_provider::open_chatgpt(s);
}

fn agents(s: &App, t: &T) -> El {
    let mut sections = col().w_full().gap(20.);
    for env in &s.ui.wizard.setup {
        sections = sections.child(agent_section(s, env, t));
    }
    col()
        .w_full()
        .gap(20.)
        .child(heading("Connect your agents", Some("Choose an agent to start coding. You can add more later."), t))
        .child(kit::scroll_area("wizard-agents", sections.scroll(rune::ScrollSpec { stick_bottom: false }), t).max_h(512.))
        .child(row().w_full().child(rune::el().grow(1.)).child(continue_button(t, "Continue", false, |s: &mut App| go(s, Step::Projects))))
}

/// One computer's part of the Projects step: its scan's state, then its candidates.
fn scan_section(s: &App, found: &Scan, legend: bool, t: &T) -> El {
    let ui = &s.ui.wizard;
    let mut list = col().w_full().gap(2.);
    if legend {
        let label = Some(crate::env::label(s, &found.env)).filter(|l| !l.is_empty()).unwrap_or_else(|| "Computer".into());
        list = list.child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground).mb(6.));
    }
    let candidates = found.candidates.as_deref().unwrap_or_default();
    if found.loading() {
        list = list.child(
            row()
                .w_full()
                .gap(8.)
                .py(12.)
                .center_y()
                .child(kit::Spinner::new().size("md").build(&kit::theme(t)))
                .child(text("Looking for projects…").font(theme::TEXT_SM).color(t.c.muted_foreground)),
        );
    } else if let Some(error) = &found.error {
        let env = found.env.clone();
        list = list.child(
            row()
                .w_full()
                .gap(12.)
                .center_y()
                .child(text(format!("Could not check projects. {error}")).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap().grow(1.).shrink().min_w(0.))
                .child(kit::Button::new("Retry").variant("ghost").size("sm").on_press(move |s: &mut App| scan(s, &env)).build(t)),
        );
    } else if candidates.is_empty() {
        list = list.child(text("No existing Claude Code or Codex projects found.").font(theme::TEXT_SM).color(t.c.muted_foreground).py(8.));
    }
    if found.truncated {
        list = list.child(text(SCAN_LIMIT_MESSAGE).font(theme::TEXT_XS).color(t.c.muted_foreground));
    }
    list.child(import_list(&found.env, candidates, ui, t))
}

fn projects(s: &App, t: &T) -> El {
    let ui = &s.ui.wizard;
    let importing = ui.import.is_some();
    let skip = kit::Button::new("Do not import projects").variant("ghost-muted").disabled(importing).on_press(|s: &mut App| finish(s, None)).build(t);
    if ui.scans.iter().all(Scan::loading) {
        return col()
            .w_full()
            .min_h(160.)
            .child(heading("Your projects", None, t))
            .child(
                col()
                    .grow(1.)
                    .center()
                    .gap(12.)
                    .py(24.)
                    .child(kit::Spinner::new().size("lg").tone("muted").build(&kit::theme(t)))
                    .child(text("Looking for projects from Claude Code and Codex…").font(theme::TEXT_SM).color(t.c.muted_foreground)),
            )
            .child(row().w_full().child(rune::el().grow(1.)).child(skip));
    }
    let mut body = col().w_full().gap(12.).child(heading("Import your projects", Some("Import projects and conversations from Claude Code and Codex on your selected computers."), t));
    let total = ui.scans.iter().map(|scan| scan.candidates.as_ref().map_or(0, Vec::len)).sum::<usize>();
    let chosen = selected_candidates(ui).count();
    if total > 0 {
        body = body.child(
            row()
                .w_full()
                .gap(4.)
                .center_y()
                .child(text(format!("{chosen} of {total} selected")).font(theme::TEXT_XS).color(t.c.muted_foreground).grow(1.))
                .child(
                    kit::Button::new("Select all")
                        .variant("ghost")
                        .size("xs")
                        .disabled(importing || chosen == total)
                        .on_press(|s: &mut App| {
                            let keys: Vec<String> = s.ui.wizard.scans.iter().flat_map(|scan| scan.candidates.iter().flatten()).map(|c| data::s(c, KEY).to_owned()).collect();
                            set_selected(s, &keys, true);
                        })
                        .build(t),
                )
                .child(
                    kit::Button::new("Select none")
                        .variant("ghost")
                        .size("xs")
                        .disabled(importing || chosen == 0)
                        .on_press(|s: &mut App| {
                            if s.ui.wizard.import.is_none() {
                                s.ui.wizard.selected.clear();
                            }
                        })
                        .build(t),
                ),
        );
    }
    let mut list = col().w_full().gap(20.);
    for scan in &ui.scans {
        list = list.child(scan_section(s, scan, ui.scans.len() > 1, t));
    }
    body = body.child(kit::scroll_area("wizard-import-list", list.scroll(rune::ScrollSpec { stick_bottom: false }), t).max_h(320.));
    let label = if importing { "Importing…".into() } else { format!("Import {chosen} {}", if chosen == 1 { "project" } else { "projects" }) };
    let can_import = can_import(s);
    if !can_import {
        body = body.child(text(IMPORT_PERMISSION_MESSAGE).font(theme::TEXT_SM).color(t.c.destructive).wrap());
    }
    body.child(rune::el().h(8.)).child(row().w_full().gap(8.).child(rune::el().grow(1.)).child(skip).child(continue_button(t, &label, !can_import || importing || chosen == 0, start_import)))
}

pub fn overlays(s: &App, t: &T, layer: &mut Vec<El>) {
    let ui = &s.ui.wizard;
    if !ui.open {
        return;
    }
    let current = match ui.step {
        Step::Connect => 0,
        Step::Agents => 1,
        Step::Projects => 2,
    };
    let importing = ui.import.is_some();
    let mut steps = row().gap(6.).center_y();
    for (i, name) in STAGES.iter().enumerate() {
        if i > 0 {
            steps = steps.child(rune::el().w(16.).h(1.).fill(t.c.border));
        }
        let tone = if i == current { t.c.foreground } else { t.c.muted_foreground };
        let mut chip = row()
            .gap(6.)
            .center_y()
            .child(row().size(18., 18.).center().radius(9.).fill(if i <= current { t.c.primary } else { t.c.muted }).child(text((i + 1).to_string()).font(theme::XS_MEDIUM).color(if i <= current { t.c.primary_foreground } else { t.c.muted_foreground })))
            .child(text(name.to_string()).font(theme::XS_MEDIUM).color(tone));
        if i < current && !importing {
            chip = chip.role("button").label(*name).cursor(rune::Cursor::Pointer).on_press(move |s: &mut App| go(s, if i == 0 { Step::Connect } else { Step::Agents }));
        }
        steps = steps.child(chip);
    }
    let content = match ui.step {
        Step::Connect => connect(s, t),
        Step::Agents => agents(s, t),
        Step::Projects => projects(s, t),
    };
    kit::Dialog::new("welcome-wizard", "Set up T3 Code")
        .width(640.)
        .button_only()
        .on_close(close)
        .child(steps)
        .child(rune::el().w_full().h(1.).fill(t.c.border.alpha_mul(0.8)))
        .child(content)
        .open(true)
        .build(t, layer);
    // Settings mounts the same dialog when it is the screen.
    if !matches!(s.route, crate::Route::Settings(_)) {
        crate::settings::add_provider::chatgpt_dialog(s, t, layer);
    }
}

/// Headless scene: Add a computer open with its pairing help.
pub(crate) fn scene_pairing(s: &mut App) {
    let ui = &mut s.ui.wizard;
    ui.open = true;
    ui.pairing_open = true;
    ui.pairing_help = true;
}

/// Headless scene: the Agents step with Claude's sign-in terminal open and ready.
pub(crate) fn scene_setup_terminal(s: &mut App) {
    crate::scene_clock(s);
    let env = s.ui.env.active.clone();
    let (id, command) = ("onboarding-claudeAgent-scene".to_owned(), "claude auth login".to_owned());
    let snapshot = json!({"type": "snapshot", "snapshot": {"cols": 80, "rows": 12, "history": format!("~ $ {command}"), "status": "running", "label": "", "sequence": 1}});
    crate::panel::terminal::fixture(s, SETUP_THREAD, &id, &snapshot.to_string());
    let ui = &mut s.ui.wizard;
    ui.setup = vec![env.clone()];
    ui.step = Step::Agents;
    ui.open = true;
    ui.terminal = Some(Setup { env, driver: "claudeAgent", instance: "claudeAgent".into(), id, cwd: "/home/me".into(), command, back: None });
}

/// Headless scene: the Agents step with "Connect another ChatGPT account" pressed.
pub(crate) fn scene_chatgpt(s: &mut App) {
    let env = s.ui.env.active.clone();
    let ui = &mut s.ui.wizard;
    ui.setup = vec![env.clone()];
    ui.step = Step::Agents;
    ui.open = true;
    another_chatgpt_account(s, &env);
}

/// Headless scene: every listed computer ticked, so those not connected are named as skipped.
pub(crate) fn scene_computers(s: &mut App) {
    for env in crate::env::keys(s) {
        s.ui.wizard.seen.insert(env.clone());
        s.ui.wizard.chosen.insert(env);
    }
    s.ui.wizard.open = true;
}

/// Headless scene: the T3 Connect card with one account computer added and ticked, and one
/// unticked while its relay is offline.
pub(crate) fn scene_t3_connect(s: &mut App) {
    crate::cloud::scene_signed_in(s);
    s.ui.env.saved.push(crate::env::Saved { id: "env-box".into(), label: "Hetzner box".into(), base: "https://box.tunnel.example.com".into(), bearer: crate::env::T3_CONNECT.into() });
    let mut state = crate::data::empty_state();
    state.conn = s5_net::Conn::Open;
    state.shell_ready = true;
    crate::env::park(s, "env-box", crate::data::Source::Fixture(state));
    s.ui.wizard.seen.extend(["env-box".to_owned(), "env-studio".to_owned()]);
    s.ui.wizard.chosen.insert("env-box".into());
}

/// Headless scene: the Projects step on a computer whose connection may read but not operate.
pub(crate) fn scene_import_denied(s: &mut App) {
    crate::scene_clock(s);
    let env = s.ui.env.active.clone();
    let candidates = tag_candidates(
        &env,
        vec![
            json!({"path": "/Users/me/code/app", "title": "app", "git": {"remoteKey": "github.com/me/app", "repository": "me/app"}, "sources": ["claudeAgent", "codex"], "threadCount": 12, "lastActiveAt": "2026-11-08T10:00:00.000Z"}),
            json!({"path": "/Users/me/notes", "title": "notes", "git": null, "sources": ["codex"], "threadCount": 2, "lastActiveAt": "2026-10-01T10:00:00.000Z"}),
        ],
    );
    let ui = &mut s.ui.wizard;
    ui.selected.insert(data::s(&candidates[0], KEY).to_owned());
    ui.scans = vec![Scan { env: env.clone(), pending: None, candidates: Some(candidates), truncated: false, error: None }];
    ui.setup = vec![env.clone()];
    ui.step = Step::Projects;
    ui.open = true;
    crate::settings::permissions::set_session(s, &env, json!({"authenticated": true, "scopes": ["orchestration:read"]}));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_provider_states_match_the_web() {
        assert_eq!(provider_state(None), "checking");
        let p = |v: Value| provider_state(Some(&v));
        assert_eq!(p(json!({"enabled": false, "status": "ready"})), "disabled");
        assert_eq!(p(json!({"enabled": true, "installed": false, "status": "warning", "auth": {"status": "unknown"}})), "checking");
        assert_eq!(p(json!({"enabled": true, "installed": false, "status": "error", "auth": {"status": "unknown"}})), "install");
        assert_eq!(p(json!({"enabled": true, "installed": true, "status": "ready", "auth": {"status": "unauthenticated"}})), "signIn");
        assert_eq!(p(json!({"enabled": true, "installed": true, "status": "ready", "auth": {"status": "authenticated"}})), "ready");
        assert_eq!(p(json!({"enabled": true, "installed": true, "status": "warning", "auth": {"status": "authenticated"}})), "attention");
        assert_eq!(summary_line(Some(&json!({"installed": false}))), "Not found · CLI not detected on PATH.");
    }

    #[test]
    fn agent_cards_read_like_the_web_summary() {
        assert_eq!(summary_line(None), "Checking provider status · Waiting for the server to report installation and authentication details.");
        let line = |v: Value| summary_line(Some(&v));
        assert_eq!(line(json!({"enabled": false, "installed": true})), "Disabled · This provider is installed but disabled for new sessions in T3 Code.");
        assert_eq!(line(json!({"enabled": true, "installed": true, "status": "ready", "auth": {"status": "unauthenticated", "type": "apiKey"}})), "Not authenticated · apiKey");
        assert_eq!(line(json!({"enabled": true, "installed": true, "status": "ready", "auth": {"status": "unauthenticated", "label": ""}})), "Not authenticated");
        let failing = json!({"enabled": true, "installed": true, "status": "error", "auth": {"status": "authenticated"}, "message": "Exited with 1"});
        assert_eq!(provider_state(Some(&failing)), "attention");
        assert_eq!(summary(Some(&failing)).0, "Unavailable", "the badge names the trouble, not a generic label");
        assert_eq!(summary_line(Some(&failing)), "Unavailable · Exited with 1");

        let codex = |id: &str, status: &str| json!({"instanceId": id, "driver": "codex", "enabled": true, "installed": true, "status": status, "auth": {"status": "authenticated"}});
        let claude = |id: &str, status: &str| json!({"instanceId": id, "driver": "claudeAgent", "enabled": true, "installed": true, "status": status, "auth": {"status": "authenticated"}});
        let providers = [codex("codex", "ready"), codex("codex_work", "warning"), claude("claude_a", "warning"), claude("claude_b", "ready"), claude("claude_c", "ready")];
        fn ids(cards: Vec<(&str, &str, Option<&Value>)>) -> Vec<String> {
            cards.into_iter().map(|(driver, _, p)| p.map_or(driver.to_owned(), |p| data::s(p, "instanceId").to_owned())).collect()
        }
        assert_eq!(ids(agent_cards(&providers, &Value::Null)), ["codex", "codex_work", "claude_b"], "every Codex account, and the first ready Claude");
        assert_eq!(ids(agent_cards(&[], &Value::Null)), ["codex", "claudeAgent"], "placeholders while the server reports");
        let policy = json!({"limits": {"providers": {"allowedDrivers": ["claudeAgent"]}}});
        assert_eq!(ids(agent_cards(&providers, &policy)), ["claude_b"]);
    }

    #[test]
    fn setup_takes_connected_computers_and_names_the_ones_it_skips() {
        let envs = [("".to_owned(), Phase::Connected), ("box".to_owned(), Phase::Other), ("lab".to_owned(), Phase::Connecting)];
        fn chosen(keys: &[&str]) -> HashSet<String> {
            keys.iter().map(|k| (*k).to_owned()).collect()
        }
        assert_eq!(resolve_setup(&envs, &chosen(&["", "box"])), (true, vec!["".to_owned()], vec!["box".to_owned()]));
        assert_eq!(resolve_setup(&envs, &chosen(&["", "lab"])), (false, vec!["".to_owned()], Vec::<String>::new()), "a first connection is waited for");
        assert_eq!(resolve_setup(&envs, &chosen(&["box"])), (false, Vec::<String>::new(), vec!["box".to_owned()]));
        assert_eq!(skipped_note(&[]), None);
        assert_eq!(skipped_note(&["Build box".into()]).unwrap(), "Not connected, so setup skips Build box. You can set it up later from Settings.");
        assert_eq!(skipped_note(&["Build box".into(), "Lab".into()]).unwrap(), "Not connected, so setup skips Build box, Lab. You can set them up later from Settings.");
        assert_eq!(privacy_copy(Some("Acme turned off anonymous usage data.")), "Acme turned off anonymous usage data. To read more about how your data is used, see our privacy policy.");
    }

    #[test]
    fn import_results_read_like_the_web() {
        assert_eq!(import_warning(3, 0, 0), None);
        assert_eq!(import_warning(3, 2, 0).unwrap(), "Imported 3 threads. 2 threads could not be imported.");
        assert_eq!(import_warning(1, 1, 0).unwrap(), "Imported 1 thread. 1 thread could not be imported.");
        assert_eq!(import_warning(0, 1, 0).unwrap(), "1 thread could not be imported.");
        assert_eq!(import_warning(0, 4, 1).unwrap(), "4 threads could not be imported.");
        assert_eq!(import_warning(2, 0, 1).unwrap(), "Imported 2 threads. Some thread history could not be imported.");
        assert_eq!(import_warning(0, 0, 1).unwrap(), "Could not import thread history.");
    }

    #[test]
    fn importing_needs_operate_on_every_computer_with_a_selected_project() {
        let mut s = App::new();
        s.src = crate::data::Source::Fixture(crate::data::empty_state());
        let env = crate::env::LOCAL;
        let candidates = tag_candidates(env, vec![json!({"path": "/w/app", "title": "app"})]);
        s.ui.wizard.scans.push(Scan { env: env.into(), pending: None, candidates: Some(candidates), truncated: false, error: None });
        crate::settings::permissions::set_session(&mut s, env, json!({"authenticated": true, "scopes": ["orchestration:read"]}));
        assert!(can_import(&s), "with nothing selected there is nothing to refuse");
        s.ui.wizard.selected.insert(key(env, "/w/app"));
        assert!(!can_import(&s), "a read-only session shows the permission message");
        start_import(&mut s);
        assert!(s.ui.wizard.import.is_none());
        crate::settings::permissions::set_session(&mut s, env, json!({"authenticated": true, "scopes": ["orchestration:operate"]}));
        assert!(can_import(&s));
    }

    #[test]
    fn setup_commands_follow_the_web() {
        let linux = json!({"environment": {"platform": {"os": "linux"}}, "settings": {"providerInstances": {}}});
        let claude = json!({"driver": "claudeAgent", "instanceId": "claudeAgent", "installed": true});
        assert_eq!(setup_command(&claude, &linux), "claude auth login");
        assert_eq!(setup_command(&json!({"driver": "codex", "instanceId": "codex", "installed": false}), &linux), "curl -fsSL https://chatgpt.com/codex/install.sh | sh");
        let windows = json!({"environment": {"platform": {"os": "windows"}}});
        assert_eq!(setup_command(&json!({"driver": "claudeAgent", "installed": false}), &windows), "irm https://claude.ai/install.ps1 | iex");
        let custom = json!({"environment": {"platform": {"os": "darwin"}}, "settings": {"providerInstances": {"work": {"config": {"binaryPath": "~/My Tools/codex"}}}}});
        assert_eq!(setup_command(&json!({"driver": "codex", "instanceId": "work", "installed": true}), &custom), "~/'My Tools/codex' login");
        // `quoteProviderBinary`.
        assert_eq!(quote_binary("/usr/bin/claude", "claude", "linux"), "/usr/bin/claude");
        assert_eq!(quote_binary("it's", "claude", "linux"), "'it'\"'\"'s'");
        assert_eq!(quote_binary("C:\\Program Files\\codex.exe", "codex", "windows"), "& 'C:\\Program Files\\codex.exe'");
        assert_eq!(quote_binary("C:\\tools\\codex.exe", "codex", "windows"), "C:\\tools\\codex.exe");
        assert_eq!(quote_binary("a b", "codex", "freebsd"), "codex");
    }

    #[test]
    fn the_setup_terminal_opens_on_its_computer_and_puts_focus_back() {
        let mut s = App::new();
        s.src = crate::data::Source::Fixture(crate::data::empty_state());
        let mut remote = crate::data::empty_state();
        remote.config = Some(json!({"cwd": "/srv", "environment": {"platform": {"os": "linux"}}, "providers": [{"driver": "codex", "instanceId": "codex", "installed": true}]}));
        s.ui.env.saved.push(crate::env::Saved { id: "box".into(), label: "Box".into(), base: "https://box.example".into(), bearer: "t".into() });
        crate::env::park(&mut s, "box", crate::data::Source::Fixture(remote));
        s.ui.wizard.open = true;

        open_setup(&mut s, "box", "codex", "codex");
        assert_eq!(s.ui.env.active, "box", "FAILED: the terminal runs on the computer whose agent it sets up");
        let setup = s.ui.wizard.terminal.as_ref().expect("a setup terminal");
        assert_eq!((setup.command.as_str(), setup.cwd.as_str(), setup.back.as_deref()), ("codex login", "/srv", Some(crate::env::LOCAL)));
        assert_eq!(setup_status(&s, setup), ("Preparing command...".to_owned(), false));
        assert!(crate::panel::terminal::session(&s, SETUP_THREAD, &setup.id).is_some());

        close_setup(&mut s);
        assert_eq!(s.ui.env.active, crate::env::LOCAL, "FAILED: closing puts back the computer that was in focus");
        assert!(s.ui.wizard.terminal.is_none());

        crate::settings::permissions::set_session(&mut s, "box", json!({"authenticated": true, "permissions": ["orchestration:read"]}));
        open_setup(&mut s, "box", "codex", "codex");
        assert!(s.ui.wizard.terminal.is_none() && s.ui.env.active == crate::env::LOCAL, "FAILED: a connection that cannot control terminals opens none");
    }

    #[test]
    fn only_the_servers_own_untouched_project_counts_as_fresh() {
        let mut st = data::empty_state();
        st.config = Some(json!({"cwd": "/home/me/app"}));
        assert!(fresh(&st));
        st.projects = vec![json!({"id": "p", "workspaceRoot": "/home/me/app/"})];
        st.threads = vec![json!({"id": "t", "projectId": "p", "latestUserMessageAt": null})];
        assert!(fresh(&st), "the server's cwd project and its bootstrap thread");
        st.threads[0]["latestUserMessageAt"] = json!("2026-11-01T00:00:00.000Z");
        assert!(!fresh(&st), "a thread someone wrote in");
        st.threads.clear();
        st.projects[0]["workspaceRoot"] = json!("/home/me/other");
        assert!(!fresh(&st), "a project the person added");
    }

    #[test]
    fn import_candidates_group_by_repository_and_recent_ones_start_selected() {
        let now = data::iso_ms("2026-11-09T12:00:00.000Z").unwrap();
        let c = |path: &str, git: Option<Value>, threads: u64, at: Option<&str>| {
            let mut v = json!({"path": path, "title": path.rsplit('/').next(), "sources": ["codex"], "threadCount": threads, "lastActiveAt": at, "alreadyImported": false});
            if let Some(git) = git {
                v["git"] = git;
            }
            v
        };
        let repo = |name: &str| json!({"remoteKey": format!("github.com/{name}"), "repository": name});
        let candidates = [
            c("/w/app", Some(repo("me/app")), 5, Some("2026-11-01T00:00:00.000Z")),
            c("/w/app-wt", Some(repo("me/app")), 1, Some("2026-11-08T00:00:00.000Z")),
            c("/w/old", Some(repo("me/old")), 9, Some("2026-08-01T00:00:00.000Z")),
            c("/w/legacy", None, 3, Some("2026-11-05T00:00:00.000Z")),
            c("/tmp/notes", Some(Value::Null), 7, Some("2026-11-08T00:00:00.000Z")),
        ];
        let picked: Vec<&str> = candidates.iter().filter(|c| recent(c, now)).map(|c| data::s(c, "path")).collect();
        assert_eq!(picked, ["/w/app", "/w/legacy"], "few threads, a stale month, or no git leave a project unselected");

        let (repositories, other) = groups(&candidates);
        let summary: Vec<(&str, &[usize], u64)> = repositories.iter().map(|g| (g.label.as_str(), &g.members[..], g.threads)).collect();
        assert_eq!(summary, [("me/app", &[0, 1][..], 6), ("legacy", &[3][..], 3), ("me/old", &[2][..], 9)]);
        assert_eq!(repositories[0].last_active, data::ts(&candidates[1], "lastActiveAt"));
        assert_eq!(other, [4]);
        assert_eq!(split_path("/w/app/"), ("/w/", "app"));
        assert_eq!(split_path("app"), ("", "app"));
        assert_eq!(split_path("C:\\"), ("", "C:\\"));
        assert_eq!(split_path("\\\\server\\share\\"), ("", "\\\\server\\share\\"));
        assert_eq!(split_path("\\\\server\\share\\repo"), ("\\\\server\\share\\", "repo"));
    }

    #[test]
    fn the_wizard_opens_once_on_a_fresh_workspace_and_finishing_records_it() {
        let dir = std::env::temp_dir().join(format!("s5-wizard-{}", std::process::id()));
        // SAFETY: tests that read the settings path set it to their own file first.
        unsafe { std::env::set_var("S5_CLIENT_SETTINGS", dir.join("settings.json")) };
        let mut s = App::new();
        let mut state = data::empty_state();
        state.shell_ready = true;
        s.src = crate::data::Source::Fixture(state);
        s.client_settings.values.insert("onboardingCompletedAt".into(), Value::Null);
        assert!(poll(&mut s) && s.ui.wizard.open);
        close(&mut s);
        assert!(!s.ui.wizard.open && completed(&s));
        assert!(!poll(&mut s) && !s.ui.wizard.open);

        let mut s = App::new();
        let mut state = data::fixture_state();
        state.shell_ready = true;
        s.src = crate::data::Source::Fixture(state);
        s.client_settings.values.insert("onboardingCompletedAt".into(), Value::Null);
        poll(&mut s);
        assert!(!s.ui.wizard.open && completed(&s), "a used workspace skips the wizard and records it");
    }

    #[test]
    fn a_pairing_from_the_wizard_sets_up_the_new_computer() {
        let mut s = App::new();
        s.ui.wizard.open = true;
        s.ui.wizard.decided = true;
        s.ui.wizard.pairing_open = true;
        s.ui.wizard.link = rune::TextEdit::new("https://box.example/pair#token=t");
        s.ui.env.saved.push(crate::env::Saved { id: "box".into(), label: "Box".into(), base: "https://box.example".into(), bearer: "t".into() });
        crate::env::park(&mut s, "box", crate::data::Source::Fixture(crate::data::empty_state()));

        s.ui.env.saved.push(crate::env::Saved { id: "off".into(), label: "Off".into(), base: "https://off.example".into(), bearer: "t".into() });
        s.ui.env.off.push("off".into());

        s.ui.wizard.paired_here = true;
        poll(&mut s);
        assert!(s.ui.wizard.pairing_failed, "a pairing that added nothing shows its error");
        assert!(s.ui.wizard.pairing_open);
        assert!(s.ui.wizard.chosen.contains("box") && !s.ui.wizard.chosen.contains("off"), "listed computers start ticked unless they are off");
        toggle_computer(&mut s, "box");
        poll(&mut s);
        assert!(!s.ui.wizard.chosen.contains("box"), "an unticked computer stays unticked");

        s.ui.wizard.paired_here = true;
        s.ui.env.last_paired = Some("box".into());
        poll(&mut s);
        assert!(s.ui.wizard.chosen.contains("box"), "the computer just paired is ticked");
        assert!(!s.ui.wizard.pairing_open && s.ui.wizard.link.text.is_empty());
    }

    #[test]
    fn account_computers_are_ticked_and_added_once_and_unticking_sticks() {
        // SAFETY: tests that save environments all point here; keeps them off the real config dir.
        unsafe { std::env::set_var("S5_ENVIRONMENTS", std::env::temp_dir().join(format!("s5-env-{}.json", std::process::id()))) };
        let mut s = App::new();
        s.ui.wizard.open = true;
        s.ui.wizard.decided = true;
        crate::cloud::scene_signed_in(&mut s);
        s.ui.cloud.availability.insert("env-studio".into(), crate::cloud::Availability::Checking);
        poll(&mut s);
        let saved = |s: &App, id: &str| s.ui.env.saved.iter().find(|e| e.id == id).map(|e| e.bearer.clone());
        assert!(chosen(&s, "env-box") && saved(&s, "env-box").as_deref() == Some(crate::env::T3_CONNECT), "a computer with a status is ticked and connects through T3 Connect");
        assert!(!chosen(&s, "env-studio") && saved(&s, "env-studio").is_none(), "one still being checked waits");

        let studio = crate::cloud::linked(&s, "env-studio").unwrap();
        choose(&mut s, &studio, true);
        assert_eq!(saved(&s, "env-studio").as_deref(), Some(crate::env::T3_CONNECT), "ticking adds it, so it is connecting by Continue");
        choose(&mut s, &studio, false);
        s.ui.cloud.availability.insert("env-studio".into(), crate::cloud::Availability::Online);
        poll(&mut s);
        assert!(!chosen(&s, "env-studio") && saved(&s, "env-studio").is_some(), "unticking leaves it saved but out of setup, and it stays unticked");
        let (_, _, skipped) = setup_of(&s);
        assert!(skipped.iter().all(|id| id != "env-studio"));
    }
}
