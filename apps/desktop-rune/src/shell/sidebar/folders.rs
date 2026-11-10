//! Project grouping (`buildProjectGroups`, `sidebarProjectGroupingMode`) and the boring sidebar's
//! project folders (`boringSidebarEnabled`, `SidebarProjectFolderHeader`): active threads sit under
//! one folder per project group, at the Active section's place.

use std::collections::{HashMap, HashSet};

use rune::{Cursor, El, PointerPress, Tx, col, el, icon, row, text};
use serde_json::{Value, json};

use super::Model;
use super::logic::{Status, Thread};
use crate::data::{iso_ms, s as field};
use crate::icons as i;
use crate::kit::{self, MenuItem};
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route};

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub key: String,
    pub label: String,
    /// Member project ids, in server order.
    pub members: Vec<String>,
    pub representative: String,
    pub root: String,
    pub pinned: bool,
    pub archived: bool,
}

pub struct Grouping {
    mode: String,
    overrides: Value,
    environment: String,
}

impl Grouping {
    pub fn read(s: &App) -> Grouping {
        let environment = s.state().config.as_ref().map(|c| field(&c["environment"], "environmentId").to_string()).unwrap_or_default();
        Grouping {
            mode: crate::settings::value(s, "sidebarProjectGroupingMode").as_str().unwrap_or("repository").to_string(),
            overrides: crate::settings::value(s, "sidebarProjectGroupingOverrides"),
            environment,
        }
    }

    /// `deriveLogicalProjectKey` with the project's override, if any.
    pub fn key(&self, project: &Value) -> String {
        let physical = format!("{}:{}", self.environment, normalize(field(project, "workspaceRoot")));
        let mode = self.overrides[&physical].as_str().unwrap_or(&self.mode);
        if mode == "separate" {
            return physical;
        }
        let identity = &project["repositoryIdentity"];
        let canonical = identity["origin"]["canonicalKey"].as_str().or(identity["canonicalKey"].as_str()).map(str::trim);
        let Some(canonical) = canonical.filter(|k| !k.is_empty() && !k.contains('\0')) else { return physical };
        match relative_path(project).filter(|_| mode == "repository_path") {
            Some(rel) if !rel.is_empty() => format!("{canonical}::{rel}"),
            _ => canonical.to_string(),
        }
    }
}

/// `normalizeProjectPathForComparison`: no trailing separators.
fn normalize(path: &str) -> String {
    let path = path.trim();
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() { path.chars().take(1).collect() } else { trimmed.to_string() }
}

fn relative_path(project: &Value) -> Option<String> {
    let root = normalize(project["repositoryIdentity"]["rootPath"].as_str()?);
    let path = normalize(field(project, "workspaceRoot"));
    if root.is_empty() || path.is_empty() {
        return None;
    }
    if root == path {
        return Some(String::new());
    }
    let separator = if root.contains('\\') { '\\' } else { '/' };
    path.strip_prefix(&format!("{root}{separator}")).map(|rel| rel.replace('\\', "/"))
}

