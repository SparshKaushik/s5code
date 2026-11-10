//! Saved environments: other T3 servers this window pairs with (apps/web Settings > Connections >
//! Environments, "Add environment"). Each keeps its own s5_net client and stays connected.
//!
//! Like the web, the sidebar, palette and search list every environment at once. `App::src` is the
//! environment that owns what's on screen (the route's thread or project; this machine otherwise),
//! so the surfaces that read `s.state()` or send through `s.src.net()` reach the right server. The
//! other environments' project and thread rows are copied into its lists, tagged with their owner
//! ([`TAG`]); thread views stay with their own server. Request ids are process-unique, and replies
//! from every environment are handed to `App::src`, where the UI polls for them.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use rune::{El, TextEdit, col, icon, row, text};
use serde_json::{Value, json};

use crate::data::Source;
use crate::theme::{self, T};
use crate::{App, Route, icons, kit};

/// The key of this machine's own server in `active` and `parked`.
pub const LOCAL: &str = "";
/// Marks a row copied in from another environment, holding that environment's key.
const TAG: &str = "_environment";

#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    pub id: String,
    pub label: String,
    pub base: String,
    pub bearer: String,
}

#[derive(Default)]
pub struct Ui {
    pub saved: Vec<Saved>,
    /// Connected environments that are not on screen, by key.
    parked: Vec<(String, Source)>,
    /// The environment `App::src` belongs to.
    pub active: String,
    pub host: TextEdit,
    pub code: TextEdit,
    pairing: Option<Receiver<Result<Saved, String>>>,
    pub error: Option<String>,
    /// Each parked environment's `shell_revision` when its rows were last copied in.
    merged: HashMap<String, u64>,
    /// Rows copied into `App::src`, and which environment owns each copied id.
    foreign: usize,
    owners: HashMap<String, String>,
}

fn path() -> PathBuf {
    match std::env::var_os("S5_ENVIRONMENTS") {
        Some(path) => path.into(),
        None => crate::settings::client::config_path().with_file_name("environments.json"),
    }
}

fn decode(value: &Value) -> Vec<Saved> {
    let field = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_owned();
    value["environments"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| Saved { id: field(v, "environmentId"), label: field(v, "label"), base: field(v, "httpBaseUrl"), bearer: field(v, "bearerToken") })
        .filter(|e| !e.id.is_empty() && !e.base.is_empty() && !e.bearer.is_empty())
        .collect()
}

fn encode(saved: &[Saved]) -> Value {
    let rows: Vec<Value> = saved.iter().map(|e| json!({"environmentId": e.id, "label": e.label, "httpBaseUrl": e.base, "bearerToken": e.bearer})).collect();
    json!({ "version": 1, "environments": rows })
}

/// Bearer tokens are credentials: the file is the user's alone.
fn save(saved: &[Saved]) -> Result<(), String> {
    let path = path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let temporary = path.with_extension("json.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut file, serde_json::to_string_pretty(&encode(saved)).unwrap_or_default().as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, &path).map_err(|e| e.to_string())
}

fn connect(s: &App, env: &Saved) -> Source {
    let waker = s.ui.panel.waker.clone();
    Source::Live(s5_net::Net::connect(&env.base, &env.bearer, move || {
        if let Some(w) = &waker {
            w()
        }
    }))
}

/// Loads the saved environments and connects each in the background; this machine stays active.
pub fn start(s: &mut App) {
    let saved = std::fs::read_to_string(path()).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()).map(|v| decode(&v)).unwrap_or_default();
    for env in &saved {
        let src = connect(s, env);
        s.ui.env.parked.push((env.id.clone(), src));
    }
    s.ui.env.saved = saved;
}

/// This machine's server, wherever it is: `App::src` while active, else parked.
pub fn local_mut(s: &mut App) -> &mut Source {
    if s.ui.env.active == LOCAL {
        return &mut s.src;
    }
    let ui = &mut s.ui.env;
    let i = match ui.parked.iter().position(|(k, _)| k == LOCAL) {
        Some(i) => i,
        None => {
            ui.parked.push((LOCAL.into(), Source::Fixture(crate::data::empty_state())));
            ui.parked.len() - 1
        }
    };
    &mut ui.parked[i].1
}

