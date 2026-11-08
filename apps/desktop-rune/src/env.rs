//! Saved environments: other T3 servers this window pairs with (apps/web Settings > Connections >
//! Environments, "Add environment"). Each keeps its own s5_net client and stays connected. The
//! active one is `App::src`, so every surface keeps talking to one server and switching swaps it.
//! The web lists every environment's threads at once; that needs per-thread routing wherever
//! `src` is used, so here one environment is on screen at a time.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use rune::{Cursor, El, TextEdit, col, icon, row, text};
use serde_json::{Value, json};

use crate::data::Source;
use crate::theme::{self, T};
use crate::{App, Route, icons, kit};

/// The key of this machine's own server in `active` and `parked`.
pub const LOCAL: &str = "";

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
    pub menu_open: bool,
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

/// Puts `key`'s environment on screen. Routes and requests in flight belong to the old server, so
/// the window returns to a new thread, and Settings stays open over it.
pub fn switch(s: &mut App, key: &str) {
    s.ui.env.menu_open = false;
    if s.ui.env.active == key {
        return;
    }
    let Some(i) = s.ui.env.parked.iter().position(|(k, _)| k == key) else { return };
    let (_, next) = s.ui.env.parked.remove(i);
    let previous = std::mem::replace(&mut s.src, next);
    let old = std::mem::replace(&mut s.ui.env.active, key.to_owned());
    s.ui.env.parked.push((old, previous));
    s.ui.shell.panel_open = false;
    s.ui.search.open = false;
    let home = Route::Home { project_id: None };
    if matches!(s.route, Route::Settings(_) | Route::Usage) {
        s.ui.settings.return_route = Some(home.clone());
        s.ui.pages.main_route = Some(home);
    } else {
        s.navigate(home);
    }
}

