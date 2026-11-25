//! Add project: pick a source, then browse a folder, clone a repository or name a new project.
//! Mirrors CommandPalette's add-project flow and client-runtime's path helpers.
use super::{Action, Item, Trailing, item, keywords, push_page, set_query, toast};
use crate::{App, Route, data};
use rune::Tx;
use serde_json::{Value, json};

pub struct Provider {
    pub kind: &'static str,
    /// `pickerLabel`, for the source rows; `name` is the definition's `label`.
    pub label: &'static str,
    pub name: &'static str,
    pub hint: &'static str,
    https: bool,
    owner: Owner,
}
enum Owner {
    Always,
    WithAccount,
    Never,
}
/// The built-in source control definitions, in registry order.
pub const PROVIDERS: [Provider; 6] = [
    Provider { kind: "github", label: "GitHub", name: "GitHub", hint: "owner/repo", https: true, owner: Owner::Always },
    Provider { kind: "gitlab", label: "GitLab", name: "GitLab", hint: "group/project", https: false, owner: Owner::Always },
    Provider { kind: "gitcafe", label: "GitCafe", name: "GitCafe", hint: "owner/repo", https: false, owner: Owner::WithAccount },
    Provider { kind: "forgejo", label: "Forgejo / Gitea", name: "Forgejo", hint: "owner/repo", https: true, owner: Owner::WithAccount },
    Provider { kind: "bitbucket", label: "Bitbucket", name: "Bitbucket", hint: "workspace/repository", https: false, owner: Owner::Never },
    Provider { kind: "azure-devops", label: "Azure DevOps", name: "Azure DevOps", hint: "project/repository", https: false, owner: Owner::Never },
];

#[derive(Clone, PartialEq, Debug)]
pub enum Source {
    New,
    Existing,
    Browse,
    Clone(Option<usize>),
}

#[derive(Default)]
pub struct State {
    pub browse: Option<Browse>,
    pub discovery: Option<Value>,
    pub(super) discovery_pending: Option<u64>,
    pub clone: Option<CloneFlow>,
    pending: Option<(u64, Pending)>,
    pub publish: bool,
    /// The host picked for the new repository (`newProjectPublishKind`), by `PROVIDERS` index.
    pub publish_kind: Option<usize>,
    pub(super) host_open: bool,
    pub(super) published: Option<(u64, &'static str)>,
    /// The environment the flow adds the project to, picked when it started; every request and
    /// server setting it reads belongs to it, wherever focus moves meanwhile.
    pub env: Option<String>,
}
#[derive(Default)]
pub struct Browse {
    pub directory: String,
    pub(super) pending: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<String>,
}
pub struct CloneFlow {
    pub provider: Option<usize>,
    pub confirm: Option<Confirm>,
}
pub struct Confirm {
    pub input: String,
    pub name_with_owner: Option<String>,
    pub url: String,
    pub remote_url: String,
}
enum Pending {
    Add(String),
    Unarchive(Route),
    Lookup(String),
    Clone(String),
    Create(String, Option<usize>),
}

// Paths, as client-runtime's state/projects.ts reads them.

fn drive(v: &str) -> bool {
    let b = v.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b.len() == 2 || b[2] == b'/' || b[2] == b'\\')
}
fn windows_absolute(v: &str) -> bool {
    v.starts_with("\\\\") || drive(v)
}
fn unix(v: &str) -> bool {
    !windows_absolute(v) && v.starts_with('/')
}
fn separator(v: &str) -> char {
    if windows_absolute(v) {
        '\\'
    } else if unix(v) || !v.contains('\\') {
        '/'
    } else {
        '\\'
    }
}
pub fn trailing_separator(v: &str) -> bool {
    if unix(v) { v.ends_with('/') } else { v.ends_with('/') || v.ends_with('\\') }
}
fn last_separator(v: &str) -> Option<usize> {
    if unix(v) { v.rfind('/') } else { v.rfind(['/', '\\']) }
}
pub fn directory(v: &str) -> String {
    if trailing_separator(v) {
        return v.into();
    }
    last_separator(v).map(|i| v[..=i].to_string()).unwrap_or_else(|| v.into())
}
fn leaf(v: &str) -> &str {
    last_separator(v).map(|i| &v[i + 1..]).unwrap_or(v)
}
pub fn ensure_directory(v: &str) -> String {
    let v = v.trim();
    if v.is_empty() || trailing_separator(v) { v.into() } else { format!("{v}{}", separator(v)) }
}
fn append_segment(current: &str, segment: &str) -> String {
    format!("{}{segment}{}", directory(current), separator(current))
}
fn root(v: &str) -> bool {
    v == "/" || v == "\\" || (v.len() == 3 && drive(v))
}
fn normalize(v: &str) -> String {
    let mut v = v.trim();
    while v.len() > 1 && !root(v) && (v.ends_with('/') || v.ends_with('\\')) {
        v = &v[..v.len() - 1];
    }
    v.into()
}
fn compare(v: &str) -> String {
    let v = normalize(v);
    if windows_absolute(&v) { v.replace('/', "\\").to_lowercase() } else { v }
}
/// Root, separator and segments of an absolute path.
fn split_absolute(v: &str) -> Option<(String, char, Vec<&str>)> {
    if unix(v) {
        return Some(("/".into(), '/', v.split('/').filter(|s| !s.is_empty()).collect()));
    }
    if drive(v) {
        return Some((format!("{}\\", &v[..2]), '\\', v[2..].split(['/', '\\']).filter(|s| !s.is_empty()).collect()));
    }
    None
}
fn parent(current: &str) -> Option<String> {
    let trimmed = normalize(current);
    if let Some((root, separator, segments)) = split_absolute(&trimmed) {
        return match segments.len() {
            0 => None,
            1 => Some(root),
            n => Some(format!("{root}{}{separator}", segments[..n - 1].join(&separator.to_string()))),
        };
    }
    let i = last_separator(&trimmed)?;
    if i == 2 && drive(&trimmed) {
        return Some(format!("{}{}", &trimmed[..2], separator(current)));
    }
    Some(trimmed[..=i].into())
}
fn relative(v: &str) -> bool {
    v == "." || v == ".." || ["./", "../", ".\\", "..\\"].iter().any(|p| v.starts_with(p))
}
pub fn browse_query(v: &str, os: &str) -> bool {
    ["./", "../", ".\\", "..\\", "/", "~/"].iter().any(|p| v.starts_with(p))
        || (os.to_lowercase().starts_with("win") && windows_absolute(v))
}
fn resolve(v: &str, cwd: Option<&str>) -> String {
    let v = v.trim();
    let (Some(cwd), true) = (cwd, relative(v)) else {
        return normalize(v);
    };
    let cwd = normalize(cwd);
    let Some((root, separator, base)) = split_absolute(&cwd) else {
        return normalize(v);
    };
    let mut segments: Vec<&str> = base;
    for segment in v.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    format!("{root}{}", segments.join(&separator.to_string()))
}
fn title_from_path(v: &str) -> String {
    let v = normalize(v);
    v.split(['/', '\\']).rfind(|s| !s.is_empty()).unwrap_or(&v).to_string()
}
pub fn folder_name(name: &str) -> String {
    use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
    let mut slug = String::new();
    for c in name.nfkd().filter(|c| !is_combining_mark(*c)).flat_map(char::to_lowercase) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_start_matches('-').chars().take(64).collect();
    let slug = slug.trim_end_matches('-');
    let reserved = matches!(slug, "con" | "prn" | "aux" | "nul")
        || (slug.len() == 4 && (slug.starts_with("com") || slug.starts_with("lpt")) && matches!(slug.as_bytes()[3], b'1'..=b'9'));
    if slug.is_empty() {
        "project".into()
    } else if reserved {
        format!("{slug}-project")
    } else {
        slug.into()
    }
}
pub fn clone_directory_name(input: &str) -> String {
    let base = input.split(['?', '#']).next().unwrap_or("").trim();
    let scheme = base.find("://");
    let scp = {
        let at = base.find('@');
        let colon = base.find(':');
        matches!((at, colon), (Some(a), Some(c)) if a > 0 && c > a + 1 && !base[..c].contains(['/', '\\']))
    };
    let has_host = scheme.is_some() || scp;
    let path = scheme.map(|i| &base[i + 3..]).unwrap_or(base);
    let segments: Vec<&str> = path.split(['/', '\\', ':']).filter(|s| !s.trim().is_empty()).collect();
    if has_host && segments.len() < 2 {
        return String::new();
    }
    let last = segments.last().map(|s| s.trim()).unwrap_or("");
    if has_host && segments.len() == 2 && !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit()) {
        return String::new();
    }
    last.strip_suffix(".git").unwrap_or(last).into()
}
pub fn clone_destination(directory: &str, name: &str) -> String {
    let name = name.trim();
    if name.is_empty() { directory.into() } else { format!("{}{name}", ensure_directory(directory)) }
}
fn owner_segment(s: &str, underscore: bool) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b.len() <= 39 && b[0].is_ascii_alphanumeric()
        && b[1..].iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-' || (underscore && *c == b'_'))
}
/// `owner/name` reads as a repository on GitHub, the default host.
pub fn normalize_clone_url(input: &str) -> String {
    let input = input.trim();
    let shorthand = input.split_once('/').is_some_and(|(owner, name)| {
        owner_segment(owner, false)
            && !name.is_empty()
            && name.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    });
    if !shorthand {
        return input.into();
    }
    let path = if input.ends_with(".git") { input.into() } else { format!("{input}.git") };
    format!("https://github.com/{path}")
}
fn github_owner_only(source: Option<usize>, input: &str) -> bool {
    let input = input.trim();
    source == Some(0) && input.ends_with('/') && owner_segment(input.trim_end_matches('/'), true)
}