fn source<'a>(s: &'a App, key: &str) -> Option<&'a Source> {
    if s.ui.env.active == key {
        return Some(&s.src);
    }
    s.ui.env.parked.iter().find(|(k, _)| k == key).map(|(_, src)| src)
}

fn local_id(s: &App) -> Option<String> {
    source(s, LOCAL)?.state().config.as_ref()?["environment"]["environmentId"].as_str().map(str::to_owned)
}

pub fn label(s: &App, key: &str) -> String {
    if key == LOCAL {
        let label = source(s, LOCAL).and_then(|src| src.state().config.as_ref()).and_then(|c| c["environment"]["label"].as_str()).unwrap_or("This machine");
        return label.to_owned();
    }
    s.ui.env.saved.iter().find(|e| e.id == key).map(|e| e.label.clone()).unwrap_or_default()
}

/// What the row says about an environment's connection.
pub fn status(s: &App, key: &str) -> String {
    let Some(src) = source(s, key) else { return "Not connected".into() };
    let st = src.state();
    match &st.conn {
        s5_net::Conn::Open if st.shell_ready => "Connected".into(),
        s5_net::Conn::Open | s5_net::Conn::Connecting => "Connecting…".into(),
        s5_net::Conn::Closed(reason) => format!("Reconnecting · {reason}"),
    }
}

/// Makes `key`'s environment the one `App::src` talks to; the others' rows are copied back in.
pub fn switch(s: &mut App, key: &str) {
    if s.ui.env.active == key {
        return;
    }
    let Some(i) = s.ui.env.parked.iter().position(|(k, _)| k == key) else { return };
    let (_, next) = s.ui.env.parked.remove(i);
    let mut previous = std::mem::replace(&mut s.src, next);
    strip(state_mut(&mut previous));
    let old = std::mem::replace(&mut s.ui.env.active, key.to_owned());
    s.ui.env.parked.push((old, previous));
    s.ui.env.merged.clear();
    merge(s);
}

/// The environment that owns `route`: its thread's or project's, else this machine's.
pub fn follow(s: &mut App, route: &Route) {
    let key = match route {
        Route::Thread(id) | Route::Home { project_id: Some(id) } => owner(s, id),
        Route::Home { project_id: None } => crate::chat::home::default_project(s).map_or_else(|| LOCAL.to_owned(), |p| of(s, p)),
        _ => LOCAL.to_owned(),
    };
    switch(s, &key);
}

/// Which environment a thread or project id belongs to.
pub fn owner(s: &App, id: &str) -> String {
    s.ui.env.owners.get(id).cloned().unwrap_or_else(|| s.ui.env.active.clone())
}

/// The environment a sidebar row came from: its tag, or the one on screen.
pub fn of(s: &App, row: &Value) -> String {
    row[TAG].as_str().map_or_else(|| s.ui.env.active.clone(), str::to_owned)
}

/// Whether rows from more than one environment are listed (the web's `projectGroupsSpanEnvironments`).
pub fn spans(s: &App) -> bool {
    s.ui.env.foreign > 0
}

/// The web's command palette location: "Local" for this machine, else the environment's label.
pub fn location(s: &App, row: &Value) -> String {
    match of(s, row) {
        key if key == LOCAL => "Local".into(),
        key => label(s, &key),
    }
}

/// The environment a row lives on, when that's not this machine.
pub fn remote_label(s: &App, row: &Value) -> Option<String> {
    let key = of(s, row);
    (key != LOCAL).then(|| label(s, &key))
}

/// The client to send a command about `id` through; an id no environment lists goes to `App::src`.
pub fn net_for<'a>(s: &'a mut App, id: &str) -> Option<&'a mut s5_net::Net> {
    let key = owner(s, id);
    if key == s.ui.env.active {
        return s.src.net();
    }
    s.ui.env.parked.iter_mut().find(|(k, _)| *k == key).and_then(|(_, src)| src.net())
}

