//! The Diff surface, ported from web's DiffPanel: a scope picker over the same review and
//! checkpoint RPCs, the header's view toggles, and the diff with an optional file tree.
mod file_menu;
mod state;
mod view;

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use rune::{Color, Cursor, El, Icon, ScrollSpec, Span, Tx, col, el, icon, rich, row, text};
use rune_kit::diff::{FileDiff, FileStatus};
use serde_json::{Value, json};

pub use state::DiffMode;
use state::DiffState;

use crate::{
    App, data, icons, kit,
    theme::{self, AlphaMul, T},
};

/// lucide `chevrons-down-up`, which the icon set lacks.
static CHEVRONS_DOWN_UP: Icon = Icon {
    name: "chevrons-down-up",
    view: 24.0,
    body: r#"<path d="m7 20 5-5 5 5M7 4l5 5 5-5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

const SCOPE_KEY: &str = "panel-diff-scope";
/// Files fetched one by one after a cut patch; past this the size-limit banner stays.
const FILE_FETCH_LIMIT: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// "Changes": the branch against its base.
    Branch,
    /// "Uncommitted": the working tree.
    Unstaged,
    Commit(String),
    /// One turn's checkpoint diff; `None` follows the latest turn.
    Turn(Option<u64>),
}

#[derive(Clone, Debug, PartialEq)]
struct Turn {
    count: u64,
    at: i64,
}

#[derive(Clone, Debug)]
struct Commit {
    sha: String,
    subject: String,
    at: i64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sub {
    Turns,
    Commits,
}

/// What the view shows besides the files: the refs and subject of the git source.
#[derive(Default)]
struct Source {
    head: Option<String>,
    base: Option<String>,
    subject: Option<String>,
    truncated: bool,
}

pub struct Ui {
    pub state: DiffState,
    scope: Scope,
    /// The branch scope's chosen base; `None` is "Automatic".
    base_ref: Option<String>,
    /// Every file of the loaded patch; `state` holds the shown ones.
    files: Vec<FileDiff>,
    source: Source,
    /// The scope whose patch `files` holds.
    shown: Option<Scope>,
    pending: Option<(u64, Scope)>,
    error: Option<String>,
    /// What the shown patch was read from, to refetch when it moves.
    seen: u64,
    /// The user folded or opened a file, so reloads keep their choice over the default.
    touched: bool,
    /// Those choices by path, kept while a file is out of the list (hidden, or its patch loading).
    folds: HashMap<String, bool>,
    /// Each thread and scope's fold choices, back when it shows again (`collapsedFileKeysByScopeKey`).
    remembered: HashMap<(String, Scope), HashMap<String, bool>>,
    /// The panel's own wrap and whitespace toggles; until pressed they follow the settings,
    /// which pressing leaves alone (web's DiffPanel seeds local state from them).
    wrap: Option<bool>,
    ignore_ws: Option<bool>,
    menu_open: bool,
    sub: Option<Sub>,
    commits: Option<Result<(Vec<Commit>, bool), String>>,
    commits_pending: Option<u64>,
    /// Each ref's name, and its remote's name (`Some("")` when the server named none) if remote.
    refs: Vec<(String, Option<String>)>,
    refs_pending: Vec<u64>,
    refs_cwd: String,
    refs_combo: kit::ComboboxState,
    hide_tests: bool,
    tree_open: bool,
    folded_dirs: HashSet<String>,
    /// Per-file patches requested when the whole patch passed the server's size limit.
    file_pending: Vec<(u64, Scope)>,
    /// The server's line counts by path, which hold even for a file it cut short.
    counts: HashMap<String, (usize, usize)>,
    /// Files the server still cut short on their own.
    partial: HashSet<String>,
    /// A file to open and scroll to once the chosen scope's patch shows (a changed-files row).
    pub reveal: Option<String>,
    file_menu: file_menu::State,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            state: DiffState::new(vec![]),
            scope: Scope::Branch,
            base_ref: None,
            files: vec![],
            source: Source::default(),
            shown: None,
            pending: None,
            error: None,
            seen: 0,
            touched: false,
            folds: HashMap::new(),
            remembered: HashMap::new(),
            wrap: None,
            ignore_ws: None,
            menu_open: false,
            sub: None,
            commits: None,
            commits_pending: None,
            refs: vec![],
            refs_pending: vec![],
            refs_cwd: String::new(),
            refs_combo: Default::default(),
            hide_tests: false,
            tree_open: false,
            folded_dirs: HashSet::new(),
            file_pending: vec![],
            counts: HashMap::new(),
            partial: HashSet::new(),
            reveal: None,
            file_menu: Default::default(),
        }
    }
}

fn setting(app: &App, key: &str, default: bool) -> bool {
    app.client_settings.values.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn wrap_on(app: &App) -> bool {
    app.ui.panel.diff.wrap.unwrap_or_else(|| setting(app, "wordWrap", true))
}

fn ignore_ws(app: &App) -> bool {
    app.ui.panel.diff.ignore_ws.unwrap_or_else(|| setting(app, "diffIgnoreWhitespace", true))
}

fn save_setting(app: &mut App, key: &str, value: Value) {
    app.client_settings.values.insert(key.into(), value);
    app.client_settings.save();
}

/// A new thread starts on "Changes"; the view preferences stay.
pub fn reset(app: &mut App, previous: &str) {
    let mut old = std::mem::take(&mut app.ui.panel.diff);
    remember(&mut old, previous);
    let ui = &mut app.ui.panel.diff;
    ui.hide_tests = old.hide_tests;
    ui.tree_open = old.tree_open;
    ui.remembered = old.remembered;
}

/// Keeps the shown scope's fold choices for when it shows again.
fn remember(ui: &mut Ui, thread: &str) {
    let Some(shown) = ui.shown.clone() else { return };
    capture(&mut ui.folds, &ui.state, ui.touched);
    if !ui.folds.is_empty() {
        ui.remembered.insert((thread.to_owned(), shown), std::mem::take(&mut ui.folds));
    }
}

/// Starts `scope` from its remembered fold choices, or the default.
fn recall(ui: &mut Ui, thread: &str, scope: &Scope) {
    ui.touched = false;
    ui.folds = ui.remembered.get(&(thread.to_owned(), scope.clone())).cloned().unwrap_or_default();
}

/// The git card's "Changes" row (`openChangesFromThreadPanel` selects the branch scope).
/// The panel loads it when it activates.
pub fn open_changes(app: &mut App) {
    choose(app, Scope::Branch);
}

/// A turn stays chosen when newer turns land, as web pins the run it opened.
pub fn open_turn(app: &mut App, count: u64) {
    choose(app, Scope::Turn(Some(count)));
}

/// "Latest turn" pins the newest turn now; with none yet it follows the first to land.
fn latest_turn(app: &App) -> Scope {
    Scope::Turn(turns(app).first().map(|t| t.count))
}

fn choose(app: &mut App, scope: Scope) {
    let thread = app.ui.panel.thread.clone();
    let ui = &mut app.ui.panel.diff;
    ui.menu_open = false;
    ui.sub = None;
    if ui.scope != scope {
        remember(ui, &thread);
        recall(ui, &thread, &scope);
        ui.scope = scope;
    }
}

fn select(app: &mut App, scope: Scope) {
    choose(app, scope);
    load(app);
}

fn is_repo(app: &App) -> Option<bool> {
    let (_, cwd) = super::context(app)?;
    app.state().vcs.get(&cwd).and_then(|v| v["local"]["isRepo"].as_bool())
}

/// The thread's turns with a checkpoint, newest first, one per turn count.
fn turns(app: &App) -> Vec<Turn> {
    let rows = super::context(app)
        .and_then(|(id, _)| app.state().thread_views.get(&id))
        .and_then(|v| v.projection["checkpoints"].as_array().cloned())
        .unwrap_or_default();
    turn_list(&rows)
}

fn turn_list(rows: &[Value]) -> Vec<Turn> {
    let mut by_count: HashMap<u64, i64> = HashMap::new();
    for r in rows {
        let Some(count) = r["checkpointTurnCount"].as_u64().or(r["appRunOrdinal"].as_u64()) else { continue };
        let at = data::iso_ms(data::s(r, "capturedAt")).unwrap_or(0);
        let entry = by_count.entry(count).or_insert(at);
        *entry = (*entry).max(at);
    }
    let mut list: Vec<Turn> = by_count.into_iter().map(|(count, at)| Turn { count, at }).collect();
    list.sort_by(|a, b| b.count.cmp(&a.count).then(b.at.cmp(&a.at)));
    list
}

fn turn_of(scope: &Scope, turns: &[Turn]) -> Option<u64> {
    match scope {
        Scope::Turn(Some(n)) if turns.iter().any(|t| t.count == *n) => Some(*n),
        Scope::Turn(_) => turns.first().map(|t| t.count),
        _ => None,
    }
}

/// `review.getDiffPreview` for a git scope, as DiffPanel builds it, with the patch inline.
fn preview_request(cwd: &str, scope: &Scope, base_ref: Option<&str>, ignore_ws: bool) -> Value {
    let mut req = json!({ "cwd": cwd, "ignoreWhitespace": ignore_ws });
    if let Some(base) = base_ref.filter(|_| matches!(scope, Scope::Branch | Scope::Commit(_))) {
        req["baseRef"] = json!(base);
    }
    match scope {
        Scope::Commit(sha) => req["commitSha"] = json!(sha),
        Scope::Unstaged => req["sourceKind"] = json!("working-tree"),
        _ => req["sourceKind"] = json!("branch-range"),
    }
    req
}

fn source_kind(scope: &Scope) -> &'static str {
    match scope {
        Scope::Unstaged => "working-tree",
        Scope::Commit(_) => "commit",
        _ => "branch-range",
    }
}