pub fn remove(s: &mut App, id: &str) {
    if s.ui.env.active == id {
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

/// Keeps parked environments current and lands a pairing that finished.
pub fn poll(s: &mut App) -> bool {
    for (_, src) in &mut s.ui.env.parked {
        if let Some(net) = src.net() {
            net.drain();
        }
    }
    let Some(result) = s.ui.env.pairing.as_ref().and_then(|rx| rx.try_recv().ok()) else { return false };
    s.ui.env.pairing = None;
    paired(s, result);
    true
}

fn keys(s: &App) -> Vec<String> {
    std::iter::once(LOCAL.to_owned()).chain(s.ui.env.saved.iter().map(|e| e.id.clone())).collect()
}

/// Settings > Connections > Environments: this machine, each saved environment, and the pairing form.
pub fn settings_rows(s: &App, t: &T) -> Vec<El> {
    let mut rows = Vec::new();
    for key in keys(s) {
        let active = s.ui.env.active == key;
        let mut detail = status(s, &key);
        if key == LOCAL {
            detail = format!("This machine · {detail}");
        } else if let Some(env) = s.ui.env.saved.iter().find(|e| e.id == key) {
            detail = format!("{} · {detail}", env.base);
        }
        let mut actions = row().gap(8.0).center_y();
        if active {
            actions = actions.child(text("Active").font(theme::XS_MEDIUM).color(t.c.success_foreground));
        } else {
            let target = key.clone();
            actions = actions.child(kit::Button::new("Switch").variant("outline").size("xs").on_press(move |s: &mut App| switch(s, &target)).build(t));
        }
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
    let pairing = s.ui.env.pairing.is_some();
    let field = |label: &str, input: El| col().grow(1.0).shrink().min_w(0.0).gap(6.0).child(text(label.to_owned()).font(theme::XS_MEDIUM).color(t.c.foreground)).child(input);
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
                .child(field("Host", kit::Input::new(&s.ui.env.host).placeholder("backend.example.com").on_edit(|s: &mut App| &mut s.ui.env.host).on_submit(|s: &mut App, _| add(s)).build(t).single_line()))
                .child(
                    field("Pairing code", kit::Input::new(&s.ui.env.code).placeholder("PAIRCODE").on_edit(|s: &mut App| &mut s.ui.env.code).on_submit(|s: &mut App, _| add(s)).build(t).single_line())
                        .w(160.0)
                        .grow(0.0),
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
    rows.push(form);
    rows
}

/// The sidebar's environment switcher, under the brand row once another environment is saved.
pub fn sidebar_switcher(s: &App, t: &T) -> Option<El> {
    if s.ui.env.saved.is_empty() {
        return None;
    }
    let active = s.ui.env.active.clone();
    let connected = status(s, &active) == "Connected";
    Some(
        row().px(8.0).pb(4.0).child(
            row()
                .key("sb-env")
                .grow(1.0)
                .h(28.0)
                .px(8.0)
                .gap(8.0)
                .radius(theme::radius::MD)
                .center_y()
                .hover(t.sb.sidebar_row_hover)
                .cursor(Cursor::Pointer)
                .role("button")
                .label("Switch environment")
                .help("Switch environment")
                .child(icon(if active == LOCAL { &icons::LAPTOP } else { &icons::SERVER }, 14.0).color(t.sb.sidebar_icon_color))
                .child(text(label(s, &active)).font(theme::TEXT_XS).color(t.sb.sidebar_foreground).truncate().grow(1.0).shrink().min_w(0.0))
                .child(rune::el().size(6.0, 6.0).radius(3.0).fill(if connected { t.c.success_foreground } else { t.c.warning_foreground }))
                .child(icon(&icons::CHEVRONS_UP_DOWN, 12.0).color(t.sb.sidebar_icon_color))
                .on_press(|s: &mut App| s.ui.env.menu_open = !s.ui.env.menu_open),
        ),
    )
}

pub fn sidebar_menu(s: &App, t: &T, overlays: &mut Vec<El>) {
    if !s.ui.env.menu_open || s.ui.env.saved.is_empty() {
        return;
    }
    let mut menu = kit::Menu::new("sb-env-menu")
        .anchor("sb-env")
        .placement(kit::Placement::BottomStart)
        .min_width(224.0)
        .open(true)
        .on_close(|s: &mut App| s.ui.env.menu_open = false);
    for key in keys(s) {
        let target = key.clone();
        menu = menu.item(
            kit::MenuItem::new(label(s, &key))
                .icon(if key == LOCAL { &icons::LAPTOP } else { &icons::SERVER })
                .checked(s.ui.env.active == key)
                .on_select(move |s: &mut App| switch(s, &target)),
        );
    }
    menu = menu.separator().item(kit::MenuItem::new("Manage environments").icon(&icons::SETTINGS).on_select(|s: &mut App| {
        s.ui.env.menu_open = false;
        crate::shell::sidebar::go(s, Route::Settings("connections".into()));
    }));
    menu.overlay(&kit::theme(t), overlays);
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
    fn switching_swaps_the_active_source_and_parks_the_previous_one() {
        // SAFETY: only this test reads S5_ENVIRONMENTS; keeps `remove` off the real config dir.
        unsafe { std::env::set_var("S5_ENVIRONMENTS", std::env::temp_dir().join(format!("s5-env-{}.json", std::process::id()))) };
        let mut s = App::new();
        let mut local = crate::data::empty_state();
        local.shell_ready = true;
        s.src = Source::Fixture(local);
        s.ui.env.saved.push(saved("a"));
        s.ui.env.parked.push(("a".into(), Source::Fixture(crate::data::empty_state())));
        s.route = Route::Thread("t1".into());
        switch(&mut s, "a");
        assert_eq!(s.ui.env.active, "a");
        assert!(!s.state().shell_ready, "the remote's state is on screen");
        assert_eq!(s.route, Route::Home { project_id: None });
        assert!(matches!(local_mut(&mut s), Source::Fixture(st) if st.shell_ready), "this machine's server is parked, not lost");
        remove(&mut s, "a");
        assert_eq!(s.ui.env.active, LOCAL);
        assert!(s.state().shell_ready);
        assert!(s.ui.env.parked.is_empty());
    }
}