fn state_mut(src: &mut Source) -> &mut s5_net::State {
    match src {
        Source::Live(net) => net.state_mut(),
        Source::Fixture(state) => state,
    }
}

fn lists(state: &mut s5_net::State) -> [&mut Vec<Value>; 3] {
    [&mut state.projects, &mut state.threads, &mut state.archived]
}

fn strip(state: &mut s5_net::State) {
    for rows in lists(state) {
        rows.retain(|r| r.get(TAG).is_none());
    }
}

/// Copies the parked environments' rows into `App::src` when any of them moved, or when a shell
/// snapshot on `App::src` replaced its lists and dropped them.
fn merge(s: &mut App) -> bool {
    let env = &mut s.ui.env;
    if env.parked.is_empty() && env.merged.is_empty() {
        return false;
    }
    let moved = env.merged.len() != env.parked.len() || env.parked.iter().any(|(k, src)| env.merged.get(k) != Some(&src.state().shell_revision));
    let present = s.src.state().projects.iter().chain(&s.src.state().threads).chain(&s.src.state().archived).filter(|r| r.get(TAG).is_some()).count();
    if !moved && present == env.foreign {
        return false;
    }
    let state = state_mut(&mut s.src);
    strip(state);
    env.owners.clear();
    env.merged.clear();
    env.foreign = 0;
    for (key, src) in &env.parked {
        let theirs = src.state();
        env.merged.insert(key.clone(), theirs.shell_revision);
        let [projects, threads, archived] = lists(state);
        for (from, into) in [(&theirs.projects, projects), (&theirs.threads, threads), (&theirs.archived, archived)] {
            for row in from {
                let mut row = row.clone();
                row[TAG] = Value::String(key.clone());
                env.owners.insert(crate::data::s(&row, "id").to_owned(), key.clone());
                into.push(row);
                env.foreign += 1;
            }
        }
    }
    true
}

/// Drains every environment, hands their replies to `App::src`, and refreshes the merged lists.
pub fn drain(s: &mut App) -> bool {
    let mut changed = s.src.net().is_some_and(|net| net.drain());
    for (_, src) in &mut s.ui.env.parked {
        if let Some(net) = src.net() {
            changed |= net.drain();
            if let Source::Live(to) = &mut s.src {
                net.hand_off(to);
            }
        }
    }
    merge(s) | changed
}

pub fn remove(s: &mut App, id: &str) {
    if s.ui.env.active == id {
        s.navigate(Route::Home { project_id: None });
        switch(s, LOCAL);
    }
    s.ui.env.parked.retain(|(k, _)| k != id);
    s.ui.env.saved.retain(|e| e.id != id);
    s.ui.env.error = save(&s.ui.env.saved).err().map(|e| format!("Could not save environments: {e}"));
}

/// `preparePairingRegistration`: the descriptor first, so a wrong address never spends the code.
fn pair(host: String, code: String) -> Result<Saved, String> {
    let (base, credential) = s5_net::resolve_pairing(&host, &code)?;
    let descriptor = s5_net::fetch_descriptor(&base)?;
    let bearer = s5_net::exchange_bootstrap(&base, &credential).map_err(|e| format!("The pairing code was refused: {e}"))?;
    let id = descriptor["environmentId"].as_str().unwrap_or_default().to_owned();
    let label = descriptor["label"].as_str().filter(|l| !l.is_empty()).unwrap_or(&base).to_owned();
    Ok(Saved { id, label, base, bearer })
}

fn add(s: &mut App) {
    if s.ui.env.pairing.is_some() {
        return;
    }
    let (host, code) = (s.ui.env.host.text.clone(), s.ui.env.code.text.clone());
    if let Err(e) = s5_net::resolve_pairing(&host, &code) {
        s.ui.env.error = Some(e);
        return;
    }
    let (tx, rx) = channel();
    let waker = s.ui.panel.waker.clone();
    std::thread::spawn(move || {
        let _ = tx.send(pair(host, code));
        if let Some(w) = waker {
            w();
        }
    });
    s.ui.env.error = None;
    s.ui.env.pairing = Some(rx);
}