/// What a scope's patch depends on: the working tree for git scopes, the turn for turn scopes.
fn signature(app: &App, scope: &Scope) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    scope.hash(&mut h);
    ignore_ws(app).hash(&mut h);
    app.ui.panel.diff.base_ref.hash(&mut h);
    match scope {
        Scope::Turn(_) => turn_of(scope, &turns(app)).hash(&mut h),
        _ => {
            if let Some((_, cwd)) = super::context(app) {
                let local = app.state().vcs.get(&cwd).and_then(|v| v["local"].as_object().cloned()).unwrap_or_default();
                for (k, v) in local.iter().filter(|(k, _)| !k.ends_with("At")) {
                    (k, v.to_string()).hash(&mut h);
                }
            }
        }
    }
    h.finish()
}

pub fn load(app: &mut App) {
    sync_settings(app);
    let Some((thread, cwd)) = super::context(app) else { return };
    let scope = app.ui.panel.diff.scope.clone();
    let ignore_ws = ignore_ws(app);
    let request = match &scope {
        Scope::Turn(_) => turn_of(&scope, &turns(app)).map(|n| {
            ("orchestration.getTurnDiff", json!({ "threadId": thread, "fromTurnCount": n.saturating_sub(1), "toTurnCount": n, "ignoreWhitespace": ignore_ws }))
        }),
        _ => Some(("review.getDiffPreview", preview_request(&cwd, &scope, app.ui.panel.diff.base_ref.as_deref(), ignore_ws))),
    };
    app.ui.panel.diff.seen = signature(app, &scope);
    app.ui.panel.diff.file_pending.clear();
    if matches!(scope, Scope::Branch) && app.ui.panel.diff.refs_cwd != cwd {
        load_refs(app, &cwd);
    }
    let Some((method, payload)) = request else {
        let ui = &mut app.ui.panel.diff;
        ui.pending = None;
        ui.shown = Some(scope);
        ui.files.clear();
        ui.source = Source::default();
        apply(app);
        return;
    };
    let id = app.src.net().map(|net| {
        net.watch_vcs(&cwd);
        net.request(method, payload)
    });
    let ui = &mut app.ui.panel.diff;
    match id {
        Some(id) => ui.pending = Some((id, scope)),
        // Scenes have no connection; their fixture fills the files.
        None => ui.shown = Some(scope),
    }
}

fn load_refs(app: &mut App, cwd: &str) {
    let ui_cwd = cwd.to_string();
    let ids: Vec<u64> = app
        .src
        .net()
        .map(|net| {
            ["local", "remote"]
                .iter()
                .map(|kind| net.request("vcs.listRefs", json!({ "cwd": cwd, "includeMatchingRemoteRefs": true, "refKind": kind, "limit": 100 })))
                .collect()
        })
        .unwrap_or_default();
    let ui = &mut app.ui.panel.diff;
    ui.refs.clear();
    ui.refs_pending = ids;
    ui.refs_cwd = ui_cwd;
}

fn load_commits(app: &mut App) {
    let Some((_, cwd)) = super::context(app) else { return };
    let mut req = json!({ "cwd": cwd });
    if let Some(base) = &app.ui.panel.diff.base_ref {
        req["baseRef"] = json!(base);
    }
    app.ui.panel.diff.commits_pending = app.src.net().map(|net| net.request("review.getBranchCommits", req));
}

/// Puts the loaded files into the view, dropping tests when they are hidden.
fn apply(app: &mut App) {
    let default_collapsed = setting(app, "diffFilesCollapsed", true);
    let ui = &mut app.ui.panel.diff;
    let files: Vec<FileDiff> = ui.files.iter().filter(|f| !ui.hide_tests || !is_test_file(f.path())).cloned().collect();
    let collapsed = collapsed_for(&mut ui.folds, &ui.state, ui.touched, &files, default_collapsed);
    ui.state.set_files(files, &collapsed);
}

fn collapsed_for(folds: &mut HashMap<String, bool>, shown: &DiffState, touched: bool, files: &[FileDiff], default: bool) -> Vec<String> {
    capture(folds, shown, touched);
    files.iter().map(|f| f.path()).filter(|p| folds.get(*p).copied().unwrap_or(default)).map(str::to_string).collect()
}

/// Records the shown files' folds once the user has changed any.
fn capture(folds: &mut HashMap<String, bool>, shown: &DiffState, touched: bool) {
    if touched {
        let collapsed = shown.collapsed_paths();
        for f in shown.files() {
            folds.insert(f.path().to_string(), collapsed.iter().any(|p| p == f.path()));
        }
    }
}

fn take(app: &mut App, id: u64) -> Option<Result<Value, Value>> {
    app.src.net().and_then(|n| n.take_reply(id))
}

fn message(e: &Value) -> String {
    e["message"].as_str().map(str::to_string).unwrap_or_else(|| e.to_string())
}

/// Opens and scrolls to the file `reveal` names once its scope's patch is shown.
pub fn reveal(app: &mut App, tx: &mut rune::Tx) -> bool {
    let ui = &mut app.ui.panel.diff;
    if ui.reveal.is_none() || ui.pending.is_some() || ui.shown.as_ref() != Some(&ui.scope) {
        return false;
    }
    let path = ui.reveal.take().unwrap_or_default();
    let Some(i) = ui.state.files().iter().position(|f| f.path() == path) else { return false };
    ui.touched = true;
    ui.state.set_collapsed(i, false);
    ui.state.scroll_to_file(i, tx);
    true
}

fn visible(app: &App) -> bool {
    app.ui.shell.panel_open && app.ui.panel.active == Some(super::Surface::Diff)
}