fn unique<'a>(values: impl Iterator<Item = Option<&'a str>>) -> Vec<&'a str> {
    let mut out: Vec<&str> = vec![];
    for v in values.flatten().map(str::trim).filter(|v| !v.is_empty()) {
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

/// `deriveProjectGroupLabel`: a shared title, else the repository's display name, else its name.
fn label(members: &[&Value]) -> String {
    let titles = unique(members.iter().map(|p| p["title"].as_str()));
    let display = unique(members.iter().map(|p| {
        let identity = &p["repositoryIdentity"];
        if identity["origin"].is_object() { identity["origin"]["displayName"].as_str().or(identity["origin"]["canonicalKey"].as_str()) } else { identity["displayName"].as_str() }
    }));
    let names = unique(members.iter().map(|p| p["repositoryIdentity"]["name"].as_str()));
    match (titles.as_slice(), display.as_slice(), names.as_slice()) {
        ([title], ..) if !display.contains(title) && !names.contains(title) => title.to_string(),
        (_, [name], _) | (_, _, [name]) => name.to_string(),
        _ => field(members[0], "title").to_string(),
    }
}

/// `buildProjectGroups` for one environment: groups in first-member order.
pub fn groups(projects: &[Value], grouping: &Grouping) -> Vec<Group> {
    let mut order: Vec<String> = vec![];
    let mut members: HashMap<String, Vec<&Value>> = HashMap::new();
    for p in projects {
        let key = grouping.key(p);
        if !members.contains_key(&key) {
            order.push(key.clone());
        }
        members.entry(key).or_default().push(p);
    }
    order
        .into_iter()
        .map(|key| {
            let list = &members[&key];
            Group {
                label: if list.len() == 1 { field(list[0], "title").to_string() } else { label(list) },
                members: list.iter().map(|p| field(p, "id").to_string()).collect(),
                representative: field(list[0], "id").to_string(),
                root: field(list[0], "workspaceRoot").to_string(),
                pinned: list.iter().any(|p| !p["pinnedAt"].is_null()),
                archived: list.iter().all(|p| !p["archivedAt"].is_null()),
                key,
            }
        })
        .collect()
}

/// The grouping of the current settings, keyed by member project id.
pub fn group_of(s: &App) -> (Vec<Group>, HashMap<String, usize>) {
    let groups = groups(&s.state().projects, &Grouping::read(s));
    let by_project = groups.iter().enumerate().flat_map(|(n, g)| g.members.iter().map(move |m| (m.clone(), n))).collect();
    (groups, by_project)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Folder {
    pub group: Group,
    /// Active rows (indices into `Model::threads`), newest first.
    pub rows: Vec<usize>,
}

pub struct Plan<'a> {
    pub groups: &'a [Group],
    pub projects: &'a [Value],
    pub threads: &'a [Thread],
    pub active: &'a [usize],
    /// Group keys the list is scoped to; empty for all.
    pub scope: &'a HashSet<String>,
    pub sort: &'a str,
}

/// `max(createdAt, latestUserAuthoredMessageAt ?? latestRun.requestedAt)`.
fn folder_time(t: &Thread) -> i64 {
    let touched = t.latest_user_authored_at.flatten().or(t.run.as_ref().and_then(|r| r.requested_at));
    touched.map_or(t.created_at, |at| at.max(t.created_at))
}

/// `getProjectSortTimestamp` over the group's threads, else the project's own time.
fn project_time(p: &Plan, group: &Group) -> i64 {
    let created = p.sort == "created_at";
    let threads = p.threads.iter().filter(|t| !t.archived && group.members.contains(&t.project_id));
    let latest = threads.map(|t| if created { t.created_at } else { t.latest_user_message_at.unwrap_or(t.created_at) }).max();
    latest.unwrap_or_else(|| {
        let project = p.projects.iter().find(|v| field(v, "id") == group.representative);
        let at = project.and_then(|v| if created { v["createdAt"].as_str() } else { v["updatedAt"].as_str().or(v["createdAt"].as_str()) });
        at.and_then(iso_ms).unwrap_or(i64::MIN)
    })
}

/// The folder list: open groups pinned first then by Project order, then archived groups that
/// still hold active threads. Empty folders show only when scoped or pinned.
pub fn plan(p: &Plan) -> Vec<Folder> {
    let index: HashMap<&str, usize> = p.groups.iter().enumerate().flat_map(|(n, g)| g.members.iter().map(move |m| (m.as_str(), n))).collect();
    let mut rows: Vec<Vec<usize>> = vec![vec![]; p.groups.len()];
    for &i in p.active {
        if let Some(&n) = index.get(p.threads[i].project_id.as_str()) {
            rows[n].push(i);
        }
    }
    for list in &mut rows {
        list.sort_by(|a, b| folder_time(&p.threads[*b]).cmp(&folder_time(&p.threads[*a])).then_with(|| p.threads[*a].id.cmp(&p.threads[*b].id)));
    }
    let mut open: Vec<usize> = (0..p.groups.len()).filter(|n| !p.groups[*n].archived).collect();
    if p.sort != "manual" {
        let times: Vec<i64> = p.groups.iter().map(|g| project_time(p, g)).collect();
        open.sort_by(|a, b| times[*b].cmp(&times[*a]).then_with(|| p.groups[*a].label.cmp(&p.groups[*b].label)).then_with(|| p.groups[*a].key.cmp(&p.groups[*b].key)));
    }
    open.sort_by_key(|n| !p.groups[*n].pinned);
    open.extend((0..p.groups.len()).filter(|n| p.groups[*n].archived && !rows[*n].is_empty()));
    open.retain(|n| p.scope.is_empty() || p.scope.contains(&p.groups[*n].key));
    open.retain(|n| !rows[*n].is_empty() || !p.scope.is_empty() || (p.groups[*n].pinned && !p.groups[*n].archived));
    open.into_iter().map(|n| Folder { group: p.groups[n].clone(), rows: std::mem::take(&mut rows[n]) }).collect()
}

/// Collapsed folders (the web's `projectExpandedById`); folders start open. Kept beside the
/// client settings so they outlive a restart.
#[derive(Default)]
pub struct State {
    collapsed: HashSet<String>,
    loaded: bool,
    pub hover: Option<String>,
    /// The folder context menu: group key and the point it opened at.
    pub menu: Option<(String, f32, f32)>,
}

const FILE: &str = "sidebar-folders.json";

impl State {
    fn path() -> Option<std::path::PathBuf> {
        (!cfg!(test)).then(|| crate::settings::client::sibling_path(FILE))
    }

    fn load(&mut self) {
        if std::mem::replace(&mut self.loaded, true) {
            return;
        }
        let saved = Self::path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        self.collapsed = saved.iter().flat_map(|v| v["collapsed"].as_array().into_iter().flatten()).filter_map(|k| k.as_str().map(str::to_string)).collect();
    }

    pub fn expanded(&self, key: &str) -> bool {
        !self.collapsed.contains(key)
    }

    pub fn toggle(&mut self, key: &str, open: bool) {
        self.load();
        if open { self.collapsed.remove(key) } else { self.collapsed.insert(key.to_string()) };
        let Some(path) = Self::path() else { return };
        let mut keys: Vec<&String> = self.collapsed.iter().collect();
        keys.sort();
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        let ok = serde_json::to_vec(&json!({"collapsed": keys})).ok().is_some_and(|bytes| {
            path.parent().is_none_or(|p| std::fs::create_dir_all(p).is_ok()) && std::fs::write(&temporary, bytes).is_ok() && std::fs::rename(&temporary, &path).is_ok()
        });
        if !ok {
            eprintln!("sidebar: saving folder state failed");
        }
    }
}

/// Reads the saved folder state once, before the first frame that shows folders.
pub fn ensure_loaded(s: &mut App) {
    s.ui.shell.sidebar.folders.load();
}

/// Rows a folder shows: all when open, else only the open thread.
pub fn shown(folder: &Folder, model: &Model, state: &State) -> Vec<usize> {
    if state.expanded(&folder.group.key) {
        return folder.rows.clone();
    }
    folder.rows.iter().copied().filter(|i| model.route.as_deref() == Some(model.threads[*i].id.as_str())).collect()
}

/// The collapsed folder's dot: needs you, failed, unread done, then working.
fn dot(t: &T, model: &Model, folder: &Folder) -> Option<rune::Color> {
    let statuses: Vec<(Status, bool)> = folder.rows.iter().map(|i| (super::logic::status(&model.threads[*i]), model.threads[*i].unread())).collect();
    if statuses.iter().any(|(s, _)| matches!(s, Status::Approval | Status::Input | Status::Request)) {
        Some(t.sb.warning_foreground)
    } else if statuses.iter().any(|(s, _)| *s == Status::Failed) {
        Some(t.sb.destructive_foreground)
    } else if statuses.iter().any(|(s, unread)| *unread && matches!(s, Status::Ready | Status::Waiting)) {
        Some(t.sb.success_foreground)
    } else if statuses.iter().any(|(s, _)| *s == Status::Working) {
        Some(t.sb.info_foreground)
    } else {
        None
    }
}

fn new_thread_in(s: &mut App, project: &str) {
    s.ui.chat.home.workspace = None;
    super::go(s, Route::Home { project_id: Some(project.to_string()) });
}

/// `SidebarProjectFolderHeader`: favicon, label and, folded, a status dot, the hidden count and
/// the chevron; hovering offers "New thread in …".
pub fn header(s: &App, t: &T, model: &Model, folder: &Folder) -> El {
    let ui = &s.ui.shell.sidebar;
    let g = &folder.group;
    let open = ui.folders.expanded(&g.key);
    let hovered = ui.folders.hover.as_deref() == Some(g.key.as_str());
    let hidden = folder.rows.len() - shown(folder, model, &ui.folders).len();
    let project = s.state().projects.iter().find(|p| field(p, "id") == g.representative);
    let favicon = match project {
        Some(p) => super::favicon::view(s, t, p, 16.0),
        None => super::favicon::mark(s, t, &g.representative, &g.label, false, 16.0),
    };
    let mut button = row()
        .key(format!("sb-folder-{}", g.key))
        .role("button")
        .label(g.label.clone())
        .expanded(open)
        .w_full()
        .h(32.0)
        .px(10.0)
        .gap(10.0)
        .center_y()
        .radius(radius::MD)
        .hover(t.sb.sidebar_row_hover)
        .cursor(Cursor::Pointer)
        .focusable(true)
        .focus_ring(2.0, -2.0, t.sb.ring)
        .child(favicon)
        .child(text(g.label.clone()).font(theme::SM_MEDIUM).color(t.sb.sidebar_foreground.alpha_mul(0.9)).truncate().grow(1.0).shrink());
    if !open {
        if let Some(color) = dot(t, model, folder) {
            button = button.child(el().size(6.0, 6.0).radius(3.0).fill(color).shrink());
        }
        if hidden > 0 {
            button = button.child(text(hidden.to_string()).font(theme::TEXT_XS.features("'tnum' 1")).color(t.sb.muted_foreground).opacity(if hovered { 0.0 } else { 1.0 }));
        }
    }
    let chevron = el().size(12.0, 12.0).center().child(icon(if open { &i::CHEVRON_DOWN } else { &i::CHEVRON_RIGHT }, 12.0).color(t.sb.muted_foreground));
    button = button.child(chevron);
    let (key, hover_key, menu_key, key_key) = (g.key.clone(), g.key.clone(), g.key.clone(), g.key.clone());
    let button = button
        .on_press(move |s: &mut App| {
            let folders = &mut s.ui.shell.sidebar.folders;
            let open = !folders.expanded(&key);
            folders.toggle(&key, open);
        })
        .on_hover_change(move |s: &mut App, on, _: &mut Tx| {
            let f = &mut s.ui.shell.sidebar.folders;
            if on {
                f.hover = Some(hover_key.clone());
            } else if f.hover.as_deref() == Some(hover_key.as_str()) {
                f.hover = None;
            }
        })
        .on_secondary_press(move |s: &mut App, at: PointerPress, _: &mut Tx| s.ui.shell.sidebar.folders.menu = Some((menu_key.clone(), at.x, at.y)))
        // `sidebarTree`: Right opens a folder, Left folds it.
        .on_key(move |s: &mut App, e: &rune::KeyEvent, _: &mut Tx| {
            let open = match e.key {
                rune::Key::Right => true,
                rune::Key::Left => false,
                _ => return false,
            };
            s.ui.shell.sidebar.folders.toggle(&key_key, open);
            true
        });
    let mut li = el().key(format!("sb-folder-li-{}", g.key)).w_full().child(button);
    if hovered {
        let project = g.representative.clone();
        let new = super::row::hover_action(t, format!("sb-folder-new-{}", g.key), &i::SQUARE_PEN, 14.0, None, 6.0)
            .role("button")
            .label(format!("New thread in {}", g.label))
            .on_press(move |s: &mut App| new_thread_in(s, &project));
        li = li.child(new.pin(None, Some(4.0), Some(24.0), None));
    }
    li
}

/// The folder's right-click menu (`SidebarProjectFolderHeader`'s context menu).
pub fn menu(s: &App, t: &T, overlays: &mut Vec<El>) {
    let Some((key, x, y)) = s.ui.shell.sidebar.folders.menu.clone() else { return };
    let (groups, _) = group_of(s);
    let Some(g) = groups.into_iter().find(|g| g.key == key) else { return };
    let close = |s: &mut App| s.ui.shell.sidebar.folders.menu = None;
    let project = g.representative.clone();
    let mut menu = kit::Menu::new("sb-folder-menu")
        .native(true)
        .width(198.0)
        .open(true)
        .on_close(close)
        .at(x, y)
        .item(MenuItem::new("New thread").on_select(move |s: &mut App| {
            close(s);
            new_thread_in(s, &project);
        }));
    let root = g.root.clone();
    menu = menu.item(MenuItem::new("Copy path").on_select_tx(move |s: &mut App, tx: &mut Tx| {
        close(s);
        tx.copy(root.clone());
        crate::shell::palette::toast(s, "Path copied", Some(&root));
    }));
    if crate::shell::palette::actions::can_organize_projects(s) {
        let (members, pinned) = (g.members.clone(), g.pinned);
        menu = menu.item(MenuItem::new(if pinned { "Unpin project" } else { "Pin project" }).on_select(move |s: &mut App| {
            close(s);
            for m in &members {
                crate::shell::palette::actions::pin_project(s, m, !pinned);
            }
        }));
    }
    let settings = g.representative.clone();
    menu = menu.item(MenuItem::new("Project settings").on_select(move |s: &mut App| {
        close(s);
        s.ui.settings.project = Some(settings.clone());
        super::go(s, Route::Settings("projects".into()));
    }));
    menu.overlay(&kit::theme(t), overlays);
}

/// The folders, stacked where the Active section would be.
pub fn view(s: &App, t: &T, model: &Model, row_of: &dyn Fn(usize) -> El) -> El {
    let mut out = col().key("sb-folders").w_full();
    for (n, folder) in model.folders.iter().enumerate() {
        let mut li = col().w_full().child(header(s, t, model, folder));
        if n > 0 {
            li = li.mt(8.0);
        }
        for i in shown(folder, model, &s.ui.shell.sidebar.folders) {
            li = li.child(row_of(i));
        }
        out = out.child(li);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grouping(mode: &str) -> Grouping {
        Grouping { mode: mode.into(), overrides: json!({}), environment: "env".into() }
    }

    fn project(id: &str, title: &str, root: &str, identity: Value) -> Value {
        json!({"id": id, "title": title, "workspaceRoot": root, "repositoryIdentity": identity, "pinnedAt": null, "archivedAt": null})
    }

    #[test]
    fn grouping_keys_follow_the_mode_and_origin() {
        let identity = json!({"canonicalKey": "github.com/fork/app", "origin": {"canonicalKey": "github.com/acme/app"}, "rootPath": "/src/app/"});
        let nested = project("p", "web", "/src/app/apps/web", identity.clone());
        let root = project("r", "app", "/src/app", identity);
        assert_eq!(grouping("repository").key(&nested), "github.com/acme/app");
        assert_eq!(grouping("repository_path").key(&nested), "github.com/acme/app::apps/web");
        assert_eq!(grouping("repository_path").key(&root), "github.com/acme/app", "the repository root keeps the plain key");
        assert_eq!(grouping("separate").key(&nested), "env:/src/app/apps/web");
        assert_eq!(grouping("repository").key(&project("x", "x", "/tmp/x/", Value::Null)), "env:/tmp/x", "no identity falls back to the path");
        let mut overridden = grouping("repository");
        overridden.overrides = json!({"env:/src/app/apps/web": "separate"});
        assert_eq!(overridden.key(&nested), "env:/src/app/apps/web");
    }

    #[test]
    fn a_clone_and_its_worktree_share_one_labelled_group() {
        let identity = |root: &str| json!({"canonicalKey": "github.com/acme/app", "name": "app", "rootPath": root});
        let mut worktree = project("w", "app-feature", "/w/app-feature", identity("/w/app-feature"));
        worktree["pinnedAt"] = json!("2026-01-01T00:00:00Z");
        let projects = vec![project("c", "app main", "/src/app", identity("/src/app")), worktree, project("o", "Other", "/other", Value::Null)];
        let built = groups(&projects, &grouping("repository"));
        assert_eq!(built.len(), 2);
        assert_eq!(built[0].members, vec!["c", "w"]);
        assert_eq!(built[0].label, "app", "differing titles fall back to the repository name");
        assert!(built[0].pinned, "any pinned member pins the group");
        assert_eq!(built[1].label, "Other");
        assert_eq!(groups(&projects, &grouping("separate")).len(), 3);
    }

    fn thread(id: &str, project: &str, created: i64, latest: Option<i64>) -> Thread {
        let mut t = Thread::from_shell(&json!({"id": id, "projectId": project, "title": id}));
        t.created_at = created;
        t.latest_user_authored_at = Some(latest);
        t.latest_user_message_at = latest;
        t
    }

    #[test]
    fn folders_order_pinned_then_recent_and_drop_empty_ones() {
        let projects = vec![
            project("a", "Alpha", "/a", Value::Null),
            project("b", "Beta", "/b", Value::Null),
            project("c", "Gamma", "/c", Value::Null),
            json!({"id": "d", "title": "Delta", "workspaceRoot": "/d", "pinnedAt": "2026-01-01T00:00:00Z", "archivedAt": null}),
            json!({"id": "e", "title": "Old", "workspaceRoot": "/e", "pinnedAt": null, "archivedAt": "2026-01-01T00:00:00Z"}),
        ];
        let groups = groups(&projects, &grouping("repository"));
        let threads = vec![thread("a1", "a", 1_000, Some(5_000)), thread("a2", "a", 4_000, None), thread("b1", "b", 9_000, None), thread("e1", "e", 2_000, None)];
        let none = HashSet::new();
        let plan_with = |scope: &HashSet<String>| {
            plan(&Plan { groups: &groups, projects: &projects, threads: &threads, active: &[0, 1, 2, 3], scope, sort: "updated_at" })
                .into_iter()
                .map(|f| (f.group.label, f.rows.iter().map(|i| threads[*i].id.clone()).collect::<Vec<_>>()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            plan_with(&none),
            vec![
                ("Delta".to_string(), vec![]),
                ("Beta".to_string(), vec!["b1".to_string()]),
                ("Alpha".to_string(), vec!["a1".to_string(), "a2".to_string()]),
                ("Old".to_string(), vec!["e1".to_string()]),
            ],
            "pinned leads even when empty, Gamma has nothing to show, archived Old keeps its live thread"
        );
        let scoped = HashSet::from(["env:/c".to_string()]);
        assert_eq!(plan_with(&scoped), vec![("Gamma".to_string(), vec![])], "a scoped folder shows even when empty");
    }
}