/// Folds a finished pairing in, replacing an environment saved before (a fresh session).
fn paired(s: &mut App, result: Result<Saved, String>) {
    let env = match result {
        Ok(env) if Some(&env.id) == local_id(s).as_ref() => Err("That address reaches this machine's own server.".to_owned()),
        other => other,
    };
    let env = match env {
        Ok(env) => env,
        Err(e) => {
            s.ui.env.error = Some(e);
            return;
        }
    };
    if s.ui.env.active == env.id {
        s.navigate(Route::Home { project_id: None });
        switch(s, LOCAL);
    }
    s.ui.env.parked.retain(|(k, _)| *k != env.id);
    let src = connect(s, &env);
    s.ui.env.parked.push((env.id.clone(), src));
    s.ui.env.saved.retain(|e| e.id != env.id);
    s.ui.env.saved.push(env);
    s.ui.env.host.clear();
    s.ui.env.code.clear();
    s.ui.env.error = save(&s.ui.env.saved).err().map(|e| format!("Could not save environments: {e}"));
}

/// Lands a pairing that finished.
pub fn poll(s: &mut App) -> bool {
    let Some(result) = s.ui.env.pairing.as_ref().and_then(|rx| rx.try_recv().ok()) else { return false };
    s.ui.env.pairing = None;
    paired(s, result);
    true
}

pub fn keys(s: &App) -> Vec<String> {
    std::iter::once(LOCAL.to_owned()).chain(s.ui.env.saved.iter().map(|e| e.id.clone())).collect()
}

/// Settings > Connections > Environments: this machine, each saved environment, and the pairing form.
pub fn settings_rows(s: &App, t: &T) -> Vec<El> {
    let mut rows = Vec::new();
    for key in keys(s) {
        let mut detail = status(s, &key);
        if key == LOCAL {
            detail = format!("This machine · {detail}");
        } else if let Some(env) = s.ui.env.saved.iter().find(|e| e.id == key) {
            detail = format!("{} · {detail}", env.base);
        }
        let mut actions = row().gap(8.0).center_y();
        if key != LOCAL {
            let target = key.clone();
            actions = actions.child(
                kit::icon_button(&icons::TRASH_2, "Remove environment").variant("ghost").size("icon-xs").on_press(move |s: &mut App| remove(s, &target)).build(t),
            );
        }
        rows.push(
            row()
                .key(format!("env-{key}"))
                .w_full()
                .padding(16.0)
                .gap(12.0)
                .center_y()
                .child(icon(if key == LOCAL { &icons::LAPTOP } else { &icons::SERVER }, 16.0).color(t.c.muted_foreground))
                .child(
                    col()
                        .grow(1.0)
                        .shrink()
                        .min_w(0.0)
                        .gap(2.0)
                        .child(text(label(s, &key)).font(theme::SM_MEDIUM).color(t.c.foreground).truncate())
                        .child(text(detail).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().max_lines(2)),
                )
                .child(actions),
        );
    }
    rows.push(pairing_form(s, t));
    rows
}