pub fn poll(app: &mut App) -> bool {
    let mut changed = file_menu::poll(app);
    if let Some(id) = app.ui.panel.diff.commits_pending
        && let Some(reply) = take(app, id)
    {
        let ui = &mut app.ui.panel.diff;
        ui.commits_pending = None;
        ui.commits = Some(reply.map(|v| (commit_list(&v), v["truncated"].as_bool().unwrap_or(false))).map_err(|e| message(&e)));
        changed = true;
    }
    for id in app.ui.panel.diff.refs_pending.clone() {
        if let Some(reply) = take(app, id) {
            let ui = &mut app.ui.panel.diff;
            ui.refs_pending.retain(|v| *v != id);
            if let Ok(v) = reply {
                for r in v["refs"].as_array().into_iter().flatten() {
                    let name = data::s(r, "name").to_string();
                    if !name.is_empty() && !ui.refs.iter().any(|(n, _)| *n == name) {
                        let remote = r["isRemote"].as_bool().unwrap_or(false).then(|| data::s(r, "remoteName").to_string());
                        ui.refs.push((name, remote));
                    }
                }
            }
            changed = true;
        }
    }
    if let Some((id, scope)) = app.ui.panel.diff.pending.clone()
        && let Some(reply) = take(app, id)
    {
        let thread = app.ui.panel.thread.clone();
        let ui = &mut app.ui.panel.diff;
        ui.pending = None;
        if scope != ui.scope {
            return true;
        }
        if ui.shown.as_ref() != Some(&scope) {
            recall(ui, &thread, &scope);
        }
        ui.shown = Some(scope.clone());
        let mut missing = vec![];
        ui.counts.clear();
        ui.partial.clear();
        match reply {
            Ok(v) => {
                ui.error = None;
                let (patch, mut source, meta) = match scope {
                    Scope::Turn(_) => (v["diff"].as_str().unwrap_or("").to_string(), Source::default(), vec![]),
                    _ => {
                        let src = git_source(&v, &scope);
                        let opt = |k: &str| src[k].as_str().filter(|v| !v.is_empty()).map(str::to_string);
                        let source = Source {
                            head: opt("headRef"),
                            base: opt("baseRef"),
                            subject: opt("commitSubject"),
                            truncated: src["truncated"].as_bool().unwrap_or(false) || v["incomplete"].as_bool().unwrap_or(false),
                        };
                        (src["diff"].as_str().unwrap_or("").to_string(), source, file_stats(&src))
                    }
                };
                let mut files = rune_kit::diff::parse(&patch);
                if source.truncated && !meta.is_empty() {
                    missing = incomplete(&files, &meta);
                    files.retain(|f| !missing.iter().any(|m| m.path == f.path()));
                    source.truncated = missing.len() > FILE_FETCH_LIMIT;
                    missing.truncate(FILE_FETCH_LIMIT);
                }
                ui.counts = meta.into_iter().map(|m| (m.path, (m.additions, m.deletions))).collect();
                files.sort_by(|a, b| a.path().cmp(b.path()));
                ui.files = files;
                ui.source = source;
            }
            Err(e) => {
                ui.error = Some(message(&e));
                ui.files.clear();
                ui.source = Source::default();
            }
        }
        apply(app);
        fetch_files(app, &scope, missing);
        return true;
    }
    for (id, scope) in app.ui.panel.diff.file_pending.clone() {
        let Some(reply) = take(app, id) else { continue };
        let ui = &mut app.ui.panel.diff;
        ui.file_pending.retain(|(v, _)| *v != id);
        changed = true;
        if scope != ui.scope {
            continue;
        }
        match reply {
            Ok(v) => {
                let src = git_source(&v, &scope);
                for file in rune_kit::diff::parse(src["diff"].as_str().unwrap_or("")) {
                    if src["truncated"].as_bool().unwrap_or(false) {
                        ui.partial.insert(file.path().to_string());
                    }
                    ui.files.retain(|f| f.path() != file.path());
                    ui.files.push(file);
                }
                ui.files.sort_by(|a, b| a.path().cmp(b.path()));
            }
            Err(_) => ui.source.truncated = true,
        }
        apply(app);
    }
    if visible(app) && app.ui.panel.diff.pending.is_none() && app.ui.panel.diff.shown.is_some() {
        let scope = app.ui.panel.diff.scope.clone();
        if signature(app, &scope) != app.ui.panel.diff.seen {
            load(app);
            changed = true;
        }
    }
    changed
}

fn commit_list(v: &Value) -> Vec<Commit> {
    v["commits"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| Commit { sha: data::s(c, "sha").to_string(), subject: data::s(c, "subject").to_string(), at: data::iso_ms(data::s(c, "committedAt")).unwrap_or(0) })
        .collect()
}

/// The source a git scope reads, as web picks it by kind.
fn git_source(v: &Value, scope: &Scope) -> Value {
    let kind = source_kind(scope);
    v["sources"].as_array().and_then(|s| s.iter().find(|s| s["kind"] == kind)).cloned().unwrap_or(Value::Null)
}

#[derive(Clone, Debug, PartialEq)]
struct FileStat {
    path: String,
    previous: Option<String>,
    additions: usize,
    deletions: usize,
}

fn file_stats(src: &Value) -> Vec<FileStat> {
    src["files"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| FileStat {
            path: data::s(f, "path").to_string(),
            previous: f["previousPath"].as_str().map(str::to_string),
            additions: f["additions"].as_u64().unwrap_or(0) as usize,
            deletions: f["deletions"].as_u64().unwrap_or(0) as usize,
        })
        .filter(|f| !f.path.is_empty())
        .collect()
}

/// The files a cut patch lacks or holds only part of, by the server's counts.
fn incomplete(files: &[FileDiff], meta: &[FileStat]) -> Vec<FileStat> {
    meta.iter()
        .filter(|m| !files.iter().any(|f| f.path() == m.path && f.additions() == m.additions && f.deletions() == m.deletions))
        .cloned()
        .collect()
}

/// Asks for each file's own patch, as web's lazy per-file loads do (`input.file`).
fn fetch_files(app: &mut App, scope: &Scope, files: Vec<FileStat>) {
    let Some((_, cwd)) = super::context(app) else { return };
    let ignore_ws = ignore_ws(app);
    let base = app.ui.panel.diff.base_ref.clone();
    let mut ids = vec![];
    if let Some(net) = app.src.net() {
        for f in files {
            let mut req = preview_request(&cwd, scope, base.as_deref(), ignore_ws);
            req["file"] = json!({ "path": f.path, "previousPath": f.previous, "sourceKind": source_kind(scope) });
            ids.push((net.request("review.getDiffPreview", req), scope.clone()));
        }
    }
    app.ui.panel.diff.file_pending.extend(ids);
}

/// `isTestFile` from `changedFileKind.ts`: tests and their mocks, never generated files.
pub fn is_test_file(path: &str) -> bool {
    const LOCKFILES: &[&str] = &[
        "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "bun.lockb", "bun.lock", "Cargo.lock", "Gemfile.lock",
        "poetry.lock", "composer.lock", "go.sum", "Pipfile.lock", "uv.lock", "flake.lock", "mix.lock", "pubspec.lock",
        "Podfile.lock", "packages.lock.json", "gradle.lockfile",
    ];
    const GENERATED_DIRS: &[&str] = &["node_modules", "dist", "build", "generated", "__generated__", "vendor", ".next", "out", "target"];
    const MOCK_DIRS: &[&str] = &["__mocks__", "__fixtures__", "__snapshots__", "fixtures", "mocks", "stubs", "fakes", "testdata", "test-data", "test_data"];
    const TEST_DIRS: &[&str] = &["__tests__", "__test__", "cypress", "e2e", "evals", "spec", "specs", "test", "tests"];
    let parts: Vec<&str> = path.split('/').collect();
    let (name, dirs) = parts.split_last().map(|(n, d)| (*n, d)).unwrap_or((path, &[]));
    let lower = name.to_ascii_lowercase();
    if LOCKFILES.contains(&name) || lower.contains(".generated.") || lower.contains(".gen.") || lower.ends_with(".min.js") || dirs.iter().any(|d| GENERATED_DIRS.contains(d)) {
        return false;
    }
    let stem_tags = |tags: &[&str], sep: char| {
        let mut pieces = lower.split('.');
        let first = pieces.next().unwrap_or("");
        let rest: Vec<&str> = pieces.collect();
        // A tag before the extension (`a.test.ts`), or after `_` in the stem (`a_test.go`).
        (rest.len() >= 2 && tags.contains(&rest[rest.len() - 2]))
            || (sep == '_' && rest.len() == 1 && first.rsplit_once('_').is_some_and(|(_, t)| tags.contains(&t)))
    };
    let mock_name = ["mock", "fixture", "stub", "fake"].iter().any(|p| {
        name.strip_prefix(p).is_some_and(|rest| rest.is_empty() || rest.starts_with(['-', '_', '.']) || rest.starts_with(|c: char| c.is_ascii_uppercase()))
    });
    if lower.ends_with(".snap") || stem_tags(&["mock", "mocks", "fixture", "fixtures", "stub", "stubs", "fake", "fakes"], '.') || mock_name || dirs.iter().any(|d| MOCK_DIRS.contains(d)) {
        return true;
    }
    let class_test = name.rsplit_once('.').is_some_and(|(stem, ext)| {
        ["java", "kt", "kts", "scala", "groovy", "cs", "php", "swift", "m"].contains(&ext) && (stem.ends_with("Test") || stem.ends_with("Tests") || stem.ends_with("Spec"))
    });
    stem_tags(&["test", "spec", "e2e", "eval"], '.')
        || stem_tags(&["test", "spec"], '_')
        || (lower.starts_with("test_") && lower.ends_with(".py"))
        || lower == "conftest.py"
        || lower.starts_with("vitest.setup.")
        || lower.starts_with("jest.setup.")
        || class_test
        || dirs.iter().any(|d| TEST_DIRS.contains(d))
}

/// `formatCompactDiffCount`.
fn compact(n: usize) -> String {
    let short = |v: f64, unit: &str| if v < 10.0 { format!("{}{unit}", format!("{v:.1}").trim_end_matches(".0")) } else { format!("{}{unit}", v.round()) };
    match n {
        0..1000 => n.to_string(),
        1000..999_500 => short(n as f64 / 1e3, "k"),
        _ => short(n as f64 / 1e6, "m"),
    }
}