// Server state the flow reads.

fn target(s: &App) -> String {
    s.ui.shell.palette.add.env.clone().unwrap_or_else(|| s.ui.env.active.clone())
}
fn net(s: &mut App) -> Option<&mut s5_net::Net> {
    let key = target(s);
    crate::env::net_of(s, &key)
}
fn config(s: &App) -> &Value {
    static NULL: Value = Value::Null;
    let key = target(s);
    crate::env::source(s, &key).and_then(|src| src.state().config.as_ref()).unwrap_or(&NULL)
}
fn os(s: &App) -> String {
    config(s)["environment"]["platform"]["os"].as_str().unwrap_or("").into()
}
pub fn new_projects_root(s: &App) -> Option<String> {
    config(s)["newProjectsRoot"].as_str().filter(|v| !v.is_empty()).map(str::to_owned)
}
fn initial_query(s: &App) -> String {
    let base = config(s)["settings"]["addProjectBaseDirectory"].as_str().unwrap_or("").trim().to_owned();
    if base.is_empty() { "~/".into() } else { ensure_directory(&base) }
}
fn current_cwd(s: &App) -> Option<String> {
    let state = s.state();
    let (thread, project_id) = match &s.route {
        Route::Thread(id) => {
            let thread = state.threads.iter().find(|v| v["id"] == *id)?;
            (Some(thread), data::s(thread, "projectId").to_owned())
        }
        Route::Home { project_id: Some(id) } => (None, id.clone()),
        _ => return None,
    };
    if crate::env::owner(s, &project_id) != target(s) {
        return None;
    }
    if let Some(path) = thread.and_then(|v| v["worktreePath"].as_str()).filter(|v| !v.is_empty()) {
        return Some(path.into());
    }
    state.projects.iter().find(|v| v["id"] == project_id).map(|v| data::s(v, "workspaceRoot").to_owned())
}
fn readiness(s: &App, provider: usize) -> (bool, String) {
    let p = &PROVIDERS[provider];
    let entry = s.ui.shell.palette.add.discovery.as_ref().and_then(|d| {
        d["sourceControlProviders"].as_array().and_then(|v| v.iter().find(|v| v["kind"] == p.kind)).cloned()
    });
    let Some(entry) = entry else {
        // `buildAddProjectRemoteSourceReadiness`'s wording; web's case-sensitive rewrite below
        // leaves it and the unauthenticated fallback as they are.
        return (false, "Provider status unavailable. Open Source Control settings and rescan.".into());
    };
    let hint = if entry["status"] != "available" {
        data::s(&entry, "installHint").to_owned()
    } else if entry["auth"]["status"] == "unauthenticated" {
        option(&entry["auth"]["detail"]).unwrap_or_else(|| {
            format!("{} is not authenticated. Open Source Control settings for setup guidance.", data::s(&entry, "label"))
        })
    } else {
        return (true, String::new());
    };
    (false, hint.replace("Open source control settings", "Open Settings -> Source control"))
}
/// Effect's `Option` encodes as `{_tag:"Some",value}`; older payloads send the bare value.
fn option(v: &Value) -> Option<String> {
    v["value"].as_str().or(v.as_str()).map(str::to_owned)
}
/// `getNewProjectPublishTargets`: the hosts a new project can publish to, with their owner.
fn publish_targets(s: &App) -> Vec<(usize, Option<String>)> {
    (0..PROVIDERS.len())
        .filter(|i| readiness(s, *i).0)
        .filter_map(|i| {
            let account = s.ui.shell.palette.add.discovery.as_ref().and_then(|d| {
                d["sourceControlProviders"].as_array()?.iter().find(|v| v["kind"] == PROVIDERS[i].kind).and_then(|v| option(&v["auth"]["account"]))
            });
            match PROVIDERS[i].owner {
                Owner::Always => Some((i, account)),
                Owner::WithAccount => account.map(|a| (i, Some(a))),
                Owner::Never => None,
            }
        })
        .collect()
}
/// `getNewProjectPublishTarget`: the picked host while it can take one, else the first; and
/// whether there is a choice.
fn publish_target(s: &App) -> Option<(usize, Option<String>, bool)> {
    let targets = publish_targets(s);
    let kind = s.ui.shell.palette.add.publish_kind;
    let (i, owner) = targets.iter().find(|(i, _)| Some(*i) == kind).or(targets.first())?;
    Some((*i, owner.clone(), targets.len() > 1))
}
/// A new project whose repository can go to GitHub or GitLab, the host picker open.
pub(crate) fn scene_host(s: &mut App) {
    s.ui.shell.palette.open = true;
    start_new(s);
    let host = |kind: &str, account: &str| json!({"kind": kind, "status": "available", "auth": {"status": "authenticated", "account": account}});
    let add = &mut s.ui.shell.palette.add;
    add.discovery = Some(json!({"sourceControlProviders": [host("github", "octo"), host("gitlab", "octo")]}));
    add.publish = true;
    add.host_open = true;
    s.ui.shell.palette.query = rune::TextEdit::new("demo-app");
}
/// The host picker's choice.
pub(super) fn pick_host(s: &mut App, kind: &str) {
    let add = &mut s.ui.shell.palette.add;
    add.publish_kind = PROVIDERS.iter().position(|p| p.kind == kind);
    add.host_open = false;
}
fn new_repository(owner: &Option<String>, workspace_root: &str) -> String {
    let folder = workspace_root.split(['/', '\\']).rfind(|s| !s.is_empty()).unwrap_or("");
    match owner {
        Some(owner) => format!("{owner}/{folder}"),
        None => folder.into(),
    }
}

// What the view shows.