/// The web's remote pairing fields: a host (or a whole pairing URL) and a pairing code.
pub fn pairing_form(s: &App, t: &T) -> El {
    let pairing = s.ui.env.pairing.is_some();
    let field = |label: &str, input: El| col().gap(6.0).child(text(label.to_owned()).font(theme::XS_MEDIUM).color(t.c.foreground)).child(input);
    let mut form = col()
        .key("env-add")
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .child(text("Add environment").font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(
            row()
                .w_full()
                .gap(12.0)
                .child(field("Host", kit::Input::new(&s.ui.env.host).placeholder("backend.example.com").on_edit(|s: &mut App| &mut s.ui.env.host).on_submit(|s: &mut App, _| add(s)).build(t).single_line()).grow(1.0).shrink().min_w(0.0))
                .child(
                    field("Pairing code", kit::Input::new(&s.ui.env.code).placeholder("PAIRCODE").on_edit(|s: &mut App| &mut s.ui.env.code).on_submit(|s: &mut App, _| add(s)).build(t).single_line())
                        .w(160.0),
                ),
        )
        .child(text("Paste a full pairing URL here to fill both fields automatically.").font(theme::TEXT_XS).color(t.c.muted_foreground));
    if let Some(error) = &s.ui.env.error {
        form = form.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive_foreground));
    }
    form = form.child(
        row().child(
            kit::Button::new(if pairing { "Adding…" } else { "Add environment" })
                .size("sm")
                .disabled(pairing || s.ui.env.host.text.trim().is_empty())
                .on_press(add)
                .build(t),
        ),
    );
    form
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(id: &str) -> Saved {
        Saved { id: id.into(), label: format!("Box {id}"), base: format!("https://{id}.example"), bearer: "token".into() }
    }

    #[test]
    fn saved_environments_round_trip_and_drop_incomplete_rows() {
        let list = vec![saved("a"), saved("b")];
        assert_eq!(decode(&encode(&list)), list);
        let partial = json!({"environments": [{"environmentId": "c", "label": "C", "httpBaseUrl": "https://c"}]});
        assert!(decode(&partial).is_empty(), "a row without a bearer cannot connect");
    }

    #[test]
    fn every_environment_is_listed_and_commands_follow_the_owner() {
        // SAFETY: only this test reads S5_ENVIRONMENTS; keeps `remove` off the real config dir.
        unsafe { std::env::set_var("S5_ENVIRONMENTS", std::env::temp_dir().join(format!("s5-env-{}.json", std::process::id()))) };
        let mut s = App::new();
        let mut local = crate::data::empty_state();
        local.shell_ready = true;
        local.projects.push(json!({"id": "p-here", "title": "Here"}));
        local.threads.push(json!({"id": "t-here", "projectId": "p-here"}));
        s.src = Source::Fixture(local);
        let mut remote = crate::data::empty_state();
        remote.projects.push(json!({"id": "p-there", "title": "There"}));
        remote.threads.push(json!({"id": "t-there", "projectId": "p-there"}));
        remote.shell_revision = 1;
        s.ui.env.saved.push(saved("a"));
        s.ui.env.parked.push(("a".into(), Source::Fixture(remote)));
        let ids = |s: &App| s.state().threads.iter().map(|t| crate::data::s(t, "id").to_owned()).collect::<Vec<_>>();

        assert!(drain(&mut s));
        assert_eq!(ids(&s), ["t-here", "t-there"]);
        assert_eq!((owner(&s, "t-there"), owner(&s, "p-there"), owner(&s, "t-here")), ("a".into(), "a".into(), LOCAL.into()));
        assert_eq!(remote_label(&s, &s.state().projects[1]), Some("Box a".into()));
        assert!(!drain(&mut s), "nothing moved, so nothing is copied again");

        s.navigate(Route::Thread("t-there".into()));
        assert_eq!(s.ui.env.active, "a", "the thread's own server is the one on screen");
        assert!(!s.state().shell_ready);
        assert_eq!(ids(&s), ["t-there", "t-here"]);
        assert_eq!(owner(&s, "t-here"), LOCAL);
        assert_eq!(location(&s, &s.state().threads[1]), "Local");

        // A shell snapshot replaces the lists and drops the copies; the next drain restores them.
        if let Source::Fixture(state) = &mut s.src {
            state.threads.retain(|t| t.get(TAG).is_none());
        }
        assert!(drain(&mut s));
        assert_eq!(ids(&s), ["t-there", "t-here"]);

        s.navigate(Route::Settings("general".into()));
        assert_eq!(s.ui.env.active, LOCAL);
        assert!(s.state().shell_ready);
        remove(&mut s, "a");
        drain(&mut s);
        assert_eq!(ids(&s), ["t-here"]);
        assert!(s.ui.env.parked.is_empty());
    }
}