/// One row of the file tree: a folder (with chains of single folders joined) or a file.
#[derive(Debug, PartialEq)]
struct TreeRow {
    depth: u32,
    label: String,
    /// The folder's full path, for folding it.
    dir: Option<String>,
    file: Option<usize>,
}

#[derive(Default)]
struct Node {
    name: String,
    children: Vec<Node>,
    file: Option<usize>,
}

fn tree_rows(paths: &[&str], folded: &HashSet<String>) -> Vec<TreeRow> {
    let mut root = Node::default();
    for (i, path) in paths.iter().enumerate() {
        let mut node = &mut root;
        let parts: Vec<&str> = path.split('/').collect();
        for (d, part) in parts.iter().enumerate() {
            let leaf = d + 1 == parts.len();
            let at = node.children.iter().position(|c| c.name == *part && c.file.is_none() && !leaf);
            let at = match at {
                Some(at) => at,
                None => {
                    node.children.push(Node { name: part.to_string(), children: vec![], file: leaf.then_some(i) });
                    node.children.len() - 1
                }
            };
            node = &mut node.children[at];
        }
    }
    fn walk(node: &Node, prefix: &str, depth: u32, folded: &HashSet<String>, out: &mut Vec<TreeRow>) {
        for child in &node.children {
            if let Some(file) = child.file {
                out.push(TreeRow { depth, label: child.name.clone(), dir: None, file: Some(file) });
                continue;
            }
            let (mut label, mut dir) = (child.name.clone(), child);
            while dir.children.len() == 1 && dir.children[0].file.is_none() {
                dir = &dir.children[0];
                label = format!("{label}/{}", dir.name);
            }
            let path = if prefix.is_empty() { label.clone() } else { format!("{prefix}/{label}") };
            let open = !folded.contains(&path);
            out.push(TreeRow { depth, label, dir: Some(path.clone()), file: None });
            if open {
                walk(dir, &path, depth + 1, folded, out);
            }
        }
    }
    let mut out = vec![];
    walk(&root, "", 0, folded, &mut out);
    out
}

fn all_dirs(paths: &[&str]) -> HashSet<String> {
    tree_rows(paths, &HashSet::new()).into_iter().filter_map(|r| r.dir).collect()
}

fn scope_label(ui: &Ui, turns: &[Turn]) -> String {
    match &ui.scope {
        Scope::Branch => "Changes".into(),
        Scope::Unstaged => "Uncommitted".into(),
        Scope::Commit(sha) => ui.source.subject.clone().filter(|_| ui.shown.as_ref() == Some(&ui.scope)).unwrap_or_else(|| format!("Commit {}", short(sha))),
        Scope::Turn(n) => match turn_of(&ui.scope, turns) {
            Some(c) if n.is_none() || turns.first().map(|t| t.count) == Some(c) => "Latest turn".into(),
            Some(c) => format!("Turn {c}"),
            None => "Latest turn".into(),
        },
    }
}

/// `projectCommitUrl`: the code host's page for `sha`, from the project's repository identity.
fn commit_url(identity: &Value, sha: &str) -> Option<String> {
    let (host, repository) = identity["canonicalKey"].as_str()?.split_once('/')?;
    let route = match identity["provider"].as_str()? {
        "github" if repository.split('/').count() == 2 => "commit",
        "gitlab" => "-/commit",
        "bitbucket" => "commits",
        "gitcafe" | "forgejo" | "azure-devops" => "commit",
        _ => return None,
    };
    let base = match identity["webUrl"].as_str() {
        Some(web) => web.to_string(),
        None => format!("{}/{repository}", web_origin(host, identity["locator"]["remoteUrl"].as_str().unwrap_or("").trim())),
    };
    let base = base.split(['?', '#']).next()?;
    let (scheme, rest) = base.split_once("://")?;
    if host.is_empty() || !matches!(scheme, "http" | "https") {
        return None;
    }
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let path = path.trim_end_matches('/');
    let path = if path.is_empty() { String::new() } else { format!("{path}/") };
    Some(format!("{scheme}://{authority}/{path}{route}/{}", sha.to_lowercase()))
}

/// `remoteWebOrigin`: an HTTP remote on `host` names the web origin, port included; SSH does not.
fn web_origin(host: &str, remote: &str) -> String {
    let http = remote.split_once("://").filter(|(scheme, _)| matches!(*scheme, "http" | "https"));
    if let Some((scheme, rest)) = http {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h).to_lowercase();
        let name = authority.split(':').next().unwrap_or("");
        if authority.eq_ignore_ascii_case(host) || name.eq_ignore_ascii_case(host) {
            return format!("{scheme}://{authority}");
        }
    }
    format!("https://{host}")
}

/// A commit's short SHA, linked to its host page when the project has one.
fn sha_link(app: &App, t: &T, sha: &str) -> El {
    let label = short(sha).to_string();
    let muted = t.c.muted_foreground;
    let identity = match &app.route {
        crate::Route::Thread(id) => app.state().threads.iter().find(|v| v["id"] == *id).and_then(|thread| app.state().projects.iter().find(|p| p["id"] == thread["projectId"])).map(|p| &p["repositoryIdentity"]),
        _ => None,
    };
    let Some(url) = identity.and_then(|i| commit_url(i, sha)) else {
        return text(label).font(theme::MONO_XS).color(muted).px(8.0);
    };
    let n = label.len();
    rich(label.clone(), vec![Span::new(0, n).underline(muted)])
        .font(theme::MONO_XS)
        .color(muted)
        .px(8.0)
        .cursor(Cursor::Pointer)
        .role("link")
        .label(format!("Open commit {label}"))
        .help(url.clone())
        .on_press_tx(move |s: &mut App, tx: &mut Tx| super::terminal::open_web(s, tx, &url, false))
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

/// "9:00 AM" in local time, or "09:00" under the 24-hour setting (`formatShortTimestamp`).
fn clock(app: &App, ms: i64) -> String {
    let hour12 = app.client_settings.values.get("timestampFormat").and_then(Value::as_str) != Some("24-hour");
    clock_at(data::Zone::Local.wall(ms), hour12)
}

fn clock_at(wall: i64, hour12: bool) -> String {
    let mins = wall.rem_euclid(86_400_000) / 60_000;
    let (h, m) = (mins / 60, mins % 60);
    if hour12 {
        format!("{}:{m:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" })
    } else {
        format!("{h:02}:{m:02}")
    }
}

/// A header toggle: ghost `sm` button that shows pressed with the accent fill.
fn toggle(t: &T, key: &str, label: &str, glyph: &'static Icon, on: bool, act: impl Fn(&mut App) + 'static) -> El {
    let fg = t.c.contrast_foreground;
    row()
        .key(key)
        .size(28.0, 28.0)
        .radius(8.0)
        .center()
        .fill(if on { t.c.accent } else { Color::TRANSPARENT })
        .hover(t.c.accent)
        .cursor(Cursor::Pointer)
        .focusable(true)
        .focus_ring(2.0, -2.0, t.c.ring)
        .role("button")
        .label(label.to_owned())
        .help(label.to_owned())
        .checked(on)
        .child(icon(glyph, 14.0).color(fg).opacity(0.8))
        .on_press(act)
}

fn icon_button(t: &T, key: &str, label: &str, glyph: &'static Icon, act: impl Fn(&mut App) + 'static) -> El {
    kit::Button::new(label).icon(glyph).variant("ghost").size("icon-sm").on_press(act).build(t).key(key).help(label.to_owned())
}

/// The stacked / split segmented control.
fn layout_group(app: &App, t: &T) -> El {
    let mode = app.ui.panel.diff.state.mode();
    let pressed = if t.dark { t.c.input.alpha_mul(0.72) } else { t.c.background };
    let item = |key: &str, label: &str, glyph: &'static Icon, m: DiffMode| {
        let on = mode == m;
        row()
            .key(key)
            .h(24.0)
            .px(10.0)
            .radius(6.0)
            .center()
            .fill(if on { pressed } else { Color::TRANSPARENT })
            .hover(if on { pressed } else if t.dark { t.c.input.alpha_mul(0.32) } else { t.c.background.alpha_mul(0.6) })
            .cursor(Cursor::Pointer)
            .focusable(true)
            .role("button")
            .label(label.to_owned())
            .help(label.to_owned())
            .checked(on)
            .child(icon(glyph, 14.0).color(if on { t.c.foreground } else { t.c.muted_foreground }))
            .on_press(move |s: &mut App| {
                s.ui.panel.diff.state.set_mode(m);
                save_setting(s, "diffLayout", json!(if m == DiffMode::Split { "split" } else { "stacked" }));
            })
    };
    row()
        .key("panel-diff-layout")
        .padding(2.0)
        .gap(2.0)
        .radius(8.0)
        .fill(t.c.input.alpha_mul(0.4))
        .child(item("panel-diff-stacked", "Stacked diff", &icons::ROWS_3, DiffMode::Unified))
        .child(item("panel-diff-split", "Split diff", &icons::COLUMNS_2, DiffMode::Split))
}