pub fn page_is_flow(page: &str) -> bool {
    page.starts_with("add-")
}
fn confirm_step(s: &App) -> bool {
    s.ui.shell.palette.page == "add-clone" && s.ui.shell.palette.add.clone.as_ref().is_some_and(|c| c.confirm.is_some())
}
pub fn repository_step(s: &App) -> bool {
    s.ui.shell.palette.page == "add-clone" && !confirm_step(s)
}
pub fn browsing(s: &App) -> bool {
    let ui = &s.ui.shell.palette;
    (matches!(ui.page.as_str(), "" | "add-project" | "add-browse") || confirm_step(s)) && browse_query(&ui.query.text, &os(s))
}
/// Views without a highlighted row until the arrows pick one, and without fuzzy ranking.
pub fn raw(s: &App) -> bool {
    browsing(s) || matches!(s.ui.shell.palette.page.as_str(), "add-browse" | "add-clone" | "add-new")
}
fn pinned_name(s: &App) -> String {
    s.ui.shell.palette.add.clone.as_ref().and_then(|c| c.confirm.as_ref())
        .map(|c| clone_directory_name(c.name_with_owner.as_deref().unwrap_or(&c.remote_url)))
        .unwrap_or_default()
}
pub struct View {
    pub entries: Vec<(String, String)>,
    pub exact: Option<String>,
    pub can_up: bool,
    pub needs_project: bool,
}
pub fn view(s: &App) -> View {
    let query = &s.ui.shell.palette.query.text;
    let browse = s.ui.shell.palette.add.browse.as_ref().filter(|b| b.directory == directory(query));
    let filter = if trailing_separator(query) { "" } else { leaf(query) };
    let pinned = pinned_name(s);
    let case_sensitive = !os(s).to_lowercase().starts_with("win");
    let same = |a: &str, b: &str| if case_sensitive { a == b } else { a.eq_ignore_ascii_case(b) };
    let visible_filter = if !pinned.is_empty() && same(filter, &pinned) { "" } else { filter };
    let lower = visible_filter.to_lowercase();
    let all: Vec<(String, String)> = browse
        .and_then(|b| b.result.as_ref())
        .and_then(|r| r["entries"].as_array())
        .into_iter()
        .flatten()
        .map(|e| (data::s(e, "name").to_owned(), data::s(e, "fullPath").to_owned()))
        .collect();
    let entries = all.iter()
        .filter(|(name, _)| name.to_lowercase().starts_with(&lower) && (visible_filter.starts_with('.') || !name.starts_with('.')))
        .cloned()
        .collect::<Vec<_>>();
    let exact = (!filter.is_empty())
        .then(|| {
            if pinned.is_empty() { entries.iter().find(|(n, _)| n == filter) } else { all.iter().find(|(n, _)| same(n, filter)) }
        })
        .flatten()
        .map(|(_, path)| path.clone());
    let needs_project = relative(query.trim()) && current_cwd(s).is_none();
    let can_up = !needs_project && trailing_separator(&directory(query)) && parent(&directory(query)).is_some();
    View { entries, exact, can_up, needs_project }
}
fn browse_result(s: &App) -> Option<&Value> {
    let query = &s.ui.shell.palette.query.text;
    s.ui.shell.palette.add.browse.as_ref().filter(|b| b.directory == directory(query)).and_then(|b| b.result.as_ref())
}
/// `browseAccess`: `None` while the target environment's session is checked.
fn browse_access(s: &App) -> Option<Result<(), String>> {
    crate::settings::permissions::access_or(s, &target(s), "filesystem:read", "This connection cannot browse host folders.")
}
pub fn browse_pending(s: &App) -> bool {
    if browse_access(s).is_none() {
        return true;
    }
    let query = &s.ui.shell.palette.query.text;
    s.ui.shell.palette.add.browse.as_ref().is_some_and(|b| b.directory == directory(query) && b.pending.is_some() && b.result.is_none())
}
pub fn browse_error(s: &App) -> Option<String> {
    if view(s).needs_project {
        return Some("Relative paths require an active project.".into());
    }
    if let Some(Err(denied)) = browse_access(s) {
        return Some(denied);
    }
    let query = &s.ui.shell.palette.query.text;
    s.ui.shell.palette.add.browse.as_ref().filter(|b| b.directory == directory(query)).and_then(|b| b.error.clone())
}
fn resolved_path(s: &App) -> String {
    let query = &s.ui.shell.palette.query.text;
    if trailing_separator(query) {
        browse_result(s).and_then(|r| r["parentPath"].as_str()).map(str::to_owned).unwrap_or_else(|| query.trim().into())
    } else {
        view(s).exact.unwrap_or_else(|| query.trim().into())
    }
}
/// The folder does not exist yet, so submitting creates it.
pub fn will_create(s: &App) -> bool {
    let ui = &s.ui.shell.palette;
    let query = ui.query.text.trim();
    if query.is_empty() || browse_pending(s) || browse_error(s).is_some() || ui.picked {
        return false;
    }
    if trailing_separator(&ui.query.text) { browse_result(s).is_none() } else { view(s).exact.is_none() }
}
pub fn busy(s: &App) -> bool {
    s.ui.shell.palette.add.pending.is_some()
}

/// Rows for the add-project views that skip fuzzy ranking; `None` for every other view.
pub fn raw_items(s: &App) -> Option<Vec<Item>> {
    if browsing(s) {
        let v = view(s);
        // `relativePathNeedsActiveProject` shows only its message.
        if v.needs_project {
            return Some(vec![]);
        }
        let mut rows = vec![];
        let group = if confirm_step(s) { "Select where to clone" } else { "Directories" };
        if v.can_up {
            let to = clone_destination(&parent(&directory(&s.ui.shell.palette.query.text)).unwrap_or_default(), &pinned_name(s));
            rows.push(item("..", group, &crate::icons::CORNER_LEFT_UP, "", Action::Browse(to)));
        }
        let pinned = pinned_name(s);
        let mut entries = v.entries;
        if !pinned.is_empty() {
            entries.sort_by_key(|(name, _)| *name != pinned);
        }
        for (name, _) in entries {
            let query = &s.ui.shell.palette.query.text;
            let to = if pinned.is_empty() {
                append_segment(query, &name)
            } else if name == pinned {
                append_segment(query, &name)
            } else {
                clone_destination(&append_segment(query, &name), &pinned)
            };
            rows.push(item(name, group, &crate::icons::FOLDER, "", Action::Browse(to)));
        }
        return Some(rows);
    }
    match s.ui.shell.palette.page.as_str() {
        "add-browse" | "add-clone" => Some(vec![]),
        "add-new" => Some(new_project_items(s)),
        _ => None,
    }
}
pub fn source_items(s: &App) -> Vec<Item> {
    let mut rows = vec![];
    if new_projects_root(s).is_some() {
        let mut v = item("New project", "Sources", &crate::icons::FOLDER_GIT_2, "", Action::Source(Source::New));
        v.description = Some("Start a new Git repository from a name".into());
        v.fields = keywords(&["new project", "create", "empty", "repository", "git init"]);
        rows.push(v);
    }
    let mut local = item("Local folder", "Sources", &crate::icons::FOLDER_PLUS, "", Action::Source(Source::Browse));
    local.description = Some("Browse a folder on disk".into());
    local.fields = keywords(&["local", "folder", "directory", "browse"]);
    rows.push(local);
    let mut url = item("Git URL", "Sources", &crate::icons::LINK, "", Action::Source(Source::Clone(None)));
    url.description = Some("Clone from a remote URL".into());
    url.fields = keywords(&["clone", "remote", "repository", "repo", "git", "Git URL"]);
    rows.push(url);
    let mut order: Vec<usize> = (0..PROVIDERS.len()).collect();
    order.sort_by_key(|i| (!readiness(s, *i).0, PROVIDERS[*i].label));
    for i in order {
        let p = &PROVIDERS[i];
        let (ready, hint) = readiness(s, i);
        let mut v = item(format!("{} repository", p.label), "Sources", &crate::icons::GIT_PULL_REQUEST_ARROW, "", Action::Source(Source::Clone(Some(i))));
        v.brand = Some(i);
        v.description = Some(format!("Clone {} {}", p.label, p.hint));
        let mut terms = vec!["clone", "remote", "repository", "repo", "git", p.label];
        if !ready {
            terms.push("setup required");
            v.disabled = true;
            v.trailing = Trailing::Setup(hint);
        }
        v.fields = keywords(&terms);
        rows.push(v);
    }
    rows
}
fn new_project_items(s: &App) -> Vec<Item> {
    let mut rows = vec![];
    if new_project_machines(s) {
        let selected = target(s);
        for key in new_project_options(s) {
            let local = key == crate::env::LOCAL;
            let label = option_label(s, &key);
            let mut v = item(label.clone(), "Environments", crate::settings::env_machine_icon(s, &key), "", Action::NewProjectIn(key.clone()));
            v.description = Some(if local { "This device".into() } else { key.clone() });
            v.fields = keywords(&[label.as_str(), key.as_str(), if local { "this device" } else { "" }]);
            if key == selected {
                v.trailing = Trailing::Picked;
            }
            v.disabled = busy(s);
            rows.push(v);
        }
    }
    let name = s.ui.shell.palette.query.text.trim().to_owned();
    if let (Some((i, owner, several)), Some(root)) = (publish_target(s), new_projects_root(s)) {
        let label = PROVIDERS[i].name;
        let title = if several { "Create private repository".to_string() } else { format!("Create private repository on {label}") };
        let mut v = item(title, "Options", &crate::icons::GIT_PULL_REQUEST_ARROW, "", Action::TogglePublish);
        v.brand = Some(i);
        v.description = Some(if name.is_empty() {
            owner.clone().unwrap_or_else(|| format!("Your {label} account"))
        } else {
            new_repository(&owner, &clone_destination(&root, &folder_name(&name)))
        });
        v.trailing = if several {
            Trailing::Host(publish_targets(s).into_iter().map(|t| t.0).collect(), i, s.ui.shell.palette.add.publish)
        } else {
            Trailing::Check(s.ui.shell.palette.add.publish)
        };
        v.disabled = busy(s);
        rows.push(v);
    }
    let mut existing = item("Add existing project", "", &crate::icons::FOLDER_PLUS, "", Action::Source(Source::Existing));
    existing.description = Some("Open a folder or clone a repository".into());
    existing.trailing = Trailing::Chevron;
    rows.push(existing);
    rows
}