/// One base-ref row per branch (`buildBaseRefChoices`): a local branch paired with its remote
/// twin, `origin` first, then the remotes no local branch claimed.
#[derive(Debug, PartialEq)]
struct BaseChoice {
    label: String,
    local: Option<String>,
    remote: Option<String>,
}

impl BaseChoice {
    /// The ref the row picks: the remote twin only while it is the chosen base.
    fn value(&self, base: Option<&str>) -> String {
        match (&self.remote, base) {
            (Some(remote), Some(base)) if remote == base => remote.clone(),
            _ => self.local.clone().or(self.remote.clone()).unwrap_or_default(),
        }
    }
}

/// `head` is left out of the local branches, so its remote twin lists as remote-only.
fn base_choices(refs: &[(String, Option<String>)], head: Option<&str>) -> Vec<BaseChoice> {
    let branch = |name: &str, remote: &str| {
        if remote.is_empty() { name.to_string() } else { name.strip_prefix(&format!("{remote}/")).unwrap_or(name).to_string() }
    };
    let remotes: Vec<(&String, &String)> = refs.iter().filter_map(|(n, r)| r.as_ref().map(|r| (n, r))).collect();
    let mut used = HashSet::new();
    let mut choices: Vec<BaseChoice> = refs
        .iter()
        .filter(|(n, r)| r.is_none() && Some(n.as_str()) != head)
        .map(|(local, _)| {
            let matches: Vec<&(&String, &String)> = remotes.iter().filter(|(n, r)| !used.contains(*n) && branch(n, r) == *local).collect();
            let remote = matches.iter().find(|(_, r)| r.as_str() == "origin").or(matches.first()).map(|(n, _)| (*n).clone());
            if let Some(remote) = &remote {
                used.insert(remote.clone());
            }
            BaseChoice { label: local.clone(), local: Some(local.clone()), remote }
        })
        .collect();
    choices.extend(remotes.iter().filter(|(n, _)| !used.contains(*n)).map(|(n, _)| BaseChoice {
        label: (*n).clone(),
        local: None,
        remote: Some((*n).clone()),
    }));
    choices
}

fn refs_combo(app: &App, t: &T, layer: &mut Vec<El>) -> El {
    let ui = &app.ui.panel.diff;
    let choices = base_choices(&ui.refs, ui.source.head.as_deref());
    let mut options = vec![kit::ComboboxOption::new("", "Automatic")];
    let mut pairs = HashMap::new();
    for choice in &choices {
        let value = choice.value(ui.base_ref.as_deref());
        if let (Some(local), Some(remote)) = (&choice.local, &choice.remote) {
            pairs.insert(value.clone(), (choice.label.clone(), local.clone(), remote.clone()));
        } else if choice.remote.is_some() {
            pairs.insert(value.clone(), (choice.label.clone(), String::new(), String::new()));
        }
        options.push(kit::ComboboxOption::new(value, choice.label.clone()));
    }
    let k = kit::theme(t);
    let muted = t.c.muted_foreground;
    let trailing = move |o: &kit::ComboboxOption| {
        let (label, local, remote) = pairs.get(&o.id)?.clone();
        if local.is_empty() {
            return Some(row().w(32.).center_y().justify_end().child(icon(&icons::CHECK, 12.).color(muted)).help("Remote only").label("Remote only"));
        }
        let checked = o.id == remote;
        let switch = kit::Switch::new(checked)
            .label(format!("Use remote version of {label}"))
            .hide_label()
            .size("sm")
            .on_toggle(move |s: &mut App| {
                let ui = &mut s.ui.panel.diff;
                ui.base_ref = Some(if checked { local.clone() } else { remote.clone() });
                ui.commits = None;
                load(s);
            })
            .build(&k);
        Some(row().w(32.).center_y().justify_end().child(switch))
    };
    let column = |label: &str| text(label.to_uppercase()).font(rune::TextStyle { tracking: 0.25, ..theme::weight(theme::TEXT_3XS, theme::MEDIUM) }).color(muted);
    // Lines "Remote" up with the trailing column, which sits before the row's 14px check.
    let columns = row()
        .w_full()
        .pl(12.)
        .pr(34.)
        .pt(8.)
        .pb(6.)
        .center_y()
        .child(column("Branch").grow(1.))
        .child(row().w(32.).justify_end().child(column("Remote")));
    let columns = col().w_full().child(columns).child(el().w_full().h(1.).fill(t.c.border.alpha_mul(0.7)));
    let base = ui.source.base.clone().or(ui.base_ref.clone()).unwrap_or_else(|| "Automatic".into());
    let trigger = row()
        .h(24.0)
        .px(7.0)
        .gap(4.0)
        .radius(8.0)
        .center_y()
        .max_w(192.0)
        .min_w(36.0)
        .hover(t.c.accent)
        .cursor(Cursor::Pointer)
        .role("button")
        .label(format!("Change comparison target. Currently {base}"))
        .child(text(base).font(theme::XS_MEDIUM).color(t.c.contrast_muted_foreground).single_line().truncate())
        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(t.c.contrast_muted_foreground).opacity(0.7));
    kit::Combobox::new("panel-diff-base", &ui.refs_combo, &options, |s: &mut App| &mut s.ui.panel.diff.refs_combo)
        .label("Comparison target")
        .current(Some(ui.base_ref.as_deref().unwrap_or("")))
        .search_placeholder("Search refs...")
        .empty_message("No matching refs.")
        .width(288.0)
        .trigger(trigger)
        .trailing(trailing)
        .header(columns)
        .on_choose(|s: &mut App, id, _| {
            s.ui.panel.diff.refs_combo.close();
            s.ui.panel.diff.base_ref = (!id.is_empty()).then(|| id.to_string());
            s.ui.panel.diff.commits = None;
            load(s);
        })
        .build(&kit::theme(t), layer)
}

/// A file's line counts: the server's when it gave them, else the patch's.
fn counts(ui: &Ui, f: &FileDiff) -> (usize, usize) {
    ui.counts.get(f.path()).copied().unwrap_or((f.additions(), f.deletions()))
}

/// `refs_w`: the refs area's width last frame, for the `@max-[20rem]/diff-refs` query.
fn header(app: &App, t: &T, refs_w: Option<f32>) -> El {
    let ui = &app.ui.panel.diff;
    let turns = turns(app);
    let label = scope_label(ui, &turns);
    let fg = t.c.contrast_secondary_foreground;
    let commit = matches!(ui.scope, Scope::Commit(_));
    let scope = row()
        .key(SCOPE_KEY)
        .h(24.0)
        .px(7.0)
        .gap(4.0)
        .radius(8.0)
        .center_y()
        .fill(t.c.secondary)
        .hover(t.c.secondary.alpha_mul(0.9))
        .cursor(Cursor::Pointer)
        .focusable(true)
        .focus_ring(2.0, -2.0, t.c.ring)
        .role("button")
        .label(format!("Diff scope: {label}"))
        .expanded(ui.menu_open)
        .child(if commit { text(label).font(theme::XS_MEDIUM).color(fg).single_line().truncate().min_w(0.0).shrink() } else { text(label).font(theme::XS_MEDIUM).color(fg).single_line() })
        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(fg).opacity(0.7))
        .on_press(|s: &mut App| {
            let ui = &mut s.ui.panel.diff;
            ui.menu_open = !ui.menu_open;
            ui.sub = None;
        });
    let scope = if commit { scope.min_w(0.0).shrink() } else { scope };
    let mut left = row().key("panel-diff-refs").grow(1.0).min_w(0.0).shrink().gap(12.0).center_y().child(scope);
    if let Scope::Commit(sha) = &ui.scope {
        left = left.child(sha_link(app, t, sha).key("panel-diff-sha"));
    }
    let git_ok = is_repo(app) != Some(false);
    if ui.scope == Scope::Branch && git_ok {
        let head = ui.source.head.clone().or_else(|| super::context(app).and_then(|(_, cwd)| app.state().vcs.get(&cwd).and_then(|v| v["local"]["refName"].as_str().map(str::to_string)))).unwrap_or_else(|| "HEAD".into());
        let base = ui.source.base.clone().or(ui.base_ref.clone()).unwrap_or_else(|| "Automatic".into());
        let mut refs = row().min_w(0.0).shrink().gap(8.0).center_y().label(format!("Comparing {head} against {base}"));
        // Too narrow for both refs: keep the target readable and drop the head, the thread's own branch.
        refs = if refs_w.is_some_and(|w| w < 320.0) {
            refs.child(text("vs").font(theme::TEXT_XS).color(t.c.muted_foreground).help(format!("{head} → {base}")))
        } else {
            refs.child(
                row()
                    .min_w(48.0)
                    .shrink()
                    .gap(8.0)
                    .center_y()
                    .help(format!("{head} → {base}"))
                    .child(text(head).font(theme::TEXT_XS).color(t.c.muted_foreground).single_line().truncate().max_w(192.0).min_w(0.0).shrink())
                    .child(icon(&icons::ARROW_RIGHT, 14.0).color(t.c.muted_foreground).opacity(0.7)),
            )
        };
        left = left.child(refs.child(refs_combo(app, t, &mut vec![])));
    }
    let st = &ui.state;
    let files = st.files();
    let mut right = row().gap(4.0).center_y();
    if !files.is_empty() {
        let (add, del) = files.iter().map(|f| counts(ui, f)).fold((0, 0), |(a, d), (fa, fd)| (a + fa, d + fd));
        right = right.child(
            row()
                .mr(4.0)
                .gap(4.0)
                .child(text(format!("+{}", compact(add))).font(theme::mono(theme::TEXT_2XS)).color(t.c.diff_addition_foreground))
                .child(text(format!("-{}", compact(del))).font(theme::mono(theme::TEXT_2XS)).color(t.c.diff_deletion_foreground)),
        );
    }
    if !matches!(ui.scope, Scope::Turn(_)) && git_ok {
        right = right.child(icon_button(t, "panel-diff-refresh", "Refresh diff", &icons::REFRESH_CW, load));
    }
    if !files.is_empty() {
        let all = st.all_collapsed();
        right = right.child(icon_button(
            t,
            "panel-diff-collapse",
            if all { "Expand all files" } else { "Collapse all files" },
            if all { &icons::CHEVRONS_UP_DOWN } else { &CHEVRONS_DOWN_UP },
            move |s: &mut App| {
                s.ui.panel.diff.touched = true;
                s.ui.panel.diff.state.set_all_collapsed(!all);
            },
        ));
    }
    right = right.child(layout_group(app, t));
    let wrap = wrap_on(app);
    right = right.child(toggle(t, "panel-diff-wrap", if wrap { "Disable line wrapping" } else { "Enable line wrapping" }, &icons::TEXT_WRAP, wrap, move |s: &mut App| {
        s.ui.panel.diff.wrap = Some(!wrap);
        s.ui.panel.diff.state.set_wrap(!wrap);
    }));
    let ignore = ignore_ws(app);
    right = right.child(toggle(t, "panel-diff-whitespace", if ignore { "Show whitespace changes" } else { "Hide whitespace changes" }, &icons::PILCROW, ignore, move |s: &mut App| {
        s.ui.panel.diff.ignore_ws = Some(!ignore);
        load(s);
    }));
    if ui.files.iter().any(|f| is_test_file(f.path())) {
        let hide = ui.hide_tests;
        right = right.child(toggle(t, "panel-diff-tests", if hide { "Show test files" } else { "Hide test files" }, &icons::FLASK_CONICAL_OFF, hide, move |s: &mut App| {
            s.ui.panel.diff.hide_tests = !hide;
            apply(s);
        }));
    }
    if !ui.files.is_empty() {
        let open = ui.tree_open;
        right = right.child(toggle(t, "panel-diff-tree", if open { "Hide file tree" } else { "Show file tree" }, &icons::FOLDER_TREE, open, move |s: &mut App| {
            s.ui.panel.diff.tree_open = !open;
        }));
    }
    row()
        .key("panel-diff-header")
        .h(40.0)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .w_full()
        .child(left)
        .child(right)
}

fn tree_pane(app: &App, t: &T) -> El {
    let st = &app.ui.panel.diff.state;
    let ui = &app.ui.panel.diff;
    let paths: Vec<&str> = st.files().iter().map(FileDiff::path).collect();
    let rows = tree_rows(&paths, &ui.folded_dirs);
    let dirs = all_dirs(&paths);
    let current = st.visible_file();
    let mut head = row()
        .h(40.0)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .w_full()
        .child(text("Files").font(theme::XS_MEDIUM).color(t.c.foreground))
        .child(el().grow(1.0))
        .child(text(paths.len().to_string()).font(theme::TEXT_XS).color(t.c.muted_foreground));
    if !dirs.is_empty() {
        let all_folded = dirs.iter().all(|d| ui.folded_dirs.contains(d));
        head = head.child(
            kit::Button::new(if all_folded { "Expand all folders" } else { "Collapse all folders" })
                .icon(if all_folded { &icons::CHEVRONS_UP_DOWN } else { &CHEVRONS_DOWN_UP })
                .variant("ghost")
                .size("icon-xs")
                .on_press(move |s: &mut App| {
                    let ui = &mut s.ui.panel.diff;
                    if all_folded {
                        ui.folded_dirs.clear();
                    } else {
                        let paths: Vec<&str> = ui.state.files().iter().map(FileDiff::path).collect();
                        ui.folded_dirs = all_dirs(&paths);
                    }
                })
                .build(t),
        );
    }
    let mut list = col().key("panel-diff-tree-rows").w_full().padding(4.0).gap(1.0).role("tree").label("Changed files");
    for (i, r) in rows.into_iter().enumerate() {
        let pad = 8.0 + r.depth as f32 * 12.0;
        let base = row().key(format!("t{i}")).h(24.0).pl(pad).pr(8.0).gap(6.0).radius(6.0).center_y().w_full().cursor(Cursor::Pointer).focusable(true).role("treeitem");
        let line = if let Some(dir) = r.dir {
            let open = !ui.folded_dirs.contains(&dir);
            base.hover(t.c.foreground.alpha(0.07))
                .label(r.label.clone())
                .expanded(open)
                .child(icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 12.0).color(t.c.muted_foreground))
                .child(icon(if open { &icons::FOLDER_OPEN } else { &icons::FOLDER }, 14.0).color(t.c.muted_foreground))
                .child(text(r.label).font(theme::TEXT_XS).color(t.c.foreground).single_line().truncate().min_w(0.0).shrink())
                .on_press(move |s: &mut App| {
                    let folded = &mut s.ui.panel.diff.folded_dirs;
                    if !folded.remove(&dir) {
                        folded.insert(dir.clone());
                    }
                })
        } else {
            let f = r.file.unwrap_or(0);
            let file = &st.files()[f];
            let color = match file.status {
                FileStatus::Added | FileStatus::Copied => t.c.diff_addition_foreground,
                FileStatus::Deleted => t.c.diff_deletion_foreground,
                FileStatus::Renamed => t.c.diff_modified_icon.alpha(1.0),
                FileStatus::Modified => t.c.foreground,
            };
            let selected = f == current;
            base.pl(pad + 18.0)
                .fill(if selected { t.c.foreground.alpha(0.12) } else { Color::TRANSPARENT })
                .hover(t.c.foreground.alpha(if selected { 0.12 } else { 0.07 }))
                .label(file.path().to_string())
                .child(icon(&icons::FILE, 14.0).color(t.c.muted_foreground))
                .child(text(r.label).font(theme::TEXT_XS).color(color).single_line().truncate().min_w(0.0).shrink())
                .on_press_tx(move |s: &mut App, tx| s.ui.panel.diff.state.scroll_to_file(f, tx))
        };
        list = list.child(line);
    }
    col()
        .key("panel-diff-tree")
        .w_frac(0.4)
        .min_w(0.0)
        .min_h(0.0)
        .self_stretch()
        .child(head)
        .child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.6)))
        .child(col().grow(1.0).min_h(0.0).w_full().child(list).scroll(ScrollSpec { stick_bottom: false }))
}