/// `newProjectEnvironmentOptions`: connected environments with a new-projects root, this device
/// first, then by label.
pub fn new_project_options(s: &App) -> Vec<String> {
    let has_root = |key: &str| {
        crate::env::source(s, key)
            .and_then(|src| src.state().config.as_ref())
            .and_then(|c| c["newProjectsRoot"].as_str())
            .is_some_and(|root| !root.is_empty())
    };
    let mut keys: Vec<String> = crate::env::keys(s).into_iter().filter(|k| crate::env::connected(s, k) && has_root(k)).collect();
    keys.sort_by_key(|k| (k != crate::env::LOCAL, option_label(s, k)));
    keys
}

/// `showNewProjectMachines`: several environments can take it, or the picked one no longer can.
pub fn new_project_machines(s: &App) -> bool {
    let options = new_project_options(s);
    options.len() > 1 || (!options.is_empty() && !options.contains(&target(s)))
}

/// The summary's " on {label}" while the environment switcher shows.
pub fn new_project_suffix(s: &App) -> String {
    if new_project_machines(s) { format!(" on {}", option_label(s, &target(s))) } else { String::new() }
}

/// The root "New project": the first environment that can take one.
pub fn start_new(s: &mut App) {
    let Some(key) = new_project_options(s).into_iter().next() else { return };
    set_env(s, &key);
    choose(s, Source::New);
}

/// `switchNewProjectEnvironment`.
pub fn switch_new(s: &mut App, key: &str) {
    if busy(s) {
        return;
    }
    set_env(s, key);
    request_discovery(s);
}

// Transitions.

pub fn request_discovery(s: &mut App) {
    let add = &mut s.ui.shell.palette.add;
    if add.discovery.is_some() || add.discovery_pending.is_some() {
        return;
    }
    s.ui.shell.palette.add.discovery_pending = net(s).map(|net| net.request("server.discoverSourceControl", json!({})));
}
/// Where "Add project" starts (`resolveAddProjectEntry`): the environment list when several are
/// connected, the sources of the one that is, else the way to connect one.
pub fn open(s: &mut App) {
    s.ui.shell.palette.add.clone = None;
    let ready: Vec<String> = crate::env::keys(s).into_iter().filter(|k| crate::env::connected(s, k)).collect();
    match ready.as_slice() {
        [only] => open_in(s, only),
        _ => push_page(s, "add-environment", ""),
    }
}
/// The source list for adding a project to the environment `key`.
pub fn open_in(s: &mut App, key: &str) {
    set_env(s, key);
    s.ui.shell.palette.add.clone = None;
    push_page(s, "add-project", "");
    request_discovery(s);
}
fn set_env(s: &mut App, key: &str) {
    let add = &mut s.ui.shell.palette.add;
    if add.env.as_deref() != Some(key) {
        add.discovery = None;
        add.discovery_pending = None;
        add.browse = None;
    }
    add.env = Some(key.to_owned());
}
/// Whether any environment can take a new project (`canCreateProjectInEnvironment`).
pub fn any_connected(s: &App) -> bool {
    crate::env::keys(s).iter().any(|k| crate::env::connected(s, k))
}

/// `resolveEnvironmentOptionLabel`: this device's own label unless it is a generic one, else the
/// environment's runtime label, else its saved name.
fn option_label(s: &App, key: &str) -> String {
    let runtime = crate::env::source(s, key).and_then(|src| src.state().config.as_ref()).and_then(|c| c["environment"]["label"].as_str()).map(str::trim).filter(|l| !l.is_empty());
    if key == crate::env::LOCAL {
        let generic = |l: &&str| matches!(l.to_lowercase().as_str(), "local" | "local environment");
        return runtime.filter(|l| !generic(l)).unwrap_or("This device").to_owned();
    }
    runtime.map(str::to_owned).unwrap_or_else(|| Some(crate::env::label(s, key)).filter(|l| !l.is_empty()).unwrap_or_else(|| key.to_owned()))
}

/// The connected environments "Add project" can target, this device first, then by label, as
/// the web's `buildEnvironmentItem` rows.
pub fn environment_items(s: &App) -> Vec<Item> {
    let mut keys: Vec<String> = crate::env::keys(s).into_iter().filter(|k| crate::env::connected(s, k)).collect();
    if keys.is_empty() {
        let mut v = item("Connect an environment", "", &crate::icons::SETTINGS, "", Action::Navigate(crate::Route::Settings("connections".into())));
        v.description = Some("No connected environment can add a project".into());
        v.fields = keywords(&["connect", "environment", "connection", "settings"]);
        return vec![v];
    }
    keys.sort_by_key(|k| (k != crate::env::LOCAL, option_label(s, k)));
    keys.into_iter()
        .map(|key| {
            let local = key == crate::env::LOCAL;
            let label = option_label(s, &key);
            let mut v = item(label.clone(), "Environments", crate::settings::env_machine_icon(s, &key), "", Action::AddProjectIn(key.clone()));
            v.description = Some(if local { "This device".into() } else { key.clone() });
            v.fields = keywords(&[label.as_str(), key.as_str(), if local { "this device" } else { "" }]);
            v
        })
        .collect()
}