fn body(app: &App, t: &T) -> El {
    let ui = &app.ui.panel.diff;
    let Some(_) = super::context(app) else {
        if !ui.state.files().is_empty() {
            return diff_body(app, t);
        }
        return super::notice(t, "Select a thread to inspect turn diffs.");
    };
    if is_repo(app) == Some(false) {
        return super::notice(t, "Turn diffs are unavailable because this project is not a git repository.");
    }
    if matches!(ui.scope, Scope::Turn(_)) && turns(app).is_empty() {
        return super::notice(t, "No completed turns yet.");
    }
    if let Some(error) = &ui.error {
        return col().grow(1.0).min_h(0.0).center().px(20.0).child(text(error.clone()).font(theme::TEXT_2XS).color(t.c.error.alpha_mul(0.8)).wrap());
    }
    if ui.shown.as_ref() != Some(&ui.scope) {
        let label = match ui.scope {
            Scope::Turn(_) => "Loading checkpoint diff...",
            Scope::Unstaged => "Loading uncommitted changes...",
            Scope::Commit(_) => "Loading commit...",
            Scope::Branch => "Loading changes...",
        };
        return super::notice(t, label);
    }
    if ui.state.files().is_empty() && !ui.file_pending.is_empty() {
        return super::notice(t, "Loading changes...");
    }
    if ui.state.files().is_empty() && !ui.source.truncated {
        return super::notice(t, "No net changes in this selection.");
    }
    diff_body(app, t)
}

fn diff_body(app: &App, t: &T) -> El {
    let ui = &app.ui.panel.diff;
    let look = Rc::new(view::Look::new(t));
    let mut main = row().grow(1.0).min_h(0.0).w_full().items_stretch().child(view::build(app, look));
    if ui.tree_open && !ui.state.files().is_empty() {
        main = main.child(el().w(1.0).self_stretch().fill(t.c.border.alpha_mul(0.6))).child(tree_pane(app, t));
    }
    let mut out = col().grow(1.0).min_h(0.0).w_full();
    if ui.source.truncated {
        out = out
            .child(
                row()
                    .w_full()
                    .px(12.0)
                    .py(6.0)
                    .fill(t.c.muted.alpha_mul(0.4))
                    .child(text("This preview exceeds the size limit. Changes shown are incomplete.").font(theme::TEXT_2XS).color(t.c.muted_foreground).wrap()),
            )
            .child(el().h(1.0).w_full().fill(t.c.border));
    }
    out.child(main)
}

pub fn view(app: &App, t: &T, refs_w: Option<f32>) -> El {
    col()
        .grow(1.0)
        .min_h(0.0)
        .child(header(app, t, refs_w))
        .child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.6)))
        .child(body(app, t))
}

fn scope_menu(app: &App, t: &T, layer: &mut Vec<El>) {
    use kit::{MenuEntry, MenuItem};
    let ui = &app.ui.panel.diff;
    let turns = turns(app);
    let current = turn_of(&ui.scope, &turns);
    let latest = matches!(ui.scope, Scope::Turn(_)) && current == turns.first().map(|t| t.count);
    let turn_entries: Vec<MenuEntry> = turns
        .iter()
        .map(|turn| {
            let n = turn.count;
            MenuEntry::item(
                MenuItem::new(format!("Turn {n}"))
                    .radio(matches!(ui.scope, Scope::Turn(_)) && current == Some(n))
                    .shortcut(clock(app, turn.at))
                    .on_select(move |s: &mut App| {
                        open_turn(s, n);
                        load(s);
                    }),
            )
        })
        .collect();
    let now = data::now_ms();
    let mut commit_entries: Vec<MenuEntry> = vec![];
    let note = |label: String| MenuEntry::item(MenuItem::new(label).disabled(true));
    match &ui.commits {
        None => commit_entries.push(note("Loading commits...".into())),
        Some(Err(e)) => commit_entries.push(note(e.clone())),
        Some(Ok((commits, truncated))) => {
            for c in commits {
                let sha = c.sha.clone();
                commit_entries.push(MenuEntry::item(
                    MenuItem::new(c.subject.clone())
                        .radio(ui.scope == Scope::Commit(sha.clone()))
                        .shortcut(crate::shell::sidebar::logic::relative_label(c.at, now))
                        .trailing(sha_link(app, t, &sha))
                        .on_select(move |s: &mut App| select(s, Scope::Commit(sha.clone()))),
                ));
            }
            if commits.is_empty() {
                commit_entries.push(note("No commits on this branch.".into()));
            } else if *truncated {
                commit_entries.push(note(format!("Showing latest {} commits", commits.len())));
            }
        }
    }
    let sub = ui.sub;
    kit::Menu::new("panel-diff-scope-menu")
        .anchor(SCOPE_KEY)
        .placement(kit::Placement::BottomStart)
        .min_width(180.0)
        .open(true)
        .on_close(|s: &mut App| {
            s.ui.panel.diff.menu_open = false;
            s.ui.panel.diff.sub = None;
        })
        .item(MenuItem::new("Changes").radio(ui.scope == Scope::Branch).on_select(|s: &mut App| select(s, Scope::Branch)))
        .item(MenuItem::new("Uncommitted").radio(ui.scope == Scope::Unstaged).on_select(|s: &mut App| select(s, Scope::Unstaged)))
        .item(MenuItem::new("Latest turn").radio(latest).on_select(|s: &mut App| select(s, latest_turn(s))))
        .item(MenuItem::new("Turn").submenu(turn_entries, sub == Some(Sub::Turns)).on_submenu_change(|s: &mut App, on| {
            s.ui.panel.diff.sub = if on { Some(Sub::Turns) } else { None };
        }))
        .item(MenuItem::new("Commits").submenu(commit_entries, sub == Some(Sub::Commits)).submenu_min_width(320.0).on_submenu_change(|s: &mut App, on| {
            s.ui.panel.diff.sub = if on { Some(Sub::Commits) } else { None };
            if on {
                load_commits(s);
            }
        }))
        .overlay(&kit::theme(t), layer);
}

pub fn overlays(app: &App, t: &T) -> Vec<El> {
    let mut layer = vec![];
    let ui = &app.ui.panel.diff;
    if ui.menu_open {
        scope_menu(app, t, &mut layer);
    }
    if ui.refs_combo.open && ui.scope == Scope::Branch {
        refs_combo(app, t, &mut layer);
    }
    file_menu::overlay(app, t, &mut layer);
    layer
}

pub fn fixture(app: &mut App) {
    let value: Value = serde_json::from_str(include_str!("../../../fixtures/panel/diff.json")).unwrap();
    let ui = &mut app.ui.panel.diff;
    let mut files = rune_kit::diff::parse(value["diff"].as_str().unwrap());
    files.sort_by(|a, b| a.path().cmp(b.path()));
    ui.files = files;
    ui.touched = true;
    ui.shown = Some(ui.scope.clone());
    apply(app);
}

/// The persisted layout setting, which other clients may change too, and the wrap toggle.
fn sync_settings(app: &mut App) {
    let split = app.client_settings.values.get("diffLayout").and_then(Value::as_str) == Some("split");
    let wrap = wrap_on(app);
    let st = &mut app.ui.panel.diff.state;
    st.set_mode(if split { DiffMode::Split } else { DiffMode::Unified });
    st.set_wrap(wrap);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_refs_pair_each_branch_with_its_origin_twin() {
        let local = |n: &str| -> (String, Option<String>) { (n.to_string(), None) };
        let remote = |n: &str, r: &str| (n.to_string(), Some(r.to_string()));
        let refs = [local("main"), remote("upstream/main", "upstream"), remote("origin/main", "origin"), remote("origin/release", "origin"), local("wip")];
        let choices = base_choices(&refs, None);
        let rows: Vec<(&str, Option<&str>, Option<&str>)> = choices.iter().map(|c| (c.label.as_str(), c.local.as_deref(), c.remote.as_deref())).collect();
        assert_eq!(
            rows,
            [
                ("main", Some("main"), Some("origin/main")),
                ("wip", Some("wip"), None),
                ("upstream/main", None, Some("upstream/main")),
                ("origin/release", None, Some("origin/release")),
            ]
        );
        assert_eq!(choices[0].value(None), "main");
        assert_eq!(choices[0].value(Some("origin/main")), "origin/main");
        assert_eq!(choices[3].value(Some("main")), "origin/release");
        // The thread's own branch isn't a base; its remote twin still is.
        let labels: Vec<String> = base_choices(&refs, Some("main")).into_iter().map(|c| c.label).collect();
        assert_eq!(labels, ["wip", "upstream/main", "origin/main", "origin/release"]);
    }

    #[test]
    fn git_scopes_ask_for_their_source_like_web() {
        let branch = preview_request("/r", &Scope::Branch, Some("main"), true);
        assert_eq!(branch, json!({ "cwd": "/r", "ignoreWhitespace": true, "baseRef": "main", "sourceKind": "branch-range" }));
        let unstaged = preview_request("/r", &Scope::Unstaged, Some("main"), false);
        assert_eq!(unstaged, json!({ "cwd": "/r", "ignoreWhitespace": false, "sourceKind": "working-tree" }));
        let commit = preview_request("/r", &Scope::Commit("abc".into()), None, true);
        assert_eq!(commit, json!({ "cwd": "/r", "ignoreWhitespace": true, "commitSha": "abc" }));
        assert_eq!(source_kind(&Scope::Commit("abc".into())), "commit");
    }

    #[test]
    fn turns_are_newest_first_once_each_and_a_gone_turn_falls_back_to_latest() {
        let rows = vec![
            json!({ "appRunOrdinal": 1, "capturedAt": "2026-01-01T09:00:00Z" }),
            json!({ "appRunOrdinal": 2, "capturedAt": "2026-01-01T10:00:00Z" }),
            json!({ "appRunOrdinal": 2, "capturedAt": "2026-01-01T10:05:00Z" }),
            json!({ "appRunOrdinal": null }),
        ];
        let turns = turn_list(&rows);
        assert_eq!(turns.iter().map(|t| t.count).collect::<Vec<_>>(), vec![2, 1]);
        assert_eq!(turns[0].at, data::iso_ms("2026-01-01T10:05:00Z").unwrap());
        assert_eq!(turn_of(&Scope::Turn(None), &turns), Some(2));
        assert_eq!(turn_of(&Scope::Turn(Some(1)), &turns), Some(1));
        assert_eq!(turn_of(&Scope::Turn(Some(9)), &turns), Some(2));
        assert_eq!((clock_at(0, true), clock_at(13 * 3_600_000 + 5 * 60_000, true)), ("12:00 AM".into(), "1:05 PM".into()));
        assert_eq!(clock_at(13 * 3_600_000 + 5 * 60_000, false), "13:05");
        assert_eq!(turn_of(&Scope::Branch, &turns), None);
    }

    #[test]
    fn a_cut_patch_refetches_the_files_it_lacks_or_cut_short() {
        let patch = "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1 +1,2 @@\n-x\n+y\n+z\ndiff --git a/b.ts b/b.ts\nnew file mode 100644\n--- /dev/null\n+++ b/b.ts\n@@ -0,0 +1,3 @@\n+1\n";
        let files = rune_kit::diff::parse(patch);
        let stat = |path: &str, additions, deletions| FileStat { path: path.into(), previous: None, additions, deletions };
        let meta = vec![stat("a.ts", 2, 1), stat("b.ts", 3, 0), stat("c.ts", 4, 0)];
        let paths: Vec<String> = incomplete(&files, &meta).into_iter().map(|f| f.path).collect();
        assert_eq!(paths, vec!["b.ts", "c.ts"]);
        let src = json!({ "files": [{ "path": "n.ts", "previousPath": "o.ts", "additions": 1, "deletions": 2 }] });
        assert_eq!(file_stats(&src), vec![FileStat { path: "n.ts".into(), previous: Some("o.ts".into()), additions: 1, deletions: 2 }]);
    }

    #[test]
    fn a_file_keeps_its_fold_while_out_of_the_list_and_newcomers_take_the_default() {
        let patch = |paths: &[&str]| rune_kit::diff::parse(&paths.iter().map(|p| format!("diff --git a/{p} b/{p}\n--- a/{p}\n+++ b/{p}\n@@ -1 +1 @@\n-x\n+y\n")).collect::<String>());
        let mut folds = HashMap::new();
        let mut state = DiffState::new(vec![]);
        let ab = patch(&["a.ts", "b.ts"]);
        let collapsed = collapsed_for(&mut folds, &state, false, &ab, true);
        state.set_files(ab, &collapsed);
        assert_eq!(state.collapsed_paths(), vec!["a.ts", "b.ts"]);
        state.set_collapsed(0, false);
        let b = patch(&["b.ts"]);
        let collapsed = collapsed_for(&mut folds, &state, true, &b, true);
        state.set_files(b, &collapsed);
        let abc = patch(&["a.ts", "b.ts", "c.ts"]);
        assert_eq!(collapsed_for(&mut folds, &state, true, &abc, true), vec!["b.ts", "c.ts"]);

        // Each thread's scope gets its folds back when it shows again.
        let mut ui = Ui { shown: Some(Scope::Branch), folds, ..Ui::default() };
        remember(&mut ui, "t1");
        recall(&mut ui, "t1", &Scope::Unstaged);
        assert!(ui.folds.is_empty() && !ui.touched);
        recall(&mut ui, "t1", &Scope::Branch);
        assert_eq!(ui.folds.get("a.ts"), Some(&false));
        recall(&mut ui, "t2", &Scope::Branch);
        assert!(ui.folds.is_empty());
    }

    #[test]
    fn test_files_match_web_and_generated_files_never_do() {
        for path in [
            "src/a.test.ts", "src/a.spec.tsx", "pkg/a_test.go", "tests/helpers.rs", "app/__tests__/x.js", "test_api.py",
            "conftest.py", "vitest.setup.ts", "src/FooTest.java", "src/FooSpec.scala", "a/__mocks__/b.ts", "x.snap",
            "src/user.fixture.json", "mockServer.ts", "evals/run.ts",
        ] {
            assert!(is_test_file(path), "{path}");
        }
        for path in ["src/main.rs", "src/latest.ts", "src/contest.py", "pnpm-lock.yaml", "dist/a.test.js", "src/mockingbird.ts", "src/footest.java"] {
            assert!(!is_test_file(path), "{path}");
        }
    }

    #[test]
    fn the_tree_joins_single_folder_chains_and_folds() {
        let paths = ["README.md", "apps/web/src/a.ts", "apps/web/src/b.ts", "apps/server/c.ts"];
        let rows = tree_rows(&paths, &HashSet::new());
        let shape: Vec<(u32, &str)> = rows.iter().map(|r| (r.depth, r.label.as_str())).collect();
        assert_eq!(shape, vec![(0, "README.md"), (0, "apps"), (1, "web/src"), (2, "a.ts"), (2, "b.ts"), (1, "server"), (2, "c.ts")]);
        assert_eq!(rows[3].file, Some(1));
        let folded = HashSet::from(["apps/web/src".to_string()]);
        let rows = tree_rows(&paths, &folded);
        assert_eq!(rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), vec!["README.md", "apps", "web/src", "server", "c.ts"]);
        assert_eq!(all_dirs(&paths), HashSet::from(["apps".to_string(), "apps/web/src".to_string(), "apps/server".to_string()]));
    }

    #[test]
    fn commit_links_follow_the_hosts_routes() {
        let sha = "ABCDEF1234";
        let github = json!({"provider": "github", "canonicalKey": "github.com/s5labs/demo", "locator": {"remoteUrl": "git@github.com:s5labs/demo.git"}});
        assert_eq!(commit_url(&github, sha).as_deref(), Some("https://github.com/s5labs/demo/commit/abcdef1234"));
        let gitlab = json!({"provider": "gitlab", "canonicalKey": "gitlab.example.com/group/sub/repo", "locator": {"remoteUrl": "https://me:secret@gitlab.example.com:8443/group/sub/repo.git"}});
        assert_eq!(commit_url(&gitlab, sha).as_deref(), Some("https://gitlab.example.com:8443/group/sub/repo/-/commit/abcdef1234"));
        let web = json!({"provider": "bitbucket", "canonicalKey": "bitbucket.org/team/repo", "webUrl": "https://bitbucket.org/team/repo/", "locator": {"remoteUrl": ""}});
        assert_eq!(commit_url(&web, sha).as_deref(), Some("https://bitbucket.org/team/repo/commits/abcdef1234"));
        assert_eq!(commit_url(&json!({"canonicalKey": "github.com/a/b", "locator": {"remoteUrl": ""}}), sha), None, "no provider, no link");
        assert_eq!(commit_url(&json!({"provider": "github", "canonicalKey": "github.com/a/b/c"}), sha), None);
        assert_eq!(commit_url(&Value::Null, sha), None);
    }

    #[test]
    fn counts_compact_like_web() {
        assert_eq!(compact(999), "999");
        assert_eq!(compact(1000), "1k");
        assert_eq!(compact(1260), "1.3k");
        assert_eq!(compact(12_400), "12k");
        assert_eq!(compact(999_600), "1m");
        assert_eq!(compact(2_000_000), "2m");
    }
}