/// Whether picked folders can come from this device's file manager: only for this device's
/// own environment (`CP:2394`).
pub fn can_pick_locally(s: &App) -> bool {
    target(s) == crate::env::LOCAL
}
pub fn choose(s: &mut App, source: Source) {
    s.ui.shell.palette.add.clone = None;
    match source {
        Source::Existing => {
            super::pop_page(s);
            if s.ui.shell.palette.page != "add-project" {
                let key = target(s);
                open_in(s, &key);
            }
        }
        Source::New => {
            s.ui.shell.palette.add.publish = false;
            s.ui.shell.palette.add.publish_kind = None;
            push_page(s, "add-new", "");
            request_discovery(s);
        }
        Source::Browse => {
            let query = initial_query(s);
            push_page(s, "add-browse", &query);
            sync_browse(s);
        }
        Source::Clone(provider) => {
            s.ui.shell.palette.add.clone = Some(CloneFlow { provider, confirm: None });
            push_page(s, "add-clone", "");
        }
    }
}
/// Requests the listing for the query's directory once it changes.
pub fn sync_browse(s: &mut App) {
    if !browsing(s) {
        return;
    }
    let dir = directory(&s.ui.shell.palette.query.text);
    if s.ui.shell.palette.add.browse.as_ref().is_some_and(|b| b.directory == dir) || dir.trim().is_empty() {
        return;
    }
    // Asked again each poll, so the listing loads once the access check grants it.
    if !matches!(browse_access(s), Some(Ok(()))) {
        return;
    }
    let cwd = current_cwd(s);
    let mut payload = json!({"partialPath": dir.trim()});
    if let Some(cwd) = cwd {
        payload["cwd"] = cwd.into();
    }
    let pending = net(s).map(|net| net.request("filesystem.browse", payload));
    s.ui.shell.palette.add.browse = Some(Browse { directory: dir, pending, ..Default::default() });
}
/// Tab: into the highlighted folder, the only match, or the matches' common prefix.
pub fn complete(s: &mut App, highlighted: Option<Action>) -> bool {
    if !browsing(s) {
        return false;
    }
    if let Some(Action::Browse(to)) = highlighted {
        set_query(s, &to);
        sync_browse(s);
        return true;
    }
    let v = view(s);
    let query = s.ui.shell.palette.query.text.clone();
    let names: Vec<&str> = v.entries.iter().map(|(n, _)| n.as_str()).collect();
    match names.as_slice() {
        [] => {}
        [only] => set_query(s, &append_segment(&query, only)),
        [first, rest @ ..] => {
            let mut prefix = first.len();
            for name in rest {
                prefix = prefix.min(first.chars().zip(name.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum());
            }
            if prefix > leaf(&query).len() {
                set_query(s, &format!("{}{}", directory(&query), &first[..prefix]));
            }
        }
    }
    sync_browse(s);
    true
}
/// Enter, or the submit button: adds the browsed folder, clones, looks up or creates.
pub fn submit(s: &mut App) -> bool {
    if s.ui.shell.palette.page == "add-new" {
        create(s);
        return true;
    }
    if repository_step(s) {
        lookup(s);
        return true;
    }
    if !browsing(s) {
        return false;
    }
    if view(s).needs_project {
        return true;
    }
    let path = resolved_path(s);
    if !target_connected(s) {
        return true;
    }
    if confirm_step(s) {
        start_clone(s, &path);
    } else {
        add(s, &path);
    }
    true
}

/// The picked environment is still connected, else web's "Environment unavailable" toast.
fn target_connected(s: &mut App) -> bool {
    let key = target(s);
    if crate::env::connected(s, &key) {
        return true;
    }
    let label = option_label(s, &key);
    super::toast_error_brief(s, "Environment unavailable", Some(&format!("{label} is not connected.")));
    false
}
fn add(s: &mut App, raw: &str) {
    if busy(s) {
        return;
    }
    let raw = raw.trim();
    if raw.is_empty() {
        return;
    }
    let os = os(s);
    if windows_absolute(raw) && !os.to_lowercase().starts_with("win") {
        return toast(s, "Failed to add project", Some("Windows-style paths are only supported on Windows."));
    }
    let cwd = current_cwd(s);
    if relative(raw) && cwd.is_none() {
        return toast(s, "Failed to add project", Some("Relative paths require an active project."));
    }
    let path = resolve(raw, cwd.as_deref());
    let key = target(s);
    let existing = s.state().projects.iter().filter(|p| crate::env::of(s, p) == key).find(|p| compare(data::s(p, "workspaceRoot")) == compare(&path)).cloned();
    if let Some(project) = existing {
        let id = data::s(&project, "id").to_owned();
        let route = latest_thread(s, &id).map(Route::Thread).unwrap_or(Route::Home { project_id: Some(id.clone()) });
        let archived = !project["archivedAt"].is_null() || project["archived"] == true;
        if archived && let Some(net) = net(s) {
            let pending = net.request("projects.mutate", json!({"type":"project.update","commandId":s5_net::uuid(),"projectId":id,"archived":false}));
            s.ui.shell.palette.add.pending = Some((pending, Pending::Unarchive(route)));
            return;
        }
        s.ui.shell.palette.open = false;
        crate::shell::sidebar::go(s, route);
        return;
    }
    if !crate::shell::sidebar::grants_on(s, &key, "orchestration:operate") {
        return super::toast_error_brief(s, "Cannot add project", Some("This connection cannot add projects."));
    }
    let project_id = s5_net::uuid();
    let command = json!({
        "type": "project.create",
        "commandId": s5_net::uuid(),
        "projectId": project_id,
        "title": title_from_path(&path),
        "workspaceRoot": path,
        "createWorkspaceRootIfMissing": true,
        "defaultModelSelection": null,
    });
    if let Some(net) = net(s) {
        let pending = net.request("projects.mutate", command);
        s.ui.shell.palette.add.pending = Some((pending, Pending::Add(project_id)));
    }
}
fn latest_thread(s: &App, project: &str) -> Option<String> {
    let key = if crate::settings::threads_by_creation(s) { "createdAt" } else { "updatedAt" };
    s.state().threads.iter()
        .filter(|v| v["projectId"] == project && v["archivedAt"].is_null())
        .max_by_key(|v| data::ts(v, key).unwrap_or(0))
        .map(|v| data::s(v, "id").to_owned())
}
fn lookup(s: &mut App) {
    let Some(flow) = &s.ui.shell.palette.add.clone else { return };
    let provider = flow.provider;
    let input = s.ui.shell.palette.query.text.trim().to_owned();
    if input.is_empty() || busy(s) {
        return;
    }
    if github_owner_only(provider, &input) {
        return toast(s, "Repository name incomplete", Some("Enter the full owner/repository name."));
    }
    let Some(i) = provider else {
        let remote_url = normalize_clone_url(&input);
        confirm(s, Confirm { input, name_with_owner: None, url: remote_url.clone(), remote_url });
        return;
    };
    if let Some(net) = net(s) {
        let pending = net.request("sourceControl.lookupRepository", json!({"provider": PROVIDERS[i].kind, "repository": input}));
        s.ui.shell.palette.add.pending = Some((pending, Pending::Lookup(input)));
    }
}
pub fn confirm(s: &mut App, confirm: Confirm) {
    let name = clone_directory_name(confirm.name_with_owner.as_deref().unwrap_or(&confirm.input));
    let name = if name.is_empty() { clone_directory_name(&confirm.remote_url) } else { name };
    let query = clone_destination(&initial_query(s), &name);
    if let Some(flow) = &mut s.ui.shell.palette.add.clone {
        flow.confirm = Some(confirm);
    }
    set_query(s, &query);
    sync_browse(s);
}
fn start_clone(s: &mut App, destination: &str) {
    if s.ui.shell.palette.add.clone.as_ref().and_then(|c| c.confirm.as_ref()).is_none() {
        return;
    }
    if busy(s) || destination.trim().is_empty() {
        return;
    }
    let key = target(s);
    if !crate::shell::sidebar::grants_on(s, &key, "source-control:write") || !crate::shell::sidebar::grants_on(s, &key, "orchestration:operate") {
        return super::toast_error_brief(s, "Clone unavailable", Some("This connection needs permission to write source control and add projects."));
    }
    let Some(confirm) = s.ui.shell.palette.add.clone.as_ref().and_then(|c| c.confirm.as_ref()) else { return };
    let project_id = s5_net::uuid();
    let title = confirm.name_with_owner.as_deref()
        .and_then(|v| v.rsplit('/').next())
        .map(str::to_owned)
        .unwrap_or_else(|| title_from_path(destination));
    let payload = json!({
        "projectId": project_id,
        "title": title,
        "createdAt": iso_now(),
        "remoteUrl": confirm.remote_url,
        "destinationPath": resolve(destination, current_cwd(s).as_deref()),
    });
    if let Some(net) = net(s) {
        let pending = net.request("projectClone.start", payload);
        s.ui.shell.palette.add.pending = Some((pending, Pending::Clone(project_id)));
    }
}
pub(crate) fn iso_now() -> String {
    let ms = data::now_ms();
    let (days, rest) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let (h, m, sec, milli) = (rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60, rest % 1000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{d:02}T{h:02}:{m:02}:{sec:02}.{milli:03}Z")
}
fn create(s: &mut App) {
    let name = s.ui.shell.palette.query.text.trim().to_owned();
    if name.is_empty() || busy(s) {
        return;
    }
    let publish = s.ui.shell.palette.add.publish.then(|| publish_target(s).map(|t| t.0)).flatten();
    if let Some(net) = net(s) {
        let pending = net.request("projects.createNew", json!({"name": name}));
        s.ui.shell.palette.add.pending = Some((pending, Pending::Create(name, publish)));
    }
}
pub fn toggle_publish(s: &mut App) {
    s.ui.shell.palette.add.publish ^= true;
}
pub fn pick_folder(s: &mut App, tx: &mut Tx) {
    let query = s.ui.shell.palette.query.text.trim().to_owned();
    let initial = if trailing_separator(&s.ui.shell.palette.query.text) {
        browse_result(s).and_then(|r| r["parentPath"].as_str()).map(str::to_owned).unwrap_or(query.clone())
    } else {
        let dir = directory(&query);
        if dir.is_empty() { query.clone() } else { dir }
    };
    let initial = resolve(&initial, current_cwd(s).as_deref());
    let mut options = rune::FileDialogOptions { title: "Add project".into(), ..Default::default() };
    if initial.starts_with('/') || windows_absolute(&initial) {
        options.directory = Some(initial.into());
    }
    tx.open_folder(options, |s: &mut App, picked: rune::FileDialogResult, _| {
        if let Ok(Some(path)) = picked {
            add(s, &path.display().to_string());
        }
    });
}

/// Folds in browse, discovery and mutation replies.
pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    let Some(net) = s.src.net() else { return false };
    if let Some(browse) = &mut s.ui.shell.palette.add.browse
        && let Some(id) = browse.pending
        && let Some(reply) = net.take_reply(id)
    {
        browse.pending = None;
        match reply {
            Ok(v) => browse.result = Some(v),
            Err(e) => browse.error = Some(super::error_message(&e, "Could not read this folder.")),
        }
        changed = true;
    }
    if let Some(id) = s.ui.shell.palette.add.discovery_pending
        && let Some(reply) = net.take_reply(id)
    {
        s.ui.shell.palette.add.discovery_pending = None;
        s.ui.shell.palette.add.discovery = Some(reply.unwrap_or(json!({"sourceControlProviders": []})));
        changed = true;
    }
    if let Some((id, label)) = s.ui.shell.palette.add.published
        && let Some(reply) = net.take_reply(id)
    {
        s.ui.shell.palette.add.published = None;
        match reply {
            Ok(v) => toast(s, &format!("Published to {label}"), Some(data::s(&v["repository"], "nameWithOwner"))),
            Err(e) => toast(s, &format!("Could not create the {label} repository"), Some(&format!(
                "{} Use Publish Repository in the Git menu to try again.",
                super::error_message(&e, "An error occurred.")
            ))),
        }
        return true;
    }
    let Some((id, _)) = &s.ui.shell.palette.add.pending else { return changed };
    let Some(reply) = s.src.net().and_then(|net| net.take_reply(*id)) else { return changed };
    let Some((_, pending)) = s.ui.shell.palette.add.pending.take() else { return changed };
    match (pending, reply) {
        (Pending::Add(_), Err(e)) => toast(s, "Failed to add project", Some(&super::error_message(&e, "An error occurred."))),
        (Pending::Add(id), Ok(_)) | (Pending::Clone(id), Ok(_)) => finish(s, Route::Home { project_id: Some(id) }),
        (Pending::Unarchive(_), Err(e)) => toast(s, "Failed to add project", Some(&super::error_message(&e, "An error occurred."))),
        (Pending::Unarchive(route), Ok(_)) => finish(s, route),
        (Pending::Lookup(_), Err(e)) => toast(s, "Repository lookup failed", Some(&super::error_message(&e, "An error occurred."))),
        (Pending::Lookup(input), Ok(v)) => {
            let https = s.ui.shell.palette.add.clone.as_ref().and_then(|c| c.provider).is_some_and(|i| PROVIDERS[i].https);
            let url = data::s(&v, "url").to_owned();
            let remote_url = if https { url.clone() } else { data::s(&v, "sshUrl").to_owned() };
            confirm(s, Confirm { input, name_with_owner: Some(data::s(&v, "nameWithOwner").to_owned()), url, remote_url });
        }
        (Pending::Clone(_), Err(e)) => toast(s, "Clone failed", Some(&super::error_message(&e, "An error occurred."))),
        (Pending::Create(_, _), Err(e)) => toast(s, "Could not create the project", Some(&super::error_message(&e, "An error occurred."))),
        (Pending::Create(name, publish), Ok(v)) => {
            let root = data::s(&v, "workspaceRoot").to_owned();
            match v["commitError"].as_str() {
                None => toast(s, &format!("Created {name}"), Some(&root)),
                Some(error) => toast(s, &format!("Created {name} without a first commit"), Some(&format!("{error} The project is in {root}."))),
            }
            if let Some(i) = publish
                && let Some((_, owner)) = publish_targets(s).into_iter().find(|t| t.0 == i)
                && let Some(net) = self::net(s)
            {
                let pending = net.request("sourceControl.publishRepository", json!({
                    "cwd": root, "provider": PROVIDERS[i].kind, "repository": new_repository(&owner, &root), "visibility": "private",
                }));
                s.ui.shell.palette.add.published = Some((pending, PROVIDERS[i].name));
            }
            finish(s, Route::Home { project_id: Some(data::s(&v, "projectId").to_owned()) });
        }
    }
    true
}
fn finish(s: &mut App, route: Route) {
    if let Route::Home { project_id: Some(id) } = &route {
        let key = target(s);
        crate::env::adopt(s, id, &key);
    }
    s.ui.shell.palette.open = false;
    s.ui.shell.palette.add.clone = None;
    crate::shell::sidebar::go(s, route);
}

// Brand marks. Icons render as one-colour masks, so multi-colour marks stack a layer per colour.
// Bitbucket and Azure DevOps marks are gradients Rune cannot draw; they keep the generic glyph.

static GITHUB: rune::Icon = rune::Icon { name: "github", view: 16.0, body: "<path d=\"M8 0C3.58 0 0 3.58 0 8C0 11.54 2.29 14.53 5.47 15.59C5.87 15.66 6.02 15.42 6.02 15.21C6.02 15.02 6.01 14.39 6.01 13.72C4 14.09 3.48 13.23 3.32 12.78C3.23 12.55 2.84 11.84 2.5 11.65C2.22 11.5 1.82 11.13 2.49 11.12C3.12 11.11 3.57 11.7 3.72 11.94C4.44 13.15 5.59 12.81 6.05 12.6C6.12 12.08 6.33 11.73 6.56 11.53C4.78 11.33 2.92 10.64 2.92 7.58C2.92 6.71 3.23 5.99 3.74 5.43C3.66 5.23 3.38 4.41 3.82 3.31C3.82 3.31 4.49 3.1 6.02 4.13C6.66 3.95 7.34 3.86 8.02 3.86C8.7 3.86 9.38 3.95 10.02 4.13C11.55 3.09 12.22 3.31 12.22 3.31C12.66 4.41 12.38 5.23 12.3 5.43C12.81 5.99 13.12 6.7 13.12 7.58C13.12 10.65 11.25 11.33 9.47 11.53C9.76 11.78 10.01 12.26 10.01 13.01C10.01 14.08 10 14.94 10 15.21C10 15.42 10.15 15.67 10.55 15.59C13.71 14.53 16 11.53 16 8C16 3.58 12.42 0 8 0Z\" fill=\"currentColor\" fill-rule=\"evenodd\"/>" };
static GITLAB_RED: rune::Icon = rune::Icon { name: "gitlab-a", view: 32.0, body: "<path d=\"m31.46 12.78-.04-.12-4.35-11.35A1.14 1.14 0 0 0 25.94.6c-.24 0-.47.1-.66.24-.19.15-.33.36-.39.6l-2.94 9h-11.9l-2.94-9A1.14 1.14 0 0 0 6.07.58a1.15 1.15 0 0 0-1.14.72L.58 12.68l-.05.11a8.1 8.1 0 0 0 2.68 9.34l.02.01.04.03 6.63 4.97 3.28 2.48 2 1.52a1.35 1.35 0 0 0 1.62 0l2-1.52 3.28-2.48 6.67-5h.02a8.09 8.09 0 0 0 2.7-9.36Z\" fill=\"currentColor\"/>" };
static GITLAB_ORANGE: rune::Icon = rune::Icon { name: "gitlab-b", view: 32.0, body: "<path d=\"m31.46 12.78-.04-.12a14.75 14.75 0 0 0-5.86 2.64l-9.55 7.24 6.09 4.6 6.67-5h.02a8.09 8.09 0 0 0 2.67-9.36Z\" fill=\"currentColor\"/><path d=\"M6.44 15.3a14.71 14.71 0 0 0-5.86-2.63l-.05.12a8.1 8.1 0 0 0 2.68 9.34l.02.01.04.03 6.63 4.97 6.1-4.6-9.56-7.24Z\" fill=\"currentColor\"/>" };
static GITLAB_YELLOW: rune::Icon = rune::Icon { name: "gitlab-c", view: 32.0, body: "<path d=\"m9.9 27.14 3.28 2.48 2 1.52a1.35 1.35 0 0 0 1.62 0l2-1.52 3.28-2.48-6.1-4.6-6.07 4.6Z\" fill=\"currentColor\"/>" };
static GITCAFE: rune::Icon = rune::Icon { name: "gitcafe", view: 64.0, body: "<path d=\"M4.8 4.8h54.4v54.4H4.8z\" fill=\"currentColor\"/>" };
static FORGEJO_ORANGE: rune::Icon = rune::Icon { name: "forgejo-a", view: 212.0, body: "<path d=\"M64 174 v-98 a50 50 0 0 1 50-50 h20\" stroke=\"currentColor\" stroke-width=\"25\" fill=\"none\"/><path d=\"M130 26 a18 18 0 1 0 36 0 a18 18 0 1 0 -36 0z\" stroke=\"currentColor\" stroke-width=\"15\" fill=\"none\"/>" };
static FORGEJO_RED: rune::Icon = rune::Icon { name: "forgejo-b", view: 212.0, body: "<path d=\"M64 174 v-30 a50 50 0 0 1 50-50 h20\" stroke=\"currentColor\" stroke-width=\"25\" fill=\"none\"/><path d=\"M130 94 a18 18 0 1 0 36 0 a18 18 0 1 0 -36 0z\" stroke=\"currentColor\" stroke-width=\"15\" fill=\"none\"/><path d=\"M46 186 a18 18 0 1 0 36 0 a18 18 0 1 0 -36 0z\" stroke=\"currentColor\" stroke-width=\"15\" fill=\"none\"/>" };

fn rgb(v: &str) -> rune::Color {
    rune::color::hex(v).unwrap_or(rune::Color::BLACK)
}
/// A provider's mark at `size`, or `None` for the generic fallback glyph.
static BITBUCKET_BODY: rune::Icon = rune::Icon { name: "bitbucket-a", view: 2490.0, body: "<path d=\"M88.92,14.4C45.02,13.83,8.97,48.96,8.41,92.86c-0.06,4.61,0.28,9.22,1.02,13.77l337.48,2048.72 c8.68,51.75,53.26,89.8,105.74,90.24h1619.03c39.38,0.5,73.19-27.9,79.49-66.78l337.49-2071.78c7.03-43.34-22.41-84.17-65.75-91.2 c-4.55-0.74-9.15-1.08-13.76-1.02L88.92,14.4z M1509.99,1495.09H993.24l-139.92-731h781.89L1509.99,1495.09z\" fill=\"currentColor\"/>" };
static BITBUCKET_FOLD: rune::Icon = rune::Icon { name: "bitbucket-b", view: 2490.0, body: "<path d=\"M2379.27,763.06h-745.5l-125.12,730.42H992.31l-609.67,723.67c19.32,16.71,43.96,26,69.5,26.21h1618.13 c39.35,0.51,73.14-27.88,79.44-66.72L2379.27,763.06z\" fill=\"currentColor\"/>" };
static AZURE_LEFT: rune::Icon = rune::Icon { name: "azure-devops-a", view: 512.0, body: "<path d=\"M116.1 73.4c-5.8 5.8-13.7 13.8-13.7 22.6v216.4c0 8.9-3.3 17.3-9 23.4s-13.3 9.2-21.1 8.5l-42.7-3.1c-7.9-.5-15-3.9-20.3-9.3-5.7-5.7-9.3-13.7-9.3-22.6V202.7c0-8.9 3.6-16.9 9.4-22.6l2-1.9z\" fill=\"currentColor\"/>" };
static AZURE_BOTTOM: rune::Icon = rune::Icon { name: "azure-devops-b", view: 512.0, body: "<path d=\"m500.5 366-128 106.7c-5.5 4.6-12.7 7.4-20.5 7.4q-2.4 0-4.8-.3l-112.5-16.9v27.8c0 11.7-9.6 21.3-21.3 21.3-7.3 0-13.5-3.6-17.5-9.1l-36.2-51.4-5.4-7.6-36.6-51.9-7-10c-2.6-3.5-3.9-7.8-3.9-12.3 0-6 2.5-11.6 6.8-15.7 3.8-3.7 9.1-5.8 14.5-5.8.4 0 1 .1 1.5.1l105.2 7.5L384 366.5l93.7 6.7c.7.1 1.6.1 2.3.1 7.8 0 14.9-2.7 20.5-7.3\" fill=\"currentColor\"/>" };
static AZURE_RIGHT: rune::Icon = rune::Icon { name: "azure-devops-c", view: 512.0, body: "<path d=\"M512 170.7v170.7c0 9.9-4.5 18.8-11.5 24.6l-128 106.7c7-5.9 11.5-14.7 11.5-24.6V175.3c0-16.7 13-30.7 29.8-31.9l63.9-4.6c.7-.1 1.6-.1 2.3-.1 7.8 0 14.9 2.8 20.5 7.4 7 5.8 11.5 14.6 11.5 24.6\" fill=\"currentColor\"/>" };
static AZURE_TOP: rune::Icon = rune::Icon { name: "azure-devops-d", view: 512.0, body: "<path d=\"M384 125.5c0 11.2-8.6 20.5-19.8 21.3-.5 0-1.1.1-1.6.1l-127.9 9.2-132.3 9.1-72.7 5.5c-6.9.4-13.2 3.1-18.2 7.4L116.1 73.4c4.7-4.7 10.9-8 17.8-9.1l100.8-15.1V21.3C234.7 9.6 244.3 0 256 0c5.8 0 11.1 2.3 14.9 6.1l33.4 32.6 16.9 16.5 40.3 39.3 16.1 15.8c4.1 3.9 6.4 9.4 6.4 15.2\" fill=\"currentColor\"/>" };
pub fn brand(provider: usize, size: f32, muted: rune::Color) -> Option<rune::El> {
    let layers: Vec<(&'static rune::Icon, rune::Color)> = match PROVIDERS[provider].kind {
        "github" => vec![(&GITHUB, muted)],
        "gitlab" => vec![(&GITLAB_RED, rgb("#E24329")), (&GITLAB_ORANGE, rgb("#FC6D26")), (&GITLAB_YELLOW, rgb("#FCA326"))],
        "gitcafe" => vec![(&GITCAFE, rgb("#a78bfa"))],
        "forgejo" => vec![(&FORGEJO_ORANGE, rgb("#ff6600")), (&FORGEJO_RED, rgb("#d40000"))],
        // The marks' gradients flattened to one stop per layer.
        "bitbucket" => vec![(&BITBUCKET_BODY, rgb("#2684FF")), (&BITBUCKET_FOLD, rgb("#0052CC"))],
        "azure-devops" => vec![(&AZURE_LEFT, rgb("#2052cb")), (&AZURE_BOTTOM, rgb("#2052cb")), (&AZURE_RIGHT, rgb("#367af2")), (&AZURE_TOP, rgb("#0fafff"))],
        _ => return None,
    };
    let mut mark = rune::el().w(size).h(size);
    for (glyph, color) in layers {
        mark = mark.child(rune::icon(glyph, size).color(color).absolute(0., 0.));
    }
    Some(mark)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_new_repository_goes_to_the_picked_host_with_its_owner() {
        let mut s = App::new();
        let host = |kind: &str, account: &str| json!({"kind": kind, "status": "available", "auth": {"status": "authenticated", "account": account}});
        s.ui.shell.palette.add.discovery = Some(json!({"sourceControlProviders": [host("gitlab", "lab-me"), host("github", "octo")]}));
        let (first, owner, several) = publish_target(&s).expect("a host");
        assert_eq!((PROVIDERS[first].kind, owner.as_deref(), several), ("github", Some("octo"), true));
        s.ui.shell.palette.add.host_open = true;
        pick_host(&mut s, "gitlab");
        assert!(!s.ui.shell.palette.add.host_open, "FAILED: picking closes the picker");
        let (picked, owner, _) = publish_target(&s).expect("a host");
        assert_eq!((PROVIDERS[picked].kind, owner.as_deref()), ("gitlab", Some("lab-me")), "FAILED: the picked host and its own account");
        s.ui.shell.palette.add.discovery = Some(json!({"sourceControlProviders": [host("github", "octo")]}));
        let (only, _, several) = publish_target(&s).expect("a host");
        assert_eq!((PROVIDERS[only].kind, several), ("github", false), "FAILED: a host that can no longer publish falls back to the first");
    }
    #[test]
    fn a_readded_project_opens_its_latest_thread_by_the_sort_order() {
        let mut s = App::new();
        if let crate::data::Source::Fixture(state) = &mut s.src {
            state.threads = vec![
                json!({"id": "old", "projectId": "p", "archivedAt": null, "createdAt": "2026-01-01T00:00:00.000Z", "updatedAt": "2026-03-01T00:00:00.000Z"}),
                json!({"id": "new", "projectId": "p", "archivedAt": null, "createdAt": "2026-02-01T00:00:00.000Z", "updatedAt": "2026-02-02T00:00:00.000Z"}),
            ];
        }
        assert_eq!(latest_thread(&s, "p").as_deref(), Some("old"));
        s.client_settings.values.insert("sidebarThreadSortOrder".into(), json!("created_at"));
        assert_eq!(latest_thread(&s, "p").as_deref(), Some("new"), "FAILED: created_at picks the newest thread");
    }

    #[test]
    fn add_project_asks_which_environment_and_the_flow_stays_on_it() {
        let ready = |root: &str| {
            let mut state = crate::data::empty_state();
            state.conn = s5_net::Conn::Open;
            state.shell_ready = true;
            state.config = Some(json!({"newProjectsRoot": root}));
            state
        };
        let mut s = App::new();
        s.src = crate::data::Source::Fixture(ready("/home/me/new"));
        s.ui.env.saved.push(crate::env::Saved { id: "box".into(), label: "Box".into(), base: "https://box".into(), bearer: "t".into() });
        crate::env::park(&mut s, "box", crate::data::Source::Fixture(ready("/srv/new")));

        crate::shell::palette::open_add_project(&mut s);
        assert_eq!(s.ui.shell.palette.page, "add-environment");
        let rows = environment_items(&s);
        assert_eq!(rows.iter().map(|r| r.description.clone().unwrap_or_default()).collect::<Vec<_>>(), ["This device", "box"]);
        open_in(&mut s, "box");
        assert_eq!(s.ui.env.active, crate::env::LOCAL, "picking an environment does not move focus");
        assert_eq!(new_projects_root(&s).as_deref(), Some("/srv/new"), "the flow reads the picked environment's settings");

        if let Some(crate::data::Source::Fixture(state)) = crate::env::source_mut(&mut s, "box") {
            state.conn = s5_net::Conn::Closed("gone".into());
        }
        crate::shell::palette::open_add_project(&mut s);
        assert_eq!(s.ui.shell.palette.page, "add-project", "one connected environment skips the list");
        assert_eq!(new_projects_root(&s).as_deref(), Some("/home/me/new"));
        assert_eq!(environment_items(&s).len(), 1, "FAILED: a disconnected environment is not listed, as on web");
        s.ui.shell.palette.query = rune::TextEdit::new("~/projects/");
        assert_eq!(browse_error(&s), None);
        crate::settings::permissions::set_session(&mut s, crate::env::LOCAL, json!({"authenticated": true, "permissions": ["orchestration:read"]}));
        assert_eq!(browse_error(&s).as_deref(), Some("This connection cannot browse host folders."));
        assert!(!browse_pending(&s), "FAILED: a denied session is an answer, not a wait");

        let mut closed = ready("/home/me/new");
        closed.conn = s5_net::Conn::Closed("gone".into());
        s.src = crate::data::Source::Fixture(closed);
        crate::shell::palette::open_add_project(&mut s);
        let rows = environment_items(&s);
        assert_eq!(s.ui.shell.palette.page, "add-environment");
        assert_eq!(rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["Connect an environment"]);
        assert!(matches!(&rows[0].action, Action::Navigate(crate::Route::Settings(p)) if p == "connections"));
    }
    #[test]
    fn new_project_starts_on_the_first_environment_and_switches_between_them() {
        let ready = |root: &str| {
            let mut state = crate::data::empty_state();
            state.conn = s5_net::Conn::Open;
            state.shell_ready = true;
            state.config = Some(json!({"newProjectsRoot": root}));
            state
        };
        let mut s = App::new();
        s.src = crate::data::Source::Fixture(ready("/home/me/new"));
        assert_eq!(new_project_options(&s), [crate::env::LOCAL]);
        assert!(!new_project_machines(&s), "FAILED: one environment needs no switcher");
        s.ui.env.saved.push(crate::env::Saved { id: "box".into(), label: "Box".into(), base: "https://box".into(), bearer: "t".into() });
        crate::env::park(&mut s, "box", crate::data::Source::Fixture(ready("/srv/new")));
        crate::shell::palette::reset(&mut s, true);
        start_new(&mut s);
        assert_eq!((s.ui.shell.palette.page.as_str(), target(&s)), ("add-new", crate::env::LOCAL.to_owned()));
        let rows = new_project_items(&s);
        let shape: Vec<_> = rows.iter().map(|r| (r.group, r.title.as_str(), matches!(r.trailing, Trailing::Picked))).collect();
        assert_eq!(shape, [("Environments", "This device", true), ("Environments", "Box", false), ("", "Add existing project", false)]);
        assert_eq!(new_project_suffix(&s), " on This device");
        switch_new(&mut s, "box");
        assert_eq!(new_projects_root(&s).as_deref(), Some("/srv/new"), "FAILED: the picked environment's root");
        assert_eq!(new_project_suffix(&s), " on Box");
        assert_eq!(s.ui.shell.palette.page, "add-new", "FAILED: switching stays on the page");
    }
    #[test]
    fn browse_paths_follow_client_runtime() {
        assert_eq!(directory("~/projects/s5"), "~/projects/");
        assert_eq!(directory("~/projects/"), "~/projects/");
        assert_eq!(leaf("~/projects/s5"), "s5");
        assert_eq!(append_segment("~/pro", "projects"), "~/projects/");
        assert_eq!(parent("/home/ubuntu/"), Some("/home/".into()));
        assert_eq!(parent("/home/"), Some("/".into()));
        assert_eq!(parent("/"), None);
        assert_eq!(parent("~/projects/"), Some("~/".into()));
        assert_eq!(parent("~/"), None);
        assert_eq!(parent("C:\\Users\\me\\"), Some("C:\\Users\\".into()));
        assert_eq!(ensure_directory(" /srv "), "/srv/");
        assert!(browse_query("~/", "linux") && browse_query("/", "linux") && browse_query("./src", "linux"));
        assert!(!browse_query("C:\\", "linux") && browse_query("C:\\", "windows"));
        assert!(!browse_query("settings", "linux"));
        assert_eq!(resolve("../web", Some("/repo/apps/server")), "/repo/apps/web");
        assert_eq!(resolve("/srv/app/", None), "/srv/app");
        assert_eq!(title_from_path("/home/me/my-app/"), "my-app");
        assert_eq!(compare("C:/Users/Me/"), "c:\\users\\me");
    }
    #[test]
    fn clone_names_and_urls() {
        assert_eq!(clone_directory_name("https://github.com/acme/widgets.git"), "widgets");
        assert_eq!(clone_directory_name("git@github.com:acme/widgets.git"), "widgets");
        assert_eq!(clone_directory_name("acme/widgets"), "widgets");
        assert_eq!(clone_directory_name("https://github.com"), "");
        assert_eq!(clone_directory_name("https://host/123"), "");
        assert_eq!(clone_destination("~/code", "widgets"), "~/code/widgets");
        assert_eq!(normalize_clone_url("acme/widgets"), "https://github.com/acme/widgets.git");
        assert_eq!(normalize_clone_url("git@gitlab.com:a/b.git"), "git@gitlab.com:a/b.git");
        assert!(github_owner_only(Some(0), "acme/"));
        assert!(!github_owner_only(Some(0), "acme/widgets"));
        assert!(!github_owner_only(None, "acme/"));
        assert_eq!(folder_name("Café Déjà Vu!"), "cafe-deja-vu");
        assert_eq!(folder_name("!!!"), "project");
        assert_eq!(folder_name("con"), "con-project");
        assert_eq!(iso_now().len(), 24);
    }
}
